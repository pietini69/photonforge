use super::catalog::SkyObject;
use super::coordinates::{ra_dec_to_alt_az, DEFAULT_LATITUDE_DEG, DEFAULT_LONGITUDE_DEG};
pub use super::seestar::FilterChoice;
use super::seestar::{DitherConfig, SeestarClient, SeestarState};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    Pending,
    Slewing,
    Solving,
    Focusing,
    Stacking,
    Dithering,
    WaitingConnection,
    Completed,
    Skipped,
    Failed,
}

impl JobStatus {
    pub fn display_fi(&self) -> &'static str {
        match self {
            JobStatus::Pending => "Odottaa",
            JobStatus::Slewing => "Kääntyy...",
            JobStatus::Solving => "Kenttätunnistus...",
            JobStatus::Focusing => "Tarkentaa...",
            JobStatus::Stacking => "Pinoaa",
            JobStatus::Dithering => "Ditheröi...",
            JobStatus::WaitingConnection => "Odottaa yhteyttä...",
            JobStatus::Completed => "Valmis",
            JobStatus::Skipped => "Ohitettu",
            JobStatus::Failed => "Epäonnistui",
        }
    }
}

/// Exposure Frame Type for Advanced Sequencer (Light vs Calibration frames)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameType {
    Light,
    Dark,
    Flat,
    Bias,
}

impl FrameType {
    pub fn display_fi(&self) -> &'static str {
        match self {
            FrameType::Light => "Light (Kohde)",
            FrameType::Dark => "Dark (Pimeä)",
            FrameType::Flat => "Flat (Tasakenttä)",
            FrameType::Bias => "Bias (Lukukohina)",
        }
    }

    #[allow(dead_code)]
    pub fn display_en(&self) -> &'static str {
        match self {
            FrameType::Light => "Light (Target)",
            FrameType::Dark => "Dark (Calibration)",
            FrameType::Flat => "Flat (Field)",
            FrameType::Bias => "Bias (Readout)",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            FrameType::Light => "✨",
            FrameType::Dark => "🌑",
            FrameType::Flat => "⬜",
            FrameType::Bias => "⚡",
        }
    }
}

/// Modular Sequence Step
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SequenceStep {
    /// Ajo kohteeseen (GoTo / Slew)
    SlewToTarget {
        name: String,
        ra_hours: f64,
        dec_deg: f64,
        solve_and_center: bool,
        min_altitude_deg: f64,
    },
    /// Valotussarja (Exposures block)
    TakeExposures {
        frame_type: FrameType,
        exposure_secs: u32,
        count: u32,
        dither_every_n: Option<u32>,
    },
    /// Automaattitarkennus (Auto-Focus)
    AutoFocus {
        reason: String,
    },
    /// Kiinteä tarkennusasento (Set Focuser Position)
    SetFocuserPosition {
        position: u32,
    },
    /// Suotimen asetus (Set Filter)
    SetFilter {
        filter: FilterChoice,
    },
    /// Odotusaika / Tauko (Wait / Pause)
    WaitSeconds {
        seconds: u32,
        reason: String,
    },
    /// Kaukoputken ajo lepoasentoon (Park Telescope)
    Park,
}

/// A single manageable item in the Sequence List
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SequenceItem {
    pub id: usize,
    pub enabled: bool,
    pub step: SequenceStep,
    pub status: JobStatus,
    pub progress_str: String,
    pub elapsed_seconds: u32,
}

impl SequenceItem {
    pub fn new(id: usize, step: SequenceStep) -> Self {
        Self {
            id,
            enabled: true,
            step,
            status: JobStatus::Pending,
            progress_str: String::new(),
            elapsed_seconds: 0,
        }
    }

