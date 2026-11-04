use crate::core::image::AstroImage;
use crate::processing::alignment::{Star, StarDetector};
use crate::telescope::catalog::{DsoType, SkyObject};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

/// Configuration for the Celestial WCS & Annotation Overlay on the image canvas
#[derive(Clone, Debug)]
pub struct AnnotationConfig {
    pub enabled: bool,
    pub show_grid: bool,
    pub show_dso_objects: bool,
    pub show_detected_stars: bool,
    pub show_compass: bool,
    pub max_annotated_stars: usize,
}

impl Default for AnnotationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            show_grid: true,
            show_dso_objects: true,
            show_detected_stars: true,
            show_compass: true,
            max_annotated_stars: 25,
        }
    }
}

/// A catalog object projected into the current image's WCS frame
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct AnnotatedObject {
    pub id: String,
    pub name: String,
    pub obj_type: DsoType,
    pub ra_hours: f64,
    pub dec_deg: f64,
    pub magnitude: f32,
    pub size_arcmin: f32,
    pub pixel_x: f32,
    pub pixel_y: f32,
    pub radius_px: f32,
}

/// Complete World Coordinate System (WCS) & Plate Solving solution for an AstroImage
#[derive(Clone, Debug)]
pub struct WcsSolution {
    /// Center Right Ascension in hours (0.0 .. 24.0)
    pub center_ra_hours: f64,
    /// Center Declination in degrees (-90.0 .. +90.0)
    pub center_dec_deg: f64,
    /// Reference pixel X (0-indexed display coordinates)
    pub crpix_x: f64,
    /// Reference pixel Y (0-indexed display coordinates)
    pub crpix_y: f64,
    /// Plate scale in arcseconds per pixel
    pub arcsec_per_px: f64,
    /// Position angle / field rotation in degrees (North-up = 0°)
    pub rotation_deg: f64,
    /// Sky parity (+1.0 standard East-left, -1.0 mirrored)
    pub parity: f64,
    pub image_width: usize,
    pub image_height: usize,
    /// Description of how the plate solve was obtained
    pub solver_source: String,
    /// Matched DSO catalog objects within or near the field of view
    pub matched_objects: Vec<AnnotatedObject>,
    /// Brightest detected stars in pixel coordinates
    pub detected_stars: Vec<Star>,
}

impl WcsSolution {
    /// Returns field of view (width_deg, height_deg)
    pub fn fov_deg(&self) -> (f64, f64) {
        let w_deg = (self.image_width as f64 * self.arcsec_per_px) / 3600.0;
        let h_deg = (self.image_height as f64 * self.arcsec_per_px) / 3600.0;
        (w_deg, h_deg)
    }

    /// Converts image pixel coordinates (px, py) to celestial (ra_hours, dec_deg) using Gnomonic (TAN) projection
    pub fn pixel_to_radec(&self, px: f64, py: f64) -> (f64, f64) {
        let scale_rad = (self.arcsec_per_px / 3600.0).to_radians();
        let dx = (px - self.crpix_x) * scale_rad;
        let dy = -(py - self.crpix_y) * scale_rad; // Display Y grows downward, North is up (-Y)

        let theta = self.rotation_deg.to_radians();
        let cos_t = theta.cos();
        let sin_t = theta.sin();

        let xi = -(dx * cos_t - dy * sin_t) * self.parity;
        let eta = dx * sin_t + dy * cos_t;

        let ra0 = (self.center_ra_hours * 15.0).to_radians();
        let dec0 = self.center_dec_deg.to_radians();

        let rho = (xi * xi + eta * eta).sqrt();
        if rho < 1e-12 {
            return (self.center_ra_hours, self.center_dec_deg);
        }

        let c = rho.atan();
        let sin_c = c.sin();
        let cos_c = c.cos();
        let sin_dec0 = dec0.sin();
        let cos_dec0 = dec0.cos();

        let sin_dec = (cos_c * sin_dec0 + (eta * sin_c * cos_dec0) / rho).clamp(-1.0, 1.0);
        let dec_rad = sin_dec.asin();

        let y_term = xi * sin_c;
        let x_term = rho * cos_dec0 * cos_c - eta * sin_dec0 * sin_c;
        let mut ra_rad = ra0 + y_term.atan2(x_term);
        let two_pi = std::f64::consts::TAU;
        ra_rad = ((ra_rad % two_pi) + two_pi) % two_pi;

        let ra_hours = ra_rad.to_degrees() / 15.0;
        let dec_deg = dec_rad.to_degrees();
        (ra_hours, dec_deg)
    }

