use crate::telescope::catalog::{DsoType, SkyObject};
use crate::telescope::coordinates::{
    alt_az_to_canvas_normalized, format_dec, format_ra, night_altitude_profile, ra_dec_to_alt_az,
};
use crate::telescope::downloader::LiveIngestManager;
use crate::telescope::mosaic::{MosaicGridConfig, MosaicPlan, MosaicScopeModel};
use crate::telescope::seestar::{SeestarClient, SeestarState};
use crate::telescope::sequencer::{JobStatus, NightSequencer};
use chrono::Utc;
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Stroke, Vec2};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TelescopeCenterView {
    StarMap,
    Sequencer,
    ImageViewer,
}

pub struct TelescopeViewState {
    pub center_view: TelescopeCenterView,
    pub seestar_ip: String,
    pub selected_object: Option<SkyObject>,
    pub search_query: String,
    pub filter_type: Option<DsoType>,
    pub map_zoom: f32,
    pub map_pan: Vec2,
    pub map_fill_width: bool,
    pub sequence_duration_mins: u32,
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
}

impl Default for TelescopeViewState {
    fn default() -> Self {
        let mosaic_config = MosaicGridConfig::default();
        let mosaic_plan = MosaicPlan::compute(&mosaic_config);

        Self {
            center_view: TelescopeCenterView::StarMap,
            seestar_ip: "192.168.110.1".to_string(),
            selected_object: None,
            search_query: String::new(),
            filter_type: None,
            map_zoom: 1.0,
            map_pan: Vec2::ZERO,
            map_fill_width: true,
            sequence_duration_mins: 60,
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
        }
    }
}

