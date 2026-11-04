use crate::telescope::sequencer::SequenceTarget;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TelescopeProtocol {
    Seestar,      // ZWO Seestar TCP JSON-RPC (port 4700)
    AscomAlpaca,  // ASCOM Alpaca HTTP REST (port 11111 / 32323)
    DwarfLab,     // DwarfLab Open API WebSocket / dwarfAlp (port 9900 / 11111)
}

impl TelescopeProtocol {
    pub fn protocol_name(&self) -> &'static str {
        match self {
            TelescopeProtocol::Seestar => "Seestar TCP",
            TelescopeProtocol::AscomAlpaca => "ASCOM Alpaca",
            TelescopeProtocol::DwarfLab => "DwarfLab API",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TelescopeModel {
    SeestarS30,
    SeestarS30Pro,
    SeestarS50,
    SeestarS50Pro,
    Dwarf3,
    DwarfMini,
    Dwarf2,
    DwarfDraco,
    CelestronOrigin,
    AscomAlpaca,
    Custom,
}

/// Backwards compatibility alias
pub type MosaicScopeModel = TelescopeModel;

#[allow(dead_code)]
impl TelescopeModel {
    pub fn all() -> &'static [TelescopeModel] {
        &[
            TelescopeModel::SeestarS30,
            TelescopeModel::SeestarS30Pro,
            TelescopeModel::SeestarS50,
            TelescopeModel::SeestarS50Pro,
            TelescopeModel::Dwarf3,
            TelescopeModel::DwarfMini,
            TelescopeModel::Dwarf2,
            TelescopeModel::DwarfDraco,
            TelescopeModel::CelestronOrigin,
            TelescopeModel::AscomAlpaca,
            TelescopeModel::Custom,
        ]
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            TelescopeModel::SeestarS30 => "ZWO Seestar S-30 (150mm f/5, 2.13° × 1.20°)",
            TelescopeModel::SeestarS30Pro => "ZWO Seestar S-30 Pro (160mm f/5.3 4K, 3.98° × 2.24°)",
            TelescopeModel::SeestarS50 => "ZWO Seestar S-50 (250mm f/5, 1.28° × 0.72°)",
            TelescopeModel::SeestarS50Pro => "ZWO Seestar S-50 Pro (260mm f/5.2 4K, 2.45° × 1.38°)",
            TelescopeModel::Dwarf3 => "DwarfLab DWARF 3 (150mm f/4.3 4K, 2.93° × 1.65°)",
            TelescopeModel::DwarfMini => "DwarfLab DWARF Mini (150mm f/5, 2.13° × 1.20°)",
            TelescopeModel::Dwarf2 => "DwarfLab DWARF II (100mm f/4.2 4K, 3.19° × 1.79°)",
            TelescopeModel::DwarfDraco => "DwarfLab DRACO (340mm f/3.8, 90mm aukko, 1.62° × 1.21°)",
            TelescopeModel::CelestronOrigin => "Celestron Origin RASA (335mm f/2.2, 1.26° × 0.84°)",
            TelescopeModel::AscomAlpaca => "ASCOM Alpaca (Universal REST Mount)",
            TelescopeModel::Custom => "Oma määritys (Custom Telescope & FOV)",
        }
    }

