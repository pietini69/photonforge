use crate::core::image::AstroImage;
use crate::core::loader::ImageLoader;
use crate::processing::alignment::{Star, StarDetector, StarMatcher, Transform2D};
use crate::processing::stf::StfParameters;
use chrono::Local;
use rayon::prelude::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Clone, Debug)]
pub struct LiveStackConfig {
    pub enabled: bool,
    pub align_frames: bool,
    pub auto_stretch: bool,
    pub min_stars: usize,
    pub drizzle_scale: usize,
    pub default_sub_exposure_secs: f32,
    #[allow(dead_code)]
    pub watch_directory: Option<PathBuf>,
}

impl Default for LiveStackConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            align_frames: true,
            auto_stretch: true,
            min_stars: 4,
            drizzle_scale: 1,
            default_sub_exposure_secs: 10.0,
            watch_directory: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LiveStackStats {
    pub total_received: usize,
    pub stacked_count: usize,
    pub rejected_count: usize,
    pub last_rejection_reason: String,
    pub total_exposure_secs: f32,
    pub snr_gain_factor: f32,
    pub detected_stars_reference: usize,
    pub detected_stars_last_frame: usize,
    pub last_frame_name: String,
    pub last_frame_time: String,
    pub is_running: bool,
    pub status_message: String,
    pub drizzle_scale: usize,
    pub output_resolution: (usize, usize),
}

impl Default for LiveStackStats {
    fn default() -> Self {
        Self {
            total_received: 0,
            stacked_count: 0,
            rejected_count: 0,
            last_rejection_reason: String::new(),
            total_exposure_secs: 0.0,
            snr_gain_factor: 1.0,
            detected_stars_reference: 0,
            detected_stars_last_frame: 0,
            last_frame_name: String::new(),
            last_frame_time: String::new(),
            is_running: true,
            status_message: "Odottaa ensimmäistä valotusta...".to_string(),
            drizzle_scale: 1,
            output_resolution: (0, 0),
        }
    }
}

struct EngineInternal {
    config: LiveStackConfig,
    stats: LiveStackStats,
    reference_image: Option<Arc<AstroImage>>,
    reference_stars: Vec<Star>,
    accum_sum: Vec<f32>,
    coverage_count: Vec<u32>,
    current_stacked: Option<Arc<AstroImage>>,
    stf_params: StfParameters,
    processed_paths: HashSet<PathBuf>,
}

pub struct LiveStackEngine {
    internal: Arc<Mutex<EngineInternal>>,
    frame_tx: Sender<PathBuf>,
    stop_signal: Arc<AtomicBool>,
}

impl LiveStackEngine {
    pub fn new() -> Arc<Self> {
        let (tx, rx): (Sender<PathBuf>, Receiver<PathBuf>) = channel();
        let stop_signal = Arc::new(AtomicBool::new(false));

        let internal = Arc::new(Mutex::new(EngineInternal {
            config: LiveStackConfig::default(),
            stats: LiveStackStats::default(),
            reference_image: None,
            reference_stars: Vec::new(),
            accum_sum: Vec::new(),
            coverage_count: Vec::new(),
            current_stacked: None,
            stf_params: StfParameters::default(),
            processed_paths: HashSet::new(),
        }));

        let internal_worker = Arc::clone(&internal);
        let stop_clone = Arc::clone(&stop_signal);

        // Dedicated background processor for incoming frames
        thread::spawn(move || {
            while !stop_clone.load(Ordering::SeqCst) {
                // Wait for new frame path with a short timeout so stop_signal is checked
                if let Ok(path) = rx.recv_timeout(std::time::Duration::from_millis(200)) {
                    Self::process_file_in_worker(&internal_worker, path);
                }
            }
        });

        Arc::new(Self {
            internal,
            frame_tx: tx,
            stop_signal,
        })
    }

    /// Process a new image file in the background thread
    fn process_file_in_worker(internal_arc: &Arc<Mutex<EngineInternal>>, path: PathBuf) {
        {
            let internal = internal_arc.lock().unwrap();
            if !internal.config.enabled || !internal.stats.is_running {
                return;
            }
            if internal.processed_paths.contains(&path) {
                return;
            }
        }

        // Load image from disk
        let image = match ImageLoader::load_file(&path) {
            Ok(img) => img,
            Err(e) => {
                let mut internal = internal_arc.lock().unwrap();
                internal.stats.total_received += 1;
                internal.stats.rejected_count += 1;
                internal.stats.last_rejection_reason = format!("Tiedoston lukuvirhe: {}", e);
                return;
            }
        };

        let file_name = path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| "Tuntematon ruutu".to_string());

