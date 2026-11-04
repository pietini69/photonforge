use crate::core::image::{AstroImage, ImageMetadata};
use crate::processing::alignment::StarDetector;
use rayon::prelude::*;

/// Calibration mode: classic Photometric (PCC) or Spectrophotometric (SPCC)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PccMode {
    /// Classic Photometric Color Calibration (Ensemble aperture photometry + blackbody locus)
    Pcc,
    /// Spectrophotometric Color Calibration (Sensor QE curve + Filter bandpass + White reference spectrum)
    Spcc,
}

#[allow(dead_code)]
impl PccMode {
    pub fn label(&self) -> &'static str {
        match self {
            PccMode::Pcc => "PCC (Fotometrinen)",
            PccMode::Spcc => "SPCC (Spektrofotometrinen)",
        }
    }
}

/// Astrophysical white reference spectrum for PCC / SPCC
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WhiteReference {
    /// Average Spiral Galaxy (Sb/Sc ~5600 K) - PixInsight SPCC standard white reference
    AverageSpiralGalaxy,
    /// G2V Solar-type star (5778 K) - Classical photometric white point
    G2VStar,
    /// A0V Vega star (9600 K) - Photometric zero-point reference
    A0VVega,
    /// Elliptical Galaxy (E0 ~4600 K) - Older stellar population reference
    EllipticalGalaxy,
}

impl WhiteReference {
    pub fn label(&self) -> &'static str {
        match self {
            WhiteReference::AverageSpiralGalaxy => "Keskimääräinen spiraaligalaksi (5600 K)",
            WhiteReference::G2VStar => "G2V Auringon kaltainen tähti (5778 K)",
            WhiteReference::A0VVega => "A0V Vega -referenssi (9600 K)",
            WhiteReference::EllipticalGalaxy => "Elliptinen galaksi (4600 K)",
        }
    }

    pub fn effective_temp_k(&self) -> f32 {
        match self {
            WhiteReference::AverageSpiralGalaxy => 5600.0,
            WhiteReference::G2VStar => 5778.0,
            WhiteReference::A0VVega => 9600.0,
            WhiteReference::EllipticalGalaxy => 4600.0,
        }
    }

    /// Returns target (R/G, B/G) continuum flux ratios relative to G2V neutral white
    pub fn target_rg_bg_ratios(&self) -> (f32, f32) {
        match self {
            WhiteReference::AverageSpiralGalaxy => (1.022, 0.978),
            WhiteReference::G2VStar => (1.000, 1.000),
            WhiteReference::A0VVega => (0.875, 1.160),
            WhiteReference::EllipticalGalaxy => (1.125, 0.865),
        }
    }
}

/// Camera sensor quantum efficiency (QE) profile for SPCC
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SensorProfile {
    /// ZWO Seestar S50 (Sony IMX462 STARVIS - high Red/NIR sensitivity)
    SeestarS50Imx462,
    /// ZWO Seestar S30 (Sony IMX662 STARVIS 2 - ultra-low read noise, high G/R QE)
    SeestarS30Imx662,
    /// ZWO ASI533MC / ASI2600MC (Sony IMX533 / IMX571 BSI)
    SonyImx533Imx571,
    /// Generic OSC / DSLR Bayer RGB sensor
    GenericOsc,
}

impl SensorProfile {
    pub fn label(&self) -> &'static str {
        match self {
            SensorProfile::SeestarS50Imx462 => "Seestar S50 (Sony IMX462)",
            SensorProfile::SeestarS30Imx662 => "Seestar S30 (Sony IMX662)",
            SensorProfile::SonyImx533Imx571 => "ZWO ASI533 / ASI2600 (IMX533/571)",
            SensorProfile::GenericOsc => "Yleinen OSC / DSLR (Standard RGB)",
        }
    }

    /// Relative spectral efficiency correction factors (R/G, B/G) for emission/continuum balance
    pub fn spcc_qe_correction(&self) -> (f32, f32) {
        match self {
            // IMX462 has elevated red response relative to blue; slight boost to B/G balances continuum + emission
            SensorProfile::SeestarS50Imx462 => (0.975, 1.045),
            // IMX662 STARVIS 2 has very balanced G/R and improved blue QE
            SensorProfile::SeestarS30Imx662 => (0.985, 1.030),
            // IMX533 / IMX571 BSI sensors
            SensorProfile::SonyImx533Imx571 => (0.990, 1.020),
            SensorProfile::GenericOsc => (1.000, 1.000),
        }
    }
}