impl TelescopeViewState {
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
        ui.set_width(ui.available_width());

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
            ui.set_width(ui.available_width());
            let (sr, sg, sb) = telem.state.color_rgb();
            let arrow = if state.section_connection_open { "▼" } else { "▶" };
            let scope_header = format!("{} 🔭 {}", arrow, telem.scope_model.display_short());

            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(RichText::new(scope_header).color(Color32::from_rgb(80, 200, 255)).strong()).frame(false)).clicked() {
                    state.section_connection_open = !state.section_connection_open;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("● {}", telem.state.display_fi())).color(Color32::from_rgb(sr, sg, sb)).strong().small());
                });
            });

            if state.section_connection_open {
                ui.add_space(4.0);

                // Scope model selector
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(crate::i18n::tr("Kaukoputki:")).strong());
                    let mut current_model = telem.scope_model;
                    let combo_w = (ui.available_width() - 8.0).max(180.0);
                    egui::ComboBox::from_id_salt("sidebar_telescope_model_select")
                        .width(combo_w)
                        .selected_text(current_model.display_short())
                        .show_ui(ui, |ui| {
                            ui.label(RichText::new("── ZWO Seestar ──").small().color(Color32::from_rgb(100, 200, 255)));
                            ui.selectable_value(&mut current_model, MosaicScopeModel::SeestarS50, "ZWO Seestar S-50 (250mm f/5, 1.3° × 0.7°)");
                            ui.selectable_value(&mut current_model, MosaicScopeModel::SeestarS50Pro, "ZWO Seestar S-50 Pro (260mm f/5.2 4K, 2.5° × 1.4°)");
                            ui.selectable_value(&mut current_model, MosaicScopeModel::SeestarS30, "ZWO Seestar S-30 (150mm f/5, 2.1° × 1.2°)");
                            ui.selectable_value(&mut current_model, MosaicScopeModel::SeestarS30Pro, "ZWO Seestar S-30 Pro (160mm f/5.3 4K, 4.0° × 2.2°)");

                            ui.separator();
                            ui.label(RichText::new("── DwarfLab ──").small().color(Color32::from_rgb(255, 180, 100)));
                            ui.selectable_value(&mut current_model, MosaicScopeModel::Dwarf3, "DwarfLab DWARF 3 (150mm f/4.3 4K, 2.9° × 1.7°)");
                            ui.selectable_value(&mut current_model, MosaicScopeModel::DwarfMini, "DwarfLab DWARF Mini (150mm f/5, 2.1° × 1.2°)");
                            ui.selectable_value(&mut current_model, MosaicScopeModel::Dwarf2, "DwarfLab DWARF II (100mm f/4.2 4K, 3.2° × 1.8°)");
                            ui.selectable_value(&mut current_model, MosaicScopeModel::DwarfDraco, "DwarfLab DRACO (340mm f/3.8, 90mm aukko, 1.6° × 1.2°)");

                            ui.separator();
                            ui.label(RichText::new("── Vaonis ──").small().color(Color32::from_rgb(200, 160, 255)));
                            ui.selectable_value(&mut current_model, MosaicScopeModel::Vespera2, "Vaonis Vespera II (250mm f/5 4K, 2.6° × 1.4°)");
                            ui.selectable_value(&mut current_model, MosaicScopeModel::VesperaPro, "Vaonis Vespera Pro (250mm f/5 12.5MP, 1.6° × 1.6°)");

                            ui.separator();
                            ui.label(RichText::new("── Celestron ──").small().color(Color32::from_rgb(255, 220, 120)));
                            ui.selectable_value(&mut current_model, MosaicScopeModel::CelestronOrigin, "Celestron Origin RASA (335mm f/2.2, 1.3° × 0.8°)");

                            ui.separator();
                            ui.label(RichText::new("── Universaali / Oma ──").small().color(Color32::from_rgb(180, 220, 180)));
                            ui.selectable_value(&mut current_model, MosaicScopeModel::AscomAlpaca, "ASCOM Alpaca (Universal REST Mount)");
                            ui.selectable_value(&mut current_model, MosaicScopeModel::Custom, crate::i18n::tr("Oma määritys"));
                        });
                    if current_model != telem.scope_model {
                        client.set_model(current_model);
                        state.mosaic_config.scope_model = current_model;
                        state.seestar_ip = current_model.default_ip().to_string();
                        ingest_mgr.set_model_source_path(current_model);
                        state.recompute_mosaic();
                    }
                });

                // Specs badge
                let (fov_w, fov_h) = telem.scope_model.fov_deg();
                ui.label(
                    RichText::new(format!(
                        "📐 {}mm f/{:.1} | Kenno: {} | {} | FOV: {:.2}° × {:.2}°",
                        telem.scope_model.focal_length_mm(),
                        telem.scope_model.focal_ratio(),
                        telem.scope_model.sensor_name(),
                        telem.scope_model.protocol().protocol_name(),
                        fov_w,
                        fov_h
                    ))
                    .small()
                    .color(Color32::from_rgb(140, 185, 215)),
                );
                ui.add_space(2.0);

                // Connection controls
                ui.horizontal(|ui| {
                    ui.label("IP:");
                    ui.add(egui::TextEdit::singleline(&mut state.seestar_ip).desired_width(120.0));

                    if telem.connected || telem.state == SeestarState::Reconnecting {
                        if ui.button(crate::i18n::tr("Katkaise")).clicked() {
                            client.disconnect();
                        }
                    } else if ui.button(crate::i18n::tr("🔌 Yhdistä")).clicked() {
                        client.connect(&state.seestar_ip);
                    }

                    let mut sim = telem.is_simulation;
                    if ui.checkbox(&mut sim, crate::i18n::tr("🎮 Simulaatio")).on_hover_text(crate::i18n::tr("Kokeile ohjausta ja karttaa ilman kaukoputkea")).changed() {
                        client.set_simulation(sim);
                    }
                });

                if telem.scope_model.brand() == "ZWO Seestar" {
                    ui.add_space(2.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(crate::i18n::tr(
                                "📡 ASCOM Alpaca -ohjaus (portti 32323): Ota Alpaca päälle Seestar-sovelluksesta (Profiili -> Laitehallinta -> Lisätoiminnot -> Seestar Alpaca: Päällä)."
                            ))
                            .small()
                            .color(Color32::from_rgb(100, 220, 160))
                        );
                    });
                    ui.horizontal(|ui| {
                        if ui.button(RichText::new(crate::i18n::tr("📁 Yhdistä kuvat Macille (SMB)")).color(Color32::from_rgb(100, 220, 255)).strong())
                            .on_hover_text(crate::i18n::tr("Avaa Seestarin jaettu kansio suoraan Finderissä (smb://<IP>/EMMC Images). Tällöin PhotonForge pinoaa ja venyttää kuvat reaaliajassa Macilla!"))
                            .clicked()
                        {
                            LiveIngestManager::open_seestar_smb(&state.seestar_ip);
                        }
                    });
                    ui.add_space(2.0);
                }

                ui.horizontal(|ui| {
                    let mut auto_reconn = telem.auto_reconnect_enabled;
                    if ui
                        .checkbox(&mut auto_reconn, crate::i18n::tr("🛡 Automaattinen uudelleenyhdistys"))
                        .on_hover_text(crate::i18n::tr("Palauttaa TCP-yhteyden automaattisesti eksponentiaalisella viiveellä Wi-Fi-katkoksen sattuessa"))
                        .changed()
                    {
                        client.set_auto_reconnect(auto_reconn);
                    }
                    if telem.state == SeestarState::Reconnecting {
                        ui.label(
                            RichText::new(format!("(Yritys {}/{})", telem.reconnect_attempts, telem.max_reconnect_attempts))
                                .small()
                                .color(Color32::from_rgb(255, 170, 60)),
                        );
                    }
                });

                ui.add_space(4.0);

                // Telemetry indicators
                ui.horizontal(|ui| {
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
                        format!("🔍 Tarkennus: {} ({:.1} °C)", telem.focus_position, ftemp)
                    } else {
                        format!("🔍 Tarkennus: {}", telem.focus_position)
                    };
                    ui.label(RichText::new(focus_str).small().color(Color32::from_rgb(180, 215, 245)));
                    ui.separator();
                    ui.label(
                        RichText::new(format!(
                            "🔀 Dither: {} ({} kpl)",
                            if telem.dither_config.enabled { crate::i18n::tr("Päällä") } else { crate::i18n::tr("Pois") },
                            telem.dither_count
                        ))
                        .small()
                        .color(Color32::from_rgb(190, 175, 245)),
                    );
                });

                ui.add_space(2.0);

                // Scope current position
                let (scope_alt, scope_az) = ra_dec_to_alt_az(telem.current_ra, telem.current_dec, lat, lon, &now_utc);
                ui.label(
                    RichText::new(format!(
                        "Suunta: RA {} | Dec {}\nKorkeus: {:.1}° | Atsimuutti: {:.1}°",
                        format_ra(telem.current_ra),
                        format_dec(telem.current_dec),
                        scope_alt,
                        scope_az
                    ))
                    .size(11.5)
                    .color(Color32::from_rgb(170, 185, 210)),
                );

                ui.add_space(4.0);

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
                        if ui.button(crate::i18n::tr("🚀 Avaa kaukoputki")).on_hover_text(crate::i18n::tr("Avaa kaukoputki lepoasennosta")).clicked() {
                            client.unpark();
                        }
                    } else if ui.button(crate::i18n::tr("🏠 Parkkiin")).on_hover_text(crate::i18n::tr("Aja kaukoputki turva-asentoon")).clicked() {
                        client.park();
                    }
                });

                if !telem.last_log_message.is_empty() {
                    ui.label(RichText::new(format!("Tila: {}", telem.last_log_message)).small().color(Color32::from_rgb(140, 190, 240)));
                }
            }
        });

        ui.add_space(6.0);

        // 2. LIVE FRAME AUTO-DOWNLOAD (INGEST)
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            let mut cfg = ingest_mgr.config.lock().unwrap();
            let st = ingest_mgr.status.lock().unwrap().clone();
            let arrow = if state.section_ingest_open { "▼" } else { "▶" };

            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(RichText::new(format!("{} 📥 Kuvien lataus Macille", arrow)).color(Color32::from_rgb(100, 220, 255)).strong()).frame(false)).clicked() {
                    state.section_ingest_open = !state.section_ingest_open;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if st.is_active {
                        ui.label(RichText::new(format!("🟢 {} kpl", st.downloaded_count)).color(Color32::from_rgb(100, 255, 140)).strong().small());
                    } else {
                        ui.label(RichText::new(crate::i18n::tr("⚪ Ei yhteyttä")).color(Color32::from_rgb(150, 160, 180)).small());
                    }
                });
            });

            if state.section_ingest_open {
                ui.add_space(2.0);
                ui.checkbox(&mut cfg.enabled, crate::i18n::tr("✅ Tallenna uudet ruudut automaattisesti Macille"));

                ui.horizontal(|ui| {
                    let dest_str = cfg.destination_path.to_string_lossy().to_string();
                    let short_dest = if dest_str.len() > 30 {
                        format!("...{}", &dest_str[dest_str.len() - 27..])
                    } else {
                        dest_str
                    };
                    ui.label(RichText::new(format!("Kohde: {}", short_dest)).small().color(Color32::from_rgb(200, 210, 230)));

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
                            .on_hover_text(crate::i18n::tr("Avaa Seestarin jaettu kansio suoraan Finderissä (smb://<IP>/EMMC Images)"))
                            .clicked()
                        {
                            LiveIngestManager::open_seestar_smb(&state.seestar_ip);
                        }
                    } else {
                        ui.label(RichText::new(format!("📁 Jako: {}", share_hint)).small().color(Color32::from_rgb(170, 200, 230)));
                    }
                });

                if !st.status_message.is_empty() {
                    ui.label(RichText::new(&st.status_message).small().color(Color32::from_rgb(160, 180, 210)));
                }
            }
        });

        ui.add_space(6.0);

        // 3. SELECTED TARGET CARD & NIGHT ALTITUDE GRAPH
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            let arrow = if state.section_target_open { "▼" } else { "▶" };
            let target_badge = state.selected_object.as_ref().map(|o| {
                let (alt, _) = ra_dec_to_alt_az(o.ra_hours, o.dec_deg, lat, lon, &now_utc);
                format!("{} (Alt: {:.0}°)", o.id, alt)
            });

            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(RichText::new(format!("{} 🌌 Valittu kohde", arrow)).color(Color32::from_rgb(220, 230, 255)).strong()).frame(false)).clicked() {
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
                    ui.horizontal(|ui| {
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

                    ui.horizontal(|ui| {
                        ui.label(format!("Tyyppi: {} {}", target.obj_type.icon(), target.obj_type.name_fi()));
                        ui.separator();
                        ui.label(format!("Mag: {:.1}", target.magnitude));
                    });

                    ui.horizontal(|ui| {
                        ui.label(format!("Korkeus: {:.1}°", cur_alt));
                        ui.colored_label(alt_quality.1, alt_quality.0);
                    });

                    ui.label(format!("Suunta (Atsimuutti): {:.1}° | Tähdistö: {}", cur_az, target.constellation));
                    ui.label(RichText::new(format!("Suodatin: {}", target.filter_recommendation)).small().color(Color32::from_rgb(140, 220, 255)));

                    ui.add_space(4.0);

                    // 24h Night Altitude Plot
                    ui.label(RichText::new(crate::i18n::tr("📈 Korkeuskäyrä tänä yönä (klo 18 - 06):")).small().strong());
                    let profile = night_altitude_profile(target.ra_hours, target.dec_deg, lat, lon, &now_utc);
                    Self::draw_altitude_profile_widget(ui, &profile);

                    ui.add_space(4.0);

                    // Add to Sequencer Row
                    ui.horizontal(|ui| {
                        ui.label(crate::i18n::tr("Kesto:"));
                        ui.add(egui::DragValue::new(&mut state.sequence_duration_mins).range(5..=360).suffix(" min"));
                        ui.checkbox(&mut state.sequence_use_lp, crate::i18n::tr("LP-suodatin"));

                        if ui.button(crate::i18n::tr("➕ Lisää sekvensseriin")).clicked() {
                            sequencer.add_target(target, state.sequence_duration_mins, state.sequence_use_lp);
                        }
                    });
                } else {
                    ui.label(RichText::new(crate::i18n::tr("Klikkaa kohdetta tähtikartalta tai valitse alta haku.")).color(Color32::from_rgb(150, 160, 180)));
                }
            }
        });

        ui.add_space(6.0);

        // 4. TARGET SEARCH & CATALOG LIST
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            let arrow = if state.section_search_open { "▼" } else { "▶" };

            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(RichText::new(format!("{} 🔍 Etsi taivaankappaletta", arrow)).color(Color32::from_rgb(180, 210, 240)).strong()).frame(false)).clicked() {
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
            ui.set_width(ui.available_width());
            let arrow = if state.section_mosaic_open { "▼" } else { "▶" };
            let panel_summary = format!("{} ruutua ({:.1} deg²)", state.mosaic_plan.panels.len(), state.mosaic_plan.total_area_sq_deg);

            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(RichText::new(format!("{} 🧩 Mosaiikkikuvaus", arrow)).color(Color32::from_rgb(120, 220, 200)).strong()).frame(false)).clicked() {
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
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(panel_summary).color(Color32::from_rgb(130, 240, 200)).small());
                    });
                }
            });

            if state.section_mosaic_open && state.mosaic_enabled {
                ui.add_space(2.0);

                // Scope model selector
                ui.horizontal_wrapped(|ui| {
                    ui.label(crate::i18n::tr("Kaukoputki:"));
                    let prev_model = state.mosaic_config.scope_model;
                    let combo_w = (ui.available_width() - 12.0).max(180.0);
                    egui::ComboBox::from_id_salt("mosaic_scope_model_combo")
                        .width(combo_w)
                        .selected_text(state.mosaic_config.scope_model.display_short())
                        .show_ui(ui, |ui| {
                            ui.label(RichText::new("── ZWO Seestar ──").small().color(Color32::from_rgb(100, 200, 255)));
                            ui.selectable_value(&mut state.mosaic_config.scope_model, MosaicScopeModel::SeestarS50, "ZWO Seestar S-50 (250mm f/5, 1.3° × 0.7°)");
                            ui.selectable_value(&mut state.mosaic_config.scope_model, MosaicScopeModel::SeestarS50Pro, "ZWO Seestar S-50 Pro (260mm f/5.2 4K, 2.5° × 1.4°)");
                            ui.selectable_value(&mut state.mosaic_config.scope_model, MosaicScopeModel::SeestarS30, "ZWO Seestar S-30 (150mm f/5, 2.1° × 1.2°)");
                            ui.selectable_value(&mut state.mosaic_config.scope_model, MosaicScopeModel::SeestarS30Pro, "ZWO Seestar S-30 Pro (160mm f/5.3 4K, 4.0° × 2.2°)");

                            ui.separator();
                            ui.label(RichText::new("── DwarfLab ──").small().color(Color32::from_rgb(255, 180, 100)));
                            ui.selectable_value(&mut state.mosaic_config.scope_model, MosaicScopeModel::Dwarf3, "DwarfLab DWARF 3 (150mm f/4.3 4K, 2.9° × 1.7°)");
                            ui.selectable_value(&mut state.mosaic_config.scope_model, MosaicScopeModel::DwarfMini, "DwarfLab DWARF Mini (150mm f/5, 2.1° × 1.2°)");
                            ui.selectable_value(&mut state.mosaic_config.scope_model, MosaicScopeModel::Dwarf2, "DwarfLab DWARF II (100mm f/4.2 4K, 3.2° × 1.8°)");
                            ui.selectable_value(&mut state.mosaic_config.scope_model, MosaicScopeModel::DwarfDraco, "DwarfLab DRACO (340mm f/3.8, 90mm aukko, 1.6° × 1.2°)");

                            ui.separator();
                            ui.label(RichText::new("── Vaonis ──").small().color(Color32::from_rgb(200, 160, 255)));
                            ui.selectable_value(&mut state.mosaic_config.scope_model, MosaicScopeModel::Vespera2, "Vaonis Vespera II (250mm f/5 4K, 2.6° × 1.4°)");
                            ui.selectable_value(&mut state.mosaic_config.scope_model, MosaicScopeModel::VesperaPro, "Vaonis Vespera Pro (250mm f/5 12.5MP, 1.6° × 1.6°)");

                            ui.separator();
                            ui.label(RichText::new("── Celestron ──").small().color(Color32::from_rgb(255, 220, 120)));
                            ui.selectable_value(&mut state.mosaic_config.scope_model, MosaicScopeModel::CelestronOrigin, "Celestron Origin RASA (335mm f/2.2, 1.3° × 0.8°)");

                            ui.separator();
                            ui.label(RichText::new("── Universaali / Oma ──").small().color(Color32::from_rgb(180, 220, 180)));
                            ui.selectable_value(&mut state.mosaic_config.scope_model, MosaicScopeModel::AscomAlpaca, "ASCOM Alpaca (Universal REST Mount)");
                            ui.selectable_value(&mut state.mosaic_config.scope_model, MosaicScopeModel::Custom, crate::i18n::tr("Oma määritys"));
                        });
                    if prev_model != state.mosaic_config.scope_model {
                        state.recompute_mosaic();
                    }
                });

                if state.mosaic_config.scope_model == MosaicScopeModel::Custom {
                    ui.group(|ui| {
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
                ui.label(RichText::new(format!("Keskipiste: {}", center_label)).small().color(Color32::from_rgb(160, 210, 240)));

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
                ui.label(RichText::new(format!(
                    "📐 Kokonaisala: {:.2}° × {:.2}° ({:.1} deg²) | Ruutuja: {} kpl",
                    plan.total_fov_w_deg, plan.total_fov_h_deg, plan.total_area_sq_deg, plan.panels.len()
                )).small().color(Color32::from_rgb(130, 240, 200)));

                ui.label(RichText::new(format!(
                    "⏱ Kokonaisvalotusaika yöllä: {} h {:02} min",
                    plan.total_exposure_mins / 60, plan.total_exposure_mins % 60
                )).small().color(Color32::from_rgb(255, 220, 140)));

                ui.add_space(2.0);

                if ui.add(egui::Button::new(RichText::new(crate::i18n::tr("📋 Lisää mosaiikin ruudut yösekvenssiin")).strong().color(Color32::from_rgb(100, 255, 200))).min_size(Vec2::new(ui.available_width(), 26.0))).clicked() {
                    let targets = state.mosaic_plan.to_sequence_targets();
                    for t in targets {
                        sequencer.add_sequence_target(t);
                    }
                }

                ui.add_space(2.0);
                if ui.add(egui::Button::new(RichText::new(crate::i18n::tr("🧩 Avaa mosaiikkipaneelien yhdistäjä")).strong().color(Color32::from_rgb(130, 210, 255))).min_size(Vec2::new(ui.available_width(), 26.0)))
                    .on_hover_text(crate::i18n::tr("Siirry Pinoaminen -> Mosaiikit -välilehdelle yhdistämään otettuja ruutuja yhtenäiseksi kuvaksi"))
                    .clicked()
                {
                    state.request_mosaic_stitcher = true;
                }
            } else if state.section_mosaic_open && !state.mosaic_enabled {
                ui.label(RichText::new(crate::i18n::tr("Kytke mosaiikkikuvaus päälle rastittamalla 'Käytössä' yltä.")).small().color(Color32::from_rgb(150, 160, 180)));
            }
        });

        ui.add_space(6.0);

        // 6. NIGHT SEQUENCER (N.I.N.A. SCHEDULER)
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            let seq_state = sequencer.state.lock().unwrap().clone();
            let arrow = if state.section_sequencer_open { "▼" } else { "▶" };

            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(RichText::new(format!("{} 📅 Yösekvensseri", arrow)).color(Color32::from_rgb(255, 200, 80)).strong()).frame(false)).clicked() {
                    state.section_sequencer_open = !state.section_sequencer_open;
                }
                if seq_state.is_running {
                    ui.label(RichText::new(crate::i18n::tr("● KÄYNNISSÄ")).color(Color32::from_rgb(100, 255, 120)).strong().small());
                } else {
                    ui.label(RichText::new(crate::i18n::tr("● Valmiustila")).color(Color32::from_rgb(150, 160, 180)).small());
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button(crate::i18n::tr("⛶ Päänäkymä")).on_hover_text(crate::i18n::tr("Avaa koko yösekvensserin iso hallintapöytä päänäkymään")).clicked() {
                        state.center_view = TelescopeCenterView::Sequencer;
                    }
                });
            });

            if state.section_sequencer_open {
                ui.add_space(4.0);

                // Controls
                ui.horizontal(|ui| {
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

                ui.label(RichText::new(&seq_state.current_step_description).strong().size(12.0).color(Color32::from_rgb(255, 230, 120)));

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
                                        .add(egui::DragValue::new(&mut dither_cfg.interval_frames).range(1..=20).prefix("joka ").suffix(". kuva"))
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

                // Target List - Structured 2-row cards with guaranteed visible title and buttons
                if seq_state.targets.is_empty() {
                    ui.label(RichText::new(crate::i18n::tr("Kuvauslista on tyhjä. Valitse kohteita tähtikartalta ja paina 'Lisää sekvensseriin'.")).small().color(Color32::from_rgb(140, 150, 170)));
                } else {
                    for (idx, target) in seq_state.targets.iter().enumerate() {
                        let is_active = seq_state.current_job_idx == Some(idx);
                        ui.group(|ui| {
                            ui.set_width(ui.available_width());

                            // Row 1: Right-to-left delete button + status, left title fills remainder
                            ui.horizontal(|ui| {
                                let title_col = if is_active {
                                    Color32::from_rgb(255, 230, 80)
                                } else if target.status == JobStatus::Completed {
                                    Color32::from_rgb(100, 230, 120)
                                } else {
                                    Color32::from_rgb(200, 220, 245)
                                };

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.small_button("✖").on_hover_text(crate::i18n::tr("Poista sekvenssistä")).clicked() {
                                        sequencer.remove_target(idx);
                                    }
                                    let status_col = match target.status {
                                        JobStatus::Completed => Color32::from_rgb(100, 255, 120),
                                        JobStatus::Stacking => Color32::from_rgb(80, 200, 255),
                                        JobStatus::Focusing => Color32::from_rgb(255, 225, 90),
                                        JobStatus::Dithering => Color32::from_rgb(190, 140, 255),
                                        JobStatus::WaitingConnection => Color32::from_rgb(255, 160, 60),
                                        JobStatus::Failed => Color32::from_rgb(255, 100, 100),
                                        _ => Color32::from_rgb(160, 175, 200),
                                    };
                                    ui.label(RichText::new(target.status.display_fi()).small().color(status_col));

                                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        let prefix = if is_active { "▶ " } else { "" };
                                        let label_str = format!("{}{}. {}", prefix, idx + 1, target.name);
                                        ui.add(egui::Label::new(RichText::new(&label_str).strong().color(title_col)).truncate())
                                            .on_hover_text(&target.name);
                                    });
                                });
                            });

                            // Row 2: Metadata Badges
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(format!("⏱ {} min", target.duration_minutes)).small().color(Color32::from_rgb(255, 220, 120)));
                                ui.label(RichText::new("•").small().color(Color32::from_rgb(100, 110, 130)));
                                let filt_label = if target.use_lp_filter { "🌙 LP" } else { "☀️ UV/IR" };
                                ui.label(RichText::new(filt_label).small().color(Color32::from_rgb(130, 220, 255)));
                                ui.label(RichText::new("•").small().color(Color32::from_rgb(100, 110, 130)));
                                let (alt, _) = ra_dec_to_alt_az(target.ra_hours, target.dec_deg, lat, lon, &now_utc);
                                ui.label(RichText::new(format!("📐 Alt: {:.0}°", alt)).small().color(Color32::from_rgb(170, 185, 210)));
                            });
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
        _state: &mut TelescopeViewState,
        sequencer: &Arc<NightSequencer>,
        client: &Arc<SeestarClient>,
        lat: f64,
        lon: f64,
    ) {
        let seq_state = sequencer.state.lock().unwrap().clone();
        let telem = client.get_telemetry();
        let now_utc = Utc::now();
        ui.set_width(ui.available_width());

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
                        ui.label(RichText::new(format!("Kaukoputki: {}", telem.state.display_fi())).color(Color32::from_rgb(sr, sg, sb)).strong());
                        ui.separator();
                        ui.label(format!("Akku: {}%", telem.battery_pct));
                        ui.separator();
                        ui.label(format!("Lämpötila: {:.1} °C", telem.temp_c));
                        ui.separator();
                        ui.label(format!("Tarkennus: {} (AF: {} krt)", telem.focus_position, seq_state.af_runs_count));
                        ui.separator();
                        ui.label(format!("Ditheröinnit: {} kpl", seq_state.dithers_count));
                        ui.separator();
                        let total_mins: u32 = seq_state.targets.iter().map(|t| t.duration_minutes).sum();
                        ui.label(RichText::new(format!("Kokonaisaika: {} h {:02} min ({} kohdetta)", total_mins / 60, total_mins % 60, seq_state.targets.len())).color(Color32::from_rgb(255, 220, 120)));
                    });

                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&seq_state.current_step_description).strong().color(Color32::from_rgb(255, 230, 140)));
                        let mut auto_park = seq_state.auto_park_at_end;
                        if ui.checkbox(&mut auto_park, crate::i18n::tr("🏠 Parkkeeraa aamulla automaattisesti")).changed() {
                            sequencer.state.lock().unwrap().auto_park_at_end = auto_park;
                        }
                    });
                });

                ui.add_space(6.0);

                // Night Automation & Resilience Panel (3-column layout)
                ui.group(|ui| {
                    ui.set_width(ui.available_width());
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
                            ui.set_width(ui.available_width());
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
                                format!("Viimeisin AF: {:.1} °C ({} min sitten)", last_t, seq_state.minutes_since_last_af)
                            } else {
                                crate::i18n::tr("Viimeisin AF: Ei vielä suoritettu yöajossa").to_string()
                            };
                            ui.label(RichText::new(af_info).small().color(Color32::from_rgb(160, 180, 205)));
                        });

                        // Column 2: Dithering
                        cols[1].group(|ui| {
                            ui.set_width(ui.available_width());
                            dither_changed |= ui
                                .checkbox(
                                    &mut dither_cfg.enabled,
                                    RichText::new(crate::i18n::tr("🔀 Ditheröinti")).strong().color(Color32::from_rgb(195, 155, 255)),
                                )
                                .changed();
                            ui.horizontal(|ui| {
                                ui.label(crate::i18n::tr("Dither-väli:"));
                                dither_changed |= ui
                                    .add(egui::DragValue::new(&mut dither_cfg.interval_frames).range(1..=20).prefix("joka ").suffix(". ruutu"))
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
                                RichText::new(format!("Suoritetut ditheröinnit: {} kpl", seq_state.dithers_count))
                                    .small()
                                    .color(Color32::from_rgb(160, 180, 205)),
                            );
                        });

                        // Column 3: Connection Guard & Recovery
                        cols[2].group(|ui| {
                            ui.set_width(ui.available_width());
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
                                format!("🟢 Yhteys kunnossa (toipumiset: {} krt)", seq_state.reconnect_events_count)
                            } else {
                                format!(
                                    "🟠 Palautetaan yhteyttä (yritys {}/{})",
                                    telem.reconnect_attempts, telem.max_reconnect_attempts
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

                // Target Sequence Table / Cards
                ui.heading(RichText::new(crate::i18n::tr("Kuvauslista")).size(16.0));

                if seq_state.targets.is_empty() {
                    ui.group(|ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space(20.0);
                            ui.label(RichText::new(crate::i18n::tr("Kuvauslista on tyhjä.")).size(16.0).color(Color32::from_rgb(160, 170, 190)));
                            ui.label(crate::i18n::tr("Valitse kohteita tähtikartalta tai luo mosaiikki ja lisää se sekvenssiin."));
                            ui.add_space(20.0);
                        });
                    });
                } else {
                    for (idx, target) in seq_state.targets.iter().enumerate() {
                        let is_active = seq_state.current_job_idx == Some(idx);

                        ui.group(|ui| {
                            ui.set_width(ui.available_width());

                            // Row 1: Right-aligned delete button and status badge, left fills with Title
                            ui.horizontal(|ui| {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button(crate::i18n::tr("✖ Poista")).clicked() {
                                        sequencer.remove_target(idx);
                                    }

                                    let status_color = match target.status {
                                        JobStatus::Completed => Color32::from_rgb(100, 255, 120),
                                        JobStatus::Stacking => Color32::from_rgb(80, 200, 255),
                                        JobStatus::Focusing => Color32::from_rgb(255, 225, 90),
                                        JobStatus::Dithering => Color32::from_rgb(190, 140, 255),
                                        JobStatus::WaitingConnection => Color32::from_rgb(255, 160, 60),
                                        JobStatus::Slewing | JobStatus::Solving => Color32::from_rgb(255, 210, 80),
                                        JobStatus::Failed => Color32::from_rgb(255, 100, 100),
                                        JobStatus::Skipped => Color32::from_rgb(150, 160, 180),
                                        JobStatus::Pending => Color32::from_rgb(180, 190, 210),
                                    };
                                    ui.label(RichText::new(target.status.display_fi()).color(status_color).strong());

                                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        let prefix = if is_active { "▶ " } else { "" };
                                        let title_str = format!("{}{}. {}", prefix, idx + 1, target.name);
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&title_str)
                                                    .color(if is_active { Color32::from_rgb(255, 230, 80) } else { Color32::from_rgb(200, 220, 255) })
                                                    .size(14.0)
                                                    .strong(),
                                            )
                                            .truncate(),
                                        )
                                        .on_hover_text(&target.name);
                                    });
                                });
                            });

                            ui.add_space(3.0);

                            // Row 2: Target Badges / Metrics
                            ui.horizontal_wrapped(|ui| {
                                let (alt, az) = ra_dec_to_alt_az(target.ra_hours, target.dec_deg, lat, lon, &now_utc);
                                ui.label(RichText::new(format!("📐 Korkeus nyt: {:.1}° (Az: {:.0}°)", alt, az)).color(Color32::from_rgb(170, 185, 210)));
                                ui.separator();
                                ui.label(RichText::new(format!("⏱ Kesto: {} min", target.duration_minutes)).color(Color32::from_rgb(255, 220, 120)));
                                ui.separator();
                                let filt_label = if target.use_lp_filter { "Dual-Band LP" } else { "UV/IR Cut" };
                                ui.label(RichText::new(format!("🛡 Suodatin: {}", filt_label)).color(Color32::from_rgb(130, 220, 255)));
                                ui.separator();
                                ui.label(RichText::new(format!("📍 RA: {:.2}h, Dec: {:+.1}°", target.ra_hours, target.dec_deg)).color(Color32::from_rgb(150, 165, 185)));
                            });

                            if is_active && target.duration_minutes > 0 {
                                ui.add_space(4.0);
                                let total_secs = target.duration_minutes * 60;
                                let frac = (target.elapsed_seconds as f32 / total_secs as f32).clamp(0.0, 1.0);
                                ui.add(egui::ProgressBar::new(frac).show_percentage().animate(true));
                            }
                        });
                        ui.add_space(4.0);
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
        ui.set_width(ui.available_width());

        // Top view bar
        ui.horizontal(|ui| {
            ui.heading(RichText::new(crate::i18n::tr("🌌 Interaktiivinen tähtikartta")).color(Color32::from_rgb(80, 200, 255)).strong());
            ui.separator();
            ui.label(format!("Sijainti: {:.2}° N, {:.2}° E | UTC: {}", lat, lon, now_utc.format("%H:%M:%S")));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(crate::i18n::tr("🔍 Nollaa")).on_hover_text(crate::i18n::tr("Palauta keskitetty näkymä ja zoomaus")).clicked() {
                    state.map_zoom = 1.0;
                    state.map_pan = Vec2::ZERO;
                }

                if ui.selectable_label(state.map_fill_width, crate::i18n::tr("⛶ Täytä näkymä"))
                    .on_hover_text(crate::i18n::tr("Sovita tähtikartta täyttämään koko ruudun leveys (poistaa tyhjän mustan alueen)"))
                    .clicked()
                {
                    state.map_fill_width = true;
                    state.map_zoom = 1.0;
                    state.map_pan = Vec2::ZERO;
                }

                if ui.selectable_label(!state.map_fill_width, crate::i18n::tr("⭕ Koko taivas"))
                    .on_hover_text(crate::i18n::tr("Näytä koko 360° taivaskupu ympyränä kerralla"))
                    .clicked()
                {
                    state.map_fill_width = false;
                    state.map_zoom = 1.0;
                    state.map_pan = Vec2::ZERO;
                }
            });
        });

        ui.separator();

        // Canvas allocation
        let available_size = ui.available_size();
        let (response, painter) = ui.allocate_painter(available_size, egui::Sense::click_and_drag());
        let rect = response.rect;

        // Pan and Zoom (only when mouse pointer is over the star map canvas)
        if response.dragged() {
            state.map_pan += response.drag_delta();
        }

        if response.hovered() {
            let scroll = ui.input(|i| {
                if i.smooth_scroll_delta.y != 0.0 {
                    i.smooth_scroll_delta.y
                } else {
                    i.raw_scroll_delta.y
                }
            });

            if scroll != 0.0 {
                let zoom_factor = if scroll > 0.0 { 1.10_f32 } else { 0.90_f32 };
                let old_zoom = state.map_zoom;
                state.map_zoom = (state.map_zoom * zoom_factor).clamp(0.5_f32, 5.0_f32);

                if let Some(hover_pos) = response.hover_pos() {
                    let center_offset = hover_pos - rect.center() - state.map_pan;
                    state.map_pan -= center_offset * (state.map_zoom / old_zoom - 1.0_f32);
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

        // Draw sky objects
        let mouse_pos = response.hover_pos();
        let mut hovered: Option<SkyObject> = None;
        let mut clicked_object: Option<SkyObject> = None;

        for obj in catalog {
            let (alt, az) = ra_dec_to_alt_az(obj.ra_hours, obj.dec_deg, lat, lon, &now_utc);
            if let Some((norm_x, norm_y)) = alt_az_to_canvas_normalized(alt, az) {
                let screen_x = center.x + norm_x * radius;
                let screen_y = center.y + norm_y * radius;
                let pt = Pos2::new(screen_x, screen_y);

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
                let base_color = Color32::from_rgb(r, g, b);

                // Draw object shape
                match obj.obj_type {
                    DsoType::Star => {
                        let dot_r = (3.5_f32 - obj.magnitude * 0.4_f32).clamp(1.5_f32, 4.5_f32);
                        painter.circle_filled(pt, dot_r, base_color);
                    }
                    DsoType::Galaxy => {
                        // Ellipse
                        let rect_w = 7.0;
                        let rect_h = 4.0;
                        painter.rect_filled(Rect::from_center_size(pt, Vec2::new(rect_w, rect_h)), 2.0, base_color);
                    }
                    DsoType::EmissionNebula | DsoType::SupernovaRemnant => {
                        // Square box
                        painter.rect_filled(Rect::from_center_size(pt, Vec2::splat(6.0)), 1.5, base_color);
                    }
                    DsoType::ReflectionNebula => {
                        // Diamond (e.g. Pleiades M45)
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
                    // Object ID text for bright objects
                    painter.text(
                        Pos2::new(pt.x + 8.0, pt.y - 6.0),
                        egui::Align2::LEFT_CENTER,
                        &obj.id,
                        egui::FontId::proportional(10.5),
                        Color32::from_rgba_unmultiplied(r, g, b, 200),
                    );
                }
            }
        }

        // Draw Seestar Telescope Pointing Reticle
        let (scope_alt, scope_az) = ra_dec_to_alt_az(telem.current_ra, telem.current_dec, lat, lon, &now_utc);
        if let Some((norm_x, norm_y)) = alt_az_to_canvas_normalized(scope_alt, scope_az) {
            let pt = Pos2::new(center.x + norm_x * radius, center.y + norm_y * radius);

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
                format!("⊕ {} ({})", telem.scope_model.display_short(), telem.state.display_fi()),
                egui::FontId::proportional(12.0),
                reticle_color,
            );
        }

        // Draw Mosaic Framing Grid if enabled
        if state.mosaic_enabled {
            let px_per_deg = (radius / 90.0) * state.map_zoom;
            for panel in &state.mosaic_plan.panels {
                let (p_alt, p_az) = ra_dec_to_alt_az(panel.ra_hours, panel.dec_deg, lat, lon, &now_utc);
                if let Some((norm_x, norm_y)) = alt_az_to_canvas_normalized(p_alt, p_az) {
                    let panel_center = Pos2::new(center.x + norm_x * radius, center.y + norm_y * radius);

                    // Panel rectangular dimensions on canvas
                    let panel_w = (panel.fov_w_deg * px_per_deg).max(22.0);
                    let panel_h = (panel.fov_h_deg * px_per_deg).max(14.0);
                    let panel_rect = Rect::from_center_size(panel_center, Vec2::new(panel_w, panel_h));

                    // Fill panel with subtle cyan/teal translucent tint
                    painter.rect_filled(panel_rect, 2.0, Color32::from_rgba_unmultiplied(30, 160, 200, 35));

                    // Stroke panel boundary
                    painter.rect_stroke(
                        panel_rect,
                        2.0,
                        Stroke::new(1.5_f32, Color32::from_rgba_unmultiplied(70, 220, 210, 200)),
                        egui::StrokeKind::Inside,
                    );

                    // Panel number label
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
                ui.label(format!("Katalogi: {} | Tähdistö: {}", h.id, h.constellation));
                ui.label(format!("Korkeus: {:.1}° | Atsimuutti: {:.1}°", alt, az));
                ui.label(format!("Magnitudi: {:.1} | Suodatin: {}", h.magnitude, h.filter_recommendation));
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
            format!("Huippu: {:.0}° ({})", max_alt, max_time),
            egui::FontId::proportional(10.0),
            Color32::from_rgb(255, 220, 100),
        );
    }
}