    pub fn icon(&self) -> &'static str {
        match &self.step {
            SequenceStep::SlewToTarget { .. } => "🎯",
            SequenceStep::TakeExposures { frame_type, .. } => frame_type.icon(),
            SequenceStep::AutoFocus { .. } => "🔍",
            SequenceStep::SetFocuserPosition { .. } => "🎛",
            SequenceStep::SetFilter { .. } => "🛡",
            SequenceStep::WaitSeconds { .. } => "⏳",
            SequenceStep::Park => "🏠",
        }
    }

    pub fn type_name_fi(&self) -> &'static str {
        match &self.step {
            SequenceStep::SlewToTarget { .. } => "Ajo kohteeseen",
            SequenceStep::TakeExposures { frame_type, .. } => frame_type.display_fi(),
            SequenceStep::AutoFocus { .. } => "Automaattitarkennus",
            SequenceStep::SetFocuserPosition { .. } => "Tarkennusasento",
            SequenceStep::SetFilter { .. } => "Suodatin",
            SequenceStep::WaitSeconds { .. } => "Odotus",
            SequenceStep::Park => "Parkkiasento",
        }
    }

    pub fn title(&self) -> String {
        match &self.step {
            SequenceStep::SlewToTarget { name, .. } => {
                format!("Ajo: {}", name)
            }
            SequenceStep::TakeExposures { frame_type, exposure_secs, count, .. } => {
                let total_s = exposure_secs * count;
                format!(
                    "{} {} × {}s ({} min)",
                    frame_type.display_fi(),
                    count,
                    exposure_secs,
                    (total_s + 59) / 60
                )
            }
            SequenceStep::AutoFocus { reason } => {
                if reason.is_empty() {
                    "Automaattitarkennus".to_string()
                } else {
                    format!("Automaattitarkennus ({})", reason)
                }
            }
            SequenceStep::SetFocuserPosition { position } => {
                format!("Aseta tarkennus: askel {}", position)
            }
            SequenceStep::SetFilter { filter } => {
                format!("Suodatin: {}", filter.display_fi())
            }
            SequenceStep::WaitSeconds { seconds, reason } => {
                if reason.is_empty() {
                    format!("Odotus: {}s", seconds)
                } else {
                    format!("Odotus: {}s ({})", seconds, reason)
                }
            }
            SequenceStep::Park => "Aja kaukoputki parkkiin".to_string(),
        }
    }

    pub fn estimated_duration_secs(&self) -> u32 {
        match &self.step {
            SequenceStep::SlewToTarget { .. } => 35,
            SequenceStep::TakeExposures { exposure_secs, count, .. } => exposure_secs * count,
            SequenceStep::AutoFocus { .. } => 45,
            SequenceStep::SetFocuserPosition { .. } => 5,
            SequenceStep::SetFilter { .. } => 3,
            SequenceStep::WaitSeconds { seconds, .. } => *seconds,
            SequenceStep::Park => 15,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutoFocusConfig {
    pub enabled: bool,
    pub on_target_start: bool,
    pub use_time_trigger: bool,
    pub interval_minutes: u32,
    pub use_temp_trigger: bool,
    pub temp_delta_c: f32,
}

impl Default for AutoFocusConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            on_target_start: true,
            use_time_trigger: true,
            interval_minutes: 60,
            use_temp_trigger: true,
            temp_delta_c: 1.5,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionGuardConfig {
    pub auto_reconnect: bool,
    pub grace_period_minutes: u32,
    pub resume_stacking_after_reconnect: bool,
}

impl Default for ConnectionGuardConfig {
    fn default() -> Self {
        Self {
            auto_reconnect: true,
            grace_period_minutes: 15,
            resume_stacking_after_reconnect: true,
        }
    }
}

fn default_target_sub_exposure() -> u32 {
    10
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SequenceTarget {
    pub id: usize,
    pub name: String,
    pub constellation: String,
    pub ra_hours: f64,
    pub dec_deg: f64,
    pub duration_minutes: u32,
    #[serde(default = "default_target_sub_exposure")]
    pub sub_exposure_secs: u32,
    pub min_altitude_deg: f64,
    pub use_lp_filter: bool,
    pub status: JobStatus,
    pub elapsed_seconds: u32,
}

#[derive(Clone, Debug)]
pub struct SequencerState {
    pub is_running: bool,
    pub current_job_idx: Option<usize>,
    pub current_item_idx: Option<usize>,
    pub current_step_description: String,
    pub auto_park_at_end: bool,
    pub autofocus_config: AutoFocusConfig,
    pub dither_config: DitherConfig,
    pub connection_guard: ConnectionGuardConfig,
    pub default_sub_exposure_secs: u32,
    pub af_runs_count: u32,
    pub dithers_count: u32,
    pub reconnect_events_count: u32,
    pub last_af_temp_c: Option<f32>,
    pub minutes_since_last_af: u32,
    pub items: Vec<SequenceItem>,
    pub targets: Vec<SequenceTarget>,
    pub logs: Vec<String>,
}

impl Default for SequencerState {
    fn default() -> Self {
        let targets = vec![
            SequenceTarget {
                id: 1,
                name: "M45 Plejadit".to_string(),
                constellation: "Tau".to_string(),
                ra_hours: 3.79,
                dec_deg: 24.11,
                duration_minutes: 60,
                sub_exposure_secs: 10,
                min_altitude_deg: 25.0,
                use_lp_filter: false,
                status: JobStatus::Pending,
                elapsed_seconds: 0,
            },
            SequenceTarget {
                id: 2,
                name: "M42 Orionin suuri kaasusumu".to_string(),
                constellation: "Ori".to_string(),
                ra_hours: 5.59,
                dec_deg: -5.39,
                duration_minutes: 90,
                sub_exposure_secs: 10,
                min_altitude_deg: 20.0,
                use_lp_filter: true,
                status: JobStatus::Pending,
                elapsed_seconds: 0,
            },
        ];

        let items = vec![
            SequenceItem::new(1, SequenceStep::SlewToTarget {
                name: "M45 Plejadit".to_string(),
                ra_hours: 3.79,
                dec_deg: 24.11,
                solve_and_center: true,
                min_altitude_deg: 25.0,
            }),
            SequenceItem::new(2, SequenceStep::SetFilter {
                filter: FilterChoice::Normal,
            }),
            SequenceItem::new(3, SequenceStep::AutoFocus {
                reason: "M45 alustus".to_string(),
            }),
            SequenceItem::new(4, SequenceStep::TakeExposures {
                frame_type: FrameType::Light,
                exposure_secs: 10,
                count: 360,
                dither_every_n: Some(3),
            }),
            SequenceItem::new(5, SequenceStep::SlewToTarget {
                name: "M42 Orionin suuri kaasusumu".to_string(),
                ra_hours: 5.59,
                dec_deg: -5.39,
                solve_and_center: true,
                min_altitude_deg: 20.0,
            }),
            SequenceItem::new(6, SequenceStep::SetFilter {
                filter: FilterChoice::LightPollution,
            }),
            SequenceItem::new(7, SequenceStep::AutoFocus {
                reason: "M42 suotimen vaihto".to_string(),
            }),
            SequenceItem::new(8, SequenceStep::TakeExposures {
                frame_type: FrameType::Light,
                exposure_secs: 10,
                count: 540,
                dither_every_n: Some(3),
            }),
        ];

        Self {
            is_running: false,
            current_job_idx: None,
            current_item_idx: None,
            current_step_description: "Sekvensseri valmiina.".to_string(),
            auto_park_at_end: true,
            autofocus_config: AutoFocusConfig::default(),
            dither_config: DitherConfig::default(),
            connection_guard: ConnectionGuardConfig::default(),
            default_sub_exposure_secs: 10,
            af_runs_count: 0,
            dithers_count: 0,
            reconnect_events_count: 0,
            last_af_temp_c: None,
            minutes_since_last_af: 0,
            items,
            targets,
            logs: vec![
                format!("[{}] Yösekvensseri alustettu (Modulaarinen suorituslista, Ditheröinti ja Yhteysvahti valmiina).", Utc::now().format("%H:%M:%S")),
            ],
        }
    }
}

pub struct NightSequencer {
    pub state: Arc<Mutex<SequencerState>>,
    cancel_signal: Arc<AtomicBool>,
}

impl Default for NightSequencer {
    fn default() -> Self {
        Self::new()
    }
}

impl NightSequencer {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(SequencerState::default())),
            cancel_signal: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Add a modular SequenceStep directly to the sequence list
    pub fn add_item(&self, step: SequenceStep) -> usize {
        let mut s = self.state.lock().unwrap();
        let next_id = s.items.len() + 1;
        let item = SequenceItem::new(next_id, step);
        s.logs.push(format!("[{}] Lisätty sekvenssiin: {}", Utc::now().format("%H:%M:%S"), item.title()));
        s.items.push(item);
        s.items.len() - 1
    }

    /// Alias for add_item
    #[allow(dead_code)]
    pub fn add_step(&self, step: SequenceStep) -> usize {
        self.add_item(step)
    }

    /// Remove a sequence item by index
    pub fn remove_item(&self, idx: usize) {
        let mut s = self.state.lock().unwrap();
        if idx < s.items.len() {
            let removed = s.items.remove(idx);
            s.logs.push(format!("[{}] Poistettu sekvenssistä: {}", Utc::now().format("%H:%M:%S"), removed.title()));
        }
    }

    /// Move sequence item up by one position
    pub fn move_item_up(&self, idx: usize) {
        let mut s = self.state.lock().unwrap();
        if idx > 0 && idx < s.items.len() {
            s.items.swap(idx, idx - 1);
        }
    }

    /// Move sequence item down by one position
    pub fn move_item_down(&self, idx: usize) {
        let mut s = self.state.lock().unwrap();
        if idx + 1 < s.items.len() {
            s.items.swap(idx, idx + 1);
        }
    }

    /// Duplicate an item in place
    pub fn duplicate_item(&self, idx: usize) {
        let mut s = self.state.lock().unwrap();
        if idx < s.items.len() {
            let mut dup = s.items[idx].clone();
            dup.id = s.items.len() + 1;
            dup.status = JobStatus::Pending;
            dup.elapsed_seconds = 0;
            dup.progress_str.clear();
            s.items.insert(idx + 1, dup);
        }
    }

    /// Toggle enabled state of an item
    pub fn toggle_item(&self, idx: usize) {
        let mut s = self.state.lock().unwrap();
        if idx < s.items.len() {
            s.items[idx].enabled = !s.items[idx].enabled;
        }
    }

    /// Adds a dedicated Dark Calibration frames sequence (Filter: Dark -> Exposures -> Filter: Normal)
    pub fn add_dark_calibration_sequence(&self, exposure_secs: u32, count: u32) {
        let mut s = self.state.lock().unwrap();
        let id_start = s.items.len() + 1;
        s.items.push(SequenceItem::new(id_start, SequenceStep::SetFilter { filter: FilterChoice::Dark }));
        s.items.push(SequenceItem::new(id_start + 1, SequenceStep::TakeExposures {
            frame_type: FrameType::Dark,
            exposure_secs,
            count,
            dither_every_n: None,
        }));
        s.items.push(SequenceItem::new(id_start + 2, SequenceStep::SetFilter { filter: FilterChoice::Normal }));
        s.logs.push(format!(
            "[{}] Lisätty pimeäkuvien (Dark) kalibrointisarja: {} × {}s",
            Utc::now().format("%H:%M:%S"), count, exposure_secs
        ));
    }

    #[allow(dead_code)]
    pub fn add_target(&self, obj: &SkyObject, duration_minutes: u32, use_lp: bool) {
        let def_exp = self.state.lock().unwrap().default_sub_exposure_secs;
        self.add_target_with_exposure(obj, duration_minutes, def_exp, use_lp);
    }

    pub fn add_target_with_exposure(
        &self,
        obj: &SkyObject,
        duration_minutes: u32,
        sub_exposure_secs: u32,
        use_lp: bool,
    ) {
        let mut s = self.state.lock().unwrap();
        let next_id = s.targets.len() + 1;
        let name = if obj.id == obj.name || obj.name.is_empty() {
            obj.id.clone()
        } else if obj.name.starts_with(&obj.id) {
            obj.name.clone()
        } else {
            let clean_name = obj.name.split('/').next().unwrap_or(&obj.name).trim();
            format!("{} {}", obj.id, clean_name)
        };
        let sub_exp = sub_exposure_secs.clamp(1, 300);

        // Keep legacy targets synchronized
        s.targets.push(SequenceTarget {
            id: next_id,
            name: name.clone(),
            constellation: obj.constellation.clone(),
            ra_hours: obj.ra_hours,
            dec_deg: obj.dec_deg,
            duration_minutes,
            sub_exposure_secs: sub_exp,
            min_altitude_deg: 20.0,
            use_lp_filter: use_lp,
            status: JobStatus::Pending,
            elapsed_seconds: 0,
        });

        // Add modular steps to items list
        let filter = if use_lp { FilterChoice::LightPollution } else { FilterChoice::Normal };
        let count = ((duration_minutes * 60) / sub_exp).max(1);
        let step_start = s.items.len() + 1;

        s.items.push(SequenceItem::new(step_start, SequenceStep::SlewToTarget {
            name: name.clone(),
            ra_hours: obj.ra_hours,
            dec_deg: obj.dec_deg,
            solve_and_center: true,
            min_altitude_deg: 20.0,
        }));
        s.items.push(SequenceItem::new(step_start + 1, SequenceStep::SetFilter { filter }));
        s.items.push(SequenceItem::new(step_start + 2, SequenceStep::AutoFocus { reason: name.clone() }));
        s.items.push(SequenceItem::new(step_start + 3, SequenceStep::TakeExposures {
            frame_type: FrameType::Light,
            exposure_secs: sub_exp,
            count,
            dither_every_n: Some(3),
        }));

        s.logs.push(format!(
            "[{}] Lisätty kohteelle {} modulaarinen kuvaussarja (Ajo -> Suodin -> AF -> {} × {}s Light)",
            Utc::now().format("%H:%M:%S"),
            name,
            count,
            sub_exp
        ));
    }

    pub fn add_sequence_target(&self, target: SequenceTarget) {
        let mut s = self.state.lock().unwrap();
        let next_id = s.targets.len() + 1;
        let mut t = target;
        t.id = next_id;

        let filter = if t.use_lp_filter { FilterChoice::LightPollution } else { FilterChoice::Normal };
        let count = ((t.duration_minutes * 60) / t.sub_exposure_secs.max(1)).max(1);
        let step_start = s.items.len() + 1;

        s.items.push(SequenceItem::new(step_start, SequenceStep::SlewToTarget {
            name: t.name.clone(),
            ra_hours: t.ra_hours,
            dec_deg: t.dec_deg,
            solve_and_center: true,
            min_altitude_deg: t.min_altitude_deg,
        }));
        s.items.push(SequenceItem::new(step_start + 1, SequenceStep::SetFilter { filter }));
        s.items.push(SequenceItem::new(step_start + 2, SequenceStep::AutoFocus { reason: t.name.clone() }));
        s.items.push(SequenceItem::new(step_start + 3, SequenceStep::TakeExposures {
            frame_type: FrameType::Light,
            exposure_secs: t.sub_exposure_secs,
            count,
            dither_every_n: Some(3),
        }));

        s.logs.push(format!(
            "[{}] Lisätty sekvenssiin kohde: {} ({} min, {})",
            Utc::now().format("%H:%M:%S"),
            t.name,
            t.duration_minutes,
            if t.use_lp_filter { "LP-suodatin" } else { "Normaali" }
        ));
        s.targets.push(t);
    }

    #[allow(dead_code)]
    pub fn remove_target(&self, idx: usize) {
        let mut s = self.state.lock().unwrap();
        if idx < s.targets.len() {
            let removed = s.targets.remove(idx);
            s.logs.push(format!("[{}] Poistettu kohteista: {}", Utc::now().format("%H:%M:%S"), removed.name));
        }
    }

    pub fn clear_all(&self) {
        let mut s = self.state.lock().unwrap();
        s.targets.clear();
        s.items.clear();
        s.logs.push(format!("[{}] Kuvauslista tyhjennetty.", Utc::now().format("%H:%M:%S")));
    }

    /// Helper that waits for connection to recover within the configured grace period.
    /// Returns `true` if connected, `false` if cancelled or grace period expired.
    fn wait_for_connection(
        state_clone: &Arc<Mutex<SequencerState>>,
        cancel_clone: &Arc<AtomicBool>,
        client: &Arc<SeestarClient>,
        item_idx: Option<usize>,
        grace_mins: u32,
    ) -> bool {
        let telem = client.get_telemetry();
        if telem.connected || telem.is_simulation {
            return true;
        }

        let grace_secs = (grace_mins.max(1) as u64) * 60;
        let outage_start = Instant::now();
        {
            let mut s = state_clone.lock().unwrap();
            s.reconnect_events_count += 1;
            if let Some(idx) = item_idx {
                if idx < s.items.len() {
                    s.items[idx].status = JobStatus::WaitingConnection;
                }
                if idx < s.targets.len() {
                    s.targets[idx].status = JobStatus::WaitingConnection;
                }
            }
            s.current_step_description = format!(
                "⚠ Yhteys katkesi! Odotetaan yhteyden palautumista (armonaika {} min)...",
                grace_mins
            );
            s.logs.push(format!(
                "[{}] ⚠ Yhteys kaukoputkeen katkesi – yhteysvahti aktivoitu (armonaika {} min).",
                Utc::now().format("%H:%M:%S"),
                grace_mins
            ));
        }

        if telem.state == SeestarState::Disconnected {
            client.connect(&telem.ip_address);
        }

        while outage_start.elapsed().as_secs() < grace_secs {
            if cancel_clone.load(Ordering::SeqCst) {
                return false;
            }
            thread::sleep(Duration::from_secs(1));

            let cur_telem = client.get_telemetry();
            if cur_telem.connected || cur_telem.is_simulation {
                let elapsed_outage = outage_start.elapsed().as_secs();
                let mut s = state_clone.lock().unwrap();
                s.logs.push(format!(
                    "[{}] ✓ Yhteys kaukoputkeen palautui ({}s katkoksen jälkeen)! Jatketaan yöajoa.",
                    Utc::now().format("%H:%M:%S"),
                    elapsed_outage
                ));
                return true;
            }

            let waited = outage_start.elapsed().as_secs();
            let remain = grace_secs.saturating_sub(waited);
            let mut s = state_clone.lock().unwrap();
            s.current_step_description = format!(
                "⚠ Odottaa yhteyttä Seestariin... (yritys {}/{}, armonaikaa {:02}m {:02}s)",
                cur_telem.reconnect_attempts,
                cur_telem.max_reconnect_attempts,
                remain / 60,
                remain % 60
            );
        }

        let mut s = state_clone.lock().unwrap();
        s.logs.push(format!(
            "[{}] ❌ Yhteyskatkos kesti yli armonajan ({} min). Keskeytetään toiminto turvallisesti.",
            Utc::now().format("%H:%M:%S"),
            grace_mins
        ));
        false
    }

    /// Runs an Auto-Focus cycle and waits for completion.
    fn execute_autofocus(
        state_clone: &Arc<Mutex<SequencerState>>,
        cancel_clone: &Arc<AtomicBool>,
        client: &Arc<SeestarClient>,
        item_idx: Option<usize>,
        reason: &str,
    ) -> Option<f32> {
        {
            let mut s = state_clone.lock().unwrap();
            if let Some(idx) = item_idx {
                if idx < s.items.len() {
                    s.items[idx].status = JobStatus::Focusing;
                }
                if idx < s.targets.len() {
                    s.targets[idx].status = JobStatus::Focusing;
                }
            }
            s.current_step_description = format!("🔍 Automaattitarkennus käynnissä ({})...", reason);
            s.logs.push(format!(
                "[{}] 🔍 Käynnistetään automaattitarkennus ({})",
                Utc::now().format("%H:%M:%S"),
                reason
            ));
        }

        client.auto_focus();

        let af_wait_start = Instant::now();
        thread::sleep(Duration::from_millis(400));
        while af_wait_start.elapsed().as_secs() < 90 {
            if cancel_clone.load(Ordering::SeqCst) {
                return None;
            }
            let telem = client.get_telemetry();
            if telem.state != SeestarState::Focusing {
                let mut s = state_clone.lock().unwrap();
                s.af_runs_count += 1;
                s.last_af_temp_c = Some(telem.temp_c);
                s.minutes_since_last_af = 0;
                s.logs.push(format!(
                    "[{}] ✓ Tarkennus valmis: positio {} (lämpötila {:.1} °C)",
                    Utc::now().format("%H:%M:%S"),
                    telem.focus_position,
                    telem.temp_c
                ));
                return Some(telem.temp_c);
            }
            thread::sleep(Duration::from_millis(300));
        }

        let telem = client.get_telemetry();
        let mut s = state_clone.lock().unwrap();
        s.logs.push(format!(
            "[{}] ⚠ Automaattitarkennuksen aikakatkaisu, jatketaan nykyisellä tarkennuksella.",
            Utc::now().format("%H:%M:%S")
        ));
        Some(telem.temp_c)
    }

    /// Starts execution loop over all items in the sequence list
    pub fn start(&self, client: Arc<SeestarClient>) {
        let mut s = self.state.lock().unwrap();
        if s.is_running {
            return;
        }

        // If items are empty but targets exist, convert targets to items
        if s.items.is_empty() && !s.targets.is_empty() {
            let targets = s.targets.clone();
            s.items.clear();
            for t in targets {
                let filter = if t.use_lp_filter { FilterChoice::LightPollution } else { FilterChoice::Normal };
                let count = ((t.duration_minutes * 60) / t.sub_exposure_secs.max(1)).max(1);
                let step_start = s.items.len() + 1;
                s.items.push(SequenceItem::new(step_start, SequenceStep::SlewToTarget {
                    name: t.name.clone(),
                    ra_hours: t.ra_hours,
                    dec_deg: t.dec_deg,
                    solve_and_center: true,
                    min_altitude_deg: t.min_altitude_deg,
                }));
                s.items.push(SequenceItem::new(step_start + 1, SequenceStep::SetFilter { filter }));
                s.items.push(SequenceItem::new(step_start + 2, SequenceStep::AutoFocus { reason: t.name.clone() }));
                s.items.push(SequenceItem::new(step_start + 3, SequenceStep::TakeExposures {
                    frame_type: FrameType::Light,
                    exposure_secs: t.sub_exposure_secs,
                    count,
                    dither_every_n: Some(3),
                }));
            }
        }

        if s.items.is_empty() {
            s.logs.push(format!("[{}] Kuvauslista on tyhjä, ei toimintoja suoritettavaksi!", Utc::now().format("%H:%M:%S")));
            return;
        }

        s.is_running = true;
        s.current_item_idx = Some(0);
        s.current_job_idx = Some(0);
        s.af_runs_count = 0;
        s.dithers_count = 0;
        s.reconnect_events_count = 0;
        let af_on = s.autofocus_config.enabled;
        let dither_on = s.dither_config.enabled;
        let guard_on = s.connection_guard.auto_reconnect;
        let items_count = s.items.len();
        s.logs.push(format!(
            "[{}] ▶ YÖAJO KÄYNNISTETTY! (Toimintoja: {}, AF: {}, Dither: {}, Yhteysvahti: {})",
            Utc::now().format("%H:%M:%S"),
            items_count,
            if af_on { "PÄÄLLÄ" } else { "POIS" },
            if dither_on { "PÄÄLLÄ" } else { "POIS" },
            if guard_on { "PÄÄLLÄ" } else { "POIS" }
        ));
        let initial_dither = s.dither_config;
        let initial_conn_guard = s.connection_guard;
        drop(s);

        client.configure_dither(initial_dither);
        client.set_auto_reconnect(initial_conn_guard.auto_reconnect);

        self.cancel_signal.store(false, Ordering::SeqCst);
        let state_clone = Arc::clone(&self.state);
        let cancel_clone = Arc::clone(&self.cancel_signal);

        thread::spawn(move || {
            let total_items = {
                let s = state_clone.lock().unwrap();
                s.items.len()
            };

            let mut _last_af_instant = Instant::now();
            let mut _last_af_temp_c = client.get_telemetry().temp_c;

            for i in 0..total_items {
                if cancel_clone.load(Ordering::SeqCst) {
                    break;
                }

                // 1. Prepare item
                let (enabled, step, conn_guard, dither_cfg, _af_cfg) = {
                    let mut s = state_clone.lock().unwrap();
                    s.current_item_idx = Some(i);
                    s.current_job_idx = Some(i);
                    if i < s.items.len() {
                        let is_en = s.items[i].enabled;
                        if !is_en {
                            s.items[i].status = JobStatus::Skipped;
                        } else {
                            s.items[i].status = JobStatus::Pending;
                            s.items[i].elapsed_seconds = 0;
                        }
                        (is_en, s.items[i].step.clone(), s.connection_guard, s.dither_config, s.autofocus_config)
                    } else {
                        break;
                    }
                };

                if !enabled {
                    continue;
                }

                // Ensure connection is healthy before starting step
                if !Self::wait_for_connection(&state_clone, &cancel_clone, &client, Some(i), conn_guard.grace_period_minutes) {
                    let mut s = state_clone.lock().unwrap();
                    if i < s.items.len() {
                        s.items[i].status = if cancel_clone.load(Ordering::SeqCst) {
                            JobStatus::Skipped
                        } else {
                            JobStatus::Failed
                        };
                    }
                    break;
                }

                client.configure_dither(dither_cfg);
                client.set_auto_reconnect(conn_guard.auto_reconnect);

                match step {
                    SequenceStep::SlewToTarget { name, ra_hours, dec_deg, solve_and_center, min_altitude_deg } => {
                        {
                            let mut s = state_clone.lock().unwrap();
                            if i < s.items.len() {
                                s.items[i].status = JobStatus::Slewing;
                            }
                            s.current_step_description = format!("Käännytään kohteeseen {}...", name);
                            s.logs.push(format!(
                                "[{}] GoTo: Käännytään kohteeseen {} (RA: {:.2}h, Dec: {:+.1}°)...",
                                Utc::now().format("%H:%M:%S"), name, ra_hours, dec_deg
                            ));
                        }

                        // Check altitude warning
                        let now_utc = Utc::now();
                        let (current_alt, _) = ra_dec_to_alt_az(ra_hours, dec_deg, DEFAULT_LATITUDE_DEG, DEFAULT_LONGITUDE_DEG, &now_utc);
                        if current_alt < min_altitude_deg {
                            let mut s = state_clone.lock().unwrap();
                            s.logs.push(format!(
                                "[{}] Varoitus: Kohde {} on alle minimikorkeuden ({:.1}° < {:.1}°), siirrytään kohteeseen silti.",
                                Utc::now().format("%H:%M:%S"), name, current_alt, min_altitude_deg
                            ));
                        }

                        client.goto_target(&name, ra_hours, dec_deg);

                        for _ in 0..90 {
                            if cancel_clone.load(Ordering::SeqCst) {
                                break;
                            }
                            thread::sleep(Duration::from_millis(200));
                            let telem = client.get_telemetry();
                            if solve_and_center && telem.state == SeestarState::Solving {
                                let mut s = state_clone.lock().unwrap();
                                if i < s.items.len() {
                                    s.items[i].status = JobStatus::Solving;
                                }
                                s.current_step_description = format!("Kenttätunnistus (Plate Solving): {}...", name);
                            } else if telem.state == SeestarState::Tracking {
                                break;
                            }
                        }

                        let mut s = state_clone.lock().unwrap();
                        if i < s.items.len() {
                            s.items[i].status = if cancel_clone.load(Ordering::SeqCst) {
                                JobStatus::Skipped
                            } else {
                                JobStatus::Completed
                            };
                        }
                        s.logs.push(format!("[{}] ✓ Kohde valmis ja seuranta päällä: {}", Utc::now().format("%H:%M:%S"), name));
                    }

                    SequenceStep::SetFilter { filter } => {
                        {
                            let mut s = state_clone.lock().unwrap();
                            s.current_step_description = format!("Asetetaan suodatin: {}...", filter.display_fi());
                            s.logs.push(format!("[{}] 🛡 Asetetaan suodatin: {}", Utc::now().format("%H:%M:%S"), filter.display_fi()));
                        }

                        client.set_filter_choice(filter);
                        thread::sleep(Duration::from_secs(2));

                        let mut s = state_clone.lock().unwrap();
                        if i < s.items.len() {
                            s.items[i].status = JobStatus::Completed;
                        }
                        s.logs.push(format!("[{}] ✓ Suodatin asetettu: {}", Utc::now().format("%H:%M:%S"), filter.display_fi()));
                    }

                    SequenceStep::AutoFocus { reason } => {
                        let desc_reason = if reason.is_empty() { "määrätty tarkennus".to_string() } else { reason.clone() };
                        if let Some(new_temp) = Self::execute_autofocus(&state_clone, &cancel_clone, &client, Some(i), &desc_reason) {
                            _last_af_temp_c = new_temp;
                            _last_af_instant = Instant::now();
                        }
                        let mut s = state_clone.lock().unwrap();
                        if i < s.items.len() {
                            s.items[i].status = JobStatus::Completed;
                        }
                    }

                    SequenceStep::SetFocuserPosition { position } => {
                        {
                            let mut s = state_clone.lock().unwrap();
                            if i < s.items.len() {
                                s.items[i].status = JobStatus::Focusing;
                            }
                            s.current_step_description = format!("Asetetaan tarkennusmoottori asentoon {}...", position);
                            s.logs.push(format!("[{}] 🎛 Asetetaan tarkennusasento: {}", Utc::now().format("%H:%M:%S"), position));
                        }

                        client.set_focus_position(position);
                        thread::sleep(Duration::from_secs(2));

                        let mut s = state_clone.lock().unwrap();
                        if i < s.items.len() {
                            s.items[i].status = JobStatus::Completed;
                        }
                        s.logs.push(format!("[{}] ✓ Tarkennusmoottorin asento asetettu: {}", Utc::now().format("%H:%M:%S"), position));
                    }

                    SequenceStep::TakeExposures { frame_type, exposure_secs, count, dither_every_n } => {
                        let exp_s = exposure_secs.max(1);
                        let total_frames = count.max(1);
                        {
                            let mut s = state_clone.lock().unwrap();
                            if i < s.items.len() {
                                s.items[i].status = JobStatus::Stacking;
                            }
                            s.current_step_description = format!(
                                "Valotussarja käynnissä: {} {} × {}s (yht. {} s)...",
                                frame_type.display_fi(), total_frames, exp_s, total_frames * exp_s
                            );
                            s.logs.push(format!(
                                "[{}] 📸 Aloitetaan {}: {} ruutua × {}s",
                                Utc::now().format("%H:%M:%S"), frame_type.display_fi(), total_frames, exp_s
                            ));
                        }

                        client.set_exposure_time(exp_s);
                        if frame_type == FrameType::Light {
                            client.start_stacking();
                        } else {
                            client.start_calibration_exposure(exp_s, frame_type == FrameType::Dark);
                        }

                        let mut completed_frames: u32 = 0;
                        let mut last_dithered_frame: u32 = 0;
                        let mut step_failed = false;

                        while completed_frames < total_frames {
                            if cancel_clone.load(Ordering::SeqCst) {
                                break;
                            }

                            // Connection check
                            let telem = client.get_telemetry();
                            if !telem.connected && !telem.is_simulation {
                                if !Self::wait_for_connection(&state_clone, &cancel_clone, &client, Some(i), conn_guard.grace_period_minutes) {
                                    step_failed = true;
                                    break;
                                }
                                if frame_type == FrameType::Light {
                                    client.start_stacking();
                                } else {
                                    client.start_calibration_exposure(exp_s, frame_type == FrameType::Dark);
                                }
                            }

                            // Wait exposure time in 1-second chunks for responsive cancellation
                            for _ in 0..exp_s {
                                if cancel_clone.load(Ordering::SeqCst) {
                                    break;
                                }
                                thread::sleep(Duration::from_secs(1));
                                let mut s = state_clone.lock().unwrap();
                                if i < s.items.len() {
                                    s.items[i].elapsed_seconds += 1;
                                }
                            }

                            if cancel_clone.load(Ordering::SeqCst) {
                                break;
                            }

                            completed_frames += 1;

                            {
                                let mut s = state_clone.lock().unwrap();
                                if i < s.items.len() {
                                    s.items[i].progress_str = format!("{}/{} ruutua", completed_frames, total_frames);
                                }
                                let rem_s = (total_frames - completed_frames) * exp_s;
                                s.current_step_description = format!(
                                    "{} ruutu {}/{} otettu – Jäljellä {:02}m {:02}s",
                                    frame_type.display_fi(), completed_frames, total_frames, rem_s / 60, rem_s % 60
                                );
                            }

                            // Dither check for Light frames
                            let dither_interval = dither_every_n.unwrap_or(dither_cfg.interval_frames);
                            if frame_type == FrameType::Light
                                && dither_cfg.enabled
                                && dither_interval > 0
                                && completed_frames > last_dithered_frame
                                && completed_frames % dither_interval == 0
                                && completed_frames < total_frames
                            {
                                last_dithered_frame = completed_frames;
                                {
                                    let mut s = state_clone.lock().unwrap();
                                    s.dithers_count += 1;
                                    if i < s.items.len() {
                                        s.items[i].status = JobStatus::Dithering;
                                    }
                                    s.current_step_description = format!("🔀 Ditheröidään (ruutu #{}/{})...", completed_frames, total_frames);
                                    s.logs.push(format!("[{}] 🔀 Ditheröinti ruudun {} jälkeen ({} px)", Utc::now().format("%H:%M:%S"), completed_frames, dither_cfg.step_pixels));
                                }
                                client.trigger_dither(dither_cfg.step_pixels, dither_cfg.settle_seconds);
                                let settle_wait = if telem.is_simulation { dither_cfg.settle_seconds.min(2) } else { dither_cfg.settle_seconds };
                                for _ in 0..settle_wait {
                                    if cancel_clone.load(Ordering::SeqCst) { break; }
                                    thread::sleep(Duration::from_secs(1));
                                }
                                let mut s = state_clone.lock().unwrap();
                                if i < s.items.len() {
                                    s.items[i].status = JobStatus::Stacking;
                                }
                            }
                        }

                        if frame_type == FrameType::Light {
                            client.stop_stacking();
                        }

                        {
                            let mut s = state_clone.lock().unwrap();
                            if i < s.items.len() {
                                s.items[i].status = if cancel_clone.load(Ordering::SeqCst) {
                                    JobStatus::Skipped
                                } else if step_failed {
                                    JobStatus::Failed
                                } else {
                                    JobStatus::Completed
                                };
                            }
                            if !step_failed && !cancel_clone.load(Ordering::SeqCst) {
                                s.logs.push(format!(
                                    "[{}] ✓ Valotussarja valmis: {} ({} ruutua × {}s)",
                                    Utc::now().format("%H:%M:%S"), frame_type.display_fi(), completed_frames, exp_s
                                ));
                            }
                        }

                        if step_failed {
                            break;
                        }
                    }

                    SequenceStep::WaitSeconds { seconds, reason } => {
                        {
                            let mut s = state_clone.lock().unwrap();
                            if i < s.items.len() {
                                s.items[i].status = JobStatus::Pending;
                            }
                            s.current_step_description = format!("Odotus: {}s ({})", seconds, reason);
                            s.logs.push(format!("[{}] ⏳ Odotus aloitettu: {}s ({})", Utc::now().format("%H:%M:%S"), seconds, reason));
                        }

                        for sec in 0..seconds {
                            if cancel_clone.load(Ordering::SeqCst) {
                                break;
                            }
                            thread::sleep(Duration::from_secs(1));
                            let remain = seconds.saturating_sub(sec + 1);
                            let mut s = state_clone.lock().unwrap();
                            if i < s.items.len() {
                                s.items[i].elapsed_seconds = sec + 1;
                                s.items[i].progress_str = format!("{}s / {}s", sec + 1, seconds);
                            }
                            s.current_step_description = format!("Odotetaan: jäljellä {:02}m {:02}s ({})", remain / 60, remain % 60, reason);
                        }

                        let mut s = state_clone.lock().unwrap();
                        if i < s.items.len() {
                            s.items[i].status = if cancel_clone.load(Ordering::SeqCst) { JobStatus::Skipped } else { JobStatus::Completed };
                        }
                        s.logs.push(format!("[{}] ✓ Odotus päättynyt: {}s", Utc::now().format("%H:%M:%S"), seconds));
                    }

                    SequenceStep::Park => {
                        {
                            let mut s = state_clone.lock().unwrap();
                            if i < s.items.len() {
                                s.items[i].status = JobStatus::Completed;
                            }
                            s.current_step_description = "Ajetaan kaukoputki parkkiasentoon...".to_string();
                            s.logs.push(format!("[{}] 🏠 Ajo parkkiin käynnistetty.", Utc::now().format("%H:%M:%S")));
                        }
                        client.park();
                        thread::sleep(Duration::from_secs(2));
                        let mut s = state_clone.lock().unwrap();
                        s.logs.push(format!("[{}] ✓ Kaukoputki parkkiasennossa.", Utc::now().format("%H:%M:%S")));
                    }
                }

                thread::sleep(Duration::from_millis(500));
            }

            // End of Sequence: Auto-Park if requested
            let auto_park = {
                let mut s = state_clone.lock().unwrap();
                s.is_running = false;
                s.current_item_idx = None;
                s.current_job_idx = None;
                s.current_step_description = if cancel_clone.load(Ordering::SeqCst) {
                    "Sekvenssi keskeytetty käyttäjän toimesta.".to_string()
                } else {
                    "Kaikki yötoiminnot suoritettu onnistuneesti!".to_string()
                };
                s.logs.push(format!("[{}] 🏁 Yöajo päättynyt.", Utc::now().format("%H:%M:%S")));
                s.auto_park_at_end
            };

            if auto_park {
                client.park();
                let mut s = state_clone.lock().unwrap();
                s.logs.push(format!("[{}] 🏠 Seestar ajettu parkkiasentoon.", Utc::now().format("%H:%M:%S")));
            }
        });
    }

    pub fn stop(&self) {
        self.cancel_signal.store(true, Ordering::SeqCst);
        let mut s = self.state.lock().unwrap();
        s.is_running = false;
        s.logs.push(format!("[{}] ⏹ Yöajo keskeytetty.", Utc::now().format("%H:%M:%S")));
    }
}
