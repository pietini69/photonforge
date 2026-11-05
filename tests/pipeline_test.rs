use photon_forge::core::image::{AstroImage, ImageMetadata};
use photon_forge::core::loader::ImageLoader;
use photon_forge::processing::background::{BackgroundExtractor, ExtractionMode};
use photon_forge::processing::scnr::Scnr;
use photon_forge::processing::stacking::{Stacker, StackingConfig, StackingMethod};
use photon_forge::processing::stf::StfParameters;
use std::sync::Arc;

#[test]
fn test_tiff_loading_and_processing() {
    let test_file = "/Volumes/Dataa/Ohjelmointi/VideoToFits/Jupiter-timelapse-RAW/frame_00000.tiff";
    if !std::path::Path::new(test_file).exists() {
        return;
    }

    // 1. Test loader
    let img = ImageLoader::load_file(test_file).expect("Failed to load TIFF image");
    assert!(img.width > 0 && img.height > 0);
    println!("Loaded TIFF: {}x{}, channels={}", img.width, img.height, img.channels);

    // 2. Test STF
    let stf = StfParameters::compute_auto(&img, false);
    println!("Auto STF: shadows={:?}, midtones={:?}, highlights={:?}", 
        stf.channels[0].shadows,
        stf.channels[0].midtones,
        stf.channels[0].highlights
    );

    // 3. Test SCNR
    let scnr_img = Scnr::remove_green(&img, 0.8).expect("SCNR failed");
    assert_eq!(scnr_img.data.len(), img.data.len());

    // 4. Test ABE
    let abe_img = BackgroundExtractor::extract(&img, 1, ExtractionMode::Subtraction).expect("ABE failed");
    assert_eq!(abe_img.data.len(), img.data.len());
}

#[test]
fn test_stacking_pipeline() {
    let base_dir = "/Volumes/Dataa/Ohjelmointi/VideoToFits/Jupiter-timelapse-RAW";
    if !std::path::Path::new(base_dir).exists() {
        return;
    }

    let files = vec![
        format!("{}/frame_00000.tiff", base_dir),
        format!("{}/frame_00001.tiff", base_dir),
        format!("{}/frame_00002.tiff", base_dir),
        format!("{}/frame_00003.tiff", base_dir),
    ];

    let images: Vec<Arc<photon_forge::core::image::AstroImage>> = files.iter()
        .filter_map(|f| ImageLoader::load_file(f).ok())
        .map(Arc::new)
        .collect();

    assert_eq!(images.len(), 4);

    let config = StackingConfig {
        method: StackingMethod::SigmaClipping,
        sigma_low: 2.0,
        sigma_high: 2.0,
        align_stars: false,
        auto_crop_borders: false,
        ..Default::default()
    };

    let stacked = Stacker::stack(&images, &config).expect("Stacking failed");
    assert_eq!(stacked.width, images[0].width);
    assert_eq!(stacked.height, images[0].height);
    println!("Successfully stacked 4 frames into {}x{} image", stacked.width, stacked.height);
}

#[test]
fn test_fits_write_and_read_roundtrip() {
    use photon_forge::core::fits::FitsParser;
    use photon_forge::core::image::{AstroImage, ImageMetadata};

    // Create a 16x16 3-channel test synthetic nebula/star pattern
    let width = 16;
    let height = 16;
    let mut data = vec![0.0f32; width * height * 3];
    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) * 3;
            data[idx] = (x as f32) / 16.0;         // R
            data[idx + 1] = (y as f32) / 16.0;     // G
            data[idx + 2] = ((x + y) as f32) / 32.0; // B
        }
    }

    let orig_img = AstroImage {
        width,
        height,
        channels: 3,
        data,
        metadata: ImageMetadata {
            file_name: "test_pattern.fits".to_string(),
            target: Some("M42 Orion Nebula".to_string()),
            telescope: Some("Newtonian 200/800".to_string()),
            camera: Some("ASI2600MC".to_string()),
            ..Default::default()
        },
    };

    let temp_fits = std::env::temp_dir().join("photon_forge_test.fits");
    FitsParser::write_file(&orig_img, &temp_fits).expect("Writing FITS failed");

    // Read it back
    let read_img = FitsParser::read_file(&temp_fits).expect("Reading FITS failed");
    assert_eq!(read_img.width, orig_img.width);
    assert_eq!(read_img.height, orig_img.height);
    assert_eq!(read_img.channels, orig_img.channels);

    // Verify metadata preserved
    assert_eq!(read_img.metadata.target.as_deref(), Some("M42 Orion Nebula"));
    assert_eq!(read_img.metadata.camera.as_deref(), Some("ASI2600MC"));

    // Verify float pixel values
    for (orig_val, read_val) in orig_img.data.iter().zip(read_img.data.iter()) {
        assert!((orig_val - read_val).abs() < 1e-5, "Pixel mismatch: {} vs {}", orig_val, read_val);
    }

    let _ = std::fs::remove_file(temp_fits);
    println!("FITS write and read roundtrip test passed successfully!");
}

#[test]
fn test_seestar_fits_debayer() {
    let test_file = "/Volumes/Dataa/sky/IC 63_sub/Light_IC 63_30.0s_LP_20260105-171840.fit";
    if !std::path::Path::new(test_file).exists() {
        return;
    }

    let img = ImageLoader::load_file(test_file).expect("Failed to load Seestar FITS file");
    println!("Loaded Seestar S30 frame: {}x{}, channels={}", img.width, img.height, img.channels);
    assert_eq!(img.width, 1080);
    assert_eq!(img.height, 1920);
    assert_eq!(img.channels, 3, "Seestar CFA file should be automatically debayered into 3-channel RGB");
    assert_eq!(img.metadata.bayer_pattern.as_deref(), Some("GRBG"));
    assert_eq!(img.metadata.camera.as_deref(), Some("Seestar S30"));

    // Verify STF and SCNR work seamlessly on the debayered Seestar image
    let stf = StfParameters::compute_auto(&img, false);
    assert!(stf.channels[0].midtones > 0.0);
    let scnr = Scnr::remove_green(&img, 0.7).expect("SCNR should succeed on RGB");
    assert_eq!(scnr.channels, 3);
    println!("Seestar debayer, STF, and SCNR successfully verified!");
}

#[test]
fn test_star_alignment_real_frames() {
    use photon_forge::processing::alignment::{ImageWarper, StarDetector, StarMatcher};

    let f1 = "/Volumes/Dataa/sky/IC 63_sub/Light_IC 63_30.0s_LP_20260105-171840.fit";
    let f2 = "/Volumes/Dataa/sky/IC 63_sub/Light_IC 63_30.0s_LP_20260105-172044.fit";

    if !std::path::Path::new(f1).exists() || !std::path::Path::new(f2).exists() {
        return;
    }

    let img1 = ImageLoader::load_file(f1).expect("Failed to load f1");
    let img2 = ImageLoader::load_file(f2).expect("Failed to load f2");

    let stars1 = StarDetector::detect(&img1, 50);
    let stars2 = StarDetector::detect(&img2, 50);

    println!("Detected {} stars in Frame 1, {} stars in Frame 2", stars1.len(), stars2.len());
    for (i, s) in stars1.iter().take(5).enumerate() {
        println!("  F1 Star #{}: pos=({:.2}, {:.2}), flux={:.0}", i, s.x, s.y, s.flux);
    }
    for (i, s) in stars2.iter().take(5).enumerate() {
        println!("  F2 Star #{}: pos=({:.2}, {:.2}), flux={:.0}", i, s.x, s.y, s.flux);
    }
    assert!(stars1.len() >= 5);
    assert!(stars2.len() >= 5);

    let tf = StarMatcher::match_stars(&stars1, &stars2).expect("Star matching should succeed");
    let rot_deg = tf.theta.to_degrees();
    println!("Computed Registration Transform: rot={:.3}°, dx={:.2} px, dy={:.2} px", rot_deg, tf.tx, tf.ty);

    // Verify warping
    let warped = ImageWarper::warp(&img2, &tf);
    assert_eq!(warped.width, img2.width);
    assert_eq!(warped.height, img2.height);
    assert_eq!(warped.channels, img2.channels);

    // Test stacking with alignment
    let frames = vec![Arc::new(img1), Arc::new(img2)];
    let config = StackingConfig {
        method: StackingMethod::SigmaClipping,
        sigma_low: 2.0,
        sigma_high: 2.0,
        align_stars: true,
        ..Default::default()
    };

    let stacked = Stacker::stack(&frames, &config).expect("Aligned stacking should succeed");
    assert!(stacked.width <= 1080 && stacked.width >= 1000);
    assert!(stacked.height <= 1920 && stacked.height >= 1800);
    println!("Output dimensions with overlap auto-crop: {}x{}", stacked.width, stacked.height);
    println!("Star alignment & aligned stacking successfully verified on real Seestar frames!");
}

#[test]
fn test_auto_crop_with_synthetic_border() {
    use photon_forge::core::image::{AstroImage, ImageMetadata};

    let w = 100;
    let h = 100;
    let channels = 3;
    let mut data = vec![0.05f32; w * h * channels];

    // Set a 5-pixel border to 0.0
    for y in 0..h {
        for x in 0..w {
            if x < 5 || x >= 95 || y < 4 || y >= 96 {
                for c in 0..channels {
                    data[(y * w + x) * channels + c] = 0.0;
                }
            }
        }
    }

    let img = AstroImage {
        width: w,
        height: h,
        channels,
        data,
        metadata: ImageMetadata::default(),
    };

    let cropped = img.auto_crop().expect("Auto-crop should crop synthetic 0.0 borders");
    assert_eq!(cropped.width, 90);
    assert_eq!(cropped.height, 92);
    // Ensure cropped image has no 0.0
    for v in &cropped.data {
        assert!(*v > 0.0);
    }
    println!("Auto-crop synthetic border test passed successfully: {}x{} -> {}x{}", w, h, cropped.width, cropped.height);
}

