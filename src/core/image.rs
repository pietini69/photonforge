use rayon::prelude::*;

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChannelCount {
    Mono = 1,
    Rgb = 3,
}

#[derive(Clone)]
pub struct AstroImage {
    pub width: usize,
    pub height: usize,
    pub channels: usize,
    pub data: Vec<f32>, // Channel-planar or interleaved: we use interleaved [R, G, B, R, G, B...] or [M, M, M...]
    pub metadata: ImageMetadata,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Default)]
pub struct ImageMetadata {
    pub file_name: String,
    pub file_path: String,
    pub exposure_time: Option<f32>,
    pub gain: Option<f32>,
    pub iso: Option<u32>,
    pub target: Option<String>,
    pub telescope: Option<String>,
    pub camera: Option<String>,
    pub date_obs: Option<String>,
    pub bitpix: Option<i32>,
    pub bayer_pattern: Option<String>,
    pub filter_name: Option<String>,
}

#[allow(dead_code)]
impl AstroImage {
    pub fn new_mono(width: usize, height: usize, initial: f32) -> Self {
        Self {
            width,
            height,
            channels: 1,
            data: vec![initial; width * height],
            metadata: ImageMetadata::default(),
        }
    }

    pub fn new_rgb(width: usize, height: usize, initial: f32) -> Self {
        Self {
            width,
            height,
            channels: 3,
            data: vec![initial; width * height * 3],
            metadata: ImageMetadata::default(),
        }
    }

    pub fn is_mono(&self) -> bool {
        self.channels == 1
    }

    pub fn is_rgb(&self) -> bool {
        self.channels == 3
    }

    pub fn is_cfa(&self) -> bool {
        self.channels == 1 && self.metadata.bayer_pattern.is_some()
    }

    pub fn get_pixel(&self, x: usize, y: usize) -> &[f32] {
        let idx = (y * self.width + x) * self.channels;
        &self.data[idx..idx + self.channels]
    }

    pub fn get_pixel_mut(&mut self, x: usize, y: usize) -> &mut [f32] {
        let idx = (y * self.width + x) * self.channels;
        &mut self.data[idx..idx + self.channels]
    }

    /// Computes min and max pixel values across all channels
    pub fn min_max(&self) -> (f32, f32) {
        self.data
            .par_iter()
            .fold(
                || (f32::INFINITY, f32::NEG_INFINITY),
                |(min_val, max_val), &val| (min_val.min(val), max_val.max(val)),
            )
            .reduce(
                || (f32::INFINITY, f32::NEG_INFINITY),
                |(a_min, a_max), (b_min, b_max)| (a_min.min(b_min), a_max.max(b_max)),
            )
    }

