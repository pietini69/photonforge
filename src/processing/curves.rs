use crate::core::image::AstroImage;
use rayon::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurvePoint {
    pub x: f32, // 0.0 ..= 1.0
    pub y: f32, // 0.0 ..= 1.0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurvesChannel {
    Rgb,        // Master RGB / Luminance
    Red,        // Red
    Green,      // Green
    Blue,       // Blue
    Saturation, // Saturation vs Luminance
}

impl CurvesChannel {
    pub fn name(&self) -> &'static str {
        match self {
            CurvesChannel::Rgb => "RGB (Kaikki)",
            CurvesChannel::Red => "Punainen (R)",
            CurvesChannel::Green => "Vihreä (G)",
            CurvesChannel::Blue => "Sininen (B)",
            CurvesChannel::Saturation => "Kylläisyys (S)",
        }
    }

    pub fn color_rgb(&self) -> (u8, u8, u8) {
        match self {
            CurvesChannel::Rgb => (240, 240, 255),
            CurvesChannel::Red => (255, 80, 80),
            CurvesChannel::Green => (80, 240, 110),
            CurvesChannel::Blue => (90, 180, 255),
            CurvesChannel::Saturation => (255, 210, 60),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SplineCurve {
    pub points: Vec<CurvePoint>,
}

impl Default for SplineCurve {
    fn default() -> Self {
        Self::new_linear()
    }
}

impl SplineCurve {
    pub fn new_linear() -> Self {
        Self {
            points: vec![
                CurvePoint { x: 0.0, y: 0.0 },
                CurvePoint { x: 1.0, y: 1.0 },
            ],
        }
    }

    pub fn is_identity(&self) -> bool {
        if self.points.len() == 2 {
            (self.points[0].x - 0.0).abs() < 1e-4
                && (self.points[0].y - 0.0).abs() < 1e-4
                && (self.points[1].x - 1.0).abs() < 1e-4
                && (self.points[1].y - 1.0).abs() < 1e-4
        } else {
            self.points.iter().all(|p| (p.x - p.y).abs() < 1e-4)
        }
    }

    pub fn reset(&mut self) {
        self.points = vec![
            CurvePoint { x: 0.0, y: 0.0 },
            CurvePoint { x: 1.0, y: 1.0 },
        ];
    }

    pub fn add_point(&mut self, x: f32, y: f32) -> usize {
        let x_clamped = x.clamp(0.01, 0.99);
        let y_clamped = y.clamp(0.0, 1.0);

        // Don't add if very close to existing point
        for (i, p) in self.points.iter().enumerate() {
            if (p.x - x_clamped).abs() < 0.02 {
                self.points[i].y = y_clamped;
                return i;
            }
        }

        let insert_idx = self
            .points
            .iter()
            .position(|p| p.x > x_clamped)
            .unwrap_or(self.points.len() - 1);

        self.points.insert(
            insert_idx,
            CurvePoint {
                x: x_clamped,
                y: y_clamped,
            },
        );
        insert_idx
    }

    pub fn remove_point(&mut self, index: usize) -> bool {
        if index > 0 && index < self.points.len() - 1 {
            self.points.remove(index);
            true
        } else {
            false
        }
    }

    /// Evaluates the curve using Monotone Piecewise Cubic Hermite Interpolating Polynomial (PCHIP).
    /// Guarantees that the curve stays smooth, respects monotonicity, and never overshoots.
    pub fn evaluate_pchip(&self, x: f32) -> f32 {
        let n = self.points.len();
        if n == 0 {
            return x;
        }
        if n == 1 {
            return self.points[0].y;
        }

        let x_clamp = x.clamp(0.0, 1.0);
        if x_clamp <= self.points[0].x {
            return self.points[0].y;
        }
        if x_clamp >= self.points[n - 1].x {
            return self.points[n - 1].y;
        }

        if n == 2 {
            let p0 = self.points[0];
            let p1 = self.points[1];
            let t = (x_clamp - p0.x) / (p1.x - p0.x).max(1e-6);
            return (p0.y + t * (p1.y - p0.y)).clamp(0.0, 1.0);
        }

        // Interval widths and slopes
        let mut h = vec![0.0f32; n - 1];
        let mut delta = vec![0.0f32; n - 1];
        for k in 0..n - 1 {
            h[k] = (self.points[k + 1].x - self.points[k].x).max(1e-6);
            delta[k] = (self.points[k + 1].y - self.points[k].y) / h[k];
        }

        // Internal tangents (standard PCHIP harmonic mean)
        let mut d = vec![0.0f32; n];
        for k in 1..n - 1 {
            if delta[k - 1] * delta[k] <= 0.0 {
                d[k] = 0.0;
            } else {
                let w1 = 2.0 * h[k] + h[k - 1];
                let w2 = h[k] + 2.0 * h[k - 1];
                d[k] = (w1 + w2) / (w1 / delta[k - 1] + w2 / delta[k]);
            }
        }

        // Endpoint tangents
        d[0] = delta[0];
        if d[0] * delta[0] <= 0.0 {
            d[0] = 0.0;
        } else if delta.len() > 1 && delta[0] * delta[1] <= 0.0 {
            d[0] = d[0].clamp(-3.0 * delta[0].abs(), 3.0 * delta[0].abs());
        }

        d[n - 1] = delta[n - 2];
        if d[n - 1] * delta[n - 2] <= 0.0 {
            d[n - 1] = 0.0;
        }

        // Find interval k
        let mut k = 0;
        for i in 0..n - 1 {
            if x_clamp >= self.points[i].x && x_clamp <= self.points[i + 1].x {
                k = i;
                break;
            }
        }

        let hk = h[k];
        let t = (x_clamp - self.points[k].x) / hk;
        let t2 = t * t;
        let t3 = t2 * t;

        let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
        let h10 = t3 - 2.0 * t2 + t;
        let h01 = -2.0 * t3 + 3.0 * t2;
        let h11 = t3 - t2;

        let y = h00 * self.points[k].y
            + h10 * hk * d[k]
            + h01 * self.points[k + 1].y
            + h11 * hk * d[k + 1];

        y.clamp(0.0, 1.0)
    }

    /// Precomputes a 4096-element floating-point Look-Up Table for fast evaluation.
    pub fn generate_lut_4096(&self) -> [f32; 4096] {
        let mut lut = [0.0f32; 4096];
        for i in 0..4096 {
            let x = i as f32 / 4095.0;
            lut[i] = self.evaluate_pchip(x);
        }
        lut
    }

    /// Precomputes a 256-element byte Look-Up Table for fast 8-bit live viewport display.
    pub fn generate_lut_256(&self) -> [u8; 256] {
        let mut lut = [0u8; 256];
        for i in 0..256 {
            let x = i as f32 / 255.0;
            let y = (self.evaluate_pchip(x) * 255.0).round().clamp(0.0, 255.0);
            lut[i] = y as u8;
        }
        lut
    }

    // Presets
    pub fn preset_s_curve() -> Self {
        Self {
            points: vec![
                CurvePoint { x: 0.0, y: 0.0 },
                CurvePoint { x: 0.25, y: 0.18 },
                CurvePoint { x: 0.75, y: 0.82 },
                CurvePoint { x: 1.0, y: 1.0 },
            ],
        }
    }

    pub fn preset_lift_midtones() -> Self {
        Self {
            points: vec![
                CurvePoint { x: 0.0, y: 0.0 },
                CurvePoint { x: 0.30, y: 0.48 },
                CurvePoint { x: 1.0, y: 1.0 },
            ],
        }
    }

    pub fn preset_darken_shadows() -> Self {
        Self {
            points: vec![
                CurvePoint { x: 0.0, y: 0.0 },
                CurvePoint { x: 0.18, y: 0.10 },
                CurvePoint { x: 0.60, y: 0.60 },
                CurvePoint { x: 1.0, y: 1.0 },
            ],
        }
    }

    pub fn preset_high_contrast() -> Self {
        Self {
            points: vec![
                CurvePoint { x: 0.0, y: 0.0 },
                CurvePoint { x: 0.20, y: 0.10 },
                CurvePoint { x: 0.70, y: 0.85 },
                CurvePoint { x: 1.0, y: 1.0 },
            ],
        }
    }

    pub fn preset_saturation_boost() -> Self {
        Self {
            points: vec![
                CurvePoint { x: 0.0, y: 0.20 }, // keep dark noise desaturated
                CurvePoint { x: 0.20, y: 0.35 },
                CurvePoint { x: 0.50, y: 0.65 }, // boost colorful nebulosity
                CurvePoint { x: 0.85, y: 0.70 },
                CurvePoint { x: 1.0, y: 0.60 },
            ],
        }
    }
}

#[inline(always)]
pub fn eval_lut_4096(lut: &[f32; 4096], x: f32) -> f32 {
    let scaled = (x * 4095.0).clamp(0.0, 4095.0);
    let idx = scaled.floor() as usize;
    if idx >= 4095 {
        lut[4095]
    } else {
        let frac = scaled - idx as f32;
        lut[idx] * (1.0 - frac) + lut[idx + 1] * frac
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CurvesSet {
    pub rgb: SplineCurve,
    pub red: SplineCurve,
    pub green: SplineCurve,
    pub blue: SplineCurve,
    pub saturation: SplineCurve,
}

impl Default for CurvesSet {
    fn default() -> Self {
        Self {
            rgb: SplineCurve::new_linear(),
            red: SplineCurve::new_linear(),
            green: SplineCurve::new_linear(),
            blue: SplineCurve::new_linear(),
            saturation: SplineCurve::new_linear(),
        }
    }
}

impl CurvesSet {
    pub fn is_identity(&self) -> bool {
        self.rgb.is_identity()
            && self.red.is_identity()
            && self.green.is_identity()
            && self.blue.is_identity()
            && self.saturation.is_identity()
    }

    pub fn reset_all(&mut self) {
        self.rgb.reset();
        self.red.reset();
        self.green.reset();
        self.blue.reset();
        self.saturation.reset();
    }

    pub fn get_curve(&self, channel: CurvesChannel) -> &SplineCurve {
        match channel {
            CurvesChannel::Rgb => &self.rgb,
            CurvesChannel::Red => &self.red,
            CurvesChannel::Green => &self.green,
            CurvesChannel::Blue => &self.blue,
            CurvesChannel::Saturation => &self.saturation,
        }
    }

    pub fn get_curve_mut(&mut self, channel: CurvesChannel) -> &mut SplineCurve {
        match channel {
            CurvesChannel::Rgb => &mut self.rgb,
            CurvesChannel::Red => &mut self.red,
            CurvesChannel::Green => &mut self.green,
            CurvesChannel::Blue => &mut self.blue,
            CurvesChannel::Saturation => &mut self.saturation,
        }
    }

    /// Fast 8-bit LUTs for real-time live preview on viewport canvas
    pub fn generate_rgb8_luts(&self) -> ([u8; 256], [u8; 256], [u8; 256]) {
        let rgb_lut = self.rgb.generate_lut_256();
        let r_lut = self.red.generate_lut_256();
        let g_lut = self.green.generate_lut_256();
        let b_lut = self.blue.generate_lut_256();

        let mut final_r = [0u8; 256];
        let mut final_g = [0u8; 256];
        let mut final_b = [0u8; 256];

        for i in 0..256 {
            final_r[i] = rgb_lut[r_lut[i] as usize];
            final_g[i] = rgb_lut[g_lut[i] as usize];
            final_b[i] = rgb_lut[b_lut[i] as usize];
        }

        (final_r, final_g, final_b)
    }

    /// Applies curves in real-time to an 8-bit RGB display buffer in milliseconds.
    pub fn apply_to_rgb8(&self, rgb_data: &mut [u8]) {
        if self.is_identity() {
            return;
        }

        let (r_lut, g_lut, b_lut) = self.generate_rgb8_luts();
        let has_sat = !self.saturation.is_identity();
        let sat_lut = if has_sat {
            Some(self.saturation.generate_lut_4096())
        } else {
            None
        };

        rgb_data
            .par_chunks_exact_mut(3)
            .for_each(|pixel| {
                let mut r = r_lut[pixel[0] as usize] as f32 / 255.0;
                let mut g = g_lut[pixel[1] as usize] as f32 / 255.0;
                let mut b = b_lut[pixel[2] as usize] as f32 / 255.0;

                if let Some(ref s_lut) = sat_lut {
                    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                    let factor = eval_lut_4096(s_lut, y) * 2.0;
                    r = (y + factor * (r - y)).clamp(0.0, 1.0);
                    g = (y + factor * (g - y)).clamp(0.0, 1.0);
                    b = (y + factor * (b - y)).clamp(0.0, 1.0);
                }

                pixel[0] = (r * 255.0).round() as u8;
                pixel[1] = (g * 255.0).round() as u8;
                pixel[2] = (b * 255.0).round() as u8;
            });
    }

    /// Permanently applies the CurvesSet to full 32-bit floating point image data.
    pub fn apply_to_image(&self, image: &AstroImage) -> AstroImage {
        let mut out = image.clone();
        if self.is_identity() {
            return out;
        }

        let rgb_lut = self.rgb.generate_lut_4096();
        let r_lut = self.red.generate_lut_4096();
        let g_lut = self.green.generate_lut_4096();
        let b_lut = self.blue.generate_lut_4096();
        let sat_lut = self.saturation.generate_lut_4096();

        let has_rgb = !self.rgb.is_identity();
        let has_r = !self.red.is_identity();
        let has_g = !self.green.is_identity();
        let has_b = !self.blue.is_identity();
        let has_sat = !self.saturation.is_identity() && image.channels == 3;

        out.data
            .par_chunks_exact_mut(image.channels)
            .for_each(|pixel| {
                if image.channels == 1 {
                    let mut val = pixel[0];
                    if has_r {
                        val = eval_lut_4096(&r_lut, val);
                    }
                    if has_rgb {
                        val = eval_lut_4096(&rgb_lut, val);
                    }
                    pixel[0] = val.clamp(0.0, 1.0);
                } else {
                    let mut r = pixel[0];
                    let mut g = pixel[1];
                    let mut b = pixel[2];

                    if has_r { r = eval_lut_4096(&r_lut, r); }
                    if has_g { g = eval_lut_4096(&g_lut, g); }
                    if has_b { b = eval_lut_4096(&b_lut, b); }

                    if has_rgb {
                        r = eval_lut_4096(&rgb_lut, r);
                        g = eval_lut_4096(&rgb_lut, g);
                        b = eval_lut_4096(&rgb_lut, b);
                    }

                    if has_sat {
                        let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                        let factor = eval_lut_4096(&sat_lut, y) * 2.0;
                        r = y + factor * (r - y);
                        g = y + factor * (g - y);
                        b = y + factor * (b - y);
                    }

                    pixel[0] = r.max(0.0);
                    pixel[1] = g.max(0.0);
                    pixel[2] = b.max(0.0);
                }
            });

        out
    }
}