#[test]
fn test_m42_multi_night_alignment_and_normalization() {
    let f1 = "/Volumes/Sky/Kuvat/data/M42/12.2.2026/lights/Light_M 42_60.0s_LP_20260212-232325.fit";
    let f2 = "/Volumes/Sky/Kuvat/data/M42/17.2.2026/M 42_sub/Light_M 42_60.0s_LP_20260217-230438.fit";

    if !std::path::Path::new(f1).exists() || !std::path::Path::new(f2).exists() {
        println!("M42 files not found on volume, skipping");
        return;
    }

    let files = vec![
        "/Volumes/Sky/Kuvat/data/M42/12.2.2026/lights/Light_M 42_60.0s_LP_20260212-232325.fit",
        "/Volumes/Sky/Kuvat/data/M42/12.2.2026/lights/Light_M 42_60.0s_LP_20260212-232425.fit",
        "/Volumes/Sky/Kuvat/data/M42/12.2.2026/lights/Light_M 42_60.0s_LP_20260212-232525.fit",
        "/Volumes/Sky/Kuvat/data/M42/17.2.2026/M 42_sub/Light_M 42_60.0s_LP_20260217-230438.fit",
        "/Volumes/Sky/Kuvat/data/M42/17.2.2026/M 42_sub/Light_M 42_60.0s_LP_20260217-230538.fit",
        "/Volumes/Sky/Kuvat/data/M42/17.2.2026/M 42_sub/Light_M 42_60.0s_LP_20260217-230639.fit",
    ];

    let frames: Vec<Arc<photon_forge::core::image::AstroImage>> = files
        .iter()
        .map(|p| Arc::new(ImageLoader::load_file(p).expect("Failed to load frame")))
        .collect();

    let (m1_r, _) = frames[0].channel_median_and_mad(0);
    let (m2_r, _) = frames[3].channel_median_and_mad(0);
    println!("Raw medians Red: Night 1 = {:.5}, Night 2 = {:.5} (Diff = {:.5})", m1_r, m2_r, (m1_r - m2_r).abs());

    let config = StackingConfig {
        method: StackingMethod::SigmaClipping,
        sigma_low: 2.5,
        sigma_high: 3.0,
        align_stars: true,
        normalize_background: true,
        ..Default::default()
    };

    let start = std::time::Instant::now();
    let stacked = Stacker::stack(&frames, &config).expect("Stacking multi-night M42 failed");
    let dur = start.elapsed();
    println!("Stacked {} frames in {:.2}s", frames.len(), dur.as_secs_f32());
    assert!(stacked.width <= 1080);
    assert!(stacked.height <= 1920);
    println!("Output dimensions after overlap crop: {}x{}", stacked.width, stacked.height);

    // Test Auto-Crop on stacked image
    if let Ok(cropped) = stacked.auto_crop() {
        println!("Auto-cropped from {}x{} to {}x{}", stacked.width, stacked.height, cropped.width, cropped.height);
        assert!(cropped.width <= stacked.width);
        assert!(cropped.height <= stacked.height);
    } else {
        println!("No zero-padded borders on reference canvas (all edge pixels covered by reference frame)");
    }
    println!("Successfully stacked multi-night M42 frames with star alignment and background normalization!");

    let stf_unlinked = StfParameters::compute_auto(&stacked, false);
    println!("Stacked STF unlinked: R=({:.4},{:.4}), G=({:.4},{:.4}), B=({:.4},{:.4})",
        stf_unlinked.channels[0].shadows, stf_unlinked.channels[0].midtones,
        stf_unlinked.channels[1].shadows, stf_unlinked.channels[1].midtones,
        stf_unlinked.channels[2].shadows, stf_unlinked.channels[2].midtones,
    );
}

#[test]
fn test_all_55_m42_frames_matching() {
    let mut files = Vec::new();
    let dir1 = std::path::Path::new("/Volumes/Sky/Kuvat/data/M42/12.2.2026/lights");
    let dir2 = std::path::Path::new("/Volumes/Sky/Kuvat/data/M42/17.2.2026/M 42_sub");

    if !dir1.exists() || !dir2.exists() {
        return;
    }

    if let Ok(entries) = std::fs::read_dir(dir1) {
        for e in entries.flatten() {
            if e.path().extension().and_then(|s| s.to_str()) == Some("fit") {
                files.push(e.path());
            }
        }
    }
    if let Ok(entries) = std::fs::read_dir(dir2) {
        for e in entries.flatten() {
            if e.path().extension().and_then(|s| s.to_str()) == Some("fit") {
                files.push(e.path());
            }
        }
    }

    files.sort();
    println!("Total M42 files found: {}", files.len());
    if files.is_empty() {
        return;
    }

    use photon_forge::processing::alignment::{StarDetector, StarMatcher};

    let ref_img = ImageLoader::load_file(&files[0]).expect("Failed to load ref");
    let ref_stars = StarDetector::detect(&ref_img, 60);
    println!("Ref frame: {}, detected {} stars", files[0].display(), ref_stars.len());

    let mut matched_count = 0;
    let mut failed_files = Vec::new();

    for (i, file) in files.iter().enumerate().skip(1) {
        let img = ImageLoader::load_file(file).expect("Failed to load frame");
        let stars = StarDetector::detect(&img, 60);
        if let Some(tf) = StarMatcher::match_stars(&ref_stars, &stars) {
            matched_count += 1;
            println!("  [OK] #{:02} ({} stars): rot={:+.2}°, dx={:+.1}, dy={:+.1} - {}",
                i + 1, stars.len(), tf.theta.to_degrees(), tf.tx, tf.ty, file.file_name().unwrap().to_str().unwrap());
        } else {
            failed_files.push((i + 1, file.file_name().unwrap().to_str().unwrap().to_string(), stars.len()));
            println!("  [FAIL] #{:02} ({} stars): {}",
                i + 1, stars.len(), file.file_name().unwrap().to_str().unwrap());
        }
    }

    println!("\n=== Star Alignment Summary ===");
    println!("Total: {}, Matched: {}, Failed: {}", files.len(), matched_count + 1, failed_files.len());
    assert_eq!(matched_count + 1, files.len(), "All 55 frames should match successfully without false rotations");
}

#[test]
fn test_stack_and_render_all_55_m42() {
    use photon_forge::processing::histogram::HistogramProcessor;

    let mut files = Vec::new();
    let dir1 = std::path::Path::new("/Volumes/Sky/Kuvat/data/M42/12.2.2026/lights");
    let dir2 = std::path::Path::new("/Volumes/Sky/Kuvat/data/M42/17.2.2026/M 42_sub");

    if !dir1.exists() || !dir2.exists() {
        return;
    }

    if let Ok(entries) = std::fs::read_dir(dir1) {
        for e in entries.flatten() {
            if e.path().extension().and_then(|s| s.to_str()) == Some("fit") {
                files.push(e.path());
            }
        }
    }
    if let Ok(entries) = std::fs::read_dir(dir2) {
        for e in entries.flatten() {
            if e.path().extension().and_then(|s| s.to_str()) == Some("fit") {
                files.push(e.path());
            }
        }
    }

    files.sort();
    if files.is_empty() {
        return;
    }

    println!("Loading {} frames for full stack test...", files.len());
    let images: Vec<Arc<photon_forge::core::image::AstroImage>> = files
        .iter()
        .map(|f| Arc::new(ImageLoader::load_file(f).expect("load failed")))
        .collect();

    let config = StackingConfig {
        method: StackingMethod::SigmaClipping,
        sigma_low: 2.5,
        sigma_high: 3.0,
        align_stars: true,
        normalize_background: true,
        ..Default::default()
    };

    let start = std::time::Instant::now();
    let stacked = Stacker::stack(&images, &config).expect("Stacking 55 frames failed");
    println!("Stacked {} frames in {:.2}s!", images.len(), start.elapsed().as_secs_f32());

    // Auto-crop
    let cropped = stacked.auto_crop().unwrap_or(stacked.clone());
    println!("Final image: {}x{} px", cropped.width, cropped.height);

    // Compute unlinked STF
    let stf = StfParameters::compute_auto(&cropped, false);
    println!("Unlinked STF details:");
    for c in 0..3 {
        println!("  Ch {}: shadows={:.6}, midtones={:.6}, highlights={:.6}",
            c, stf.channels[c].shadows, stf.channels[c].midtones, stf.channels[c].highlights);
    }

    let stf_linked = StfParameters::compute_auto(&cropped, true);
    println!("Linked STF details:");
    for c in 0..3 {
        println!("  Ch {}: shadows={:.6}, midtones={:.6}, highlights={:.6}",
            c, stf_linked.channels[c].shadows, stf_linked.channels[c].midtones, stf_linked.channels[c].highlights);
    }

    // Test ABE (Automatic Background Extraction with degree 2 and channel neutralization)
    use photon_forge::processing::background::{BackgroundExtractor, ExtractionMode};
    use photon_forge::processing::scnr::Scnr;

    let abe = BackgroundExtractor::extract(&cropped, 2, ExtractionMode::Subtraction).expect("ABE failed");
    println!("ABE degree 2 background extraction completed!");

    let scnr = Scnr::remove_green(&abe, 0.8).expect("SCNR failed");
    println!("SCNR green noise reduction completed!");

    // Dual-band boost (Ha + OIII)
    let dual = HistogramProcessor::dual_band_boost(&scnr, 1.25, 1.30).unwrap_or(scnr);
    let sat = HistogramProcessor::adjust_saturation(&dual, 1.4).unwrap_or(dual);

    let stf_post = StfParameters::compute_auto_with_target(&sat, false, 0.09);
    let params_post = [
        (stf_post.channels[0].shadows, stf_post.channels[0].midtones, stf_post.channels[0].highlights),
        (stf_post.channels[1].shadows, stf_post.channels[1].midtones, stf_post.channels[1].highlights),
        (stf_post.channels[2].shadows, stf_post.channels[2].midtones, stf_post.channels[2].highlights),
    ];
    let stretched_post = HistogramProcessor::apply_permanent_stretch_channels(&sat, &params_post);
    let out_post = "/tmp/m42_full_processed.png";
    let _ = ImageLoader::save_image(&stretched_post, out_post);
    println!("Saved fully processed image to {}", out_post);

    // Render stretched preview
    let params = [
        (stf.channels[0].shadows, stf.channels[0].midtones, stf.channels[0].highlights),
        (stf.channels[1].shadows, stf.channels[1].midtones, stf.channels[1].highlights),
        (stf.channels[2].shadows, stf.channels[2].midtones, stf.channels[2].highlights),
    ];
    let stretched = HistogramProcessor::apply_permanent_stretch_channels(&cropped, &params);

    let out_path = "/tmp/m42_full_stack_stretched.png";
    let _ = ImageLoader::save_image(&stretched, out_path);
    println!("Saved preview to {}", out_path);
}

#[test]
fn test_drizzle_2x_stacking() {
    let f1 = "/Volumes/Sky/Kuvat/data/M42/12.2.2026/lights/Light_M 42_60.0s_LP_20260212-232325.fit";
    let f2 = "/Volumes/Sky/Kuvat/data/M42/12.2.2026/lights/Light_M 42_60.0s_LP_20260212-232425.fit";

    if !std::path::Path::new(f1).exists() || !std::path::Path::new(f2).exists() {
        return;
    }

    let img1 = ImageLoader::load_file(f1).expect("Failed to load f1");
    let img2 = ImageLoader::load_file(f2).expect("Failed to load f2");

    let config = StackingConfig {
        method: StackingMethod::SigmaClipping,
        sigma_low: 2.5,
        sigma_high: 3.0,
        align_stars: true,
        normalize_background: true,
        drizzle_scale: 2, // 2x Drizzle Super-Resolution
        auto_crop_borders: false,
        ..Default::default()
    };

    let frames = vec![Arc::new(img1), Arc::new(img2)];
    let stacked_2x = Stacker::stack(&frames, &config).expect("2x Drizzle stacking failed");

    // Dimensions must be 2x the original: 1080x2 = 2160, 1920x2 = 3840
    assert_eq!(stacked_2x.width, 2160);
    assert_eq!(stacked_2x.height, 3840);
    assert_eq!(stacked_2x.channels, 3);
    println!("2x Drizzle Super-Resolution successfully produced {}x{} master stack!", stacked_2x.width, stacked_2x.height);
}

