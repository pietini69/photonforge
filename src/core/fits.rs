use super::image::{AstroImage, ImageMetadata};
use crate::processing::debayer::{BayerPattern, Debayer};
use byteorder::{BigEndian, ByteOrder};
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::Path;

pub struct FitsParser;

impl FitsParser {
    pub fn read_file<P: AsRef<Path>>(path: P) -> Result<AstroImage, String> {
        Self::read_file_with_options(path, true)
    }

    pub fn read_file_with_options<P: AsRef<Path>>(
        path: P,
        auto_debayer: bool,
    ) -> Result<AstroImage, String> {
        let path_ref = path.as_ref();
        let file = File::open(path_ref).map_err(|e| format!("Could not open file: {}", e))?;
        let mut reader = BufReader::new(file);

        let mut metadata = ImageMetadata {
            file_name: path_ref
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default(),
            file_path: path_ref.to_string_lossy().to_string(),
            ..Default::default()
        };

        let mut bitpix: i32 = 16;
        let mut naxis: usize = 0;
        let mut naxis1: usize = 0;
        let mut naxis2: usize = 0;
        let mut naxis3: usize = 1;
        let mut bzero: f64 = 0.0;
        let mut bscale: f64 = 1.0;

        let mut _header_bytes = 0;
        let mut end_found = false;

        // Read 2880-byte header blocks
        let mut block = [0u8; 2880];
        while !end_found {
            reader
                .read_exact(&mut block)
                .map_err(|e| format!("Failed to read header block: {}", e))?;
            _header_bytes += 2880;

            for i in (0..2880).step_by(80) {
                let card = &block[i..i + 80];
                let card_str = String::from_utf8_lossy(card);

                let key = card_str[..8].trim();
                let value_part = if card_str.len() > 10 && card_str.chars().nth(8) == Some('=') {
                    card_str[9..].split('/').next().unwrap_or("").trim()
                } else {
                    ""
                };

                match key {
                    "SIMPLE" => {}
                    "BITPIX" => {
                        bitpix = value_part.parse().unwrap_or(16);
                        metadata.bitpix = Some(bitpix);
                    }
                    "NAXIS" => naxis = value_part.parse().unwrap_or(0),
                    "NAXIS1" => naxis1 = value_part.parse().unwrap_or(0),
                    "NAXIS2" => naxis2 = value_part.parse().unwrap_or(0),
                    "NAXIS3" => naxis3 = value_part.parse().unwrap_or(1),
                    "BZERO" => bzero = value_part.parse().unwrap_or(0.0),
                    "BSCALE" => bscale = value_part.parse().unwrap_or(1.0),
                    "EXPTIME" | "EXPOSURE" => {
                        metadata.exposure_time = value_part.parse().ok();
                    }
                    "GAIN" => metadata.gain = value_part.parse().ok(),
                    "ISO" => metadata.iso = value_part.parse().ok(),
                    "TELESCOP" => {
                        metadata.telescope = Some(value_part.trim_matches('\'').trim().to_string())
                    }
                    "INSTRUME" => {
                        metadata.camera = Some(value_part.trim_matches('\'').trim().to_string())
                    }
                    "OBJECT" => {
                        metadata.target = Some(value_part.trim_matches('\'').trim().to_string())
                    }
                    "DATE-OBS" => {
                        metadata.date_obs = Some(value_part.trim_matches('\'').trim().to_string())
                    }
                    "BAYERPAT" | "COLORTYP" => {
                        let pat = value_part.trim_matches('\'').trim().to_uppercase();
                        if !pat.is_empty() {
                            metadata.bayer_pattern = Some(pat);
                        }
                    }
                    "FILTER" | "FILTNAM" => {
                        let filt = value_part.trim_matches('\'').trim().to_string();
                        if !filt.is_empty() {
                            metadata.filter_name = Some(filt);
                        }
                    }
                    "END" => {
                        end_found = true;
                        break;
                    }
                    _ => {}
                }
            }
        }

        if naxis < 2 || naxis1 == 0 || naxis2 == 0 {
            return Err("Invalid FITS dimensions (NAXIS < 2)".to_string());
        }

        let channels = if naxis >= 3 && naxis3 >= 3 { 3 } else { 1 };
        let bytes_per_pixel = match bitpix {
            8 => 1,
            16 => 2,
            32 | -32 => 4,
            -64 => 8,
            _ => return Err(format!("Unsupported BITPIX: {}", bitpix)),
        };

        let raw_data_size = naxis1 * naxis2 * (if naxis >= 3 { naxis3 } else { 1 }) * bytes_per_pixel;
        let mut raw_bytes = vec![0u8; raw_data_size];
        reader
            .read_exact(&mut raw_bytes)
            .map_err(|e| format!("Failed to read pixel data: {}", e))?;

        // Check if raw CFA (Bayer pattern) image should be auto-debayered into RGB
        let is_bayer = channels == 1 && metadata.bayer_pattern.is_some();
        if is_bayer && auto_debayer {
            if let Some(bayer_pat) = metadata
                .bayer_pattern
                .as_ref()
                .and_then(|p| BayerPattern::from_str(p))
            {
                let mut raw_cfa = vec![0.0f32; naxis1 * naxis2];
                for y in 0..naxis2 {
                    for x in 0..naxis1 {
                        let pixel_idx = y * naxis1 + x;
                        let byte_idx = pixel_idx * bytes_per_pixel;

                        let raw_val: f64 = match bitpix {
                            8 => raw_bytes[byte_idx] as f64,
                            16 => {
                                let val = BigEndian::read_i16(&raw_bytes[byte_idx..byte_idx + 2]) as f64;
                                val * bscale + bzero
                            }
                            32 => {
                                let val = BigEndian::read_i32(&raw_bytes[byte_idx..byte_idx + 4]) as f64;
                                val * bscale + bzero
                            }
                            -32 => BigEndian::read_f32(&raw_bytes[byte_idx..byte_idx + 4]) as f64,
                            -64 => BigEndian::read_f64(&raw_bytes[byte_idx..byte_idx + 8]),
                            _ => 0.0,
                        };

                        let normalized: f32 = match bitpix {
                            8 => (raw_val / 255.0) as f32,
                            16 => (raw_val / 65535.0) as f32,
                            32 => (raw_val / 4294967295.0) as f32,
                            -32 | -64 => raw_val as f32,
                            _ => 0.0,
                        };

                        raw_cfa[pixel_idx] = normalized.clamp(0.0, 1.0);
                    }
                }

                let raw_img = AstroImage {
                    width: naxis1,
                    height: naxis2,
                    channels: 1,
                    data: raw_cfa,
                    metadata: metadata.clone(),
                };

                let debayered = Debayer::debayer(&raw_img, bayer_pat)?;

                // Flip RGB vertically for display coordinates
                let mut flipped_rgb = vec![0.0f32; naxis1 * naxis2 * 3];
                for y in 0..naxis2 {
                    let flipped_y = naxis2 - 1 - y;
                    let src_start = y * naxis1 * 3;
                    let dst_start = flipped_y * naxis1 * 3;
                    flipped_rgb[dst_start..dst_start + naxis1 * 3]
                        .copy_from_slice(&debayered.data[src_start..src_start + naxis1 * 3]);
                }

                return Ok(AstroImage {
                    width: naxis1,
                    height: naxis2,
                    channels: 3,
                    data: flipped_rgb,
                    metadata,
                });
            }
        }

        // Standard mono or already-planar RGB loading
        let total_pixels = naxis1 * naxis2 * channels;
        let mut data = vec![0.0f32; total_pixels];
        let plane_size = naxis1 * naxis2;
        for c in 0..channels {
            let plane_byte_offset = c * plane_size * bytes_per_pixel;
            for y in 0..naxis2 {
                let flipped_y = naxis2 - 1 - y;
                for x in 0..naxis1 {
                    let pixel_idx = y * naxis1 + x;
                    let byte_idx = plane_byte_offset + pixel_idx * bytes_per_pixel;

                    let raw_val: f64 = match bitpix {
                        8 => raw_bytes[byte_idx] as f64,
                        16 => {
                            let val = BigEndian::read_i16(&raw_bytes[byte_idx..byte_idx + 2]) as f64;
                            val * bscale + bzero
                        }
                        32 => {
                            let val = BigEndian::read_i32(&raw_bytes[byte_idx..byte_idx + 4]) as f64;
                            val * bscale + bzero
                        }
                        -32 => BigEndian::read_f32(&raw_bytes[byte_idx..byte_idx + 4]) as f64,
                        -64 => BigEndian::read_f64(&raw_bytes[byte_idx..byte_idx + 8]),
                        _ => 0.0,
                    };

                    let normalized: f32 = match bitpix {
                        8 => (raw_val / 255.0) as f32,
                        16 => (raw_val / 65535.0) as f32,
                        32 => (raw_val / 4294967295.0) as f32,
                        -32 | -64 => raw_val as f32,
                        _ => 0.0,
                    };

                    let target_idx = (flipped_y * naxis1 + x) * channels + c;
                    data[target_idx] = normalized.clamp(0.0, 1.0);
                }
            }
        }

        Ok(AstroImage {
            width: naxis1,
            height: naxis2,
            channels,
            data,
            metadata,
        })
    }

