use crate::core::image::AstroImage;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SharpeningMethod {
    /// Classical Unsharp Masking with Gaussian filter
    UnsharpMask,
    /// Multiscale À Trous Wavelet detail enhancement
    Wavelet,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SharpeningConfig {
    pub method: SharpeningMethod,

    // Unsharp Mask parameters
    pub usm_radius: f32,    // Sigma: 0.5 .. 8.0, default 1.5
    pub usm_amount: f32,    // 0.1 .. 3.0, default 1.0
    pub usm_threshold: f32, // Noise floor gate: 0.0 .. 0.05, default 0.005

    // Wavelet parameters (Scale 1: 1px, Scale 2: 2px, Scale 3: 4px)
    pub wavelet_scale1: f32,          // Fine details (stars/craters): 0.0 .. 3.0, default 1.2
    pub wavelet_scale2: f32,          // Mid details (dust lanes/knots): 0.0 .. 3.0, default 1.5
    pub wavelet_scale3: f32,          // Broad details (nebula arcs): 0.0 .. 3.0, default 1.2
    pub wavelet_noise_threshold: f32, // 0.0 .. 0.05, default 0.004

    // Common options
    pub luminance_only: bool, // Preserve RGB chromaticity, avoid color noise: default true
    pub blend_amount: f32,    // Overall opacity / blend: 0.0 .. 1.0, default 1.0
}

impl Default for SharpeningConfig {
    fn default() -> Self {
        Self {
            method: SharpeningMethod::Wavelet,
            usm_radius: 1.5,
            usm_amount: 1.0,
            usm_threshold: 0.005,
            wavelet_scale1: 1.25,
            wavelet_scale2: 1.50,
            wavelet_scale3: 1.20,
            wavelet_noise_threshold: 0.003,
            luminance_only: true,
            blend_amount: 1.0,
        }
    }
}

pub struct ImageSharpening;

impl ImageSharpening {
    /// Applies astronomical detail sharpening according to config
    pub fn sharpen(image: &AstroImage, config: &SharpeningConfig) -> Result<AstroImage, String> {
        if image.data.is_empty() {
            return Err("Tyhjä kuva".to_string());
        }

        let width = image.width;
        let height = image.height;
        let channels = image.channels;

        let mut out = image.clone();

        if channels > 1 && config.luminance_only {
            // 1. Extract luminance plane
            let mut lum = vec![0.0f32; width * height];
            lum.par_chunks_mut(width)
                .enumerate()
                .for_each(|(y, row_lum)| {
                    for x in 0..width {
                        let idx = (y * width + x) * channels;
                        let r = image.data[idx];
                        let g = image.data[idx + 1];
                        let b = image.data[idx + 2];
                        row_lum[x] = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                    }
                });

            // 2. Sharpen luminance plane
            let sharp_lum = match config.method {
                SharpeningMethod::UnsharpMask => {
                    Self::apply_usm_plane(&lum, width, height, config.usm_radius, config.usm_amount, config.usm_threshold)
                }
                SharpeningMethod::Wavelet => {
                    Self::apply_wavelet_plane(
                        &lum,
                        width,
                        height,
                        config.wavelet_scale1,
                        config.wavelet_scale2,
                        config.wavelet_scale3,
                        config.wavelet_noise_threshold,
                    )
                }
            };

            // 3. Recombine sharpened luminance with original chromaticity vectors
            let blend = config.blend_amount.clamp(0.0, 1.0);
            out.data
                .par_chunks_exact_mut(channels)
                .enumerate()
                .for_each(|(i, pixel)| {
                    let orig_l = lum[i];
                    let s_l = sharp_lum[i];
                    let target_l = (1.0 - blend) * orig_l + blend * s_l;

                    if orig_l > 1e-7 {
                        let scale = target_l / orig_l;
                        for c in 0..channels.min(3) {
                            pixel[c] = (pixel[c] * scale).clamp(0.0, 1.0);
                        }
                    }
                });
        } else {
            // Process all channels independently (e.g. Mono or per-channel RGB)
            let blend = config.blend_amount.clamp(0.0, 1.0);

            for c in 0..channels {
                let mut plane = vec![0.0f32; width * height];
                plane
                    .par_chunks_mut(width)
                    .enumerate()
                    .for_each(|(y, row)| {
                        for x in 0..width {
                            let idx = (y * width + x) * channels + c;
                            row[x] = image.data[idx];
                        }
                    });

                let sharp_plane = match config.method {
                    SharpeningMethod::UnsharpMask => {
                        Self::apply_usm_plane(&plane, width, height, config.usm_radius, config.usm_amount, config.usm_threshold)
                    }
                    SharpeningMethod::Wavelet => {
                        Self::apply_wavelet_plane(
                            &plane,
                            width,
                            height,
                            config.wavelet_scale1,
                            config.wavelet_scale2,
                            config.wavelet_scale3,
                            config.wavelet_noise_threshold,
                        )
                    }
                };

                out.data
                    .par_chunks_exact_mut(channels)
                    .enumerate()
                    .for_each(|(i, pixel)| {
                        let orig_val = plane[i];
                        let s_val = sharp_plane[i];
                        pixel[c] = ((1.0 - blend) * orig_val + blend * s_val).clamp(0.0, 1.0);
                    });
            }
        }

        Ok(out)
    }

    /// Single-plane Unsharp Mask (Gaussian blur subtraction with noise threshold)
    fn apply_usm_plane(
        plane: &[f32],
        width: usize,
        height: usize,
        radius: f32,
        amount: f32,
        threshold: f32,
    ) -> Vec<f32> {
        let blurred = gaussian_blur_plane(plane, width, height, radius);
        let mut sharp = vec![0.0f32; width * height];
        let thresh = threshold.max(0.0);

        sharp
            .par_chunks_mut(width)
            .enumerate()
            .for_each(|(y, row_sharp)| {
                let row_start = y * width;
                for x in 0..width {
                    let idx = row_start + x;
                    let orig = plane[idx];
                    let blur = blurred[idx];
                    let diff = orig - blur;
                    let abs_diff = diff.abs();

                    let gain = if thresh <= 1e-6 {
                        1.0
                    } else if abs_diff <= thresh * 0.5 {
                        0.0
                    } else if abs_diff < thresh {
                        (abs_diff - thresh * 0.5) / (thresh * 0.5)
                    } else {
                        1.0
                    };

                    row_sharp[x] = (orig + amount * gain * diff).clamp(0.0, 1.0);
                }
            });

        sharp
    }

    /// Multiscale À Trous Wavelet decomposition using B3-spline [1/16, 1/4, 3/8, 1/4, 1/16]
    fn apply_wavelet_plane(
        plane: &[f32],
        width: usize,
        height: usize,
        k1: f32,
        k2: f32,
        k3: f32,
        noise_threshold: f32,
    ) -> Vec<f32> {
        // Level 0: original
        let c0 = plane.to_vec();

        // Level 1: step 1
        let c1 = atrous_convolve_plane(&c0, width, height, 1);
        let w1: Vec<f32> = c0.par_iter().zip(&c1).map(|(&a, &b)| a - b).collect();

        // Level 2: step 2
        let c2 = atrous_convolve_plane(&c1, width, height, 2);
        let w2: Vec<f32> = c1.par_iter().zip(&c2).map(|(&a, &b)| a - b).collect();

        // Level 3: step 4
        let c3 = atrous_convolve_plane(&c2, width, height, 4);
        let w3: Vec<f32> = c2.par_iter().zip(&c3).map(|(&a, &b)| a - b).collect();

        // Reconstruct: c3 + k3*w3 + k2*w2 + k1*w1 with noise thresholding
        let thresh = noise_threshold.max(0.0);
        let mut result = vec![0.0f32; width * height];

        result
            .par_chunks_mut(width)
            .enumerate()
            .for_each(|(y, row_res)| {
                let row_start = y * width;
                for x in 0..width {
                    let idx = row_start + x;

                    let mod_detail = |w: f32, boost: f32, scale_num: f32| -> f32 {
                        let scale_thresh = thresh / scale_num;
                        let abs_w = w.abs();
                        if abs_w < scale_thresh {
                            // Suppress pure background noise
                            w * 0.5
                        } else {
                            w * boost
                        }
                    };

                    let detail1 = mod_detail(w1[idx], k1, 1.0);
                    let detail2 = mod_detail(w2[idx], k2, 1.5);
                    let detail3 = mod_detail(w3[idx], k3, 2.0);

                    let val = c3[idx] + detail3 + detail2 + detail1;
                    row_res[x] = val.clamp(0.0, 1.0);
                }
            });

        result
    }
}

/// Separable 2D Gaussian blur of a single plane
fn gaussian_blur_plane(plane: &[f32], width: usize, height: usize, sigma: f32) -> Vec<f32> {
    let sig = sigma.max(0.2);
    let r = (sig * 3.0).ceil() as isize;
    let r = r.clamp(1, 25);

    // Compute 1D Gaussian kernel
    let mut kernel = Vec::with_capacity((2 * r + 1) as usize);
    let mut sum = 0.0f32;
    for i in -r..=r {
        let v = (-((i * i) as f32) / (2.0 * sig * sig)).exp();
        kernel.push(v);
        sum += v;
    }
    for v in &mut kernel {
        *v /= sum;
    }

    // Horizontal pass
    let mut horiz = vec![0.0f32; width * height];
    horiz
        .par_chunks_mut(width)
        .enumerate()
        .for_each(|(y, row_out)| {
            let row_in = &plane[y * width..(y + 1) * width];
            for x in 0..width {
                let mut acc = 0.0f32;
                for (k_idx, &k_val) in kernel.iter().enumerate() {
                    let offset = k_idx as isize - r;
                    let sx = (x as isize + offset).clamp(0, width as isize - 1) as usize;
                    acc += row_in[sx] * k_val;
                }
                row_out[x] = acc;
            }
        });

    // Vertical pass
    let mut vert = vec![0.0f32; width * height];
    vert.par_chunks_mut(width)
        .enumerate()
        .for_each(|(y, row_out)| {
            for x in 0..width {
                let mut acc = 0.0f32;
                for (k_idx, &k_val) in kernel.iter().enumerate() {
                    let offset = k_idx as isize - r;
                    let sy = (y as isize + offset).clamp(0, height as isize - 1) as usize;
                    acc += horiz[sy * width + x] * k_val;
                }
                row_out[x] = acc;
            }
        });

    vert
}

/// 2D À Trous convolution using separable B3-spline filter: [1/16, 1/4, 3/8, 1/4, 1/16] with step size
fn atrous_convolve_plane(plane: &[f32], width: usize, height: usize, step: usize) -> Vec<f32> {
    let s = step as isize;
    let w = [0.0625f32, 0.25f32, 0.375f32, 0.25f32, 0.0625f32];
    let offsets = [-2 * s, -s, 0, s, 2 * s];

    // Horizontal pass
    let mut horiz = vec![0.0f32; width * height];
    horiz
        .par_chunks_mut(width)
        .enumerate()
        .for_each(|(y, row_out)| {
            let row_in = &plane[y * width..(y + 1) * width];
            for x in 0..width {
                let mut acc = 0.0f32;
                for i in 0..5 {
                    let sx = (x as isize + offsets[i]).clamp(0, width as isize - 1) as usize;
                    acc += row_in[sx] * w[i];
                }
                row_out[x] = acc;
            }
        });

    // Vertical pass
    let mut vert = vec![0.0f32; width * height];
    vert.par_chunks_mut(width)
        .enumerate()
        .for_each(|(y, row_out)| {
            for x in 0..width {
                let mut acc = 0.0f32;
                for i in 0..5 {
                    let sy = (y as isize + offsets[i]).clamp(0, height as isize - 1) as usize;
                    acc += horiz[sy * width + x] * w[i];
                }
                row_out[x] = acc;
            }
        });

    vert
}
