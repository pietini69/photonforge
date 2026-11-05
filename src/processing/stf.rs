use crate::core::image::AstroImage;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StretchMode {
    /// Pure raw linear 32-bit floating point directly mapped to screen (Linear display)
    Linear,
    /// Automated Screen Transfer Function (PixInsight-style Median + MAD)
    AutoStf,
    /// Manual Shadows / Midtones / Highlights MTF sliders
    ManualMtf,
    /// Color-preserving Arcsinh stretch (Lupton/Siril algorithm)
    Arcsinh,
    /// Riccardo Paterniti's VeraLux HyperMetric Stretch (HMS: Photometric Hyperbolic Stretch & True Color vector preservation)
    HyperMetric,
    /// Logarithmic stretch (for high dynamic range nebula/galaxy cores)
    Logarithmic,
    /// Generalized Hyperbolic Stretch (GHS-style localized contrast)
    Ghs,
    /// Root / Gamma soft stretch
    Root,
}

impl StretchMode {
    pub fn all() -> &'static [StretchMode] {
        &[
            StretchMode::Linear,
            StretchMode::AutoStf,
            StretchMode::ManualMtf,
            StretchMode::Arcsinh,
            StretchMode::HyperMetric,
            StretchMode::Logarithmic,
            StretchMode::Ghs,
            StretchMode::Root,
        ]
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            StretchMode::Linear => "🟢 Lineaarinen (Raaka)",
            StretchMode::AutoStf => "⚡ Auto-STF (Automaatti)",
            StretchMode::ManualMtf => "🎚 Manuaalinen MTF",
            StretchMode::Arcsinh => "🌈 Arcsinh (Värit säilyttävä)",
            StretchMode::HyperMetric => "✨ VeraLux HyperMetric (HMS)",
            StretchMode::Logarithmic => "📐 Logaritminen (HDR)",
            StretchMode::Ghs => "📈 Hyperbolinen (GHS)",
            StretchMode::Root => "🌿 Juuri (Gamma)",
        }
    }

    pub fn short_name(&self) -> &'static str {
        match self {
            StretchMode::Linear => "Lineaarinen",
            StretchMode::AutoStf => "Auto-STF",
            StretchMode::ManualMtf => "Manuaali-MTF",
            StretchMode::Arcsinh => "Arcsinh",
            StretchMode::HyperMetric => "HyperMetric",
            StretchMode::Logarithmic => "Logaritminen",
            StretchMode::Ghs => "GHS",
            StretchMode::Root => "Juuri",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChannelStf {
    pub shadows: f32,
    pub midtones: f32,
    pub highlights: f32,
}

impl Default for ChannelStf {
    fn default() -> Self {
        Self {
            shadows: 0.0,
            midtones: 0.5,
            highlights: 1.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StfParameters {
    pub mode: StretchMode,
    pub enabled: bool,
    pub linked_channels: bool,
    pub target_bg: f32,
    pub channels: [ChannelStf; 3],

    // Linear mode exposure adjustment (-4.0 .. +4.0 EV, default 0.0)
    pub linear_exposure_ev: f32,

    // Arcsinh stretch parameters
    pub arcsinh_stretch: f32, // 1.0 .. 1000.0, default 100.0
    pub arcsinh_black: f32,   // 0.0 .. 0.2, default 0.0

    // VeraLux HyperMetric Stretch (HMS) parameters
    pub hms_d: f32,          // Stretch Intensity D: 1.0 .. 500.0, default 40.0
    pub hms_b: f32,          // Softening / Linearity b: 0.001 .. 0.5, default 0.05
    pub hms_sp: f32,         // Shadow Point SP: 0.0 .. 0.2, default 0.0
    pub hms_color_grip: f32, // Vector color / chromatic grip: 0.2 .. 3.0, default 1.0

    // Logarithmic stretch parameters
    pub log_factor: f32,      // 10.0 .. 50000.0, default 1000.0
    pub log_black: f32,       // 0.0 .. 0.2, default 0.0

    // Generalized Hyperbolic Stretch parameters
    pub ghs_d: f32,           // 1.0 .. 50.0, default 10.0
    pub ghs_b: f32,           // 0.0 .. 0.5, default 0.05

    // Root / Gamma stretch parameters
    pub gamma: f32,           // 0.1 .. 1.0, default 0.5
    pub gamma_black: f32,     // 0.0 .. 0.2, default 0.0
}

impl Default for StfParameters {
    fn default() -> Self {
        Self {
            mode: StretchMode::AutoStf,
            enabled: true,
            linked_channels: false, // Default unlinked for natural background neutralization on OSC/color
            target_bg: 0.10,        // PixInsight-style deep space background level (0.08 - 0.12)
            channels: [ChannelStf::default(), ChannelStf::default(), ChannelStf::default()],
            linear_exposure_ev: 0.0,
            arcsinh_stretch: 100.0,
            arcsinh_black: 0.0,
            hms_d: 40.0,
            hms_b: 0.05,
            hms_sp: 0.0,
            hms_color_grip: 1.0,
            log_factor: 1000.0,
            log_black: 0.0,
            ghs_d: 10.0,
            ghs_b: 0.05,
            gamma: 0.5,
            gamma_black: 0.0,
        }
    }
}

impl StfParameters {
    pub fn is_linear(&self) -> bool {
        self.mode == StretchMode::Linear || !self.enabled
    }

    pub fn set_linear(&mut self) {
        self.mode = StretchMode::Linear;
        self.enabled = false;
    }

    pub fn set_mode(&mut self, mode: StretchMode) {
        self.mode = mode;
        if mode == StretchMode::Linear {
            self.enabled = false;
        } else {
            self.enabled = true;
        }
    }

    /// Computes PixInsight-style Auto-STF parameters with default deep-space target background (0.10)
    pub fn compute_auto(image: &AstroImage, linked: bool) -> Self {
        Self::compute_auto_with_target(image, linked, 0.10)
    }

    /// Computes PixInsight-style Auto-STF parameters with customizable target background level
    pub fn compute_auto_with_target(image: &AstroImage, linked: bool, target_bg: f32) -> Self {
        let mut stf = Self {
            mode: StretchMode::AutoStf,
            enabled: true,
            linked_channels: linked,
            target_bg: target_bg.clamp(0.01, 0.50),
            channels: [ChannelStf::default(), ChannelStf::default(), ChannelStf::default()],
            linear_exposure_ev: 0.0,
            arcsinh_stretch: 100.0,
            arcsinh_black: 0.0,
            hms_d: 40.0,
            hms_b: 0.05,
            hms_sp: 0.0,
            hms_color_grip: 1.0,
            log_factor: 1000.0,
            log_black: 0.0,
            ghs_d: 10.0,
            ghs_b: 0.05,
            gamma: 0.5,
            gamma_black: 0.0,
        };

        let target_bg = stf.target_bg;
        let shadows_clip = -2.8; // Number of MADs below median

        if image.is_mono() {
            let (median, mad) = image.channel_median_and_mad(0);
            let shadows = (median + shadows_clip * mad).max(0.0);
            let highlights = 1.0;

            let m = if (highlights - shadows) > 1e-5 {
                let norm_med = ((median - shadows) / (highlights - shadows)).clamp(0.001, 0.999);
                find_midtones_balance(target_bg, norm_med)
            } else {
                0.5
            };

            let ch = ChannelStf {
                shadows,
                midtones: m,
                highlights,
            };
            stf.channels = [ch, ch, ch];
        } else if linked {
            // Linked RGB mode: each channel preserves its own shadow clip to avoid color clipping,
            // but shares a common midtones curve to preserve raw color ratios.
            let mut norm_meds = [0.5f32; 3];
            for c in 0..image.channels.min(3) {
                let (median, mad) = image.channel_median_and_mad(c);
                let shadows = (median + shadows_clip * mad).max(0.0);
                let highlights = 1.0;
                stf.channels[c].shadows = shadows;
                stf.channels[c].highlights = highlights;

                if (highlights - shadows) > 1e-5 {
                    norm_meds[c] = ((median - shadows) / (highlights - shadows)).clamp(0.001, 0.999);
                }
            }

            let avg_norm = (norm_meds[0] + norm_meds[1] + norm_meds[2]) / 3.0;
            let common_m = find_midtones_balance(target_bg, avg_norm);
            for c in 0..3 {
                stf.channels[c].midtones = common_m;
            }
        } else {
            // Unlinked per channel: independent background neutralization (standard for raw OSC)
            for c in 0..image.channels.min(3) {
                let (median, mad) = image.channel_median_and_mad(c);
                let shadows = (median + shadows_clip * mad).max(0.0);
                let highlights = 1.0;

                let m = if (highlights - shadows) > 1e-5 {
                    let norm_med = ((median - shadows) / (highlights - shadows)).clamp(0.001, 0.999);
                    find_midtones_balance(target_bg, norm_med)
                } else {
                    0.5
                };

                stf.channels[c] = ChannelStf {
                    shadows,
                    midtones: m,
                    highlights,
                };
            }
        }

        let (first_med, first_mad) = image.channel_median_and_mad(0);
        stf.hms_sp = (first_med - 2.8 * first_mad).max(0.0);

        stf
    }
}

/// Finds midtone parameter 'm' such that MTF(m, x) = target
fn find_midtones_balance(target: f32, x: f32) -> f32 {
    if x <= 0.0 {
        return 0.5;
    }
    if x >= 1.0 {
        return 0.5;
    }
    if (x - target).abs() < 1e-6 {
        return 0.5;
    }

    // Solve MTF(m, x) = target for m:
    // (m - 1)*x / ((2m - 1)*x - m) = target
    // x*m - x = target * (2*x*m - x - m) = 2*target*x*m - target*x - target*m
    // m * (x - 2*target*x + target) = x - target*x
    // m = x*(1 - target) / (x + target - 2*target*x)
    let num = x * (1.0 - target);
    let den = x + target - 2.0 * target * x;
    if den.abs() < 1e-7 {
        0.5
    } else {
        (num / den).clamp(0.0001, 0.9999)
    }
}