    /// Computes channel-wise statistics (median and Median Absolute Deviation - MAD)
    pub fn channel_median_and_mad(&self, channel: usize) -> (f32, f32) {
        if self.width == 0 || self.height == 0 || channel >= self.channels {
            return (0.0, 0.0);
        }

        // Subsample for fast estimation if image is large
        let step = if self.width * self.height > 2_000_000 { 4 } else { 1 };
        let mut samples: Vec<f32> = Vec::with_capacity((self.width * self.height) / step);

        for (i, chunk) in self.data.chunks_exact(self.channels).enumerate() {
            if i % step == 0 {
                let v = chunk[channel];
                if v > 0.0 {
                    samples.push(v);
                }
            }
        }

        if samples.is_empty() {
            // Fallback if all values are 0.0
            return (0.0, 0.0);
        }

        let mid = samples.len() / 2;
        samples.select_nth_unstable_by(mid, |a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let median = samples[mid];

        let mut diffs: Vec<f32> = samples.iter().map(|&v| (v - median).abs()).collect();
        let diff_mid = diffs.len() / 2;
        diffs.select_nth_unstable_by(diff_mid, |a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mad = diffs[diff_mid];

        (median, mad)
    }

    /// Crop sub-rectangle [x0, y0, width, height]
    pub fn crop(&self, x0: usize, y0: usize, new_w: usize, new_h: usize) -> Result<AstroImage, String> {
        if x0 + new_w > self.width || y0 + new_h > self.height || new_w == 0 || new_h == 0 {
            return Err("Virheelliset rajausmitat".to_string());
        }

        let mut cropped_data = Vec::with_capacity(new_w * new_h * self.channels);
        for y in y0..(y0 + new_h) {
            let row_start = (y * self.width + x0) * self.channels;
            let row_end = row_start + new_w * self.channels;
            cropped_data.extend_from_slice(&self.data[row_start..row_end]);
        }

        let mut meta = self.metadata.clone();
        meta.file_name = format!("{}_Crop.fits", meta.file_name);

        Ok(AstroImage {
            width: new_w,
            height: new_h,
            channels: self.channels,
            data: cropped_data,
            metadata: meta,
        })
    }

    /// Automatically find the largest axis-aligned rectangle without black border zeros (from alignment/warping)
    pub fn auto_crop(&self) -> Result<AstroImage, String> {
        let mut min_x = 0;
        let mut max_x = self.width.saturating_sub(1);
        let mut min_y = 0;
        let mut max_y = self.height.saturating_sub(1);

        let has_zero = |x: usize, y: usize| -> bool {
            let idx = (y * self.width + x) * self.channels;
            self.data[idx] <= 0.0
        };

        // 1. Shrink edge if that edge is entirely or almost entirely (>= 95%) zeros
        while min_y < max_y {
            let total = max_x - min_x + 1;
            let zeros = (min_x..=max_x).filter(|&x| has_zero(x, min_y)).count();
            if zeros == total || (total >= 20 && zeros >= total * 95 / 100) {
                min_y += 1;
            } else {
                break;
            }
        }
        while max_y > min_y {
            let total = max_x - min_x + 1;
            let zeros = (min_x..=max_x).filter(|&x| has_zero(x, max_y)).count();
            if zeros == total || (total >= 20 && zeros >= total * 95 / 100) {
                max_y -= 1;
            } else {
                break;
            }
        }
        while min_x < max_x {
            let total = max_y - min_y + 1;
            let zeros = (min_y..=max_y).filter(|&y| has_zero(min_x, y)).count();
            if zeros == total || (total >= 20 && zeros >= total * 95 / 100) {
                min_x += 1;
            } else {
                break;
            }
        }
        while max_x > min_x {
            let total = max_y - min_y + 1;
            let zeros = (min_y..=max_y).filter(|&y| has_zero(max_x, y)).count();
            if zeros == total || (total >= 20 && zeros >= total * 95 / 100) {
                max_x -= 1;
            } else {
                break;
            }
        }

        // 2. Shrink edge with the most zeros if any corner contains zero,
        // but NEVER crop away more than 15% of width or height (prevents field rotation from destroying the image)
        let min_allowed_w = (self.width * 85) / 100;
        let min_allowed_h = (self.height * 85) / 100;

        while min_x + 10 < max_x && min_y + 10 < max_y {
            if (max_x - min_x + 1) <= min_allowed_w || (max_y - min_y + 1) <= min_allowed_h {
                break;
            }

            let tl = has_zero(min_x, min_y);
            let tr = has_zero(max_x, min_y);
            let bl = has_zero(min_x, max_y);
            let br = has_zero(max_x, max_y);

            if !tl && !tr && !bl && !br {
                break;
            }

            let top_zeros = (min_x..=max_x).filter(|&x| has_zero(x, min_y)).count();
            let btm_zeros = (min_x..=max_x).filter(|&x| has_zero(x, max_y)).count();
            let left_zeros = (min_y..=max_y).filter(|&y| has_zero(min_x, y)).count();
            let right_zeros = (min_y..=max_y).filter(|&y| has_zero(max_x, y)).count();

            let max_edge = top_zeros.max(btm_zeros).max(left_zeros.max(right_zeros));
            if max_edge == 0 {
                break;
            }

            if top_zeros == max_edge {
                min_y += 1;
            } else if btm_zeros == max_edge {
                max_y -= 1;
            } else if left_zeros == max_edge {
                min_x += 1;
            } else {
                max_x -= 1;
            }
        }

        let new_w = max_x - min_x + 1;
        let new_h = max_y - min_y + 1;

        if new_w == self.width && new_h == self.height {
            return Err("Kuvassa ei ole leikattavia mustia reunuksia.".to_string());
        }

        self.crop(min_x, min_y, new_w, new_h)
    }

    /// Rotate image 90 degrees clockwise
    pub fn rotate_90_cw(&self) -> AstroImage {
        let new_w = self.height;
        let new_h = self.width;
        let channels = self.channels;
        let mut out_data = vec![0.0f32; new_w * new_h * channels];

        out_data
            .par_chunks_exact_mut(new_w * channels)
            .enumerate()
            .for_each(|(ny, row)| {
                let old_x = ny;
                for nx in 0..new_w {
                    let old_y = self.height - 1 - nx;
                    let src_idx = (old_y * self.width + old_x) * channels;
                    let dst_idx = nx * channels;
                    row[dst_idx..dst_idx + channels]
                        .copy_from_slice(&self.data[src_idx..src_idx + channels]);
                }
            });

        let mut meta = self.metadata.clone();
        meta.file_name = format!("{}_Rot90CW.fits", meta.file_name.trim_end_matches(".fits"));

        AstroImage {
            width: new_w,
            height: new_h,
            channels,
            data: out_data,
            metadata: meta,
        }
    }

    /// Rotate image 90 degrees counter-clockwise
    pub fn rotate_90_ccw(&self) -> AstroImage {
        let new_w = self.height;
        let new_h = self.width;
        let channels = self.channels;
        let mut out_data = vec![0.0f32; new_w * new_h * channels];

        out_data
            .par_chunks_exact_mut(new_w * channels)
            .enumerate()
            .for_each(|(ny, row)| {
                let old_x = self.width - 1 - ny;
                for nx in 0..new_w {
                    let old_y = nx;
                    let src_idx = (old_y * self.width + old_x) * channels;
                    let dst_idx = nx * channels;
                    row[dst_idx..dst_idx + channels]
                        .copy_from_slice(&self.data[src_idx..src_idx + channels]);
                }
            });

        let mut meta = self.metadata.clone();
        meta.file_name = format!("{}_Rot90CCW.fits", meta.file_name.trim_end_matches(".fits"));

        AstroImage {
            width: new_w,
            height: new_h,
            channels,
            data: out_data,
            metadata: meta,
        }
    }

    /// Rotate image 180 degrees
    pub fn rotate_180(&self) -> AstroImage {
        let channels = self.channels;
        let mut out_data = vec![0.0f32; self.width * self.height * channels];

        out_data
            .par_chunks_exact_mut(self.width * channels)
            .enumerate()
            .for_each(|(ny, row)| {
                let old_y = self.height - 1 - ny;
                for nx in 0..self.width {
                    let old_x = self.width - 1 - nx;
                    let src_idx = (old_y * self.width + old_x) * channels;
                    let dst_idx = nx * channels;
                    row[dst_idx..dst_idx + channels]
                        .copy_from_slice(&self.data[src_idx..src_idx + channels]);
                }
            });

        let mut meta = self.metadata.clone();
        meta.file_name = format!("{}_Rot180.fits", meta.file_name.trim_end_matches(".fits"));

        AstroImage {
            width: self.width,
            height: self.height,
            channels,
            data: out_data,
            metadata: meta,
        }
    }

    /// Flip image horizontally (mirror along vertical axis)
    pub fn flip_horizontal(&self) -> AstroImage {
        let channels = self.channels;
        let mut out_data = vec![0.0f32; self.width * self.height * channels];

        out_data
            .par_chunks_exact_mut(self.width * channels)
            .enumerate()
            .for_each(|(ny, row)| {
                let old_y = ny;
                for nx in 0..self.width {
                    let old_x = self.width - 1 - nx;
                    let src_idx = (old_y * self.width + old_x) * channels;
                    let dst_idx = nx * channels;
                    row[dst_idx..dst_idx + channels]
                        .copy_from_slice(&self.data[src_idx..src_idx + channels]);
                }
            });

        let mut meta = self.metadata.clone();
        meta.file_name = format!("{}_FlipH.fits", meta.file_name.trim_end_matches(".fits"));

        AstroImage {
            width: self.width,
            height: self.height,
            channels,
            data: out_data,
            metadata: meta,
        }
    }

    /// Flip image vertically (mirror along horizontal axis)
    pub fn flip_vertical(&self) -> AstroImage {
        let channels = self.channels;
        let mut out_data = vec![0.0f32; self.width * self.height * channels];

        out_data
            .par_chunks_exact_mut(self.width * channels)
            .enumerate()
            .for_each(|(ny, row)| {
                let old_y = self.height - 1 - ny;
                let src_row_start = old_y * self.width * channels;
                let src_row_end = src_row_start + self.width * channels;
                row.copy_from_slice(&self.data[src_row_start..src_row_end]);
            });

        let mut meta = self.metadata.clone();
        meta.file_name = format!("{}_FlipV.fits", meta.file_name.trim_end_matches(".fits"));

        AstroImage {
            width: self.width,
            height: self.height,
            channels,
            data: out_data,
            metadata: meta,
        }
    }

    /// Rotate image by an arbitrary angle in degrees with bilinear interpolation
    pub fn rotate_arbitrary(&self, angle_degrees: f32, auto_crop_borders: bool) -> Result<AstroImage, String> {
        let mut norm_angle = angle_degrees % 360.0;
        if norm_angle > 180.0 {
            norm_angle -= 360.0;
        } else if norm_angle <= -180.0 {
            norm_angle += 360.0;
        }

        if norm_angle.abs() < 1e-4 {
            return Ok(self.clone());
        }
        if (norm_angle - 90.0).abs() < 1e-4 {
            return Ok(self.rotate_90_cw());
        }
        if (norm_angle + 90.0).abs() < 1e-4 {
            return Ok(self.rotate_90_ccw());
        }
        if (norm_angle - 180.0).abs() < 1e-4 || (norm_angle + 180.0).abs() < 1e-4 {
            return Ok(self.rotate_180());
        }

        let cx = (self.width as f32 - 1.0) * 0.5;
        let cy = (self.height as f32 - 1.0) * 0.5;
        let rad = norm_angle.to_radians();
        let cos_a = rad.cos();
        let sin_a = rad.sin();

        let channels = self.channels;
        let mut out_data = vec![0.0f32; self.width * self.height * channels];

        out_data
            .par_chunks_exact_mut(self.width * channels)
            .enumerate()
            .for_each(|(ny, row)| {
                let dy = ny as f32 - cy;
                for nx in 0..self.width {
                    let dx = nx as f32 - cx;
                    // Clockwise rotation inverse mapping
                    let src_x = cx + dx * cos_a + dy * sin_a;
                    let src_y = cy - dx * sin_a + dy * cos_a;

                    let x0 = src_x.floor() as isize;
                    let y0 = src_y.floor() as isize;
                    let x1 = x0 + 1;
                    let y1 = y0 + 1;

                    let dst_idx = nx * channels;

                    if x0 >= 0 && x1 < self.width as isize && y0 >= 0 && y1 < self.height as isize {
                        let fx = src_x - x0 as f32;
                        let fy = src_y - y0 as f32;
                        let w00 = (1.0 - fx) * (1.0 - fy);
                        let w10 = fx * (1.0 - fy);
                        let w01 = (1.0 - fx) * fy;
                        let w11 = fx * fy;

                        let idx00 = (y0 as usize * self.width + x0 as usize) * channels;
                        let idx10 = (y0 as usize * self.width + x1 as usize) * channels;
                        let idx01 = (y1 as usize * self.width + x0 as usize) * channels;
                        let idx11 = (y1 as usize * self.width + x1 as usize) * channels;

                        for c in 0..channels {
                            let v = w00 * self.data[idx00 + c]
                                + w10 * self.data[idx10 + c]
                                + w01 * self.data[idx01 + c]
                                + w11 * self.data[idx11 + c];
                            row[dst_idx + c] = v;
                        }
                    } else if x0 >= 0 && x0 < self.width as isize && y0 >= 0 && y0 < self.height as isize {
                        let idx = (y0 as usize * self.width + x0 as usize) * channels;
                        for c in 0..channels {
                            row[dst_idx + c] = self.data[idx + c];
                        }
                    } else {
                        for c in 0..channels {
                            row[dst_idx + c] = 0.0;
                        }
                    }
                }
            });

        let mut meta = self.metadata.clone();
        meta.file_name = format!("{}_Rot{:.1}.fits", meta.file_name.trim_end_matches(".fits"), norm_angle);

        let mut res_img = AstroImage {
            width: self.width,
            height: self.height,
            channels,
            data: out_data,
            metadata: meta,
        };

        if auto_crop_borders {
            if let Ok(cropped) = res_img.auto_crop() {
                res_img = cropped;
            }
        }

        Ok(res_img)
    }

    /// Convert image to 8-bit RGB byte buffer for display using the specified STF / stretch parameters
    pub fn to_rgb8_with_stf(&self, stf: &crate::processing::stf::StfParameters) -> Vec<u8> {
        let count = self.width * self.height;
        let mut out = vec![0u8; count * 3];

        if stf.is_linear() {
            let mult = 2.0_f32.powf(stf.linear_exposure_ev) * 255.0;
            out.par_chunks_exact_mut(3)
                .enumerate()
                .for_each(|(i, pixel)| {
                    let src_idx = i * self.channels;
                    if self.channels == 1 {
                        let v = (self.data[src_idx] * mult).clamp(0.0, 255.0) as u8;
                        pixel[0] = v;
                        pixel[1] = v;
                        pixel[2] = v;
                    } else {
                        for c in 0..3 {
                            let v = (self.data[src_idx + c] * mult).clamp(0.0, 255.0) as u8;
                            pixel[c] = v;
                        }
                    }
                });
            return out;
        }

        match stf.mode {
            crate::processing::stf::StretchMode::Linear => {
                // Already handled above
                out
            }
            crate::processing::stf::StretchMode::AutoStf | crate::processing::stf::StretchMode::ManualMtf => {
                let params = [
                    (stf.channels[0].shadows, stf.channels[0].midtones, stf.channels[0].highlights),
                    (stf.channels[1].shadows, stf.channels[1].midtones, stf.channels[1].highlights),
                    (stf.channels[2].shadows, stf.channels[2].midtones, stf.channels[2].highlights),
                ];
                self.to_rgb8_stretched_channels(&params)
            }
            crate::processing::stf::StretchMode::Arcsinh => {
                let s = stf.arcsinh_stretch.max(1.0);
                let b = stf.arcsinh_black;
                let norm = s.asinh();

                out.par_chunks_exact_mut(3)
                    .enumerate()
                    .for_each(|(i, pixel)| {
                        let src_idx = i * self.channels;
                        if self.channels == 1 {
                            let v = (self.data[src_idx] - b).max(0.0);
                            let stretched = (s * v).asinh() / norm;
                            let byte = (stretched * 255.0).clamp(0.0, 255.0) as u8;
                            pixel[0] = byte;
                            pixel[1] = byte;
                            pixel[2] = byte;
                        } else {
                            let r = self.data[src_idx];
                            let g = self.data[src_idx + 1];
                            let bl = self.data[src_idx + 2];
                            let intensity = (r + g + bl) / 3.0;
                            let v = (intensity - b).max(0.0);
                            if intensity > 1e-6 {
                                let stretched_i = (s * v).asinh() / norm;
                                let scale = stretched_i / intensity;
                                pixel[0] = (r * scale * 255.0).clamp(0.0, 255.0) as u8;
                                pixel[1] = (g * scale * 255.0).clamp(0.0, 255.0) as u8;
                                pixel[2] = (bl * scale * 255.0).clamp(0.0, 255.0) as u8;
                            } else {
                                pixel[0] = 0;
                                pixel[1] = 0;
                                pixel[2] = 0;
                            }
                        }
                    });
                out
            }
            crate::processing::stf::StretchMode::Logarithmic => {
                let a = stf.log_factor.max(1.0);
                let b = stf.log_black;
                let norm = (1.0 + a).ln().max(1e-6);

                out.par_chunks_exact_mut(3)
                    .enumerate()
                    .for_each(|(i, pixel)| {
                        let src_idx = i * self.channels;
                        if self.channels == 1 {
                            let v = (self.data[src_idx] - b).max(0.0);
                            let stretched = (1.0 + a * v).ln() / norm;
                            let byte = (stretched * 255.0).clamp(0.0, 255.0) as u8;
                            pixel[0] = byte;
                            pixel[1] = byte;
                            pixel[2] = byte;
                        } else {
                            for c in 0..3 {
                                let v = (self.data[src_idx + c] - b).max(0.0);
                                let stretched = (1.0 + a * v).ln() / norm;
                                pixel[c] = (stretched * 255.0).clamp(0.0, 255.0) as u8;
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

                out.par_chunks_exact_mut(3)
                    .enumerate()
                    .for_each(|(i, pixel)| {
                        let src_idx = i * self.channels;
                        if self.channels == 1 {
                            let v = self.data[src_idx];
                            let num = (d * (v - b)).asinh() - sub0;
                            let stretched = (num / den).clamp(0.0, 1.0);
                            let byte = (stretched * 255.0).clamp(0.0, 255.0) as u8;
                            pixel[0] = byte;
                            pixel[1] = byte;
                            pixel[2] = byte;
                        } else {
                            for c in 0..3 {
                                let v = self.data[src_idx + c];
                                let num = (d * (v - b)).asinh() - sub0;
                                let stretched = (num / den).clamp(0.0, 1.0);
                                pixel[c] = (stretched * 255.0).clamp(0.0, 255.0) as u8;
                            }
                        }
                    });
                out
            }
            crate::processing::stf::StretchMode::Root => {
                let gamma = stf.gamma.clamp(0.05, 2.0);
                let b = stf.gamma_black;
                let den = (1.0 - b).max(1e-5);

                out.par_chunks_exact_mut(3)
                    .enumerate()
                    .for_each(|(i, pixel)| {
                        let src_idx = i * self.channels;
                        if self.channels == 1 {
                            let v = ((self.data[src_idx] - b) / den).clamp(0.0, 1.0);
                            let stretched = v.powf(gamma);
                            let byte = (stretched * 255.0).clamp(0.0, 255.0) as u8;
                            pixel[0] = byte;
                            pixel[1] = byte;
                            pixel[2] = byte;
                        } else {
                            for c in 0..3 {
                                let v = ((self.data[src_idx + c] - b) / den).clamp(0.0, 1.0);
                                let stretched = v.powf(gamma);
                                pixel[c] = (stretched * 255.0).clamp(0.0, 255.0) as u8;
                            }
                        }
                    });
                out
            }
        }
    }

    /// Convert to 8-bit RGB byte buffer for display or saving using independent parameters per channel
    pub fn to_rgb8_stretched_channels(&self, channels_params: &[(f32, f32, f32)]) -> Vec<u8> {
        let count = self.width * self.height;
        let mut out = vec![0u8; count * 3];

        out.par_chunks_exact_mut(3)
            .enumerate()
            .for_each(|(i, pixel)| {
                let src_idx = i * self.channels;
                if self.channels == 1 {
                    let (shadows, midtones, highlights) = channels_params.first().copied().unwrap_or((0.0, 0.5, 1.0));
                    let v = self.data[src_idx];
                    let stretched = apply_mtf_stretch(v, shadows, midtones, highlights);
                    let byte = (stretched * 255.0).clamp(0.0, 255.0) as u8;
                    pixel[0] = byte;
                    pixel[1] = byte;
                    pixel[2] = byte;
                } else {
                    for c in 0..3 {
                        let (shadows, midtones, highlights) = channels_params.get(c).copied().unwrap_or((0.0, 0.5, 1.0));
                        let v = self.data[src_idx + c];
                        let stretched = apply_mtf_stretch(v, shadows, midtones, highlights);
                        pixel[c] = (stretched * 255.0).clamp(0.0, 255.0) as u8;
                    }
                }
            });

        out
    }

    /// Convert to 8-bit RGB byte buffer for display or saving with single parameter set
    pub fn to_rgb8_stretched(&self, shadows: f32, midtones: f32, highlights: f32) -> Vec<u8> {
        self.to_rgb8_stretched_channels(&[(shadows, midtones, highlights); 3])
    }
}

#[inline(always)]
pub fn mtf(m: f32, x: f32) -> f32 {
    if x <= 0.0 {
        0.0
    } else if x >= 1.0 {
        1.0
    } else if (m - 0.5).abs() < 1e-6 {
        x
    } else {
        let num = (m - 1.0) * x;
        let den = (2.0 * m - 1.0) * x - m;
        if den.abs() < 1e-7 {
            x
        } else {
            (num / den).clamp(0.0, 1.0)
        }
    }
}

#[inline(always)]
pub fn apply_mtf_stretch(val: f32, shadows: f32, midtones: f32, highlights: f32) -> f32 {
    if highlights <= shadows {
        return val;
    }
    let normalized = ((val - shadows) / (highlights - shadows)).clamp(0.0, 1.0);
    mtf(midtones, normalized)
}
