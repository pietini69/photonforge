use crate::core::image::AstroImage;

#[derive(Clone, Copy, Debug)]
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

#[derive(Clone, Debug)]
pub struct StfParameters {
    pub enabled: bool,
    pub linked_channels: bool,
    pub target_bg: f32,
    pub channels: [ChannelStf; 3],
}

impl Default for StfParameters {
    fn default() -> Self {
        Self {
            enabled: true,
            linked_channels: false, // Default unlinked for natural background neutralization on OSC/color
            target_bg: 0.10,        // PixInsight-style deep space background level (0.08 - 0.12)
            channels: [ChannelStf::default(), ChannelStf::default(), ChannelStf::default()],
        }
    }
}

impl StfParameters {
    /// Computes PixInsight-style Auto-STF parameters with default deep-space target background (0.10)
    pub fn compute_auto(image: &AstroImage, linked: bool) -> Self {
        Self::compute_auto_with_target(image, linked, 0.10)
    }

    /// Computes PixInsight-style Auto-STF parameters with customizable target background level
    pub fn compute_auto_with_target(image: &AstroImage, linked: bool, target_bg: f32) -> Self {
        let mut stf = Self {
            enabled: true,
            linked_channels: linked,
            target_bg: target_bg.clamp(0.01, 0.50),
            channels: [ChannelStf::default(), ChannelStf::default(), ChannelStf::default()],
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
