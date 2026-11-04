use crate::core::image::{AstroImage, ImageMetadata};
use crate::processing::alignment::{StarDetector, StarMatcher, Transform2D};
use rayon::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Configuration parameters for Mosaic Stitching
#[derive(Clone, Debug)]
pub struct MosaicStitchConfig {
    /// Width of the blending feather margin along panel boundaries (in pixels)
    pub feather_radius_px: f32,
    /// Equalize background median levels across all panels to prevent tile boundary jumps
    pub normalize_background: bool,
    /// Automatically crop outer transparent/empty borders
    pub auto_crop: bool,
    /// Number of brightest stars to detect per panel for overlap matching
    pub star_detection_count: usize,
}

impl Default for MosaicStitchConfig {
    fn default() -> Self {
        Self {
            feather_radius_px: 45.0,
            normalize_background: true,
            auto_crop: false,
            star_detection_count: 75,
        }
    }
}

/// Progress reporting event for Mosaic Stitching
#[derive(Clone, Debug, Default)]
pub struct MosaicStitchProgress {
    pub stage: String,
    pub detail: String,
    pub fraction: f32,
}

/// High-performance Astrophotography Mosaic Stitcher
pub struct MosaicStitcher;

impl MosaicStitcher {
    /// Stitches multiple overlapping astrophotography panels into a seamless wide-field master mosaic
    pub fn stitch(
        panels: &[Arc<AstroImage>],
        config: &MosaicStitchConfig,
        on_progress: Option<Box<dyn Fn(MosaicStitchProgress) + Send + Sync>>,
        cancel_token: Arc<AtomicBool>,
    ) -> Result<AstroImage, String> {
        if panels.is_empty() {
            return Err("Ei kuvapaneeleja mosaiikin yhdistämiseen.".to_string());
        }

        if panels.len() == 1 {
            return Ok((*panels[0]).clone());
        }

        let num_panels = panels.len();
        let channels = panels[0].channels;
        for (i, p) in panels.iter().enumerate() {
            if p.channels != channels {
                return Err(format!(
                    "Paneelien kanavamäärät eivät täsmää: Paneeli 1 on {}-kanavainen, Paneeli {} on {}-kanavainen.",
                    channels,
                    i + 1,
                    p.channels
                ));
            }
        }

        // 1. STAR DETECTION ACROSS ALL PANELS
        if let Some(cb) = &on_progress {
            cb(MosaicStitchProgress {
                stage: "Tähtien tunnistus (Star Detection)".to_string(),
                detail: format!("Tunnistetaan tähtiä {} paneelista...", num_panels),
                fraction: 0.05,
            });
        }

        let all_stars: Vec<_> = panels
            .par_iter()
            .enumerate()
            .map(|(_idx, p)| {
                if cancel_token.load(Ordering::SeqCst) {
                    return Vec::new();
                }
                StarDetector::detect(p, config.star_detection_count)
            })
            .collect();

        if cancel_token.load(Ordering::SeqCst) {
            return Err("Mosaiikin yhdistäminen peruutettu.".to_string());
        }

        // 2. GRAPH-BASED MULTI-PANEL REGISTRATION
        if let Some(cb) = &on_progress {
            cb(MosaicStitchProgress {
                stage: "Paneelien kohdistus (Registration)".to_string(),
                detail: "Etsitään paneelien välisiä tähtipareja ja päällekkäisyyksiä...".to_string(),
                fraction: 0.20,
            });
        }

        // transforms[i] maps panel i local coordinates into the reference panel 0 coordinate system
        let mut transforms_to_ref: Vec<Option<Transform2D>> = vec![None; num_panels];
        transforms_to_ref[0] = Some(Transform2D::identity());

        let mut registered_count = 1;

        // Iterate until all panels are registered through the overlap graph
        while registered_count < num_panels {
            if cancel_token.load(Ordering::SeqCst) {
                return Err("Mosaiikin yhdistäminen peruutettu.".to_string());
            }

            let mut matched_in_this_pass = false;

            for u in 0..num_panels {
                if transforms_to_ref[u].is_some() {
                    continue; // Already registered
                }

                // Try to match unregistered panel u against any already registered panel r
                for r in 0..num_panels {
                    if let Some(t_ref_from_r) = &transforms_to_ref[r] {
                        if let Some(t_r_from_u) = StarMatcher::match_stars_with_limit(
                            &all_stars[r],
                            &all_stars[u],
                            config.star_detection_count,
                            3,
                        ) {
                            let t_ref_from_u = t_ref_from_r.compose(&t_r_from_u);
                            transforms_to_ref[u] = Some(t_ref_from_u);
                            registered_count += 1;
                            matched_in_this_pass = true;

                            if let Some(cb) = &on_progress {
                                cb(MosaicStitchProgress {
                                    stage: "Paneelien kohdistus (Registration)".to_string(),
                                    detail: format!(
                                        "Kohdistettu paneeli {} / {} (kytketty paneeliin {})...",
                                        registered_count, num_panels, r + 1
                                    ),
                                    fraction: 0.20 + 0.25 * (registered_count as f32 / num_panels as f32),
                                });
                            }
                            break;
                        }
                    }
                }
            }

            if !matched_in_this_pass {
                // Find first unregistered panel
                let missing_idx = transforms_to_ref
                    .iter()
                    .position(|t| t.is_none())
                    .unwrap_or(0);
                return Err(format!(
                    "Paneelia {} ei voitu kohdistaa tähtien perusteella mihinkään muuhun paneeliin. Varmista, että vierekkäisissä ruuduissa on riittävästi päällekkäisyyttä (overlap) ja tunnistettavia tähtiä.",
                    missing_idx + 1
                ));
            }
        }

        // 3. COMPUTE GLOBAL CANVAS BOUNDING BOX
        if let Some(cb) = &on_progress {
            cb(MosaicStitchProgress {
                stage: "Kanvaan luonti (Canvas Projection)".to_string(),
                detail: "Lasketaan yhdistetyn mosaiikin kokonaiskoko...".to_string(),
                fraction: 0.50,
            });
        }

        let mut min_x = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_y = f32::NEG_INFINITY;

        for (i, p) in panels.iter().enumerate() {
            let tf = transforms_to_ref[i].as_ref().unwrap();
            let w = p.width as f32;
            let h = p.height as f32;
            let corners = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)];
            for (cx, cy) in corners {
                let (gx, gy) = tf.apply(cx, cy);
                min_x = min_x.min(gx);
                max_x = max_x.max(gx);
                min_y = min_y.min(gy);
                max_y = max_y.max(gy);
            }
        }

        let canvas_w = ((max_x - min_x).ceil() as usize).max(panels[0].width) + 2;
        let canvas_h = ((max_y - min_y).ceil() as usize).max(panels[0].height) + 2;

        // Shift transform to map min_x, min_y to (1.0, 1.0)
        let shift_tf = Transform2D {
            theta: 0.0,
            tx: -min_x + 1.0,
            ty: -min_y + 1.0,
        };

        // For each panel, compute transform from canvas pixel (X, Y) to panel pixel (u, v)
        let inv_transforms: Vec<Transform2D> = transforms_to_ref
            .iter()
            .map(|opt_tf| {
                let canvas_from_panel = shift_tf.compose(opt_tf.as_ref().unwrap());
                canvas_from_panel.inverse()
            })
            .collect();

        // 4. BACKGROUND PHOTOMETRIC NORMALIZATION
        let bg_offsets: Vec<Vec<f32>> = if config.normalize_background {
            let ref_medians: Vec<f32> = (0..channels)
                .map(|c| panels[0].channel_median_and_mad(c).0)
                .collect();

            panels
                .iter()
                .map(|p| {
                    (0..channels)
                        .map(|c| {
                            let (m, _) = p.channel_median_and_mad(c);
                            ref_medians[c] - m
                        })
                        .collect()
                })
                .collect()
        } else {
            vec![vec![0.0f32; channels]; num_panels]
        };

        // 5. PARALLEL FEATHERED MOSAIC BLENDING
        if let Some(cb) = &on_progress {
            cb(MosaicStitchProgress {
                stage: "Saumaton yhdistäminen (Feathering & Blending)".to_string(),
                detail: format!(
                    "Renderoinnissa {} × {} px jättimosaiikki ({} Mpx)...",
                    canvas_w,
                    canvas_h,
                    (canvas_w * canvas_h) / 1_000_000
                ),
                fraction: 0.60,
            });
        }

        let mut output_data = vec![0.0f32; canvas_w * canvas_h * channels];
        let feather = config.feather_radius_px.max(5.0);

        output_data
            .par_chunks_exact_mut(canvas_w * channels)
            .enumerate()
            .for_each(|(y_out, row_out)| {
                let py = y_out as f32 + 0.5;

                for x_out in 0..canvas_w {
                    let px = x_out as f32 + 0.5;

                    let mut total_weight = 0.0f32;
                    let mut accum = [0.0f32; 4];

                    for (p_idx, p) in panels.iter().enumerate() {
                        let inv_tf = &inv_transforms[p_idx];
                        let (u, v) = inv_tf.apply(px, py);

                        let pw = p.width as f32;
                        let ph = p.height as f32;

                        if u >= 0.0 && u < pw - 1.0 && v >= 0.0 && v < ph - 1.0 {
                            // Distance to closest panel border
                            let d_edge = u.min(pw - 1.0 - u).min(v).min(ph - 1.0 - v);
                            if d_edge > 0.0 {
                                // Smooth cubic hermite feathering weight
                                let t = (d_edge / feather).min(1.0);
                                let w = t * t * (3.0 - 2.0 * t);

                                if w > 0.0001 {
                                    // Bilinear sampling
                                    let x0 = u.floor() as usize;
                                    let y0 = v.floor() as usize;
                                    let x1 = x0 + 1;
                                    let y1 = y0 + 1;

                                    let fx = u - x0 as f32;
                                    let fy = v - y0 as f32;

                                    let w00 = (1.0 - fx) * (1.0 - fy);
                                    let w10 = fx * (1.0 - fy);
                                    let w01 = (1.0 - fx) * fy;
                                    let w11 = fx * fy;

                                    let idx00 = (y0 * p.width + x0) * channels;
                                    let idx10 = (y0 * p.width + x1) * channels;
                                    let idx01 = (y1 * p.width + x0) * channels;
                                    let idx11 = (y1 * p.width + x1) * channels;

                                    let bg_adj = &bg_offsets[p_idx];

                                    for c in 0..channels {
                                        let sample = w00 * p.data[idx00 + c]
                                            + w10 * p.data[idx10 + c]
                                            + w01 * p.data[idx01 + c]
                                            + w11 * p.data[idx11 + c];
                                        accum[c] += w * (sample + bg_adj[c]).max(0.0);
                                    }
                                    total_weight += w;
                                }
                            }
                        }
                    }

                    let out_idx = x_out * channels;
                    if total_weight > 0.0001 {
                        let inv_w = 1.0 / total_weight;
                        for c in 0..channels {
                            row_out[out_idx + c] = (accum[c] * inv_w).max(0.0);
                        }
                    } else {
                        for c in 0..channels {
                            row_out[out_idx + c] = 0.0;
                        }
                    }
                }
            });

        if cancel_token.load(Ordering::SeqCst) {
            return Err("Mosaiikin yhdistäminen peruutettu.".to_string());
        }

        // 6. BUILD FINAL ASTROIMAGE
        let mut final_image = AstroImage {
            width: canvas_w,
            height: canvas_h,
            channels,
            data: output_data,
            metadata: ImageMetadata {
                file_name: format!("Master_Mosaic_{}x{}.fits", canvas_w, canvas_h),
                file_path: String::new(),
                filter_name: panels[0].metadata.filter_name.clone(),
                exposure_time: panels.iter().filter_map(|p| p.metadata.exposure_time).reduce(|a, b| a + b),
                gain: panels[0].metadata.gain,
                iso: panels[0].metadata.iso,
                target: panels[0].metadata.target.clone().or_else(|| Some("Mosaiikki".to_string())),
                telescope: panels[0].metadata.telescope.clone(),
                camera: panels[0].metadata.camera.clone(),
                date_obs: panels[0].metadata.date_obs.clone(),
                bitpix: Some(-32),
                bayer_pattern: None,
            },
        };

        if config.auto_crop {
            final_image = final_image.auto_crop().unwrap_or(final_image);
        }

        if let Some(cb) = &on_progress {
            cb(MosaicStitchProgress {
                stage: "Valmis".to_string(),
                detail: format!("Mosaiikki valmis! Resoluutio: {} × {} px", final_image.width, final_image.height),
                fraction: 1.0,
            });
        }

        Ok(final_image)
    }
}
