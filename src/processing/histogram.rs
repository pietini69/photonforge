use crate::core::image::AstroImage;
use rayon::prelude::*;

pub struct HistogramProcessor;

impl HistogramProcessor {
    /// Computes 256-bin histogram for each channel
    #[allow(dead_code)]
    pub fn compute_histogram(image: &AstroImage) -> Vec<Vec<u32>> {
        let mut hist = vec![vec![0u32; 256]; image.channels];

        for chunk in image.data.chunks_exact(image.channels) {
            for (c, &val) in chunk.iter().enumerate() {
                let bin = (val.clamp(0.0, 1.0) * 255.0) as usize;
                hist[c][bin.min(255)] += 1;
            }
        }

        hist
    }

    /// Permanent stretch: applies shadows, midtones, highlights to image data per channel
    pub fn apply_permanent_stretch_channels(
        image: &AstroImage,
        params: &[(f32, f32, f32)],
    ) -> AstroImage {
        let mut out = image.clone();

        out.data
            .par_chunks_exact_mut(image.channels)
            .for_each(|pixel| {
                if image.channels == 1 {
                    let (shadows, midtones, highlights) = params.first().copied().unwrap_or((0.0, 0.5, 1.0));
                    pixel[0] = crate::core::image::apply_mtf_stretch(pixel[0], shadows, midtones, highlights);
                } else {
                    for c in 0..image.channels.min(3) {
                        let (shadows, midtones, highlights) = params.get(c).copied().unwrap_or((0.0, 0.5, 1.0));
                        pixel[c] = crate::core::image::apply_mtf_stretch(pixel[c], shadows, midtones, highlights);
                    }
                }
            });

        out
    }

    /// Permanent stretch: applies uniform shadows, midtones, highlights to image data
    #[allow(dead_code)]
    pub fn apply_permanent_stretch(
        image: &AstroImage,
        shadows: f32,
        midtones: f32,
        highlights: f32,
    ) -> AstroImage {
        Self::apply_permanent_stretch_channels(image, &[(shadows, midtones, highlights); 3])
    }

    /// Saturation adjustment (RGB only)
    pub fn adjust_saturation(image: &AstroImage, factor: f32) -> Result<AstroImage, String> {
        if !image.is_rgb() {
            return Err("Saturation adjustment requires RGB image".to_string());
        }

        let mut out = image.clone();

        out.data.par_chunks_exact_mut(3).for_each(|pixel| {
            let r = pixel[0];
            let g = pixel[1];
            let b = pixel[2];

            // Standard luminance weights for astrophotography
            let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;

            pixel[0] = (lum + (r - lum) * factor).clamp(0.0, 1.0);
            pixel[1] = (lum + (g - lum) * factor).clamp(0.0, 1.0);
            pixel[2] = (lum + (b - lum) * factor).clamp(0.0, 1.0);
        });

        Ok(out)
    }

    /// Color balance / channel multipliers (RGB only)
    pub fn color_balance(
        image: &AstroImage,
        r_scale: f32,
        g_scale: f32,
        b_scale: f32,
    ) -> Result<AstroImage, String> {
        if !image.is_rgb() {
            return Err("Color balance requires an RGB image".to_string());
        }

        let mut out = image.clone();
        out.data.par_chunks_exact_mut(3).for_each(|pixel| {
            if pixel[0] > 0.0 || pixel[1] > 0.0 || pixel[2] > 0.0 {
                pixel[0] = (pixel[0] * r_scale).clamp(0.0, 1.0);
                pixel[1] = (pixel[1] * g_scale).clamp(0.0, 1.0);
                pixel[2] = (pixel[2] * b_scale).clamp(0.0, 1.0);
            }
        });

        Ok(out)
    }