        Self::integrate_frame(internal_arc, image, &file_name, Some(path));
    }

    /// Feeds an already loaded AstroImage directly (useful for tests and manual import)
    #[allow(dead_code)]
    pub fn feed_image(&self, image: AstroImage, name: &str) {
        Self::integrate_frame(&self.internal, image, name, None);
    }

    /// Feeds a file path to the background queue for asynchronous processing
    pub fn feed_file(&self, path: PathBuf) {
        let _ = self.frame_tx.send(path);
    }

    /// Core integration logic for an incoming frame
    fn integrate_frame(
        internal_arc: &Arc<Mutex<EngineInternal>>,
        incoming: AstroImage,
        name: &str,
        path: Option<PathBuf>,
    ) {
        let now_str = Local::now().format("%H:%M:%S").to_string();

        let mut internal = internal_arc.lock().unwrap();
        internal.stats.total_received += 1;
        internal.stats.last_frame_name = name.to_string();
        internal.stats.last_frame_time = now_str.clone();

        if let Some(p) = path {
            internal.processed_paths.insert(p);
        }

        let sub_exp = internal.config.default_sub_exposure_secs;
        let scale = internal.config.drizzle_scale.clamp(1, 4);

        // 1. If this is the FIRST frame: establish as Reference Frame
        if internal.reference_image.is_none() {
            let stars = StarDetector::detect(&incoming, 60);
            let star_count = stars.len();

            let out_w = incoming.width * scale;
            let out_h = incoming.height * scale;
            let total_pixels = out_w * out_h;
            let channels = incoming.channels;

            internal.reference_stars = stars;
            internal.reference_image = Some(Arc::new(incoming.clone()));

            if scale == 1 {
                internal.accum_sum = incoming.data.clone();
                internal.coverage_count = vec![1; total_pixels];
                let auto_stf = StfParameters::compute_auto(&incoming, false);
                internal.stf_params = auto_stf;
                internal.current_stacked = Some(Arc::new(incoming));
            } else {
                let mut accum = vec![0.0f32; total_pixels * channels];
                let mut coverage = vec![0u32; total_pixels];
                Self::accumulate_frame_scaled(
                    &incoming,
                    &Transform2D::identity(),
                    scale,
                    out_w,
                    0.0,
                    &mut accum,
                    &mut coverage,
                );

                let mut stacked_data = vec![0.0f32; total_pixels * channels];
                stacked_data
                    .par_chunks_exact_mut(out_w * channels)
                    .zip(accum.par_chunks_exact(out_w * channels))
                    .zip(coverage.par_chunks_exact(out_w))
                    .for_each(|((out_row, accum_row), cov_row)| {
                        for x in 0..out_w {
                            let cov = cov_row[x].max(1) as f32;
                            let idx = x * channels;
                            for c in 0..channels {
                                out_row[idx + c] = accum_row[idx + c] / cov;
                            }
                        }
                    });

                let drizzle_img = AstroImage {
                    width: out_w,
                    height: out_h,
                    channels,
                    data: stacked_data,
                    metadata: crate::core::image::ImageMetadata {
                        file_name: format!("{}_{}xDrizzle.fits", name, scale),
                        exposure_time: Some(sub_exp),
                        ..Default::default()
                    },
                };

                let auto_stf = StfParameters::compute_auto(&drizzle_img, false);
                internal.stf_params = auto_stf;
                internal.current_stacked = Some(Arc::new(drizzle_img));
                internal.accum_sum = accum;
                internal.coverage_count = coverage;
            }

            internal.stats.stacked_count = 1;
            internal.stats.total_exposure_secs = sub_exp;
            internal.stats.snr_gain_factor = 1.0;
            internal.stats.detected_stars_reference = star_count;
            internal.stats.detected_stars_last_frame = star_count;
            internal.stats.drizzle_scale = scale;
            internal.stats.output_resolution = (out_w, out_h);
            internal.stats.status_message = format!(
                "✓ Referenssiruutu asetettu: {} ({} tähteä{})",
                name,
                star_count,
                if scale > 1 { format!(" [{}x Drizzle]", scale) } else { "".to_string() }
            );
            return;
        }

        // 2. Validate dimensions against 1x reference frame
        let ref_w = internal.reference_image.as_ref().unwrap().width;
        let ref_h = internal.reference_image.as_ref().unwrap().height;
        let ref_ch = internal.reference_image.as_ref().unwrap().channels;

        if incoming.width != ref_w || incoming.height != ref_h || incoming.channels != ref_ch {
            internal.stats.rejected_count += 1;
            internal.stats.last_rejection_reason = format!(
                "Resoluutio ei täsmää: {}x{} vs ref {}x{}",
                incoming.width, incoming.height, ref_w, ref_h
            );
            internal.stats.status_message = format!("❌ Hylätty: {}", internal.stats.last_rejection_reason);
            return;
        }

        // 3. Star detection and alignment
        let align_enabled = internal.config.align_frames;
        let min_stars = internal.config.min_stars;
        let ref_stars = internal.reference_stars.clone();

        let target_stars = StarDetector::detect(&incoming, 60);
        let target_star_count = target_stars.len();
        internal.stats.detected_stars_last_frame = target_star_count;

        let transform = if align_enabled {
            if target_star_count < min_stars {
                internal.stats.rejected_count += 1;
                internal.stats.last_rejection_reason = format!(
                    "Liian vähän tähtiä tunnistettu ({}/{} kpl) - pilvisyys?",
                    target_star_count, min_stars
                );
                internal.stats.status_message = format!("❌ Hylätty: {}", internal.stats.last_rejection_reason);
                return;
            }

            match StarMatcher::match_stars(&ref_stars, &target_stars) {
                Some(t) => t,
                None => {
                    internal.stats.rejected_count += 1;
                    internal.stats.last_rejection_reason =
                        "Tähtikuvio ei täsmää referenssiin (seurantavirhe tai pilviä)".to_string();
                    internal.stats.status_message = format!("❌ Hylätty: {}", internal.stats.last_rejection_reason);
                    return;
                }
            }
        } else {
            Transform2D::identity()
        };

        // 4. Background median normalization against reference
        let ref_bg = internal.reference_image.as_ref().unwrap().channel_median_and_mad(0).0;
        let cur_bg = incoming.channel_median_and_mad(0).0;
        let bg_diff = ref_bg - cur_bg;

        // 5. Accumulate into master buffer (supports subpixel 2x Drizzle super-resolution)
        let out_w = ref_w * scale;
        let out_h = ref_h * scale;
        let total_pixels = out_w * out_h;
        let channels = ref_ch;

        let internal_mut = &mut *internal;
        let accum_sum = &mut internal_mut.accum_sum;
        let coverage_count = &mut internal_mut.coverage_count;

        Self::accumulate_frame_scaled(
            &incoming,
            &transform,
            scale,
            out_w,
            bg_diff,
            accum_sum,
            coverage_count,
        );

        // 6. Compute integrated live stacked image
        let mut stacked_data = vec![0.0f32; total_pixels * channels];
        stacked_data
            .par_chunks_exact_mut(out_w * channels)
            .zip(internal.accum_sum.par_chunks_exact(out_w * channels))
            .zip(internal.coverage_count.par_chunks_exact(out_w))
            .for_each(|((out_row, accum_row), cov_row)| {
                for x in 0..out_w {
                    let cov = cov_row[x].max(1) as f32;
                    let idx = x * channels;
                    for c in 0..channels {
                        out_row[idx + c] = accum_row[idx + c] / cov;
                    }
                }
            });

        let new_stacked_img = AstroImage {
            width: out_w,
            height: out_h,
            channels,
            data: stacked_data,
            metadata: crate::core::image::ImageMetadata {
                file_name: format!(
                    "LiveStack_{}_frames_{}x.fits",
                    internal.stats.stacked_count + 1,
                    scale
                ),
                exposure_time: Some(internal.stats.total_exposure_secs + sub_exp),
                ..Default::default()
            },
        };

        // Recompute Auto STF if enabled
        if internal.config.auto_stretch {
            internal.stf_params = StfParameters::compute_auto(&new_stacked_img, false);
        }

        internal.current_stacked = Some(Arc::new(new_stacked_img));
        internal.stats.stacked_count += 1;
        internal.stats.total_exposure_secs += sub_exp;
        internal.stats.snr_gain_factor = (internal.stats.stacked_count as f32).sqrt();
        internal.stats.drizzle_scale = scale;
        internal.stats.output_resolution = (out_w, out_h);
        internal.stats.status_message = format!(
            "✓ Pinottu ruutu #{} ({}) | SNR: {:.1}x | Tähdet: {} kpl{}",
            internal.stats.stacked_count,
            name,
            internal.stats.snr_gain_factor,
            target_star_count,
            if scale > 1 { format!(" [{}x Drizzle]", scale) } else { "".to_string() }
        );
    }

    /// Sub-pixel bilinear accumulation onto scaled (drizzle) canvas
    fn accumulate_frame_scaled(
        image: &AstroImage,
        transform: &Transform2D,
        scale: usize,
        out_w: usize,
        bg_diff: f32,
        accum_sum: &mut [f32],
        coverage_count: &mut [u32],
    ) {
        let inv_scale = 1.0 / (scale as f32);
        let inv_tf = transform.inverse();
        let src = &image.data;
        let in_w = image.width;
        let in_h = image.height;
        let channels = image.channels;

        accum_sum
            .par_chunks_exact_mut(out_w * channels)
            .zip(coverage_count.par_chunks_exact_mut(out_w))
            .enumerate()
            .for_each(|(y_out, (row_accum, row_cov))| {
                let ref_y = (y_out as f32 + 0.5) * inv_scale - 0.5;
                for x_out in 0..out_w {
                    let ref_x = (x_out as f32 + 0.5) * inv_scale - 0.5;
                    let (src_x, src_y) = inv_tf.apply(ref_x, ref_y);
                    let out_idx = x_out * channels;

                    if src_x >= 0.0
                        && src_x <= (in_w - 1) as f32
                        && src_y >= 0.0
                        && src_y <= (in_h - 1) as f32
                    {
                        let x0 = (src_x.floor() as usize).min(in_w - 1);
                        let y0 = (src_y.floor() as usize).min(in_h - 1);
                        let x1 = (x0 + 1).min(in_w - 1);
                        let y1 = (y0 + 1).min(in_h - 1);

                        let fx = src_x - x0 as f32;
                        let fy = src_y - y0 as f32;

                        let w00 = (1.0 - fx) * (1.0 - fy);
                        let w10 = fx * (1.0 - fy);
                        let w01 = (1.0 - fx) * fy;
                        let w11 = fx * fy;

                        let idx00 = (y0 * in_w + x0) * channels;
                        let idx10 = (y0 * in_w + x1) * channels;
                        let idx01 = (y1 * in_w + x0) * channels;
                        let idx11 = (y1 * in_w + x1) * channels;

                        let mut is_valid = false;
                        for c in 0..channels {
                            let s = w00 * src[idx00 + c]
                                + w10 * src[idx10 + c]
                                + w01 * src[idx01 + c]
                                + w11 * src[idx11 + c];
                            if s > 1e-4 {
                                is_valid = true;
                                break;
                            }
                        }

                        if is_valid {
                            row_cov[x_out] += 1;
                            for c in 0..channels {
                                let s = w00 * src[idx00 + c]
                                    + w10 * src[idx10 + c]
                                    + w01 * src[idx01 + c]
                                    + w11 * src[idx11 + c];
                                let adjusted = (s + bg_diff).max(0.0);
                                row_accum[out_idx + c] += adjusted;
                            }
                        }
                    }
                }
            });
    }

    /// Resets the live stack accumulator to empty
    pub fn reset(&self) {
        let mut internal = self.internal.lock().unwrap();
        internal.reference_image = None;
        internal.reference_stars.clear();
        internal.accum_sum.clear();
        internal.coverage_count.clear();
        internal.current_stacked = None;
        internal.processed_paths.clear();
        let scale = internal.config.drizzle_scale.clamp(1, 4);
        internal.stats = LiveStackStats {
            status_message: "Live-pino nollattu. Odottaa uutta referenssiruutua.".to_string(),
            drizzle_scale: scale,
            output_resolution: (0, 0),
            ..Default::default()
        };
    }

    /// Pause or resume live stacking
    pub fn set_running(&self, running: bool) {
        let mut internal = self.internal.lock().unwrap();
        internal.stats.is_running = running;
        if running {
            internal.stats.status_message = "Live-pinoaminen aktiivinen.".to_string();
        } else {
            internal.stats.status_message = "Live-pinoaminen tauolla.".to_string();
        }
    }

    /// Returns current snapshot of statistics
    pub fn get_stats(&self) -> LiveStackStats {
        self.internal.lock().unwrap().stats.clone()
    }

    /// Returns current config
    pub fn get_config(&self) -> LiveStackConfig {
        self.internal.lock().unwrap().config.clone()
    }

    /// Update configuration
    pub fn update_config<F>(&self, update_fn: F)
    where
        F: FnOnce(&mut LiveStackConfig),
    {
        let mut internal = self.internal.lock().unwrap();
        let old_scale = internal.config.drizzle_scale;
        update_fn(&mut internal.config);
        if internal.config.drizzle_scale != old_scale && internal.reference_image.is_some() {
            // Scale changed while stack was active; reset accumulator for new resolution
            internal.reference_image = None;
            internal.reference_stars.clear();
            internal.accum_sum.clear();
            internal.coverage_count.clear();
            internal.current_stacked = None;
            internal.processed_paths.clear();
            let scale = internal.config.drizzle_scale.clamp(1, 4);
            internal.stats.stacked_count = 0;
            internal.stats.total_exposure_secs = 0.0;
            internal.stats.snr_gain_factor = 1.0;
            internal.stats.drizzle_scale = scale;
            internal.stats.output_resolution = (0, 0);
            internal.stats.status_message = format!(
                "Drizzle-tila vaihdettu ({}x). Odottaa uutta referenssiruutua.",
                if scale > 1 { format!("{}x Drizzle", scale) } else { "1x".to_string() }
            );
        }
    }

    /// Returns the current stacked AstroImage, if any
    pub fn get_current_image(&self) -> Option<Arc<AstroImage>> {
        self.internal.lock().unwrap().current_stacked.clone()
    }

    /// Returns current STF parameters
    pub fn get_stf_params(&self) -> StfParameters {
        self.internal.lock().unwrap().stf_params.clone()
    }

    /// Scan and load existing frames from a folder into the live stack
    pub fn ingest_folder(&self, folder: &Path) -> usize {
        let mut found = 0;
        if let Ok(entries) = std::fs::read_dir(folder) {
            let mut paths: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
                        let l = ext.to_lowercase();
                        l == "fits" || l == "fit" || l == "fts" || l == "tiff" || l == "tif"
                    } else {
                        false
                    }
                })
                .collect();

            // Sort paths by file name / time
            paths.sort();

            for path in paths {
                self.feed_file(path);
                found += 1;
            }
        }
        found
    }

    /// Export current stacked image to 32-bit FITS file
    pub fn export_fits(&self, path: &Path) -> Result<(), String> {
        let img = self
            .get_current_image()
            .ok_or_else(|| "Ei pinottua kuvaa tallennettavaksi".to_string())?;
        crate::core::fits::FitsParser::write_file(&img, path)
    }

    /// Export current stacked image to PNG with auto-stretch
    pub fn export_png(&self, path: &Path, stretch: bool) -> Result<(), String> {
        let img = self
            .get_current_image()
            .ok_or_else(|| "Ei pinottua kuvaa tallennettavaksi".to_string())?;

        let rgb_bytes = if stretch {
            let stf = self.get_stf_params();
            let params = [
                (stf.channels[0].shadows, stf.channels[0].midtones, stf.channels[0].highlights),
                (stf.channels[1].shadows, stf.channels[1].midtones, stf.channels[1].highlights),
                (stf.channels[2].shadows, stf.channels[2].midtones, stf.channels[2].highlights),
            ];
            img.to_rgb8_stretched_channels(&params)
        } else {
            img.to_rgb8_stretched(0.0, 0.5, 1.0)
        };

        image::save_buffer(
            path,
            &rgb_bytes,
            img.width as u32,
            img.height as u32,
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|e| format!("PNG tallennusvirhe: {}", e))
    }
}

impl Drop for LiveStackEngine {
    fn drop(&mut self) {
        self.stop_signal.store(true, Ordering::SeqCst);
    }
}
