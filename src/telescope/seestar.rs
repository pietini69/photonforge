use chrono::Local;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SeestarState {
    Disconnected,
    Connecting,
    Reconnecting,
    Idle,
    Slewing,
    Solving,
    Focusing,
    Tracking,
    Stacking,
    Dithering,
    Parking,
    Parked,
}

impl SeestarState {
    pub fn display_fi(&self) -> &'static str {
        match self {
            SeestarState::Disconnected => "Ei yhteyttä",
            SeestarState::Connecting => "Yhdistetään...",
            SeestarState::Reconnecting => "Palautetaan yhteyttä...",
            SeestarState::Idle => "Valmiustilassa",
            SeestarState::Slewing => "Kääntyy kohteeseen...",
            SeestarState::Solving => "Kenttätunnistus...",
            SeestarState::Focusing => "Tarkentaa...",
            SeestarState::Tracking => "Seuraa kohdetta",
            SeestarState::Stacking => "Valottaa & pinoaa",
            SeestarState::Dithering => "Ditheröi...",
            SeestarState::Parking => "Ajaa lepoasentoon...",
            SeestarState::Parked => "Lepoasennossa",
        }
    }

    pub fn color_rgb(&self) -> (u8, u8, u8) {
        match self {
            SeestarState::Disconnected => (150, 150, 160),
            SeestarState::Connecting => (255, 200, 60),
            SeestarState::Reconnecting => (255, 150, 50),
            SeestarState::Idle => (120, 220, 150),
            SeestarState::Slewing => (80, 200, 255),
            SeestarState::Solving => (255, 180, 80),
            SeestarState::Focusing => (255, 225, 90),
            SeestarState::Tracking => (100, 255, 120),
            SeestarState::Stacking => (255, 90, 130),
            SeestarState::Dithering => (190, 140, 255),
            SeestarState::Parking => (200, 140, 255),
            SeestarState::Parked => (160, 160, 180),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DitherConfig {
    pub enabled: bool,
    pub interval_frames: u32,
    pub step_pixels: u32,
    pub settle_seconds: u32,
}

impl Default for DitherConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_frames: 3,
            step_pixels: 8,
            settle_seconds: 4,
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct SeestarTelemetry {
    pub connected: bool,
    pub is_simulation: bool,
    pub ip_address: String,
    pub scope_model: crate::telescope::mosaic::TelescopeModel,
    pub protocol: crate::telescope::mosaic::TelescopeProtocol,
    pub port: u16,
    pub state: SeestarState,
    pub battery_pct: u8,
    pub temp_c: f32,
    pub current_ra: f64,
    pub current_dec: f64,
    pub current_alt: f64,
    pub current_az: f64,
    pub target_name: String,
    pub target_ra: f64,
    pub target_dec: f64,
    pub stacked_frames: u32,
    pub total_stack_seconds: u32,
    pub lp_filter_active: bool,
    pub last_log_message: String,
    pub auto_reconnect_enabled: bool,
    pub reconnect_attempts: u32,
    pub max_reconnect_attempts: u32,
    pub focus_position: u32,
    pub last_focus_temp_c: Option<f32>,
    pub last_focus_time: Option<String>,
    pub dither_config: DitherConfig,
    pub dither_count: u32,
}

impl Default for SeestarTelemetry {
    fn default() -> Self {
        Self {
            connected: true,
            is_simulation: true, // Default to simulation mode so user can explore right away
            ip_address: "192.168.110.1".to_string(), // Default Seestar AP IP or station IP
            scope_model: crate::telescope::mosaic::TelescopeModel::SeestarS50,
            protocol: crate::telescope::mosaic::TelescopeProtocol::Seestar,
            port: 4700,
            state: SeestarState::Idle,
            battery_pct: 88,
            temp_c: 12.5,
            current_ra: 3.79,   // Near M45
            current_dec: 24.11,
            current_alt: 45.0,
            current_az: 180.0,
            target_name: "M45 Plejadit".to_string(),
            target_ra: 3.79,
            target_dec: 24.11,
            stacked_frames: 0,
            total_stack_seconds: 0,
            lp_filter_active: false,
            last_log_message: "Kaukoputkiohjain valmiina (Simulaatiotila).".to_string(),
            auto_reconnect_enabled: true,
            reconnect_attempts: 0,
            max_reconnect_attempts: 15,
            focus_position: 1450,
            last_focus_temp_c: None,
            last_focus_time: None,
            dither_config: DitherConfig::default(),
            dither_count: 0,
        }
    }
}

pub struct SeestarClient {
    pub telemetry: Arc<Mutex<SeestarTelemetry>>,
    stop_signal: Arc<AtomicBool>,
    command_tx: std::sync::mpsc::Sender<SeestarCommand>,
}

#[derive(Debug)]
enum SeestarCommand {
    Connect(String),
    Disconnect,
    SetSimulation(bool),
    SetModel(crate::telescope::mosaic::TelescopeModel),
    GoTo {
        name: String,
        ra: f64,
        dec: f64,
    },
    StartStacking,
    StopStacking,
    Park,
    Unpark,
    ToggleLpFilter,
    SetLpFilter(bool),
    AutoFocus,
    ConfigureDither(DitherConfig),
    TriggerDither {
        pixels: u32,
        settle_secs: u32,
    },
    SetAutoReconnect(bool),
}

fn alpaca_put(
    agent: &ureq::Agent,
    ip: &str,
    port: u16,
    endpoint: &str,
    form_data: &[(&str, &str)],
) -> Result<serde_json::Value, String> {
    let url = format!("http://{}:{}/api/v1/{}", ip, port, endpoint);
    let resp = agent
        .put(&url)
        .set("Content-Type", "application/x-www-form-urlencoded")
        .send_form(form_data)
        .map_err(|e| e.to_string())?;

    let text = resp.into_string().map_err(|e| e.to_string())?;
    let val: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if let Some(err_num) = val.get("ErrorNumber").and_then(|v| v.as_i64()) {
        if err_num != 0 {
            let msg = val
                .get("ErrorMessage")
                .and_then(|v| v.as_str())
                .unwrap_or("Alpaca error");
            return Err(format!("Alpaca virhe {}: {}", err_num, msg));
        }
    }
    Ok(val)
}

fn alpaca_get(
    agent: &ureq::Agent,
    ip: &str,
    port: u16,
    endpoint: &str,
    query_params: &[(&str, &str)],
) -> Result<serde_json::Value, String> {
    let mut url = format!("http://{}:{}/api/v1/{}", ip, port, endpoint);
    if !query_params.is_empty() {
        let q_str = query_params
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&");
        url = format!("{}?{}", url, q_str);
    }
    let resp = agent.get(&url).call().map_err(|e| e.to_string())?;
    let text = resp.into_string().map_err(|e| e.to_string())?;
    let val: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if let Some(err_num) = val.get("ErrorNumber").and_then(|v| v.as_i64()) {
        if err_num != 0 {
            let msg = val
                .get("ErrorMessage")
                .and_then(|v| v.as_str())
                .unwrap_or("Alpaca error");
            return Err(format!("Alpaca virhe {}: {}", err_num, msg));
        }
    }
    Ok(val)
}

impl Default for SeestarClient {
    fn default() -> Self {
        Self::new()
    }
}

impl SeestarClient {
    pub fn new() -> Self {
        let telemetry = Arc::new(Mutex::new(SeestarTelemetry::default()));
        let stop_signal = Arc::new(AtomicBool::new(false));
        let (command_tx, command_rx) = std::sync::mpsc::channel();

        let telem_clone = Arc::clone(&telemetry);
        let stop_clone = Arc::clone(&stop_signal);

        // Background worker thread for handling networking / simulation
        thread::spawn(move || {
            let alpaca_agent = ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(3))
                .timeout_read(Duration::from_secs(3))
                .timeout_write(Duration::from_secs(3))
                .build();
            let mut alpaca_fail_count: u32 = 0;
            let mut active_stream: Option<TcpStream> = None;
            let mut last_heartbeat = Instant::now();
            let mut stack_timer = Instant::now();
            let mut slew_start: Option<(Instant, f64, f64, f64, f64)> = None; // start_time, start_ra, start_dec, end_ra, end_dec
            let mut focus_start: Option<(Instant, SeestarState)> = None; // start_time, return_state
            let mut dither_start: Option<(Instant, u32, SeestarState)> = None; // start_time, settle_secs, return_state
            let mut user_disconnected = false;
            let mut next_reconnect_at: Option<Instant> = None;
            let mut pre_disconnect_state = SeestarState::Idle;

            while !stop_clone.load(Ordering::SeqCst) {
                // Handle commands
                while let Ok(cmd) = command_rx.try_recv() {
                    match cmd {
                        SeestarCommand::SetModel(model) => {
                            let mut t = telem_clone.lock().unwrap();
                            t.scope_model = model;
                            t.protocol = model.protocol();
                            t.port = model.default_port();
                            t.last_log_message = format!(
                                "Kaukoputkimalliksi valittu: {} ({})",
                                model.display_short(),
                                model.protocol().protocol_name()
                            );
                        }
                        SeestarCommand::Connect(ip) => {
                            user_disconnected = false;
                            next_reconnect_at = None;
                            let mut t = telem_clone.lock().unwrap();
                            t.ip_address = ip.clone();
                            t.state = SeestarState::Connecting;
                            t.reconnect_attempts = 0;
                            let port = t.port;
                            let proto = t.protocol;
                            let scope_name = t.scope_model.display_short();
                            t.last_log_message = format!("Yhdistetään kohteeseen {} ({}:{})...", scope_name, ip, port);
                            let _dither_cfg = t.dither_config;
                            drop(t);

                            if proto == crate::telescope::mosaic::TelescopeProtocol::AscomAlpaca {
                                let connect_form = [("Connected", "true"), ("ClientID", "1"), ("ClientTransactionID", "1")];
                                match alpaca_put(&alpaca_agent, &ip, port, "telescope/0/connected", &connect_form) {
                                    Ok(_) => {
                                        // Connect companion devices
                                        let _ = alpaca_put(&alpaca_agent, &ip, port, "camera/0/connected", &connect_form);
                                        let _ = alpaca_put(&alpaca_agent, &ip, port, "filterwheel/0/connected", &connect_form);
                                        let _ = alpaca_put(&alpaca_agent, &ip, port, "focuser/0/connected", &connect_form);

                                        let mut t = telem_clone.lock().unwrap();
                                        t.connected = true;
                                        t.is_simulation = false;
                                        t.reconnect_attempts = 0;
                                        alpaca_fail_count = 0;

                                        // Query current park state
                                        if let Ok(val) = alpaca_get(&alpaca_agent, &ip, port, "telescope/0/atpark", &[("ClientID", "1")]) {
                                            if let Some(at_park) = val.get("Value").and_then(|v| v.as_bool()) {
                                                t.state = if at_park { SeestarState::Parked } else { SeestarState::Idle };
                                            } else {
                                                t.state = SeestarState::Idle;
                                            }
                                        } else {
                                            t.state = SeestarState::Idle;
                                        }

                                        // Query initial filter position (2 = LP, 1 = Normal/IR)
                                        if let Ok(val) = alpaca_get(&alpaca_agent, &ip, port, "filterwheel/0/position", &[("ClientID", "1")]) {
                                            if let Some(pos) = val.get("Value").and_then(|v| v.as_i64()) {
                                                t.lp_filter_active = pos == 2;
                                            }
                                        }

                                        // Query initial CCD temp
                                        if let Ok(val) = alpaca_get(&alpaca_agent, &ip, port, "camera/0/ccdtemperature", &[("ClientID", "1")]) {
                                            if let Some(temp) = val.get("Value").and_then(|v| v.as_f64()) {
                                                t.temp_c = temp as f32;
                                            }
                                        }

                                        t.last_log_message = format!("Yhteys muodostettu kohteeseen {} Alpaca-rajapinnalla ({}:{})!", scope_name, ip, port);
                                    }
                                    Err(e) => {
                                        let mut t = telem_clone.lock().unwrap();
                                        t.connected = false;
                                        t.state = SeestarState::Disconnected;
                                        t.last_log_message = format!("Alpaca-yhteysvirhe ({}): {}. Varmista että Seestar Alpaca on päällä puhelinsovelluksessa.", scope_name, e);
                                    }
                                }
                            } else {
                                // Raw TCP protocols (supports both numeric IPs and hostnames like seestar.local)
                                let addr_str = format!("{}:{}", ip, port);
                                let resolved_addr = if let Ok(addr) = addr_str.parse::<SocketAddr>() {
                                    Some(addr)
                                } else if let Ok(mut addrs) = addr_str.to_socket_addrs() {
                                    addrs.next()
                                } else {
                                    None
                                };

                                if let Some(addr) = resolved_addr {
                                    match TcpStream::connect_timeout(&addr, Duration::from_secs(3)) {
                                        Ok(mut stream) => {
                                            let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                                            let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));

                                            match proto {
                                                crate::telescope::mosaic::TelescopeProtocol::Seestar => {
                                                    let query = serde_json::json!({
                                                        "id": 100,
                                                        "method": "get_device_state"
                                                    });
                                                    let _ = stream.write_all(format!("{}\r\n", query).as_bytes());
                                                }
                                                crate::telescope::mosaic::TelescopeProtocol::DwarfLab => {
                                                    let hello = serde_json::json!({
                                                        "interface": 10001,
                                                        "cmd": "connect"
                                                    });
                                                    let _ = stream.write_all(format!("{}\r\n", hello).as_bytes());
                                                }
                                                _ => {}
                                            }

                                            active_stream = Some(stream);

                                            let mut t = telem_clone.lock().unwrap();
                                            t.connected = true;
                                            t.is_simulation = false;
                                            t.state = SeestarState::Idle;
                                            t.last_log_message = format!("Yhteys muodostettu kohteeseen {} ({}:{})!", scope_name, ip, port);
                                        }
                                        Err(e) => {
                                            let mut t = telem_clone.lock().unwrap();
                                            t.connected = false;
                                            t.state = SeestarState::Disconnected;
                                            t.last_log_message = format!("Yhteysvirhe ({}): {}. Voit käyttää simulaatiotilaa.", scope_name, e);
                                        }
                                    }
                                } else {
                                    let mut t = telem_clone.lock().unwrap();
                                    t.connected = false;
                                    t.state = SeestarState::Disconnected;
                                    t.last_log_message = format!("Virheellinen osoite: {}:{}", ip, port);
                                }
                            }
                        }
                        SeestarCommand::Disconnect => {
                            user_disconnected = true;
                            next_reconnect_at = None;
                            active_stream = None;
                            let (ip, port, proto) = {
                                let mut t = telem_clone.lock().unwrap();
                                t.connected = false;
                                t.reconnect_attempts = 0;
                                t.state = SeestarState::Disconnected;
                                t.last_log_message = "Yhteys suljettu käyttäjän toimesta.".to_string();
                                (t.ip_address.clone(), t.port, t.protocol)
                            };
                            if proto == crate::telescope::mosaic::TelescopeProtocol::AscomAlpaca {
                                let disc_form = [("Connected", "false"), ("ClientID", "1"), ("ClientTransactionID", "1")];
                                let _ = alpaca_put(&alpaca_agent, &ip, port, "telescope/0/connected", &disc_form);
                                let _ = alpaca_put(&alpaca_agent, &ip, port, "camera/0/connected", &disc_form);
                            }
                        }
                        SeestarCommand::SetSimulation(sim) => {
                            let mut t = telem_clone.lock().unwrap();
                            t.is_simulation = sim;
                            if sim {
                                next_reconnect_at = None;
                                t.connected = true;
                                t.reconnect_attempts = 0;
                                t.state = SeestarState::Idle;
                                t.last_log_message = "Simulaatiotila aktivoitu. Kaukoputki virtuaalisesti valmiina!".to_string();
                            } else if active_stream.is_none() && !t.connected {
                                t.connected = false;
                                t.state = SeestarState::Disconnected;
                                t.last_log_message = "Simulaatiotila poistettu käytöstä. Yhdistä Seestariin IP-osoitteella.".to_string();
                            }
                        }
                        SeestarCommand::GoTo { name, ra, dec } => {
                            let (start_ra, start_dec, is_sim, ip, port, proto) = {
                                let mut t = telem_clone.lock().unwrap();
                                let s_ra = t.current_ra;
                                let s_dec = t.current_dec;
                                t.target_name = name.clone();
                                t.target_ra = ra;
                                t.target_dec = dec;
                                t.state = SeestarState::Slewing;
                                t.last_log_message = format!("GoTo: Käännytään kohteeseen {} (RA {:.2}h, Dec {:.2}°)...", name, ra, dec);
                                (s_ra, s_dec, t.is_simulation, t.ip_address.clone(), t.port, t.protocol)
                            };

                            if is_sim {
                                slew_start = Some((Instant::now(), start_ra, start_dec, ra, dec));
                            } else if proto == crate::telescope::mosaic::TelescopeProtocol::AscomAlpaca {
                                // Unpark first if parked
                                let _ = alpaca_put(&alpaca_agent, &ip, port, "telescope/0/unpark", &[("ClientID", "1"), ("ClientTransactionID", "100")]);
                                let ra_str = ra.to_string();
                                let dec_str = dec.to_string();
                                let res = alpaca_put(
                                    &alpaca_agent,
                                    &ip,
                                    port,
                                    "telescope/0/slewtocoordinatesasync",
                                    &[("RightAscension", &ra_str), ("Declination", &dec_str), ("ClientID", "1"), ("ClientTransactionID", "101")],
                                );
                                if let Err(e) = res {
                                    let mut t = telem_clone.lock().unwrap();
                                    t.state = SeestarState::Idle;
                                    if e.contains("1279") || e.to_lowercase().contains("operate") || e.to_lowercase().contains("sync") {
                                        t.last_log_message = "⚠️ Seestar vaatii alkusuuntauksen: Avaa putki ja tee alkusuuntaus (vaaterointi + tähtitunnistus) kerran Seestar-mobiilisovelluksella.".to_string();
                                    } else {
                                        t.last_log_message = format!("GoTo-virhe: {}", e);
                                    }
                                }
                            } else if let Some(stream) = &mut active_stream {
                                match proto {
                                    crate::telescope::mosaic::TelescopeProtocol::Seestar => {
                                        let payload = serde_json::json!({
                                            "id": 101,
                                            "method": "iscope_start_view",
                                            "params": {
                                                "target_name": name,
                                                "ra": ra,
                                                "dec": dec
                                            }
                                        });
                                        let msg = format!("{}\r\n", payload);
                                        let _ = stream.write_all(msg.as_bytes());
                                    }
                                    crate::telescope::mosaic::TelescopeProtocol::DwarfLab => {
                                        let payload = serde_json::json!({
                                            "interface": 10005,
                                            "cmd": "goto",
                                            "target": name,
                                            "ra": ra,
                                            "dec": dec
                                        });
                                        let msg = format!("{}\r\n", payload);
                                        let _ = stream.write_all(msg.as_bytes());
                                    }
                                    _ => {}
                                }
                            }
                        }
                        SeestarCommand::StartStacking => {
                            let (is_sim, ip, port, proto) = {
                                let mut t = telem_clone.lock().unwrap();
                                let resuming_same_target = t.stacked_frames > 0 && (t.state == SeestarState::Tracking || t.state == SeestarState::Focusing || t.state == SeestarState::Dithering);
                                t.state = SeestarState::Stacking;
                                if !resuming_same_target {
                                    t.stacked_frames = 0;
                                    t.total_stack_seconds = 0;
                                }
                                t.last_log_message = format!("Pinoaminen käynnissä kohteelle: {}!", t.target_name);
                                let _dither_cfg = t.dither_config;
                                (t.is_simulation, t.ip_address.clone(), t.port, t.protocol)
                            };
                            stack_timer = Instant::now();

                            if !is_sim {
                                if proto == crate::telescope::mosaic::TelescopeProtocol::AscomAlpaca {
                                    let _ = alpaca_put(
                                        &alpaca_agent,
                                        &ip,
                                        port,
                                        "camera/0/startexposure",
                                        &[("Duration", "10"), ("Light", "true"), ("ClientID", "1"), ("ClientTransactionID", "102")],
                                    );
                                } else if let Some(stream) = &mut active_stream {
                                    let dither_cfg = { telem_clone.lock().unwrap().dither_config };
                                    match proto {
                                        crate::telescope::mosaic::TelescopeProtocol::Seestar => {
                                            let dither_payload = serde_json::json!({
                                                "id": 107,
                                                "method": "set_setting",
                                                "params": {
                                                    "dither": {
                                                        "enabled": dither_cfg.enabled,
                                                        "pix": dither_cfg.step_pixels,
                                                        "interval": dither_cfg.interval_frames
                                                    }
                                                }
                                            });
                                            let _ = stream.write_all(format!("{}\r\n", dither_payload).as_bytes());

                                            let payload = serde_json::json!({
                                                "id": 102,
                                                "method": "start_stack"
                                            });
                                            let msg = format!("{}\r\n", payload);
                                            let _ = stream.write_all(msg.as_bytes());
                                        }
                                        crate::telescope::mosaic::TelescopeProtocol::DwarfLab => {
                                            let payload = serde_json::json!({
                                                "interface": 10007,
                                                "cmd": "start_stack"
                                            });
                                            let msg = format!("{}\r\n", payload);
                                            let _ = stream.write_all(msg.as_bytes());
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        SeestarCommand::StopStacking => {
                            let (is_sim, ip, port, proto) = {
                                let mut t = telem_clone.lock().unwrap();
                                t.state = SeestarState::Tracking;
                                t.last_log_message = format!("Pinoaminen pysäytetty kohteelle {}. Valotettu {} ruutua.", t.target_name, t.stacked_frames);
                                (t.is_simulation, t.ip_address.clone(), t.port, t.protocol)
                            };

                            if !is_sim {
                                if proto == crate::telescope::mosaic::TelescopeProtocol::AscomAlpaca {
                                    let _ = alpaca_put(
                                        &alpaca_agent,
                                        &ip,
                                        port,
                                        "camera/0/stopexposure",
                                        &[("ClientID", "1"), ("ClientTransactionID", "103")],
                                    );
                                } else if let Some(stream) = &mut active_stream {
                                    match proto {
                                        crate::telescope::mosaic::TelescopeProtocol::Seestar => {
                                            let payload = serde_json::json!({
                                                "id": 103,
                                                "method": "iscope_stop_view"
                                            });
                                            let msg = format!("{}\r\n", payload);
                                            let _ = stream.write_all(msg.as_bytes());
                                        }
                                        crate::telescope::mosaic::TelescopeProtocol::DwarfLab => {
                                            let payload = serde_json::json!({
                                                "interface": 10008,
                                                "cmd": "stop_stack"
                                            });
                                            let msg = format!("{}\r\n", payload);
                                            let _ = stream.write_all(msg.as_bytes());
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        SeestarCommand::Park => {
                            let (ip, port, proto, is_sim, scope_name) = {
                                let mut t = telem_clone.lock().unwrap();
                                t.state = SeestarState::Parked;
                                let s_name = t.scope_model.display_short();
                                t.last_log_message = format!("{} ajettu parkkiasentoon (virransäästö / suoja).", s_name);
                                (t.ip_address.clone(), t.port, t.protocol, t.is_simulation, s_name)
                            };

                            if !is_sim {
                                if proto == crate::telescope::mosaic::TelescopeProtocol::AscomAlpaca {
                                    let res = alpaca_put(
                                        &alpaca_agent,
                                        &ip,
                                        port,
                                        "telescope/0/park",
                                        &[("ClientID", "1"), ("ClientTransactionID", "104")],
                                    );
                                    if let Err(e) = res {
                                        let mut t = telem_clone.lock().unwrap();
                                        t.last_log_message = format!("Park-virhe ({}): {}", scope_name, e);
                                    }
                                } else if let Some(stream) = &mut active_stream {
                                    match proto {
                                        crate::telescope::mosaic::TelescopeProtocol::Seestar => {
                                            let payload = serde_json::json!({
                                                "id": 104,
                                                "method": "scope_park"
                                            });
                                            let msg = format!("{}\r\n", payload);
                                            let _ = stream.write_all(msg.as_bytes());
                                        }
                                        crate::telescope::mosaic::TelescopeProtocol::DwarfLab => {
                                            let payload = serde_json::json!({
                                                "interface": 10009,
                                                "cmd": "park"
                                            });
                                            let msg = format!("{}\r\n", payload);
                                            let _ = stream.write_all(msg.as_bytes());
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        SeestarCommand::Unpark => {
                            let (ip, port, proto, is_sim, scope_name) = {
                                let mut t = telem_clone.lock().unwrap();
                                t.state = SeestarState::Idle;
                                let s_name = t.scope_model.display_short();
                                t.last_log_message = format!("{} avattu parkkiasennosta.", s_name);
                                (t.ip_address.clone(), t.port, t.protocol, t.is_simulation, s_name)
                            };

                            if !is_sim && proto == crate::telescope::mosaic::TelescopeProtocol::AscomAlpaca {
                                let res = alpaca_put(
                                    &alpaca_agent,
                                    &ip,
                                    port,
                                    "telescope/0/unpark",
                                    &[("ClientID", "1"), ("ClientTransactionID", "105")],
                                );
                                if let Err(e) = res {
                                    let mut t = telem_clone.lock().unwrap();
                                    t.last_log_message = format!("Avausvirhe ({}): {}", scope_name, e);
                                }
                            }
                        }
                        SeestarCommand::ToggleLpFilter => {
                            let (ip, port, proto, is_sim, active) = {
                                let mut t = telem_clone.lock().unwrap();
                                t.lp_filter_active = !t.lp_filter_active;
                                let cur = t.lp_filter_active;
                                t.last_log_message = format!("Valosaastesuodatin (LP / Dual-band): {}", if cur { "PÄÄLLÄ" } else { "POIS" });
                                (t.ip_address.clone(), t.port, t.protocol, t.is_simulation, cur)
                            };

                            if !is_sim {
                                if proto == crate::telescope::mosaic::TelescopeProtocol::AscomAlpaca {
                                    // 2 = LP filter, 1 = Normal/IR filter on Seestar S30/S50
                                    let pos_str = if active { "2" } else { "1" };
                                    let _ = alpaca_put(
                                        &alpaca_agent,
                                        &ip,
                                        port,
                                        "filterwheel/0/position",
                                        &[("Position", pos_str), ("ClientID", "1"), ("ClientTransactionID", "105")],
                                    );
                                } else if let Some(stream) = &mut active_stream {
                                    let filter_name = if active { "lp" } else { "clear" };
                                    let payload = serde_json::json!({
                                        "id": 105,
                                        "method": "set_filter",
                                        "params": { "filter": filter_name }
                                    });
                                    let msg = format!("{}\r\n", payload);
                                    let _ = stream.write_all(msg.as_bytes());
                                }
                            }
                        }
                        SeestarCommand::SetLpFilter(active) => {
                            let (ip, port, proto, is_sim) = {
                                let mut t = telem_clone.lock().unwrap();
                                if t.lp_filter_active != active {
                                    t.lp_filter_active = active;
                                    t.last_log_message = format!("Valosaastesuodatin (LP / Dual-band): {}", if active { "PÄÄLLÄ" } else { "POIS" });
                                }
                                (t.ip_address.clone(), t.port, t.protocol, t.is_simulation)
                            };

                            if !is_sim {
                                if proto == crate::telescope::mosaic::TelescopeProtocol::AscomAlpaca {
                                    let pos_str = if active { "2" } else { "1" };
                                    let _ = alpaca_put(
                                        &alpaca_agent,
                                        &ip,
                                        port,
                                        "filterwheel/0/position",
                                        &[("Position", pos_str), ("ClientID", "1"), ("ClientTransactionID", "105")],
                                    );
                                } else if let Some(stream) = &mut active_stream {
                                    let filter_name = if active { "lp" } else { "clear" };
                                    let payload = serde_json::json!({
                                        "id": 105,
                                        "method": "set_filter",
                                        "params": { "filter": filter_name }
                                    });
                                    let msg = format!("{}\r\n", payload);
                                    let _ = stream.write_all(msg.as_bytes());
                                }
                            }
                        }
                        SeestarCommand::AutoFocus => {
                            let (is_sim, ip, port, proto, prev_state) = {
                                let mut t = telem_clone.lock().unwrap();
                                let p_state = match t.state {
                                    SeestarState::Focusing => SeestarState::Tracking,
                                    ref s => s.clone(),
                                };
                                t.state = SeestarState::Focusing;
                                t.last_log_message = "Automaattitarkennus (Auto-Focus) käynnistetty...".to_string();
                                (t.is_simulation, t.ip_address.clone(), t.port, t.protocol, p_state)
                            };

                            focus_start = Some((Instant::now(), prev_state));

                            if !is_sim {
                                if proto == crate::telescope::mosaic::TelescopeProtocol::AscomAlpaca {
                                    let _ = alpaca_put(
                                        &alpaca_agent,
                                        &ip,
                                        port,
                                        "focuser/0/move",
                                        &[("Position", "1196"), ("ClientID", "1"), ("ClientTransactionID", "106")],
                                    );
                                } else if let Some(stream) = &mut active_stream {
                                    match proto {
                                        crate::telescope::mosaic::TelescopeProtocol::Seestar => {
                                            let payload = serde_json::json!({
                                                "id": 106,
                                                "method": "auto_focal"
                                            });
                                            let msg = format!("{}\r\n", payload);
                                            let _ = stream.write_all(msg.as_bytes());
                                        }
                                        crate::telescope::mosaic::TelescopeProtocol::DwarfLab => {
                                            let payload = serde_json::json!({
                                                "interface": 10010,
                                                "cmd": "autofocus"
                                            });
                                            let msg = format!("{}\r\n", payload);
                                            let _ = stream.write_all(msg.as_bytes());
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        SeestarCommand::ConfigureDither(cfg) => {
                            let mut t = telem_clone.lock().unwrap();
                            t.dither_config = cfg;
                            t.last_log_message = if cfg.enabled {
                                format!(
                                    "Ditheröinti asetettu: joka {}. kuva, {} px, asettuminen {}s.",
                                    cfg.interval_frames, cfg.step_pixels, cfg.settle_seconds
                                )
                            } else {
                                "Ditheröinti kytketty pois päältä.".to_string()
                            };
                            let is_sim = t.is_simulation;
                            drop(t);

                            if !is_sim {
                                if let Some(stream) = &mut active_stream {
                                    let payload = serde_json::json!({
                                        "id": 107,
                                        "method": "set_setting",
                                        "params": {
                                            "dither": {
                                                "enabled": cfg.enabled,
                                                "pix": cfg.step_pixels,
                                                "interval": cfg.interval_frames
                                            }
                                        }
                                    });
                                    let _ = stream.write_all(format!("{}\r\n", payload).as_bytes());
                                }
                            }
                        }
                        SeestarCommand::TriggerDither { pixels, settle_secs } => {
                            let mut t = telem_clone.lock().unwrap();
                            let prev_state = match t.state {
                                SeestarState::Dithering => SeestarState::Stacking,
                                ref s => s.clone(),
                            };
                            t.state = SeestarState::Dithering;
                            t.dither_count += 1;
                            let count = t.dither_count;
                            t.last_log_message = format!(
                                "Ditheröinti #{}: mikrosiirto {} px, asettuminen {}s...",
                                count, pixels, settle_secs
                            );
                            // Apply tiny deterministic spiral micro-offset in RA/Dec (~2.4 arcsec/px on S-30)
                            let angle = (count as f64) * 2.39996; // Golden angle spiral
                            let offset_deg = (pixels as f64) * (2.4 / 3600.0);
                            t.current_ra += (offset_deg * angle.cos()) / 15.0;
                            t.current_dec += offset_deg * angle.sin();
                            let is_sim = t.is_simulation;
                            drop(t);

                            dither_start = Some((Instant::now(), settle_secs.max(1), prev_state));

                            if !is_sim {
                                if let Some(stream) = &mut active_stream {
                                    let payload = serde_json::json!({
                                        "id": 108,
                                        "method": "scope_step_move",
                                        "params": {
                                            "pix": pixels,
                                            "settle_sec": settle_secs
                                        }
                                    });
                                    let _ = stream.write_all(format!("{}\r\n", payload).as_bytes());
                                }
                            }
                        }
                        SeestarCommand::SetAutoReconnect(enabled) => {
                            let mut t = telem_clone.lock().unwrap();
                            t.auto_reconnect_enabled = enabled;
                            t.last_log_message = format!(
                                "Automaattinen uudelleenyhdistys: {}",
                                if enabled { "PÄÄLLÄ" } else { "POIS" }
                            );
                        }
                    }
                }

                // Handle focus & dither completion and simulation progression
                {
                    let mut t = telem_clone.lock().unwrap();

                    // Auto-Focus completion check (both simulation and real timeout fallback)
                    if let Some((f_start, ref ret_state)) = focus_start {
                        let af_duration = if t.is_simulation { 3.0 } else { 15.0 };
                        if f_start.elapsed().as_secs_f64() >= af_duration {
                            // Adjust simulated focus position based on temperature (~ -4 steps per °C drop)
                            let temp_offset = ((15.0 - t.temp_c) * 4.0).round() as i32;
                            t.focus_position = (1450 + temp_offset).clamp(500, 2500) as u32;
                            t.last_focus_temp_c = Some(t.temp_c);
                            t.last_focus_time = Some(Local::now().format("%H:%M:%S").to_string());
                            t.state = if *ret_state == SeestarState::Stacking {
                                SeestarState::Stacking
                            } else {
                                SeestarState::Tracking
                            };
                            t.last_log_message = format!(
                                "Automaattitarkennus valmis! Positio: {} (Lämpötila: {:.1} °C)",
                                t.focus_position, t.temp_c
                            );
                            focus_start = None;
                        }
                    }

                    // Dither settling completion check
                    if let Some((d_start, settle_secs, ref ret_state)) = dither_start {
                        let sim_settle = if t.is_simulation {
                            (settle_secs as f64).min(2.5)
                        } else {
                            settle_secs as f64
                        };
                        if d_start.elapsed().as_secs_f64() >= sim_settle {
                            t.state = ret_state.clone();
                            t.last_log_message = format!(
                                "Ditheröinti #{} valmis, seuranta vakiintunut.",
                                t.dither_count
                            );
                            dither_start = None;
                        }
                    }

                    if t.is_simulation {
                        if let Some((start_t, s_ra, s_dec, e_ra, e_dec)) = slew_start {
                            let elapsed = start_t.elapsed().as_secs_f64();
                            let slew_duration = 3.5; // simulate 3.5s slew
                            if elapsed < slew_duration {
                                let frac = (elapsed / slew_duration).clamp(0.0, 1.0);
                                t.current_ra = s_ra + (e_ra - s_ra) * frac;
                                t.current_dec = s_dec + (e_dec - s_dec) * frac;
                                t.state = SeestarState::Slewing;
                            } else if elapsed < slew_duration + 2.0 {
                                // Plate solving phase
                                t.current_ra = e_ra;
                                t.current_dec = e_dec;
                                t.state = SeestarState::Solving;
                            } else {
                                // Arrived & tracking
                                t.current_ra = e_ra;
                                t.current_dec = e_dec;
                                t.state = SeestarState::Tracking;
                                t.last_log_message = format!("Kohde {} lukittu! Plate solving onnistui, seuranta aktiivinen.", t.target_name);
                                slew_start = None;
                            }
                        }

                        // Stacking frame counter in simulation
                        if t.state == SeestarState::Stacking {
                            if stack_timer.elapsed() >= Duration::from_secs(10) {
                                t.stacked_frames += 1;
                                t.total_stack_seconds += 10;
                                // Simulate gradual ambient cooling at night (-0.04 °C per frame)
                                t.temp_c = (t.temp_c - 0.04).max(-25.0);
                                stack_timer = Instant::now();
                            }
                        }
                    }
                }

                // Telemetry and health check on real telescope
                let mut stream_lost = false;
                let (is_conn, is_sim, cur_proto, cur_ip, cur_port, cur_state) = {
                    let t = telem_clone.lock().unwrap();
                    (t.connected, t.is_simulation, t.protocol, t.ip_address.clone(), t.port, t.state.clone())
                };

                if is_conn && !is_sim {
                    if cur_proto == crate::telescope::mosaic::TelescopeProtocol::AscomAlpaca {
                        if last_heartbeat.elapsed() > Duration::from_millis(1800) {
                            let mut poll_success = false;

                            // 1. Check park state
                            if let Ok(val) = alpaca_get(&alpaca_agent, &cur_ip, cur_port, "telescope/0/atpark", &[("ClientID", "1")]) {
                                poll_success = true;
                                if let Some(at_park) = val.get("Value").and_then(|v| v.as_bool()) {
                                    let mut t = telem_clone.lock().unwrap();
                                    if at_park {
                                        t.state = SeestarState::Parked;
                                    } else if t.state == SeestarState::Parked {
                                        t.state = SeestarState::Idle;
                                    }
                                }
                            }

                            // 2. Check slewing state
                            if let Ok(val) = alpaca_get(&alpaca_agent, &cur_ip, cur_port, "telescope/0/slewing", &[("ClientID", "1")]) {
                                poll_success = true;
                                if let Some(slewing) = val.get("Value").and_then(|v| v.as_bool()) {
                                    let mut t = telem_clone.lock().unwrap();
                                    if slewing {
                                        t.state = SeestarState::Slewing;
                                    } else if t.state == SeestarState::Slewing {
                                        t.state = SeestarState::Tracking;
                                    }
                                }
                            }

                            // 3. Check tracking state (if not parked, slewing, or stacking)
                            if cur_state != SeestarState::Parked && cur_state != SeestarState::Slewing && cur_state != SeestarState::Stacking {
                                if let Ok(val) = alpaca_get(&alpaca_agent, &cur_ip, cur_port, "telescope/0/tracking", &[("ClientID", "1")]) {
                                    poll_success = true;
                                    if let Some(tracking) = val.get("Value").and_then(|v| v.as_bool()) {
                                        let mut t = telem_clone.lock().unwrap();
                                        if tracking && t.state == SeestarState::Idle {
                                            t.state = SeestarState::Tracking;
                                        }
                                    }
                                }
                            }

                            // 4. Coordinates
                            if let Ok(val) = alpaca_get(&alpaca_agent, &cur_ip, cur_port, "telescope/0/rightascension", &[("ClientID", "1")]) {
                                poll_success = true;
                                if let Some(ra) = val.get("Value").and_then(|v| v.as_f64()) {
                                    let mut t = telem_clone.lock().unwrap();
                                    t.current_ra = ra;
                                }
                            }
                            if let Ok(val) = alpaca_get(&alpaca_agent, &cur_ip, cur_port, "telescope/0/declination", &[("ClientID", "1")]) {
                                poll_success = true;
                                if let Some(dec) = val.get("Value").and_then(|v| v.as_f64()) {
                                    let mut t = telem_clone.lock().unwrap();
                                    t.current_dec = dec;
                                }
                            }
                            if let Ok(val) = alpaca_get(&alpaca_agent, &cur_ip, cur_port, "telescope/0/altitude", &[("ClientID", "1")]) {
                                if let Some(alt) = val.get("Value").and_then(|v| v.as_f64()) {
                                    let mut t = telem_clone.lock().unwrap();
                                    t.current_alt = alt;
                                }
                            }
                            if let Ok(val) = alpaca_get(&alpaca_agent, &cur_ip, cur_port, "telescope/0/azimuth", &[("ClientID", "1")]) {
                                if let Some(az) = val.get("Value").and_then(|v| v.as_f64()) {
                                    let mut t = telem_clone.lock().unwrap();
                                    t.current_az = az;
                                }
                            }

                            // 5. CCD temperature
                            if let Ok(val) = alpaca_get(&alpaca_agent, &cur_ip, cur_port, "camera/0/ccdtemperature", &[("ClientID", "1")]) {
                                if let Some(temp) = val.get("Value").and_then(|v| v.as_f64()) {
                                    let mut t = telem_clone.lock().unwrap();
                                    t.temp_c = temp as f32;
                                }
                            }

                            // 6. Filterwheel position
                            if let Ok(val) = alpaca_get(&alpaca_agent, &cur_ip, cur_port, "filterwheel/0/position", &[("ClientID", "1")]) {
                                if let Some(pos) = val.get("Value").and_then(|v| v.as_i64()) {
                                    let mut t = telem_clone.lock().unwrap();
                                    if pos == 2 {
                                        t.lp_filter_active = true;
                                    } else if pos == 1 {
                                        t.lp_filter_active = false;
                                    }
                                }
                            }

                            // 7. Focuser position (if unparked)
                            if cur_state != SeestarState::Parked {
                                if let Ok(val) = alpaca_get(&alpaca_agent, &cur_ip, cur_port, "focuser/0/position", &[("ClientID", "1")]) {
                                    if let Some(pos) = val.get("Value").and_then(|v| v.as_i64()) {
                                        if pos > 0 {
                                            let mut t = telem_clone.lock().unwrap();
                                            t.focus_position = pos as u32;
                                        }
                                    }
                                }
                            }

                            if poll_success {
                                alpaca_fail_count = 0;
                            } else {
                                alpaca_fail_count += 1;
                                if alpaca_fail_count >= 5 {
                                    // 5 consecutive complete polling failures (~9s) means Wi-Fi / power dropped
                                    stream_lost = true;
                                }
                            }

                            last_heartbeat = Instant::now();
                        }
                    } else if let Some(stream) = &mut active_stream {
                        // Raw TCP stream protocol (e.g. port 4700 or DwarfLab)
                        if last_heartbeat.elapsed() > Duration::from_secs(3) {
                            let query = serde_json::json!({
                                "id": 999,
                                "method": "get_device_state"
                            });
                            if stream.write_all(format!("{}\r\n", query).as_bytes()).is_err() {
                                stream_lost = true;
                            }
                            last_heartbeat = Instant::now();
                        }

                        if !stream_lost {
                            let mut reader = BufReader::new(stream);
                            let mut line = String::new();
                            match reader.read_line(&mut line) {
                                Ok(0) => {
                                    stream_lost = true;
                                    let is_seestar_raw = { telem_clone.lock().unwrap().protocol == crate::telescope::mosaic::TelescopeProtocol::Seestar };
                                    if is_seestar_raw {
                                        let mut t = telem_clone.lock().unwrap();
                                        t.last_log_message = "Seestar sulki TCP-ohjausportin 4700 (ZWO firmware -suojaus). Ota Seestar Alpaca käyttöön (portti 32323) tai pinoa kuvat tietokoneella SMB-jaolla.".to_string();
                                        user_disconnected = true;
                                    }
                                }
                                Ok(_) if !line.is_empty() => {
                                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) {
                                        let mut t = telem_clone.lock().unwrap();
                                        if val.get("error").is_some() || line.contains("verify") || line.contains("unauthorized") {
                                            t.last_log_message = "ZWO Seestar vaatii RSA-varmennuksen portissa 4700. Ota Seestar Alpaca käyttöön (portti 32323) tai käytä SMB-jakoa.".to_string();
                                            stream_lost = true;
                                            user_disconnected = true;
                                        }
                                        if let Some(result) = val.get("result") {
                                            if let Some(battery) = result.get("battery").and_then(|v| v.as_u64()) {
                                                t.battery_pct = battery as u8;
                                            }
                                            if let Some(temp) = result.get("temp").and_then(|v| v.as_f64()) {
                                                t.temp_c = temp as f32;
                                            }
                                            if let Some(ra) = result.get("cur_ra").and_then(|v| v.as_f64()) {
                                                t.current_ra = ra;
                                            }
                                            if let Some(dec) = result.get("cur_dec").and_then(|v| v.as_f64()) {
                                                t.current_dec = dec;
                                            }
                                            if let Some(pos) = result.get("focus_pos").and_then(|v| v.as_u64()) {
                                                t.focus_position = pos as u32;
                                            }
                                            if let Some(frames) = result.get("stacked_frame").and_then(|v| v.as_u64()) {
                                                t.stacked_frames = frames as u32;
                                            }
                                            if let Some(stage) = result.get("stage").and_then(|v| v.as_str()) {
                                                match stage {
                                                    "Stack" | "ContinuousExposure" => t.state = SeestarState::Stacking,
                                                    "AutoGoto" => t.state = SeestarState::Slewing,
                                                    "AutoFocus" => t.state = SeestarState::Focusing,
                                                    "Tracking" => {
                                                        if t.state == SeestarState::Focusing {
                                                            t.last_focus_temp_c = Some(t.temp_c);
                                                            t.last_focus_time = Some(Local::now().format("%H:%M:%S").to_string());
                                                            focus_start = None;
                                                        }
                                                        t.state = SeestarState::Tracking;
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        }
                                    }
                                }
                                Ok(_) => {}
                                Err(e) => {
                                    if e.kind() != ErrorKind::WouldBlock && e.kind() != ErrorKind::TimedOut {
                                        stream_lost = true;
                                    }
                                }
                            }
                        }
                    }
                }

                // Handle unexpected connection loss
                if stream_lost {
                    active_stream = None;
                    let mut t = telem_clone.lock().unwrap();
                    t.connected = false;
                    if t.auto_reconnect_enabled && !user_disconnected {
                        pre_disconnect_state = t.state.clone();
                        t.state = SeestarState::Reconnecting;
                        t.reconnect_attempts = 1;
                        next_reconnect_at = Some(Instant::now() + Duration::from_secs(2));
                        t.last_log_message = format!(
                            "Yhteys Seestariin katkesi! Yritetään palautusta automaattisesti (1/{})...",
                            t.max_reconnect_attempts
                        );
                    } else {
                        t.state = SeestarState::Disconnected;
                        t.last_log_message = "Yhteys Seestariin katkesi.".to_string();
                    }
                }

                // Execute scheduled auto-reconnect attempts with exponential backoff
                if let Some(reconnect_instant) = next_reconnect_at {
                    if Instant::now() >= reconnect_instant {
                        let (ip, port, proto, attempt, max_attempts, auto_enabled) = {
                            let t = telem_clone.lock().unwrap();
                            (
                                t.ip_address.clone(),
                                t.port,
                                t.protocol,
                                t.reconnect_attempts,
                                t.max_reconnect_attempts,
                                t.auto_reconnect_enabled,
                            )
                        };

                        if !auto_enabled || user_disconnected {
                            next_reconnect_at = None;
                        } else {
                            let mut reconnected = false;

                            if proto == crate::telescope::mosaic::TelescopeProtocol::AscomAlpaca {
                                let connect_form = [("Connected", "true"), ("ClientID", "1"), ("ClientTransactionID", "1")];
                                if alpaca_put(&alpaca_agent, &ip, port, "telescope/0/connected", &connect_form).is_ok() {
                                    let _ = alpaca_put(&alpaca_agent, &ip, port, "camera/0/connected", &connect_form);
                                    let _ = alpaca_put(&alpaca_agent, &ip, port, "filterwheel/0/connected", &connect_form);
                                    let _ = alpaca_put(&alpaca_agent, &ip, port, "focuser/0/connected", &connect_form);
                                    reconnected = true;
                                    alpaca_fail_count = 0;
                                }
                            } else {
                                let addr_str = format!("{}:{}", ip, port);
                                let resolved_addr = if let Ok(addr) = addr_str.parse::<SocketAddr>() {
                                    Some(addr)
                                } else if let Ok(mut addrs) = addr_str.to_socket_addrs() {
                                    addrs.next()
                                } else {
                                    None
                                };

                                if let Some(addr) = resolved_addr {
                                    if let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_secs(3)) {
                                        let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                                        let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));

                                        let query = serde_json::json!({
                                            "id": 999,
                                            "method": "get_device_state"
                                        });
                                        let _ = stream.write_all(format!("{}\r\n", query).as_bytes());
                                        active_stream = Some(stream);
                                        reconnected = true;
                                    }
                                }
                            }

                            let mut t = telem_clone.lock().unwrap();
                            if reconnected {
                                next_reconnect_at = None;
                                t.connected = true;
                                t.reconnect_attempts = 0;
                                t.state = match pre_disconnect_state {
                                    SeestarState::Reconnecting | SeestarState::Disconnected | SeestarState::Connecting => {
                                        SeestarState::Tracking
                                    }
                                    ref s => s.clone(),
                                };
                                t.last_log_message = format!(
                                    "✓ Yhteys Seestariin ({}) palautettu automaattisesti! Tila täsmäytetty.",
                                    ip
                                );
                            } else if attempt >= max_attempts {
                                next_reconnect_at = None;
                                t.state = SeestarState::Disconnected;
                                t.last_log_message = format!(
                                    "Yhteyden palautus epäonnistui ({} yritystä). Tarkista Seestarin Wi-Fi.",
                                    max_attempts
                                );
                            } else {
                                let next_attempt = attempt + 1;
                                t.reconnect_attempts = next_attempt;
                                let delay_secs = (2u64.saturating_pow(attempt.min(4))).min(30);
                                next_reconnect_at = Some(Instant::now() + Duration::from_secs(delay_secs));
                                t.last_log_message = format!(
                                    "Uudelleenyhdistys ({}/{}) {}s kuluttua...",
                                    next_attempt, max_attempts, delay_secs
                                );
                            }
                        }
                    }
                }

                thread::sleep(Duration::from_millis(100));
            }
        });

        Self {
            telemetry,
            stop_signal,
            command_tx,
        }
    }

    pub fn connect(&self, ip: &str) {
        let _ = self.command_tx.send(SeestarCommand::Connect(ip.to_string()));
    }

    pub fn disconnect(&self) {
        let _ = self.command_tx.send(SeestarCommand::Disconnect);
    }

    pub fn set_simulation(&self, sim: bool) {
        let _ = self.command_tx.send(SeestarCommand::SetSimulation(sim));
    }

    pub fn set_model(&self, model: crate::telescope::mosaic::TelescopeModel) {
        let _ = self.command_tx.send(SeestarCommand::SetModel(model));
    }

    pub fn goto_target(&self, name: &str, ra_hours: f64, dec_deg: f64) {
        let _ = self.command_tx.send(SeestarCommand::GoTo {
            name: name.to_string(),
            ra: ra_hours,
            dec: dec_deg,
        });
    }

    pub fn start_stacking(&self) {
        let _ = self.command_tx.send(SeestarCommand::StartStacking);
    }

    pub fn stop_stacking(&self) {
        let _ = self.command_tx.send(SeestarCommand::StopStacking);
    }

    pub fn park(&self) {
        let _ = self.command_tx.send(SeestarCommand::Park);
    }

    pub fn unpark(&self) {
        let _ = self.command_tx.send(SeestarCommand::Unpark);
    }

    pub fn toggle_lp_filter(&self) {
        let _ = self.command_tx.send(SeestarCommand::ToggleLpFilter);
    }

    pub fn set_lp_filter(&self, enabled: bool) {
        let _ = self.command_tx.send(SeestarCommand::SetLpFilter(enabled));
    }

    pub fn auto_focus(&self) {
        let _ = self.command_tx.send(SeestarCommand::AutoFocus);
    }

    pub fn configure_dither(&self, config: DitherConfig) {
        let _ = self.command_tx.send(SeestarCommand::ConfigureDither(config));
    }

    pub fn trigger_dither(&self, pixels: u32, settle_secs: u32) {
        let _ = self.command_tx.send(SeestarCommand::TriggerDither { pixels, settle_secs });
    }

    pub fn set_auto_reconnect(&self, enabled: bool) {
        let _ = self.command_tx.send(SeestarCommand::SetAutoReconnect(enabled));
    }

    pub fn get_telemetry(&self) -> SeestarTelemetry {
        self.telemetry.lock().unwrap().clone()
    }
}

impl Drop for SeestarClient {
    fn drop(&mut self) {
        self.stop_signal.store(true, Ordering::SeqCst);
    }
}
