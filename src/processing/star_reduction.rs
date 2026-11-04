use crate::core::image::AstroImage;
use rayon::prelude::*;

pub struct StarReduction;

impl StarReduction {
    /// Reduces the apparent size and brightness of stars while preserving nebulosity and background.
    ///
    /// Uses morphological top-hat star detection to isolate stars, followed by masked
    /// morphological erosion to shrink bloated star halos into sharp, tiny points.
    pub fn reduce_stars(
        image: &AstroImage,
        amount: f32,       // 0.0 ..= 1.0 (e.g. 0.5)
        radius: usize,     // 1 for 3x3, 2 for 5x5
        iterations: usize, // 1 or 2
    ) -> Result<AstroImage, String> {
        if amount <= 0.0 {
            return Ok(image.clone());
        }

        let width = image.width;
        let height = image.height;
        let channels = image.channels;
        let amt = amount.clamp(0.0, 1.0);
        let rad = radius.clamp(1, 3);
        let iters = iterations.clamp(1, 3);

        let mut current_img = image.clone();

        for _ in 0..iters {
            // 1. Compute luminance
            let mut lum = vec![0.0f32; width * height];
            lum.par_chunks_mut(width)
                .enumerate()
                .for_each(|(y, row_lum)| {
                    for x in 0..width {
                        let idx = (y * width + x) * channels;
                        if channels == 1 {
                            row_lum[x] = current_img.data[idx];
                        } else {
                            let r = current_img.data[idx];
                            let g = current_img.data[idx + 1];
                            let b = current_img.data[idx + 2];
                            row_lum[x] = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                        }
                    }
                });

            // 2. Morphological Opening (Erosion followed by Dilation) to extract background/nebula
            // Structuring element radius for star detection: rad + 2 (typically 3 or 4)
            let open_rad = rad + 2;
            let eroded_lum = min_filter_2d(&lum, width, height, open_rad);
            let opened_lum = max_filter_2d(&eroded_lum, width, height, open_rad);

            // 3. Top-Hat Transform: StarSignal = max(0, Lum - OpenedLum)
            // Stars are the high-frequency peaks above the opened surface
            let mut star_mask = vec![0.0f32; width * height];
            star_mask
                .par_chunks_mut(width)
                .enumerate()
                .for_each(|(y, row_mask)| {
                    for x in 0..width {
                        let idx = y * width + x;
                        let star_peak = (lum[idx] - opened_lum[idx]).max(0.0);
                        // Scale and soft clamp
                        let weight = (star_peak * 12.0).clamp(0.0, 1.0);
                        row_mask[x] = weight;
                    }
                });

            // Feather / blur the star mask with a 3x3 box blur to avoid sharp halo boundaries
            let smooth_mask = blur_mask(&star_mask, width, height);

            // 4. Morphological Erosion on image channels
            let mut eroded_img_data = vec![0.0f32; width * height * channels];
            for c in 0..channels {
                let mut ch_plane = vec![0.0f32; width * height];
                for i in 0..(width * height) {
                    ch_plane[i] = current_img.data[i * channels + c];
                }

                let eroded_plane = min_filter_2d(&ch_plane, width, height, rad);
                for i in 0..(width * height) {
                    eroded_img_data[i * channels + c] = eroded_plane[i];
                }
            }

            // 5. Blend: Original * (1 - amt * mask) + Eroded * (amt * mask)
            current_img
                .data
                .par_chunks_exact_mut(channels)
                .enumerate()
                .for_each(|(i, pixel)| {
                    let m = smooth_mask[i] * amt;
                    if m > 1e-4 {
                        let inv_m = 1.0 - m;
                        for c in 0..channels {
                            let orig = pixel[c];
                            let eroded = eroded_img_data[i * channels + c];
                            pixel[c] = orig * inv_m + eroded * m;
                        }
                    }
                });
        }

        Ok(current_img)
    }
}

/// 2D Minimum filter (Erosion)
fn min_filter_2d(src: &[f32], width: usize, height: usize, radius: usize) -> Vec<f32> {
    let mut temp = vec![0.0f32; width * height];
    let r = radius as isize;

    // Horizontal pass
    temp.par_chunks_mut(width).enumerate().for_each(|(y, row_out)| {
        let src_row = &src[y * width..(y + 1) * width];
        for x in 0..width {
            let mut min_val = src_row[x];
            let x_start = (x as isize - r).max(0) as usize;
            let x_end = ((x as isize + r).min(width as isize - 1)) as usize;
            for kx in x_start..=x_end {
                let v = src_row[kx];
                if v < min_val {
                    min_val = v;
                }
            }
            row_out[x] = min_val;
        }
    });

    let mut out = vec![0.0f32; width * height];
    // Vertical pass
    out.par_chunks_mut(width).enumerate().for_each(|(y, row_out)| {
        let y_start = (y as isize - r).max(0) as usize;
        let y_end = ((y as isize + r).min(height as isize - 1)) as usize;

        for x in 0..width {
            let mut min_val = temp[y * width + x];
            for ky in y_start..=y_end {
                let v = temp[ky * width + x];
                if v < min_val {
                    min_val = v;
                }
            }
            row_out[x] = min_val;
        }
    });

    out
}

/// 2D Maximum filter (Dilation)
fn max_filter_2d(src: &[f32], width: usize, height: usize, radius: usize) -> Vec<f32> {
    let mut temp = vec![0.0f32; width * height];
    let r = radius as isize;

    // Horizontal pass
    temp.par_chunks_mut(width).enumerate().for_each(|(y, row_out)| {
        let src_row = &src[y * width..(y + 1) * width];
        for x in 0..width {
            let mut max_val = src_row[x];
            let x_start = (x as isize - r).max(0) as usize;
            let x_end = ((x as isize + r).min(width as isize - 1)) as usize;
            for kx in x_start..=x_end {
                let v = src_row[kx];
                if v > max_val {
                    max_val = v;
                }
            }
            row_out[x] = max_val;
        }
    });

    let mut out = vec![0.0f32; width * height];
    // Vertical pass
    out.par_chunks_mut(width).enumerate().for_each(|(y, row_out)| {
        let y_start = (y as isize - r).max(0) as usize;
        let y_end = ((y as isize + r).min(height as isize - 1)) as usize;

        for x in 0..width {
            let mut max_val = temp[y * width + x];
            for ky in y_start..=y_end {
                let v = temp[ky * width + x];
                if v > max_val {
                    max_val = v;
                }
            }
            row_out[x] = max_val;
        }
    });

    out
}

/// Smooth mask with a 3x3 separable box blur
fn blur_mask(mask: &[f32], width: usize, height: usize) -> Vec<f32> {
    let mut temp = vec![0.0f32; width * height];

    temp.par_chunks_mut(width).enumerate().for_each(|(y, row_out)| {
        let src_row = &mask[y * width..(y + 1) * width];
        for x in 0..width {
            let x_prev = x.saturating_sub(1);
            let x_next = (x + 1).min(width - 1);
            row_out[x] = (src_row[x_prev] + src_row[x] + src_row[x_next]) / 3.0;
        }
    });

    let mut out = vec![0.0f32; width * height];
    out.par_chunks_mut(width).enumerate().for_each(|(y, row_out)| {
        let y_prev = y.saturating_sub(1);
        let y_next = (y + 1).min(height - 1);
        for x in 0..width {
            let v_prev = temp[y_prev * width + x];
            let v_curr = temp[y * width + x];
            let v_next = temp[y_next * width + x];
            row_out[x] = (v_prev + v_curr + v_next) / 3.0;
        }
    });

    out
}