#[test]
fn test_star_reduction() {
    use photon_forge::processing::star_reduction::StarReduction;

    let f = "/Volumes/Sky/Kuvat/data/M42/12.2.2026/lights/Light_M 42_60.0s_LP_20260212-232325.fit";
    if !std::path::Path::new(f).exists() {
        return;
    }

    let img = ImageLoader::load_file(f).expect("Failed to load frame");
    let reduced = StarReduction::reduce_stars(&img, 0.6, 1, 1).expect("Star reduction failed");

    assert_eq!(reduced.width, img.width);
    assert_eq!(reduced.height, img.height);
    assert_eq!(reduced.channels, img.channels);

    // Sum of intensities on bright stars should decrease
    let mut orig_sum = 0.0f32;
    let mut red_sum = 0.0f32;
    for (a, b) in img.data.iter().zip(reduced.data.iter()) {
        if *a > 0.3 {
            orig_sum += *a;
            red_sum += *b;
        }
    }
    assert!(red_sum < orig_sum, "Star reduction should reduce bright star flux");
    println!("Star reduction test successfully verified: star intensity reduced from {:.1} to {:.1} (-{:.1}%)",
        orig_sum, red_sum, (1.0 - red_sum / orig_sum) * 100.0);
}

#[test]
fn test_stack_with_progress_and_cancel() {
    use photon_forge::processing::stacking::StackingProgress;
    use std::sync::atomic::AtomicBool;
    use std::sync::Mutex;

    let f1 = "/Volumes/Sky/Kuvat/data/M42/12.2.2026/lights/Light_M 42_60.0s_LP_20260212-232325.fit";
    let f2 = "/Volumes/Sky/Kuvat/data/M42/12.2.2026/lights/Light_M 42_60.0s_LP_20260212-232425.fit";
    if !std::path::Path::new(f1).exists() || !std::path::Path::new(f2).exists() {
        return;
    }

    let img1 = Arc::new(ImageLoader::load_file(f1).unwrap());
    let img2 = Arc::new(ImageLoader::load_file(f2).unwrap());
    let images = vec![img1, img2];

    let progress_history = Arc::new(Mutex::new(Vec::<StackingProgress>::new()));
    let ph_clone = Arc::clone(&progress_history);
    let cancel = Arc::new(AtomicBool::new(false));

    let config = StackingConfig {
        align_stars: true,
        normalize_background: true,
        auto_crop_borders: false,
        drizzle_scale: 1,
        ..Default::default()
    };

    let result = Stacker::stack_with_progress(
        &images,
        &config,
        Some(Box::new(move |p| {
            ph_clone.lock().unwrap().push(p);
        })),
        cancel,
    );

    assert!(result.is_ok());
    let hist = progress_history.lock().unwrap();
    assert!(!hist.is_empty(), "Progress updates should have been received");
    println!("Received {} progress updates during stacking! Final stage: {} ({:.0}%)",
        hist.len(),
        hist.last().unwrap().stage,
        hist.last().unwrap().fraction * 100.0
    );

    // Test cancellation
    let cancel2 = Arc::new(AtomicBool::new(true)); // Pre-cancelled
    let res_cancel = Stacker::stack_with_progress(
        &images,
        &config,
        None::<Box<dyn Fn(StackingProgress) + Send + Sync>>,
        cancel2,
    );
    assert!(res_cancel.is_err(), "Stacking should abort immediately when cancelled");
    println!("Cancellation test passed: returned {:?}", res_cancel.err().unwrap());
}

#[test]
fn test_m45_investigation() {
    use photon_forge::core::image::AstroImage;
    use photon_forge::processing::alignment::{StarDetector, StarMatcher};

    let dir = "/Volumes/Sky/Kuvat/data/M45_1+2/20.12.2025/M 45_sub/lights";
    if !std::path::Path::new(dir).exists() {
        return;
    }
    let entries = std::fs::read_dir(dir).unwrap();
    let mut files: Vec<String> = entries
        .filter_map(|e| e.ok().map(|e| e.path().to_string_lossy().to_string()))
        .filter(|p| p.ends_with(".fit"))
        .collect();
    files.sort();

    println!("Total M45 files found: {}", files.len());
    let ref_img = ImageLoader::load_file(&files[0]).unwrap();
    println!("Ref img size: {}x{}, channels: {}", ref_img.width, ref_img.height, ref_img.channels);

    let ref_stars = StarDetector::detect(&ref_img, 60);
    println!("Ref stars detected: {}", ref_stars.len());

    let mut matched_count = 0;
    let mut failed_indices = Vec::new();

    // Check a sample of 20 images across the session
    let step = files.len() / 20;
    for i in (0..files.len()).step_by(step) {
        let tgt = ImageLoader::load_file(&files[i]).unwrap();
        let tgt_stars = StarDetector::detect(&tgt, 60);
        let tf = StarMatcher::match_stars(&ref_stars, &tgt_stars);
        if let Some(t) = tf {
            matched_count += 1;
            println!("Img #{:3} matched! theta={:.3} rad ({:.1} deg), tx={:.1}, ty={:.1}",
                i, t.theta, t.theta.to_degrees(), t.tx, t.ty);
        } else {
            failed_indices.push(i);
            println!("Img #{:3} FAILED to match! (tgt stars={})", i, tgt_stars.len());
        }
    }

    println!("Sample matched: {} / 20", matched_count);

    // Now stack a subset of 10 frames
    let sample_files: Vec<String> = (0..files.len()).step_by(files.len() / 10).map(|i| files[i].clone()).collect();
    let loaded: Vec<Arc<AstroImage>> = sample_files.iter().map(|f| Arc::new(ImageLoader::load_file(f).unwrap())).collect();

    // Test with auto_crop = true
    let config = StackingConfig {
        align_stars: true,
        normalize_background: true,
        auto_crop_borders: true,
        min_coverage_ratio: 0.10,
        drizzle_scale: 2,
        ..Default::default()
    };

    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stacked = Stacker::stack_with_progress(&loaded, &config, None::<Box<dyn Fn(photon_forge::processing::stacking::StackingProgress) + Send + Sync>>, cancel).unwrap();
    println!("Stacked (safe crop) size: {}x{}", stacked.width, stacked.height);
    assert!(stacked.width > 1800, "Width should be preserved (> 1800 px), got {}", stacked.width);
    assert!(stacked.height > 3000, "Height should be preserved (> 3000 px), got {}", stacked.height);

    // Test reflection nebula boost
    use photon_forge::processing::histogram::HistogramProcessor;
    let boosted = HistogramProcessor::reflection_nebula_boost(&stacked, 2.0, 1.3, 1.5).unwrap();
    assert_eq!(boosted.width, stacked.width);
    println!("Reflection boost successfully applied to M45!");

    // Test arcsinh stretch
    let stretched = HistogramProcessor::arcsinh_stretch(&boosted, 25.0, 0.05).unwrap();
    assert_eq!(stretched.width, stacked.width);
    println!("Arcsinh stretch successfully applied to M45!");
}

#[test]
fn test_undo_redo_functionality() {
    use photon_forge::core::image::AstroImage;
    use photon_forge::gui::PhotonForgeApp;

    let mut app = PhotonForgeApp::default();
    assert_eq!(app.undo_stack_len(), 0);
    assert_eq!(app.redo_stack_len(), 0);
    assert_eq!(app.image_count(), 0);

    // Initial state: push 1 image
    let img1 = AstroImage::new_rgb(100, 100, 0.1);
    app.push_image(img1);
    assert_eq!(app.image_count(), 1);

    // Mutate state with undo snapshot
    app.push_undo("Lisää kuva 2");
    let img2 = AstroImage::new_rgb(100, 100, 0.5);
    app.push_image(img2);
    assert_eq!(app.image_count(), 2);
    assert_eq!(app.undo_stack_len(), 1);
    assert_eq!(app.redo_stack_len(), 0);

    // Another mutation
    app.push_undo("Lisää kuva 3");
    let img3 = AstroImage::new_rgb(100, 100, 0.9);
    app.push_image(img3);
    assert_eq!(app.image_count(), 3);
    assert_eq!(app.undo_stack_len(), 2);

    // Undo once
    app.undo();
    assert_eq!(app.image_count(), 2);
    assert_eq!(app.undo_stack_len(), 1);
    assert_eq!(app.redo_stack_len(), 1);

    // Undo again
    app.undo();
    assert_eq!(app.image_count(), 1);
    assert_eq!(app.undo_stack_len(), 0);
    assert_eq!(app.redo_stack_len(), 2);

    // Redo once
    app.redo();
    assert_eq!(app.image_count(), 2);
    assert_eq!(app.undo_stack_len(), 1);
    assert_eq!(app.redo_stack_len(), 1);

    // Redo to final
    app.redo();
    assert_eq!(app.image_count(), 3);
    assert_eq!(app.undo_stack_len(), 2);
    assert_eq!(app.redo_stack_len(), 0);

    // Test bounded stack at 30
    for i in 4..=40 {
        app.push_undo(&format!("Operaatio {}", i));
        let img = AstroImage::new_rgb(10, 10, 0.0);
        app.push_image(img);
    }
    assert_eq!(app.undo_stack_len(), 30);
}

