use crate::core::image::AstroImage;
use rayon::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ExtractionMode {
    Subtraction,
    Division,
}

pub struct BackgroundExtractor;

impl BackgroundExtractor {
    #[allow(dead_code)]
    pub fn extract(
        image: &AstroImage,
        degree: usize, // 1 to 4
        mode: ExtractionMode,
    ) -> Result<AstroImage, String> {
        Self::extract_with_params(image, degree, mode, 3.5)
    }

    pub fn extract_with_params(
        image: &AstroImage,
        degree: usize, // 1 to 4
        mode: ExtractionMode,
        deviation_factor: f32, // Standard deviation / MAD tolerance (e.g. 2.0 to 5.0)
    ) -> Result<AstroImage, String> {
        let width = image.width;
        let height = image.height;
        let channels = image.channels;

        // Sample background points on a coarse grid, excluding high-variance stars
        let grid_size = 32;
        let step_x = (width / grid_size).max(4);
        let step_y = (height / grid_size).max(4);

        let mut out_image = image.clone();

        // Compute medians for all channels
        let mut ch_medians = vec![0.0f32; channels];
        for c in 0..channels {
            let (med, _) = image.channel_median_and_mad(c);
            ch_medians[c] = med;
        }
        // Subtraction mode equalizes background to a common baseline to neutralize light pollution
        let common_target_bg = ch_medians.iter().copied().fold(f32::INFINITY, f32::min);

        let dev = deviation_factor.clamp(1.0, 8.0);

        for c in 0..channels {
            let (median, mad) = image.channel_median_and_mad(c);
            let upper_threshold = median + dev * mad;
            let lower_threshold = (median - 2.5 * mad).max(0.0);

            // Collect background sample points (x_norm, y_norm, val)
            let mut sample_x = Vec::new();
            let mut sample_y = Vec::new();
            let mut sample_v = Vec::new();

            for gy in (step_y / 2..height).step_by(step_y) {
                let y_norm = gy as f64 / height as f64;
                for gx in (step_x / 2..width).step_by(step_x) {
                    let x_norm = gx as f64 / width as f64;
                    let idx = (gy * width + gx) * channels + c;
                    let val = image.data[idx];

                    // Only take background (avoid bright stars, hot pixels, and zero-padded borders)
                    if val > 1e-5 && val >= lower_threshold && val <= upper_threshold {
                        sample_x.push(x_norm);
                        sample_y.push(y_norm);
                        sample_v.push(val as f64);
                    }
                }
            }

            if sample_x.len() < 10 {
                continue; // Not enough samples to fit background
            }

            let coeffs = fit_2d_polynomial(&sample_x, &sample_y, &sample_v, degree)?;

            // Apply correction to channel in parallel
            let w_f64 = width as f64;
            let h_f64 = height as f64;
            let ch_median = ch_medians[c];

            out_image
                .data
                .par_chunks_exact_mut(channels)
                .enumerate()
                .for_each(|(pixel_idx, pixel)| {
                    let orig_val = pixel[c];
                    if orig_val <= 0.0 {
                        pixel[c] = 0.0;
                        return;
                    }

                    let x = (pixel_idx % width) as f64 / w_f64;
                    let y = (pixel_idx / width) as f64 / h_f64;

                    let bg_model = eval_2d_polynomial(&coeffs, x, y, degree) as f32;

                    let corrected = match mode {
                        ExtractionMode::Subtraction => orig_val - bg_model + common_target_bg,
                        ExtractionMode::Division => {
                            if bg_model > 1e-4 {
                                (orig_val / bg_model) * ch_median
                            } else {
                                orig_val
                            }
                        }
                    };

                    pixel[c] = corrected.clamp(0.0, 1.0);
                });
        }

        Ok(out_image)
    }
}

fn compute_basis(x: f64, y: f64, degree: usize) -> Vec<f64> {
    match degree {
        1 => vec![1.0, x, y],
        2 => vec![1.0, x, y, x * x, x * y, y * y],
        3 => vec![
            1.0,
            x, y,
            x * x, x * y, y * y,
            x * x * x, x * x * y, x * y * y, y * y * y,
        ],
        _ => vec![
            1.0,
            x, y,
            x * x, x * y, y * y,
            x * x * x, x * x * y, x * y * y, y * y * y,
            x * x * x * x, x * x * x * y, x * x * y * y, x * y * y * y, y * y * y * y,
        ],
    }
}

fn fit_2d_polynomial(
    xs: &[f64],
    ys: &[f64],
    zs: &[f64],
    degree: usize,
) -> Result<Vec<f64>, String> {
    let num_terms = match degree {
        1 => 3,
        2 => 6,
        3 => 10,
        _ => 15,
    };
    let n = xs.len();

    // Normal equations A^T * A * c = A^T * z
    let mut ata = vec![vec![0.0f64; num_terms]; num_terms];
    let mut atz = vec![0.0f64; num_terms];

    for i in 0..n {
        let x = xs[i];
        let y = ys[i];
        let z = zs[i];

        let basis = compute_basis(x, y, degree);

        for r in 0..num_terms {
            atz[r] += basis[r] * z;
            for c in 0..num_terms {
                ata[r][c] += basis[r] * basis[c];
            }
        }
    }

    // Solve small linear system with Gaussian elimination
    solve_linear_system(&mut ata, &mut atz)
}

fn eval_2d_polynomial(coeffs: &[f64], x: f64, y: f64, degree: usize) -> f64 {
    let basis = compute_basis(x, y, degree);
    let mut sum = 0.0;
    for (i, &c) in coeffs.iter().enumerate() {
        if i < basis.len() {
            sum += c * basis[i];
        }
    }
    sum
}

fn solve_linear_system(a: &mut [Vec<f64>], b: &mut [f64]) -> Result<Vec<f64>, String> {
    let n = b.len();

    for i in 0..n {
        // Find pivot
        let mut max_row = i;
        let mut max_val = a[i][i].abs();
        for r in i + 1..n {
            if a[r][i].abs() > max_val {
                max_val = a[r][i].abs();
                max_row = r;
            }
        }

        if max_val < 1e-12 {
            return Err("Singular matrix in polynomial fitting".to_string());
        }

        a.swap(i, max_row);
        b.swap(i, max_row);

        // Normalize row i
        let pivot = a[i][i];
        for c in i..n {
            a[i][c] /= pivot;
        }
        b[i] /= pivot;

        // Eliminate column i
        for r in 0..n {
            if r != i {
                let factor = a[r][i];
                for c in i..n {
                    a[r][c] -= factor * a[i][c];
                }
                b[r] -= factor * b[i];
            }
        }
    }

    Ok(b.to_vec())
}
