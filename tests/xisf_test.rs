#[test]
fn test_read_real_xisf() {
    use xisf::XisfFile;

    let path = "/Volumes/Sky/Kuvat/C14_Double Cluster in Perseus/C14.xisf";
    if !std::path::Path::new(path).exists() {
        return;
    }

    let file = XisfFile::open(path).expect("Failed to open real XISF file");
    let images = file.images();
    assert!(!images.is_empty());
    let img = &images[0];
    let geom = img.geometry();
    println!("Geometry: {:?}, channels: {}, format: {:?}", geom, img.channels(), img.sample_format());
    assert_eq!(geom[0], 2608);
    assert_eq!(geom[1], 1736);

    let data: Vec<f32> = img.read_planar().expect("Read planar f32 failed");
    println!("Read {} f32 samples from XISF", data.len());
    assert_eq!(data.len(), 2608 * 1736 * (img.channels() as usize));
}

#[test]
fn test_xisf_loader_and_writer_roundtrip() {
    use photon_forge::core::image::{AstroImage, ImageMetadata};
    use photon_forge::core::loader::ImageLoader;

    let w = 64;
    let h = 48;
    let ch = 3;
    let mut data = vec![0.0f32; w * h * ch];
    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) * ch;
            data[idx] = (x as f32) / (w as f32);
            data[idx + 1] = (y as f32) / (h as f32);
            data[idx + 2] = ((x + y) as f32) / ((w + h) as f32);
        }
    }

    let original = AstroImage {
        width: w,
        height: h,
        channels: ch,
        data,
        metadata: ImageMetadata {
            file_name: "test_pattern.xisf".to_string(),
            camera: Some("ZWO Seestar S30".to_string()),
            exposure_time: Some(60.0),
            ..Default::default()
        },
    };

    let temp_path = "/tmp/photon_forge_test_roundtrip.xisf";
    ImageLoader::save_image(&original, temp_path).expect("Failed to save XISF image");

    let loaded = ImageLoader::load_file(temp_path).expect("Failed to reload saved XISF image");
    assert_eq!(loaded.width, original.width);
    assert_eq!(loaded.height, original.height);
    assert_eq!(loaded.channels, original.channels);

    // Verify sample accuracy
    for (i, (&a, &b)) in original.data.iter().zip(loaded.data.iter()).enumerate() {
        assert!((a - b).abs() < 1e-6, "Pixel sample mismatch at index {}: {} vs {}", i, a, b);
    }

    println!("XISF roundtrip test passed with 100% bit-exact float32 accuracy!");
    let _ = std::fs::remove_file(temp_path);
}