#[test]
fn test_filter_detection() {
    use photon_forge::processing::channel_combination::AstroFilter;

    // From header keywords
    assert_eq!(AstroFilter::detect(Some("Ha"), "light_001.fits"), Some(AstroFilter::HAlpha));
    assert_eq!(AstroFilter::detect(Some("H-alpha"), "light_001.fits"), Some(AstroFilter::HAlpha));
    assert_eq!(AstroFilter::detect(Some("OIII"), "light_001.fits"), Some(AstroFilter::OIII));
    assert_eq!(AstroFilter::detect(Some("[OIII]"), "light_001.fits"), Some(AstroFilter::OIII));
    assert_eq!(AstroFilter::detect(Some("SII"), "light_001.fits"), Some(AstroFilter::SII));
    assert_eq!(AstroFilter::detect(Some("Luminance"), "light_001.fits"), Some(AstroFilter::Luminance));
    assert_eq!(AstroFilter::detect(Some("Red"), "light_001.fits"), Some(AstroFilter::Red));
    assert_eq!(AstroFilter::detect(Some("Green"), "light_001.fits"), Some(AstroFilter::Green));
    assert_eq!(AstroFilter::detect(Some("Blue"), "light_001.fits"), Some(AstroFilter::Blue));

    // From file names
    assert_eq!(AstroFilter::detect(None, "M42_300s_Ha_001.fits"), Some(AstroFilter::HAlpha));
    assert_eq!(AstroFilter::detect(None, "Rosette_OIII_bin1.fits"), Some(AstroFilter::OIII));
    assert_eq!(AstroFilter::detect(None, "Pillars_SII_600s.fits"), Some(AstroFilter::SII));
    assert_eq!(AstroFilter::detect(None, "Andromeda_Lum_120s.fits"), Some(AstroFilter::Luminance));
    assert_eq!(AstroFilter::detect(None, "M31_Red_001.fits"), Some(AstroFilter::Red));
    assert_eq!(AstroFilter::detect(None, "M31_Green_001.fits"), Some(AstroFilter::Green));
    assert_eq!(AstroFilter::detect(None, "M31_Blue_001.fits"), Some(AstroFilter::Blue));
}

#[test]
fn test_channel_combination_rgb_and_lrgb() {
    use photon_forge::core::image::AstroImage;
    use photon_forge::processing::channel_combination::{ChannelCombiner, ChannelCombinerConfig};

    let w = 100;
    let h = 100;

    let r = AstroImage::new_mono(w, h, 0.4);
    let g = AstroImage::new_mono(w, h, 0.3);
    let b = AstroImage::new_mono(w, h, 0.2);
    let l = AstroImage::new_mono(w, h, 0.8);

    let config = ChannelCombinerConfig {
        align_stars: false,
        linear_fit: false,
        ..Default::default()
    };

    // Test RGB combination
    let rgb = ChannelCombiner::combine(&r, &g, &b, None, &config).expect("RGB combination failed");
    assert_eq!(rgb.channels, 3);
    assert_eq!(rgb.width, w);
    assert_eq!(rgb.height, h);
    assert_eq!(rgb.data.len(), w * h * 3);
    assert!((rgb.data[0] - 0.4).abs() < 1e-4);
    assert!((rgb.data[1] - 0.3).abs() < 1e-4);
    assert!((rgb.data[2] - 0.2).abs() < 1e-4);

    // Test LRGB combination
    let lrgb = ChannelCombiner::combine(&r, &g, &b, Some(&l), &config).expect("LRGB combination failed");
    assert_eq!(lrgb.channels, 3);
    assert_eq!(lrgb.width, w);
    assert_eq!(lrgb.height, h);
    assert_eq!(lrgb.data.len(), w * h * 3);
    assert!(lrgb.data[0] > 0.0);
}

#[test]
fn test_curves_transformation_and_splines() {
    use photon_forge::core::image::AstroImage;
    use photon_forge::processing::curves::{CurvesSet, SplineCurve};

    // 1. Linear identity
    let linear = SplineCurve::new_linear();
    assert!(linear.is_identity());
    assert!((linear.evaluate_pchip(0.0) - 0.0).abs() < 1e-4);
    assert!((linear.evaluate_pchip(0.5) - 0.5).abs() < 1e-4);
    assert!((linear.evaluate_pchip(1.0) - 1.0).abs() < 1e-4);

    // 2. S-Curve monotonicity & shape
    let s_curve = SplineCurve::preset_s_curve();
    assert!(!s_curve.is_identity());
    let mut prev = -1.0f32;
    for i in 0..=50 {
        let x = i as f32 / 50.0;
        let y = s_curve.evaluate_pchip(x);
        assert!(y >= 0.0 && y <= 1.0, "PCHIP overshoot at x={}: y={}", x, y);
        assert!(y >= prev - 1e-4, "Monotonicity broken at x={}: prev={}, cur={}", x, prev, y);
        prev = y;
    }
    // S-curve darkens shadows and lifts highlights
    assert!(s_curve.evaluate_pchip(0.25) < 0.25);
    assert!(s_curve.evaluate_pchip(0.75) > 0.75);

    // 3. CurvesSet application to 32-bit float RGB image
    let mut img = AstroImage::new_rgb(20, 20, 0.5);
    img.data[0] = 0.2;
    img.data[1] = 0.5;
    img.data[2] = 0.8;
    let mut curves = CurvesSet::default();
    curves.rgb = SplineCurve::preset_lift_midtones();
    assert!(!curves.is_identity());

    let modified = curves.apply_to_image(&img);
    assert_eq!(modified.width, 20);
    assert_eq!(modified.height, 20);
    assert_eq!(modified.channels, 3);
    // Midtones lifted
    assert!(modified.data[0] > 0.2); // R
    assert!(modified.data[1] > 0.5); // G

    // 4. Fast 8-bit viewport LUT application
    let mut rgb_bytes = vec![50u8, 128u8, 200u8];
    curves.apply_to_rgb8(&mut rgb_bytes);
    assert!(rgb_bytes[1] > 128); // lifted midtone in viewport preview

    // 5. Saturation boost curve
    let mut sat_curves = CurvesSet::default();
    sat_curves.saturation = SplineCurve::preset_saturation_boost();
    let sat_img = sat_curves.apply_to_image(&img);
    assert_eq!(sat_img.channels, 3);
}

#[test]
fn test_telescope_and_star_chart() {
    use chrono::Utc;
    use photon_forge::telescope::catalog::{load_sky_catalog, DsoType};
    use photon_forge::telescope::coordinates::{
        alt_az_to_canvas_normalized, format_dec, format_ra, night_altitude_profile, ra_dec_to_alt_az,
        DEFAULT_LATITUDE_DEG, DEFAULT_LONGITUDE_DEG,
    };
    use photon_forge::telescope::seestar::{SeestarClient, SeestarState};
    use photon_forge::telescope::sequencer::NightSequencer;
    use std::sync::Arc;

    // 1. Test Sky Catalog
    let catalog = load_sky_catalog();
    assert!(catalog.len() >= 50);

    let m45 = catalog.iter().find(|o| o.id == "M45").expect("M45 should be in catalog");
    assert_eq!(m45.obj_type, DsoType::ReflectionNebula);
    assert!((m45.ra_hours - 3.79).abs() < 0.1);
    assert!((m45.dec_deg - 24.11).abs() < 0.1);

    let m42 = catalog.iter().find(|o| o.id == "M42").expect("M42 should be in catalog");
    assert_eq!(m42.obj_type, DsoType::EmissionNebula);

    let polaris = catalog.iter().find(|o| o.id == "Polaris").expect("Polaris in catalog");
    assert_eq!(polaris.obj_type, DsoType::Star);

    // 2. Coordinate conversions
    let now = Utc::now();
    let (alt, az) = ra_dec_to_alt_az(polaris.ra_hours, polaris.dec_deg, DEFAULT_LATITUDE_DEG, DEFAULT_LONGITUDE_DEG, &now);
    // In Southern Finland (lat ~60.17°), Polaris altitude should be very close to the latitude!
    assert!((alt - DEFAULT_LATITUDE_DEG).abs() < 2.0, "Polaris altitude should match latitude ~60°: got {}", alt);
    assert!(az >= 0.0 && az <= 360.0);

    // Formatter tests
    assert_eq!(format_ra(3.79), "03h 47m 24s");
    assert_eq!(format_dec(24.11), "+24° 06' 36\"");

    // 3. Canvas projection
    let norm = alt_az_to_canvas_normalized(alt, az).expect("Polaris is above horizon");
    assert!(norm.0 >= -1.0 && norm.0 <= 1.0);
    assert!(norm.1 >= -1.0 && norm.1 <= 1.0);

    // 4. Night altitude profile
    let profile = night_altitude_profile(m45.ra_hours, m45.dec_deg, DEFAULT_LATITUDE_DEG, DEFAULT_LONGITUDE_DEG, &now);
    assert_eq!(profile.len(), 25);
    for (time_str, alt_val) in profile {
        assert!(!time_str.is_empty());
        assert!(alt_val >= -90.0 && alt_val <= 90.0);
    }

    // 5. Seestar S-30 Client Simulation
    let client = Arc::new(SeestarClient::new());
    client.set_simulation(true);
    std::thread::sleep(std::time::Duration::from_millis(50));
    let telem = client.get_telemetry();
    assert!(telem.is_simulation);
    assert_eq!(telem.state, SeestarState::Idle);

    // Test GoTo command in simulation
    client.goto_target("M45", 3.79, 24.11);
    std::thread::sleep(std::time::Duration::from_millis(150));
    let slewing_telem = client.get_telemetry();
    assert_eq!(slewing_telem.state, SeestarState::Slewing);

    // 6. Night Sequencer
    let sequencer = NightSequencer::new();
    sequencer.add_target(m45, 60, false);
    sequencer.add_target(m42, 90, true);

    let state = sequencer.state.lock().unwrap();
    assert_eq!(state.targets.len(), 4); // 2 default + 2 added
    assert!(!state.is_running);

    // 7. Verify Smart Telescope Models & FOV specifications
    use photon_forge::telescope::mosaic::TelescopeModel;
    assert_eq!(TelescopeModel::all().len(), 11);
    for m in TelescopeModel::all() {
        let (w, h) = m.fov_deg();
        assert!(w > 0.5 && w < 10.0, "Model {:?} FOV W out of range: {}", m, w);
        assert!(h > 0.5 && h < 10.0, "Model {:?} FOV H out of range: {}", m, h);
        assert!(m.focal_length_mm() > 20.0);
        assert!(m.aperture_mm() > 10.0);
    }

    // Verify S30 Pro and S50 Pro have larger FOVs than base models due to 4K IMX585 sensor
    assert!(TelescopeModel::SeestarS30Pro.fov_deg().0 > TelescopeModel::SeestarS30.fov_deg().0);
    assert!(TelescopeModel::SeestarS50Pro.fov_deg().0 > TelescopeModel::SeestarS50.fov_deg().0);
    assert_eq!(TelescopeModel::SeestarS50Pro.brand(), "ZWO Seestar");
    assert_eq!(TelescopeModel::Dwarf3.brand(), "DwarfLab");
    assert_eq!(TelescopeModel::DwarfMini.brand(), "DwarfLab");
    assert_eq!(TelescopeModel::DwarfDraco.brand(), "DwarfLab");
    assert_eq!(TelescopeModel::CelestronOrigin.brand(), "Celestron");
    assert_eq!(TelescopeModel::AscomAlpaca.brand(), "ASCOM Alpaca");
    assert_eq!(TelescopeModel::Custom.brand(), "Custom");
}