    /// Converts celestial (ra_hours, dec_deg) to image pixel coordinates (px, py) using Gnomonic (TAN) projection
    pub fn radec_to_pixel(&self, ra_hours: f64, dec_deg: f64) -> Option<(f32, f32)> {
        let ra0 = (self.center_ra_hours * 15.0).to_radians();
        let dec0 = self.center_dec_deg.to_radians();
        let ra = (ra_hours * 15.0).to_radians();
        let dec = dec_deg.to_radians();

        let d_ra = ra - ra0;
        let sin_dec0 = dec0.sin();
        let cos_dec0 = dec0.cos();
        let sin_dec = dec.sin();
        let cos_dec = dec.cos();

        let cos_c = sin_dec0 * sin_dec + cos_dec0 * cos_dec * d_ra.cos();
        if cos_c <= 0.15 {
            return None; // More than ~80° away from field center
        }

        let xi = (cos_dec * d_ra.sin()) / cos_c;
        let eta = (cos_dec0 * sin_dec - sin_dec0 * cos_dec * d_ra.cos()) / cos_c;

        let theta = self.rotation_deg.to_radians();
        let cos_t = theta.cos();
        let sin_t = theta.sin();

        let dx_p = -xi / self.parity;
        let dy_p = eta;

        let dx = dx_p * cos_t + dy_p * sin_t;
        let dy = -dx_p * sin_t + dy_p * cos_t;

        let scale_rad = (self.arcsec_per_px / 3600.0).to_radians().max(1e-9);
        let px = self.crpix_x + dx / scale_rad;
        let py = self.crpix_y - dy / scale_rad;

        Some((px as f32, py as f32))
    }

    /// Re-projects all catalog objects within or near the image frame
    pub fn populate_catalog_objects(&mut self, catalog: &[SkyObject]) {
        self.matched_objects.clear();
        let margin_x = (self.image_width as f32) * 0.25;
        let margin_y = (self.image_height as f32) * 0.25;

        for obj in catalog {
            if let Some((px, py)) = self.radec_to_pixel(obj.ra_hours, obj.dec_deg) {
                if px >= -margin_x
                    && px <= (self.image_width as f32 + margin_x)
                    && py >= -margin_y
                    && py <= (self.image_height as f32 + margin_y)
                {
                    let radius_px = ((obj.size_arcmin * 60.0) / 2.0 / (self.arcsec_per_px as f32))
                        .clamp(12.0, (self.image_width.max(self.image_height) as f32) * 0.48);

                    self.matched_objects.push(AnnotatedObject {
                        id: obj.id.clone(),
                        name: obj.name.clone(),
                        obj_type: obj.obj_type,
                        ra_hours: obj.ra_hours,
                        dec_deg: obj.dec_deg,
                        magnitude: obj.magnitude,
                        size_arcmin: obj.size_arcmin,
                        pixel_x: px,
                        pixel_y: py,
                        radius_px,
                    });
                }
            }
        }
    }
}

pub struct PlateSolver;