/// Optical filter transmission profile for SPCC
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpccFilterProfile {
    /// Standard UV/IR Cut broadband filter (400 - 700 nm)
    UvIrCut,
    /// Seestar internal Dual-Band LP filter (OIII 30nm @ 500.7nm + Ha 20nm @ 656.3nm)
    SeestarDualBandLp,
    /// Narrower Dual-Band filter (Optolong L-eNhance / L-eXtreme)
    NarrowDualBand,
}

impl SpccFilterProfile {
    pub fn label(&self) -> &'static str {
        match self {
            SpccFilterProfile::UvIrCut => "UV/IR Cut (Broadband / Laajakaista)",
            SpccFilterProfile::SeestarDualBandLp => "Seestar LP Dual-Band (OIII 30nm + Ha 20nm)",
            SpccFilterProfile::NarrowDualBand => "Kapea Dual-Band (L-eXtreme / L-eNhance)",
        }
    }

    /// Bandpass width & transmission correction (R/G, B/G) when calibrating on continuum stars
    pub fn spcc_bandpass_correction(&self) -> (f32, f32) {
        match self {
            SpccFilterProfile::UvIrCut => (1.000, 1.000),
            // In Seestar Dual-Band (20nm Ha vs 30nm OIII), continuum stars pass 1.5x more photons in OIII
            // than Ha relative to narrowband emission lines; compensating preserves natural Ha/OIII ratio
            SpccFilterProfile::SeestarDualBandLp => (1.140, 0.960),
            SpccFilterProfile::NarrowDualBand => (1.180, 0.950),
        }
    }
}

/// Configuration for PCC / SPCC calibration
#[derive(Clone, Debug)]
pub struct PccConfig {
    pub mode: PccMode,
    pub white_reference: WhiteReference,
    pub sensor: SensorProfile,
    pub filter: SpccFilterProfile,
    pub neutralize_background: bool,
    pub max_stars: usize,
    pub aperture_radius: f32,
    pub annulus_inner: f32,
    pub annulus_outer: f32,
    pub saturation_limit: f32,
}

impl Default for PccConfig {
    fn default() -> Self {
        Self {
            mode: PccMode::Spcc,
            white_reference: WhiteReference::AverageSpiralGalaxy,
            sensor: SensorProfile::SeestarS30Imx662,
            filter: SpccFilterProfile::UvIrCut,
            neutralize_background: true,
            max_stars: 120,
            aperture_radius: 4.5,
            annulus_inner: 6.5,
            annulus_outer: 10.5,
            saturation_limit: 0.92,
        }
    }
}

