use crate::telescope::sequencer::SequenceTarget;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MosaicScopeModel {
    SeestarS30,
    SeestarS50,
    Custom,
}

impl MosaicScopeModel {
    #[allow(dead_code)]
    pub fn display_name(&self) -> &'static str {
        match self {
            MosaicScopeModel::SeestarS30 => "ZWO Seestar S-30 (150mm f/5, 2.13° × 1.20°)",
            MosaicScopeModel::SeestarS50 => "ZWO Seestar S-50 (250mm f/5, 1.28° × 0.72°)",
            MosaicScopeModel::Custom => "Oma määritys (Custom FOV)",
        }
    }

    pub fn display_short(&self) -> &'static str {
        match self {
            MosaicScopeModel::SeestarS30 => "Seestar S-30 (2.1° × 1.2°)",
            MosaicScopeModel::SeestarS50 => "Seestar S-50 (1.3° × 0.7°)",
            MosaicScopeModel::Custom => "Oma määritys",
        }
    }

    pub fn fov_deg(&self) -> (f32, f32) {
        match self {
            MosaicScopeModel::SeestarS30 => (2.127, 1.196),
            MosaicScopeModel::SeestarS50 => (1.276, 0.718),
            MosaicScopeModel::Custom => (2.0, 1.5),
        }
    }
}

#[derive(Clone, Debug)]
pub struct MosaicGridConfig {
    pub scope_model: MosaicScopeModel,
    pub target_name: String,
    pub center_ra_hours: f64,
    pub center_dec_deg: f64,
    pub cols: usize,
    pub rows: usize,
    pub overlap_pct: f32,
    pub exposure_time_per_panel_mins: u32,
    pub use_lp_filter: bool,
    pub custom_fov_w_deg: f32,
    pub custom_fov_h_deg: f32,
}

impl Default for MosaicGridConfig {
    fn default() -> Self {
        Self {
            scope_model: MosaicScopeModel::SeestarS30,
            target_name: "M31 Andromeda".to_string(),
            center_ra_hours: 0.712,
            center_dec_deg: 41.27,
            cols: 2,
            rows: 2,
            overlap_pct: 20.0,
            exposure_time_per_panel_mins: 30,
            use_lp_filter: false,
            custom_fov_w_deg: 2.13,
            custom_fov_h_deg: 1.20,
        }
    }
}

#[derive(Clone, Debug)]
pub struct MosaicPanel {
    pub panel_index: usize,
    pub total_panels: usize,
    #[allow(dead_code)]
    pub row: usize,
    #[allow(dead_code)]
    pub col: usize,
    pub name: String,
    pub ra_hours: f64,
    pub dec_deg: f64,
    pub fov_w_deg: f32,
    pub fov_h_deg: f32,
    pub exposure_mins: u32,
    pub use_lp: bool,
}

#[derive(Clone, Debug)]
pub struct MosaicPlan {
    pub panels: Vec<MosaicPanel>,
    pub total_fov_w_deg: f32,
    pub total_fov_h_deg: f32,
    pub total_area_sq_deg: f32,
    pub total_exposure_mins: u32,
}

impl MosaicPlan {
    /// Computes the mosaic plan panels and celestial coordinates from grid settings
    pub fn compute(config: &MosaicGridConfig) -> Self {
        let (base_w, base_h) = match config.scope_model {
            MosaicScopeModel::Custom => (config.custom_fov_w_deg, config.custom_fov_h_deg),
            other => other.fov_deg(),
        };

        let cols = config.cols.clamp(1, 6);
        let rows = config.rows.clamp(1, 6);
        let overlap = (config.overlap_pct / 100.0).clamp(0.05, 0.50);

        // Effective step size per tile
        let step_x_deg = base_w * (1.0 - overlap);
        let step_y_deg = base_h * (1.0 - overlap);

        let total_fov_w_deg = base_w + (cols as f32 - 1.0) * step_x_deg;
        let total_fov_h_deg = base_h + (rows as f32 - 1.0) * step_y_deg;
        let total_area_sq_deg = total_fov_w_deg * total_fov_h_deg;

        let total_panels = cols * rows;
        let mut panels = Vec::with_capacity(total_panels);

        // Precompute center declination in radians for RA cosine scaling
        let dec_rad = config.center_dec_deg.to_radians();
        let cos_dec = dec_rad.cos().abs().max(0.05); // prevent division by zero near poles

        let mut idx = 1;
        for r in 0..rows {
            // Row offset from center: top is positive Dec
            let offset_y_deg = (r as f32 - (rows as f32 - 1.0) / 2.0) * step_y_deg;
            let panel_dec = (config.center_dec_deg + offset_y_deg as f64).clamp(-90.0, 90.0);

            for c in 0..cols {
                // Col offset from center: East is positive RA, West is negative
                let offset_x_deg = (c as f32 - (cols as f32 - 1.0) / 2.0) * step_x_deg;

                // Convert degrees on sphere to RA hours: 1 hour = 15 degrees
                let delta_ra_hours = (offset_x_deg as f64 / cos_dec) / 15.0;
                let mut panel_ra = config.center_ra_hours + delta_ra_hours;
                while panel_ra < 0.0 {
                    panel_ra += 24.0;
                }
                while panel_ra >= 24.0 {
                    panel_ra -= 24.0;
                }

                // Sanitize base name to prevent double "Mosaiikki" appending or panel indices
                let mut clean_base = config.target_name.as_str();
                if let Some(pos) = clean_base.find("Mosaiikki") {
                    clean_base = clean_base[..pos].trim();
                }
                if let Some(pos) = clean_base.find('[') {
                    clean_base = clean_base[..pos].trim();
                }
                if let Some(pos) = clean_base.find('(') {
                    clean_base = clean_base[..pos].trim();
                }
                let clean_base = clean_base.trim();
                let display_base = if clean_base.is_empty() { "Mosaiikki" } else { clean_base };

                let panel_name = format!(
                    "{} [R{}C{} {}/{}]",
                    display_base,
                    r + 1,
                    c + 1,
                    idx,
                    total_panels
                );

                panels.push(MosaicPanel {
                    panel_index: idx,
                    total_panels,
                    row: r + 1,
                    col: c + 1,
                    name: panel_name,
                    ra_hours: panel_ra,
                    dec_deg: panel_dec,
                    fov_w_deg: base_w,
                    fov_h_deg: base_h,
                    exposure_mins: config.exposure_time_per_panel_mins,
                    use_lp: config.use_lp_filter,
                });

                idx += 1;
            }
        }

        let total_exposure_mins = (total_panels as u32) * config.exposure_time_per_panel_mins;

        Self {
            panels,
            total_fov_w_deg,
            total_fov_h_deg,
            total_area_sq_deg,
            total_exposure_mins,
        }
    }

    /// Converts all mosaic panels into NightSequencer jobs
    pub fn to_sequence_targets(&self) -> Vec<SequenceTarget> {
        self.panels
            .iter()
            .map(|p| SequenceTarget {
                id: p.panel_index,
                name: p.name.clone(),
                constellation: String::new(),
                ra_hours: p.ra_hours,
                dec_deg: p.dec_deg,
                duration_minutes: p.exposure_mins,
                min_altitude_deg: 20.0,
                use_lp_filter: p.use_lp,
                status: crate::telescope::sequencer::JobStatus::Pending,
                elapsed_seconds: 0,
            })
            .collect()
    }
}