impl PlateSolver {
    /// Solves the celestial coordinate frame (WCS) of an `AstroImage`:
    /// 1. Parses FITS WCS / coordinate headers (`CRVAL1`/`CRVAL2`, `RA`/`DEC`, `CD1_1`, `FOCALLEN`, `XPIXSZ`) if available.
    /// 2. Matches target name / filename against the built-in DSO catalog (`M45`, `M31`, `NGC 7000`, etc.) and centers on the star field centroid.
    /// 3. Falls back to telescope telemetry / manual target coordinates if provided.
    pub fn solve(
        image: &AstroImage,
        catalog: &[SkyObject],
        fallback_radec: Option<(f64, f64, &str)>,
    ) -> WcsSolution {
        let detected_stars = StarDetector::detect(image, 60);

        // Estimate default pixel scale from metadata (Seestar S50 = 2.39"/px, Seestar S30 = 3.99"/px)
        let mut arcsec_per_px = Self::estimate_pixel_scale(image);
        let mut rotation_deg = 0.0f64;
        let mut crpix_x = (image.width as f64) * 0.5;
        let mut crpix_y = (image.height as f64) * 0.5;
        let mut center_ra_hours: Option<f64> = None;
        let mut center_dec_deg: Option<f64> = None;
        let mut solver_source = String::new();

        // Tier 1: Try reading FITS WCS / RA-Dec headers from file on disk
        if !image.metadata.file_path.is_empty() {
            if let Some(fits_wcs) = Self::try_parse_fits_wcs(Path::new(&image.metadata.file_path)) {
                center_ra_hours = Some(fits_wcs.ra_hours);
                center_dec_deg = Some(fits_wcs.dec_deg);
                if let Some(scale) = fits_wcs.arcsec_per_px {
                    arcsec_per_px = scale;
                }
                if let Some(rot) = fits_wcs.rotation_deg {
                    rotation_deg = rot;
                }
                if let (Some(cx), Some(cy)) = (fits_wcs.crpix_x, fits_wcs.crpix_y) {
                    crpix_x = cx;
                    crpix_y = cy;
                }
                solver_source = if fits_wcs.has_cd_matrix {
                    "FITS WCS -ratkaisu (CRVAL + CD-matriisi)".to_string()
                } else {
                    "FITS-otsikon koordinaatit (RA / DEC)".to_string()
                };
            }
        }

        // Tier 2: Match OBJECT metadata or filename against the sky catalog
        if center_ra_hours.is_none() || center_dec_deg.is_none() {
            if let Some(matched_obj) = Self::match_target_in_catalog(image, catalog) {
                center_ra_hours = Some(matched_obj.ra_hours);
                center_dec_deg = Some(matched_obj.dec_deg);

                // Refine reference pixel to the flux-weighted centroid of the brightest central stars
                if let Some((star_cx, star_cy)) = Self::compute_star_cluster_centroid(
                    &detected_stars,
                    image.width,
                    image.height,
                ) {
                    crpix_x = star_cx;
                    crpix_y = star_cy;
                }

                solver_source = format!(
                    "Katalogi- & tähtikenttäsovitus ({} – {})",
                    matched_obj.id, matched_obj.name
                );
            }
        }

        // Tier 3: Fallback to active telescope / user target coordinates
        if center_ra_hours.is_none() || center_dec_deg.is_none() {
            if let Some((fb_ra, fb_dec, fb_name)) = fallback_radec {
                center_ra_hours = Some(fb_ra);
                center_dec_deg = Some(fb_dec);
                solver_source = format!("Seestar-kohdekoordinaatit ({})", fb_name);
            } else {
                // Default to M45 Pleiades if completely unknown
                center_ra_hours = Some(3.790);
                center_dec_deg = Some(24.117);
                solver_source = "Oletuskenttä (M45 Plejadit – valitse kohde listalta)".to_string();
            }
        }

        let mut solution = WcsSolution {
            center_ra_hours: center_ra_hours.unwrap_or(3.790),
            center_dec_deg: center_dec_deg.unwrap_or(24.117),
            crpix_x,
            crpix_y,
            arcsec_per_px,
            rotation_deg,
            parity: 1.0,
            image_width: image.width,
            image_height: image.height,
            solver_source,
            matched_objects: Vec::new(),
            detected_stars,
        };

        solution.populate_catalog_objects(catalog);
        solution
    }

    fn estimate_pixel_scale(image: &AstroImage) -> f64 {
        let info = format!(
            "{} {} {}",
            image.metadata.telescope.as_deref().unwrap_or(""),
            image.metadata.camera.as_deref().unwrap_or(""),
            image.metadata.file_name
        )
        .to_uppercase();

        if info.contains("S30") || info.contains("IMX662") {
            3.99 // Seestar S30: f=150mm, 2.9um pixel -> 3.99"/px
        } else {
            2.39 // Seestar S50: f=250mm, 2.9um pixel -> 2.39"/px
        }
    }

