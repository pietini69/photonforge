use super::image::{AstroImage, ImageMetadata};
use std::path::Path;
use xisf::{BlockOptions, Bounds, ColorSpace, Image, PendingImage, SampleFormat, Writer, XisfFile};

pub struct XisfParser;

impl XisfParser {
    pub fn read_file<P: AsRef<Path>>(path: P) -> Result<AstroImage, String> {
        let path_ref = path.as_ref();
        let file = XisfFile::open(path_ref)
            .map_err(|e| format!("Virhe avattaessa XISF-tiedostoa {}: {}", path_ref.display(), e))?;

        let images = file.images();
        let img_ref = images
            .first()
            .ok_or_else(|| format!("XISF-tiedostossa {} ei ole kuvadataa", path_ref.display()))?;

        let geom = img_ref.geometry();
        if geom.len() < 2 {
            return Err("Virheellinen XISF-geometria (vähemmän kuin 2 ulottuvuutta)".to_string());
        }

        let width = geom[0] as usize;
        let height = geom[1] as usize;
        let channels = img_ref.channels().max(1) as usize;
        let plane_size = width * height;

        // Read planar samples according to sample format
        let planar_floats: Vec<f32> = match img_ref.sample_format() {
            SampleFormat::Float32 => {
                img_ref.read_planar::<f32>().map_err(|e| format!("Virhe luettaessa f32 XISF-dataa: {}", e))?
            }
            SampleFormat::Float64 => {
                let f64_samples = img_ref.read_planar::<f64>().map_err(|e| format!("Virhe luettaessa f64 XISF-dataa: {}", e))?;
                f64_samples.into_iter().map(|v| v as f32).collect()
            }
            SampleFormat::UInt16 => {
                let u16_samples = img_ref.read_planar::<u16>().map_err(|e| format!("Virhe luettaessa u16 XISF-dataa: {}", e))?;
                u16_samples.into_iter().map(|v| v as f32 / 65535.0).collect()
            }
            SampleFormat::UInt8 => {
                let u8_samples = img_ref.read_planar::<u8>().map_err(|e| format!("Virhe luettaessa u8 XISF-dataa: {}", e))?;
                u8_samples.into_iter().map(|v| v as f32 / 255.0).collect()
            }
            SampleFormat::UInt32 => {
                let u32_samples = img_ref.read_planar::<u32>().map_err(|e| format!("Virhe luettaessa u32 XISF-dataa: {}", e))?;
                u32_samples.into_iter().map(|v| (v as f64 / 4294967295.0) as f32).collect()
            }
            _ => {
                return Err(format!("XISF-näyteformaattia {:?} ei tueta", img_ref.sample_format()));
            }
        };

        let expected_samples = plane_size * channels;
        if planar_floats.len() < expected_samples {
            return Err(format!(
                "XISF-datan pituus ei täsmää: odotettiin {} näytettä, saatiin {}",
                expected_samples,
                planar_floats.len()
            ));
        }

        // Convert planar samples [Plane 0, Plane 1, ...] to interleaved [Pixel 0 RGB, Pixel 1 RGB, ...]
        let mut interleaved = vec![0.0f32; expected_samples];
        for c in 0..channels {
            let plane = &planar_floats[c * plane_size..(c + 1) * plane_size];
            for i in 0..plane_size {
                interleaved[i * channels + c] = plane[i];
            }
        }

        let mut metadata = ImageMetadata {
            file_name: path_ref
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default(),
            file_path: path_ref.to_string_lossy().to_string(),
            ..Default::default()
        };

        for (key, val, _) in img_ref.fits_keywords() {
            match key.to_uppercase().as_str() {
                "EXPTIME" | "EXPOSURE" => {
                    if let Ok(exp) = val.trim_matches('\'').trim().parse::<f32>() {
                        metadata.exposure_time = Some(exp);
                    }
                }
                "INSTRUME" | "CAMERA" => {
                    metadata.camera = Some(val.trim_matches('\'').trim().to_string());
                }
                "TELESCOP" => {
                    metadata.telescope = Some(val.trim_matches('\'').trim().to_string());
                }
                "FILTER" => {
                    metadata.filter_name = Some(val.trim_matches('\'').trim().to_string());
                }
                "BAYERPAT" => {
                    metadata.bayer_pattern = Some(val.trim_matches('\'').trim().to_string());
                }
                _ => {}
            }
        }

        Ok(AstroImage {
            width,
            height,
            channels,
            data: interleaved,
            metadata,
        })
    }

    pub fn write_file<P: AsRef<Path>>(img: &AstroImage, path: P) -> Result<(), String> {
        let path_ref = path.as_ref();
        let width = img.width;
        let height = img.height;
        let channels = img.channels;
        let plane_size = width * height;

        let mut writer = Writer::new().with_creator("PhotonForge 0.1.0");

        let image_desc = Image {
            dimensions: vec![width as u64, height as u64],
            channels: channels as u64,
            sample_format: SampleFormat::Float32,
            color_space: if channels >= 3 {
                ColorSpace::Rgb
            } else {
                ColorSpace::Gray
            },
            pixel_storage: xisf::PixelStorage::Planar,
            bounds: Some(Bounds { low: 0.0, high: 1.0 }),
            id: None,
            uuid: None,
            image_type: None,
            offset: None,
            orientation: None,
        };

        // Convert interleaved to planar byte stream (f32 LE)
        let mut planar_bytes = Vec::with_capacity(plane_size * channels * 4);
        for c in 0..channels {
            for i in 0..plane_size {
                let val = img.data[i * channels + c];
                planar_bytes.extend_from_slice(&val.to_le_bytes());
            }
        }

        writer
            .add_image(PendingImage::new(image_desc, planar_bytes, BlockOptions::default()))
            .map_err(|e| format!("Virhe lisättäessä XISF-kuvaa: {}", e))?;

        let bytes = writer
            .to_bytes()
            .map_err(|e| format!("Virhe sarjallistettaessa XISF: {}", e))?;

        std::fs::write(path_ref, bytes)
            .map_err(|e| format!("Virhe tallennettaessa XISF-tiedostoa {}: {}", path_ref.display(), e))?;

        Ok(())
    }
}