#[test]
fn test_live_ingest_downloader() {
    use photon_forge::telescope::downloader::LiveIngestManager;
    use std::fs;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    let unique_id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_root = std::env::temp_dir().join(format!("seestar_ingest_test_{}", unique_id));
    let mock_source = temp_root.join("EMMC_Images").join("MyWorks").join("M45_sub");
    let mock_dest = temp_root.join("PhotonForge_Local");

    fs::create_dir_all(&mock_source).expect("Failed to create mock source");
    fs::create_dir_all(&mock_dest).expect("Failed to create mock dest");

    // Initialize manager
    let ingest_mgr = LiveIngestManager::new();
    {
        let mut cfg = ingest_mgr.config.lock().unwrap();
        cfg.source_path = mock_source.clone();
        cfg.destination_path = mock_dest.clone();
        cfg.enabled = true;
    }

    // Write a mock .fits subframe into mock Seestar storage
    let test_frame = mock_source.join("Light_M45_10s_0001.fits");
    fs::write(&test_frame, b"SIMPLE  =                    T / Mock FITS header").unwrap();

    // Give background worker time to poll and copy (runs every 2s)
    std::thread::sleep(Duration::from_millis(2500));

    // Verify copy succeeded
    let copied_frame = mock_dest.join("Light_M45_10s_0001.fits");
    assert!(copied_frame.exists(), "Downloaded frame should exist in destination directory");

    let status = ingest_mgr.status.lock().unwrap();
    assert!(status.downloaded_count >= 1);
    assert_eq!(status.last_file_name, "Light_M45_10s_0001.fits");
    assert!(status.source_found);

    // Clean up
    let _ = fs::remove_dir_all(&temp_root);
}

#[test]
fn test_live_stack_engine() {
    use photon_forge::core::image::AstroImage;
    use photon_forge::processing::live_stack::LiveStackEngine;

    let engine = LiveStackEngine::new();

    // Helper to generate a test image with synthetic stars
    let create_test_frame = |shift_x: usize, shift_y: usize| {
        let width = 200;
        let height = 200;
        let mut img = AstroImage::new_rgb(width, height, 0.05);

        // Add 8 synthetic bright stars
        let star_coords = [
            (40 + shift_x, 40 + shift_y),
            (140 + shift_x, 50 + shift_y),
            (50 + shift_x, 150 + shift_y),
            (150 + shift_x, 140 + shift_y),
            (90 + shift_x, 90 + shift_y),
            (120 + shift_x, 80 + shift_y),
            (80 + shift_x, 130 + shift_y),
            (110 + shift_x, 160 + shift_y),
        ];

        for (sx, sy) in star_coords {
            for dy in -2..=2 {
                for dx in -2..=2 {
                    let px = (sx as isize + dx) as usize;
                    let py = (sy as isize + dy) as usize;
                    if px < width && py < height {
                        let dist = ((dx * dx + dy * dy) as f32).sqrt();
                        let brightness = (1.0 - dist * 0.35).max(0.0) * 0.9;
                        let offset = (py * width + px) * 3;
                        img.data[offset] = (img.data[offset] + brightness).min(1.0);
                        img.data[offset + 1] = (img.data[offset + 1] + brightness).min(1.0);
                        img.data[offset + 2] = (img.data[offset + 2] + brightness).min(1.0);
                    }
                }
            }
        }
        img
    };

    // Frame 1: Reference
    let frame1 = create_test_frame(0, 0);
    engine.feed_image(frame1, "Light_M45_10s_001.fits");

    let stats1 = engine.get_stats();
    assert_eq!(stats1.stacked_count, 1);
    assert_eq!(stats1.rejected_count, 0);
    assert_eq!(stats1.total_received, 1);
    assert!(stats1.detected_stars_reference >= 4);
    assert!(engine.get_current_image().is_some());

    // Frame 2: Slight translation (1 px shift, matching stars)
    let frame2 = create_test_frame(1, 1);
    engine.feed_image(frame2, "Light_M45_10s_002.fits");

    let stats2 = engine.get_stats();
    assert_eq!(stats2.stacked_count, 2);
    assert_eq!(stats2.rejected_count, 0);
    assert!((stats2.snr_gain_factor - 1.414).abs() < 0.05);

    // Frame 3: Black image with 0 stars (clouds rolled in) -> Should be rejected
    let frame3_cloud = AstroImage::new_rgb(200, 200, 0.02);
    engine.feed_image(frame3_cloud, "Light_M45_10s_003_cloud.fits");

    let stats3 = engine.get_stats();
    assert_eq!(stats3.stacked_count, 2); // stays 2
    assert_eq!(stats3.rejected_count, 1);
    assert!(!stats3.last_rejection_reason.is_empty());

    // Test Export
    let temp_fits = std::env::temp_dir().join("test_livestack_out.fits");
    let temp_png = std::env::temp_dir().join("test_livestack_out.png");
    assert!(engine.export_fits(&temp_fits).is_ok());
    assert!(engine.export_png(&temp_png, true).is_ok());
    assert!(temp_fits.exists());
    assert!(temp_png.exists());
    let _ = std::fs::remove_file(temp_fits);
    let _ = std::fs::remove_file(temp_png);

    // Test Reset
    engine.reset();
    let stats_reset = engine.get_stats();
    assert_eq!(stats_reset.stacked_count, 0);
    assert!(engine.get_current_image().is_none());

    // Test 2x Drizzle Live Stacking
    engine.update_config(|cfg| cfg.drizzle_scale = 2);
    let frame1_drizzle = create_test_frame(0, 0);
    engine.feed_image(frame1_drizzle, "Light_M45_2xDrizzle_001.fits");

    let stats_d1 = engine.get_stats();
    assert_eq!(stats_d1.stacked_count, 1);
    assert_eq!(stats_d1.drizzle_scale, 2);
    assert_eq!(stats_d1.output_resolution, (400, 400)); // 200 * 2 = 400

    let drizzle_img = engine.get_current_image().unwrap();
    assert_eq!(drizzle_img.width, 400);
    assert_eq!(drizzle_img.height, 400);

    // Feed second shifted frame into 2x Drizzle live stack
    let frame2_drizzle = create_test_frame(1, 1);
    engine.feed_image(frame2_drizzle, "Light_M45_2xDrizzle_002.fits");

    let stats_d2 = engine.get_stats();
    assert_eq!(stats_d2.stacked_count, 2);
    assert_eq!(stats_d2.output_resolution, (400, 400));
    let drizzle_img2 = engine.get_current_image().unwrap();
    assert_eq!(drizzle_img2.width, 400);
    assert_eq!(drizzle_img2.height, 400);

    // Test 3x Drizzle Live Stacking
    engine.reset();
    engine.update_config(|cfg| cfg.drizzle_scale = 3);
    let frame1_3x = create_test_frame(0, 0);
    engine.feed_image(frame1_3x, "Light_M45_3xDrizzle_001.fits");
    let stats_3x = engine.get_stats();
    assert_eq!(stats_3x.drizzle_scale, 3);
    assert_eq!(stats_3x.output_resolution, (600, 600)); // 200 * 3 = 600
    let img_3x = engine.get_current_image().unwrap();
    assert_eq!(img_3x.width, 600);
    assert_eq!(img_3x.height, 600);

    // Test 4x Drizzle Live Stacking
    engine.reset();
    engine.update_config(|cfg| cfg.drizzle_scale = 4);
    let frame1_4x = create_test_frame(0, 0);
    engine.feed_image(frame1_4x, "Light_M45_4xDrizzle_001.fits");
    let stats_4x = engine.get_stats();
    assert_eq!(stats_4x.drizzle_scale, 4);
    assert_eq!(stats_4x.output_resolution, (800, 800)); // 200 * 4 = 800
    let img_4x = engine.get_current_image().unwrap();
    assert_eq!(img_4x.width, 800);
    assert_eq!(img_4x.height, 800);
}

#[test]
fn test_synthetic_drizzle_3x_and_4x_stacking() {
    let width = 120;
    let height = 120;
    let create_frame = |sx: usize, sy: usize| {
        let mut img = AstroImage::new_rgb(width, height, 0.05);
        let star_coords = [(30 + sx, 30 + sy), (90 + sx, 40 + sy), (40 + sx, 90 + sy), (80 + sx, 80 + sy)];
        for (x, y) in star_coords {
            for dy in -2..=2 {
                for dx in -2..=2 {
                    let px = (x as isize + dx) as usize;
                    let py = (y as isize + dy) as usize;
                    if px < width && py < height {
                        let offset = (py * width + px) * 3;
                        img.data[offset] = 0.8;
                        img.data[offset + 1] = 0.8;
                        img.data[offset + 2] = 0.8;
                    }
                }
            }
        }
        img
    };

    let f1 = Arc::new(create_frame(0, 0));
    let f2 = Arc::new(create_frame(1, 1));
    let frames = vec![f1, f2];

    // 3x Drizzle Stacking
    let config_3x = StackingConfig {
        method: StackingMethod::Average,
        align_stars: false,
        auto_crop_borders: false,
        drizzle_scale: 3,
        ..Default::default()
    };
    let res_3x = Stacker::stack(&frames, &config_3x).expect("3x stacking failed");
    assert_eq!(res_3x.width, 120 * 3);
    assert_eq!(res_3x.height, 120 * 3);

    // 4x Drizzle Stacking
    let config_4x = StackingConfig {
        method: StackingMethod::Average,
        align_stars: false,
        auto_crop_borders: false,
        drizzle_scale: 4,
        ..Default::default()
    };
    let res_4x = Stacker::stack(&frames, &config_4x).expect("4x stacking failed");
    assert_eq!(res_4x.width, 120 * 4);
    assert_eq!(res_4x.height, 120 * 4);
}

#[test]
fn test_mosaic_planning() {
    use photon_forge::telescope::mosaic::{MosaicGridConfig, MosaicPlan, MosaicScopeModel};

    // 1. Configure 2x2 mosaic for M31 Andromeda with Seestar S-30
    let mut config = MosaicGridConfig::default();
    config.scope_model = MosaicScopeModel::SeestarS30;
    config.target_name = "M31 Andromeda".to_string();
    config.center_ra_hours = 0.712; // RA ~ 00h 42m
    config.center_dec_deg = 41.27;  // Dec ~ +41° 16'
    config.cols = 2;
    config.rows = 2;
    config.overlap_pct = 20.0;
    config.exposure_time_per_panel_mins = 45;
    config.use_lp_filter = true;

    let plan = MosaicPlan::compute(&config);

    // 2. Verify panel count and overall dimensions
    assert_eq!(plan.panels.len(), 4);
    assert!(plan.total_fov_w_deg > 3.0); // > 3 degrees wide
    assert!(plan.total_fov_h_deg > 1.8); // > 1.8 degrees tall
    assert_eq!(plan.total_exposure_mins, 4 * 45); // 180 mins = 3h

    // 3. Verify panel coordinates and sequence target conversion
    let seq_targets = plan.to_sequence_targets();
    assert_eq!(seq_targets.len(), 4);

    for (i, t) in seq_targets.iter().enumerate() {
        assert_eq!(t.duration_minutes, 45);
        assert!(t.use_lp_filter);
        assert!(t.name.contains("M31"));
        assert!(t.ra_hours >= 0.0 && t.ra_hours < 24.0);
        assert!(t.dec_deg >= -90.0 && t.dec_deg <= 90.0);
        assert_eq!(t.id, i + 1);
    }

    // 4. Test 3x3 preset and custom model
    config.cols = 3;
    config.rows = 3;
    let plan_3x3 = MosaicPlan::compute(&config);
    assert_eq!(plan_3x3.panels.len(), 9);
    assert!(plan_3x3.total_area_sq_deg > plan.total_area_sq_deg * 2.0);
}

