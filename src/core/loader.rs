use super::fits::FitsParser;
use super::image::{AstroImage, ImageMetadata};
use super::xisf_io::XisfParser;
use std::path::Path;

pub struct ImageLoader;

impl ImageLoader {
    pub fn load_file<P: AsRef<Path>>(path: P) -> Result<AstroImage, String> {
        let p = path.as_ref();
        let ext = p
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let mut img = match ext.as_str() {
            "fits" | "fit" | "fts" => FitsParser::read_file(p),
            "xisf" => XisfParser::read_file(p),
            "png" | "jpg" | "jpeg" | "tiff" | "tif" | "bmp" => Self::load_standard_image(p),
            _ => {
                // Try FITS, then XISF, then standard image
                FitsParser::read_file(p)
                    .or_else(|_| XisfParser::read_file(p))
                    .or_else(|_| Self::load_standard_image(p))
            }
        }?;

        // Auto-detect filter from header or file name if not already set
        let detected = if let Some(ref h) = img.metadata.filter_name {
            crate::processing::channel_combination::AstroFilter::detect(Some(h), &img.metadata.file_name)
        } else {
            crate::processing::channel_combination::AstroFilter::detect(None, &img.metadata.file_name)
        };

        if let Some(f) = detected {
            img.metadata.filter_name = Some(f.badge().to_string());
        }

        Ok(img)
    }

    fn load_standard_image(path: &Path) -> Result<AstroImage, String> {
        let dynamic_img = image::open(path).map_err(|e| format!("Failed to open image: {}", e))?;
        let rgb_img = dynamic_img.to_rgb32f();
        let width = rgb_img.width() as usize;
        let height = rgb_img.height() as usize;

        let raw_pixels = rgb_img.into_raw();

        let metadata = ImageMetadata {
            file_name: path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default(),
            file_path: path.to_string_lossy().to_string(),
            ..Default::default()
        };

        Ok(AstroImage {
            width,
            height,
            channels: 3,
            data: raw_pixels,
            metadata,
        })
    }

    pub fn save_image<P: AsRef<Path>>(img: &AstroImage, path: P) -> Result<(), String> {
        let p = path.as_ref();
        let ext = p
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        if ext == "fits" || ext == "fit" || ext == "fts" {
            FitsParser::write_file(img, p)
        } else if ext == "xisf" {
            XisfParser::write_file(img, p)
        } else {
            // Save as standard 8-bit or 16-bit image
            let count = img.width * img.height;
            let mut rgb_bytes = vec![0u8; count * 3];

            for i in 0..count {
                if img.channels == 1 {
                    let v = (img.data[i] * 255.0).clamp(0.0, 255.0) as u8;
                    rgb_bytes[i * 3] = v;
                    rgb_bytes[i * 3 + 1] = v;
                    rgb_bytes[i * 3 + 2] = v;
                } else {
                    for c in 0..3 {
                        let v = (img.data[i * img.channels + c] * 255.0).clamp(0.0, 255.0) as u8;
                        rgb_bytes[i * 3 + c] = v;
                    }
                }
            }

            image::save_buffer(
                p,
                &rgb_bytes,
                img.width as u32,
                img.height as u32,
                image::ColorType::Rgb8,
            )
            .map_err(|e| format!("Failed to save image: {}", e))
        }
    }
}