impl PccConfig {
    /// Automatically detects sensor and filter profile from FITS metadata if available
    pub fn auto_detect_from_metadata(&mut self, meta: &ImageMetadata) -> String {
        let mut detected_notes = Vec::new();

        let scope_cam = format!(
            "{} {} {}",
            meta.telescope.as_deref().unwrap_or(""),
            meta.camera.as_deref().unwrap_or(""),
            meta.file_name
        )
        .to_uppercase();

        if scope_cam.contains("S30") || scope_cam.contains("IMX662") {
            self.sensor = SensorProfile::SeestarS30Imx662;
            detected_notes.push("Seestar S30 (IMX662)");
        } else if scope_cam.contains("S50") || scope_cam.contains("SEESTAR") || scope_cam.contains("IMX462") {
            self.sensor = SensorProfile::SeestarS50Imx462;
            detected_notes.push("Seestar S50 (IMX462)");
        } else if scope_cam.contains("533") || scope_cam.contains("2600") || scope_cam.contains("571") {
            self.sensor = SensorProfile::SonyImx533Imx571;
            detected_notes.push("ZWO IMX533/571");
        }

        if let Some(filt) = &meta.filter_name {
            let f_up = filt.to_uppercase();
            if f_up.contains("LP") || f_up.contains("DUAL") || f_up.contains("OIII") || f_up.contains("HA") {
                self.filter = SpccFilterProfile::SeestarDualBandLp;
                detected_notes.push("Dual-Band LP -suodin");
            } else if f_up.contains("EXTREME") || f_up.contains("ENHANCE") {
                self.filter = SpccFilterProfile::NarrowDualBand;
                detected_notes.push("Kapea Dual-Band -suodin");
            } else {
                self.filter = SpccFilterProfile::UvIrCut;
                detected_notes.push("UV/IR Cut (Broadband)");
            }
        }

        if detected_notes.is_empty() {
            "FITS-otsikosta ei löytynyt erillistä laiteprofiilia, käytetään nykyisiä asetuksia.".to_string()
        } else {
            format!("Tunnistettu FITS-otsikosta: {}", detected_notes.join(" + "))
        }
    }
}

/// Diagnostic result returned by PCC / SPCC calibration
#[allow(dead_code)]
#[derive(Clone)]
pub struct PccResult {
    pub image: AstroImage,
    /// Total candidate stars detected in the field
    pub detected_stars: usize,
    /// Stars that passed unsaturated aperture photometry & outlier rejection
    pub valid_stars: usize,
    /// Applied Red channel multiplier (normalized so G = 1.000)
    pub k_r: f32,
    /// Applied Green channel multiplier (1.000)
    pub k_g: f32,
    /// Applied Blue channel multiplier (normalized so G = 1.000)
    pub k_b: f32,
    /// Original background medians (R, G, B)
    pub bg_original: [f32; 3],
    /// Neutralized background target level
    pub bg_neutral: f32,
    /// Estimated mean correlated color temperature (Kelvin) of the star sample
    pub mean_star_temp_k: f32,
    /// RMS dispersion of the photometric fit
    pub fit_rms: f32,
}

#[derive(Clone, Copy, Debug)]
struct StarPhotometry {
    log_rg: f32,
    log_bg: f32,
    snr_weight: f32,
}

pub struct PhotometricColorCalibrator;