#[test]
fn test_mosaic_stitcher_engine() {
    use photon_forge::core::image::{AstroImage, ImageMetadata};
    use photon_forge::processing::mosaic_stitcher::{MosaicStitchConfig, MosaicStitcher};
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    // Create a 2-panel horizontal mosaic with 30% overlap
    // Panel 1: [0..200, 0..150]
    // Panel 2: [140..340, 0..150] -> overlap is X in [140..200]
    let panel_w = 200;
    let panel_h = 150;

    // Star coordinates on global space [0..340, 0..150]
    let global_stars = vec![
        // Panel 1 only stars
        (30.0, 40.0, 1.0f32),
        (60.0, 100.0, 0.9f32),
        (90.0, 30.0, 0.8f32),
        (110.0, 120.0, 0.95f32),
        // Overlap region stars (X in 140..200)
        (150.0, 50.0, 1.0f32),
        (165.0, 95.0, 0.9f32),
        (180.0, 35.0, 0.85f32),
        (190.0, 115.0, 0.95f32),
        // Panel 2 only stars (X in 200..340)
        (220.0, 60.0, 0.8f32),
        (250.0, 130.0, 0.9f32),
        (280.0, 45.0, 1.0f32),
        (310.0, 90.0, 0.85f32),
    ];

    // Helper to render stars into an AstroImage
    let render_panel = |offset_x: f32, offset_y: f32| -> AstroImage {
        let mut data = vec![0.02f32; panel_w * panel_h * 3]; // faint background
        for (gx, gy, brightness) in &global_stars {
            let lx = gx - offset_x;
            let ly = gy - offset_y;
            if lx >= 2.0 && lx < (panel_w - 2) as f32 && ly >= 2.0 && ly < (panel_h - 2) as f32 {
                for dy in -2..=2 {
                    for dx in -2..=2 {
                        let r2 = (dx * dx + dy * dy) as f32;
                        let profile = (-r2 / 1.5).exp() * brightness;
                        let px = (lx + dx as f32).round() as usize;
                        let py = (ly + dy as f32).round() as usize;
                        if px < panel_w && py < panel_h {
                            let idx = (py * panel_w + px) * 3;
                            data[idx] = (data[idx] + profile).min(1.0);
                            data[idx + 1] = (data[idx + 1] + profile).min(1.0);
                            data[idx + 2] = (data[idx + 2] + profile).min(1.0);
                        }
                    }
                }
            }
        }

        AstroImage {
            width: panel_w,
            height: panel_h,
            channels: 3,
            data,
            metadata: ImageMetadata {
                file_name: "Panel.fits".to_string(),
                file_path: String::new(),
                exposure_time: Some(60.0),
                gain: Some(100.0),
                iso: None,
                target: Some("Test Target".to_string()),
                telescope: None,
                camera: None,
                date_obs: None,
                bitpix: Some(-32),
                bayer_pattern: None,
                filter_name: Some("RGB".to_string()),
            },
        }
    };

    let p1 = Arc::new(render_panel(0.0, 0.0));
    let p2 = Arc::new(render_panel(140.0, 0.0));

    let config = MosaicStitchConfig {
        feather_radius_px: 20.0,
        normalize_background: true,
        auto_crop: false,
        star_detection_count: 50,
    };

    let cancel = Arc::new(AtomicBool::new(false));
    let stitched = MosaicStitcher::stitch(&[p1, p2], &config, None, cancel).expect("Mosaic stitching failed");

    // Verified wide canvas: should be wider than individual panel (at least 300px!)
    assert!(stitched.width > 310, "Expected stitched canvas width > 310, got {}", stitched.width);
    assert_eq!(stitched.channels, 3);
    assert!(!stitched.data.is_empty());
}

#[test]
fn test_image_rotation_flip_and_crop() {
    let w = 10;
    let h = 20;
    let mut data = vec![0.0f32; w * h];
    data[0 * w + 0] = 1.0;         // Top-Left (0, 0)
    data[0 * w + (w - 1)] = 2.0;   // Top-Right (9, 0)
    data[(h - 1) * w + 0] = 3.0;   // Bottom-Left (0, 19)
    data[(h - 1) * w + (w - 1)] = 4.0; // Bottom-Right (9, 19)

    let img = AstroImage {
        width: w,
        height: h,
        channels: 1,
        data,
        metadata: ImageMetadata::default(),
    };

    // 1. Rotate 90 CW
    let r90 = img.rotate_90_cw();
    assert_eq!(r90.width, 20);
    assert_eq!(r90.height, 10);
    assert_eq!(r90.get_pixel(19, 0)[0], 1.0, "Top-Left goes to Top-Right in 90 CW");
    assert_eq!(r90.get_pixel(19, 9)[0], 2.0, "Top-Right goes to Bottom-Right in 90 CW");
    assert_eq!(r90.get_pixel(0, 0)[0], 3.0, "Bottom-Left goes to Top-Left in 90 CW");
    assert_eq!(r90.get_pixel(0, 9)[0], 4.0, "Bottom-Right goes to Bottom-Left in 90 CW");

    // 2. Rotate 90 CCW
    let r90ccw = img.rotate_90_ccw();
    assert_eq!(r90ccw.width, 20);
    assert_eq!(r90ccw.height, 10);
    assert_eq!(r90ccw.get_pixel(0, 9)[0], 1.0, "Top-Left goes to Bottom-Left in 90 CCW");
    assert_eq!(r90ccw.get_pixel(0, 0)[0], 2.0, "Top-Right goes to Top-Left in 90 CCW");
    assert_eq!(r90ccw.get_pixel(19, 9)[0], 3.0, "Bottom-Left goes to Bottom-Right in 90 CCW");
    assert_eq!(r90ccw.get_pixel(19, 0)[0], 4.0, "Bottom-Right goes to Top-Right in 90 CCW");

    // 3. Rotate 180
    let r180 = img.rotate_180();
    assert_eq!(r180.width, 10);
    assert_eq!(r180.height, 20);
    assert_eq!(r180.get_pixel(9, 19)[0], 1.0);
    assert_eq!(r180.get_pixel(0, 19)[0], 2.0);
    assert_eq!(r180.get_pixel(9, 0)[0], 3.0);
    assert_eq!(r180.get_pixel(0, 0)[0], 4.0);

    // 4. Flip Horizontal
    let fh = img.flip_horizontal();
    assert_eq!(fh.get_pixel(9, 0)[0], 1.0);
    assert_eq!(fh.get_pixel(0, 0)[0], 2.0);
    assert_eq!(fh.get_pixel(9, 19)[0], 3.0);
    assert_eq!(fh.get_pixel(0, 19)[0], 4.0);

    // 5. Flip Vertical
    let fv = img.flip_vertical();
    assert_eq!(fv.get_pixel(0, 19)[0], 1.0);
    assert_eq!(fv.get_pixel(9, 19)[0], 2.0);
    assert_eq!(fv.get_pixel(0, 0)[0], 3.0);
    assert_eq!(fv.get_pixel(9, 0)[0], 4.0);

    // 6. Arbitrary angle 45 deg
    let r45 = img.rotate_arbitrary(45.0, false).expect("Arbitrary rotation failed");
    assert_eq!(r45.width, 10);
    assert_eq!(r45.height, 20);

    // 7. Crop
    let cropped = img.crop(1, 2, 5, 8).expect("Crop failed");
    assert_eq!(cropped.width, 5);
    assert_eq!(cropped.height, 8);
    assert_eq!(cropped.channels, 1);
}

#[test]
fn test_syqon_engine_integration() {
    use photon_forge::core::image::AstroImage;
    use photon_forge::processing::syqon::{SyQonEngine, SyQonModel, SyQonProgress, SyQonTask};
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    // 1. Model identities and CLI IDs
    assert_eq!(SyQonModel::AxiomMini.cli_model_id(), "axiom-mini");
    assert_eq!(SyQonModel::AxiomV3.cli_model_id(), "axiom");
    assert_eq!(SyQonModel::ParallaxNano.cli_model_id(), "parallax-nano");
    assert_eq!(SyQonModel::ParallaxAesthetics.cli_model_id(), "parallax");
    assert_eq!(SyQonModel::ParallaxAesthetics.family(), Some("aesthetics"));
    assert_eq!(SyQonModel::ParallaxClassic.family(), Some("classic"));
    assert_eq!(SyQonModel::PrismEssential.cli_model_id(), "prism-essential");
    assert_eq!(SyQonModel::DeepGradient.cli_model_id(), "deep-gradient");
    assert!(SyQonModel::AxiomMini.is_free());
    assert!(SyQonModel::ParallaxNano.is_free());
    assert!(SyQonModel::PrismEssential.is_free());

    // 2. Progress parser test
    let (stage, frac) = SyQonEngine::parse_progress_line("axiom-mini 45%").expect("Should parse progress");
    assert_eq!(stage, "axiom-mini");
    assert!((frac - 0.45).abs() < 1e-4);

    let (stage2, frac2) = SyQonEngine::parse_progress_line("90%").expect("Should parse plain pct");
    assert_eq!(stage2, "Käsitellään");
    assert!((frac2 - 0.90).abs() < 1e-4);

    // 3. Simulated execution test with synthetic image
    let mut synthetic = AstroImage::new_rgb(64, 64, 0.05);
    // Draw some bright stars
    for s in [(15, 15), (40, 20), (30, 45)] {
        for dy in 0..3 {
            for dx in 0..3 {
                let idx = ((s.1 + dy) * 64 + (s.0 + dx)) * 3;
                synthetic.data[idx] = 1.0;
                synthetic.data[idx + 1] = 0.95;
                synthetic.data[idx + 2] = 0.90;
            }
        }
    }
    synthetic.metadata.file_name = "Synthetic_Pleiades.fits".to_string();

    let progress_log = Arc::new(Mutex::new(Vec::<SyQonProgress>::new()));
    let p_clone = Arc::clone(&progress_log);
    let cancel = Arc::new(AtomicBool::new(false));

    // A. Axiom Star Separation
    let axiom_task = SyQonTask::StarSeparation {
        model: SyQonModel::AxiomMini,
        stretch: "auto".to_string(),
        generate_stars_only: true,
    };
    let res = SyQonEngine::run_simulated(
        &synthetic,
        &axiom_task,
        Some(Arc::new(move |p| p_clone.lock().unwrap().push(p))),
        cancel.clone(),
    ).expect("Axiom simulation should succeed");

    assert_eq!(res.primary.width, 64);
    assert_eq!(res.primary.height, 64);
    assert!(res.secondary.is_some(), "Stars-only image should be generated");
    let stars = res.secondary.unwrap();
    assert_eq!(stars.width, 64);
    assert!(res.primary.metadata.file_name.contains("Starless"));
    assert!(stars.metadata.file_name.contains("Stars"));
    assert!(!progress_log.lock().unwrap().is_empty());

    // B. Parallax Reduction & Deblur
    let parallax_task = SyQonTask::Parallax {
        model: SyQonModel::ParallaxNano,
        correction: true,
        reduction: true,
        reduction_level: 5,
        deblur: true,
        deblur_strength: 0.5,
    };
    let res_par = SyQonEngine::run_simulated(&synthetic, &parallax_task, None, cancel.clone())
        .expect("Parallax simulation should succeed");
    assert_eq!(res_par.primary.width, 64);
    assert!(res_par.primary.metadata.file_name.contains("Parallax"));

    // C. Deep Gradient
    let grad_task = SyQonTask::Gradient { save_gradient: true };
    let res_grad = SyQonEngine::run_simulated(&synthetic, &grad_task, None, cancel)
        .expect("Deep gradient simulation should succeed");
    assert_eq!(res_grad.primary.width, 64);
    assert!(res_grad.secondary.is_some());

    println!("✨ SyQon Neural Engine integration test passed successfully!");
}

