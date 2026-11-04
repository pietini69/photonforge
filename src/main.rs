mod core;
mod gui;
mod i18n;
mod processing;
mod telescope;

use gui::app::ActiveTab;
use gui::telescope_view::TelescopeCenterView;
use gui::PhotonForgeApp;

fn main() -> eframe::Result<()> {
    env_logger::init();
    i18n::init_from_env();

    let mut initial_tab = None;
    let mut center_view = None;
    let mut initial_files = Vec::new();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--tab" && i + 1 < args.len() {
            match args[i + 1].as_str() {
                "telescope" | "seestar" | "kartta" => initial_tab = Some(ActiveTab::Telescope),
                "stf" => initial_tab = Some(ActiveTab::Stf),
                "stacking" | "pinoaminen" => initial_tab = Some(ActiveTab::Stacking),
                "processing" | "kasittely" => initial_tab = Some(ActiveTab::Processing),
                _ => {}
            }
            i += 2;
        } else if args[i] == "--view" && i + 1 < args.len() {
            match args[i + 1].as_str() {
                "map" | "kartta" => center_view = Some(TelescopeCenterView::StarMap),
                "sequencer" | "sekvensseri" => center_view = Some(TelescopeCenterView::Sequencer),
                "image" => center_view = Some(TelescopeCenterView::ImageViewer),
                _ => {}
            }
            i += 2;
        } else {
            let p = std::path::PathBuf::from(&args[i]);
            if p.exists() {
                initial_files.push(p);
            }
            i += 1;
        }
    }

    let icon_bytes = include_bytes!("../assets/icon_512.png");
    let icon_data = eframe::icon_data::from_png_bytes(icon_bytes).ok();

    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title("PhotonForge – Astrophotography Processing Suite")
        .with_inner_size([1400.0, 900.0])
        .with_min_inner_size([960.0, 600.0]);

    if let Some(icon) = icon_data {
        viewport = viewport.with_icon(icon);
    }

    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "PhotonForge",
        native_options,
        Box::new(move |cc| {
            Ok(Box::new(PhotonForgeApp::new_with_options(
                cc,
                initial_files,
                initial_tab,
                center_view,
            )))
        }),
    )
}