impl PhotometricColorCalibrator {
    /// Performs Photometric (PCC) or Spectrophotometric (SPCC) Color Calibration on an RGB AstroImage.
    pub fn calibrate(image: &AstroImage, config: &PccConfig) -> Result<PccResult, String> {
        if !image.is_rgb() {
            return Err("PCC / SPCC vaatii 3-kanavaisen RGB-värikuvan (tee ensin Debayer tai Kanavayhdistys).".to_string());
        }

        let width = image.width;
        let height = image.height;
        if width < 16 || height < 16 {
            return Err("Kuva on liian pieni fotometriseen värikalibrointiin.".to_string());
        }

        // 1. Estimate star-free sky background medians per channel
        let (bg_r, mad_r) = Self::estimate_sky_background(image, 0);
        let (bg_g, mad_g) = Self::estimate_sky_background(image, 1);
        let (bg_b, mad_b) = Self::estimate_sky_background(image, 2);
        let bg_neutral = ((bg_r + bg_g + bg_b) / 3.0).clamp(0.0, 0.35);

        // 2. Detect stars and perform local aperture + annulus photometry
        let candidates = StarDetector::detect(image, config.max_stars.max(20));
        let detected_stars = candidates.len();

        let r_ap = config.aperture_radius.max(2.0);
        let r_in = config.annulus_inner.max(r_ap + 1.5);
        let r_out = config.annulus_outer.max(r_in + 2.5);
        let margin = (r_out.ceil() as usize) + 2;

        let mut measurements: Vec<StarPhotometry> = Vec::with_capacity(detected_stars);

        for star in &candidates {
            let cx = star.x;
            let cy = star.y;
            let ix = cx.round() as usize;
            let iy = cy.round() as usize;

            if ix < margin || ix + margin >= width || iy < margin || iy + margin >= height {
                continue;
            }

            let x0 = ix.saturating_sub(margin);
            let x1 = (ix + margin).min(width - 1);
            let y0 = iy.saturating_sub(margin);
            let y1 = (iy + margin).min(height - 1);

            let mut ap_pixels: Vec<[f32; 3]> = Vec::new();
            let mut ann_r: Vec<f32> = Vec::new();
            let mut ann_g: Vec<f32> = Vec::new();
            let mut ann_b: Vec<f32> = Vec::new();
            let mut saturated = false;

            for y in y0..=y1 {
                let dy = (y as f32) - cy;
                for x in x0..=x1 {
                    let dx = (x as f32) - cx;
                    let dist = (dx * dx + dy * dy).sqrt();
                    let px = image.get_pixel(x, y);
                    let (pr, pg, pb) = (px[0], px[1], px[2]);

                    if dist <= r_ap {
                        if pr >= config.saturation_limit
                            || pg >= config.saturation_limit
                            || pb >= config.saturation_limit
                        {
                            saturated = true;
                            break;
                        }
                        ap_pixels.push([pr, pg, pb]);
                    } else if dist >= r_in && dist <= r_out && (pr > 0.0 || pg > 0.0 || pb > 0.0) {
                        ann_r.push(pr);
                        ann_g.push(pg);
                        ann_b.push(pb);
                    }
                }
                if saturated {
                    break;
                }
            }

            if saturated || ap_pixels.len() < 5 || ann_r.len() < 8 {
                continue;
            }

            // Local sky annulus medians
            let loc_bg_r = Self::fast_median(&mut ann_r);
            let loc_bg_g = Self::fast_median(&mut ann_g);
            let loc_bg_b = Self::fast_median(&mut ann_b);

            // Integrate net star flux inside aperture
            let mut flux_r = 0.0f32;
            let mut flux_g = 0.0f32;
            let mut flux_b = 0.0f32;

            for &[pr, pg, pb] in &ap_pixels {
                flux_r += (pr - loc_bg_r).max(0.0);
                flux_g += (pg - loc_bg_g).max(0.0);
                flux_b += (pb - loc_bg_b).max(0.0);
            }

            let min_flux = (mad_g * 6.0).max(1e-4);
            if flux_r > min_flux && flux_g > min_flux && flux_b > min_flux {
                let rg = (flux_r / flux_g).clamp(0.15, 6.5);
                let bg = (flux_b / flux_g).clamp(0.15, 6.5);
                let snr_weight = (flux_g / min_flux).sqrt().clamp(1.0, 20.0);

                measurements.push(StarPhotometry {
                    log_rg: rg.ln(),
                    log_bg: bg.ln(),
                    snr_weight,
                });
            }
        }

        // 3. Robust ensemble fitting with MAD outlier rejection
        let (meas_rg, meas_bg, valid_stars, fit_rms) = if measurements.len() >= 3 {
            Self::robust_ensemble_fit(&measurements)
        } else {
            // Fallback for images with very few point stars: use unsaturated bright structure flux above sky background
            let (fb_rg, fb_bg, count) = Self::fallback_structure_flux_ratios(
                image,
                [bg_r, bg_g, bg_b],
                [mad_r, mad_g, mad_b],
                config.saturation_limit,
            );
            (fb_rg, fb_bg, count.min(measurements.len()), 0.08)
        };

        // 4. Compute theoretical target (R/G, B/G) ratios from WhiteReference + SPCC Sensor & Filter profiles
        let (mut target_rg, mut target_bg) = config.white_reference.target_rg_bg_ratios();

        if config.mode == PccMode::Spcc {
            let (qe_rg, qe_bg) = config.sensor.spcc_qe_correction();
            let (filt_rg, filt_bg) = config.filter.spcc_bandpass_correction();
            target_rg *= qe_rg * filt_rg;
            target_bg *= qe_bg * filt_bg;
        }

        // Compute normalized white balance multipliers relative to Green (k_g = 1.0)
        let k_r = (target_rg / meas_rg.max(1e-4)).clamp(0.35, 2.85);
        let k_g = 1.000f32;
        let k_b = (target_bg / meas_bg.max(1e-4)).clamp(0.35, 2.85);

        // Estimate average stellar temperature (Kelvin) from calibrated color ratios
        let mean_star_temp_k = Self::estimate_temperature_kelvin(
            meas_rg * k_r,
            meas_bg * k_b,
            config.white_reference.effective_temp_k(),
        );

        // 5. Apply background neutralization and photometric color calibration to image
        let mut out = image.clone();
        let do_bg_norm = config.neutralize_background;

        out.data.par_chunks_exact_mut(3).for_each(|pixel| {
            let r = pixel[0];
            let g = pixel[1];
            let b = pixel[2];

            if r <= 0.0 && g <= 0.0 && b <= 0.0 {
                return;
            }

            if do_bg_norm {
                // Preserve noise distribution symmetrically around neutral background while scaling signal
                let dr = r - bg_r;
                let dg = g - bg_g;
                let db = b - bg_b;

                let scale_r = if dr >= 0.0 { k_r } else { k_r.min(1.25) };
                let scale_b = if db >= 0.0 { k_b } else { k_b.min(1.25) };

                pixel[0] = (bg_neutral + dr * scale_r).clamp(0.0, 1.0);
                pixel[1] = (bg_neutral + dg * k_g).clamp(0.0, 1.0);
                pixel[2] = (bg_neutral + db * scale_b).clamp(0.0, 1.0);
            } else {
                pixel[0] = (r * k_r).clamp(0.0, 1.0);
                pixel[1] = (g * k_g).clamp(0.0, 1.0);
                pixel[2] = (b * k_b).clamp(0.0, 1.0);
            }
        });

        Ok(PccResult {
            image: out,
            detected_stars,
            valid_stars,
            k_r,
            k_g,
            k_b,
            bg_original: [bg_r, bg_g, bg_b],
            bg_neutral,
            mean_star_temp_k,
            fit_rms,
        })
    }