#[test]
fn test_night_automation_dither_autofocus_and_reconnect() {
    use photon_forge::telescope::seestar::{DitherConfig, SeestarClient, SeestarState};
    use photon_forge::telescope::sequencer::{
        AutoFocusConfig, ConnectionGuardConfig, JobStatus, NightSequencer,
    };
    use std::time::Duration;

    let client = SeestarClient::new();
    client.set_simulation(true);
    std::thread::sleep(Duration::from_millis(150));
    let telem0 = client.get_telemetry();
    assert!(telem0.is_simulation);
    assert!(telem0.auto_reconnect_enabled);
    assert!(telem0.dither_config.enabled);
    assert_eq!(telem0.dither_config.interval_frames, 3);
    assert_eq!(telem0.dither_config.step_pixels, 8);

    // 1. Test Dither configuration & execution
    let custom_dither = DitherConfig {
        enabled: true,
        interval_frames: 2,
        step_pixels: 12,
        settle_seconds: 1,
    };
    client.configure_dither(custom_dither);
    std::thread::sleep(Duration::from_millis(150));
    let telem1 = client.get_telemetry();
    assert_eq!(telem1.dither_config.interval_frames, 2);
    assert_eq!(telem1.dither_config.step_pixels, 12);

    let ra_before = telem1.current_ra;
    let dec_before = telem1.current_dec;
    client.trigger_dither(12, 1);
    std::thread::sleep(Duration::from_millis(200));
    let telem_dithering = client.get_telemetry();
    assert_eq!(telem_dithering.dither_count, 1);
    assert_eq!(telem_dithering.state, SeestarState::Dithering);
    assert!(
        (telem_dithering.current_ra - ra_before).abs() > 0.0
            || (telem_dithering.current_dec - dec_before).abs() > 0.0,
        "Dither should apply micro-offset to pointing coordinates"
    );

    // Wait for settle completion
    std::thread::sleep(Duration::from_millis(1100));
    let telem_settled = client.get_telemetry();
    assert_ne!(telem_settled.state, SeestarState::Dithering);

    // 2. Test Auto-Focus execution
    client.auto_focus();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(client.get_telemetry().state, SeestarState::Focusing);

    // Wait for simulated AF (3s) to complete
    std::thread::sleep(Duration::from_millis(3200));
    let telem_focused = client.get_telemetry();
    assert_eq!(telem_focused.state, SeestarState::Tracking);
    assert!(telem_focused.last_focus_temp_c.is_some());
    assert!(telem_focused.last_focus_time.is_some());

    // 3. Test Sequencer Automation Configurations
    let seq = NightSequencer::new();
    {
        let s = seq.state.lock().unwrap();
        assert_eq!(s.autofocus_config, AutoFocusConfig::default());
        assert_eq!(s.dither_config, DitherConfig::default());
        assert_eq!(s.connection_guard, ConnectionGuardConfig::default());
        assert_eq!(JobStatus::Focusing.display_fi(), "Tarkentaa...");
        assert_eq!(JobStatus::Dithering.display_fi(), "Ditheröi...");
        assert_eq!(JobStatus::WaitingConnection.display_fi(), "Odottaa yhteyttä...");
    }
}

#[test]
fn test_pcc_and_spcc_photometric_color_calibration() {
    use photon_forge::core::image::AstroImage;
    use photon_forge::processing::pcc::{
        PccConfig, PccMode, PhotometricColorCalibrator, SensorProfile, SpccFilterProfile,
        WhiteReference,
    };

    // Create a synthetic 128x128 RGB star field with an intentional strong green/red sky cast
    // and imbalanced star colors (e.g. raw uncalibrated sensor where R is 0.65x and B is 0.55x of G)
    let width = 128;
    let height = 128;
    let mut img = AstroImage::new_rgb(width, height, 0.0);
    let bg_r = 0.08f32;
    let bg_g = 0.14f32;
    let bg_b = 0.06f32;

    for y in 0..height {
        for x in 0..width {
            let px = img.get_pixel_mut(x, y);
            px[0] = bg_r;
            px[1] = bg_g;
            px[2] = bg_b;
        }
    }

    // Add 10 unsaturated synthetic stars with Gaussian-like 5x5 profiles
    let star_positions = [
        (25, 25),
        (55, 22),
        (95, 30),
        (30, 60),
        (64, 64),
        (100, 65),
        (22, 98),
        (58, 102),
        (92, 96),
        (78, 42),
    ];

    for &(sx, sy) in &star_positions {
        for dy in -3i32..=3 {
            for dx in -3i32..=3 {
                let dist_sq = (dx * dx + dy * dy) as f32;
                let profile = (-dist_sq / 2.2).exp() * 0.55;
                let px = img.get_pixel_mut((sx + dx) as usize, (sy + dy) as usize);
                // Simulated uncalibrated G2V star where R = 0.65 * G, B = 0.55 * G
                px[0] += profile * 0.65;
                px[1] += profile * 1.00;
                px[2] += profile * 0.55;
            }
        }
    }

    img.metadata.telescope = Some("ZWO Seestar S30".to_string());
    img.metadata.filter_name = Some("LP".to_string());

    // 1. Test PCC with G2V reference
    let mut cfg = PccConfig {
        mode: PccMode::Pcc,
        white_reference: WhiteReference::G2VStar,
        neutralize_background: true,
        ..Default::default()
    };

    let res = PhotometricColorCalibrator::calibrate(&img, &cfg).expect("PCC calibration should succeed");
    assert!(res.detected_stars >= 8, "Should detect synthetic stars");
    assert!(res.valid_stars >= 8, "Should validate unsaturated stars");
    // Since R was 0.65*G and B was 0.55*G, k_r should be > 1.3 and k_b should be > 1.5
    assert!(res.k_r > 1.30 && res.k_r < 1.80, "k_r was {}", res.k_r);
    assert!((res.k_g - 1.0).abs() < 1e-4);
    assert!(res.k_b > 1.50 && res.k_b < 2.10, "k_b was {}", res.k_b);

    // Check that sky background is neutralized across R, G, B
    let (cal_bg_r, _) = res.image.channel_median_and_mad(0);
    let (cal_bg_g, _) = res.image.channel_median_and_mad(1);
    let (cal_bg_b, _) = res.image.channel_median_and_mad(2);
    assert!((cal_bg_r - cal_bg_g).abs() < 0.01, "Background R and G should be neutralized");
    assert!((cal_bg_b - cal_bg_g).abs() < 0.01, "Background B and G should be neutralized");

    // 2. Test SPCC metadata auto-detection and calibration
    let note = cfg.auto_detect_from_metadata(&img.metadata);
    assert!(note.contains("Seestar S30"));
    assert_eq!(cfg.sensor, SensorProfile::SeestarS30Imx662);
    assert_eq!(cfg.filter, SpccFilterProfile::SeestarDualBandLp);

    cfg.mode = PccMode::Spcc;
    let spcc_res = PhotometricColorCalibrator::calibrate(&img, &cfg).expect("SPCC calibration should succeed");
    assert!(spcc_res.k_r > res.k_r, "Dual-Band LP SPCC should boost Ha/Red relative to broadband PCC");
}

#[test]
fn test_wcs_plate_solver_and_catalog_annotation() {
    use photon_forge::core::image::AstroImage;
    use photon_forge::processing::plate_solve::PlateSolver;
    use photon_forge::telescope::catalog::load_sky_catalog;

    let catalog = load_sky_catalog();
    let mut img = AstroImage::new_rgb(1080, 1920, 0.05);
    img.metadata.file_name = "M31_Andromeda_Stack.fits".to_string();
    img.metadata.target = Some("M31".to_string());
    img.metadata.telescope = Some("Seestar S30".to_string());

    // Add a few bright synthetic stars
    for &(sx, sy) in &[(540, 960), (300, 400), (750, 1200), (420, 1500)] {
        for dy in -2i32..=2 {
            for dx in -2i32..=2 {
                let px = img.get_pixel_mut((sx + dx) as usize, (sy + dy) as usize);
                px[0] = 0.85;
                px[1] = 0.85;
                px[2] = 0.85;
            }
        }
    }

    let wcs = PlateSolver::solve(&img, &catalog, None);
    // Should match M31 (RA ~0.712h, Dec ~41.27°) and Seestar S30 scale (3.99"/px)
    assert!((wcs.center_ra_hours - 0.712).abs() < 0.05);
    assert!((wcs.center_dec_deg - 41.27).abs() < 0.1);
    assert!((wcs.arcsec_per_px - 3.99).abs() < 0.05);
    assert!(wcs.detected_stars.len() >= 4);

    // Both M31 and companion galaxy M32 (and M110) should be projected into matched_objects!
    let matched_ids: Vec<&str> = wcs.matched_objects.iter().map(|o| o.id.as_str()).collect();
    assert!(matched_ids.contains(&"M31"), "M31 should be annotated in field");
    assert!(matched_ids.contains(&"M32"), "Companion M32 should also be annotated in field");

    // Verify Gnomonic projection round-trip accuracy (pixel -> radec -> pixel)
    let (ra, dec) = wcs.pixel_to_radec(250.0, 400.0);
    let (px_back, py_back) = wcs
        .radec_to_pixel(ra, dec)
        .expect("Forward Gnomonic projection should succeed");
    assert!((px_back - 250.0).abs() < 0.05, "px_back was {}", px_back);
    assert!((py_back - 400.0).abs() < 0.05, "py_back was {}", py_back);
}