    pub fn write_file<P: AsRef<Path>>(image: &AstroImage, path: P) -> Result<(), String> {
        let mut file = File::create(path).map_err(|e| format!("Could not create file: {}", e))?;

        let mut header = String::new();
        header.push_str(&format!("{:<80}", "SIMPLE  =                    T / file conforms to FITS standard"));
        header.push_str(&format!("{:<80}", "BITPIX  =                  -32 / 32-bit floating point"));
        let naxis = if image.channels == 3 { 3 } else { 2 };
        header.push_str(&format!("{:<80}", format!("NAXIS   = {:20} / number of axes", naxis)));
        header.push_str(&format!("{:<80}", format!("NAXIS1  = {:20} / width in pixels", image.width)));
        header.push_str(&format!("{:<80}", format!("NAXIS2  = {:20} / height in pixels", image.height)));
        if image.channels == 3 {
            header.push_str(&format!("{:<80}", "NAXIS3  =                    3 / RGB channels"));
        }
        header.push_str(&format!("{:<80}", "BZERO   =                  0.0 / offset"));
        header.push_str(&format!("{:<80}", "BSCALE  =                  1.0 / scale"));
        header.push_str(&format!("{:<80}", "PROGRAM = 'PhotonForge v0.1.0'  / software"));
        if let Some(target) = &image.metadata.target {
            header.push_str(&format!("{:<80}", format!("OBJECT  = '{:<18}' / observed target", target)));
        }
        if let Some(telescope) = &image.metadata.telescope {
            header.push_str(&format!("{:<80}", format!("TELESCOP= '{:<18}' / telescope", telescope)));
        }
        if let Some(camera) = &image.metadata.camera {
            header.push_str(&format!("{:<80}", format!("INSTRUME= '{:<18}' / detector/camera", camera)));
        }
        if let Some(bayer) = &image.metadata.bayer_pattern {
            header.push_str(&format!("{:<80}", format!("BAYERPAT= '{:<18}' / Bayer pattern", bayer)));
        }
        header.push_str(&format!("{:<80}", "END"));

        // Pad header to multiple of 2880 bytes
        while header.len() % 2880 != 0 {
            header.push_str(&format!("{:<80}", " "));
        }

        file.write_all(header.as_bytes())
            .map_err(|e| format!("Failed to write header: {}", e))?;

        // Write pixel data as IEEE-754 big-endian 32-bit floats
        let plane_size = image.width * image.height;
        let mut bytes = vec![0u8; plane_size * image.channels * 4];

        for c in 0..image.channels {
            let plane_byte_offset = c * plane_size * 4;
            for y in 0..image.height {
                let flipped_y = image.height - 1 - y;
                for x in 0..image.width {
                    let src_idx = (flipped_y * image.width + x) * image.channels + c;
                    let val = image.data[src_idx];
                    let dst_idx = plane_byte_offset + (y * image.width + x) * 4;
                    BigEndian::write_f32(&mut bytes[dst_idx..dst_idx + 4], val);
                }
            }
        }

        file.write_all(&bytes)
            .map_err(|e| format!("Failed to write pixels: {}", e))?;

        // Pad data block to multiple of 2880 bytes
        let remainder = bytes.len() % 2880;
        if remainder != 0 {
            let padding = vec![0u8; 2880 - remainder];
            file.write_all(&padding)
                .map_err(|e| format!("Failed to pad data: {}", e))?;
        }

        Ok(())
    }

