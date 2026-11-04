use crate::core::image::{AstroImage, ImageMetadata};
use crate::processing::alignment::{ImageWarper, StarDetector, StarMatcher};
use rayon::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AstroFilter {
    Red,
    Green,
    Blue,
    Luminance,
    HAlpha,
    OIII,
    SII,
    #[allow(dead_code)]
    Custom,
}

impl AstroFilter {
    #[allow(dead_code)]
    pub fn to_str(&self) -> &'static str {
        match self {
            AstroFilter::Red => "Red (R)",
            AstroFilter::Green => "Green (G)",
            AstroFilter::Blue => "Blue (B)",
            AstroFilter::Luminance => "Luminance (L)",
            AstroFilter::HAlpha => "H-alpha (Ha)",
            AstroFilter::OIII => "[OIII]",
            AstroFilter::SII => "[SII]",
            AstroFilter::Custom => "Muu / Custom",
        }
    }

    pub fn badge(&self) -> &'static str {
        match self {
            AstroFilter::Red => "R",
            AstroFilter::Green => "G",
            AstroFilter::Blue => "B",
            AstroFilter::Luminance => "L",
            AstroFilter::HAlpha => "Ha",
            AstroFilter::OIII => "OIII",
            AstroFilter::SII => "SII",
            AstroFilter::Custom => "Filt",
        }
    }

    #[allow(dead_code)]
    pub fn color_rgb(&self) -> (u8, u8, u8) {
        match self {
            AstroFilter::Red => (255, 90, 90),
            AstroFilter::Green => (80, 240, 120),
            AstroFilter::Blue => (90, 180, 255),
            AstroFilter::Luminance => (235, 240, 250),
            AstroFilter::HAlpha => (255, 60, 80),
            AstroFilter::OIII => (60, 235, 215),
            AstroFilter::SII => (255, 145, 50),
            AstroFilter::Custom => (180, 190, 205),
        }
    }

    pub fn detect(header_filter: Option<&str>, file_name: &str) -> Option<AstroFilter> {
        if let Some(h) = header_filter {
            let h_up = h.trim().to_uppercase();
            if h_up == "R" || h_up == "RED" || h_up.starts_with("RED") {
                return Some(AstroFilter::Red);
            }
            if h_up == "G" || h_up == "GREEN" || h_up.starts_with("GREEN") {
                return Some(AstroFilter::Green);
            }
            if h_up == "B" || h_up == "BLUE" || h_up.starts_with("BLUE") {
                return Some(AstroFilter::Blue);
            }
            if h_up == "L"
                || h_up == "LUM"
                || h_up == "LUMINANCE"
                || h_up == "CLEAR"
                || h_up.starts_with("LUM")
            {
                return Some(AstroFilter::Luminance);
            }
            if h_up == "HA"
                || h_up == "HALPHA"
                || h_up == "H-ALPHA"
                || h_up == "H_ALPHA"
                || h_up.contains("HA")
                || h_up.contains("HALPHA")
            {
                return Some(AstroFilter::HAlpha);
            }
            if h_up == "OIII"
                || h_up == "O3"
                || h_up == "O-III"
                || h_up.contains("OIII")
                || h_up.contains("O3")
            {
                return Some(AstroFilter::OIII);
            }
            if h_up == "SII"
                || h_up == "S2"
                || h_up == "S-II"
                || h_up.contains("SII")
                || h_up.contains("S2")
            {
                return Some(AstroFilter::SII);
            }
        }

        let fname_up = file_name.to_uppercase();
        let tokens: Vec<&str> = fname_up
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| !t.is_empty())
            .collect();

        for &t in &tokens {
            match t {
                "HA" | "HALPHA" | "H" => return Some(AstroFilter::HAlpha),
                "OIII" | "O3" => return Some(AstroFilter::OIII),
                "SII" | "S2" => return Some(AstroFilter::SII),
                "L" | "LUM" | "LUMINANCE" | "CLEAR" => return Some(AstroFilter::Luminance),
                "R" | "RED" => return Some(AstroFilter::Red),
                "G" | "GREEN" => return Some(AstroFilter::Green),
                "B" | "BLUE" => return Some(AstroFilter::Blue),
                _ => {}
            }
        }

        None
    }
}

