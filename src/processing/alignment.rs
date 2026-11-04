use crate::core::image::AstroImage;
use rayon::prelude::*;

#[derive(Clone, Copy, Debug)]
pub struct Star {
    pub x: f32,
    pub y: f32,
    pub flux: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Transform2D {
    pub theta: f32,
    pub tx: f32,
    pub ty: f32,
}

#[allow(dead_code)]
impl Transform2D {
    pub fn identity() -> Self {
        Self {
            theta: 0.0,
            tx: 0.0,
            ty: 0.0,
        }
    }

    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        let cos_t = self.theta.cos();
        let sin_t = self.theta.sin();
        (
            cos_t * x - sin_t * y + self.tx,
            sin_t * x + cos_t * y + self.ty,
        )
    }

    pub fn inverse(&self) -> Self {
        let cos_t = self.theta.cos();
        let sin_t = self.theta.sin();
        let inv_tx = -(cos_t * self.tx + sin_t * self.ty);
        let inv_ty = -(-sin_t * self.tx + cos_t * self.ty);
        Self {
            theta: -self.theta,
            tx: inv_tx,
            ty: inv_ty,
        }
    }

    /// Composes two transforms: self(other(x))
    pub fn compose(&self, other: &Self) -> Self {
        let cos_t = self.theta.cos();
        let sin_t = self.theta.sin();
        let rx = cos_t * other.tx - sin_t * other.ty;
        let ry = sin_t * other.tx + cos_t * other.ty;
        Self {
            theta: self.theta + other.theta,
            tx: rx + self.tx,
            ty: ry + self.ty,
        }
    }
}

pub struct StarDetector;