    /// Dual-band narrowband enhancement (Ha / [OIII] contrast boost)
    pub fn dual_band_boost(
        image: &AstroImage,
        ha_boost: f32,
        oiii_boost: f32,
    ) -> Result<AstroImage, String> {
        if !image.is_rgb() {
            return Err("Dual-band boost requires an RGB image".to_string());
        }

        let mut out = image.clone();
        let (r_bg, _) = image.channel_median_and_mad(0);
        let (g_bg, _) = image.channel_median_and_mad(1);
        let (b_bg, _) = image.channel_median_and_mad(2);

        out.data.par_chunks_exact_mut(3).for_each(|pixel| {
            let r = pixel[0];
            let g = pixel[1];
            let b = pixel[2];

            if r <= 0.0 && g <= 0.0 && b <= 0.0 {
                return;
            }

            let r_sig = (r - r_bg).max(0.0);
            let g_sig = (g - g_bg).max(0.0);
            let b_sig = (b - b_bg).max(0.0);

            let oiii_sig = (g_sig + b_sig) * 0.5;

            let new_r = r_bg + r_sig * ha_boost;
            let new_g = g_bg + g_sig + oiii_sig * (oiii_boost - 1.0);
            let new_b = b_bg + b_sig + oiii_sig * (oiii_boost - 1.0);

            pixel[0] = new_r.clamp(0.0, 1.0);
            pixel[1] = new_g.clamp(0.0, 1.0);
            pixel[2] = new_b.clamp(0.0, 1.0);
        });

        Ok(out)
    }

    /// Reflection nebula enhancement (enhances blue/cyan dust clouds like Pleiades M45, Iris NGC 7023)
    pub fn reflection_nebula_boost(
        image: &AstroImage,
        blue_boost: f32,
        cyan_boost: f32,
        saturation_boost: f32,
    ) -> Result<AstroImage, String> {
        if !image.is_rgb() {
            return Err("Heijastussumun korostus vaatii RGB-kuvan".to_string());
        }

        let mut out = image.clone();
        let (r_bg, _) = image.channel_median_and_mad(0);
        let (g_bg, _) = image.channel_median_and_mad(1);
        let (b_bg, _) = image.channel_median_and_mad(2);

        out.data.par_chunks_exact_mut(3).for_each(|pixel| {
            let r = pixel[0];
            let g = pixel[1];
            let b = pixel[2];

            if r <= 0.0 && g <= 0.0 && b <= 0.0 {
                return;
            }

            let r_sig = (r - r_bg).max(0.0);
            let g_sig = (g - g_bg).max(0.0);
            let b_sig = (b - b_bg).max(0.0);

            // Reflection nebulae emit strongly in blue starlight reflection + faint cyan
            let new_b = b_bg + b_sig * blue_boost;
            let new_g = g_bg + g_sig * cyan_boost;
            let new_r = r_bg + r_sig;

            // Apply saturation boost on the signal
            let lum = 0.2126 * new_r + 0.7152 * new_g + 0.0722 * new_b;
            let final_r = (lum + (new_r - lum) * saturation_boost).clamp(0.0, 1.0);
            let final_g = (lum + (new_g - lum) * saturation_boost).clamp(0.0, 1.0);
            let final_b = (lum + (new_b - lum) * saturation_boost).clamp(0.0, 1.0);

            pixel[0] = final_r;
            pixel[1] = final_g;
            pixel[2] = final_b;
        });

        Ok(out)
    }

    /// Arcsinh non-linear stretch: preserves color saturation while pulling up faint nebulae
    pub fn arcsinh_stretch(
        image: &AstroImage,
        stretch_factor: f32,
        black_point: f32,
    ) -> Result<AstroImage, String> {
        if !image.is_rgb() {
            return Err("Arcsinh-venytys vaatii RGB-kuvan".to_string());
        }

        let mut out = image.clone();
        let asinh_norm = (stretch_factor * (1.0 - black_point)).max(1e-4).asinh();

        out.data.par_chunks_exact_mut(3).for_each(|pixel| {
            let r = pixel[0];
            let g = pixel[1];
            let b = pixel[2];

            if r <= 0.0 && g <= 0.0 && b <= 0.0 {
                return;
            }

            let lum = (0.2126 * r + 0.7152 * g + 0.0722 * b).max(1e-7);
            let net_lum = (lum - black_point).max(0.0);

            let new_lum = if net_lum <= 0.0 {
                0.0
            } else {
                (stretch_factor * net_lum).asinh() / asinh_norm
            };

            let scale = new_lum / lum;
            pixel[0] = (r * scale).clamp(0.0, 1.0);
            pixel[1] = (g * scale).clamp(0.0, 1.0);
            pixel[2] = (b * scale).clamp(0.0, 1.0);
        });

        Ok(out)
    }
}
