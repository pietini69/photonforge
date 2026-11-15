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

    /// Permanent stretch: applies any STF stretch parameters directly to image 32-bit float data
    pub fn apply_permanent_stretch_params(
        image: &AstroImage,
        stf: &crate::processing::stf::StfParameters,
    ) -> AstroImage {
        if stf.is_linear() {
            let mult = 2.0_f32.powf(stf.linear_exposure_ev);
            let mut out = image.clone();
            out.data.par_iter_mut().for_each(|v| {
                *v = (*v * mult).clamp(0.0, 1.0);
            });
            return out;
        }

        match stf.mode {
            crate::processing::stf::StretchMode::Linear => image.clone(),
            crate::processing::stf::StretchMode::AutoStf | crate::processing::stf::StretchMode::ManualMtf => {
                let params = [
                    (stf.channels[0].shadows, stf.channels[0].midtones, stf.channels[0].highlights),
                    (stf.channels[1].shadows, stf.channels[1].midtones, stf.channels[1].highlights),
                    (stf.channels[2].shadows, stf.channels[2].midtones, stf.channels[2].highlights),
                ];
                Self::apply_permanent_stretch_channels(image, &params)
            }
            crate::processing::stf::StretchMode::Arcsinh => {
                let s = stf.arcsinh_stretch.max(1.0);
                let b = stf.arcsinh_black;
                let norm = s.asinh();
                let avg_sh = (stf.channels[0].shadows + stf.channels[1].shadows + stf.channels[2].shadows) / 3.0;
                let ch_ped = [
                    stf.channels[0].shadows - avg_sh,
                    stf.channels[1].shadows - avg_sh,
                    stf.channels[2].shadows - avg_sh,
                ];
                let mut out = image.clone();

                out.data
                    .par_chunks_exact_mut(image.channels)
                    .for_each(|pixel| {
                        if image.channels == 1 {
                            let v = (pixel[0] - b).max(0.0);
                            pixel[0] = ((s * v).asinh() / norm).clamp(0.0, 1.0);
                        } else {
                            let r_net = (pixel[0] - (b + ch_ped[0])).max(0.0);
                            let g_net = (pixel[1] - (b + ch_ped[1])).max(0.0);
                            let b_net = (pixel[2] - (b + ch_ped[2])).max(0.0);
                            let intensity = (r_net + g_net + b_net) / 3.0;
                            if intensity > 1e-7 {
                                let stretched_i = (s * intensity).asinh() / norm;
                                let scale = stretched_i / intensity;
                                pixel[0] = (r_net * scale).clamp(0.0, 1.0);
                                pixel[1] = (g_net * scale).clamp(0.0, 1.0);
                                pixel[2] = (b_net * scale).clamp(0.0, 1.0);
                            } else {
                                pixel[0] = 0.0;
                                pixel[1] = 0.0;
                                pixel[2] = 0.0;
                            }
                        }
                    });
                out
            }
            crate::processing::stf::StretchMode::HyperMetric => {
                let d = stf.hms_d.max(1.0);
                let b = stf.hms_b.max(0.0001);
                let sp = stf.hms_sp.max(0.0);
                let grip = stf.hms_color_grip.max(0.0);
                let avg_sh = 0.2126 * stf.channels[0].shadows
                    + 0.7152 * stf.channels[1].shadows
                    + 0.0722 * stf.channels[2].shadows;
                let ch_ped = [
                    stf.channels[0].shadows - avg_sh,
                    stf.channels[1].shadows - avg_sh,
                    stf.channels[2].shadows - avg_sh,
                ];

                let asinh_b = b.asinh();
                let denom = ((d * (1.0 - sp).max(1e-4) + b).asinh() - asinh_b).max(1e-6);

                let hms_eval_net = |net_val: f32| -> f32 {
                    if net_val <= 0.0 {
                        0.0
                    } else {
                        (((d * net_val + b).asinh() - asinh_b) / denom).clamp(0.0, 1.0)
                    }
                };

                let mut out = image.clone();
                out.data
                    .par_chunks_exact_mut(image.channels)
                    .for_each(|pixel| {
                        if image.channels == 1 {
                            pixel[0] = hms_eval_net((pixel[0] - sp).max(0.0));
                        } else {
                            let r_net = (pixel[0] - (sp + ch_ped[0])).max(0.0);
                            let g_net = (pixel[1] - (sp + ch_ped[1])).max(0.0);
                            let b_net = (pixel[2] - (sp + ch_ped[2])).max(0.0);

                            let lum = 0.2126 * r_net + 0.7152 * g_net + 0.0722 * b_net;
                            if lum > 1e-7 {
                                let stretched_lum = hms_eval_net(lum);
                                let scale = stretched_lum / lum;

                                let r_base = r_net * scale;
                                let g_base = g_net * scale;
                                let b_base = b_net * scale;

                                let (r_fin, g_fin, b_fin) = if (grip - 1.0).abs() < 1e-3 {
                                    (r_base, g_base, b_base)
                                } else {
                                    (
                                        stretched_lum + grip * (r_base - stretched_lum),
                                        stretched_lum + grip * (g_base - stretched_lum),
                                        stretched_lum + grip * (b_base - stretched_lum),
                                    )
                                };

                                pixel[0] = r_fin.clamp(0.0, 1.0);
                                pixel[1] = g_fin.clamp(0.0, 1.0);
                                pixel[2] = b_fin.clamp(0.0, 1.0);
                            } else {
                                pixel[0] = 0.0;
                                pixel[1] = 0.0;
                                pixel[2] = 0.0;
                            }
                        }
                    });
                out
            }
            crate::processing::stf::StretchMode::Logarithmic => {
                let a = stf.log_factor.max(1.0);
                let b = stf.log_black;
                let norm = (1.0 + a).ln().max(1e-6);
                let avg_sh = (stf.channels[0].shadows + stf.channels[1].shadows + stf.channels[2].shadows) / 3.0;
                let ch_ped = [
                    stf.channels[0].shadows - avg_sh,
                    stf.channels[1].shadows - avg_sh,
                    stf.channels[2].shadows - avg_sh,
                ];
                let mut out = image.clone();

                out.data
                    .par_chunks_exact_mut(image.channels)
                    .for_each(|pixel| {
                        if image.channels == 1 {
                            let val = (pixel[0] - b).max(0.0);
                            pixel[0] = ((1.0 + a * val).ln() / norm).clamp(0.0, 1.0);
                        } else {
                            for c in 0..3 {
                                let val = (pixel[c] - (b + ch_ped[c])).max(0.0);
                                pixel[c] = ((1.0 + a * val).ln() / norm).clamp(0.0, 1.0);
                            }
                        }
                    });
                out
            }
            crate::processing::stf::StretchMode::Ghs => {
                let d = stf.ghs_d.max(0.1);
                let b = stf.ghs_b;
                let sub0 = (d * (-b)).asinh();
                let den = ((d * (1.0 - b)).asinh() - sub0).max(1e-6);
                let avg_sh = (stf.channels[0].shadows + stf.channels[1].shadows + stf.channels[2].shadows) / 3.0;
                let ch_ped = [
                    stf.channels[0].shadows - avg_sh,
                    stf.channels[1].shadows - avg_sh,
                    stf.channels[2].shadows - avg_sh,
                ];
                let mut out = image.clone();

                out.data
                    .par_chunks_exact_mut(image.channels)
                    .for_each(|pixel| {
                        if image.channels == 1 {
                            let num = (d * (pixel[0] - b)).asinh() - sub0;
                            pixel[0] = (num / den).clamp(0.0, 1.0);
                        } else {
                            for c in 0..3 {
                                let v = (pixel[c] - ch_ped[c]).max(0.0);
                                let num = (d * (v - b)).asinh() - sub0;
                                pixel[c] = (num / den).clamp(0.0, 1.0);
                            }
                        }
                    });
                out
            }
            crate::processing::stf::StretchMode::Root => {
                let gamma = stf.gamma.clamp(0.05, 2.0);
                let b = stf.gamma_black;
                let den = (1.0 - b).max(1e-5);
                let avg_sh = (stf.channels[0].shadows + stf.channels[1].shadows + stf.channels[2].shadows) / 3.0;
                let ch_ped = [
                    stf.channels[0].shadows - avg_sh,
                    stf.channels[1].shadows - avg_sh,
                    stf.channels[2].shadows - avg_sh,
                ];
                let mut out = image.clone();

                out.data
                    .par_chunks_exact_mut(image.channels)
                    .for_each(|pixel| {
                        if image.channels == 1 {
                            let norm = ((pixel[0] - b) / den).clamp(0.0, 1.0);
                            pixel[0] = norm.powf(gamma).clamp(0.0, 1.0);
                        } else {
                            for c in 0..3 {
                                let norm = ((pixel[c] - (b + ch_ped[c])) / den).clamp(0.0, 1.0);
                                pixel[c] = norm.powf(gamma).clamp(0.0, 1.0);
                            }
                        }
                    });
                out
            }
        }
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
    /// Operates on the background-neutralized astronomical signal above the sky pedestal
    /// so that un-neutralized raw Bayer pedestals (where G_bg > B_bg) do not invert blue saturation,
    /// and preserves symmetric sky background noise distribution.
    pub fn adjust_saturation(image: &AstroImage, factor: f32) -> Result<AstroImage, String> {
        if !image.is_rgb() {
            return Err("Saturation adjustment requires RGB image".to_string());
        }

        let mut out = image.clone();
        let (r_bg, r_mad) = image.channel_median_and_mad(0);
        let (g_bg, g_mad) = image.channel_median_and_mad(1);
        let (b_bg, b_mad) = image.channel_median_and_mad(2);
        let neutral_bg = (r_bg + g_bg + b_bg) / 3.0;
        let avg_mad = ((r_mad + g_mad + b_mad) / 3.0).max(1e-5);
        let noise_gate = 1.5 * avg_mad;

        out.data.par_chunks_exact_mut(3).for_each(|pixel| {
            let r = pixel[0];
            let g = pixel[1];
            let b = pixel[2];

            if r <= 0.0 && g <= 0.0 && b <= 0.0 {
                return;
            }

            let dr = r - r_bg;
            let dg = g - g_bg;
            let db = b - b_bg;

            let sr = dr.max(0.0);
            let sg = dg.max(0.0);
            let sb = db.max(0.0);

            let max_sig = sr.max(sg).max(sb);
            // Smooth SNR weight so sky background noise is not color-amplified
            let weight = (max_sig / noise_gate).clamp(0.0, 1.0);
            let eff_factor = 1.0 + (factor - 1.0) * weight;

            let sig_lum = 0.2126 * sr + 0.7152 * sg + 0.0722 * sb;
            let new_dr = if dr >= 0.0 {
                sig_lum + (sr - sig_lum) * eff_factor
            } else {
                dr
            };
            let new_dg = if dg >= 0.0 {
                sig_lum + (sg - sig_lum) * eff_factor
            } else {
                dg
            };
            let new_db = if db >= 0.0 {
                sig_lum + (sb - sig_lum) * eff_factor
            } else {
                db
            };

            pixel[0] = (neutral_bg + new_dr).clamp(0.0, 1.0);
            pixel[1] = (neutral_bg + new_dg).clamp(0.0, 1.0);
            pixel[2] = (neutral_bg + new_db).clamp(0.0, 1.0);
        });

        Ok(out)
    }

    /// Color balance / channel multipliers (RGB only)
    /// Scales the astronomical signal above the neutralized sky background while preserving
    /// background noise symmetry so subsequent Auto-STF does not cancel out the balance adjustment.
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
        let (r_bg, _) = image.channel_median_and_mad(0);
        let (g_bg, _) = image.channel_median_and_mad(1);
        let (b_bg, _) = image.channel_median_and_mad(2);
        let neutral_bg = (r_bg + g_bg + b_bg) / 3.0;

        out.data.par_chunks_exact_mut(3).for_each(|pixel| {
            let r = pixel[0];
            let g = pixel[1];
            let b = pixel[2];
            if r > 0.0 || g > 0.0 || b > 0.0 {
                let dr = r - r_bg;
                let dg = g - g_bg;
                let db = b - b_bg;

                let new_dr = if dr >= 0.0 { dr * r_scale } else { dr };
                let new_dg = if dg >= 0.0 { dg * g_scale } else { dg };
                let new_db = if db >= 0.0 { db * b_scale } else { db };

                pixel[0] = (neutral_bg + new_dr).clamp(0.0, 1.0);
                pixel[1] = (neutral_bg + new_dg).clamp(0.0, 1.0);
                pixel[2] = (neutral_bg + new_db).clamp(0.0, 1.0);
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
        let neutral_bg = (r_bg + g_bg + b_bg) / 3.0;

        out.data.par_chunks_exact_mut(3).for_each(|pixel| {
            let r = pixel[0];
            let g = pixel[1];
            let b = pixel[2];

            if r <= 0.0 && g <= 0.0 && b <= 0.0 {
                return;
            }

            let dr = r - r_bg;
            let dg = g - g_bg;
            let db = b - b_bg;

            let r_sig = dr.max(0.0);
            let g_sig = dg.max(0.0);
            let b_sig = db.max(0.0);

            let oiii_sig = (g_sig + b_sig) * 0.5;

            let new_dr = if dr >= 0.0 { r_sig * ha_boost } else { dr };
            let new_dg = if dg >= 0.0 {
                g_sig + oiii_sig * (oiii_boost - 1.0)
            } else {
                dg
            };
            let new_db = if db >= 0.0 {
                b_sig + oiii_sig * (oiii_boost - 1.0)
            } else {
                db
            };

            pixel[0] = (neutral_bg + new_dr).clamp(0.0, 1.0);
            pixel[1] = (neutral_bg + new_dg).clamp(0.0, 1.0);
            pixel[2] = (neutral_bg + new_db).clamp(0.0, 1.0);
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
        let (r_bg, r_mad) = image.channel_median_and_mad(0);
        let (g_bg, g_mad) = image.channel_median_and_mad(1);
        let (b_bg, b_mad) = image.channel_median_and_mad(2);
        let neutral_bg = (r_bg + g_bg + b_bg) / 3.0;
        let avg_mad = ((r_mad + g_mad + b_mad) / 3.0).max(1e-5);
        let noise_gate = 1.2 * avg_mad;

        out.data.par_chunks_exact_mut(3).for_each(|pixel| {
            let r = pixel[0];
            let g = pixel[1];
            let b = pixel[2];

            if r <= 0.0 && g <= 0.0 && b <= 0.0 {
                return;
            }

            let dr = r - r_bg;
            let dg = g - g_bg;
            let db = b - b_bg;

            let r_sig = dr.max(0.0);
            let g_sig = dg.max(0.0);
            let b_sig = db.max(0.0);

            // Smoothly gate boost above sky background noise so pure sky noise stays neutral
            let max_sig = r_sig.max(g_sig).max(b_sig);
            let weight = (max_sig / noise_gate).clamp(0.0, 1.0);
            let eff_b_boost = 1.0 + (blue_boost - 1.0) * weight;
            let eff_c_boost = 1.0 + (cyan_boost - 1.0) * weight;
            let eff_sat = 1.0 + (saturation_boost - 1.0) * weight;

            // Boost blue starlight reflection + cyan dust signal above background
            let sr = r_sig;
            let sg = g_sig * eff_c_boost;
            let sb = b_sig * eff_b_boost;

            // Apply saturation boost strictly to the background-subtracted signal
            let sig_lum = 0.2126 * sr + 0.7152 * sg + 0.0722 * sb;
            let sat_sr = (sig_lum + (sr - sig_lum) * eff_sat).max(0.0);
            let sat_sg = (sig_lum + (sg - sig_lum) * eff_sat).max(0.0);
            let sat_sb = (sig_lum + (sb - sig_lum) * eff_sat).max(0.0);

            // Preserve negative background noise excursions symmetrically so MAD does not collapse
            let final_dr = if dr >= 0.0 { sat_sr } else { dr };
            let final_dg = if dg >= 0.0 { sat_sg } else { dg };
            let final_db = if db >= 0.0 { sat_sb } else { db };

            pixel[0] = (neutral_bg + final_dr).clamp(0.0, 1.0);
            pixel[1] = (neutral_bg + final_dg).clamp(0.0, 1.0);
            pixel[2] = (neutral_bg + final_db).clamp(0.0, 1.0);
        });

        Ok(out)
    }

    /// Arcsinh non-linear stretch: preserves color saturation while pulling up faint nebulae
    #[allow(dead_code)]
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

            let r_net = (r - black_point).max(0.0);
            let g_net = (g - black_point).max(0.0);
            let b_net = (b - black_point).max(0.0);
            let net_lum = 0.2126 * r_net + 0.7152 * g_net + 0.0722 * b_net;

            if net_lum > 1e-7 {
                let new_lum = (stretch_factor * net_lum).asinh() / asinh_norm;
                let scale = new_lum / net_lum;
                pixel[0] = (r_net * scale).clamp(0.0, 1.0);
                pixel[1] = (g_net * scale).clamp(0.0, 1.0);
                pixel[2] = (b_net * scale).clamp(0.0, 1.0);
            } else {
                pixel[0] = 0.0;
                pixel[1] = 0.0;
                pixel[2] = 0.0;
            }
        });

        Ok(out)
    }
}
