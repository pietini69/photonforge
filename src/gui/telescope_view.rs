use crate::telescope::catalog::{DsoType, SkyObject};
use crate::telescope::coordinates::{
    alt_az_to_canvas_normalized, format_dec, format_ra, night_altitude_profile, ra_dec_to_alt_az,
};
use crate::telescope::discovery::{discover_telescope, TelescopeConfig};
use crate::telescope::downloader::LiveIngestManager;
use crate::telescope::mosaic::{MosaicGridConfig, MosaicPlan, MosaicScopeModel};
use crate::telescope::seestar::{FilterChoice, SeestarClient, SeestarState};
use crate::telescope::sequencer::{FrameType, JobStatus, NightSequencer, SequenceItem, SequenceStep};
use chrono::Utc;
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Stroke, Vec2};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TelescopeCenterView {
    StarMap,
    Sequencer,
    ImageViewer,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StarMapMode {
    Horizon,
    ZenithAllSky,
}

pub struct TelescopeViewState {
    pub center_view: TelescopeCenterView,
    pub seestar_ip: String,
    pub selected_object: Option<SkyObject>,
    pub search_query: String,
    pub filter_type: Option<DsoType>,
    pub map_mode: StarMapMode,
    pub view_az: f64,
    pub view_alt: f64,
    pub horizon_fov_deg: f64,
    pub map_zoom: f32,
    pub map_pan: Vec2,
    pub map_fill_width: bool,
    pub sequence_duration_mins: u32,
    pub sequence_sub_exposure_secs: u32,
    pub sequence_use_lp: bool,
    pub mosaic_enabled: bool,
    pub mosaic_config: MosaicGridConfig,
    pub mosaic_plan: MosaicPlan,
    pub request_mosaic_stitcher: bool,
    pub section_connection_open: bool,
    pub section_ingest_open: bool,
    pub section_target_open: bool,
    pub section_search_open: bool,
    pub section_mosaic_open: bool,
    pub section_sequencer_open: bool,
    pub is_discovering: bool,
    pub discovery_rx: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    pub discovery_status: Option<String>,
}

impl Default for TelescopeViewState {
    fn default() -> Self {
        let cfg = TelescopeConfig::load();
        let model = cfg.get_model();
        let ip = cfg.get_ip_for_model(model);

        let mut mosaic_config = MosaicGridConfig::default();
        mosaic_config.scope_model = model;
        let mosaic_plan = MosaicPlan::compute(&mosaic_config);

        Self {
            center_view: TelescopeCenterView::StarMap,
            seestar_ip: ip,
            selected_object: None,
            search_query: String::new(),
            filter_type: None,
            map_mode: StarMapMode::Horizon,
            view_az: 180.0,
            view_alt: 30.0,
            horizon_fov_deg: 85.0,
            map_zoom: 1.0,
            map_pan: Vec2::ZERO,
            map_fill_width: true,
            sequence_duration_mins: 60,
            sequence_sub_exposure_secs: cfg.exposure_seconds,
            sequence_use_lp: false,
            mosaic_enabled: false,
            mosaic_config,
            mosaic_plan,
            request_mosaic_stitcher: false,
            section_connection_open: false,
            section_ingest_open: false,
            section_target_open: true,
            section_search_open: false,
            section_mosaic_open: true,
            section_sequencer_open: true,
            is_discovering: false,
            discovery_rx: None,
            discovery_status: None,
        }
    }
}

impl TelescopeViewState {
    pub fn save_settings(&self, scope_name: &str) {
        let clean_ip = self.seestar_ip.trim();
        if !clean_ip.is_empty() {
            let mut cfg = TelescopeConfig::load();
            cfg.last_ip = clean_ip.to_string();
            cfg.scope_model = Some(scope_name.to_string());
            cfg.exposure_seconds = self.sequence_sub_exposure_secs;
            cfg.model_ips.insert(scope_name.to_string(), clean_ip.to_string());
            cfg.save();
        }
    }

    pub fn start_discovery(&mut self) {
        if self.is_discovering {
            return;
        }
        self.is_discovering = true;
        self.discovery_status = Some(crate::i18n::tr("Etsitään kaukoputkea lähiverkosta...").to_string());
        let (tx, rx) = std::sync::mpsc::channel();
        self.discovery_rx = Some(rx);
        let last_ip = self.seestar_ip.clone();

        std::thread::spawn(move || {
            let res = discover_telescope(&last_ip);
            let _ = tx.send(res);
        });
    }

    pub fn check_discovery(&mut self) {
        if let Some(rx) = &self.discovery_rx {
            if let Ok(res) = rx.try_recv() {
                self.is_discovering = false;
                self.discovery_rx = None;
                match res {
                    Ok(found_ip) => {
                        self.seestar_ip = found_ip.clone();
                        self.discovery_status = Some(format!("{}: {}", crate::i18n::tr("Löydetty:"), found_ip));
                        let mut cfg = TelescopeConfig::load();
                        cfg.last_ip = found_ip.clone();
                        if let Some(ref model) = cfg.scope_model {
                            cfg.model_ips.insert(model.clone(), found_ip);
                        }
                        cfg.save();
                    }
                    Err(err_msg) => {
                        self.discovery_status = Some(err_msg);
                    }
                }
            }
        }
    }

    pub fn recompute_mosaic(&mut self) {
        if let Some(target) = &self.selected_object {
            let short_name = target.name.split('/').next().unwrap_or(&target.name).trim();
            let mut clean_short = short_name;
            if let Some(idx) = clean_short.find(crate::i18n::tr("Mosaiikki")) {
                clean_short = clean_short[..idx].trim();
            }
            if let Some(idx) = clean_short.find('[') {
                clean_short = clean_short[..idx].trim();
            }
            if let Some(idx) = clean_short.find('(') {
                clean_short = clean_short[..idx].trim();
            }
            let clean_short = clean_short.trim();

            self.mosaic_config.target_name = if clean_short.is_empty() || clean_short == target.id {
                target.id.clone()
            } else if clean_short.starts_with(&target.id) {
                clean_short.to_string()
            } else {
                format!("{} {}", target.id, clean_short)
            };
            self.mosaic_config.center_ra_hours = target.ra_hours;
            self.mosaic_config.center_dec_deg = target.dec_deg;
        }
        self.mosaic_plan = MosaicPlan::compute(&self.mosaic_config);
    }
}

pub struct TelescopeView;

impl TelescopeView {
    /// Renders the Left Sidebar for Seestar S-30 Control & Sequencer
    pub fn ui_sidebar(
        ui: &mut egui::Ui,
        state: &mut TelescopeViewState,
        client: &Arc<SeestarClient>,
        sequencer: &Arc<NightSequencer>,
        ingest_mgr: &Arc<LiveIngestManager>,
        catalog: &[SkyObject],
        lat: f64,
        lon: f64,
    ) {
        let telem = client.get_telemetry();
        let now_utc = Utc::now();
        state.check_discovery();
        ui.set_max_width(ui.available_width());

        // Quick Toolbar: Expand all / Collapse all
        ui.horizontal(|ui| {
            ui.label(RichText::new(crate::i18n::tr("Näkymän osiot:")).small().color(Color32::from_rgb(150, 165, 185)));
            if ui.small_button(crate::i18n::tr("⊞ Avaa kaikki")).clicked() {
                state.section_connection_open = true;
                state.section_ingest_open = true;
                state.section_target_open = true;
                state.section_search_open = true;
                state.section_mosaic_open = true;
                state.section_sequencer_open = true;
            }
            if ui.small_button(crate::i18n::tr("⊟ Tiivistä")).clicked() {
                state.section_connection_open = false;
                state.section_ingest_open = false;
                state.section_target_open = false;
                state.section_search_open = false;
                state.section_mosaic_open = true;
                state.section_sequencer_open = true;
            }
        });
        ui.add_space(4.0);

        // 1. TELESCOPE CONNECTION & TELEMETRY
        ui.group(|ui| {
            ui.set_max_width(ui.available_width());
            let (sr, sg, sb) = telem.state.color_rgb();
            let arrow = if state.section_connection_open { "▼" } else { "▶" };
            let scope_header = format!("{} 🔭 {}", arrow, telem.scope_model.display_short());

            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(RichText::new(scope_header).color(Color32::from_rgb(80, 200, 255)).strong()).frame(false)).clicked() {
                    state.section_connection_open = !state.section_connection_open;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let status_lbl = format!("● {}", crate::i18n::tr(telem.state.display_fi()));
                    ui.label(RichText::new(status_lbl).color(Color32::from_rgb(sr, sg, sb)).strong().small());
                });
            });

            if state.section_connection_open {
                ui.add_space(4.0);

                // Scope model selector
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(crate::i18n::tr("Kaukoputki:")).strong());
                    let mut current_model = telem.scope_model;
                    let combo_w = (ui.available_width() - 8.0).clamp(160.0, 280.0);
                    egui::ComboBox::from_id_salt("sidebar_telescope_model_select")
                        .width(combo_w)
                        .selected_text(current_model.display_short())
                        .show_ui(ui, |ui| {
                            ui.label(RichText::new("── ZWO Seestar ──").small().color(Color32::from_rgb(100, 200, 255)));
                            ui.selectable_value(&mut current_model, MosaicScopeModel::SeestarS30, "ZWO Seestar S-30 (150mm f/5, 2.1° × 1.2°)");
                            ui.selectable_value(&mut current_model, MosaicScopeModel::SeestarS30Pro, "ZWO Seestar S-30 Pro (160mm f/5.3 4K, 4.0° × 2.2°)");
                            ui.selectable_value(&mut current_model, MosaicScopeModel::SeestarS50, "ZWO Seestar S-50 (250mm f/5, 1.3° × 0.7°)");
                            ui.selectable_value(&mut current_model, MosaicScopeModel::SeestarS50Pro, "ZWO Seestar S-50 Pro (260mm f/5.2 4K, 2.5° × 1.4°)");

                            ui.separator();
                            ui.label(RichText::new("── DwarfLab (Avoin API & Alpaca) ──").small().color(Color32::from_rgb(255, 180, 100)));
                            ui.selectable_value(&mut current_model, MosaicScopeModel::Dwarf3, "DwarfLab DWARF 3 (150mm f/4.3 4K, 2.9° × 1.7°)");
                            ui.selectable_value(&mut current_model, MosaicScopeModel::DwarfMini, "DwarfLab DWARF Mini (150mm f/5, 2.1° × 1.2°)");
                            ui.selectable_value(&mut current_model, MosaicScopeModel::Dwarf2, "DwarfLab DWARF II (100mm f/4.2 4K, 3.2° × 1.8°)");
                            ui.selectable_value(&mut current_model, MosaicScopeModel::DwarfDraco, "DwarfLab DRACO (340mm f/3.8, 90mm aukko, 1.6° × 1.2°)");

                            ui.separator();
                            ui.label(RichText::new("── Celestron ──").small().color(Color32::from_rgb(255, 220, 120)));
                            ui.selectable_value(&mut current_model, MosaicScopeModel::CelestronOrigin, "Celestron Origin RASA (335mm f/2.2, 1.3° × 0.8°)");

                            ui.separator();
                            ui.label(RichText::new("── Universaali Alpaca / Oma ──").small().color(Color32::from_rgb(180, 220, 180)));
                            ui.selectable_value(&mut current_model, MosaicScopeModel::AscomAlpaca, "ASCOM Alpaca (Universal REST Mount)");
                            ui.selectable_value(&mut current_model, MosaicScopeModel::Custom, crate::i18n::tr("Oma määritys"));
                        });
                    if current_model != telem.scope_model {
                        client.set_model(current_model);
                        state.mosaic_config.scope_model = current_model;
                        let mut cfg = TelescopeConfig::load();
                        let target_ip = cfg.get_ip_for_model(current_model);
                        state.seestar_ip = target_ip.clone();
                        cfg.last_ip = target_ip.clone();
                        cfg.scope_model = Some(current_model.display_short().to_string());
                        cfg.model_ips.insert(current_model.display_short().to_string(), target_ip);
                        cfg.save();

                        ingest_mgr.set_model_source_path(current_model);
                        state.recompute_mosaic();
                    }
                });

                // Specs badge
                let (fov_w, fov_h) = telem.scope_model.fov_deg();
                ui.add(
                    egui::Label::new(
                        RichText::new(format!(
                            "📐 {}mm f/{:.1} | {}: {} | {} | FOV: {:.2}° × {:.2}°",
                            telem.scope_model.focal_length_mm(),
                            telem.scope_model.focal_ratio(),
                            crate::i18n::tr("Kenno"),
                            telem.scope_model.sensor_name(),
                            telem.scope_model.protocol().protocol_name(),
                            fov_w,
                            fov_h
                        ))
                        .small()
                        .color(Color32::from_rgb(140, 185, 215)),
                    )
                    .wrap(),
                );
                ui.add_space(2.0);

                // Connection controls - Row 1: IP, Etsi, Yhdistä
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("IP:").strong());
                    let ip_resp = ui.add(egui::TextEdit::singleline(&mut state.seestar_ip).desired_width(115.0));
                    if ip_resp.changed() || ip_resp.lost_focus() {
                        let clean = state.seestar_ip.trim().to_string();
                        if !clean.is_empty() {
                            let mut cfg = TelescopeConfig::load();
                            cfg.last_ip = clean.clone();
                            cfg.scope_model = Some(telem.scope_model.display_short().to_string());
                            cfg.model_ips.insert(telem.scope_model.display_short().to_string(), clean);
                            cfg.save();
                        }
                    }

                    if state.is_discovering {
                        ui.add_enabled(
                            false,
                            egui::Button::new(
                                RichText::new(crate::i18n::tr("⏳ Etsitään..."))
                                    .small()
                                    .color(Color32::from_rgb(255, 200, 100)),
                            ),
                        );
                    } else if ui
                        .button(crate::i18n::tr("🔍 Etsi"))
                        .on_hover_text(crate::i18n::tr("Hae Seestar automaattisesti lähiverkosta (mDNS / IP-haku)"))
                        .clicked()
                    {
                        state.start_discovery();
                    }

                    if telem.connected || telem.state == SeestarState::Reconnecting {
                        if ui.button(crate::i18n::tr("Katkaise")).clicked() {
                            client.disconnect();
                        }
                    } else if ui.button(crate::i18n::tr("🔌 Yhdistä")).clicked() {
                        state.save_settings(telem.scope_model.display_short());
                        client.connect(&state.seestar_ip);
                    }
                });

                // Connection controls - Row 2: Simulaatio & Automaattinen uudelleenyhdistys
                ui.horizontal_wrapped(|ui| {
                    let mut sim = telem.is_simulation;
                    if ui.checkbox(&mut sim, crate::i18n::tr("🎮 Simulaatio")).on_hover_text(crate::i18n::tr("Kokeile ohjausta ja karttaa ilman kaukoputkea")).changed() {
                        client.set_simulation(sim);
                        let mut cfg = TelescopeConfig::load();
                        cfg.is_simulation = sim;
                        cfg.save();
                    }

                    let mut auto_reconn = telem.auto_reconnect_enabled;
                    if ui
                        .checkbox(&mut auto_reconn, crate::i18n::tr("🛡 Automaattinen uudelleenyhdistys"))
                        .on_hover_text(crate::i18n::tr("Palauttaa TCP-yhteyden automaattisesti eksponentiaalisella viiveellä Wi-Fi-katkoksen sattuessa"))
                        .changed()
                    {
                        client.set_auto_reconnect(auto_reconn);
                        let mut cfg = TelescopeConfig::load();
                        cfg.auto_reconnect = auto_reconn;
                        cfg.save();
                    }
                    if telem.state == SeestarState::Reconnecting {
                        ui.label(
                            RichText::new(format!("({} {}/{})", crate::i18n::tr("Yritys"), telem.reconnect_attempts, telem.max_reconnect_attempts))
                                .small()
                                .color(Color32::from_rgb(255, 170, 60)),
                        );
                    }
                });

                if let Some(msg) = &state.discovery_status {
                    let col = if state.is_discovering {
                        Color32::from_rgb(255, 200, 100)
                    } else if msg.starts_with("Löydetty") || msg.starts_with("Found") {
                        Color32::from_rgb(100, 240, 150)
                    } else {
                        Color32::from_rgb(255, 150, 120)
                    };
                    ui.add(egui::Label::new(RichText::new(format!("📡 {}", msg)).small().color(col)).wrap());
                }

                if telem.scope_model.brand() == "ZWO Seestar" {
                    ui.add_space(2.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(crate::i18n::tr(
                                "📡 ASCOM Alpaca -ohjaus (portti 32323): Ota Alpaca päälle Seestar-sovelluksesta (Profiili -> Laitehallinta -> Lisätoiminnot -> Seestar Alpaca: Päällä)."
                            ))
                            .small()
                            .color(Color32::from_rgb(100, 220, 160))
                        )
                        .wrap(),
                    );
                    ui.add_space(1.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(crate::i18n::tr(
                                "💡 Alkusuuntaus: Avaa putki ja tee alkusuuntaus (vaaterointi + tähtitunnistus) kerran Seestar-mobiilisovelluksella. Tämän jälkeen GoTo, seuranta ja kuvaus toimivat PhotonForgesta!"
                            ))
                            .small()
                            .color(Color32::from_rgb(120, 210, 255))
                        )
                        .wrap(),
                    );
                    ui.add_space(2.0);
                    if ui
                        .button(RichText::new(crate::i18n::tr("📁 Yhdistä kuvat tietokoneelle (SMB)")).color(Color32::from_rgb(100, 220, 255)).strong())
                        .on_hover_text(crate::i18n::tr("Avaa Seestarin jaettu kansio suoraan tiedostoselaimessa (smb://<IP>/EMMC Images). Tällöin PhotonForge pinoaa ja venyttää kuvat reaaliajassa tietokoneella!"))
                        .clicked()
                    {
                        LiveIngestManager::open_seestar_smb(&state.seestar_ip);
                    }
                    ui.add_space(2.0);
                }

                if telem.scope_model.brand() == "DwarfLab" {
                    ui.add_space(2.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(crate::i18n::tr(
                                "📡 DwarfLab Open API: Yhdistä laitteen IP-osoitteeseen portilla 9900 (WebSocket) tai käytä dwarfAlp ASCOM Alpaca -siltaa (portti 11111)."
                            ))
                            .small()
                            .color(Color32::from_rgb(255, 190, 100))
                        )
                        .wrap(),
                    );
                    ui.add_space(1.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(crate::i18n::tr(
                                "📁 Kuvat: DwarfLabin kuvat siirtyvät USB-kaapelilla (/Volumes/DWARF) tai sisäänrakennetun HTTP-palvelimen kautta (http://<IP>:8082)."
                            ))
                            .small()
                            .color(Color32::from_rgb(140, 210, 255))
                        )
                        .wrap(),
                    );
                    ui.add_space(2.0);
                }

                ui.add_space(4.0);

                // Telemetry indicators
                ui.horizontal_wrapped(|ui| {
                    // Battery
                    let bat_color = if telem.battery_pct > 50 {
                        Color32::from_rgb(100, 255, 120)
                    } else if telem.battery_pct > 20 {
                        Color32::from_rgb(255, 200, 80)
                    } else {
                        Color32::from_rgb(255, 80, 80)
                    };
                    ui.label(RichText::new(format!("🔋 {}%", telem.battery_pct)).color(bat_color).strong());

                    ui.separator();

                    // Temp
                    ui.label(RichText::new(format!("🌡 {:.1} °C", telem.temp_c)).color(Color32::from_rgb(200, 220, 255)));

                    ui.separator();

                    // LP filter
                    let lp_label = if telem.lp_filter_active { crate::i18n::tr("🌙 LP: Päällä") } else { crate::i18n::tr("☀️ LP: Pois") };
                    if ui.button(lp_label).on_hover_text(crate::i18n::tr("Kytke kaukoputken sisäinen valosaastesuodatin (Dual-Band)")).clicked() {
                        client.toggle_lp_filter();
                    }
                });

                // Focus & Dither telemetry row
                ui.horizontal_wrapped(|ui| {
                    let focus_str = if let Some(ftemp) = telem.last_focus_temp_c {
                        format!("{} {} ({:.1} °C)", crate::i18n::tr("🔍 Tarkennus:"), telem.focus_position, ftemp)
                    } else {
                        format!("{} {}", crate::i18n::tr("🔍 Tarkennus:"), telem.focus_position)
                    };
                    ui.label(RichText::new(focus_str).small().color(Color32::from_rgb(180, 215, 245)));
                    ui.separator();
                    ui.label(
                        RichText::new(format!(
                            "{} {} ({} {})",
                            crate::i18n::tr("🔀 Dither:"),
                            if telem.dither_config.enabled { crate::i18n::tr("Päällä") } else { crate::i18n::tr("Pois") },
                            telem.dither_count,
                            crate::i18n::tr("kpl")
                        ))
                        .small()
                        .color(Color32::from_rgb(190, 175, 245)),
                    );
                });

                ui.add_space(2.0);

                // Scope current position
                let (scope_alt, scope_az) = ra_dec_to_alt_az(telem.current_ra, telem.current_dec, lat, lon, &now_utc);
                ui.add(
                    egui::Label::new(
                        RichText::new(format!(
                            "{}: RA {} | Dec {}\n{}: {:.1}° | {}: {:.1}°",
                            crate::i18n::tr("Suunta"),
                            format_ra(telem.current_ra),
                            format_dec(telem.current_dec),
                            crate::i18n::tr("Korkeus"),
                            scope_alt,
                            crate::i18n::tr("Atsimuutti"),
                            scope_az
                        ))
                        .size(11.5)
                        .color(Color32::from_rgb(170, 185, 210)),
                    )
                    .wrap(),
                );

                ui.add_space(4.0);

                // Sub-exposure duration
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(format!("{}:", crate::i18n::tr("Valotusaika"))).strong().color(Color32::from_rgb(180, 220, 255)));
                    for &sec in &[10, 20, 30, 60] {
                        let is_sel = telem.exposure_seconds == sec;
                        if ui.add(egui::SelectableLabel::new(is_sel, format!("{}s", sec))).on_hover_text(format!("{} {}s", crate::i18n::tr("Aseta osavalotusajaksi"), sec)).clicked() {
                            client.set_exposure_time(sec);
                            state.sequence_sub_exposure_secs = sec;
                        }
                    }
                    let mut cur_exp = telem.exposure_seconds;
                    if ui.add(egui::DragValue::new(&mut cur_exp).range(1..=300).suffix("s")).on_hover_text(crate::i18n::tr("Mukautettu valotusaika sekunneissa")).changed() {
                        client.set_exposure_time(cur_exp);
                        state.sequence_sub_exposure_secs = cur_exp;
                    }
                });

                ui.add_space(3.0);

                // Control Buttons
                ui.horizontal_wrapped(|ui| {
                    if let Some(target) = &state.selected_object {
                        let goto_btn = egui::Button::new(
                            RichText::new(format!("🚀 GoTo {}", target.id))
                                .color(Color32::from_rgb(255, 220, 80))
                                .strong(),
                        );
                        if ui.add(goto_btn).on_hover_text(crate::i18n::tr("Käännä Seestar kohteeseen ja keskitä automaattisesti plate solvingilla")).clicked() {
                            client.goto_target(&target.name, target.ra_hours, target.dec_deg);
                        }
                    }

                    if telem.state == SeestarState::Stacking {
                        if ui.button(crate::i18n::tr("⏹ Pysäytä pinoaminen")).clicked() {
                            client.stop_stacking();
                        }
                    } else if ui.button(crate::i18n::tr("▶ Aloita pinoaminen")).on_hover_text(crate::i18n::tr("Aloita reaaliaikainen valotus ja pinoaminen")).clicked() {
                        client.start_stacking();
                    }

                    if ui.button(crate::i18n::tr("🔍 Tarkenna")).on_hover_text(crate::i18n::tr("Suorita automaattinen tarkennus")).clicked() {
                        client.auto_focus();
                    }

                    if ui.button(crate::i18n::tr("🔀 Ditheröi")).on_hover_text(crate::i18n::tr("Suorita välitön dither-mikrosiirto")).clicked() {
                        client.trigger_dither(telem.dither_config.step_pixels, telem.dither_config.settle_seconds);
                    }

                    if telem.state == SeestarState::Parked {
                        if ui.button(crate::i18n::tr("🚀 Avaa kaukoputki")).on_hover_text(crate::i18n::tr("Poista lepotila ohjelmallisesti (Alpaca unpark)")).clicked() {
                            client.unpark();
                        }
                    } else if ui.button(crate::i18n::tr("🏠 Parkkiin")).on_hover_text(crate::i18n::tr("Aseta lepotila ohjelmallisesti (Alpaca park). Huom: fyysinen varren taitto kasaan vaatii Seestar-sovelluksen tai virtapainikkeen")).clicked() {
                        client.park();
                    }
                });

                if telem.state == SeestarState::Stacking {
                    ui.add_space(2.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(format!("📸 Pinottu: {} kpl ({:.1} min)", telem.stacked_frames, telem.total_stack_seconds as f32 / 60.0))
                                .strong()
                                .color(Color32::from_rgb(255, 230, 100)),
                        );
                    });
                }

                ui.horizontal_wrapped(|ui| {
                    if ui.small_button(RichText::new("📂 Avaa Seestar-verkkolevy (EMMC Images)").small()).on_hover_text(crate::i18n::tr("Avaa Seestarin sisäinen muisti Macin Finderissa (smb://seestar.local/EMMC Images) mobiiliapilla tallennettujen FITS-kuvien lukua varten")).clicked() {
                        LiveIngestManager::open_seestar_smb(&state.seestar_ip);
                    }
                });

                if !telem.last_log_message.is_empty() {
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("{}: {}", crate::i18n::tr("Tila"), telem.last_log_message))
                                .small()
                                .color(Color32::from_rgb(140, 190, 240)),
                        )
                        .wrap(),
                    );
                }
            }
        });

        ui.add_space(6.0);

        // 2. LIVE FRAME AUTO-DOWNLOAD (INGEST)
        ui.group(|ui| {
            ui.set_max_width(ui.available_width());
            let mut cfg = ingest_mgr.config.lock().unwrap();
            let st = ingest_mgr.status.lock().unwrap().clone();
            let arrow = if state.section_ingest_open { "▼" } else { "▶" };

            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(RichText::new(format!("{} 📥 {}", arrow, crate::i18n::tr("Kuvien lataus tietokoneelle"))).color(Color32::from_rgb(100, 220, 255)).strong()).frame(false)).clicked() {
                    state.section_ingest_open = !state.section_ingest_open;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if st.is_active {
                        ui.label(RichText::new(format!("🟢 {} {}", st.downloaded_count, crate::i18n::tr("kpl"))).color(Color32::from_rgb(100, 255, 140)).strong().small());
                    } else {
                        ui.label(RichText::new(crate::i18n::tr("⚪ Ei yhteyttä")).color(Color32::from_rgb(150, 160, 180)).small());
                    }
                });
            });

            if state.section_ingest_open {
                ui.add_space(2.0);
                ui.checkbox(&mut cfg.enabled, crate::i18n::tr("✅ Tallenna uudet ruudut automaattisesti tietokoneelle"));

                ui.horizontal_wrapped(|ui| {
                    let dest_str = cfg.destination_path.to_string_lossy().to_string();
                    let short_dest = if dest_str.len() > 28 {
                        format!("...{}", &dest_str[dest_str.len() - 25..])
                    } else {
                        dest_str
                    };
                    ui.label(RichText::new(format!("{}: {}", crate::i18n::tr("Kohde:"), short_dest)).small().color(Color32::from_rgb(200, 210, 230)));

                    if ui.button(crate::i18n::tr("📂 Valitse kansio")).clicked() {
                        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                            cfg.destination_path = folder;
                        }
                    }
                });

                ui.horizontal_wrapped(|ui| {
                    let share_hint = telem.scope_model.smb_share_hint();
                    if telem.scope_model.brand() == "ZWO Seestar" {
                        if ui
                            .button(crate::i18n::tr("🔗 Yhdistä Seestarin verkkolevyyn (SMB)"))
                            .on_hover_text(crate::i18n::tr("Avaa Seestarin jaettu kansio suoraan tiedostoselaimessa (smb://<IP>/EMMC Images)"))
                            .clicked()
                        {
                            LiveIngestManager::open_seestar_smb(&state.seestar_ip);
                        }
                    } else {
                        ui.label(RichText::new(format!("{}: {}", crate::i18n::tr("Jako:"), share_hint)).small().color(Color32::from_rgb(170, 200, 230)));
                    }
                });

                if !st.status_message.is_empty() {
                    ui.add(egui::Label::new(RichText::new(&st.status_message).small().color(Color32::from_rgb(160, 180, 210))).wrap());
                }
            }
        });

        ui.add_space(6.0);

        // 3. SELECTED TARGET CARD & NIGHT ALTITUDE GRAPH
        ui.group(|ui| {
            ui.set_max_width(ui.available_width());
            let arrow = if state.section_target_open { "▼" } else { "▶" };
            let target_badge = state.selected_object.as_ref().map(|o| {
                let (alt, _) = ra_dec_to_alt_az(o.ra_hours, o.dec_deg, lat, lon, &now_utc);
                format!("{} (Alt: {:.0}°)", o.id, alt)
            });

            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(RichText::new(format!("{} 🌌 {}", arrow, crate::i18n::tr("Valittu kohde"))).color(Color32::from_rgb(220, 230, 255)).strong()).frame(false)).clicked() {
                    state.section_target_open = !state.section_target_open;
                }
                if let Some(tb) = target_badge {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(tb).color(Color32::from_rgb(120, 220, 255)).small());
                    });
                }
            });

            if state.section_target_open {
                if let Some(target) = &state.selected_object {
                    let (r, g, b) = target.obj_type.color_rgb();
                    ui.horizontal_wrapped(|ui| {
                        ui.heading(RichText::new(&target.id).color(Color32::from_rgb(r, g, b)).strong());
                        ui.label(RichText::new(&target.name).strong());
                    });

                    let (cur_alt, cur_az) = ra_dec_to_alt_az(target.ra_hours, target.dec_deg, lat, lon, &now_utc);
                    let alt_quality = if cur_alt > 40.0 {
                        (crate::i18n::tr("🟢 Erinomainen"), Color32::from_rgb(100, 255, 120))
                    } else if cur_alt > 20.0 {
                        (crate::i18n::tr("🟡 Hyvä"), Color32::from_rgb(255, 210, 80))
                    } else if cur_alt > 0.0 {
                        (crate::i18n::tr("🟠 Matala (Himmeä)"), Color32::from_rgb(255, 140, 60))
                    } else {
                        (crate::i18n::tr("🔴 Horisontin alla"), Color32::from_rgb(255, 80, 80))
                    };

                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!("{}: {} {}", crate::i18n::tr("Tyyppi:"), target.obj_type.icon(), crate::i18n::tr(target.obj_type.name_fi())));
                        ui.separator();
                        ui.label(format!("Mag: {:.1}", target.magnitude));
                    });

                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!("{}: {:.1}°", crate::i18n::tr("Korkeus (Alt):"), cur_alt));
                        ui.colored_label(alt_quality.1, alt_quality.0);
                    });

                    ui.add(egui::Label::new(format!("{}: {:.1}° | {}: {}", crate::i18n::tr("Suunta (Atsimuutti):"), cur_az, crate::i18n::tr("Tähdistö:"), target.constellation)).wrap());
                    ui.add(egui::Label::new(RichText::new(format!("{}: {}", crate::i18n::tr("Suodatin:"), target.filter_recommendation)).small().color(Color32::from_rgb(140, 220, 255))).wrap());

                    ui.add_space(4.0);

                    // 24h Night Altitude Plot
                    ui.label(RichText::new(crate::i18n::tr("📈 Korkeuskäyrä tänä yönä (klo 18 - 06):")).small().strong());
                    let profile = night_altitude_profile(target.ra_hours, target.dec_deg, lat, lon, &now_utc);
                    Self::draw_altitude_profile_widget(ui, &profile);

                    ui.add_space(4.0);

                    // Add to Sequencer Row
                    ui.horizontal_wrapped(|ui| {
                        ui.label(crate::i18n::tr("Kesto:"));
                        ui.add(egui::DragValue::new(&mut state.sequence_duration_mins).range(5..=360).suffix(" min"));
                        ui.label(crate::i18n::tr("Valotus:"));
                        for &sec in &[10, 20, 30, 60] {
                            let is_sel = state.sequence_sub_exposure_secs == sec;
                            if ui.add(egui::SelectableLabel::new(is_sel, format!("{}s", sec))).clicked() {
                                state.sequence_sub_exposure_secs = sec;
                            }
                        }
                        ui.add(egui::DragValue::new(&mut state.sequence_sub_exposure_secs).range(1..=300).suffix("s"));
                        ui.checkbox(&mut state.sequence_use_lp, crate::i18n::tr("LP-suodatin"));

                        if ui.button(crate::i18n::tr("➕ Lisää sekvensseriin")).clicked() {
                            sequencer.add_target_with_exposure(target, state.sequence_duration_mins, state.sequence_sub_exposure_secs, state.sequence_use_lp);
                        }
                        if ui.button(crate::i18n::tr("🌑 + Dark-sarja")).on_hover_text(crate::i18n::tr("Lisää pimeiden kalibrointikuvien sarja sekvenssiin")).clicked() {
                            sequencer.add_dark_calibration_sequence(state.sequence_sub_exposure_secs, 20);
                        }
                    });
                } else {
                    ui.add(egui::Label::new(RichText::new(crate::i18n::tr("Klikkaa kohdetta tähtikartalta tai valitse alta haku.")).color(Color32::from_rgb(150, 160, 180))).wrap());
                }
            }
        });

        ui.add_space(6.0);

        // 4. TARGET SEARCH & CATALOG LIST
        ui.group(|ui| {
            ui.set_max_width(ui.available_width());
            let arrow = if state.section_search_open { "▼" } else { "▶" };

            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(RichText::new(format!("{} 🔍 {}", arrow, crate::i18n::tr("Etsi taivaankappaletta"))).color(Color32::from_rgb(180, 210, 240)).strong()).frame(false)).clicked() {
                    state.section_search_open = !state.section_search_open;
                }
            });

            if state.section_search_open {
                ui.horizontal(|ui| {
                    let text_w = (ui.available_width() - 36.0).max(120.0);
                    ui.add(egui::TextEdit::singleline(&mut state.search_query).hint_text("M45, Orion, Andromeda, NGC 7000...").desired_width(text_w));
                    if !state.search_query.is_empty() && ui.button("✖").clicked() {
                        state.search_query.clear();
                    }
                });

                // Filter pills
                ui.horizontal_wrapped(|ui| {
                    if ui.selectable_label(state.filter_type.is_none(), crate::i18n::tr("Kaikki")).clicked() {
                        state.filter_type = None;
                    }
                    if ui.selectable_label(state.filter_type == Some(DsoType::EmissionNebula), crate::i18n::tr("🔴 Sumut")).clicked() {
                        state.filter_type = Some(DsoType::EmissionNebula);
                    }
                    if ui.selectable_label(state.filter_type == Some(DsoType::Galaxy), crate::i18n::tr("🌀 Galaksit")).clicked() {
                        state.filter_type = Some(DsoType::Galaxy);
                    }
                    if ui.selectable_label(state.filter_type == Some(DsoType::ReflectionNebula), crate::i18n::tr("🔵 Plejadit")).clicked() {
                        state.filter_type = Some(DsoType::ReflectionNebula);
                    }
                    if ui.selectable_label(state.filter_type == Some(DsoType::OpenCluster), crate::i18n::tr("✨ Joukot")).clicked() {
                        state.filter_type = Some(DsoType::OpenCluster);
                    }
                });

                ui.add_space(4.0);

                // Filtered results list
                let q = state.search_query.to_lowercase();
                let matches: Vec<&SkyObject> = catalog
                    .iter()
                    .filter(|obj| {
                        if let Some(ft) = state.filter_type {
                            if obj.obj_type != ft {
                                return false;
                            }
                        }
                        if q.is_empty() {
                            true
                        } else {
                            obj.id.to_lowercase().contains(&q)
                                || obj.name.to_lowercase().contains(&q)
                                || obj.constellation.to_lowercase().contains(&q)
                        }
                    })
                    .take(15)
                    .collect();

                egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                    for obj in matches {
                        let (alt, _) = ra_dec_to_alt_az(obj.ra_hours, obj.dec_deg, lat, lon, &now_utc);
                        let is_sel = state.selected_object.as_ref().map(|o| o.id == obj.id).unwrap_or(false);
                        let (r, g, b) = obj.obj_type.color_rgb();
                        let text = format!("{} {} – {} (Alt: {:.0}°)", obj.obj_type.icon(), obj.id, obj.name, alt);

                        let item = egui::SelectableLabel::new(
                            is_sel,
                            RichText::new(text).color(Color32::from_rgb(r, g, b)).size(12.0),
                        );
                        if ui.add(item).clicked() {
                            state.selected_object = Some(obj.clone());
                            state.recompute_mosaic();
                        }
                    }
                });
            }
        });

        ui.add_space(6.0);

        // 5. MOSAIKKIKUVAUS (MOSAIC PLANNER)
        ui.group(|ui| {
            ui.set_max_width(ui.available_width());
            let arrow = if state.section_mosaic_open { "▼" } else { "▶" };
            let panel_summary = format!("{} {} ({:.1} deg²)", state.mosaic_plan.panels.len(), crate::i18n::tr("ruutua"), state.mosaic_plan.total_area_sq_deg);

            ui.horizontal_wrapped(|ui| {
                if ui.add(egui::Button::new(RichText::new(format!("{} 🧩 {}", arrow, crate::i18n::tr("Mosaiikkikuvaus"))).color(Color32::from_rgb(120, 220, 200)).strong()).frame(false)).clicked() {
                    state.section_mosaic_open = !state.section_mosaic_open;
                }
                let was_enabled = state.mosaic_enabled;
                if ui.checkbox(&mut state.mosaic_enabled, crate::i18n::tr("Käytössä")).changed() {
                    if state.mosaic_enabled && !was_enabled {
                        state.recompute_mosaic();
                        state.section_mosaic_open = true;
                    }
                }
                if !state.section_mosaic_open {
                    ui.label(RichText::new(panel_summary).color(Color32::from_rgb(130, 240, 200)).small());
                }
            });

            if state.section_mosaic_open && state.mosaic_enabled {
                ui.add_space(2.0);

                // Scope model selector
                ui.horizontal_wrapped(|ui| {
                    ui.label(crate::i18n::tr("Kaukoputki:"));
                    let prev_model = state.mosaic_config.scope_model;
                    let combo_w = (ui.available_width() - 12.0).clamp(160.0, 280.0);
                    egui::ComboBox::from_id_salt("mosaic_scope_model_combo")
                        .width(combo_w)
                        .selected_text(state.mosaic_config.scope_model.display_short())
                        .show_ui(ui, |ui| {
                            ui.label(RichText::new("── ZWO Seestar ──").small().color(Color32::from_rgb(100, 200, 255)));
                            ui.selectable_value(&mut state.mosaic_config.scope_model, MosaicScopeModel::SeestarS30, "ZWO Seestar S-30 (150mm f/5, 2.1° × 1.2°)");
                            ui.selectable_value(&mut state.mosaic_config.scope_model, MosaicScopeModel::SeestarS30Pro, "ZWO Seestar S-30 Pro (160mm f/5.3 4K, 4.0° × 2.2°)");
                            ui.selectable_value(&mut state.mosaic_config.scope_model, MosaicScopeModel::SeestarS50, "ZWO Seestar S-50 (250mm f/5, 1.3° × 0.7°)");
                            ui.selectable_value(&mut state.mosaic_config.scope_model, MosaicScopeModel::SeestarS50Pro, "ZWO Seestar S-50 Pro (260mm f/5.2 4K, 2.5° × 1.4°)");

                            ui.separator();
                            ui.label(RichText::new("── DwarfLab (Avoin API & Alpaca) ──").small().color(Color32::from_rgb(255, 180, 100)));
                            ui.selectable_value(&mut state.mosaic_config.scope_model, MosaicScopeModel::Dwarf3, "DwarfLab DWARF 3 (150mm f/4.3 4K, 2.9° × 1.7°)");
                            ui.selectable_value(&mut state.mosaic_config.scope_model, MosaicScopeModel::DwarfMini, "DwarfLab DWARF Mini (150mm f/5, 2.1° × 1.2°)");
                            ui.selectable_value(&mut state.mosaic_config.scope_model, MosaicScopeModel::Dwarf2, "DwarfLab DWARF II (100mm f/4.2 4K, 3.2° × 1.8°)");
                            ui.selectable_value(&mut state.mosaic_config.scope_model, MosaicScopeModel::DwarfDraco, "DwarfLab DRACO (340mm f/3.8, 90mm aukko, 1.6° × 1.2°)");

                            ui.separator();
                            ui.label(RichText::new("── Celestron ──").small().color(Color32::from_rgb(255, 220, 120)));
                            ui.selectable_value(&mut state.mosaic_config.scope_model, MosaicScopeModel::CelestronOrigin, "Celestron Origin RASA (335mm f/2.2, 1.3° × 0.8°)");

                            ui.separator();
                            ui.label(RichText::new("── Universaali Alpaca / Oma ──").small().color(Color32::from_rgb(180, 220, 180)));
                            ui.selectable_value(&mut state.mosaic_config.scope_model, MosaicScopeModel::AscomAlpaca, "ASCOM Alpaca (Universal REST Mount)");
                            ui.selectable_value(&mut state.mosaic_config.scope_model, MosaicScopeModel::Custom, crate::i18n::tr("Oma määritys"));
                        });
                    if prev_model != state.mosaic_config.scope_model {
                        let new_model = state.mosaic_config.scope_model;
                        client.set_model(new_model);
                        let mut cfg = TelescopeConfig::load();
                        let target_ip = cfg.get_ip_for_model(new_model);
                        state.seestar_ip = target_ip.clone();
                        cfg.last_ip = target_ip.clone();
                        cfg.scope_model = Some(new_model.display_short().to_string());
                        cfg.model_ips.insert(new_model.display_short().to_string(), target_ip);
                        cfg.save();

                        ingest_mgr.set_model_source_path(new_model);
                        state.recompute_mosaic();
                    }
                });

                if state.mosaic_config.scope_model == MosaicScopeModel::Custom {
                    ui.group(|ui| {
                        ui.set_max_width(ui.available_width());
                        ui.label(RichText::new(crate::i18n::tr("Oman kaukoputken optiikka ja kenno:")).strong().small());
                        let mut custom_changed = false;
                        ui.horizontal_wrapped(|ui| {
                            custom_changed |= ui.add(egui::DragValue::new(&mut state.mosaic_config.custom_fov_w_deg).range(0.1..=20.0).speed(0.05).prefix("FOV L: ").suffix("°")).changed();
                            custom_changed |= ui.add(egui::DragValue::new(&mut state.mosaic_config.custom_fov_h_deg).range(0.1..=20.0).speed(0.05).prefix("FOV K: ").suffix("°")).changed();
                        });
                        ui.horizontal_wrapped(|ui| {
                            ui.add(egui::DragValue::new(&mut state.mosaic_config.custom_focal_length_mm).range(10.0..=5000.0).speed(5.0).prefix("Polttoväli: ").suffix(" mm"));
                            ui.add(egui::DragValue::new(&mut state.mosaic_config.custom_sensor_w_mm).range(1.0..=100.0).speed(0.1).prefix("Kenno L: ").suffix(" mm"));
                            ui.add(egui::DragValue::new(&mut state.mosaic_config.custom_sensor_h_mm).range(1.0..=100.0).speed(0.1).prefix("Kenno K: ").suffix(" mm"));
                            if ui.small_button(crate::i18n::tr("📐 Laske FOV")).clicked() {
                                let (w, h) = MosaicGridConfig::compute_custom_fov(
                                    state.mosaic_config.custom_focal_length_mm,
                                    state.mosaic_config.custom_sensor_w_mm,
                                    state.mosaic_config.custom_sensor_h_mm,
                                );
                                state.mosaic_config.custom_fov_w_deg = w;
                                state.mosaic_config.custom_fov_h_deg = h;
                                custom_changed = true;
                            }
                        });
                        if custom_changed {
                            state.recompute_mosaic();
                        }
                    });
                }

                // Target center info
                let center_label = state.selected_object.as_ref().map(|o| format!("{} ({})", o.id, o.name)).unwrap_or_else(|| crate::i18n::tr("Ei valittua kohdetta (valitse yltä)").to_string());
                ui.add(egui::Label::new(RichText::new(format!("{}: {}", crate::i18n::tr("Keskipiste:"), center_label)).small().color(Color32::from_rgb(160, 210, 240))).wrap());

                // Quick Presets
                ui.horizontal_wrapped(|ui| {
                    ui.label(crate::i18n::tr("Pikamallit:"));
                    if ui.small_button("2×1").on_hover_text(crate::i18n::tr("2×1 Vaaka (2 ruutua)")).clicked() {
                        state.mosaic_config.cols = 2;
                        state.mosaic_config.rows = 1;
                        state.recompute_mosaic();
                    }
                    if ui.small_button("1×2").on_hover_text(crate::i18n::tr("1×2 Pysty (2 ruutua)")).clicked() {
                        state.mosaic_config.cols = 1;
                        state.mosaic_config.rows = 2;
                        state.recompute_mosaic();
                    }
                    if ui.small_button("2×2").on_hover_text(crate::i18n::tr("2×2 Neliö (4 ruutua)")).clicked() {
                        state.mosaic_config.cols = 2;
                        state.mosaic_config.rows = 2;
                        state.recompute_mosaic();
                    }
                    if ui.small_button("3×2").on_hover_text(crate::i18n::tr("3×2 Laaja (6 ruutua)")).clicked() {
                        state.mosaic_config.cols = 3;
                        state.mosaic_config.rows = 2;
                        state.recompute_mosaic();
                    }
                    if ui.small_button("3×3").on_hover_text(crate::i18n::tr("3×3 Jätti (9 ruutua)")).clicked() {
                        state.mosaic_config.cols = 3;
                        state.mosaic_config.rows = 3;
                        state.recompute_mosaic();
                    }
                });

                ui.add_space(2.0);

                let mut grid_changed = false;
                ui.horizontal_wrapped(|ui| {
                    ui.label(crate::i18n::tr("Ruudukko:"));
                    grid_changed |= ui.add(egui::DragValue::new(&mut state.mosaic_config.cols).range(1..=5).prefix(crate::i18n::tr("Sarakkeet: "))).changed();
                    grid_changed |= ui.add(egui::DragValue::new(&mut state.mosaic_config.rows).range(1..=5).prefix(crate::i18n::tr("Rivit: "))).changed();
                });

                ui.horizontal_wrapped(|ui| {
                    ui.label(crate::i18n::tr("Limitys (Overlap):"));
                    grid_changed |= ui.add(egui::Slider::new(&mut state.mosaic_config.overlap_pct, 10.0..=40.0).suffix("%")).changed();
                });

                ui.horizontal_wrapped(|ui| {
                    ui.label(crate::i18n::tr("Valotus / ruutu:"));
                    ui.add(egui::DragValue::new(&mut state.mosaic_config.exposure_time_per_panel_mins).range(5..=180).suffix(" min"));
                    ui.checkbox(&mut state.mosaic_config.use_lp_filter, "LP");
                });

                if grid_changed {
                    state.recompute_mosaic();
                }

                let plan = &state.mosaic_plan;
                ui.add(egui::Label::new(RichText::new(format!(
                    "📐 {}: {:.2}° × {:.2}° ({:.1} deg²) | {}: {} {}",
                    crate::i18n::tr("Kokonaisala:"), plan.total_fov_w_deg, plan.total_fov_h_deg, plan.total_area_sq_deg, crate::i18n::tr("Ruutuja:"), plan.panels.len(), crate::i18n::tr("kpl")
                )).small().color(Color32::from_rgb(130, 240, 200))).wrap());

                ui.add(egui::Label::new(RichText::new(format!(
                    "⏱ {}: {} h {:02} min",
                    crate::i18n::tr("Kokonaisvalotusaika yöllä:"), plan.total_exposure_mins / 60, plan.total_exposure_mins % 60
                )).small().color(Color32::from_rgb(255, 220, 140))).wrap());

                ui.add_space(2.0);

                if ui.add_sized([ui.available_width(), 26.0], egui::Button::new(RichText::new(crate::i18n::tr("📋 Lisää mosaiikin ruudut yösekvenssiin")).strong().color(Color32::from_rgb(100, 255, 200)))).clicked() {
                    let targets = state.mosaic_plan.to_sequence_targets();
                    for t in targets {
                        sequencer.add_sequence_target(t);
                    }
                }

                ui.add_space(2.0);
                if ui.add_sized([ui.available_width(), 26.0], egui::Button::new(RichText::new(crate::i18n::tr("🧩 Avaa mosaiikkipaneelien yhdistäjä")).strong().color(Color32::from_rgb(130, 210, 255))))
                    .on_hover_text(crate::i18n::tr("Siirry Pinoaminen -> Mosaiikit -välilehdelle yhdistämään otettuja ruutuja yhtenäiseksi kuvaksi"))
                    .clicked()
                {
                    state.request_mosaic_stitcher = true;
                }
            } else if state.section_mosaic_open && !state.mosaic_enabled {
                ui.add(egui::Label::new(RichText::new(crate::i18n::tr("Kytke mosaiikkikuvaus päälle rastittamalla 'Käytössä' yltä.")).small().color(Color32::from_rgb(150, 160, 180))).wrap());
            }
        });

        ui.add_space(6.0);

        // 6. NIGHT SEQUENCER (N.I.N.A. SCHEDULER)
        ui.group(|ui| {
            ui.set_max_width(ui.available_width());
            let seq_state = sequencer.state.lock().unwrap().clone();
            let arrow = if state.section_sequencer_open { "▼" } else { "▶" };

            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(RichText::new(format!("{} 📅 {}", arrow, crate::i18n::tr("Yösekvensseri"))).color(Color32::from_rgb(255, 200, 80)).strong()).frame(false)).clicked() {
                    state.section_sequencer_open = !state.section_sequencer_open;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button(crate::i18n::tr("⛶ Päänäkymä")).on_hover_text(crate::i18n::tr("Avaa koko yösekvensserin iso hallintapöytä päänäkymään")).clicked() {
                        state.center_view = TelescopeCenterView::Sequencer;
                    }
                    if seq_state.is_running {
                        ui.label(RichText::new(crate::i18n::tr("● KÄYNNISSÄ")).color(Color32::from_rgb(100, 255, 120)).strong().small());
                    } else {
                        ui.label(RichText::new(crate::i18n::tr("● Valmiustila")).color(Color32::from_rgb(150, 160, 180)).small());
                    }
                });
            });

            if state.section_sequencer_open {
                ui.add_space(4.0);

                // Controls
                ui.horizontal_wrapped(|ui| {
                    if seq_state.is_running {
                        if ui.button(RichText::new(crate::i18n::tr("⏹ Keskeytä yöajo")).color(Color32::from_rgb(255, 100, 100)).strong()).clicked() {
                            sequencer.stop();
                        }
                    } else if ui.button(RichText::new(crate::i18n::tr("▶ KÄYNNISTÄ YÖAJO")).color(Color32::from_rgb(100, 255, 150)).strong()).clicked() {
                        sequencer.start(Arc::clone(client));
                    }

                    if ui.button(crate::i18n::tr("🗑 Tyhjennä lista")).clicked() {
                        sequencer.clear_all();
                    }
                });

                ui.add(egui::Label::new(RichText::new(&seq_state.current_step_description).strong().size(12.0).color(Color32::from_rgb(255, 230, 120))).wrap());

                let mut auto_park = seq_state.auto_park_at_end;
                if ui.checkbox(&mut auto_park, crate::i18n::tr("🏠 Parkkeeraa aamulla automaattisesti")).changed() {
                    sequencer.state.lock().unwrap().auto_park_at_end = auto_park;
                }

                // Night Automation Settings (Auto-Focus, Dithering & Connection Guard)
                ui.collapsing(
                    RichText::new(crate::i18n::tr("⚙ Yöajon automatiikka"))
                        .small()
                        .strong()
                        .color(Color32::from_rgb(150, 220, 255)),
                    |ui| {
                        let mut af_cfg = seq_state.autofocus_config;
                        let mut dither_cfg = seq_state.dither_config;
                        let mut conn_guard = seq_state.connection_guard;
                        let mut af_changed = false;
                        let mut dither_changed = false;
                        let mut guard_changed = false;

                        // 1. Auto-Focus
                        af_changed |= ui
                            .checkbox(&mut af_cfg.enabled, crate::i18n::tr("🔍 Automaattitarkennus yöajossa"))
                            .on_hover_text(crate::i18n::tr("Suorittaa automaattisen tarkennuksen säännöllisesti ja lämpötilan laskiessa"))
                            .changed();
                        if af_cfg.enabled {
                            ui.indent("sidebar_af_indent", |ui| {
                                af_changed |= ui.checkbox(&mut af_cfg.on_target_start, crate::i18n::tr("Kohteen vaihtuessa")).changed();
                                ui.horizontal(|ui| {
                                    af_changed |= ui.checkbox(&mut af_cfg.use_time_trigger, crate::i18n::tr("Aikaväli:")).changed();
                                    af_changed |= ui
                                        .add(egui::DragValue::new(&mut af_cfg.interval_minutes).range(15..=240).suffix(" min"))
                                        .changed();
                                });
                                ui.horizontal(|ui| {
                                    af_changed |= ui.checkbox(&mut af_cfg.use_temp_trigger, crate::i18n::tr("Δ Lämpötila:")).changed();
                                    af_changed |= ui
                                        .add(
                                            egui::DragValue::new(&mut af_cfg.temp_delta_c)
                                                .range(0.5..=5.0)
                                                .speed(0.1)
                                                .suffix(" °C"),
                                        )
                                        .changed();
                                });
                            });
                        }

                        ui.add_space(2.0);

                        // 2. Dithering
                        dither_changed |= ui
                            .checkbox(&mut dither_cfg.enabled, crate::i18n::tr("🔀 Ditheröinti"))
                            .on_hover_text(crate::i18n::tr("Siirtää kuvakenttää muutaman pikselin valotusten välillä kuumien pikselien ja juovien poistamiseksi"))
                            .changed();
                        if dither_cfg.enabled {
                            ui.indent("sidebar_dither_indent", |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(crate::i18n::tr("Väli:"));
                                    dither_changed |= ui
                                        .add(egui::DragValue::new(&mut dither_cfg.interval_frames).range(1..=20).prefix(crate::i18n::tr("joka ")).suffix(crate::i18n::tr(". kuva")))
                                        .changed();
                                    ui.label(crate::i18n::tr("Siirto:"));
                                    dither_changed |= ui
                                        .add(egui::DragValue::new(&mut dither_cfg.step_pixels).range(1..=30).suffix(" px"))
                                        .changed();
                                });
                                ui.horizontal(|ui| {
                                    ui.label(crate::i18n::tr("Asettumisaika:"));
                                    dither_changed |= ui
                                        .add(egui::DragValue::new(&mut dither_cfg.settle_seconds).range(1..=15).suffix(" s"))
                                        .changed();
                                });
                            });
                        }

                        ui.add_space(2.0);

                        // 3. Connection Guard
                        guard_changed |= ui
                            .checkbox(&mut conn_guard.auto_reconnect, crate::i18n::tr("🛡 Yhteyskatkoksen sieto"))
                            .on_hover_text(crate::i18n::tr("Pitää yösekvenssin hengissä Wi-Fi-katkosten yli ja täsmäyttää tilan yhteyden palautuessa"))
                            .changed();
                        if conn_guard.auto_reconnect {
                            ui.indent("sidebar_guard_indent", |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(crate::i18n::tr("Armonaika:"));
                                    guard_changed |= ui
                                        .add(egui::DragValue::new(&mut conn_guard.grace_period_minutes).range(1..=60).suffix(" min"))
                                        .changed();
                                });
                                guard_changed |= ui
                                    .checkbox(&mut conn_guard.resume_stacking_after_reconnect, crate::i18n::tr("Jatka pinoamista automaattisesti"))
                                    .changed();
                            });
                        }

                        if af_changed || dither_changed || guard_changed {
                            let mut s = sequencer.state.lock().unwrap();
                            s.autofocus_config = af_cfg;
                            s.dither_config = dither_cfg;
                            s.connection_guard = conn_guard;
                        }
                        if dither_changed {
                            client.configure_dither(dither_cfg);
                        }
                        if guard_changed {
                            client.set_auto_reconnect(conn_guard.auto_reconnect);
                        }
                    },
                );

                ui.add_space(4.0);

                // N.I.N.A. Quick-Add Step Bar
                ui.horizontal_wrapped(|ui| {
                    if ui.small_button(crate::i18n::tr("+ 🎯 Ajo")).on_hover_text(crate::i18n::tr("Lisää kääntyminen kohteeseen")).clicked() {
                        let (name, ra, dec) = if let Some(t) = &state.selected_object {
                            (t.name.clone(), t.ra_hours, t.dec_deg)
                        } else {
                            ("Uusi kohde".to_string(), 0.0, 0.0)
                        };
                        sequencer.add_item(SequenceStep::SlewToTarget {
                            name,
                            ra_hours: ra,
                            dec_deg: dec,
                            solve_and_center: true,
                            min_altitude_deg: 20.0,
                        });
                    }
                    if ui.small_button(crate::i18n::tr("+ 📸 Valotus")).on_hover_text(crate::i18n::tr("Lisää valotussarja (Light/Dark/Flat/Bias)")).clicked() {
                        sequencer.add_item(SequenceStep::TakeExposures {
                            frame_type: FrameType::Light,
                            exposure_secs: state.sequence_sub_exposure_secs,
                            count: 30,
                            dither_every_n: Some(3),
                        });
                    }
                    if ui.small_button(crate::i18n::tr("+ 🔍 AF")).on_hover_text(crate::i18n::tr("Lisää automaattitarkennus")).clicked() {
                        sequencer.add_item(SequenceStep::AutoFocus {
                            reason: "Tarkennus".to_string(),
                        });
                    }
                    if ui.small_button(crate::i18n::tr("+ 🛡 Suodin")).on_hover_text(crate::i18n::tr("Lisää suotimen asetus")).clicked() {
                        sequencer.add_item(SequenceStep::SetFilter {
                            filter: FilterChoice::LightPollution,
                        });
                    }
                    if ui.small_button(crate::i18n::tr("+ 🌑 Dark")).on_hover_text(crate::i18n::tr("Lisää mustien kalibrointikuvien sarja (20 × Dark)")).clicked() {
                        sequencer.add_dark_calibration_sequence(state.sequence_sub_exposure_secs, 20);
                    }
                    if ui.small_button(crate::i18n::tr("+ ⏳ Odotus")).on_hover_text(crate::i18n::tr("Lisää odotusaika (tauko)")).clicked() {
                        sequencer.add_item(SequenceStep::WaitSeconds {
                            seconds: 60,
                            reason: "Tauko".to_string(),
                        });
                    }
                    if ui.small_button(crate::i18n::tr("+ 🏠 Park")).on_hover_text(crate::i18n::tr("Lisää ajo lepoasentoon")).clicked() {
                        sequencer.add_item(SequenceStep::Park);
                    }
                });

                ui.add_space(4.0);

                // N.I.N.A. Instruction List
                if seq_state.items.is_empty() {
                    ui.add(egui::Label::new(RichText::new(crate::i18n::tr("Kuvauslista on tyhjä. Lisää toimintoja yltä tai valitse kohde tähtikartalta.")).small().color(Color32::from_rgb(140, 150, 170))).wrap());
                } else {
                    for (idx, item) in seq_state.items.iter().enumerate() {
                        let is_active = seq_state.current_item_idx == Some(idx) && seq_state.is_running;
                        ui.group(|ui| {
                            ui.set_max_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let mut en = item.enabled;
                                if ui.checkbox(&mut en, "").on_hover_text(crate::i18n::tr("Kytke toiminto päälle/pois")).changed() {
                                    sequencer.toggle_item(idx);
                                }

                                let title_col = if is_active {
                                    Color32::from_rgb(255, 230, 80)
                                } else if item.status == JobStatus::Completed {
                                    Color32::from_rgb(100, 230, 120)
                                } else if !item.enabled {
                                    Color32::from_rgb(130, 140, 155)
                                } else {
                                    Color32::from_rgb(200, 220, 245)
                                };

                                let prefix = if is_active { "▶ " } else { "" };
                                let label_str = format!("{}{}. {} {}", prefix, idx + 1, item.icon(), item.title());

                                let status_col = match item.status {
                                    JobStatus::Completed => Color32::from_rgb(100, 255, 120),
                                    JobStatus::Stacking => Color32::from_rgb(80, 200, 255),
                                    JobStatus::Focusing => Color32::from_rgb(255, 225, 90),
                                    JobStatus::Dithering => Color32::from_rgb(190, 140, 255),
                                    JobStatus::WaitingConnection => Color32::from_rgb(255, 160, 60),
                                    JobStatus::Slewing | JobStatus::Solving => Color32::from_rgb(255, 210, 80),
                                    JobStatus::Failed => Color32::from_rgb(255, 100, 100),
                                    JobStatus::Skipped => Color32::from_rgb(150, 160, 180),
                                    JobStatus::Pending => Color32::from_rgb(160, 175, 200),
                                };

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.small_button("✖").on_hover_text(crate::i18n::tr("Poista toiminto")).clicked() {
                                        sequencer.remove_item(idx);
                                    }
                                    if ui.small_button("⬇").on_hover_text(crate::i18n::tr("Siirrä alemmas")).clicked() {
                                        sequencer.move_item_down(idx);
                                    }
                                    if ui.small_button("⬆").on_hover_text(crate::i18n::tr("Siirrä ylemmäs")).clicked() {
                                        sequencer.move_item_up(idx);
                                    }

                                    ui.label(RichText::new(crate::i18n::tr(item.status.display_fi())).small().color(status_col));

                                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        ui.add(egui::Label::new(RichText::new(&label_str).strong().small().color(title_col)).truncate())
                                            .on_hover_text(&label_str);
                                    });
                                });
                            });

                            // Sub-row: Progress if active
                            if is_active && !item.progress_str.is_empty() {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(format!("⚡ {}", item.progress_str)).small().color(Color32::from_rgb(255, 220, 120)));
                                });
                            }
                        });
                        ui.add_space(2.0);
                    }
                }
            }
        });

        ui.add_space(24.0);
    }

    /// Renders the Full-Screen Night Sequencer Dashboard in the Central Panel
    pub fn ui_sequencer_center_view(
        ui: &mut egui::Ui,
        state: &mut TelescopeViewState,
        sequencer: &Arc<NightSequencer>,
        client: &Arc<SeestarClient>,
        lat: f64,
        lon: f64,
    ) {
        let seq_state = sequencer.state.lock().unwrap().clone();
        let telem = client.get_telemetry();
        let now_utc = Utc::now();
        ui.set_max_width(ui.available_width());

        egui::ScrollArea::vertical()
            .id_salt("sequencer_center_scroll")
            .show(ui, |ui| {
                ui.add_space(8.0);

                // Top Header Bar
                ui.horizontal(|ui| {
                    ui.heading(RichText::new(crate::i18n::tr("📅 Yösekvensseri")).color(Color32::from_rgb(255, 200, 80)).strong());
                    ui.separator();
                    if seq_state.is_running {
                        ui.label(RichText::new(crate::i18n::tr("● KÄYNNISSÄ")).color(Color32::from_rgb(80, 255, 120)).strong());
                    } else {
                        ui.label(RichText::new(crate::i18n::tr("● VALMIUSTILA")).color(Color32::from_rgb(150, 160, 180)).strong());
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if seq_state.is_running {
                            if ui.button(RichText::new(crate::i18n::tr("⏹ Keskeytä yöajo")).color(Color32::from_rgb(255, 100, 100)).strong()).clicked() {
                                sequencer.stop();
                            }
                        } else if ui.button(RichText::new(crate::i18n::tr("▶ KÄYNNISTÄ YÖAJO")).color(Color32::from_rgb(100, 255, 150)).strong()).clicked() {
                            sequencer.start(Arc::clone(client));
                        }

                        if ui.button(crate::i18n::tr("🗑 Tyhjennä kaikki")).clicked() {
                            sequencer.clear_all();
                        }
                    });
                });

                ui.separator();

                // Telemetry & Status bar
                ui.group(|ui| {
                    ui.horizontal_wrapped(|ui| {
                        let (sr, sg, sb) = telem.state.color_rgb();
                        ui.label(RichText::new(format!("{}: {}", crate::i18n::tr("Kaukoputki:"), crate::i18n::tr(telem.state.display_fi()))).color(Color32::from_rgb(sr, sg, sb)).strong());
                        ui.separator();
                        ui.label(format!("{}: {}%", crate::i18n::tr("Akku:"), telem.battery_pct));
                        ui.separator();
                        ui.label(format!("{}: {:.1} °C", crate::i18n::tr("Lämpötila:"), telem.temp_c));
                        ui.separator();
                        ui.label(format!("{}: {} (AF: {} {})", crate::i18n::tr("Tarkennus:"), telem.focus_position, seq_state.af_runs_count, crate::i18n::tr("krt")));
                        ui.separator();
                        ui.label(format!("{}: {} {}", crate::i18n::tr("Ditheröinnit:"), seq_state.dithers_count, crate::i18n::tr("kpl")));
                        ui.separator();
                        let total_secs: u32 = seq_state.items.iter().filter(|it| it.enabled).map(|it| it.estimated_duration_secs()).sum();
                        ui.label(RichText::new(format!("{}: {} h {:02} min ({} {})", crate::i18n::tr("Kokonaisaika:"), total_secs / 3600, (total_secs % 3600) / 60, seq_state.items.len(), crate::i18n::tr("toimintoa"))).color(Color32::from_rgb(255, 220, 120)));
                    });

                    ui.horizontal_wrapped(|ui| {
                        ui.add(egui::Label::new(RichText::new(&seq_state.current_step_description).strong().color(Color32::from_rgb(255, 230, 140))).wrap());
                        let mut auto_park = seq_state.auto_park_at_end;
                        if ui.checkbox(&mut auto_park, crate::i18n::tr("🏠 Parkkeeraa aamulla automaattisesti")).changed() {
                            sequencer.state.lock().unwrap().auto_park_at_end = auto_park;
                        }
                    });
                });

                ui.add_space(6.0);

                // Night Automation & Resilience Panel (3-column layout)
                ui.group(|ui| {
                    ui.set_max_width(ui.available_width());
                    ui.label(
                        RichText::new(crate::i18n::tr("⚙ Yöajon automatiikka ja vikasietoisuus"))
                            .strong()
                            .color(Color32::from_rgb(140, 215, 255)),
                    );
                    ui.add_space(4.0);

                    let mut af_cfg = seq_state.autofocus_config;
                    let mut dither_cfg = seq_state.dither_config;
                    let mut conn_guard = seq_state.connection_guard;
                    let mut af_changed = false;
                    let mut dither_changed = false;
                    let mut guard_changed = false;

                    ui.columns(3, |cols| {
                        // Column 1: Auto-Focus
                        cols[0].group(|ui| {
                            ui.set_max_width(ui.available_width());
                            af_changed |= ui
                                .checkbox(
                                    &mut af_cfg.enabled,
                                    RichText::new(crate::i18n::tr("🔍 Automaattinen tarkennus")).strong().color(Color32::from_rgb(255, 225, 110)),
                                )
                                .changed();
                            af_changed |= ui.checkbox(&mut af_cfg.on_target_start, crate::i18n::tr("Tarkenna aina uuden kohteen alussa")).changed();
                            ui.horizontal(|ui| {
                                af_changed |= ui.checkbox(&mut af_cfg.use_time_trigger, crate::i18n::tr("Aikaväli:")).changed();
                                af_changed |= ui
                                    .add(egui::DragValue::new(&mut af_cfg.interval_minutes).range(15..=240).suffix(" min"))
                                    .changed();
                            });
                            ui.horizontal(|ui| {
                                af_changed |= ui.checkbox(&mut af_cfg.use_temp_trigger, crate::i18n::tr("Lämpötilamuutos (ΔT):")).changed();
                                af_changed |= ui
                                    .add(
                                        egui::DragValue::new(&mut af_cfg.temp_delta_c)
                                            .range(0.5..=5.0)
                                            .speed(0.1)
                                            .suffix(" °C"),
                                    )
                                    .changed();
                            });
                            let af_info = if let Some(last_t) = seq_state.last_af_temp_c {
                                format!("{}: {:.1} °C ({} {})", crate::i18n::tr("Viimeisin AF:"), last_t, seq_state.minutes_since_last_af, crate::i18n::tr("min sitten"))
                            } else {
                                crate::i18n::tr("Viimeisin AF: Ei vielä suoritettu yöajossa").to_string()
                            };
                            ui.label(RichText::new(af_info).small().color(Color32::from_rgb(160, 180, 205)));
                        });

                        // Column 2: Dithering
                        cols[1].group(|ui| {
                            ui.set_max_width(ui.available_width());
                            dither_changed |= ui
                                .checkbox(
                                    &mut dither_cfg.enabled,
                                    RichText::new(crate::i18n::tr("🔀 Ditheröinti")).strong().color(Color32::from_rgb(195, 155, 255)),
                                )
                                .changed();
                            ui.horizontal(|ui| {
                                ui.label(crate::i18n::tr("Dither-väli:"));
                                dither_changed |= ui
                                    .add(egui::DragValue::new(&mut dither_cfg.interval_frames).range(1..=20).prefix(crate::i18n::tr("joka ")).suffix(crate::i18n::tr(". ruutu")))
                                    .changed();
                            });
                            ui.horizontal(|ui| {
                                ui.label(crate::i18n::tr("Siirron laajuus:"));
                                dither_changed |= ui
                                    .add(egui::DragValue::new(&mut dither_cfg.step_pixels).range(1..=30).suffix(" px"))
                                    .changed();
                            });
                            ui.horizontal(|ui| {
                                ui.label(crate::i18n::tr("Asettumisaika:"));
                                dither_changed |= ui
                                    .add(egui::DragValue::new(&mut dither_cfg.settle_seconds).range(1..=15).suffix(" s"))
                                    .changed();
                            });
                            ui.label(
                                RichText::new(format!("{}: {} {}", crate::i18n::tr("Suoritetut ditheröinnit:"), seq_state.dithers_count, crate::i18n::tr("kpl")))
                                    .small()
                                    .color(Color32::from_rgb(160, 180, 205)),
                            );
                        });

                        // Column 3: Connection Guard & Recovery
                        cols[2].group(|ui| {
                            ui.set_max_width(ui.available_width());
                            guard_changed |= ui
                                .checkbox(
                                    &mut conn_guard.auto_reconnect,
                                    RichText::new(crate::i18n::tr("🛡 Yhteysvahti & Toipuminen")).strong().color(Color32::from_rgb(110, 240, 175)),
                                )
                                .changed();
                            ui.horizontal(|ui| {
                                ui.label(crate::i18n::tr("Yhteyskatkon armonaika:"));
                                guard_changed |= ui
                                    .add(egui::DragValue::new(&mut conn_guard.grace_period_minutes).range(1..=60).suffix(" min"))
                                    .changed();
                            });
                            guard_changed |= ui
                                .checkbox(
                                    &mut conn_guard.resume_stacking_after_reconnect,
                                    crate::i18n::tr("Täsmäytä tila & jatka pinoamista palautuessa"),
                                )
                                .changed();
                            let conn_badge = if telem.connected || telem.is_simulation {
                                format!("🟢 {} ({}: {} {})", crate::i18n::tr("Yhteys kunnossa"), crate::i18n::tr("toipumiset:"), seq_state.reconnect_events_count, crate::i18n::tr("krt"))
                            } else {
                                format!(
                                    "🟠 {} ({} {}/{})",
                                    crate::i18n::tr("Palautetaan yhteyttä"),
                                    crate::i18n::tr("yritys"),
                                    telem.reconnect_attempts,
                                    telem.max_reconnect_attempts
                                )
                            };
                            ui.label(RichText::new(conn_badge).small().color(Color32::from_rgb(160, 180, 205)));
                        });
                    });

                    if af_changed || dither_changed || guard_changed {
                        let mut s = sequencer.state.lock().unwrap();
                        s.autofocus_config = af_cfg;
                        s.dither_config = dither_cfg;
                        s.connection_guard = conn_guard;
                    }
                    if dither_changed {
                        client.configure_dither(dither_cfg);
                    }
                    if guard_changed {
                        client.set_auto_reconnect(conn_guard.auto_reconnect);
                    }
                });

                ui.add_space(8.0);

                // N.I.N.A. Advanced Scheduler Header & Action Bar
                ui.horizontal_wrapped(|ui| {
                    ui.heading(RichText::new(crate::i18n::tr("Kuvauslista (N.I.N.A. Advanced Scheduler)")).size(16.0).color(Color32::from_rgb(255, 215, 100)));
                    ui.separator();
                    ui.label(RichText::new(format!("{} {}", seq_state.items.len(), crate::i18n::tr("toimintoa sekvenssissä"))).color(Color32::from_rgb(180, 195, 215)));
                });

                ui.add_space(4.0);

                // Quick Add Toolbar
                ui.horizontal_wrapped(|ui| {
                    if ui.button(RichText::new(crate::i18n::tr("+ 🎯 Ajo kohteeseen")).color(Color32::from_rgb(120, 220, 255)).strong())
                        .on_hover_text(crate::i18n::tr("Lisää kääntyminen kohteeseen, koordinaatit ja kenttätunnistus (Plate Solving)"))
                        .clicked()
                    {
                        let (name, ra, dec) = if let Some(t) = &state.selected_object {
                            (t.name.clone(), t.ra_hours, t.dec_deg)
                        } else {
                            ("Uusi kohde".to_string(), 0.0, 0.0)
                        };
                        sequencer.add_item(SequenceStep::SlewToTarget {
                            name,
                            ra_hours: ra,
                            dec_deg: dec,
                            solve_and_center: true,
                            min_altitude_deg: 20.0,
                        });
                    }

                    if ui.button(RichText::new(crate::i18n::tr("+ 📸 Valotussarja")).color(Color32::from_rgb(255, 220, 100)).strong())
                        .on_hover_text(crate::i18n::tr("Lisää valotussarja: valittavissa Light, Dark (mustat kuvat), Flat tai Bias"))
                        .clicked()
                    {
                        sequencer.add_item(SequenceStep::TakeExposures {
                            frame_type: FrameType::Light,
                            exposure_secs: state.sequence_sub_exposure_secs,
                            count: 30,
                            dither_every_n: Some(3),
                        });
                    }

                    if ui.button(RichText::new(crate::i18n::tr("+ 🔍 Automaattitarkennus")).color(Color32::from_rgb(255, 235, 120)).strong())
                        .on_hover_text(crate::i18n::tr("Lisää automaattitarkennus (Auto-Focus)"))
                        .clicked()
                    {
                        sequencer.add_item(SequenceStep::AutoFocus {
                            reason: "Tarkennus".to_string(),
                        });
                    }

                    if ui.button(RichText::new(crate::i18n::tr("+ 🎛 Tarkennusasento")).color(Color32::from_rgb(220, 180, 255)).strong())
                        .on_hover_text(crate::i18n::tr("Aseta tarkennusmoottori kiinteään askelmäärään"))
                        .clicked()
                    {
                        sequencer.add_item(SequenceStep::SetFocuserPosition {
                            position: telem.focus_position,
                        });
                    }

                    if ui.button(RichText::new(crate::i18n::tr("+ 🛡 Suodatin")).color(Color32::from_rgb(100, 240, 210)).strong())
                        .on_hover_text(crate::i18n::tr("Vaihda suodatinta (Normaali UV/IR, Valosaaste LP tai Dark suljettu)"))
                        .clicked()
                    {
                        sequencer.add_item(SequenceStep::SetFilter {
                            filter: FilterChoice::LightPollution,
                        });
                    }

                    if ui.button(RichText::new(crate::i18n::tr("+ 🌑 Pika Dark-sarja")).color(Color32::from_rgb(180, 160, 255)).strong())
                        .on_hover_text(crate::i18n::tr("Lisää pimeiden kalibrointikuvien sarja (Suodin Dark -> 20 × Dark -> Suodin Normaali)"))
                        .clicked()
                    {
                        sequencer.add_dark_calibration_sequence(state.sequence_sub_exposure_secs, 20);
                    }

                    if ui.button(RichText::new(crate::i18n::tr("+ ⏳ Odotusaika")).color(Color32::from_rgb(255, 180, 140)).strong())
                        .on_hover_text(crate::i18n::tr("Lisää määräaikainen tauko tai odotusaika"))
                        .clicked()
                    {
                        sequencer.add_item(SequenceStep::WaitSeconds {
                            seconds: 60,
                            reason: "Tauko".to_string(),
                        });
                    }

                    if ui.button(RichText::new(crate::i18n::tr("+ 🏠 Parkkeeraa")).color(Color32::from_rgb(190, 190, 200)).strong())
                        .on_hover_text(crate::i18n::tr("Aja teleskooppi lepoasentoon"))
                        .clicked()
                    {
                        sequencer.add_item(SequenceStep::Park);
                    }
                });

                // Quick Presets
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(crate::i18n::tr("Pikatoiminnot:")).small().color(Color32::from_rgb(140, 155, 175)));
                    if let Some(target) = &state.selected_object {
                        let btn_txt = format!("✨ {} ({})", crate::i18n::tr("Lisää tähtikartan kohde N.I.N.A.-sarjana:"), target.id);
                        if ui.button(RichText::new(btn_txt).small().color(Color32::from_rgb(110, 255, 200))).clicked() {
                            sequencer.add_target_with_exposure(target, state.sequence_duration_mins, state.sequence_sub_exposure_secs, state.sequence_use_lp);
                        }
                    }
                    if ui.button(RichText::new(crate::i18n::tr("🌑 Täysi Dark-paketti (20×10s + 20×30s)")).small().color(Color32::from_rgb(190, 160, 255))).clicked() {
                        sequencer.add_dark_calibration_sequence(10, 20);
                        sequencer.add_dark_calibration_sequence(30, 20);
                    }
                });

                ui.add_space(6.0);

                // N.I.N.A. Instruction List
                if seq_state.items.is_empty() {
                    ui.group(|ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space(20.0);
                            ui.label(RichText::new(crate::i18n::tr("Kuvauslista on tyhjä.")).size(16.0).color(Color32::from_rgb(160, 170, 190)));
                            ui.label(crate::i18n::tr("Lisää toimintoja yltä tai valitse kohde tähtikartalta. Voit vapaasti yhdistellä kohteeseen ajoa, valotuksia, tarkennuksia ja suotimia."));
                            ui.add_space(20.0);
                        });
                    });
                } else {
                    let mut item_update: Option<(usize, SequenceItem)> = None;

                    for (idx, item) in seq_state.items.iter().enumerate() {
                        let is_active = seq_state.current_item_idx == Some(idx) && seq_state.is_running;
                        let mut item_mut = item.clone();
                        let mut modified = false;

                        let stroke_color = if is_active {
                            Color32::from_rgb(255, 215, 60)
                        } else if !item.enabled {
                            Color32::from_rgb(60, 65, 75)
                        } else {
                            Color32::from_rgb(80, 90, 110)
                        };

                        egui::Frame::group(ui.style())
                            .stroke(Stroke::new(if is_active { 2.0_f32 } else { 1.0_f32 }, stroke_color))
                            .show(ui, |ui| {
                                ui.set_max_width(ui.available_width());

                                // Row 1: Header (Checkbox, Step Number, Icon, Title, Action Buttons, Status)
                                ui.horizontal(|ui| {
                                    if ui.checkbox(&mut item_mut.enabled, "").on_hover_text(crate::i18n::tr("Kytke toiminto päälle/pois")).changed() {
                                        modified = true;
                                    }

                                    let title_color = if is_active {
                                        Color32::from_rgb(255, 230, 80)
                                    } else if item.status == JobStatus::Completed {
                                        Color32::from_rgb(100, 235, 130)
                                    } else if !item.enabled {
                                        Color32::from_rgb(130, 140, 155)
                                    } else {
                                        Color32::from_rgb(215, 230, 255)
                                    };

                                    let prefix = if is_active { "▶ " } else { "" };
                                    let header_title = format!("{}{}. {} {}", prefix, idx + 1, item.icon(), crate::i18n::tr(item.type_name_fi()));

                                    let status_color = match item.status {
                                        JobStatus::Completed => Color32::from_rgb(100, 255, 120),
                                        JobStatus::Stacking => Color32::from_rgb(80, 200, 255),
                                        JobStatus::Focusing => Color32::from_rgb(255, 225, 90),
                                        JobStatus::Dithering => Color32::from_rgb(190, 140, 255),
                                        JobStatus::WaitingConnection => Color32::from_rgb(255, 160, 60),
                                        JobStatus::Slewing | JobStatus::Solving => Color32::from_rgb(255, 210, 80),
                                        JobStatus::Failed => Color32::from_rgb(255, 100, 100),
                                        JobStatus::Skipped => Color32::from_rgb(150, 160, 180),
                                        JobStatus::Pending => Color32::from_rgb(170, 185, 210),
                                    };

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.button(crate::i18n::tr("✖ Poista")).on_hover_text(crate::i18n::tr("Poista toiminto sekvenssistä")).clicked() {
                                            sequencer.remove_item(idx);
                                        }
                                        if ui.button(crate::i18n::tr("📋 Kopioi")).on_hover_text(crate::i18n::tr("Monista toiminto")).clicked() {
                                            sequencer.duplicate_item(idx);
                                        }
                                        if ui.button("⬇").on_hover_text(crate::i18n::tr("Siirrä alemmas")).clicked() {
                                            sequencer.move_item_down(idx);
                                        }
                                        if ui.button("⬆").on_hover_text(crate::i18n::tr("Siirrä ylemmäs")).clicked() {
                                            sequencer.move_item_up(idx);
                                        }

                                        ui.label(RichText::new(crate::i18n::tr(item.status.display_fi())).color(status_color).strong());

                                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                            ui.label(RichText::new(header_title).strong().size(14.0).color(title_color));
                                        });
                                    });
                                });

                                ui.add_space(4.0);

                                // Row 2: In-place Editable Parameters
                                match &mut item_mut.step {
                                    SequenceStep::SlewToTarget { name, ra_hours, dec_deg, solve_and_center, min_altitude_deg } => {
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label(crate::i18n::tr("Kohde:"));
                                            if ui.add(egui::TextEdit::singleline(name).desired_width(140.0)).changed() {
                                                modified = true;
                                            }
                                            if ui.add(egui::DragValue::new(ra_hours).range(0.0..=24.0).speed(0.05).prefix("RA: ").suffix("h")).changed() {
                                                modified = true;
                                            }
                                            if ui.add(egui::DragValue::new(dec_deg).range(-90.0..=90.0).speed(0.1).prefix("Dec: ").suffix("°")).changed() {
                                                modified = true;
                                            }
                                            if ui.checkbox(solve_and_center, crate::i18n::tr("Kenttätunnistus (Plate Solving)")).changed() {
                                                modified = true;
                                            }
                                            if let Some(sel) = &state.selected_object {
                                                if ui.small_button(format!("📍 {}", crate::i18n::tr("Täytä valitusta"))).clicked() {
                                                    *name = sel.name.clone();
                                                    *ra_hours = sel.ra_hours;
                                                    *dec_deg = sel.dec_deg;
                                                    modified = true;
                                                }
                                            }
                                            let (cur_alt, cur_az) = ra_dec_to_alt_az(*ra_hours, *dec_deg, lat, lon, &now_utc);
                                            ui.label(RichText::new(format!("📐 Alt: {:.1}° (Az: {:.0}°)", cur_alt, cur_az)).small().color(Color32::from_rgb(160, 180, 205)));
                                            ui.label(crate::i18n::tr("Min Alt:"));
                                            if ui.add(egui::DragValue::new(min_altitude_deg).range(0.0..=80.0).suffix("°")).changed() {
                                                modified = true;
                                            }
                                        });
                                    }

                                    SequenceStep::TakeExposures { frame_type, exposure_secs, count, dither_every_n } => {
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label(RichText::new(crate::i18n::tr("Tyyppi:")).strong());
                                            for &(ft, label) in &[
                                                (FrameType::Light, "✨ Light"),
                                                (FrameType::Dark, "🌑 Dark"),
                                                (FrameType::Flat, "⬜ Flat"),
                                                (FrameType::Bias, "⚡ Bias"),
                                            ] {
                                                let is_sel = *frame_type == ft;
                                                if ui.add(egui::SelectableLabel::new(is_sel, label)).clicked() {
                                                    *frame_type = ft;
                                                    modified = true;
                                                }
                                            }

                                            ui.separator();
                                            ui.label(RichText::new(crate::i18n::tr("Valotusaika:")).strong());
                                            for &sec in &[1, 2, 5, 10, 20, 30, 60] {
                                                let is_sel = *exposure_secs == sec;
                                                if ui.add(egui::SelectableLabel::new(is_sel, format!("{}s", sec))).clicked() {
                                                    *exposure_secs = sec;
                                                    modified = true;
                                                }
                                            }
                                            if ui.add(egui::DragValue::new(exposure_secs).range(1..=300).suffix("s")).changed() {
                                                modified = true;
                                            }

                                            ui.separator();
                                            ui.label(RichText::new(crate::i18n::tr("Määrä:")).strong());
                                            if ui.add(egui::DragValue::new(count).range(1..=1000).suffix(" kpl")).changed() {
                                                modified = true;
                                            }

                                            let total_s = (*exposure_secs) * (*count);
                                            ui.label(RichText::new(format!("⏱ {}: {} min {:02} s", crate::i18n::tr("Kesto"), total_s / 60, total_s % 60)).color(Color32::from_rgb(255, 220, 120)));

                                            if *frame_type == FrameType::Light {
                                                ui.separator();
                                                let mut use_dither = dither_every_n.is_some();
                                                if ui.checkbox(&mut use_dither, crate::i18n::tr("Ditheröinti")).changed() {
                                                    *dither_every_n = if use_dither { Some(3) } else { None };
                                                    modified = true;
                                                }
                                                if let Some(ref mut d_int) = dither_every_n {
                                                    ui.label(crate::i18n::tr("Väli:"));
                                                    if ui.add(egui::DragValue::new(d_int).range(1..=20).prefix(crate::i18n::tr("joka ")).suffix(crate::i18n::tr(". kuva"))).changed() {
                                                        modified = true;
                                                    }
                                                }
                                            }
                                        });
                                    }

                                    SequenceStep::SetFilter { filter } => {
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label(RichText::new(crate::i18n::tr("Suodattimen asetus:")).strong());
                                            for &(fc, label) in &[
                                                (FilterChoice::Normal, "☀️ Normaali (UV/IR Cut)"),
                                                (FilterChoice::LightPollution, "🌙 Valosaaste (Dual-Band LP)"),
                                                (FilterChoice::Dark, "🌑 Dark (Suljettu maski)"),
                                            ] {
                                                let is_sel = *filter == fc;
                                                if ui.add(egui::SelectableLabel::new(is_sel, label)).clicked() {
                                                    *filter = fc;
                                                    modified = true;
                                                }
                                            }
                                        });
                                    }

                                    SequenceStep::AutoFocus { reason } => {
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label(RichText::new(crate::i18n::tr("Automaattitarkennus")).strong().color(Color32::from_rgb(255, 230, 110)));
                                            ui.label(crate::i18n::tr("Syy / Kuvaus:"));
                                            if ui.add(egui::TextEdit::singleline(reason).desired_width(200.0)).changed() {
                                                modified = true;
                                            }
                                            ui.label(RichText::new(crate::i18n::tr("Ajaa HFD-tähtitarkennuksen automaattisesti")).small().color(Color32::from_rgb(160, 180, 205)));
                                        });
                                    }

                                    SequenceStep::SetFocuserPosition { position } => {
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label(RichText::new(crate::i18n::tr("Tarkennusmoottorin kiinteä asento:")).strong());
                                            if ui.add(egui::DragValue::new(position).range(0..=50000).suffix(" askelta")).changed() {
                                                modified = true;
                                            }
                                            if ui.small_button(crate::i18n::tr("Käytä nykyistä asentoa")).clicked() {
                                                *position = telem.focus_position;
                                                modified = true;
                                            }
                                        });
                                    }

                                    SequenceStep::WaitSeconds { seconds, reason } => {
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label(RichText::new(crate::i18n::tr("Odotusaika:")).strong());
                                            if ui.add(egui::DragValue::new(seconds).range(1..=3600).suffix(" s")).changed() {
                                                modified = true;
                                            }
                                            ui.label(crate::i18n::tr("Syy:"));
                                            if ui.add(egui::TextEdit::singleline(reason).desired_width(160.0)).changed() {
                                                modified = true;
                                            }
                                        });
                                    }

                                    SequenceStep::Park => {
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(crate::i18n::tr("🏠 Ajaa kaukoputken turvalliseen parkkiasentoon ja sammuttaa moottorit.")).color(Color32::from_rgb(180, 190, 210)));
                                        });
                                    }
                                }

                                // Row 3: Live Progress Display when active
                                if is_active {
                                    ui.add_space(4.0);
                                    let est_s = item.estimated_duration_secs().max(1);
                                    let frac = (item.elapsed_seconds as f32 / est_s as f32).clamp(0.0, 1.0);
                                    ui.add(egui::ProgressBar::new(frac).show_percentage().animate(true));
                                    if !item.progress_str.is_empty() {
                                        ui.label(RichText::new(format!("⚡ {}", item.progress_str)).small().color(Color32::from_rgb(255, 230, 120)));
                                    }
                                }
                            });

                        if modified {
                            item_update = Some((idx, item_mut));
                        }
                        ui.add_space(4.0);
                    }

                    if let Some((idx, new_item)) = item_update {
                        let mut s = sequencer.state.lock().unwrap();
                        if idx < s.items.len() {
                            s.items[idx] = new_item;
                        }
                    }
                }

                ui.add_space(8.0);

                // Sequencer Execution Logs
                if !seq_state.logs.is_empty() {
                    ui.collapsing(RichText::new(crate::i18n::tr("📜 Lokitapahtumat")).size(14.0), |ui| {
                        egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                            for log in seq_state.logs.iter().rev().take(20) {
                                ui.label(RichText::new(log).small().color(Color32::from_rgb(170, 180, 200)));
                            }
                        });
                    });
                }

                ui.add_space(40.0);
            });
    }

    /// Projects horizontal coordinates (Alt, Az in degrees) to screen position (Pos2)
    /// based on the current StarMapMode (Horizon 3D dome vs Zenith All-Sky).
    pub fn project_star_point(
        mode: StarMapMode,
        alt_deg: f64,
        az_deg: f64,
        rect: Rect,
        center: Pos2,
        radius: f32,
        view_az: f64,
        view_alt: f64,
        fov_deg: f64,
    ) -> Option<Pos2> {
        match mode {
            StarMapMode::ZenithAllSky => {
                let (norm_x, norm_y) = alt_az_to_canvas_normalized(alt_deg, az_deg)?;
                Some(Pos2::new(center.x + norm_x * radius, center.y + norm_y * radius))
            }
            StarMapMode::Horizon => {
                let a = alt_deg.to_radians();
                let az = az_deg.to_radians();
                let px = a.cos() * az.cos();
                let py = a.cos() * az.sin();
                let pz = a.sin();

                let a0 = view_alt.to_radians();
                let az0 = view_az.to_radians();

                // Camera Look vector forward
                let lx = a0.cos() * az0.cos();
                let ly = a0.cos() * az0.sin();
                let lz = a0.sin();

                // Camera Right vector (horizontal in XY ground plane)
                let rx = -az0.sin();
                let ry = az0.cos();
                let rz = 0.0;

                // Camera Up vector = L x R
                let ux = -a0.sin() * az0.cos();
                let uy = -a0.sin() * az0.sin();
                let uz = a0.cos();

                // Depth along line of sight
                let depth = px * lx + py * ly + pz * lz;
                if depth <= 0.06 {
                    return None;
                }

                let x_cam = px * rx + py * ry + pz * rz;
                let y_cam = px * ux + py * uy + pz * uz;

                let x_proj = x_cam / depth;
                let y_proj = y_cam / depth;

                let fov_rad = fov_deg.to_radians();
                let f = 0.5 / (fov_rad * 0.5).tan();
                let scale = rect.height() as f64 * f;

                let sx = rect.center().x as f64 + x_proj * scale;
                let sy = rect.center().y as f64 - y_proj * scale;

                Some(Pos2::new(sx as f32, sy as f32))
            }
        }
    }

    /// Renders the Interactive Star Chart in the Central Panel
    pub fn ui_star_chart_canvas(
        ui: &mut egui::Ui,
        state: &mut TelescopeViewState,
        client: &Arc<SeestarClient>,
        catalog: &[SkyObject],
        lat: f64,
        lon: f64,
    ) {
        let telem = client.get_telemetry();
        let now_utc = Utc::now();
        ui.set_max_width(ui.available_width());

        // Top view toolbar
        ui.horizontal_wrapped(|ui| {
            ui.heading(RichText::new(crate::i18n::tr("🌌 Interaktiivinen tähtikartta")).color(Color32::from_rgb(80, 200, 255)).strong());
            ui.separator();

            // Mode Selector
            let is_horizon = state.map_mode == StarMapMode::Horizon;
            if ui.selectable_label(is_horizon, crate::i18n::tr("🌐 Horisontti"))
                .on_hover_text(crate::i18n::tr("3D Taivaskupu & Horisontti: Katso taivaalle maasta käsin 360°"))
                .clicked()
            {
                state.map_mode = StarMapMode::Horizon;
            }
            if ui.selectable_label(!is_horizon, crate::i18n::tr("⭕ Koko taivas"))
                .on_hover_text(crate::i18n::tr("Koko taivas ylhäältä päin: 360° panoraama ympyränä"))
                .clicked()
            {
                state.map_mode = StarMapMode::ZenithAllSky;
            }

            ui.separator();

            if is_horizon {
                ui.label(RichText::new(crate::i18n::tr("Suunta:")).small().color(Color32::from_rgb(150, 180, 210)));
                if ui.small_button("P").on_hover_text(crate::i18n::tr("Käännä katse Pohjoiseen (0°)")).clicked() {
                    state.view_az = 0.0;
                    state.view_alt = 25.0;
                }
                if ui.small_button("KO").on_hover_text("Koillinen (45°)").clicked() {
                    state.view_az = 45.0;
                    state.view_alt = 25.0;
                }
                if ui.small_button("I").on_hover_text(crate::i18n::tr("Käännä katse Itään (90°)")).clicked() {
                    state.view_az = 90.0;
                    state.view_alt = 25.0;
                }
                if ui.small_button("KA").on_hover_text("Kaakko (135°)").clicked() {
                    state.view_az = 135.0;
                    state.view_alt = 25.0;
                }
                if ui.small_button("E").on_hover_text(crate::i18n::tr("Käännä katse Etelään (180°)")).clicked() {
                    state.view_az = 180.0;
                    state.view_alt = 25.0;
                }
                if ui.small_button("LO").on_hover_text("Lounas (225°)").clicked() {
                    state.view_az = 225.0;
                    state.view_alt = 25.0;
                }
                if ui.small_button("L").on_hover_text(crate::i18n::tr("Käännä katse Länteen (270°)")).clicked() {
                    state.view_az = 270.0;
                    state.view_alt = 25.0;
                }
                if ui.small_button("LU").on_hover_text("Luode (315°)").clicked() {
                    state.view_az = 315.0;
                    state.view_alt = 25.0;
                }

                let (scope_alt, scope_az) = ra_dec_to_alt_az(telem.current_ra, telem.current_dec, lat, lon, &now_utc);
                if ui.small_button("🔭 Putki").on_hover_text(crate::i18n::tr("Käännä näkymä suoraan kaukoputken suuntaan")).clicked() {
                    state.view_az = scope_az;
                    state.view_alt = scope_alt.clamp(5.0, 85.0);
                }
            } else {
                if ui.selectable_label(state.map_fill_width, crate::i18n::tr("⛶ Täytä näkymä"))
                    .on_hover_text(crate::i18n::tr("Sovita tähtikartta täyttämään koko ruudun leveys"))
                    .clicked()
                {
                    state.map_fill_width = !state.map_fill_width;
                }
            }

            ui.separator();

            if ui.small_button(crate::i18n::tr("🔍 Nollaa")).on_hover_text(crate::i18n::tr("Palauta oletusnäkymä ja zoomaus")).clicked() {
                match state.map_mode {
                    StarMapMode::Horizon => {
                        state.view_az = 180.0;
                        state.view_alt = 25.0;
                        state.horizon_fov_deg = 80.0;
                    }
                    StarMapMode::ZenithAllSky => {
                        state.map_zoom = 1.0;
                        state.map_pan = Vec2::ZERO;
                    }
                }
            }

            if ui.available_width() > 220.0 {
                let loc_str = format!("{} {:.2}° N, {:.2}° E | UTC: {}", crate::i18n::tr("Sijainti:"), lat, lon, now_utc.format("%H:%M:%S"));
                ui.label(RichText::new(loc_str).small().color(Color32::from_rgb(140, 160, 190)));
            }
        });

        ui.separator();

        // Canvas allocation
        let available_size = ui.available_size();
        let (response, painter) = ui.allocate_painter(available_size, egui::Sense::click_and_drag());
        let rect = response.rect;

        // Pan / Look rotation (only when dragging on the canvas)
        if response.dragged() {
            let delta = response.drag_delta();
            match state.map_mode {
                StarMapMode::Horizon => {
                    let deg_per_px = (state.horizon_fov_deg / rect.width().max(400.0) as f64) * 0.85;
                    state.view_az = (state.view_az - delta.x as f64 * deg_per_px).rem_euclid(360.0);
                    state.view_alt = (state.view_alt + delta.y as f64 * deg_per_px).clamp(-10.0, 89.0);
                }
                StarMapMode::ZenithAllSky => {
                    state.map_pan += delta;
                }
            }
        }

        // Controlled, gentle zoom proportional to scroll amount (no sudden jumps!)
        if response.hovered() {
            let scroll = ui.input(|i| {
                if i.smooth_scroll_delta.y != 0.0 {
                    i.smooth_scroll_delta.y
                } else {
                    i.raw_scroll_delta.y
                }
            });

            if scroll != 0.0 {
                let factor = (scroll as f64 * 0.0012).clamp(-0.05, 0.05);
                match state.map_mode {
                    StarMapMode::Horizon => {
                        let fov_factor = 1.0 - factor;
                        state.horizon_fov_deg = (state.horizon_fov_deg * fov_factor).clamp(15.0, 120.0);
                    }
                    StarMapMode::ZenithAllSky => {
                        let old_zoom = state.map_zoom;
                        let new_zoom = (state.map_zoom as f64 * (1.0 + factor)).clamp(0.5, 6.0) as f32;
                        state.map_zoom = new_zoom;

                        if let Some(hover_pos) = response.hover_pos() {
                            let center_offset = hover_pos - rect.center() - state.map_pan;
                            state.map_pan -= center_offset * (new_zoom / old_zoom - 1.0_f32);
                        }
                    }
                }
            }
        }

        let base_radius = if state.map_fill_width {
            (rect.width() / 2.0 - 15.0).max(rect.height() / 2.0 - 15.0)
        } else {
            (rect.width().min(rect.height()) / 2.0 - 30.0).max(120.0)
        };
        let radius = base_radius * state.map_zoom;
        let center = rect.center() + state.map_pan;

        // Clip to drawing area with nice deep night sky background
        painter.rect_filled(rect, 0.0, Color32::from_rgb(10, 14, 22));

        match state.map_mode {
            StarMapMode::ZenithAllSky => {
                // Background circular celestial dome
                painter.circle_filled(center, radius, Color32::from_rgb(14, 20, 32));

                // Outer Horizon ring (0° altitude)
                painter.circle_stroke(
                    center,
                    radius,
                    Stroke::new(2.5_f32, Color32::from_rgb(50, 75, 110)),
                );

                // Altitude circles: 30° and 60°
                let r_30 = radius * (60.0 / 90.0);
                let r_60 = radius * (30.0 / 90.0);
                painter.circle_stroke(
                    center,
                    r_30,
                    Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(60, 85, 120, 90)),
                );
                painter.circle_stroke(
                    center,
                    r_60,
                    Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(60, 85, 120, 70)),
                );

                // Altitude labels
                painter.text(
                    Pos2::new(center.x, center.y - r_30),
                    egui::Align2::CENTER_CENTER,
                    "30°",
                    egui::FontId::proportional(11.0),
                    Color32::from_rgb(100, 120, 150),
                );
                painter.text(
                    Pos2::new(center.x, center.y - r_60),
                    egui::Align2::CENTER_CENTER,
                    "60°",
                    egui::FontId::proportional(11.0),
                    Color32::from_rgb(100, 120, 150),
                );

                // Zenith marker (90°)
                painter.line_segment(
                    [Pos2::new(center.x - 6.0, center.y), Pos2::new(center.x + 6.0, center.y)],
                    Stroke::new(1.0_f32, Color32::from_rgb(120, 140, 180)),
                );
                painter.line_segment(
                    [Pos2::new(center.x, center.y - 6.0), Pos2::new(center.x, center.y + 6.0)],
                    Stroke::new(1.0_f32, Color32::from_rgb(120, 140, 180)),
                );
                painter.text(
                    Pos2::new(center.x + 8.0, center.y - 8.0),
                    egui::Align2::LEFT_BOTTOM,
                    crate::i18n::tr("Seniitti (90°)"),
                    egui::FontId::proportional(10.0),
                    Color32::from_rgb(120, 140, 180),
                );

                // Cardinal directions: N, E, S, W
                let dir_font = egui::FontId::proportional(14.0);
                painter.text(
                    Pos2::new(center.x, center.y - radius - 16.0),
                    egui::Align2::CENTER_CENTER,
                    crate::i18n::tr("P (Pohjoinen)"),
                    dir_font.clone(),
                    Color32::from_rgb(255, 100, 100),
                );
                painter.text(
                    Pos2::new(center.x - radius - 20.0, center.y),
                    egui::Align2::CENTER_CENTER,
                    crate::i18n::tr("I (Itä)"),
                    dir_font.clone(),
                    Color32::from_rgb(120, 200, 255),
                );
                painter.text(
                    Pos2::new(center.x, center.y + radius + 16.0),
                    egui::Align2::CENTER_CENTER,
                    crate::i18n::tr("E (Etelä)"),
                    dir_font.clone(),
                    Color32::from_rgb(100, 255, 120),
                );
                painter.text(
                    Pos2::new(center.x + radius + 20.0, center.y),
                    egui::Align2::CENTER_CENTER,
                    crate::i18n::tr("L (Länsi)"),
                    dir_font,
                    Color32::from_rgb(255, 210, 80),
                );
            }
            StarMapMode::Horizon => {
                // Collect horizon line points across visible azimuth range
                let mut horizon_pts = Vec::new();
                let az_center = state.view_az;
                let span = 95.0;
                let step = 2.0;
                let mut az_curr = az_center - span;
                while az_curr <= az_center + span {
                    let norm_az = az_curr.rem_euclid(360.0);
                    if let Some(pt) = Self::project_star_point(
                        StarMapMode::Horizon,
                        0.0,
                        norm_az,
                        rect,
                        center,
                        radius,
                        state.view_az,
                        state.view_alt,
                        state.horizon_fov_deg,
                    ) {
                        horizon_pts.push(pt);
                    }
                    az_curr += step;
                }

                // Ground shading below horizon line using convex trapezoid strips
                let ground_color = Color32::from_rgb(12, 18, 14);
                let bottom_y = rect.bottom() + 10.0;
                for i in 0..horizon_pts.len().saturating_sub(1) {
                    let p1 = horizon_pts[i];
                    let p2 = horizon_pts[i + 1];
                    painter.add(egui::Shape::convex_polygon(
                        vec![
                            p1,
                            p2,
                            Pos2::new(p2.x, bottom_y),
                            Pos2::new(p1.x, bottom_y),
                        ],
                        ground_color,
                        Stroke::NONE,
                    ));
                }

                // Luminous emerald horizon stroke line
                let horizon_stroke = Stroke::new(2.0_f32, Color32::from_rgb(45, 95, 65));
                for i in 0..horizon_pts.len().saturating_sub(1) {
                    painter.line_segment([horizon_pts[i], horizon_pts[i + 1]], horizon_stroke);
                }

                // Altitude arcs: 20°, 40°, 60°, 80°
                let arc_stroke = Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(70, 100, 140, 50));
                for &alt_deg in &[20.0, 40.0, 60.0, 80.0] {
                    let mut arc_pts = Vec::new();
                    let mut az_curr = az_center - 85.0;
                    while az_curr <= az_center + 85.0 {
                        let norm_az = az_curr.rem_euclid(360.0);
                        if let Some(pt) = Self::project_star_point(
                            StarMapMode::Horizon,
                            alt_deg,
                            norm_az,
                            rect,
                            center,
                            radius,
                            state.view_az,
                            state.view_alt,
                            state.horizon_fov_deg,
                        ) {
                            arc_pts.push(pt);
                        }
                        az_curr += 4.0;
                    }
                    for i in 0..arc_pts.len().saturating_sub(1) {
                        painter.line_segment([arc_pts[i], arc_pts[i + 1]], arc_stroke);
                    }
                    if let Some(lbl_pt) = Self::project_star_point(
                        StarMapMode::Horizon,
                        alt_deg,
                        az_center,
                        rect,
                        center,
                        radius,
                        state.view_az,
                        state.view_alt,
                        state.horizon_fov_deg,
                    ) {
                        if rect.contains(lbl_pt) {
                            painter.text(
                                Pos2::new(lbl_pt.x + 4.0, lbl_pt.y - 2.0),
                                egui::Align2::LEFT_BOTTOM,
                                format!("{:.0}°", alt_deg),
                                egui::FontId::proportional(10.0),
                                Color32::from_rgba_unmultiplied(120, 150, 180, 160),
                            );
                        }
                    }
                }

                // Zenith marker (90°)
                if let Some(zenith_pt) = Self::project_star_point(
                    StarMapMode::Horizon,
                    90.0,
                    0.0,
                    rect,
                    center,
                    radius,
                    state.view_az,
                    state.view_alt,
                    state.horizon_fov_deg,
                ) {
                    if rect.contains(zenith_pt) {
                        let z_color = Color32::from_rgb(140, 180, 230);
                        painter.line_segment([Pos2::new(zenith_pt.x - 6.0, zenith_pt.y), Pos2::new(zenith_pt.x + 6.0, zenith_pt.y)], Stroke::new(1.0_f32, z_color));
                        painter.line_segment([Pos2::new(zenith_pt.x, zenith_pt.y - 6.0), Pos2::new(zenith_pt.x, zenith_pt.y + 6.0)], Stroke::new(1.0_f32, z_color));
                        painter.text(
                            Pos2::new(zenith_pt.x + 8.0, zenith_pt.y - 8.0),
                            egui::Align2::LEFT_BOTTOM,
                            crate::i18n::tr("Seniitti (90°)"),
                            egui::FontId::proportional(10.5),
                            z_color,
                        );
                    }
                }

                // Compass markers along the horizon
                let compass_dirs: &[(f64, &str, Color32)] = &[
                    (0.0, "P (Pohjoinen)", Color32::from_rgb(255, 100, 100)),
                    (45.0, "KO (Koillinen)", Color32::from_rgb(180, 210, 255)),
                    (90.0, "I (Itä)", Color32::from_rgb(120, 200, 255)),
                    (135.0, "KA (Kaakko)", Color32::from_rgb(180, 230, 200)),
                    (180.0, "E (Etelä)", Color32::from_rgb(100, 255, 120)),
                    (225.0, "LO (Lounas)", Color32::from_rgb(230, 230, 140)),
                    (270.0, "L (Länsi)", Color32::from_rgb(255, 210, 80)),
                    (315.0, "LU (Luode)", Color32::from_rgb(255, 170, 140)),
                ];

                for &(deg, name, color) in compass_dirs {
                    if let Some(pt) = Self::project_star_point(
                        StarMapMode::Horizon,
                        0.0,
                        deg,
                        rect,
                        center,
                        radius,
                        state.view_az,
                        state.view_alt,
                        state.horizon_fov_deg,
                    ) {
                        if pt.x >= rect.left() - 20.0 && pt.x <= rect.right() + 20.0 {
                            painter.line_segment(
                                [Pos2::new(pt.x, pt.y - 8.0), Pos2::new(pt.x, pt.y + 8.0)],
                                Stroke::new(2.0_f32, color),
                            );
                            painter.text(
                                Pos2::new(pt.x, pt.y + 16.0),
                                egui::Align2::CENTER_TOP,
                                crate::i18n::tr(name),
                                egui::FontId::proportional(12.0),
                                color,
                            );
                        }
                    }
                }
            }
        }

        // Draw sky objects
        let mouse_pos = response.hover_pos();
        let mut hovered: Option<SkyObject> = None;
        let mut clicked_object: Option<SkyObject> = None;

        for obj in catalog {
            let (alt, az) = ra_dec_to_alt_az(obj.ra_hours, obj.dec_deg, lat, lon, &now_utc);
            let min_alt = if state.map_mode == StarMapMode::Horizon { -4.0 } else { 0.0 };
            if alt < min_alt {
                continue;
            }

            if let Some(pt) = Self::project_star_point(
                state.map_mode,
                alt,
                az,
                rect,
                center,
                radius,
                state.view_az,
                state.view_alt,
                state.horizon_fov_deg,
            ) {
                if !rect.expand(20.0).contains(pt) {
                    continue;
                }

                // Check mouse proximity
                let is_hovered = mouse_pos.map(|mp| mp.distance(pt) < 14.0).unwrap_or(false);
                if is_hovered {
                    hovered = Some(obj.clone());
                    if response.clicked() {
                        clicked_object = Some(obj.clone());
                    }
                }

                let is_selected = state.selected_object.as_ref().map(|o| o.id == obj.id).unwrap_or(false);
                let (r, g, b) = obj.obj_type.color_rgb();
                let alpha = if alt < 0.0 { 110 } else { 255 };
                let base_color = Color32::from_rgba_unmultiplied(r, g, b, alpha);

                // Draw object shape
                match obj.obj_type {
                    DsoType::Star => {
                        let dot_r = (3.5_f32 - obj.magnitude * 0.4_f32).clamp(1.5_f32, 4.5_f32);
                        painter.circle_filled(pt, dot_r, base_color);
                    }
                    DsoType::Galaxy => {
                        let rect_w = 7.0;
                        let rect_h = 4.0;
                        painter.rect_filled(Rect::from_center_size(pt, Vec2::new(rect_w, rect_h)), 2.0, base_color);
                    }
                    DsoType::EmissionNebula | DsoType::SupernovaRemnant => {
                        painter.rect_filled(Rect::from_center_size(pt, Vec2::splat(6.0)), 1.5, base_color);
                    }
                    DsoType::ReflectionNebula => {
                        let d = 5.5;
                        painter.add(egui::Shape::convex_polygon(
                            vec![
                                Pos2::new(pt.x, pt.y - d),
                                Pos2::new(pt.x + d, pt.y),
                                Pos2::new(pt.x, pt.y + d),
                                Pos2::new(pt.x - d, pt.y),
                            ],
                            base_color,
                            Stroke::NONE,
                        ));
                    }
                    DsoType::PlanetaryNebula => {
                        painter.circle_stroke(pt, 4.5, Stroke::new(1.8_f32, base_color));
                    }
                    DsoType::OpenCluster | DsoType::GlobularCluster => {
                        painter.circle_stroke(pt, 5.0, Stroke::new(1.2_f32, base_color));
                        painter.circle_filled(pt, 2.0, base_color);
                    }
                    DsoType::Planet => {
                        painter.circle_filled(pt, 5.5, base_color);
                        painter.circle_stroke(pt, 7.5, Stroke::new(1.0_f32, base_color));
                    }
                }

                // Selected ring & prominent label
                if is_selected {
                    painter.circle_stroke(pt, 12.0, Stroke::new(2.0_f32, Color32::from_rgb(255, 230, 80)));
                    painter.text(
                        Pos2::new(pt.x, pt.y - 16.0),
                        egui::Align2::CENTER_BOTTOM,
                        format!("{} ({})", obj.id, obj.name),
                        egui::FontId::proportional(12.0),
                        Color32::from_rgb(255, 240, 100),
                    );
                } else if is_hovered || obj.magnitude < 5.0 {
                    painter.text(
                        Pos2::new(pt.x + 8.0, pt.y - 6.0),
                        egui::Align2::LEFT_CENTER,
                        &obj.id,
                        egui::FontId::proportional(10.5),
                        Color32::from_rgba_unmultiplied(r, g, b, (alpha as f32 * 0.8) as u8),
                    );
                }
            }
        }

        // Draw Seestar Telescope Pointing Reticle
        let (scope_alt, scope_az) = ra_dec_to_alt_az(telem.current_ra, telem.current_dec, lat, lon, &now_utc);
        if let Some(pt) = Self::project_star_point(
            state.map_mode,
            scope_alt,
            scope_az,
            rect,
            center,
            radius,
            state.view_az,
            state.view_alt,
            state.horizon_fov_deg,
        ) {
            if rect.expand(40.0).contains(pt) {
                let reticle_color = Color32::from_rgb(80, 255, 160);
                painter.circle_stroke(pt, 16.0, Stroke::new(2.0_f32, reticle_color));
                painter.circle_stroke(pt, 5.0, Stroke::new(1.5_f32, reticle_color));

                // Crosshair ticks
                painter.line_segment([Pos2::new(pt.x - 22.0, pt.y), Pos2::new(pt.x - 7.0, pt.y)], Stroke::new(1.5_f32, reticle_color));
                painter.line_segment([Pos2::new(pt.x + 7.0, pt.y), Pos2::new(pt.x + 22.0, pt.y)], Stroke::new(1.5_f32, reticle_color));
                painter.line_segment([Pos2::new(pt.x, pt.y - 22.0), Pos2::new(pt.x, pt.y - 7.0)], Stroke::new(1.5_f32, reticle_color));
                painter.line_segment([Pos2::new(pt.x, pt.y + 7.0), Pos2::new(pt.x, pt.y + 22.0)], Stroke::new(1.5_f32, reticle_color));

                painter.text(
                    Pos2::new(pt.x, pt.y + 26.0),
                    egui::Align2::CENTER_TOP,
                    format!("⊕ {} ({})", telem.scope_model.display_short(), crate::i18n::tr(telem.state.display_fi())),
                    egui::FontId::proportional(12.0),
                    reticle_color,
                );
            }
        }

        // Draw Mosaic Framing Grid if enabled
        if state.mosaic_enabled {
            let px_per_deg = match state.map_mode {
                StarMapMode::ZenithAllSky => (radius / 90.0) * state.map_zoom,
                StarMapMode::Horizon => {
                    let fov_rad = state.horizon_fov_deg.to_radians();
                    let f = 0.5 / (fov_rad * 0.5).tan();
                    (rect.height() as f64 * f * (std::f64::consts::PI / 180.0)) as f32
                }
            };

            for panel in &state.mosaic_plan.panels {
                let (p_alt, p_az) = ra_dec_to_alt_az(panel.ra_hours, panel.dec_deg, lat, lon, &now_utc);
                if let Some(panel_center) = Self::project_star_point(
                    state.map_mode,
                    p_alt,
                    p_az,
                    rect,
                    center,
                    radius,
                    state.view_az,
                    state.view_alt,
                    state.horizon_fov_deg,
                ) {
                    if rect.expand(60.0).contains(panel_center) {
                        let panel_w = (panel.fov_w_deg as f32 * px_per_deg).clamp(18.0, 400.0);
                        let panel_h = (panel.fov_h_deg as f32 * px_per_deg).clamp(12.0, 300.0);
                        let panel_rect = Rect::from_center_size(panel_center, Vec2::new(panel_w, panel_h));

                        painter.rect_filled(panel_rect, 2.0, Color32::from_rgba_unmultiplied(30, 160, 200, 35));
                        painter.rect_stroke(
                            panel_rect,
                            2.0,
                            Stroke::new(1.5_f32, Color32::from_rgba_unmultiplied(70, 220, 210, 200)),
                            egui::StrokeKind::Inside,
                        );

                        let label = format!("#{}/{}", panel.panel_index, panel.total_panels);
                        painter.text(
                            panel_center,
                            egui::Align2::CENTER_CENTER,
                            label,
                            egui::FontId::proportional(11.0),
                            Color32::from_rgb(120, 255, 230),
                        );
                    }
                }
            }
        }

        // Bottom HUD overlay for 3D Horizon Dome
        if state.map_mode == StarMapMode::Horizon {
            let az_name = match state.view_az {
                az if az >= 337.5 || az < 22.5 => crate::i18n::tr("P (Pohjoinen)"),
                az if az < 67.5 => crate::i18n::tr("KO (Koillinen)"),
                az if az < 112.5 => crate::i18n::tr("I (Itä)"),
                az if az < 157.5 => crate::i18n::tr("KA (Kaakko)"),
                az if az < 202.5 => crate::i18n::tr("E (Etelä)"),
                az if az < 247.5 => crate::i18n::tr("LO (Lounas)"),
                az if az < 292.5 => crate::i18n::tr("L (Länsi)"),
                _ => crate::i18n::tr("LU (Luode)"),
            };

            let hud_text = format!(
                "🧭 Suunta: {:.0}° ({}) | Korkeus: {:.0}° | FOV: {:.0}°  •  {}",
                state.view_az,
                az_name,
                state.view_alt,
                state.horizon_fov_deg,
                crate::i18n::tr("Raahaa kääntääksesi • Rullaa zoomataksesi"),
            );
            painter.text(
                Pos2::new(rect.left() + 12.0, rect.bottom() - 12.0),
                egui::Align2::LEFT_BOTTOM,
                hud_text,
                egui::FontId::proportional(11.5),
                Color32::from_rgba_unmultiplied(190, 215, 240, 210),
            );
        }

        // Apply clicked selection
        if let Some(clicked) = clicked_object {
            state.selected_object = Some(clicked);
            state.recompute_mosaic();
        }

        // Render hover tooltip
        if let Some(h) = hovered {
            let (alt, az) = ra_dec_to_alt_az(h.ra_hours, h.dec_deg, lat, lon, &now_utc);
            response.on_hover_ui(|ui| {
                ui.heading(format!("{} {}", h.obj_type.icon(), h.name));
                ui.label(format!("{}: {} | {}: {}", crate::i18n::tr("Katalogi:"), h.id, crate::i18n::tr("Tähdistö:"), h.constellation));
                ui.label(format!("{}: {:.1}° | {}: {:.1}°", crate::i18n::tr("Korkeus (Alt):"), alt, crate::i18n::tr("Atsimuutti:"), az));
                ui.label(format!("{}: {:.1} | {}: {}", crate::i18n::tr("Magnitudi:"), h.magnitude, crate::i18n::tr("Suodatin:"), h.filter_recommendation));
                ui.label(RichText::new(crate::i18n::tr("Klikkaa valitaksesi kohteen")).small().color(Color32::from_rgb(120, 200, 255)));
            });
        }
    }

    /// Draws a compact 24h altitude profile widget
    fn draw_altitude_profile_widget(ui: &mut egui::Ui, profile: &[(String, f64)]) {
        let (response, painter) = ui.allocate_painter(Vec2::new(ui.available_width().max(180.0), 65.0), egui::Sense::hover());
        let rect = response.rect;

        painter.rect_filled(rect, 3.0, Color32::from_rgb(14, 18, 26));
        painter.rect_stroke(
            rect,
            3.0,
            Stroke::new(1.0_f32, Color32::from_rgb(35, 45, 65)),
            egui::StrokeKind::Inside,
        );

        // Horizontal guidelines (0° horizon, 30° min good imaging alt)
        let y_0 = rect.bottom() - 6.0;
        let y_30 = rect.bottom() - 6.0 - (30.0 / 90.0) * (rect.height() - 14.0);
        let y_60 = rect.bottom() - 6.0 - (60.0 / 90.0) * (rect.height() - 14.0);

        painter.line_segment([Pos2::new(rect.left(), y_0), Pos2::new(rect.right(), y_0)], Stroke::new(1.0_f32, Color32::from_rgb(50, 60, 80)));
        painter.line_segment([Pos2::new(rect.left(), y_30), Pos2::new(rect.right(), y_30)], Stroke::new(1.0_f32, Color32::from_rgb(40, 70, 60)));
        painter.line_segment([Pos2::new(rect.left(), y_60), Pos2::new(rect.right(), y_60)], Stroke::new(1.0_f32, Color32::from_rgb(35, 45, 60)));

        painter.text(Pos2::new(rect.left() + 4.0, y_30 - 2.0), egui::Align2::LEFT_BOTTOM, "30°", egui::FontId::proportional(9.0), Color32::from_rgb(80, 160, 100));
        painter.text(Pos2::new(rect.left() + 4.0, y_60 - 2.0), egui::Align2::LEFT_BOTTOM, "60°", egui::FontId::proportional(9.0), Color32::from_rgb(70, 90, 120));

        if profile.is_empty() {
            return;
        }

        // Connect points
        let step_x = rect.width() / (profile.len().saturating_sub(1) as f32).max(1.0);
        let mut pts = Vec::with_capacity(profile.len());

        let mut max_alt = -90.0f64;
        let mut max_time = "";

        for (i, (time_str, alt)) in profile.iter().enumerate() {
            let x = rect.left() + i as f32 * step_x;
            let alt_clamped = alt.clamp(0.0, 90.0) as f32;
            let y = y_0 - (alt_clamped / 90.0) * (rect.height() - 14.0);
            pts.push(Pos2::new(x, y));

            if *alt > max_alt {
                max_alt = *alt;
                max_time = time_str;
            }
        }

        painter.add(egui::Shape::line(pts, Stroke::new(2.0_f32, Color32::from_rgb(80, 200, 255))));

        // Print peak
        painter.text(
            Pos2::new(rect.right() - 4.0, rect.top() + 4.0),
            egui::Align2::RIGHT_TOP,
            format!("{}: {:.0}° ({})", crate::i18n::tr("Huippu:"), max_alt, max_time),
            egui::FontId::proportional(10.0),
            Color32::from_rgb(255, 220, 100),
        );
    }
}
