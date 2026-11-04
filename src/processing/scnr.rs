use crate::core::image::AstroImage;
use rayon::prelude::*;

pub struct Scnr;

impl Scnr {
    pub fn remove_green(image: &AstroImage, amount: f32) -> Result<AstroImage, String> {
        if !image.is_rgb() {
            return Err("SCNR requires an RGB image".to_string());
        }

        let mut out = image.clone();
        let amt = amount.clamp(0.0, 1.0);

        out.data
            .par_chunks_exact_mut(3)
            .for_each(|pixel| {
                let r = pixel[0];
                let g = pixel[1];
                let b = pixel[2];

                // Maximum neutral mask
                let max_rb = r.max(b);
                if g > max_rb {
                    let target_g = max_rb;
                    pixel[1] = g * (1.0 - amt) + target_g * amt;
                }
            });

        Ok(out)
    }
}