impl StarDetector {
    /// Detects candidate stars in an AstroImage and returns them sorted by flux.
    pub fn detect(image: &AstroImage, max_stars: usize) -> Vec<Star> {
        let width = image.width;
        let height = image.height;
        if width < 30 || height < 30 {
            return Vec::new();
        }

        // 1. Extract luminance (0.299R + 0.587G + 0.114B for RGB, or direct mono)
        let lum: Vec<f32> = if image.channels >= 3 {
            image
                .data
                .par_chunks_exact(image.channels)
                .map(|p| 0.299 * p[0] + 0.587 * p[1] + 0.114 * p[2])
                .collect()
        } else {
            image.data.clone()
        };

        // 2. Background statistics (sample a subset for speed)
        let sample_step = 8;
        let mut samples: Vec<f32> = lum.iter().step_by(sample_step).copied().collect();
        if samples.is_empty() {
            return Vec::new();
        }
        let mid = samples.len() / 2;
        samples.select_nth_unstable_by(mid, |a: &f32, b: &f32| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let median = samples[mid];

        let mut diffs: Vec<f32> = samples.iter().map(|&v| (v - median).abs()).collect();
        diffs.select_nth_unstable_by(mid, |a: &f32, b: &f32| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mad = diffs[mid].max(1e-5);

        let threshold = median + 3.5 * mad;

        // 3. Find 3x3 local peaks above threshold
        let mut candidates = Vec::new();
        for y in 10..(height - 10) {
            let row_offset = y * width;
            for x in 10..(width - 10) {
                let val = lum[row_offset + x];
                if val <= threshold {
                    continue;
                }

                let mut is_max = true;
                let mut count_above = 0;
                for dy in -1..=1 {
                    let n_row = ((y as isize + dy) as usize) * width;
                    for dx in -1..=1 {
                        let nx = (x as isize + dx) as usize;
                        let n_val = lum[n_row + nx];
                        if n_val > val {
                            is_max = false;
                            break;
                        }
                        if n_val > threshold {
                            count_above += 1;
                        }
                    }
                    if !is_max {
                        break;
                    }
                }

                if is_max && count_above >= 3 {
                    let mut sum_w = 0.0f32;
                    let mut sum_wx = 0.0f32;
                    let mut sum_wy = 0.0f32;
                    for dy in -1..=1 {
                        let ny = (y as isize + dy) as usize;
                        let n_row = ny * width;
                        for dx in -1..=1 {
                            let nx = (x as isize + dx) as usize;
                            let w = (lum[n_row + nx] - median).max(0.0);
                            sum_w += w;
                            sum_wx += w * (nx as f32);
                            sum_wy += w * (ny as f32);
                        }
                    }

                    if sum_w > 0.0 {
                        candidates.push(Star {
                            x: sum_wx / sum_w,
                            y: sum_wy / sum_w,
                            flux: sum_w,
                        });
                    }
                }
            }
        }

        // 4. Sort by flux descending
        candidates.sort_by(|a, b| b.flux.partial_cmp(&a.flux).unwrap_or(std::cmp::Ordering::Equal));

        // 5. Deduplicate stars that are within 6 pixels of an existing brighter star
        let mut clean_stars = Vec::with_capacity(max_stars);
        for star in candidates {
            let too_close = clean_stars.iter().any(|c: &Star| {
                let dx = star.x - c.x;
                let dy = star.y - c.y;
                (dx * dx + dy * dy) < 36.0
            });
            if !too_close {
                clean_stars.push(star);
                if clean_stars.len() >= max_stars {
                    break;
                }
            }
        }

        clean_stars
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
struct AsterismTriangle {
    r1: f32, // a / c
    r2: f32, // b / c
    v0: usize, // opposite to longest side c (between a and b)
    v1: usize, // opposite to middle side b (between a and c)
    v2: usize, // opposite to shortest side a (between b and c)
}

pub struct StarMatcher;

impl StarMatcher {
    /// Finds the 2D transform (rotation + translation) that aligns `target_stars` to `ref_stars`.
    pub fn match_stars(ref_stars: &[Star], target_stars: &[Star]) -> Option<Transform2D> {
        Self::match_stars_with_limit(ref_stars, target_stars, 25, 4)
    }

    /// Flexible star matching with custom star count limit and minimum inliers threshold (ideal for mosaics)
    pub fn match_stars_with_limit(
        ref_stars: &[Star],
        target_stars: &[Star],
        star_limit: usize,
        min_inliers: usize,
    ) -> Option<Transform2D> {
        if ref_stars.len() < 3 || target_stars.len() < 3 {
            return None;
        }

        // Build asterism triangles from top stars
        let mut t_ref = Self::build_triangles(ref_stars, star_limit);
        let mut t_tgt = Self::build_triangles(target_stars, star_limit);

        // Sort by r1 for fast windowed comparison
        t_ref.sort_by(|a, b| a.r1.partial_cmp(&b.r1).unwrap_or(std::cmp::Ordering::Equal));
        t_tgt.sort_by(|a, b| a.r1.partial_cmp(&b.r1).unwrap_or(std::cmp::Ordering::Equal));

        let mut max_inliers = 0;
        let mut best_transform = None;
        let eps = 0.015;

        for tri_a in &t_ref {
            // Find window of triangles in t_tgt matching r1
            let start = t_tgt.partition_point(|t| t.r1 < tri_a.r1 - eps);
            for tri_b in &t_tgt[start..] {
                if tri_b.r1 > tri_a.r1 + eps {
                    break;
                }
                if (tri_a.r2 - tri_b.r2).abs() < eps {
                    let p_a0 = &ref_stars[tri_a.v0];
                    let p_a1 = &ref_stars[tri_a.v1];
                    let p_b0 = &target_stars[tri_b.v0];
                    let p_b1 = &target_stars[tri_b.v1];

                    let angle_a = (p_a1.y - p_a0.y).atan2(p_a1.x - p_a0.x);
                    let angle_b = (p_b1.y - p_b0.y).atan2(p_b1.x - p_b0.x);
                    let theta = angle_a - angle_b;

                    let cos_t = theta.cos();
                    let sin_t = theta.sin();

                    let rot_bx = cos_t * p_b0.x - sin_t * p_b0.y;
                    let rot_by = sin_t * p_b0.x + cos_t * p_b0.y;

                    let tx = p_a0.x - rot_bx;
                    let ty = p_a0.y - rot_by;

                    let tf = Transform2D { theta, tx, ty };

                    // Count inliers
                    let mut inliers = 0;
                    for star_b in target_stars {
                        let (mapped_x, mapped_y) = tf.apply(star_b.x, star_b.y);
                        let matched = ref_stars.iter().any(|star_a| {
                            let dx = mapped_x - star_a.x;
                            let dy = mapped_y - star_a.y;
                            (dx * dx + dy * dy) < 9.0 // 3.0 pixels tolerance squared
                        });
                        if matched {
                            inliers += 1;
                        }
                    }

                    if inliers > max_inliers {
                        max_inliers = inliers;
                        best_transform = Some(tf);

                        // Only early-exit if practically all stars agree (>= 80% of stars)
                        if inliers >= (target_stars.len() * 4 / 5) {
                            return best_transform;
                        }
                    }
                }
            }
        }

        // Require at least min_inliers consistent matching stars
        if max_inliers >= min_inliers {
            best_transform
        } else {
            None
        }
    }

    fn build_triangles(stars: &[Star], limit: usize) -> Vec<AsterismTriangle> {
        let mut triangles = Vec::new();
        let n = stars.len().min(limit);

        for i in 0..n {
            for j in (i + 1)..n {
                let dx_ij = stars[i].x - stars[j].x;
                let dy_ij = stars[i].y - stars[j].y;
                let d_ij = (dx_ij * dx_ij + dy_ij * dy_ij).sqrt();
                if d_ij < 25.0 {
                    continue;
                }

                for k in (j + 1)..n {
                    let dx_jk = stars[j].x - stars[k].x;
                    let dy_jk = stars[j].y - stars[k].y;
                    let d_jk = (dx_jk * dx_jk + dy_jk * dy_jk).sqrt();
                    if d_jk < 25.0 {
                        continue;
                    }

                    let dx_ki = stars[k].x - stars[i].x;
                    let dy_ki = stars[k].y - stars[i].y;
                    let d_ki = (dx_ki * dx_ki + dy_ki * dy_ki).sqrt();
                    if d_ki < 25.0 {
                        continue;
                    }

                    // Vertex opposite to d_jk is i
                    // Vertex opposite to d_ki is j
                    // Vertex opposite to d_ij is k
                    let mut sides = [(d_jk, i), (d_ki, j), (d_ij, k)];
                    sides.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

                    let a = sides[0].0;
                    let b = sides[1].0;
                    let c = sides[2].0;

                    let opp_a = sides[0].1;
                    let opp_b = sides[1].1;
                    let opp_c = sides[2].1;

                    let r1 = a / c;
                    let r2 = b / c;

                    // v0 is between a and b (opp_c), v1 is between a and c (opp_b), v2 is between b and c (opp_a)
                    triangles.push(AsterismTriangle {
                        r1,
                        r2,
                        v0: opp_c,
                        v1: opp_b,
                        v2: opp_a,
                    });
                }
            }
        }

        triangles
    }
}

pub struct ImageWarper;

impl ImageWarper {
    /// Warps an AstroImage to align with the reference frame using bilinear sampling.
    #[allow(dead_code)]
    pub fn warp(image: &AstroImage, transform: &Transform2D) -> AstroImage {
        Self::warp_scaled(image, transform, 1)
    }

    /// Warps an AstroImage onto an upscaled coordinate grid (e.g. 2x Drizzle super-resolution)
    pub fn warp_scaled(image: &AstroImage, transform: &Transform2D, scale: usize) -> AstroImage {
        let scale = scale.max(1);
        let out_width = image.width * scale;
        let out_height = image.height * scale;
        let channels = image.channels;
        let inv_scale = 1.0 / (scale as f32);

        let inv_tf = transform.inverse();
        let src = &image.data;
        let mut warped_data = vec![0.0f32; out_width * out_height * channels];

        let in_w = image.width;
        let in_h = image.height;

        warped_data
            .par_chunks_exact_mut(out_width * channels)
            .enumerate()
            .for_each(|(y_out, row_out)| {
                for x_out in 0..out_width {
                    // Map pixel center on the scaled grid back to reference 1x coordinates
                    let ref_x = (x_out as f32 + 0.5) * inv_scale - 0.5;
                    let ref_y = (y_out as f32 + 0.5) * inv_scale - 0.5;

                    // Map reference coordinate back into target source image
                    let (src_x, src_y) = inv_tf.apply(ref_x, ref_y);

                    let out_idx = x_out * channels;

                    if src_x >= 0.0
                        && src_x < (in_w - 1) as f32
                        && src_y >= 0.0
                        && src_y < (in_h - 1) as f32
                    {
                        let x0 = src_x.floor() as usize;
                        let y0 = src_y.floor() as usize;
                        let x1 = x0 + 1;
                        let y1 = y0 + 1;

                        let fx = src_x - x0 as f32;
                        let fy = src_y - y0 as f32;

                        let w00 = (1.0 - fx) * (1.0 - fy);
                        let w10 = fx * (1.0 - fy);
                        let w01 = (1.0 - fx) * fy;
                        let w11 = fx * fy;

                        let idx00 = (y0 * in_w + x0) * channels;
                        let idx10 = (y0 * in_w + x1) * channels;
                        let idx01 = (y1 * in_w + x0) * channels;
                        let idx11 = (y1 * in_w + x1) * channels;

                        for c in 0..channels {
                            let val = w00 * src[idx00 + c]
                                + w10 * src[idx10 + c]
                                + w01 * src[idx01 + c]
                                + w11 * src[idx11 + c];
                            row_out[out_idx + c] = val;
                        }
                    } else {
                        for c in 0..channels {
                            row_out[out_idx + c] = 0.0;
                        }
                    }
                }
            });

        AstroImage {
            width: out_width,
            height: out_height,
            channels,
            data: warped_data,
            metadata: image.metadata.clone(),
        }
    }

    /// Warps an AstroImage onto a specific target grid (e.g. matching a reference image's width and height)
    pub fn warp_to_reference(
        image: &AstroImage,
        transform: &Transform2D,
        out_width: usize,
        out_height: usize,
    ) -> AstroImage {
        let channels = image.channels;
        let inv_tf = transform.inverse();
        let src = &image.data;
        let mut warped_data = vec![0.0f32; out_width * out_height * channels];

        let in_w = image.width;
        let in_h = image.height;

        warped_data
            .par_chunks_exact_mut(out_width * channels)
            .enumerate()
            .for_each(|(y_out, row_out)| {
                for x_out in 0..out_width {
                    let ref_x = x_out as f32;
                    let ref_y = y_out as f32;

                    let (src_x, src_y) = inv_tf.apply(ref_x, ref_y);
                    let out_idx = x_out * channels;

                    if src_x >= 0.0
                        && src_x < (in_w - 1) as f32
                        && src_y >= 0.0
                        && src_y < (in_h - 1) as f32
                    {
                        let x0 = src_x.floor() as usize;
                        let y0 = src_y.floor() as usize;
                        let x1 = x0 + 1;
                        let y1 = y0 + 1;

                        let fx = src_x - x0 as f32;
                        let fy = src_y - y0 as f32;

                        let w00 = (1.0 - fx) * (1.0 - fy);
                        let w10 = fx * (1.0 - fy);
                        let w01 = (1.0 - fx) * fy;
                        let w11 = fx * fy;

                        let idx00 = (y0 * in_w + x0) * channels;
                        let idx10 = (y0 * in_w + x1) * channels;
                        let idx01 = (y1 * in_w + x0) * channels;
                        let idx11 = (y1 * in_w + x1) * channels;

                        for c in 0..channels {
                            let val = w00 * src[idx00 + c]
                                + w10 * src[idx10 + c]
                                + w01 * src[idx01 + c]
                                + w11 * src[idx11 + c];
                            row_out[out_idx + c] = val;
                        }
                    } else {
                        for c in 0..channels {
                            row_out[out_idx + c] = 0.0;
                        }
                    }
                }
            });

        AstroImage {
            width: out_width,
            height: out_height,
            channels,
            data: warped_data,
            metadata: image.metadata.clone(),
        }
    }
}