#[derive(Clone, Debug)]
pub struct ChannelCombinerConfig {
    pub r_weight: f32,
    pub g_weight: f32,
    pub b_weight: f32,
    pub l_weight: f32,
    pub saturation_boost: f32,
    pub linear_fit: bool,
    pub align_stars: bool,
}

impl Default for ChannelCombinerConfig {
    fn default() -> Self {
        Self {
            r_weight: 1.0,
            g_weight: 1.0,
            b_weight: 1.0,
            l_weight: 1.0,
            saturation_boost: 1.25,
            linear_fit: true,
            align_stars: true,
        }
    }
}

pub struct ChannelCombiner;

impl ChannelCombiner {
    /// Extracts a single-channel mono buffer from an AstroImage.
    /// If the image is 3-channel RGB, converts to Rec.709 perceived luminance.
    pub fn extract_mono_buffer(img: &AstroImage) -> Vec<f32> {
        if img.channels == 1 {
            img.data.clone()
        } else {
            let n = img.width * img.height;
            let mut mono = vec![0.0f32; n];
            for i in 0..n {
                let r = img.data[i * 3];
                let g = img.data[i * 3 + 1];
                let b = img.data[i * 3 + 2];
                mono[i] = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            }
            mono
        }
    }

    /// Aligns `target` image to `reference` using star detection and asterism matching.
    /// If star matching fails or target already matches dimensions, returns target.
    pub fn align_to_reference(reference: &AstroImage, target: &AstroImage) -> AstroImage {
        let ref_stars = StarDetector::detect(reference, 60);
        let tgt_stars = StarDetector::detect(target, 60);
        if let Some(tf) = StarMatcher::match_stars(&ref_stars, &tgt_stars) {
            ImageWarper::warp_to_reference(target, &tf, reference.width, reference.height)
        } else {
            target.clone()
        }
    }

    /// Computes median and Median Absolute Deviation (MAD) of a slice.
    fn compute_median_mad(data: &[f32]) -> (f32, f32) {
        if data.is_empty() {
            return (0.0, 0.0);
        }

        // Subsample up to 200,000 pixels for fast median/MAD computation
        let step = (data.len() / 200_000).max(1);
        let mut sample: Vec<f32> = data
            .iter()
            .step_by(step)
            .copied()
            .filter(|v| v.is_finite())
            .collect();
        if sample.is_empty() {
            return (0.0, 0.0);
        }

        sample.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let median = sample[sample.len() / 2];

        let mut diffs: Vec<f32> = sample.iter().map(|&v| (v - median).abs()).collect();
        diffs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mad = diffs[diffs.len() / 2].max(1e-6);

        (median, mad)
    }

    /// Normalizes (linear fits) `target` channel to have matching median and dispersion as `reference`.
    pub fn linear_fit(reference: &[f32], target: &mut [f32]) {
        let (ref_med, ref_mad) = Self::compute_median_mad(reference);
        let (tgt_med, tgt_mad) = Self::compute_median_mad(target);

        let scale = (ref_mad / tgt_mad).clamp(0.2, 5.0);

        target.par_iter_mut().for_each(|val| {
            *val = ((*val - tgt_med) * scale + ref_med).max(0.0);
        });
    }