    pub fn write_raw_16bit_fits<P: AsRef<Path>>(
        width: usize,
        height: usize,
        pixels: &[u16],
        target_name: &str,
        telescope_name: &str,
        exp_secs: u32,
        bayer: &str,
        path: P,
    ) -> Result<(), String> {
        let mut file = File::create(path).map_err(|e| format!("Could not create file: {}", e))?;

        let mut header = String::new();
        header.push_str(&format!("{:<80}", "SIMPLE  =                    T / file conforms to FITS standard"));
        header.push_str(&format!("{:<80}", "BITPIX  =                   16 / 16-bit signed integer with BZERO"));
        header.push_str(&format!("{:<80}", "NAXIS   =                    2 / number of axes"));
        header.push_str(&format!("{:<80}", format!("NAXIS1  = {:20} / width in pixels", width)));
        header.push_str(&format!("{:<80}", format!("NAXIS2  = {:20} / height in pixels", height)));
        header.push_str(&format!("{:<80}", "BZERO   =                32768 / offset for unsigned 16-bit"));
        header.push_str(&format!("{:<80}", "BSCALE  =                  1.0 / scale"));
        header.push_str(&format!("{:<80}", "PROGRAM = 'PhotonForge'         / software"));
        if !target_name.is_empty() {
            header.push_str(&format!("{:<80}", format!("OBJECT  = '{:<18}' / observed target", target_name)));
        }
        if !telescope_name.is_empty() {
            header.push_str(&format!("{:<80}", format!("TELESCOP= '{:<18}' / telescope", telescope_name)));
        }
        header.push_str(&format!("{:<80}", format!("EXPTIME = {:20} / exposure time in seconds", exp_secs)));
        if !bayer.is_empty() {
            header.push_str(&format!("{:<80}", format!("BAYERPAT= '{:<18}' / Bayer color filter pattern", bayer)));
            header.push_str(&format!("{:<80}", "XBAYROFF=                    1 / Bayer offset X"));
            header.push_str(&format!("{:<80}", "YBAYROFF=                    0 / Bayer offset Y"));
        }
        header.push_str(&format!("{:<80}", "END"));

        // Pad header to multiple of 2880 bytes
        while header.len() % 2880 != 0 {
            header.push_str(&format!("{:<80}", " "));
        }

        file.write_all(header.as_bytes())
            .map_err(|e| format!("Failed to write header: {}", e))?;

        // Write pixel data as big-endian signed 16-bit integers: (pixel as i32 - 32768) as i16
        let total_pixels = width * height;
        let mut bytes = vec![0u8; total_pixels * 2];
        for i in 0..total_pixels {
            let u16_val = if i < pixels.len() { pixels[i] } else { 0 };
            let fits_i16 = (u16_val as i32 - 32768) as i16;
            BigEndian::write_i16(&mut bytes[i * 2..i * 2 + 2], fits_i16);
        }

        file.write_all(&bytes)
            .map_err(|e| format!("Failed to write pixels: {}", e))?;

        let remainder = bytes.len() % 2880;
        if remainder != 0 {
            let padding = vec![0u8; 2880 - remainder];
            file.write_all(&padding)
                .map_err(|e| format!("Failed to pad data: {}", e))?;
        }

        Ok(())
    }
}