#[test]
fn test_astrometry_net_image_preparation_and_config() {
    use photon_forge::core::image::AstroImage;
    use photon_forge::processing::plate_solve::{AstrometryConfig, AstrometryNetSolver};

    // 1. Verify default config and serialization
    let mut cfg = AstrometryConfig::default();
    cfg.api_key = "test_api_key_123".to_string();
    cfg.use_online_by_default = true;
    let json = serde_json::to_string(&cfg).expect("Config should serialize to JSON");
    let deser: AstrometryConfig = serde_json::from_str(&json).expect("Config should deserialize");
    assert_eq!(deser.api_key, "test_api_key_123");
    assert!(deser.use_online_by_default);

    // 2. Verify image preparation and JPEG encoding
    let mut img = AstroImage::new_rgb(2000, 1500, 0.02);
    // Draw central star
    for dy in -3i32..=3 {
        for dx in -3i32..=3 {
            let px = img.get_pixel_mut((1000 + dx) as usize, (750 + dy) as usize);
            px[0] = 0.9;
            px[1] = 0.9;
            px[2] = 0.9;
        }
    }

    let (jpeg_bytes, scale_factor) = AstrometryNetSolver::prepare_image(&img)
        .expect("Image preparation for Astrometry.net should succeed");

    assert!(scale_factor > 1.4, "2000px image should be scaled down to max 1400px");
    assert!(!jpeg_bytes.is_empty());
    // JPEG header check: 0xFF 0xD8 0xFF
    assert_eq!(jpeg_bytes[0], 0xFF);
    assert_eq!(jpeg_bytes[1], 0xD8);
    assert_eq!(jpeg_bytes[2], 0xFF);
}

#[test]
fn test_astrometry_net_cancellation() {
    use photon_forge::core::image::AstroImage;
    use photon_forge::processing::plate_solve::{AstrometryConfig, AstrometryNetSolver};
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    let img = AstroImage::new_rgb(400, 300, 0.02);
    let mut cfg = AstrometryConfig::default();
    cfg.api_key = "dummy_key".to_string();

    let cancel = Arc::new(AtomicBool::new(true)); // Already canceled
    let result = AstrometryNetSolver::solve_online(&img, &cfg, &[], None, Some(cancel), None);
    assert!(result.is_err(), "Pre-canceled solve should return an error");
    let err = result.unwrap_err();
    assert!(err.contains("keskeytti") || err.contains("cancel"), "Error should report cancellation, got: {}", err);
}

#[test]
fn test_stretch_modes_and_linear_default_after_stacking() {
    use photon_forge::core::image::AstroImage;
    use photon_forge::processing::histogram::HistogramProcessor;
    use photon_forge::processing::stacking::StackingConfig;
    use photon_forge::processing::stf::{StfParameters, StretchMode};

    // 1. Verify StackingConfig default: auto_stretch_after_stack is FALSE (Linear default)
    let stack_cfg = StackingConfig::default();
    assert!(!stack_cfg.auto_stretch_after_stack, "Default after stacking must be pure LINEAR (no auto-stretch)");

    // 2. Create a synthetic test image with dark linear background (0.01..0.02) and bright core/stars (0.95)
    let mut img = AstroImage::new_rgb(100, 100, 0.015);
    for y in 0..100 {
        for x in 0..100 {
            let px = img.get_pixel_mut(x, y);
            let n = ((x * 17 + y * 31) % 100) as f32 / 5000.0; // small variation so MAD > 0
            px[0] = 0.015 + n;
            px[1] = 0.018 + n;
            px[2] = 0.022 + n;
        }
    }
    // Set bright red nebula and blue star
    let neb = img.get_pixel_mut(50, 50);
    neb[0] = 0.8;
    neb[1] = 0.2;
    neb[2] = 0.1;

    // 3. Test Linear mode (Pure 1:1 display)
    let mut stf = StfParameters::default();
    stf.set_linear();
    assert!(stf.is_linear());
    assert_eq!(stf.mode, StretchMode::Linear);

    let linear_bytes = img.to_rgb8_with_stf(&stf);
    assert_eq!(linear_bytes.len(), 100 * 100 * 3);
    // Background should be very dark in linear mode (approx 0.01 * 255 = 2..3)
    assert!(linear_bytes[0] <= 4, "Linear background must remain dark (raw pixel value)");

    // 4. Test Auto-STF mode
    stf.set_mode(StretchMode::AutoStf);
    let auto_stf = StfParameters::compute_auto(&img, false);
    stf.channels = auto_stf.channels;
    assert!(!stf.is_linear());
    let stf_bytes = img.to_rgb8_with_stf(&stf);
    println!("stf_bytes[0] = {}, linear_bytes[0] = {}, ch0 = {:?}", stf_bytes[0], linear_bytes[0], stf.channels[0]);
    assert!(stf_bytes[0] >= linear_bytes[0]);

    // 5. Test Arcsinh stretch (color preserving)
    stf.set_mode(StretchMode::Arcsinh);
    stf.arcsinh_stretch = 50.0;
    stf.arcsinh_black = 0.005;
    let arcsinh_bytes = img.to_rgb8_with_stf(&stf);
    assert!(!arcsinh_bytes.is_empty());
    // Nebula pixel (50, 50) index = (50 * 100 + 50) * 3 = 15150
    let neb_idx = (50 * 100 + 50) * 3;
    assert!(arcsinh_bytes[neb_idx] > arcsinh_bytes[neb_idx + 1]); // Red channel dominates
    assert!(arcsinh_bytes[neb_idx] > 200);

    // 6. Test Logarithmic stretch
    stf.set_mode(StretchMode::Logarithmic);
    stf.log_factor = 1000.0;
    let log_bytes = img.to_rgb8_with_stf(&stf);
    assert!(!log_bytes.is_empty());

    // 7. Test Generalized Hyperbolic Stretch (GHS)
    stf.set_mode(StretchMode::Ghs);
    stf.ghs_d = 12.0;
    stf.ghs_b = 0.02;
    let ghs_bytes = img.to_rgb8_with_stf(&stf);
    assert!(!ghs_bytes.is_empty());

    // 8. Test Root / Gamma stretch
    stf.set_mode(StretchMode::Root);
    stf.gamma = 0.5; // Square root
    let root_bytes = img.to_rgb8_with_stf(&stf);
    assert!(!root_bytes.is_empty());

    // 8b. Test HyperMetric Stretch (HMS)
    stf.set_mode(StretchMode::HyperMetric);
    stf.hms_d = 40.0;
    stf.hms_b = 0.05;
    stf.hms_sp = 0.01;
    stf.hms_color_grip = 1.2;
    let hms_bytes = img.to_rgb8_with_stf(&stf);
    assert!(!hms_bytes.is_empty());
    assert!(hms_bytes[neb_idx] > hms_bytes[neb_idx + 1]); // Red channel dominates and preserved
    assert!(hms_bytes[neb_idx] > 180);

    let hms_baked = HistogramProcessor::apply_permanent_stretch_params(&img, &stf);
    assert_eq!(hms_baked.width, 100);
    assert!(hms_baked.data[neb_idx] > hms_baked.data[neb_idx + 1]);

    // 9. Test Permanent Stretch Baking
    let perm_stretched = HistogramProcessor::apply_permanent_stretch_params(&img, &stf);
    assert_eq!(perm_stretched.width, 100);
    assert_eq!(perm_stretched.height, 100);
    assert!(perm_stretched.data[0] > 0.05); // Data itself is now stretched
}

#[test]
fn test_image_sharpening_usm_and_wavelet() {
    use photon_forge::core::image::AstroImage;
    use photon_forge::processing::sharpening::{ImageSharpening, SharpeningConfig, SharpeningMethod};

    let mut img = AstroImage::new_rgb(100, 100, 0.05);
    let center_x = 50usize;
    let center_y = 50usize;
    for y in 40..60 {
        for x in 40..60 {
            let dx = (x as f32) - 50.0;
            let dy = (y as f32) - 50.0;
            let dist_sq = dx * dx + dy * dy;
            let val = 0.05 + 0.6 * (-dist_sq / 12.0).exp();
            let px = img.get_pixel_mut(x, y);
            px[0] = val;         // Red
            px[1] = val * 0.5;   // Green
            px[2] = val * 0.2;   // Blue
        }
    }

    let orig_center = img.get_pixel(center_x, center_y)[0];
    let orig_wing = img.get_pixel(center_x + 3, center_y)[0];
    let orig_ratio = orig_center / orig_wing;

    // 1. Test Unsharp Mask
    let mut usm_cfg = SharpeningConfig::default();
    usm_cfg.method = SharpeningMethod::UnsharpMask;
    usm_cfg.usm_radius = 2.0;
    usm_cfg.usm_amount = 1.5;
    usm_cfg.usm_threshold = 0.001;
    usm_cfg.luminance_only = true;

    let usm_img = ImageSharpening::sharpen(&img, &usm_cfg).expect("USM sharpening should succeed");
    let usm_center = usm_img.get_pixel(center_x, center_y)[0];
    let usm_wing = usm_img.get_pixel(center_x + 3, center_y)[0];
    let usm_ratio = usm_center / usm_wing;
    assert!(usm_ratio > orig_ratio, "USM sharpening should increase center-to-wing star contrast");

    // Check chromaticity preservation (Luminance-only mode)
    let px = usm_img.get_pixel(center_x, center_y);
    let r_ratio = px[1] / px[0];
    assert!((r_ratio - 0.5).abs() < 0.05, "Luminance-only sharpening must preserve color ratios");

    // 2. Test Multiscale Wavelet Sharpening
    let mut wav_cfg = SharpeningConfig::default();
    wav_cfg.method = SharpeningMethod::Wavelet;
    wav_cfg.wavelet_scale1 = 1.5;
    wav_cfg.wavelet_scale2 = 1.8;
    wav_cfg.wavelet_scale3 = 1.2;
    wav_cfg.wavelet_noise_threshold = 0.002;
    wav_cfg.luminance_only = true;

    let wav_img = ImageSharpening::sharpen(&img, &wav_cfg).expect("Wavelet sharpening should succeed");
    let wav_center = wav_img.get_pixel(center_x, center_y)[0];
    assert!(wav_center > orig_center, "Wavelet detail boost should increase peak sharpness");
}