    /// Combines three monochrome images into an RGB image with optional Luminance (LRGB)
    pub fn combine(
        r_img: &AstroImage,
        g_img: &AstroImage,
        b_img: &AstroImage,
        l_img_opt: Option<&AstroImage>,
        config: &ChannelCombinerConfig,
    ) -> Result<AstroImage, String> {
        let width = r_img.width;
        let height = r_img.height;

        // Optionally align G, B, and L to R reference
        let aligned_g = if config.align_stars {
            Self::align_to_reference(r_img, g_img)
        } else {
            g_img.clone()
        };

        let aligned_b = if config.align_stars {
            Self::align_to_reference(r_img, b_img)
        } else {
            b_img.clone()
        };

        let aligned_l = if let Some(l) = l_img_opt {
            if config.align_stars {
                Some(Self::align_to_reference(r_img, l))
            } else {
                Some(l.clone())
            }
        } else {
            None
        };

        let mut r_chan = Self::extract_mono_buffer(r_img);
        let mut g_chan = Self::extract_mono_buffer(&aligned_g);
        let mut b_chan = Self::extract_mono_buffer(&aligned_b);

        if r_chan.len() != width * height
            || g_chan.len() != width * height
            || b_chan.len() != width * height
        {
            return Err(format!(
                "Kuvakoot eivät täsmää: R={}x{}, G={}x{}, B={}x{}. Kohdistus ei onnistunut.",
                width, height, aligned_g.width, aligned_g.height, aligned_b.width, aligned_b.height
            ));
        }

        // Apply Linear Fit to balance color channels
        if config.linear_fit {
            Self::linear_fit(&r_chan, &mut g_chan);
            Self::linear_fit(&r_chan, &mut b_chan);
        }

        // Apply channel weights
        r_chan.par_iter_mut().for_each(|v| *v *= config.r_weight);
        g_chan.par_iter_mut().for_each(|v| *v *= config.g_weight);
        b_chan.par_iter_mut().for_each(|v| *v *= config.b_weight);

        // If Luminance is present, combine using LRGB Chrominance preservation
        let mut rgb_data = vec![0.0f32; width * height * 3];

        if let Some(l_img) = &aligned_l {
            let mut l_chan = Self::extract_mono_buffer(l_img);
            if l_chan.len() == width * height {
                if config.linear_fit {
                    Self::linear_fit(&r_chan, &mut l_chan);
                }

                let l_wt = config.l_weight.clamp(0.0, 1.0);
                let sat_boost = config.saturation_boost.max(0.0);

                rgb_data
                    .par_chunks_exact_mut(3)
                    .enumerate()
                    .for_each(|(i, rgb)| {
                        let r = r_chan[i];
                        let g = g_chan[i];
                        let b = b_chan[i];
                        let l = l_chan[i];

                        // Perceived RGB luminance
                        let y_rgb = 0.2126 * r + 0.7152 * g + 0.0722 * b;

                        // Blended luminance
                        let l_eff = (1.0 - l_wt) * y_rgb + l_wt * l;

                        // Scale RGB by luminance ratio
                        let ratio = if y_rgb > 1e-6 { l_eff / y_rgb } else { 1.0 };
                        let r_scaled = r * ratio;
                        let g_scaled = g * ratio;
                        let b_scaled = b * ratio;

                        // Boost chrominance relative to effective luminance to prevent desaturation
                        rgb[0] = (l_eff + sat_boost * (r_scaled - l_eff)).max(0.0);
                        rgb[1] = (l_eff + sat_boost * (g_scaled - l_eff)).max(0.0);
                        rgb[2] = (l_eff + sat_boost * (b_scaled - l_eff)).max(0.0);
                    });
            } else {
                // Luminance size mismatch fallback
                rgb_data
                    .par_chunks_exact_mut(3)
                    .enumerate()
                    .for_each(|(i, rgb)| {
                        rgb[0] = r_chan[i];
                        rgb[1] = g_chan[i];
                        rgb[2] = b_chan[i];
                    });
            }
        } else {
            // Standard RGB combination
            rgb_data
                .par_chunks_exact_mut(3)
                .enumerate()
                .for_each(|(i, rgb)| {
                    rgb[0] = r_chan[i];
                    rgb[1] = g_chan[i];
                    rgb[2] = b_chan[i];
                });
        }

        let mut metadata = ImageMetadata {
            file_name: if aligned_l.is_some() {
                "Composite_LRGB.fits".to_string()
            } else {
                "Composite_RGB.fits".to_string()
            },
            ..r_img.metadata.clone()
        };
        metadata.filter_name = Some("LRGB/RGB Composite".to_string());

        Ok(AstroImage {
            width,
            height,
            channels: 3,
            data: rgb_data,
            metadata,
        })
    }
}