    pub fn display_short(&self) -> &'static str {
        match self {
            TelescopeModel::SeestarS30 => "Seestar S-30 (2.1° × 1.2°)",
            TelescopeModel::SeestarS30Pro => "Seestar S-30 Pro (4.0° × 2.2°)",
            TelescopeModel::SeestarS50 => "Seestar S-50 (1.3° × 0.7°)",
            TelescopeModel::SeestarS50Pro => "Seestar S-50 Pro (2.5° × 1.4°)",
            TelescopeModel::Dwarf3 => "DWARF 3 (2.9° × 1.7°)",
            TelescopeModel::DwarfMini => "DWARF Mini (2.1° × 1.2°)",
            TelescopeModel::Dwarf2 => "DWARF II (3.2° × 1.8°)",
            TelescopeModel::DwarfDraco => "DWARF DRACO (1.6° × 1.2°)",
            TelescopeModel::CelestronOrigin => "Celestron Origin (1.3° × 0.8°)",
            TelescopeModel::AscomAlpaca => "ASCOM Alpaca",
            TelescopeModel::Custom => "Oma määritys",
        }
    }

    pub fn brand(&self) -> &'static str {
        match self {
            TelescopeModel::SeestarS30
            | TelescopeModel::SeestarS30Pro
            | TelescopeModel::SeestarS50
            | TelescopeModel::SeestarS50Pro => "ZWO Seestar",
            TelescopeModel::Dwarf3
            | TelescopeModel::DwarfMini
            | TelescopeModel::Dwarf2
            | TelescopeModel::DwarfDraco => "DwarfLab",
            TelescopeModel::CelestronOrigin => "Celestron",
            TelescopeModel::AscomAlpaca => "ASCOM Alpaca",
            TelescopeModel::Custom => "Custom",
        }
    }

    pub fn fov_deg(&self) -> (f32, f32) {
        match self {
            TelescopeModel::SeestarS30 => (2.127, 1.196),
            TelescopeModel::SeestarS30Pro => (3.984, 2.242),
            TelescopeModel::SeestarS50 => (1.276, 0.718),
            TelescopeModel::SeestarS50Pro => (2.453, 1.380),
            TelescopeModel::Dwarf3 => (2.933, 1.650),
            TelescopeModel::DwarfMini => (2.127, 1.196),
            TelescopeModel::Dwarf2 => (3.190, 1.794),
            TelescopeModel::DwarfDraco => (1.618, 1.214),
            TelescopeModel::CelestronOrigin => (1.261, 0.841),
            TelescopeModel::AscomAlpaca | TelescopeModel::Custom => (2.000, 1.500),
        }
    }

    pub fn aperture_mm(&self) -> f32 {
        match self {
            TelescopeModel::SeestarS30 | TelescopeModel::SeestarS30Pro => 30.0,
            TelescopeModel::SeestarS50 | TelescopeModel::SeestarS50Pro => 50.0,
            TelescopeModel::Dwarf3 => 35.0,
            TelescopeModel::DwarfMini => 30.0,
            TelescopeModel::Dwarf2 => 24.0,
            TelescopeModel::DwarfDraco => 90.0,
            TelescopeModel::CelestronOrigin => 152.0,
            TelescopeModel::AscomAlpaca => 100.0,
            TelescopeModel::Custom => 80.0,
        }
    }

    pub fn focal_length_mm(&self) -> f32 {
        match self {
            TelescopeModel::SeestarS30 => 150.0,
            TelescopeModel::SeestarS30Pro => 160.0,
            TelescopeModel::SeestarS50 => 250.0,
            TelescopeModel::SeestarS50Pro => 260.0,
            TelescopeModel::Dwarf3 => 150.0,
            TelescopeModel::DwarfMini => 150.0,
            TelescopeModel::Dwarf2 => 100.0,
            TelescopeModel::DwarfDraco => 340.0,
            TelescopeModel::CelestronOrigin => 335.0,
            TelescopeModel::AscomAlpaca => 500.0,
            TelescopeModel::Custom => 400.0,
        }
    }

    pub fn focal_ratio(&self) -> f32 {
        let f = self.focal_length_mm();
        let a = self.aperture_mm();
        if a > 0.0 {
            f / a
        } else {
            5.0
        }
    }

    pub fn sensor_name(&self) -> &'static str {
        match self {
            TelescopeModel::SeestarS30 | TelescopeModel::DwarfMini => "Sony IMX662",
            TelescopeModel::SeestarS30Pro => "Sony IMX585 4K",
            TelescopeModel::SeestarS50 => "Sony IMX462",
            TelescopeModel::SeestarS50Pro => "1/1.2\" 4K CMOS (IMX585)",
            TelescopeModel::Dwarf3 => "Sony IMX678 4K",
            TelescopeModel::Dwarf2 => "Sony IMX415 4K",
            TelescopeModel::DwarfDraco => "1/1.3\" 50MP Sony CMOS",
            TelescopeModel::CelestronOrigin => "Sony IMX178 (6.3 MP)",
            TelescopeModel::AscomAlpaca => "Alpaca Camera",
            TelescopeModel::Custom => "Custom Sensor",
        }
    }

    pub fn resolution(&self) -> (u32, u32) {
        match self {
            TelescopeModel::SeestarS30 | TelescopeModel::SeestarS50 | TelescopeModel::DwarfMini => {
                (1920, 1080)
            }
            TelescopeModel::SeestarS30Pro
            | TelescopeModel::SeestarS50Pro
            | TelescopeModel::Dwarf3
            | TelescopeModel::Dwarf2
            | TelescopeModel::AscomAlpaca
            | TelescopeModel::Custom => (3840, 2160),
            TelescopeModel::DwarfDraco => (4096, 3072),
            TelescopeModel::CelestronOrigin => (3072, 2048),
        }
    }

    pub fn pixel_size_um(&self) -> f32 {
        match self {
            TelescopeModel::SeestarS30
            | TelescopeModel::SeestarS30Pro
            | TelescopeModel::SeestarS50
            | TelescopeModel::SeestarS50Pro
            | TelescopeModel::DwarfMini => 2.9,
            TelescopeModel::Dwarf3 => 2.0,
            TelescopeModel::Dwarf2 => 1.45,
            TelescopeModel::DwarfDraco => 2.4,
            TelescopeModel::CelestronOrigin => 2.4,
            TelescopeModel::AscomAlpaca | TelescopeModel::Custom => 3.76,
        }
    }

    pub fn default_ip(&self) -> &'static str {
        match self {
            TelescopeModel::SeestarS30
            | TelescopeModel::SeestarS30Pro
            | TelescopeModel::SeestarS50
            | TelescopeModel::SeestarS50Pro => "seestar.local",
            TelescopeModel::Dwarf3
            | TelescopeModel::DwarfMini
            | TelescopeModel::Dwarf2
            | TelescopeModel::DwarfDraco => "192.168.88.1",
            TelescopeModel::CelestronOrigin => "192.168.1.1",
            TelescopeModel::AscomAlpaca => "127.0.0.1",
            TelescopeModel::Custom => "192.168.1.100",
        }
    }

    pub fn default_port(&self) -> u16 {
        match self {
            TelescopeModel::SeestarS30
            | TelescopeModel::SeestarS30Pro
            | TelescopeModel::SeestarS50
            | TelescopeModel::SeestarS50Pro => 32323,
            TelescopeModel::Dwarf3
            | TelescopeModel::DwarfMini
            | TelescopeModel::Dwarf2
            | TelescopeModel::DwarfDraco => 9900,
            TelescopeModel::CelestronOrigin | TelescopeModel::AscomAlpaca => 11111,
            TelescopeModel::Custom => 11111,
        }
    }

    pub fn protocol(&self) -> TelescopeProtocol {
        match self {
            TelescopeModel::SeestarS30
            | TelescopeModel::SeestarS30Pro
            | TelescopeModel::SeestarS50
            | TelescopeModel::SeestarS50Pro => TelescopeProtocol::AscomAlpaca,
            TelescopeModel::Dwarf3
            | TelescopeModel::DwarfMini
            | TelescopeModel::Dwarf2
            | TelescopeModel::DwarfDraco => TelescopeProtocol::DwarfLab,
            TelescopeModel::CelestronOrigin | TelescopeModel::AscomAlpaca | TelescopeModel::Custom => {
                TelescopeProtocol::AscomAlpaca
            }
        }
    }

    pub fn smb_share_hint(&self) -> &'static str {
        match self {
            TelescopeModel::SeestarS30
            | TelescopeModel::SeestarS30Pro
            | TelescopeModel::SeestarS50
            | TelescopeModel::SeestarS50Pro => "smb://<IP>/EMMC Images/MyWorks",
            TelescopeModel::Dwarf3
            | TelescopeModel::DwarfMini
            | TelescopeModel::Dwarf2
            | TelescopeModel::DwarfDraco => {
                "http://<IP>:8082/sdcard/ tai USB /Volumes/DWARF"
            }
            TelescopeModel::CelestronOrigin => "smb://<IP>/Origin tai Alpaca Direct",
            TelescopeModel::AscomAlpaca => "ASCOM Alpaca REST API (http://<IP>:11111/)",
            TelescopeModel::Custom => "Oma kansio tai verkkolevy",
        }
    }

    pub fn has_internal_lp_filter(&self) -> bool {
        match self {
            TelescopeModel::SeestarS30
            | TelescopeModel::SeestarS30Pro
            | TelescopeModel::SeestarS50
            | TelescopeModel::SeestarS50Pro
            | TelescopeModel::Dwarf3
            | TelescopeModel::DwarfMini
            | TelescopeModel::DwarfDraco => true,
            _ => false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MosaicGridConfig {
    pub scope_model: TelescopeModel,
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
    pub custom_focal_length_mm: f32,
    pub custom_sensor_w_mm: f32,
    pub custom_sensor_h_mm: f32,
}

impl MosaicGridConfig {
    pub fn compute_custom_fov(
        focal_length_mm: f32,
        sensor_w_mm: f32,
        sensor_h_mm: f32,
    ) -> (f32, f32) {
        if focal_length_mm <= 0.0 {
            return (2.0, 1.5);
        }
        let w_deg = (2.0 * (sensor_w_mm / (2.0 * focal_length_mm)).atan()).to_degrees();
        let h_deg = (2.0 * (sensor_h_mm / (2.0 * focal_length_mm)).atan()).to_degrees();
        (w_deg, h_deg)
    }
}

impl Default for MosaicGridConfig {
    fn default() -> Self {
        Self {
            scope_model: TelescopeModel::SeestarS30,
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
            custom_focal_length_mm: 250.0,
            custom_sensor_w_mm: 11.14,
            custom_sensor_h_mm: 6.26,
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
            TelescopeModel::Custom => (config.custom_fov_w_deg, config.custom_fov_h_deg),
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
                let display_base = if clean_base.is_empty() {
                    "Mosaiikki"
                } else {
                    clean_base
                };

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
