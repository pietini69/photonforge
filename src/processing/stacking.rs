use super::alignment::{StarDetector, StarMatcher, Transform2D};
use crate::core::image::AstroImage;
use rayon::prelude::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StackingMethod {
    Average,
    Median,
    MinMax,
    SigmaClipping,
}

#[derive(Clone, Debug)]
pub struct StackingConfig {
    pub method: StackingMethod,
    pub sigma_low: f32,
    pub sigma_high: f32,
    pub min_max_low: usize,
    pub min_max_high: usize,
    pub align_stars: bool,
    pub normalize_background: bool,
    pub min_coverage_ratio: f32,
    pub auto_crop_borders: bool,
    pub drizzle_scale: usize,
    pub auto_stretch_after_stack: bool,
}

impl Default for StackingConfig {
    fn default() -> Self {
        Self {
            method: StackingMethod::SigmaClipping,
            sigma_low: 2.5,
            sigma_high: 3.0,
            min_max_low: 1,
            min_max_high: 1,
            align_stars: true,
            normalize_background: true,
            min_coverage_ratio: 0.10,
            auto_crop_borders: true,
            drizzle_scale: 1,
            auto_stretch_after_stack: false, // Default is FALSE: newly stacked images are pure LINEAR
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct StackingProgress {
    pub stage: String,
    pub detail: String,
    pub fraction: f32,
}

pub struct Stacker;

impl Stacker {
    /// Synchronous convenience wrapper calling stack_with_progress without callback
    #[allow(dead_code)]
    pub fn stack(images: &[Arc<AstroImage>], config: &StackingConfig) -> Result<AstroImage, String> {
        let cancel = Arc::new(AtomicBool::new(false));
        Self::stack_with_progress(images, config, None, cancel)
    }

    /// Asynchronous / cancellable streaming band stacker with real-time progress reporting.
    /// Uses low-memory streaming bands to prevent out-of-memory and disk paging on 200+ frames.
    pub fn stack_with_progress(
        images: &[Arc<AstroImage>],
        config: &StackingConfig,
        on_progress: Option<Box<dyn Fn(StackingProgress) + Send + Sync>>,
        cancel_token: Arc<AtomicBool>,
    ) -> Result<AstroImage, String> {
        if images.is_empty() {
            return Err("No images provided for stacking".to_string());
        }

        let ref_img = &images[0];
        let width = ref_img.width;
        let height = ref_img.height;
        let channels = ref_img.channels;
        let num_images = images.len();

        // Verify dimensions
        for (i, img) in images.iter().enumerate() {
            if img.width != width || img.height != height || img.channels != channels {
                return Err(format!(
                    "Image #{} dimension mismatch: {}x{}x{} vs {}x{}x{}",
                    i + 1,
                    img.width,
                    img.height,
                    img.channels,
                    width,
                    height,
                    channels
                ));
            }
        }

        let scale = config.drizzle_scale.clamp(1, 4);
        let out_width = width * scale;
        let out_height = height * scale;
        let total_pixels = out_width * out_height * channels;

        if cancel_token.load(Ordering::SeqCst) {
            return Err("Pinoaminen peruutettu käyttäjän toimesta.".to_string());
        }

        // 1. Star Alignment across all frames in parallel (computed on 1x resolution)
        let transforms: Vec<Transform2D> = if config.align_stars && num_images >= 2 {
            if let Some(cb) = &on_progress {
                cb(StackingProgress {
                    stage: "Tähtien kohdistus (Star Alignment)".to_string(),
                    detail: "Etsitään referenssikuvan tähdet...".to_string(),
                    fraction: 0.02,
                });
            }

            let ref_stars = StarDetector::detect(ref_img, 60);
            let completed_counter = AtomicUsize::new(0);

            images
                .par_iter()
                .enumerate()
                .map(|(idx, img)| {
                    if cancel_token.load(Ordering::SeqCst) {
                        return Transform2D::identity();
                    }

                    let tf = if idx == 0 {
                        Transform2D::identity()
                    } else {
                        let target_stars = StarDetector::detect(img, 60);
                        StarMatcher::match_stars(&ref_stars, &target_stars).unwrap_or_else(Transform2D::identity)
                    };

                    let done = completed_counter.fetch_add(1, Ordering::SeqCst) + 1;
                    if let Some(cb) = &on_progress {
                        cb(StackingProgress {
                            stage: "Tähtien kohdistus (Star Alignment)".to_string(),
                            detail: format!("Kohdistetaan kuva {} / {}...", done, num_images),
                            fraction: 0.02 + 0.38 * (done as f32 / num_images as f32),
                        });
                    }

                    tf
                })
                .collect()
        } else {
            vec![Transform2D::identity(); num_images]
        };

        if cancel_token.load(Ordering::SeqCst) {
            return Err("Pinoaminen peruutettu käyttäjän toimesta.".to_string());
        }

        // 2. Background Normalization deltas (computed on 1x resolution)
        let deltas: Vec<Vec<f32>> = if config.normalize_background && num_images >= 2 {
            if let Some(cb) = &on_progress {
                cb(StackingProgress {
                    stage: "Taustan normalisointi (Background Normalization)".to_string(),
                    detail: "Lasketaan taustan valoisuustasoja...".to_string(),
                    fraction: 0.40,
                });
            }

            let mut ref_medians = vec![0.0f32; channels];
            for c in 0..channels {
                let (med, _) = ref_img.channel_median_and_mad(c);
                ref_medians[c] = med;
            }

            let completed_counter = AtomicUsize::new(0);

            images
                .par_iter()
                .enumerate()
                .map(|(idx, img)| {
                    let mut d = vec![0.0f32; channels];
                    if idx > 0 {
                        for c in 0..channels {
                            let (tgt_med, _) = img.channel_median_and_mad(c);
                            d[c] = tgt_med - ref_medians[c];
                        }
                    }
                    let done = completed_counter.fetch_add(1, Ordering::SeqCst) + 1;
                    if let Some(cb) = &on_progress {
                        cb(StackingProgress {
                            stage: "Taustan normalisointi (Background Normalization)".to_string(),
                            detail: format!("Normalisoidaan kuva {} / {}...", done, num_images),
                            fraction: 0.40 + 0.05 * (done as f32 / num_images as f32),
                        });
                    }
                    d
                })
                .collect()
        } else {
            vec![vec![0.0f32; channels]; num_images]
        };

        if cancel_token.load(Ordering::SeqCst) {
            return Err("Pinoaminen peruutettu käyttäjän toimesta.".to_string());
        }

        // 3. Streaming Band Warping & Integration
        // Memory safety: dynamically limit temporary band memory to ~512 MB to avoid swap on hundreds of 2x frames
        let bytes_per_row = out_width * channels * std::mem::size_of::<f32>() * num_images;
        let target_buf_bytes = 512 * 1024 * 1024;
        let band_height = (target_buf_bytes / bytes_per_row.max(1)).clamp(32, 256).min(out_height);
        let num_bands = (out_height + band_height - 1) / band_height;

        let mut output_data = vec![0.0f32; total_pixels];
        let mut band_buffers: Vec<Vec<f32>> = (0..num_images)
            .map(|_| vec![0.0f32; band_height * out_width * channels])
            .collect();

        let min_required = if num_images >= 2 && config.min_coverage_ratio > 0.0 {
            ((num_images as f32) * config.min_coverage_ratio).ceil() as usize
        } else {
            1
        };

        let method_name = match config.method {
            StackingMethod::SigmaClipping => "Sigma Clipping",
            StackingMethod::Average => "Keskiarvo",
            StackingMethod::Median => "Mediaani",
            StackingMethod::MinMax => "Min/Max",
        };

        for band_idx in 0..num_bands {
            if cancel_token.load(Ordering::SeqCst) {
                return Err("Pinoaminen peruutettu käyttäjän toimesta.".to_string());
            }

            let y_start = band_idx * band_height;
            let y_end = ((band_idx + 1) * band_height).min(out_height);
            let current_band_rows = y_end - y_start;
            let current_band_len = current_band_rows * out_width * channels;

            let pct = (band_idx as f32 / num_bands as f32) * 100.0;
            if let Some(cb) = &on_progress {
                cb(StackingProgress {
                    stage: format!("Pinotaan pikseleitä ({})", method_name),
                    detail: format!(
                        "Kaistale {} / {} ({:.0}%) - {} kuvaa{}",
                        band_idx + 1,
                        num_bands,
                        pct,
                        num_images,
                        if scale > 1 { format!(" ({}x Drizzle)", scale) } else { "".to_string() }
                    ),
                    fraction: 0.45 + 0.50 * (band_idx as f32 / num_bands as f32),
                });
            }

            // Step A: Warp this band for all images in parallel
            band_buffers
                .par_iter_mut()
                .enumerate()
                .for_each(|(img_idx, buf)| {
                    let img = &images[img_idx];
                    let tf = &transforms[img_idx];
                    let delta = &deltas[img_idx];
                    let out_slice = &mut buf[..current_band_len];
                    Self::warp_band(
                        img,
                        tf,
                        scale,
                        y_start,
                        y_end,
                        out_width,
                        channels,
                        delta,
                        out_slice,
                    );
                });

            // Step B: Integrate this band into output_data
            let out_band_slice = &mut output_data[y_start * out_width * channels..y_end * out_width * channels];
            Self::integrate_band(
                out_band_slice,
                &band_buffers,
                current_band_rows,
                out_width,
                channels,
                min_required,
                config,
            );
        }

        if cancel_token.load(Ordering::SeqCst) {
            return Err("Pinoaminen peruutettu käyttäjän toimesta.".to_string());
        }

        let stacked_img = AstroImage {
            width: out_width,
            height: out_height,
            channels,
            data: output_data,
            metadata: ref_img.metadata.clone(),
        };

        if config.auto_crop_borders {
            if let Some(cb) = &on_progress {
                cb(StackingProgress {
                    stage: "Reunojen rajaus (Auto-Crop)".to_string(),
                    detail: "Leikataan kohdistuksen mustat reunat pois...".to_string(),
                    fraction: 0.98,
                });
            }
            Ok(stacked_img.auto_crop().unwrap_or(stacked_img))
        } else {
            Ok(stacked_img)
        }
    }

    fn warp_band(
        image: &AstroImage,
        transform: &Transform2D,
        scale: usize,
        y_start: usize,
        y_end: usize,
        out_width: usize,
        channels: usize,
        delta: &[f32],
        out_slice: &mut [f32],
    ) {
        let inv_scale = 1.0 / (scale as f32);
        let inv_tf = transform.inverse();
        let src = &image.data;
        let in_w = image.width;
        let in_h = image.height;

        for y_out in y_start..y_end {
            let row_offset = (y_out - y_start) * out_width * channels;
            let row_out = &mut out_slice[row_offset..row_offset + out_width * channels];

            let ref_y = (y_out as f32 + 0.5) * inv_scale - 0.5;

            for x_out in 0..out_width {
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

                    for c in 0..channels {
                        let mut val = w00 * src[idx00 + c]
                            + w10 * src[idx10 + c]
                            + w01 * src[idx01 + c]
                            + w11 * src[idx11 + c];
                        if delta[c] != 0.0 && val > 0.0 {
                            val = (val - delta[c]).clamp(1e-7, 1.0);
                        }
                        row_out[out_idx + c] = val;
                    }
                } else {
                    for c in 0..channels {
                        row_out[out_idx + c] = 0.0;
                    }
                }
            }
        }
    }

    fn integrate_band(
        out_band: &mut [f32],
        band_buffers: &[Vec<f32>],
        _band_rows: usize,
        _out_width: usize,
        channels: usize,
        min_required: usize,
        config: &StackingConfig,
    ) {
        let num_images = band_buffers.len();

        out_band
            .par_chunks_exact_mut(channels)
            .enumerate()
            .for_each(|(pixel_idx, out_pixel)| {
                let offset = pixel_idx * channels;

                // Check frame coverage (partial overlap rejection)
                let mut valid_count = 0;
                for img_idx in 0..num_images {
                    if band_buffers[img_idx][offset] > 0.0 {
                        valid_count += 1;
                    }
                }

                if valid_count < min_required {
                    for c in 0..channels {
                        out_pixel[c] = 0.0;
                    }
                    return;
                }

                let mut raw_values = Vec::with_capacity(valid_count);

                for c in 0..channels {
                    raw_values.clear();
                    for img_idx in 0..num_images {
                        let val = band_buffers[img_idx][offset + c];
                        if val > 0.0 {
                            raw_values.push(val);
                        }
                    }

                    let mut pixel_values = raw_values.clone();
                    let count = pixel_values.len();
                    if count == 0 {
                        out_pixel[c] = 0.0;
                        continue;
                    }

                    let stacked_val = match config.method {
                        StackingMethod::Average => {
                            let sum: f32 = pixel_values.iter().sum();
                            sum / count as f32
                        }
                        StackingMethod::Median => {
                            pixel_values.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                            pixel_values[count / 2]
                        }
                        StackingMethod::MinMax => {
                            if count <= (config.min_max_low + config.min_max_high) {
                                let sum: f32 = pixel_values.iter().sum();
                                sum / count as f32
                            } else {
                                pixel_values.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                                let keep_start = config.min_max_low;
                                let keep_end = count - config.min_max_high;
                                let slice = &pixel_values[keep_start..keep_end];
                                slice.iter().sum::<f32>() / slice.len() as f32
                            }
                        }
                        StackingMethod::SigmaClipping => {
                            if count < 4 {
                                pixel_values.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                                pixel_values[count / 2]
                            } else {
                                let sum: f32 = pixel_values.iter().sum();
                                let mean = sum / count as f32;

                                let variance: f32 = pixel_values
                                    .iter()
                                    .map(|&v| {
                                        let diff = v - mean;
                                        diff * diff
                                    })
                                    .sum::<f32>()
                                    / count as f32;

                                let std_dev = variance.sqrt().max(1e-6);
                                let low_threshold = mean - config.sigma_low * std_dev;
                                let high_threshold = mean + config.sigma_high * std_dev;

                                let filtered: Vec<f32> = pixel_values
                                    .into_iter()
                                    .filter(|&v| v >= low_threshold && v <= high_threshold)
                                    .collect();

                                if filtered.is_empty() {
                                    mean
                                } else {
                                    let sum: f32 = filtered.iter().sum();
                                    sum / filtered.len() as f32
                                }
                            }
                        }
                    };

                    out_pixel[c] = stacked_val.clamp(0.0, 1.0);
                }
            });
    }
}