    /// Estimates star-free sky background median and MAD using 2-pass sigma clipping
    fn estimate_sky_background(image: &AstroImage, channel: usize) -> (f32, f32) {
        let (med0, mad0) = image.channel_median_and_mad(channel);
        let low = (med0 - 2.5 * mad0).max(0.0);
        let high = med0 + 2.5 * mad0;

        let step = if image.width * image.height > 1_000_000 { 4 } else { 1 };
        let mut sky_samples: Vec<f32> = Vec::with_capacity((image.width * image.height) / step);

        for (i, px) in image.data.chunks_exact(image.channels).enumerate() {
            if i % step == 0 {
                let v = px[channel];
                if v > 0.0 && v >= low && v <= high {
                    sky_samples.push(v);
                }
            }
        }

        if sky_samples.len() < 16 {
            return (med0, mad0.max(1e-5));
        }

        let med = Self::fast_median(&mut sky_samples);
        let mut diffs: Vec<f32> = sky_samples.iter().map(|&v| (v - med).abs()).collect();
        let mad = Self::fast_median(&mut diffs).max(1e-5);
        (med, mad)
    }

    fn fast_median(values: &mut [f32]) -> f32 {
        if values.is_empty() {
            return 0.0;
        }
        let mid = values.len() / 2;
        values.select_nth_unstable_by(mid, |a, b| {
            a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
        });
        values[mid]
    }

