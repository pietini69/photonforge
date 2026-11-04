use super::catalog::SkyObject;
use super::coordinates::{ra_dec_to_alt_az, DEFAULT_LATITUDE_DEG, DEFAULT_LONGITUDE_DEG};
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SequenceTarget {
    pub id: usize,
    pub name: String,
    pub constellation: String,
    pub ra_hours: f64,
    pub dec_deg: f64,
    pub duration_minutes: u32,
    pub min_altitude_deg: f64,
    pub use_lp_filter: bool,
    pub status: JobStatus,
    pub elapsed_seconds: u32,
}

#[derive(Clone, Debug)]
pub struct SequencerState {
    pub is_running: bool,
    pub current_job_idx: Option<usize>,
    pub current_step_description: String,
    pub auto_park_at_end: bool,
    pub autofocus_config: AutoFocusConfig,
    pub dither_config: DitherConfig,
    pub connection_guard: ConnectionGuardConfig,
    pub af_runs_count: u32,
    pub dithers_count: u32,
    pub reconnect_events_count: u32,
    pub last_af_temp_c: Option<f32>,
    pub minutes_since_last_af: u32,
    pub targets: Vec<SequenceTarget>,
    pub logs: Vec<String>,
}

impl Default for SequencerState {
    fn default() -> Self {
        Self {
            is_running: false,
            current_job_idx: None,
            current_step_description: "Sekvensseri valmiina.".to_string(),
            auto_park_at_end: true,
            autofocus_config: AutoFocusConfig::default(),
            dither_config: DitherConfig::default(),
            connection_guard: ConnectionGuardConfig::default(),
            af_runs_count: 0,
            dithers_count: 0,
            reconnect_events_count: 0,
            last_af_temp_c: None,
            minutes_since_last_af: 0,
            targets: vec![
                SequenceTarget {
                    id: 1,
                    name: "M45 Plejadit".to_string(),
                    constellation: "Tau".to_string(),
                    ra_hours: 3.79,
                    dec_deg: 24.11,
                    duration_minutes: 60,
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
                    min_altitude_deg: 20.0,
                    use_lp_filter: true,
                    status: JobStatus::Pending,
                    elapsed_seconds: 0,
                },
            ],
            logs: vec![
                format!("[{}] Yösekvensseri alustettu (Ditheröinti, Auto-Focus ja Yhteysvahti valmiina).", Utc::now().format("%H:%M:%S")),
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

    pub fn add_target(&self, obj: &SkyObject, duration_minutes: u32, use_lp: bool) {
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
        s.targets.push(SequenceTarget {
            id: next_id,
            name,
            constellation: obj.constellation.clone(),
            ra_hours: obj.ra_hours,
            dec_deg: obj.dec_deg,
            duration_minutes,
            min_altitude_deg: 20.0,
            use_lp_filter: use_lp,
            status: JobStatus::Pending,
            elapsed_seconds: 0,
        });
        s.logs.push(format!(
            "[{}] Lisätty sekvenssiin: {} ({} min, {})",
            Utc::now().format("%H:%M:%S"),
            obj.id,
            duration_minutes,
            if use_lp { "LP-suodatin" } else { "Normaali" }
        ));
    }

    pub fn add_sequence_target(&self, target: SequenceTarget) {
        let mut s = self.state.lock().unwrap();
        let next_id = s.targets.len() + 1;
        let mut t = target;
        t.id = next_id;
        s.logs.push(format!(
            "[{}] Lisätty sekvenssiin: {} ({} min, {})",
            Utc::now().format("%H:%M:%S"),
            t.name,
            t.duration_minutes,
            if t.use_lp_filter { "LP-suodatin" } else { "Normaali" }
        ));
        s.targets.push(t);
    }

    pub fn remove_target(&self, idx: usize) {
        let mut s = self.state.lock().unwrap();
        if idx < s.targets.len() {
            let removed = s.targets.remove(idx);
            s.logs.push(format!("[{}] Poistettu sekvenssistä: {}", Utc::now().format("%H:%M:%S"), removed.name));
        }
    }

    pub fn clear_all(&self) {
        let mut s = self.state.lock().unwrap();
        s.targets.clear();
        s.logs.push(format!("[{}] Kuvauslista tyhjennetty.", Utc::now().format("%H:%M:%S")));
    }

    /// Helper that waits for connection to recover within the configured grace period.
    /// Returns `true` if connected, `false` if cancelled or grace period expired.
    fn wait_for_connection(
        state_clone: &Arc<Mutex<SequencerState>>,
        cancel_clone: &Arc<AtomicBool>,
        client: &Arc<SeestarClient>,
        job_idx: usize,
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
            if job_idx < s.targets.len() {
                s.targets[job_idx].status = JobStatus::WaitingConnection;
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

        // Ensure auto-reconnect is triggered if client was in Disconnected state
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
            "[{}] ❌ Yhteyskatkos kesti yli armonajan ({} min). Keskeytetään kohde turvallisesti.",
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
        job_idx: usize,
        reason: &str,
    ) -> Option<f32> {
        {
            let mut s = state_clone.lock().unwrap();
            if job_idx < s.targets.len() {
                s.targets[job_idx].status = JobStatus::Focusing;
            }
            s.current_step_description = format!("🔍 Automaattitarkennus käynnissä ({})...", reason);
            s.logs.push(format!(
                "[{}] 🔍 Käynnistetään automaattitarkennus ({})",
                Utc::now().format("%H:%M:%S"),
                reason
            ));
        }

        client.auto_focus();

        // Wait up to 90s for focusing to complete
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

    pub fn start(&self, client: Arc<SeestarClient>) {
        let mut s = self.state.lock().unwrap();
        if s.is_running {
            return;
        }
        if s.targets.is_empty() {
            s.logs.push(format!("[{}] Ei kohteita sekvenssissä!", Utc::now().format("%H:%M:%S")));
            return;
        }

        s.is_running = true;
        s.current_job_idx = Some(0);
        s.af_runs_count = 0;
        s.dithers_count = 0;
        s.reconnect_events_count = 0;
        let af_on = s.autofocus_config.enabled;
        let dither_on = s.dither_config.enabled;
        let guard_on = s.connection_guard.auto_reconnect;
        s.logs.push(format!(
            "[{}] ▶ YÖAJO KÄYNNISTETTY! (AF: {}, Dither: {}, Yhteysvahti: {})",
            Utc::now().format("%H:%M:%S"),
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
            let total_targets = {
                let s = state_clone.lock().unwrap();
                s.targets.len()
            };

            let mut last_af_instant = Instant::now();
            let mut last_af_temp_c = client.get_telemetry().temp_c;

            for i in 0..total_targets {
                if cancel_clone.load(Ordering::SeqCst) {
                    break;
                }

                // 1. Prepare target & read current automation settings
                let (target_name, ra, dec, duration_mins, min_alt, use_lp, af_cfg, dither_cfg, conn_guard) = {
                    let mut s = state_clone.lock().unwrap();
                    s.current_job_idx = Some(i);
                    s.targets[i].status = JobStatus::Pending;
                    s.targets[i].elapsed_seconds = 0;
                    let t = &s.targets[i];
                    (
                        t.name.clone(),
                        t.ra_hours,
                        t.dec_deg,
                        t.duration_minutes,
                        t.min_altitude_deg,
                        t.use_lp_filter,
                        s.autofocus_config,
                        s.dither_config,
                        s.connection_guard,
                    )
                };

                // Ensure connection is healthy before starting target
                if !Self::wait_for_connection(&state_clone, &cancel_clone, &client, i, conn_guard.grace_period_minutes) {
                    let mut s = state_clone.lock().unwrap();
                    s.targets[i].status = if cancel_clone.load(Ordering::SeqCst) {
                        JobStatus::Skipped
                    } else {
                        JobStatus::Failed
                    };
                    break;
                }

                // Synchronize dither & auto-reconnect settings to client
                client.configure_dither(dither_cfg);
                client.set_auto_reconnect(conn_guard.auto_reconnect);

                // Check altitude
                let now_utc = Utc::now();
                let (current_alt, _) = ra_dec_to_alt_az(ra, dec, DEFAULT_LATITUDE_DEG, DEFAULT_LONGITUDE_DEG, &now_utc);
                if current_alt < min_alt {
                    let mut s = state_clone.lock().unwrap();
                    s.logs.push(format!(
                        "[{}] Varoitus: Kohde {} on alle minimikorkeuden ({:.1}° < {:.1}°), siirrytään kohteeseen silti.",
                        Utc::now().format("%H:%M:%S"), target_name, current_alt, min_alt
                    ));
                }

                // 2. Set filter deterministically
                client.set_lp_filter(use_lp);

                // 3. Slew & GoTo
                {
                    let mut s = state_clone.lock().unwrap();
                    s.targets[i].status = JobStatus::Slewing;
                    s.current_step_description = format!("Käännytään kohteeseen {}...", target_name);
                    s.logs.push(format!("[{}] GoTo: Käännytään kohteeseen {}...", Utc::now().format("%H:%M:%S"), target_name));
                }
                client.goto_target(&target_name, ra, dec);

                // Wait for slew & plate solving
                for _ in 0..90 {
                    if cancel_clone.load(Ordering::SeqCst) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(200));
                    let telem = client.get_telemetry();
                    if telem.state == SeestarState::Solving {
                        let mut s = state_clone.lock().unwrap();
                        s.targets[i].status = JobStatus::Solving;
                        s.current_step_description = format!("Kenttätunnistus (Plate Solving): {}...", target_name);
                    } else if telem.state == SeestarState::Tracking {
                        break;
                    }
                }

                if cancel_clone.load(Ordering::SeqCst) {
                    break;
                }

                // 4. Auto-Focus on Target Start (if enabled)
                if af_cfg.enabled && af_cfg.on_target_start {
                    if let Some(new_temp) = Self::execute_autofocus(
                        &state_clone,
                        &cancel_clone,
                        &client,
                        i,
                        &format!("uusi kohde: {}", target_name),
                    ) {
                        last_af_temp_c = new_temp;
                        last_af_instant = Instant::now();
                    }
                }

                if cancel_clone.load(Ordering::SeqCst) {
                    break;
                }

                // 5. Start Stacking
                {
                    let mut s = state_clone.lock().unwrap();
                    s.targets[i].status = JobStatus::Stacking;
                    s.current_step_description = format!("Valottaa ja pinoaa kohdetta {} (kesto {} min)...", target_name, duration_mins);
                    s.logs.push(format!("[{}] ▶ Aloitetaan pinoaminen: {} ({} min)", Utc::now().format("%H:%M:%S"), target_name, duration_mins));
                }
                client.start_stacking();

                // 6. Resilient Active Stacking Loop (handles periodic AF, dithering, and connection drops)
                let target_duration_secs = duration_mins * 60;
                let mut accumulated_stack_secs: u32 = 0;
                let mut last_dithered_frame: u32 = 0;
                let mut target_failed = false;

                while accumulated_stack_secs < target_duration_secs {
                    if cancel_clone.load(Ordering::SeqCst) {
                        break;
                    }
                    thread::sleep(Duration::from_secs(1));

                    let telem = client.get_telemetry();
                    let (cur_af_cfg, cur_dither_cfg, cur_conn_guard) = {
                        let mut s = state_clone.lock().unwrap();
                        s.minutes_since_last_af = (last_af_instant.elapsed().as_secs() / 60) as u32;
                        (s.autofocus_config, s.dither_config, s.connection_guard)
                    };

                    // A. Connection Health Check & Recovery
                    if !telem.connected && !telem.is_simulation {
                        let outage_clock = Instant::now();
                        if !Self::wait_for_connection(
                            &state_clone,
                            &cancel_clone,
                            &client,
                            i,
                            cur_conn_guard.grace_period_minutes,
                        ) {
                            target_failed = true;
                            break;
                        }

                        // Connection recovered! Reconcile telescope state
                        let recovered_telem = client.get_telemetry();
                        let outage_secs = outage_clock.elapsed().as_secs() as u32;
                        if recovered_telem.state == SeestarState::Stacking {
                            // Seestar kept stacking autonomously during the Wi-Fi drop
                            accumulated_stack_secs = (accumulated_stack_secs + outage_secs).min(target_duration_secs);
                            let mut s = state_clone.lock().unwrap();
                            s.targets[i].status = JobStatus::Stacking;
                            s.logs.push(format!(
                                "[{}] 🛡 Tila täsmäytetty: Seestar jatkoi pinoamista itsenäisesti katkoksen ajan (+{}s).",
                                Utc::now().format("%H:%M:%S"),
                                outage_secs
                            ));
                        } else if cur_conn_guard.resume_stacking_after_reconnect {
                            // Scope dropped out of stacking; re-issue start_stacking
                            client.start_stacking();
                            let mut s = state_clone.lock().unwrap();
                            s.targets[i].status = JobStatus::Stacking;
                            s.logs.push(format!(
                                "[{}] 🔄 Pinoaminen käynnistetty automaattisesti uudelleen yhteyskatkoksen jälkeen.",
                                Utc::now().format("%H:%M:%S")
                            ));
                        }
                        continue;
                    }

                    let remaining_secs = target_duration_secs.saturating_sub(accumulated_stack_secs);

                    // B. Periodic / Temperature-Delta Auto-Focus Check (only if > 2 min remaining on target)
                    if cur_af_cfg.enabled && remaining_secs > 120 {
                        let time_elapsed_mins = (last_af_instant.elapsed().as_secs() / 60) as u32;
                        let time_triggered = cur_af_cfg.use_time_trigger
                            && cur_af_cfg.interval_minutes > 0
                            && time_elapsed_mins >= cur_af_cfg.interval_minutes;

                        let temp_delta = (telem.temp_c - last_af_temp_c).abs();
                        let temp_triggered = cur_af_cfg.use_temp_trigger
                            && cur_af_cfg.temp_delta_c > 0.0
                            && temp_delta >= cur_af_cfg.temp_delta_c;

                        if time_triggered || temp_triggered {
                            let reason = if temp_triggered {
                                format!("lämpötilamuutos ΔT {:.1} °C ≥ {:.1} °C", temp_delta, cur_af_cfg.temp_delta_c)
                            } else {
                                format!("aikaväli {} min täyttyi", cur_af_cfg.interval_minutes)
                            };

                            if let Some(new_temp) = Self::execute_autofocus(
                                &state_clone,
                                &cancel_clone,
                                &client,
                                i,
                                &reason,
                            ) {
                                last_af_temp_c = new_temp;
                                last_af_instant = Instant::now();
                            }

                            // Ensure stacking continues after AF
                            client.start_stacking();
                            let mut s = state_clone.lock().unwrap();
                            s.targets[i].status = JobStatus::Stacking;
                        }
                    }

                    // C. Periodic Dithering Check (every N stacked frames)
                    if cur_dither_cfg.enabled
                        && cur_dither_cfg.interval_frames > 0
                        && telem.stacked_frames > last_dithered_frame
                        && telem.stacked_frames % cur_dither_cfg.interval_frames == 0
                        && remaining_secs > cur_dither_cfg.settle_seconds + 5
                    {
                        last_dithered_frame = telem.stacked_frames;
                        {
                            let mut s = state_clone.lock().unwrap();
                            s.dithers_count += 1;
                            s.targets[i].status = JobStatus::Dithering;
                            s.current_step_description = format!(
                                "🔀 Ditheröidään (ruutu #{}, siirto {} px, asettuminen {}s)...",
                                telem.stacked_frames, cur_dither_cfg.step_pixels, cur_dither_cfg.settle_seconds
                            );
                            s.logs.push(format!(
                                "[{}] 🔀 Ditheröinti (ruutu #{}): mikrosiirto {} px, asettuminen {}s",
                                Utc::now().format("%H:%M:%S"),
                                telem.stacked_frames,
                                cur_dither_cfg.step_pixels,
                                cur_dither_cfg.settle_seconds
                            ));
                        }

                        client.trigger_dither(cur_dither_cfg.step_pixels, cur_dither_cfg.settle_seconds);

                        // Wait for dither settling
                        let settle_wait = if telem.is_simulation {
                            cur_dither_cfg.settle_seconds.min(3)
                        } else {
                            cur_dither_cfg.settle_seconds
                        };
                        for _ in 0..settle_wait {
                            if cancel_clone.load(Ordering::SeqCst) {
                                break;
                            }
                            thread::sleep(Duration::from_secs(1));
                        }

                        let mut s = state_clone.lock().unwrap();
                        s.targets[i].status = JobStatus::Stacking;
                    }

                    // D. Advance active exposure clock
                    accumulated_stack_secs += 1;
                    let remaining = target_duration_secs.saturating_sub(accumulated_stack_secs);
                    let mut s = state_clone.lock().unwrap();
                    s.targets[i].status = JobStatus::Stacking;
                    s.targets[i].elapsed_seconds = accumulated_stack_secs;
                    s.current_step_description = format!(
                        "Pinoaa kohdetta {} ({} ruutua) – Jäljellä {:02}m {:02}s",
                        target_name,
                        telem.stacked_frames,
                        remaining / 60,
                        remaining % 60
                    );
                }

                // Stop stacking for this target
                client.stop_stacking();
                {
                    let mut s = state_clone.lock().unwrap();
                    s.targets[i].status = if cancel_clone.load(Ordering::SeqCst) {
                        JobStatus::Skipped
                    } else if target_failed {
                        JobStatus::Failed
                    } else {
                        JobStatus::Completed
                    };
                    if !target_failed {
                        s.logs.push(format!("[{}] ✓ Kohde valmis: {}", Utc::now().format("%H:%M:%S"), target_name));
                    }
                }

                if target_failed {
                    break;
                }

                // Cool down / settle pause between targets
                thread::sleep(Duration::from_secs(3));
            }

            // End of Sequence: Auto-Park
            let auto_park = {
                let mut s = state_clone.lock().unwrap();
                s.is_running = false;
                s.current_job_idx = None;
                s.current_step_description = if cancel_clone.load(Ordering::SeqCst) {
                    "Sekvenssi keskeytetty käyttäjän toimesta.".to_string()
                } else {
                    "Kaikki yökohteet kuvattu onnistuneesti!".to_string()
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
