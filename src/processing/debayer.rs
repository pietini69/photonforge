use crate::core::image::AstroImage;
use rayon::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BayerPattern {
    Rggb,
    Bggr,
    Grbg,
    Gbrg,
}

impl BayerPattern {
    pub fn from_str(s: &str) -> Option<Self> {
        let cleaned = s.trim().to_uppercase();
        match cleaned.as_str() {
            "RGGB" => Some(Self::Rggb),
            "BGGR" => Some(Self::Bggr),
            "GRBG" => Some(Self::Grbg),
            "GBRG" => Some(Self::Gbrg),
            _ => None,
        }
    }

    pub fn to_str(&self) -> &'static str {
        match self {
            Self::Rggb => "RGGB",
            Self::Bggr => "BGGR",
            Self::Grbg => "GRBG",
            Self::Gbrg => "GBRG",
        }
    }

    /// If an image with an even number of rows is flipped vertically (FITS bottom-to-top),
    /// the Bayer row order shifts by 1 row.
    pub fn flipped_vertical(&self, height: usize) -> Self {
        if height % 2 == 0 {
            match self {
                Self::Rggb => Self::Gbrg,
                Self::Gbrg => Self::Rggb,
                Self::Grbg => Self::Bggr,
                Self::Bggr => Self::Grbg,
            }
        } else {
            *self
        }
    }
}

pub struct Debayer;

impl Debayer {
    /// Debayers a single-channel (CFA/Bayer) AstroImage into a 3-channel RGB AstroImage
    /// using fast, high-quality parallel bilinear interpolation.
    pub fn debayer(image: &AstroImage, pattern: BayerPattern) -> Result<AstroImage, String> {
        if image.channels != 1 {
            return Err("Debayering requires a single-channel (mono/CFA) image".to_string());
        }

        let width = image.width;
        let height = image.height;

        if width < 2 || height < 2 {
            return Err("Image dimensions too small for debayering".to_string());
        }

        let src = &image.data;
        let mut rgb_data = vec![0.0f32; width * height * 3];

        // Process rows in parallel
        rgb_data
            .par_chunks_exact_mut(width * 3)
            .enumerate()
            .for_each(|(y, row_out)| {
                let y_prev = if y > 0 { y - 1 } else { y + 1 };
                let y_next = if y + 1 < height { y + 1 } else { y - 1 };

                for x in 0..width {
                    let x_prev = if x > 0 { x - 1 } else { x + 1 };
                    let x_next = if x + 1 < width { x + 1 } else { x - 1 };

                    // Current pixel value
                    let c_val = src[y * width + x];

                    // Cardinal neighbors
                    let n_val = src[y_prev * width + x];
                    let s_val = src[y_next * width + x];
                    let w_val = src[y * width + x_prev];
                    let e_val = src[y * width + x_next];

                    // Diagonal neighbors
                    let nw_val = src[y_prev * width + x_prev];
                    let ne_val = src[y_prev * width + x_next];
                    let sw_val = src[y_next * width + x_prev];
                    let se_val = src[y_next * width + x_next];

                    let avg_cardinal = (n_val + s_val + w_val + e_val) * 0.25;
                    let avg_diagonal = (nw_val + ne_val + sw_val + se_val) * 0.25;
                    let avg_horizontal = (w_val + e_val) * 0.5;
                    let avg_vertical = (n_val + s_val) * 0.5;

                    let (r, g, b) = match pattern {
                        BayerPattern::Rggb => {
                            // Row 0: R G R G ...
                            // Row 1: G B G B ...
                            match (y % 2, x % 2) {
                                (0, 0) => (c_val, avg_cardinal, avg_diagonal), // Red pixel
                                (0, 1) => (avg_horizontal, c_val, avg_vertical), // Green pixel (Red row)
                                (1, 0) => (avg_vertical, c_val, avg_horizontal), // Green pixel (Blue row)
                                (1, 1) => (avg_diagonal, avg_cardinal, c_val), // Blue pixel
                                _ => unreachable!(),
                            }
                        }
                        BayerPattern::Bggr => {
                            // Row 0: B G B G ...
                            // Row 1: G R G R ...
                            match (y % 2, x % 2) {
                                (0, 0) => (avg_diagonal, avg_cardinal, c_val), // Blue pixel
                                (0, 1) => (avg_vertical, c_val, avg_horizontal), // Green pixel (Blue row)
                                (1, 0) => (avg_horizontal, c_val, avg_vertical), // Green pixel (Red row)
                                (1, 1) => (c_val, avg_cardinal, avg_diagonal), // Red pixel
                                _ => unreachable!(),
                            }
                        }
                        BayerPattern::Grbg => {
                            // Row 0: G R G R ...
                            // Row 1: B G B G ...
                            match (y % 2, x % 2) {
                                (0, 0) => (avg_horizontal, c_val, avg_vertical), // Green pixel (Red row)
                                (0, 1) => (c_val, avg_cardinal, avg_diagonal), // Red pixel
                                (1, 0) => (avg_diagonal, avg_cardinal, c_val), // Blue pixel
                                (1, 1) => (avg_vertical, c_val, avg_horizontal), // Green pixel (Blue row)
                                _ => unreachable!(),
                            }
                        }
                        BayerPattern::Gbrg => {
                            // Row 0: G B G B ...
                            // Row 1: R G R G ...
                            match (y % 2, x % 2) {
                                (0, 0) => (avg_vertical, c_val, avg_horizontal), // Green pixel (Blue row)
                                (0, 1) => (avg_diagonal, avg_cardinal, c_val), // Blue pixel
                                (1, 0) => (c_val, avg_cardinal, avg_diagonal), // Red pixel
                                (1, 1) => (avg_horizontal, c_val, avg_vertical), // Green pixel (Red row)
                                _ => unreachable!(),
                            }
                        }
                    };

                    let out_idx = x * 3;
                    row_out[out_idx] = r;
                    row_out[out_idx + 1] = g;
                    row_out[out_idx + 2] = b;
                }
            });

        let mut metadata = image.metadata.clone();
        metadata.bayer_pattern = Some(pattern.to_str().to_string());

        Ok(AstroImage {
            width,
            height,
            channels: 3,
            data: rgb_data,
            metadata,
        })
    }
}