    fn match_target_in_catalog<'a>(
        image: &AstroImage,
        catalog: &'a [SkyObject],
    ) -> Option<&'a SkyObject> {
        let search_str = format!(
            "{} {}",
            image.metadata.target.as_deref().unwrap_or(""),
            image.metadata.file_name
        )
        .to_uppercase();

        // Normalize tokens (e.g., "M45", "NGC7000", "NGC 7000", "IC434")
        let compact_search = search_str.replace(['_', '-', ' '], "");

        // First pass: exact catalog ID match (prefer longer IDs first so "M101" matches before "M1" or "M10")
        let mut sorted_cat: Vec<&SkyObject> = catalog.iter().collect();
        sorted_cat.sort_by(|a, b| b.id.len().cmp(&a.id.len()));

        for obj in &sorted_cat {
            let id_up = obj.id.to_uppercase();
            let id_compact = id_up.replace(' ', "");

            // Check word-bounded ID match so "M4" doesn't match "M45"
            if Self::contains_catalog_id(&search_str, &id_up)
                || Self::contains_catalog_id(&compact_search, &id_compact)
            {
                return Some(obj);
            }
        }

        // Second pass: match by Finnish or English common name keywords
        for obj in catalog {
            let name_up = obj.name.to_uppercase();
            for word in name_up.split(|c: char| !c.is_alphanumeric()) {
                if word.len() >= 5 && search_str.contains(word) {
                    return Some(obj);
                }
            }
        }

        None
    }

    fn contains_catalog_id(haystack: &str, id: &str) -> bool {
        if id.is_empty() {
            return false;
        }
        for (idx, _) in haystack.match_indices(id) {
            let after_idx = idx + id.len();
            let next_is_digit = haystack[after_idx..]
                .chars()
                .next()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false);
            if !next_is_digit {
                return true;
            }
        }
        false
    }

    fn compute_star_cluster_centroid(
        stars: &[Star],
        width: usize,
        height: usize,
    ) -> Option<(f64, f64)> {
        if stars.len() < 3 {
            return None;
        }
        let cx = width as f32 * 0.5;
        let cy = height as f32 * 0.5;
        let max_r = (width.min(height) as f32) * 0.35;

        let mut sum_w = 0.0f64;
        let mut sum_x = 0.0f64;
        let mut sum_y = 0.0f64;

        for s in stars.iter().take(20) {
            let dx = s.x - cx;
            let dy = s.y - cy;
            if (dx * dx + dy * dy).sqrt() <= max_r {
                let w = (s.flux as f64).sqrt();
                sum_w += w;
                sum_x += w * (s.x as f64);
                sum_y += w * (s.y as f64);
            }
        }

        if sum_w > 0.0 {
            // Gently blend image geometric center (70%) with bright cluster center (30%)
            let scx = (cx as f64) * 0.70 + (sum_x / sum_w) * 0.30;
            let scy = (cy as f64) * 0.70 + (sum_y / sum_w) * 0.30;
            Some((scx, scy))
        } else {
            None
        }
    }

    fn try_parse_fits_wcs(path: &Path) -> Option<FitsWcsRaw> {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !matches!(ext.as_str(), "fits" | "fit" | "fts") {
            return None;
        }

        let file = File::open(path).ok()?;
        let mut reader = BufReader::new(file);

        let mut ra_deg: Option<f64> = None;
        let mut dec_deg: Option<f64> = None;
        let mut crpix_x: Option<f64> = None;
        let mut crpix_y: Option<f64> = None;
        let mut cdelt1: Option<f64> = None;
        let mut cdelt2: Option<f64> = None;
        let mut cd1_1: Option<f64> = None;
        let mut cd1_2: Option<f64> = None;
        let mut cd2_1: Option<f64> = None;
        let mut cd2_2: Option<f64> = None;
        let mut crota2: Option<f64> = None;
        let mut focallen: Option<f64> = None;
        let mut xpixsz: Option<f64> = None;

        let mut block = [0u8; 2880];
        let mut end_found = false;
        let mut blocks_read = 0;

        while !end_found && blocks_read < 20 {
            if reader.read_exact(&mut block).is_err() {
                break;
            }
            blocks_read += 1;

            for i in (0..2880).step_by(80) {
                let card_str = String::from_utf8_lossy(&block[i..i + 80]);
                let key = card_str[..8].trim();
                let val_str = if card_str.len() > 10 && card_str.chars().nth(8) == Some('=') {
                    card_str[9..]
                        .split('/')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .trim_matches('\'')
                        .trim()
                } else {
                    ""
                };

                match key {
                    "CRVAL1" => ra_deg = val_str.parse::<f64>().ok().or(ra_deg),
                    "CRVAL2" => dec_deg = val_str.parse::<f64>().ok().or(dec_deg),
                    "RA" | "OBJCTRA" => {
                        if ra_deg.is_none() {
                            ra_deg = Self::parse_ra_to_deg(val_str);
                        }
                    }
                    "DEC" | "OBJCTDEC" => {
                        if dec_deg.is_none() {
                            dec_deg = Self::parse_dms_to_deg(val_str);
                        }
                    }
                    "CRPIX1" => crpix_x = val_str.parse::<f64>().ok(),
                    "CRPIX2" => crpix_y = val_str.parse::<f64>().ok(),
                    "CDELT1" => cdelt1 = val_str.parse::<f64>().ok(),
                    "CDELT2" => cdelt2 = val_str.parse::<f64>().ok(),
                    "CD1_1" => cd1_1 = val_str.parse::<f64>().ok(),
                    "CD1_2" => cd1_2 = val_str.parse::<f64>().ok(),
                    "CD2_1" => cd2_1 = val_str.parse::<f64>().ok(),
                    "CD2_2" => cd2_2 = val_str.parse::<f64>().ok(),
                    "CROTA2" | "CROTA1" => crota2 = val_str.parse::<f64>().ok(),
                    "FOCALLEN" => focallen = val_str.parse::<f64>().ok(),
                    "XPIXSZ" | "PIXSIZE1" => xpixsz = val_str.parse::<f64>().ok(),
                    "END" => {
                        end_found = true;
                        break;
                    }
                    _ => {}
                }
            }
        }

        let ra_d = ra_deg?;
        let dec_d = dec_deg?;
        let ra_hours = (ra_d / 15.0).clamp(0.0, 24.0);

        let mut has_cd_matrix = false;
        let mut arcsec_per_px: Option<f64> = None;
        let mut rotation_deg = crota2;

        if let (Some(c11), Some(c22)) = (cd1_1, cd2_2) {
            let c12 = cd1_2.unwrap_or(0.0);
            let c21 = cd2_1.unwrap_or(0.0);
            let scale_deg = ((c11 * c11 + c21 * c21).sqrt() + (c12 * c12 + c22 * c22).sqrt()) * 0.5;
            if scale_deg > 1e-7 {
                arcsec_per_px = Some(scale_deg * 3600.0);
                rotation_deg = Some(c21.atan2(c22).to_degrees());
                has_cd_matrix = true;
            }
        } else if let (Some(d1), Some(d2)) = (cdelt1, cdelt2) {
            let scale_deg = (d1.abs() + d2.abs()) * 0.5;
            if scale_deg > 1e-7 {
                arcsec_per_px = Some(scale_deg * 3600.0);
            }
        } else if let (Some(fl), Some(px_um)) = (focallen, xpixsz) {
            if fl > 10.0 && px_um > 0.5 {
                arcsec_per_px = Some(206.265 * px_um / fl);
            }
        }

        Some(FitsWcsRaw {
            ra_hours,
            dec_deg: dec_d,
            crpix_x,
            crpix_y,
            arcsec_per_px,
            rotation_deg,
            has_cd_matrix,
        })
    }

    fn parse_ra_to_deg(s: &str) -> Option<f64> {
        if let Ok(val) = s.parse::<f64>() {
            // FITS RA keyword is typically in degrees (0..360), or if <= 24 and has HMS it's hours
            return Some(val);
        }
        // Parse sexagesimal "HH MM SS.s" or "HH:MM:SS.s"
        let parts: Vec<&str> = s.split([':', ' ']).filter(|p| !p.is_empty()).collect();
        if parts.len() >= 2 {
            let h = parts[0].parse::<f64>().ok()?;
            let m = parts[1].parse::<f64>().ok()?;
            let sec = parts.get(2).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
            return Some((h + m / 60.0 + sec / 3600.0) * 15.0);
        }
        None
    }

    fn parse_dms_to_deg(s: &str) -> Option<f64> {
        if let Ok(val) = s.parse::<f64>() {
            return Some(val);
        }
        let sign = if s.trim().starts_with('-') { -1.0 } else { 1.0 };
        let clean = s.trim().trim_start_matches(['+', '-']);
        let parts: Vec<&str> = clean.split([':', ' ']).filter(|p| !p.is_empty()).collect();
        if parts.len() >= 2 {
            let d = parts[0].parse::<f64>().ok()?;
            let m = parts[1].parse::<f64>().ok()?;
            let sec = parts.get(2).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
            return Some(sign * (d + m / 60.0 + sec / 3600.0));
        }
        None
    }
}

struct FitsWcsRaw {
    ra_hours: f64,
    dec_deg: f64,
    crpix_x: Option<f64>,
    crpix_y: Option<f64>,
    arcsec_per_px: Option<f64>,
    rotation_deg: Option<f64>,
    has_cd_matrix: bool,
}