    /// Computes MAD-clipped SNR-weighted geometric mean of (R/G, B/G) across the stellar ensemble
    fn robust_ensemble_fit(stars: &[StarPhotometry]) -> (f32, f32, usize, f32) {
        let mut log_rgs: Vec<f32> = stars.iter().map(|s| s.log_rg).collect();
        let mut log_bgs: Vec<f32> = stars.iter().map(|s| s.log_bg).collect();

        let med_rg = Self::fast_median(&mut log_rgs);
        let med_bg = Self::fast_median(&mut log_bgs);

        let mut dev_rg: Vec<f32> = stars.iter().map(|s| (s.log_rg - med_rg).abs()).collect();
        let mut dev_bg: Vec<f32> = stars.iter().map(|s| (s.log_bg - med_bg).abs()).collect();

        // 1.4826 converts MAD to standard deviation equivalent
        let sigma_rg = (Self::fast_median(&mut dev_rg) * 1.4826).max(0.08);
        let sigma_bg = (Self::fast_median(&mut dev_bg) * 1.4826).max(0.08);

        let mut sum_w = 0.0f32;
        let mut sum_rg = 0.0f32;
        let mut sum_bg = 0.0f32;
        let mut sum_sq_err = 0.0f32;
        let mut inliers = 0usize;

        for s in stars {
            let z_rg = (s.log_rg - med_rg).abs() / sigma_rg;
            let z_bg = (s.log_bg - med_bg).abs() / sigma_bg;

            // Reject extreme carbon stars / emission knots outside 2.5 sigma
            if z_rg <= 2.5 && z_bg <= 2.5 {
                let w = s.snr_weight;
                sum_w += w;
                sum_rg += w * s.log_rg;
                sum_bg += w * s.log_bg;
                let err = (s.log_rg - med_rg).powi(2) + (s.log_bg - med_bg).powi(2);
                sum_sq_err += err;
                inliers += 1;
            }
        }

        if inliers == 0 || sum_w <= 0.0 {
            return (med_rg.exp(), med_bg.exp(), stars.len(), 0.05);
        }

        let mean_log_rg = sum_rg / sum_w;
        let mean_log_bg = sum_bg / sum_w;
        let rms = (sum_sq_err / (inliers as f32)).sqrt();

        (mean_log_rg.exp(), mean_log_bg.exp(), inliers, rms)
    }

    /// Fallback flux estimator if an image has fewer than 3 detected point stars
    fn fallback_structure_flux_ratios(
        image: &AstroImage,
        bg: [f32; 3],
        mad: [f32; 3],
        sat_limit: f32,
    ) -> (f32, f32, usize) {
        let mut sum_r = 0.0f64;
        let mut sum_g = 0.0f64;
        let mut sum_b = 0.0f64;
        let mut count = 0usize;

        let thresh_r = bg[0] + 3.0 * mad[0];
        let thresh_g = bg[1] + 3.0 * mad[1];
        let thresh_b = bg[2] + 3.0 * mad[2];

        for px in image.data.chunks_exact(3) {
            let (r, g, b) = (px[0], px[1], px[2]);
            if r < sat_limit
                && g < sat_limit
                && b < sat_limit
                && (r > thresh_r || g > thresh_g || b > thresh_b)
            {
                sum_r += (r - bg[0]).max(0.0) as f64;
                sum_g += (g - bg[1]).max(0.0) as f64;
                sum_b += (b - bg[2]).max(0.0) as f64;
                count += 1;
            }
        }

        if count > 0 && sum_g > 1e-5 {
            let rg = ((sum_r / sum_g) as f32).clamp(0.3, 3.0);
            let bg_ratio = ((sum_b / sum_g) as f32).clamp(0.3, 3.0);
            (rg, bg_ratio, count)
        } else {
            (1.0, 1.0, 0)
        }
    }

    /// Estimates effective correlated color temperature (Kelvin) using Ballesteros' approximation
    fn estimate_temperature_kelvin(cal_rg: f32, cal_bg: f32, ref_temp: f32) -> f32 {
        // Synthetic B-V color index relative to G2V (B-V = 0.656)
        let color_ratio = (cal_bg / cal_rg.max(0.1)).clamp(0.25, 4.0);
        let b_minus_v = (0.656 - 1.15 * color_ratio.ln()).clamp(-0.35, 2.0);

        // Ballesteros (2012) formula for stellar temperature from (B-V)
        let t_ballesteros = 4600.0
            * (1.0 / (0.92 * b_minus_v + 1.7) + 1.0 / (0.92 * b_minus_v + 0.62));
        ((t_ballesteros * 0.5) + (ref_temp * 0.5)).clamp(3200.0, 18000.0)
    }
}
