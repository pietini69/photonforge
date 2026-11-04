use super::canvas::{CropAction, CropAspectRatio, ImageViewer, ManualCropState};
use crate::core::image::AstroImage;
use crate::core::loader::ImageLoader;
use crate::processing::background::{BackgroundExtractor, ExtractionMode};
use crate::processing::debayer::{BayerPattern, Debayer};
use crate::processing::histogram::HistogramProcessor;
use crate::processing::scnr::Scnr;
use crate::processing::stacking::{Stacker, StackingConfig, StackingMethod, StackingProgress};
use crate::processing::star_reduction::StarReduction;
use crate::processing::channel_combination::{AstroFilter, ChannelCombiner, ChannelCombinerConfig};
use crate::processing::curves::{CurvesChannel, CurvesSet, SplineCurve};
use crate::processing::live_stack::LiveStackEngine;
use crate::processing::mosaic_stitcher::{MosaicStitchConfig, MosaicStitchProgress, MosaicStitcher};
use crate::processing::pcc::{
    PccConfig, PccMode, PccResult, PhotometricColorCalibrator, SensorProfile, SpccFilterProfile,
    WhiteReference,
};
use crate::processing::plate_solve::{AnnotationConfig, PlateSolver, WcsSolution};
use crate::processing::stf::{ChannelStf, StfParameters};
use crate::processing::syqon::{SyQonEngine, SyQonModel, SyQonProgress, SyQonResult, SyQonTask};
use super::telescope_view::{TelescopeCenterView, TelescopeView, TelescopeViewState};
use crate::telescope::catalog::{load_sky_catalog, SkyObject};
use crate::telescope::coordinates::{
    format_dec, format_ra, DEFAULT_LATITUDE_DEG, DEFAULT_LONGITUDE_DEG,
};
use crate::telescope::downloader::LiveIngestManager;
use crate::telescope::seestar::SeestarClient;
use crate::telescope::sequencer::NightSequencer;
use eframe::egui::{self, Color32, Pos2, RichText, Vec2};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(PartialEq)]
pub enum ActiveTab {
    Files,
    Stf,
    Stacking,
    LiveStacking,
    ChannelCombination,
    Processing,
    Telescope,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StackingTabMode {
    Standard,
    Mosaic,
}

#[derive(Clone, Debug)]
pub struct LoadProgress {
    pub current: usize,
    pub total: usize,
    pub filename: String,
    pub fraction: f32,
}

#[derive(Clone)]
pub struct AppStateSnapshot {
    pub description: String,
    pub images: Vec<Arc<AstroImage>>,
    pub active_image_index: usize,
    pub stf_params: StfParameters,
}

pub struct PhotonForgeApp {
    images: Vec<Arc<AstroImage>>,
    active_image_index: usize,
    image_viewer: ImageViewer,
    stf_params: StfParameters,
    active_tab: ActiveTab,
    status_message: String,
    hovered_pixel_info: Option<(usize, usize, Vec<f32>)>,

    undo_stack: Vec<AppStateSnapshot>,
    redo_stack: Vec<AppStateSnapshot>,

    // Async image loading parameters
    is_loading_files: bool,
    load_progress: Arc<Mutex<Option<LoadProgress>>>,
    load_cancel: Arc<AtomicBool>,
    load_rx: Option<Receiver<Result<Vec<AstroImage>, String>>>,

    // Stacking parameters
    stacking_mode: StackingTabMode,
    mosaic_stitch_config: MosaicStitchConfig,
    stacking_config: StackingConfig,
    is_stacking: bool,
    stacking_progress: Arc<Mutex<Option<StackingProgress>>>,
    stacking_cancel: Arc<AtomicBool>,
    stacking_rx: Option<Receiver<(Result<Vec<AstroImage>, String>, Duration)>>,
    selected_stack_filter: Option<String>,

    // Channel Combination parameters
    channel_r_index: Option<usize>,
    channel_g_index: Option<usize>,
    channel_b_index: Option<usize>,
    channel_l_index: Option<usize>,
    channel_combiner_config: ChannelCombinerConfig,

    // Processing parameters
    pcc_config: PccConfig,
    last_pcc_result: Option<PccResult>,
    annotation_config: AnnotationConfig,
    wcs_solution: Option<WcsSolution>,
    wcs_solved_image_key: Option<(usize, usize, usize)>,
    wcs_manual_target_idx: Option<usize>,
    abe_degree: usize,
    abe_mode: ExtractionMode,
    abe_deviation: f32,
    scnr_amount: f32,
    ha_boost: f32,
    oiii_boost: f32,
    reflection_blue_boost: f32,
    reflection_cyan_boost: f32,
    reflection_saturation: f32,
    arcsinh_stretch_factor: f32,
    arcsinh_black_point: f32,
    wb_r: f32,
    wb_g: f32,
    wb_b: f32,
    saturation_factor: f32,
    selected_bayer_pattern: BayerPattern,
    star_reduction_amount: f32,
    star_reduction_radius: usize,

    // Curves
    curves: CurvesSet,
    curves_channel: CurvesChannel,
    curves_live_preview: bool,
    curves_selected_point: Option<usize>,
    show_curves_window: bool,

    // Telescope & Star Map & Live Stacking
    seestar_client: Arc<SeestarClient>,
    night_sequencer: Arc<NightSequencer>,
    live_ingest: Arc<LiveIngestManager>,
    live_stacker: Arc<LiveStackEngine>,
    live_image_viewer: ImageViewer,
    live_stack_counter: usize,
    sky_catalog: Vec<SkyObject>,
    telescope_view_state: TelescopeViewState,
    sky_lat: f64,
    sky_lon: f64,

    image_counter: usize,
    show_reset_dialog: bool,
    show_sidebar: bool,

    // Manual Crop & Rotation
    manual_crop: ManualCropState,
    rotation_angle: f32,
    rotation_auto_crop: bool,
    show_rotation_dialog: bool,

    // SyQon Neural AI
    syqon_cli_available: bool,
    syqon_cli_version: Option<String>,
    syqon_simulate: bool,
    syqon_progress: Arc<Mutex<Option<SyQonProgress>>>,
    syqon_cancel: Arc<AtomicBool>,
    is_syqon_running: bool,
    syqon_rx: Option<Receiver<Result<SyQonResult, String>>>,

    syqon_axiom_model: SyQonModel,
    syqon_axiom_stretch: String,
    syqon_axiom_stars_only: bool,

    syqon_parallax_model: SyQonModel,
    syqon_parallax_correction: bool,
    syqon_parallax_reduction: bool,
    syqon_parallax_reduction_level: u32,
    syqon_parallax_deblur: bool,
    syqon_parallax_deblur_strength: f32,

    syqon_prism_model: SyQonModel,
    syqon_prism_domain: String,
    syqon_prism_application: f32,

    syqon_gradient_save: bool,
}

fn expand_image_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = paths;
    while let Some(p) = stack.pop() {
        if p.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&p) {
                for entry in entries.flatten() {
                    stack.push(entry.path());
                }
            }
        } else if p.is_file() && is_supported_image(&p) {
            out.push(p);
        }
    }
    out.sort();
    out
}

fn is_supported_image(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    matches!(
        ext.as_str(),
        "fits" | "fit" | "fts" | "xisf" | "tiff" | "tif" | "png" | "jpg" | "jpeg"
    )
}

impl Default for PhotonForgeApp {
    fn default() -> Self {
        let live_stacker = LiveStackEngine::new();
        let live_ingest = Arc::new(LiveIngestManager::new());

        // Connect Seestar live download directly to Live Stacker
        let stacker_sink = Arc::clone(&live_stacker);
        live_ingest.set_on_file_downloaded(move |path| {
            stacker_sink.feed_file(path);
        });

        Self {
            images: Vec::new(),
            active_image_index: 0,
            image_viewer: ImageViewer::default(),
            stf_params: StfParameters::default(),
            active_tab: ActiveTab::Files,
            status_message: crate::i18n::tr("Valmis. Avaa kuvia yläpalkista.").to_string(),
            hovered_pixel_info: None,

            is_loading_files: false,
            load_progress: Arc::new(Mutex::new(None)),
            load_cancel: Arc::new(AtomicBool::new(false)),
            load_rx: None,

            stacking_mode: StackingTabMode::Standard,
            mosaic_stitch_config: MosaicStitchConfig::default(),
            stacking_config: StackingConfig::default(),
            is_stacking: false,
            stacking_progress: Arc::new(Mutex::new(None)),
            stacking_cancel: Arc::new(AtomicBool::new(false)),
            stacking_rx: None,
            selected_stack_filter: None,

            channel_r_index: None,
            channel_g_index: None,
            channel_b_index: None,
            channel_l_index: None,
            channel_combiner_config: ChannelCombinerConfig::default(),

            pcc_config: PccConfig::default(),
            last_pcc_result: None,
            annotation_config: AnnotationConfig::default(),
            wcs_solution: None,
            wcs_solved_image_key: None,
            wcs_manual_target_idx: None,
            abe_degree: 2,
            abe_mode: ExtractionMode::Subtraction,
            abe_deviation: 3.5,
            scnr_amount: 1.0,
            ha_boost: 1.25,
            oiii_boost: 1.30,
            reflection_blue_boost: 2.0,
            reflection_cyan_boost: 1.3,
            reflection_saturation: 1.6,
            arcsinh_stretch_factor: 25.0,
            arcsinh_black_point: 0.05,
            wb_r: 1.0,
            wb_g: 1.0,
            wb_b: 1.0,
            saturation_factor: 1.3,
            selected_bayer_pattern: BayerPattern::Grbg,
            star_reduction_amount: 0.50,
            star_reduction_radius: 1,

            curves: CurvesSet::default(),
            curves_channel: CurvesChannel::Rgb,
            curves_live_preview: true,
            curves_selected_point: None,
            show_curves_window: false,

            seestar_client: Arc::new(SeestarClient::new()),
            night_sequencer: Arc::new(NightSequencer::new()),
            live_ingest,
            live_stacker,
            live_image_viewer: ImageViewer::default(),
            live_stack_counter: 0,
            sky_catalog: load_sky_catalog(),
            telescope_view_state: TelescopeViewState::default(),
            sky_lat: DEFAULT_LATITUDE_DEG,
            sky_lon: DEFAULT_LONGITUDE_DEG,

            image_counter: 0,
            show_reset_dialog: false,
            show_sidebar: true,

            manual_crop: ManualCropState::default(),
            rotation_angle: 0.0,
            rotation_auto_crop: false,
            show_rotation_dialog: false,

            syqon_cli_available: SyQonEngine::is_available(),
            syqon_cli_version: SyQonEngine::get_version(),
            syqon_simulate: false,
            syqon_progress: Arc::new(Mutex::new(None)),
            syqon_cancel: Arc::new(AtomicBool::new(false)),
            is_syqon_running: false,
            syqon_rx: None,

            syqon_axiom_model: SyQonModel::AxiomMini,
            syqon_axiom_stretch: "auto".to_string(),
            syqon_axiom_stars_only: true,

            syqon_parallax_model: SyQonModel::ParallaxNano,
            syqon_parallax_correction: true,
            syqon_parallax_reduction: true,
            syqon_parallax_reduction_level: 5,
            syqon_parallax_deblur: true,
            syqon_parallax_deblur_strength: 0.50,

            syqon_prism_model: SyQonModel::PrismEssential,
            syqon_prism_domain: "auto".to_string(),
            syqon_prism_application: 0.85,

            syqon_gradient_save: false,

            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }
}

impl PhotonForgeApp {
    #[allow(dead_code)]
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self::default()
    }

    #[allow(dead_code)]
    pub fn new_with_files(cc: &eframe::CreationContext<'_>, files: Vec<PathBuf>) -> Self {
        Self::new_with_options(cc, files, None, None)
    }

    pub fn new_with_options(
        _cc: &eframe::CreationContext<'_>,
        files: Vec<PathBuf>,
        tab: Option<ActiveTab>,
        view: Option<TelescopeCenterView>,
    ) -> Self {
        let mut app = Self::default();
        if let Some(t) = tab {
            app.active_tab = t;
        }
        if let Some(v) = view {
            app.telescope_view_state.center_view = v;
        }
        if !files.is_empty() {
            app.start_loading_files(files);
        }
        app
    }

    #[allow(dead_code)]
    pub fn undo_stack_len(&self) -> usize {
        self.undo_stack.len()
    }

    #[allow(dead_code)]
    pub fn redo_stack_len(&self) -> usize {
        self.redo_stack.len()
    }

    #[allow(dead_code)]
    pub fn image_count(&self) -> usize {
        self.images.len()
    }

    #[allow(dead_code)]
    pub fn push_image(&mut self, img: AstroImage) {
        self.images.push(Arc::new(img));
        self.active_image_index = self.images.len() - 1;
    }

    pub fn push_undo(&mut self, description: &str) {
        let snapshot = AppStateSnapshot {
            description: description.to_string(),
            images: self.images.clone(),
            active_image_index: self.active_image_index,
            stf_params: self.stf_params.clone(),
        };
        self.undo_stack.push(snapshot);
        if self.undo_stack.len() > 30 {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    pub fn undo(&mut self) {
        if let Some(prev) = self.undo_stack.pop() {
            let current = AppStateSnapshot {
                description: prev.description.clone(),
                images: self.images.clone(),
                active_image_index: self.active_image_index,
                stf_params: self.stf_params.clone(),
            };
            self.redo_stack.push(current);

            self.images = prev.images;
            self.active_image_index = if self.images.is_empty() {
                0
            } else {
                prev.active_image_index.min(self.images.len() - 1)
            };
            self.stf_params = prev.stf_params;
            self.image_counter += 1;
            self.status_message = format!("↩ Kumottu: {}", prev.description);
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.redo_stack.pop() {
            let current = AppStateSnapshot {
                description: next.description.clone(),
                images: self.images.clone(),
                active_image_index: self.active_image_index,
                stf_params: self.stf_params.clone(),
            };
            self.undo_stack.push(current);

            self.images = next.images;
            self.active_image_index = if self.images.is_empty() {
                0
            } else {
                next.active_image_index.min(self.images.len() - 1)
            };
            self.stf_params = next.stf_params;
            self.image_counter += 1;
            self.status_message = format!("↪ Tehty uudelleen: {}", next.description);
        }
    }

    fn active_image(&self) -> Option<&AstroImage> {
        self.images.get(self.active_image_index).map(|arc| arc.as_ref())
    }

    fn active_image_arc(&self) -> Option<Arc<AstroImage>> {
        self.images.get(self.active_image_index).cloned()
    }

    fn open_files_dialog(&mut self) {
        if self.is_loading_files {
            return;
        }

        let files = rfd::FileDialog::new()
            .set_title(crate::i18n::tr("Valitse FITS, XISF, TIFF tai PNG -kuvat"))
            .add_filter("Astro Images (FITS, XISF, TIFF, PNG)", &["fits", "fit", "fts", "xisf", "tiff", "tif", "png", "jpg", "jpeg"])
            .add_filter("PixInsight XISF (.xisf)", &["xisf"])
            .add_filter("FITS Files (.fits, .fit)", &["fits", "fit", "fts"])
            .pick_files();

        if let Some(paths) = files {
            self.start_loading_files(paths);
        }
    }

    pub fn start_loading_files(&mut self, raw_paths: Vec<PathBuf>) {
        if raw_paths.is_empty() || self.is_loading_files {
            return;
        }

        let paths = expand_image_paths(raw_paths);
        if paths.is_empty() {
            self.status_message = crate::i18n::tr("Valituista tiedostoista tai kansioista ei löytynyt tuettuja kuvatiedostoja.").to_string();
            return;
        }

        self.is_loading_files = true;
        self.load_cancel.store(false, Ordering::SeqCst);
        let cancel = Arc::clone(&self.load_cancel);
        let progress_arc = Arc::clone(&self.load_progress);
        *progress_arc.lock().unwrap() = Some(LoadProgress {
            current: 0,
            total: paths.len(),
            filename: crate::i18n::tr("Valmistellaan tiedostoja...").to_string(),
            fraction: 0.0,
        });

        let (tx, rx) = std::sync::mpsc::channel();
        self.load_rx = Some(rx);

        let total = paths.len();
        std::thread::spawn(move || {
            use rayon::prelude::*;
            use std::sync::atomic::AtomicUsize;

            let counter = Arc::new(AtomicUsize::new(0));
            let results = Arc::new(Mutex::new(Vec::with_capacity(total)));

            // High-performance multi-threaded parallel loading using Rayon
            paths.par_iter().for_each(|p| {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }

                let file_name = p.file_name().unwrap_or_default().to_string_lossy().to_string();

                match ImageLoader::load_file(p) {
                    Ok(img) => {
                        let mut lock = results.lock().unwrap();
                        lock.push(img);
                    }
                    Err(e) => {
                        log::warn!("Virhe ladattaessa {}: {}", p.display(), e);
                    }
                }

                let done = counter.fetch_add(1, Ordering::SeqCst) + 1;
                let frac = (done as f32 / total as f32).clamp(0.0, 1.0);
                if let Ok(mut prog) = progress_arc.lock() {
                    *prog = Some(LoadProgress {
                        current: done,
                        total,
                        filename: file_name,
                        fraction: frac,
                    });
                }
            });

            if cancel.load(Ordering::SeqCst) {
                let _ = tx.send(Err(crate::i18n::tr("Kuvien lataus peruutettiin käyttäjän toimesta.").to_string()));
                return;
            }

            let mut loaded = match Arc::try_unwrap(results) {
                Ok(mutex) => mutex.into_inner().unwrap(),
                Err(arc) => arc.lock().unwrap().clone(),
            };

            loaded.sort_by(|a, b| a.metadata.file_path.cmp(&b.metadata.file_path));

            let _ = tx.send(Ok(loaded));
        });
    }

    fn save_current_image_dialog(&mut self) {
        let Some(img) = self.active_image() else {
            self.status_message = crate::i18n::tr("Ei tallennettavaa kuvaa.").to_string();
            return;
        };

        let file = rfd::FileDialog::new()
            .set_title(crate::i18n::tr("Tallenna kuva"))
            .add_filter("PixInsight XISF (.xisf)", &["xisf"])
            .add_filter("FITS Image (.fits)", &["fits"])
            .add_filter("TIFF Image (.tif)", &["tif", "tiff"])
            .add_filter("PNG Image (.png)", &["png"])
            .add_filter("JPEG Image (.jpg)", &["jpg", "jpeg"])
            .set_file_name("processed_image.xisf")
            .save_file();

        if let Some(p) = file {
            match ImageLoader::save_image(img, &p) {
                Ok(_) => self.status_message = format!("Tallennettu onnistuneesti: {}", p.display()),
                Err(e) => self.status_message = format!("Tallennusvirhe: {}", e),
            }
        }
    }

    fn reset_workspace(&mut self) {
        if self.is_stacking {
            self.stacking_cancel.store(true, Ordering::SeqCst);
            self.is_stacking = false;
            self.stacking_rx = None;
            if let Ok(mut lock) = self.stacking_progress.lock() {
                *lock = None;
            }
        }

        if !self.images.is_empty() {
            self.push_undo(crate::i18n::tr("Työtilan tyhjennys"));
        }

        self.images.clear();
        self.active_image_index = 0;
        self.image_viewer.reset_view();
        self.stf_params = StfParameters::default();
        self.hovered_pixel_info = None;
        self.curves.reset_all();
        self.curves_selected_point = None;
        self.active_tab = ActiveTab::Files;
        self.image_counter += 1;
        self.show_reset_dialog = false;
        self.status_message = crate::i18n::tr("Työtila tyhjennetty. Valmis uutta käsittelyä varten.").to_string();
    }

    fn run_auto_stf(&mut self) {
        if let Some(img) = self.active_image() {
            self.stf_params = StfParameters::compute_auto_with_target(
                img,
                self.stf_params.linked_channels,
                self.stf_params.target_bg,
            );
            self.stf_params.enabled = true;
            self.image_counter += 1;
            self.status_message = format!(
                "Auto-Stretch (STF) laskettu (tausta: {:.2}, {}).",
                self.stf_params.target_bg,
                if self.stf_params.linked_channels { "linkitetty" } else { "neutraali" }
            );
        }
    }

    fn run_stacking(&mut self) {
        let filter_name_opt = self.selected_stack_filter.clone();
        let images: Vec<Arc<AstroImage>> = if let Some(ref filt) = filter_name_opt {
            self.images
                .iter()
                .filter(|img| img.metadata.filter_name.as_deref() == Some(filt.as_str()))
                .cloned()
                .collect()
        } else {
            self.images.clone()
        };

        if images.len() < 2 {
            self.status_message = crate::i18n::tr("Pinoamiseen tarvitaan vähintään kaksi kuvaa valitulla suodattimella.").to_string();
            return;
        }

        if self.is_stacking {
            return;
        }

        self.is_stacking = true;
        self.stacking_cancel.store(false, Ordering::SeqCst);

        let progress_arc = Arc::clone(&self.stacking_progress);
        let filt_desc = filter_name_opt.as_deref().unwrap_or(crate::i18n::tr("Kaikki"));
        *progress_arc.lock().unwrap() = Some(StackingProgress {
            stage: format!("Valmistellaan pinoamista ({})", filt_desc),
            detail: format!("Valmistellaan {} kuvaa rinnakkaiskäsittelyyn", images.len()),
            fraction: 0.0,
        });

        self.status_message = format!("Pinotaan {} kuvaa ({}) taustalla...", images.len(), filt_desc);

        let config = self.stacking_config.clone();
        let cancel_token = Arc::clone(&self.stacking_cancel);

        let (tx, rx) = std::sync::mpsc::channel();
        self.stacking_rx = Some(rx);

        let start = std::time::Instant::now();
        let filt_for_name = filter_name_opt.clone();

        std::thread::spawn(move || {
            let on_progress = {
                let progress_arc = Arc::clone(&progress_arc);
                move |p: StackingProgress| {
                    if let Ok(mut lock) = progress_arc.lock() {
                        *lock = Some(p);
                    }
                }
            };

            let res = Stacker::stack_with_progress(&images, &config, Some(Box::new(on_progress)), cancel_token);
            let duration = start.elapsed();
            let res_vec = res.map(|mut img| {
                if let Some(f) = filt_for_name {
                    img.metadata.file_name = format!("Master_{}.fits", f);
                    img.metadata.filter_name = Some(f);
                } else {
                    img.metadata.file_name = "Master_Stacked.fits".to_string();
                }
                vec![img]
            });
            let _ = tx.send((res_vec, duration));
        });
    }

    fn run_batch_stacking(&mut self) {
        let mut groups: BTreeMap<String, Vec<Arc<AstroImage>>> = BTreeMap::new();
        for img in &self.images {
            let f = img.metadata.filter_name.clone().unwrap_or_else(|| crate::i18n::tr("Tuntematon").to_string());
            groups.entry(f).or_default().push(img.clone());
        }

        let valid_groups: Vec<(String, Vec<Arc<AstroImage>>)> = groups
            .into_iter()
            .filter(|(_, list)| list.len() >= 2)
            .collect();

        if valid_groups.is_empty() {
            self.status_message = crate::i18n::tr("Ei suodatinryhmiä, joissa olisi vähintään 2 kuvaa sarjapinoamista varten.").to_string();
            return;
        }

        if self.is_stacking {
            return;
        }

        self.is_stacking = true;
        self.stacking_cancel.store(false, Ordering::SeqCst);

        let progress_arc = Arc::clone(&self.stacking_progress);
        let total_groups = valid_groups.len();
        *progress_arc.lock().unwrap() = Some(StackingProgress {
            stage: format!("Aloitetaan sarjapinoaminen ({} suodatinryhmää)...", total_groups),
            detail: crate::i18n::tr("Valmistellaan suodatinryhmiä rinnakkaiskäsittelyyn").to_string(),
            fraction: 0.0,
        });

        self.status_message = format!("Pinotaan {} suodatinryhmää sarjana...", total_groups);

        let config = self.stacking_config.clone();
        let cancel_token = Arc::clone(&self.stacking_cancel);

        let (tx, rx) = std::sync::mpsc::channel();
        self.stacking_rx = Some(rx);

        let start = std::time::Instant::now();

        std::thread::spawn(move || {
            let mut masters = Vec::new();

            for (group_idx, (filt, group_images)) in valid_groups.iter().enumerate() {
                if cancel_token.load(Ordering::SeqCst) {
                    break;
                }

                let filt_clone = filt.clone();
                let on_progress = {
                    let progress_arc = Arc::clone(&progress_arc);
                    let filt_name = filt_clone.clone();
                    move |p: StackingProgress| {
                        if let Ok(mut lock) = progress_arc.lock() {
                            let overall_frac = (group_idx as f32 + p.fraction) / total_groups as f32;
                            *lock = Some(StackingProgress {
                                stage: format!("Suodatin {}/{} [{}]: {}", group_idx + 1, total_groups, filt_name, p.stage),
                                detail: p.detail,
                                fraction: overall_frac,
                            });
                        }
                    }
                };

                match Stacker::stack_with_progress(group_images, &config, Some(Box::new(on_progress)), Arc::clone(&cancel_token)) {
                    Ok(mut master) => {
                        master.metadata.file_name = format!("Master_{}.fits", filt);
                        master.metadata.filter_name = Some(filt.clone());
                        masters.push(master);
                    }
                    Err(e) => {
                        let duration = start.elapsed();
                        let _ = tx.send((Err(format!("Virhe suodattimen {} pinoamisessa: {}", filt, e)), duration));
                        return;
                    }
                }
            }

            let duration = start.elapsed();
            let _ = tx.send((Ok(masters), duration));
        });
    }

    fn run_mosaic_stitching(&mut self) {
        if self.images.len() < 2 {
            self.status_message = crate::i18n::tr("Mosaiikin yhdistämiseen tarvitaan vähintään kaksi kuvaa.").to_string();
            return;
        }

        if self.is_stacking {
            return;
        }

        self.is_stacking = true;
        self.stacking_cancel.store(false, Ordering::SeqCst);

        let progress_arc = Arc::clone(&self.stacking_progress);
        *progress_arc.lock().unwrap() = Some(StackingProgress {
            stage: crate::i18n::tr("Alustetaan mosaiikkia").to_string(),
            detail: format!("Valmistellaan {} paneelia yhdistämiseen", self.images.len()),
            fraction: 0.0,
        });

        self.status_message = format!("Yhdistetään {} mosaiikkipaneelia taustalla...", self.images.len());

        let panels = self.images.clone();
        let config = self.mosaic_stitch_config.clone();
        let cancel_token = Arc::clone(&self.stacking_cancel);

        let (tx, rx) = std::sync::mpsc::channel();
        self.stacking_rx = Some(rx);

        let start = std::time::Instant::now();

        std::thread::spawn(move || {
            let on_progress = {
                let progress_arc = Arc::clone(&progress_arc);
                Box::new(move |p: MosaicStitchProgress| {
                    if let Ok(mut lock) = progress_arc.lock() {
                        *lock = Some(StackingProgress {
                            stage: p.stage,
                            detail: p.detail,
                            fraction: p.fraction,
                        });
                    }
                })
            };

            let result = MosaicStitcher::stitch(&panels, &config, Some(on_progress), cancel_token)
                .map(|img| vec![img]);

            let duration = start.elapsed();
            let _ = tx.send((result, duration));
        });
    }

    fn run_channel_combination(&mut self) {
        let (Some(r_idx), Some(g_idx), Some(b_idx)) = (self.channel_r_index, self.channel_g_index, self.channel_b_index) else {
            self.status_message = crate::i18n::tr("Valitse vähintään R-, G- ja B-kanavat.").to_string();
            return;
        };

        let r_img = match self.images.get(r_idx) {
            Some(arc) => Arc::clone(arc),
            None => {
                self.status_message = crate::i18n::tr("Valittua R-kuvaa ei löydy.").to_string();
                return;
            }
        };

        let g_img = match self.images.get(g_idx) {
            Some(arc) => Arc::clone(arc),
            None => {
                self.status_message = crate::i18n::tr("Valittua G-kuvaa ei löydy.").to_string();
                return;
            }
        };

        let b_img = match self.images.get(b_idx) {
            Some(arc) => Arc::clone(arc),
            None => {
                self.status_message = crate::i18n::tr("Valittua B-kuvaa ei löydy.").to_string();
                return;
            }
        };

        let l_img = self.channel_l_index.and_then(|idx| self.images.get(idx).cloned());

        self.push_undo(crate::i18n::tr("Kanavayhdistys (LRGB/SHO)"));

        match ChannelCombiner::combine(
            &r_img,
            &g_img,
            &b_img,
            l_img.as_deref(),
            &self.channel_combiner_config,
        ) {
            Ok(mut composite) => {
                let r_filt = r_img.metadata.filter_name.as_deref().unwrap_or("R");
                let g_filt = g_img.metadata.filter_name.as_deref().unwrap_or("G");
                let b_filt = b_img.metadata.filter_name.as_deref().unwrap_or("B");

                let name = if l_img.is_some() {
                    "Composite_LRGB.fits".to_string()
                } else if r_filt == "SII" && g_filt == "Ha" && b_filt == "OIII" {
                    "Composite_SHO.fits".to_string()
                } else if r_filt == "Ha" && (g_filt == "OIII" || b_filt == "OIII") {
                    "Composite_HOO.fits".to_string()
                } else {
                    "Composite_RGB.fits".to_string()
                };

                composite.metadata.file_name = name;
                self.images.push(Arc::new(composite));
                self.active_image_index = self.images.len() - 1;
                self.image_counter += 1;
                if let Some(img) = self.active_image() {
                    self.stf_params = StfParameters::compute_auto(img, false);
                }
                self.status_message = crate::i18n::tr("Kanavat yhdistetty onnistuneesti värikuvaksi!").to_string();
            }
            Err(e) => {
                self.status_message = format!("Kanavayhdistysvirhe: {}", e);
            }
        }
    }

    fn apply_palette_preset(&mut self, preset: &str) {
        match preset {
            "LRGB" => {
                self.channel_r_index = self.find_image_by_filter(&["R", "RED"]);
                self.channel_g_index = self.find_image_by_filter(&["G", "GREEN"]);
                self.channel_b_index = self.find_image_by_filter(&["B", "BLUE"]);
                self.channel_l_index = self.find_image_by_filter(&["L", "LUM", "LUMINANCE", "CLEAR"]);
                self.status_message = crate::i18n::tr("Asetettu LRGB-paletti.").to_string();
            }
            "SHO" => {
                self.channel_r_index = self.find_image_by_filter(&["SII", "S2"]);
                self.channel_g_index = self.find_image_by_filter(&["HA", "HALPHA"]);
                self.channel_b_index = self.find_image_by_filter(&["OIII", "O3"]);
                self.channel_l_index = None;
                self.status_message = crate::i18n::tr("Asetettu SHO (Hubble) -paletti: R=SII, G=Ha, B=OIII.").to_string();
            }
            "HOO" => {
                self.channel_r_index = self.find_image_by_filter(&["HA", "HALPHA"]);
                let oiii_idx = self.find_image_by_filter(&["OIII", "O3"]);
                self.channel_g_index = oiii_idx;
                self.channel_b_index = oiii_idx;
                self.channel_l_index = None;
                self.status_message = crate::i18n::tr("Asetettu HOO (Bicolor) -paletti: R=Ha, G=OIII, B=OIII.").to_string();
            }
            "HSO" => {
                self.channel_r_index = self.find_image_by_filter(&["HA", "HALPHA"]);
                self.channel_g_index = self.find_image_by_filter(&["SII", "S2"]);
                self.channel_b_index = self.find_image_by_filter(&["OIII", "O3"]);
                self.channel_l_index = None;
                self.status_message = crate::i18n::tr("Asetettu HSO-paletti: R=Ha, G=SII, B=OIII.").to_string();
            }
            _ => {}
        }
    }

    fn find_image_by_filter(&self, targets: &[&str]) -> Option<usize> {
        for (i, img) in self.images.iter().enumerate() {
            if let Some(f) = &img.metadata.filter_name {
                let f_up = f.to_uppercase();
                for &t in targets {
                    if f_up == t || f_up.contains(t) {
                        return Some(i);
                    }
                }
            }
            let fname_up = img.metadata.file_name.to_uppercase();
            for &t in targets {
                if fname_up.contains(t) {
                    return Some(i);
                }
            }
        }
        None
    }

    pub fn start_syqon_task(&mut self, task: SyQonTask) {
        let active = match self.active_image() {
            Some(img) => img.clone(),
            None => {
                self.status_message = crate::i18n::tr("Ei valittua kuvaa SyQon AI -käsittelylle.").to_string();
                return;
            }
        };

        self.syqon_cancel.store(false, Ordering::SeqCst);
        let progress_arc = Arc::clone(&self.syqon_progress);
        *progress_arc.lock().unwrap() = Some(SyQonProgress {
            stage: crate::i18n::tr("Valmistellaan").to_string(),
            detail: crate::i18n::tr("Käynnistetään SyQon Neural AI...").to_string(),
            fraction: 0.05,
        });

        let cancel_token = Arc::clone(&self.syqon_cancel);
        let simulate = self.syqon_simulate || !self.syqon_cli_available;
        let (tx, rx) = std::sync::mpsc::channel();
        self.syqon_rx = Some(rx);
        self.is_syqon_running = true;

        let on_progress = Arc::new(move |p: SyQonProgress| {
            if let Ok(mut lock) = progress_arc.lock() {
                *lock = Some(p);
            }
        });

        std::thread::spawn(move || {
            let res = SyQonEngine::run(
                &active,
                &task,
                Some(on_progress),
                cancel_token,
                simulate,
            );
            let _ = tx.send(res);
        });
    }

    fn run_abe(&mut self) {
        let Some(img) = self.active_image_arc() else { return };
        match BackgroundExtractor::extract_with_params(&img, self.abe_degree, self.abe_mode, self.abe_deviation) {
            Ok(mut extracted) => {
                self.push_undo(crate::i18n::tr("Taustagradientin poisto (ABE)"));
                extracted.metadata.file_name = format!("{}_ABE.fits", img.metadata.file_name);
                self.images.push(Arc::new(extracted));
                self.active_image_index = self.images.len() - 1;
                self.image_counter += 1;
                self.run_auto_stf();
                self.status_message = crate::i18n::tr("Taustagradientti poistettu ja tausta neutraloitu (ABE).").to_string();
            }
            Err(e) => {
                self.status_message = format!("ABE-virhe: {}", e);
            }
        }
    }

    fn run_pcc(&mut self) {
        let Some(img) = self.active_image_arc() else { return };
        match PhotometricColorCalibrator::calibrate(&img, &self.pcc_config) {
            Ok(mut res) => {
                let tag = match self.pcc_config.mode {
                    PccMode::Pcc => "PCC",
                    PccMode::Spcc => "SPCC",
                };
                self.push_undo(&format!("Fotometrinen värikalibrointi ({})", tag));
                let base = img
                    .metadata
                    .file_name
                    .strip_suffix(".fits")
                    .or_else(|| img.metadata.file_name.strip_suffix(".fit"))
                    .or_else(|| img.metadata.file_name.strip_suffix(".xisf"))
                    .unwrap_or(&img.metadata.file_name);
                res.image.metadata.file_name = format!("{}_{}.fits", base, tag);
                let calibrated_arc = Arc::new(res.image.clone());
                self.images.push(calibrated_arc);
                self.active_image_index = self.images.len() - 1;
                self.image_counter += 1;
                self.run_auto_stf();
                self.status_message = format!(
                    "{} valmis! ({}/{} tähteä, R={:.3} G={:.3} B={:.3}, T≈{:.0} K)",
                    tag,
                    res.valid_stars,
                    res.detected_stars,
                    res.k_r,
                    res.k_g,
                    res.k_b,
                    res.mean_star_temp_k
                );
                self.last_pcc_result = Some(res);
            }
            Err(e) => {
                self.status_message = format!("PCC/SPCC-virhe: {}", e);
            }
        }
    }

    fn solve_active_image_wcs(&mut self) {
        let Some(img) = self.active_image_arc() else {
            self.status_message = crate::i18n::tr("Avaa kuva ensin WCS-kenttäratkaisua (Plate Solve) varten.").to_string();
            return;
        };

        let fallback = if let Some(idx) = self.wcs_manual_target_idx {
            self.sky_catalog
                .get(idx)
                .map(|o| (o.ra_hours, o.dec_deg, format!("{} – {}", o.id, o.name)))
        } else {
            let t = self.seestar_client.get_telemetry();
            Some((t.target_ra, t.target_dec, t.target_name))
        };

        let mut sol = if let Some(idx) = self.wcs_manual_target_idx {
            let mut s = PlateSolver::solve(
                &img,
                &self.sky_catalog,
                fallback
                    .as_ref()
                    .map(|(r, d, n)| (*r, *d, n.as_str())),
            );
            if let Some(obj) = self.sky_catalog.get(idx) {
                s.center_ra_hours = obj.ra_hours;
                s.center_dec_deg = obj.dec_deg;
                s.solver_source = format!("Valittu kohde ({} – {})", obj.id, obj.name);
                s.populate_catalog_objects(&self.sky_catalog);
            }
            s
        } else {
            PlateSolver::solve(
                &img,
                &self.sky_catalog,
                fallback
                    .as_ref()
                    .map(|(r, d, n)| (*r, *d, n.as_str())),
            )
        };

        sol.populate_catalog_objects(&self.sky_catalog);

        let (fov_w, fov_h) = sol.fov_deg();
        self.status_message = format!(
            "Plate Solve valmis: {} {} | FOV {:.2}°×{:.2}° | {} tähteä, {} DSO-kohdetta ({})",
            format_ra(sol.center_ra_hours),
            format_dec(sol.center_dec_deg),
            fov_w,
            fov_h,
            sol.detected_stars.len(),
            sol.matched_objects.len(),
            sol.solver_source
        );
        self.wcs_solved_image_key = Some((self.active_image_index, img.width, img.height));
        self.wcs_solution = Some(sol);
    }

    fn toggle_wcs_annotations(&mut self) {
        self.annotation_config.enabled = !self.annotation_config.enabled;
        if self.annotation_config.enabled {
            self.solve_active_image_wcs();
        } else {
            self.status_message = crate::i18n::tr("WCS-koordinaattiruudukko ja kohdeannotaatiot piilotettu.").to_string();
        }
    }

    fn run_scnr(&mut self) {
        let Some(img) = self.active_image_arc() else { return };
        match Scnr::remove_green(&img, self.scnr_amount) {
            Ok(mut cleaned) => {
                self.push_undo(crate::i18n::tr("Vihreän kohinan poisto (SCNR)"));
                cleaned.metadata.file_name = format!("{}_SCNR.fits", img.metadata.file_name);
                self.images.push(Arc::new(cleaned));
                self.active_image_index = self.images.len() - 1;
                self.image_counter += 1;
                self.status_message = crate::i18n::tr("Viherkohina poistettu (SCNR).").to_string();
            }
            Err(e) => {
                self.status_message = format!("SCNR-virhe: {}", e);
            }
        }
    }

    fn run_dual_band_boost(&mut self) {
        let Some(img) = self.active_image_arc() else { return };
        match HistogramProcessor::dual_band_boost(&img, self.ha_boost, self.oiii_boost) {
            Ok(mut boosted) => {
                self.push_undo(crate::i18n::tr("Dual-Band korostus"));
                boosted.metadata.file_name = format!("{}_DualBand.fits", img.metadata.file_name);
                self.images.push(Arc::new(boosted));
                self.active_image_index = self.images.len() - 1;
                self.image_counter += 1;
                self.run_auto_stf();
                self.status_message = format!("Dual-Band korostettu (Ha: {:.2}x, [OIII]: {:.2}x).", self.ha_boost, self.oiii_boost);
            }
            Err(e) => {
                self.status_message = format!("Dual-band virhe: {}", e);
            }
        }
    }

    fn run_reflection_boost(&mut self) {
        let Some(img) = self.active_image_arc() else { return };
        match HistogramProcessor::reflection_nebula_boost(
            &img,
            self.reflection_blue_boost,
            self.reflection_cyan_boost,
            self.reflection_saturation,
        ) {
            Ok(mut boosted) => {
                self.push_undo(crate::i18n::tr("Heijastussumun korostus"));
                let base = img.metadata.file_name.strip_suffix(".fits")
                    .or_else(|| img.metadata.file_name.strip_suffix(".fit"))
                    .or_else(|| img.metadata.file_name.strip_suffix(".xisf"))
                    .unwrap_or(&img.metadata.file_name);
                boosted.metadata.file_name = format!("{}_Reflection.fits", base);
                self.images.push(Arc::new(boosted));
                self.active_image_index = self.images.len() - 1;
                self.image_counter += 1;
                self.run_auto_stf();
                self.status_message = format!(
                    "Heijastussumu korostettu (Sininen: {:.2}x, Syaani: {:.2}x, Kylläisyys: {:.2}x).",
                    self.reflection_blue_boost, self.reflection_cyan_boost, self.reflection_saturation
                );
            }
            Err(e) => {
                self.status_message = format!("Heijastussumuvirhe: {}", e);
            }
        }
    }

    fn run_arcsinh_stretch(&mut self) {
        let Some(img) = self.active_image_arc() else { return };
        match HistogramProcessor::arcsinh_stretch(&img, self.arcsinh_stretch_factor, self.arcsinh_black_point) {
            Ok(mut stretched) => {
                self.push_undo(crate::i18n::tr("Arcsinh-venytys"));
                let base = img.metadata.file_name.strip_suffix(".fits")
                    .or_else(|| img.metadata.file_name.strip_suffix(".fit"))
                    .or_else(|| img.metadata.file_name.strip_suffix(".xisf"))
                    .unwrap_or(&img.metadata.file_name);
                stretched.metadata.file_name = format!("{}_Arcsinh.fits", base);
                self.images.push(Arc::new(stretched));
                self.active_image_index = self.images.len() - 1;
                self.image_counter += 1;
                self.stf_params.enabled = false; // Data is now permanently stretched
                self.status_message = format!(
                    "Arcsinh-venytys suoritettu (Kerroin: {:.1}, Musta piste: {:.3}).",
                    self.arcsinh_stretch_factor, self.arcsinh_black_point
                );
            }
            Err(e) => {
                self.status_message = format!("Arcsinh-virhe: {}", e);
            }
        }
    }

    fn run_color_balance(&mut self) {
        let Some(img) = self.active_image_arc() else { return };
        match HistogramProcessor::color_balance(&img, self.wb_r, self.wb_g, self.wb_b) {
            Ok(mut balanced) => {
                self.push_undo(crate::i18n::tr("Valkotasapainon säätö"));
                balanced.metadata.file_name = format!("{}_ColorBal.fits", img.metadata.file_name);
                self.images.push(Arc::new(balanced));
                self.active_image_index = self.images.len() - 1;
                self.image_counter += 1;
                self.run_auto_stf();
                self.status_message = format!("Valkotasapaino säädetty (R:{:.2}, G:{:.2}, B:{:.2}).", self.wb_r, self.wb_g, self.wb_b);
            }
            Err(e) => {
                self.status_message = format!("Valkotasapainovirhe: {}", e);
            }
        }
    }

    fn run_saturation(&mut self) {
        let Some(img) = self.active_image_arc() else { return };
        match HistogramProcessor::adjust_saturation(&img, self.saturation_factor) {
            Ok(mut sat) => {
                self.push_undo(crate::i18n::tr("Värikylläisyyden säätö"));
                sat.metadata.file_name = format!("{}_Sat.fits", img.metadata.file_name);
                self.images.push(Arc::new(sat));
                self.active_image_index = self.images.len() - 1;
                self.image_counter += 1;
                self.status_message = format!("Värikylläisyys säädetty ({:.1}x).", self.saturation_factor);
            }
            Err(e) => {
                self.status_message = format!("Kylläisyysvirhe: {}", e);
            }
        }
    }

    fn run_star_reduction(&mut self) {
        let Some(img) = self.active_image_arc() else { return };
        match StarReduction::reduce_stars(&img, self.star_reduction_amount, self.star_reduction_radius, 1) {
            Ok(mut reduced) => {
                self.push_undo(crate::i18n::tr("Tähtien pienennys"));
                let base = img.metadata.file_name.strip_suffix(".fits")
                    .or_else(|| img.metadata.file_name.strip_suffix(".fit"))
                    .or_else(|| img.metadata.file_name.strip_suffix(".xisf"))
                    .unwrap_or(&img.metadata.file_name);
                reduced.metadata.file_name = format!("{}_StarRed.fits", base);
                self.images.push(Arc::new(reduced));
                self.active_image_index = self.images.len() - 1;
                self.image_counter += 1;
                self.run_auto_stf();
                self.status_message = format!("Tähtiä pienennetty ({:.0}%).", self.star_reduction_amount * 100.0);
            }
            Err(e) => {
                self.status_message = format!("Tähtien pienennysvirhe: {}", e);
            }
        }
    }

    fn run_manual_debayer(&mut self) {
        let Some(img) = self.active_image_arc() else { return };
        if img.channels != 1 {
            self.status_message = crate::i18n::tr("Kuva on jo 3-kanavainen RGB.").to_string();
            return;
        }

        let pattern = self.selected_bayer_pattern.flipped_vertical(img.height);
        match Debayer::debayer(&img, pattern) {
            Ok(mut debayered) => {
                self.push_undo(crate::i18n::tr("Bayer-purku"));
                debayered.metadata.file_name = format!("{}_RGB.fits", img.metadata.file_name);
                self.images.push(Arc::new(debayered));
                self.active_image_index = self.images.len() - 1;
                self.image_counter += 1;
                self.run_auto_stf();
                self.status_message = format!("Bayer-purku suoritettu (kuvio: {}).", self.selected_bayer_pattern.to_str());
            }
            Err(e) => {
                self.status_message = format!("Debayer-virhe: {}", e);
            }
        }
    }

    fn apply_permanent_stf(&mut self) {
        let Some(img) = self.active_image_arc() else { return };
        self.push_undo(crate::i18n::tr("Pysyvä venytys (Histogram Stretch)"));
        let params = [
            (self.stf_params.channels[0].shadows, self.stf_params.channels[0].midtones, self.stf_params.channels[0].highlights),
            (self.stf_params.channels[1].shadows, self.stf_params.channels[1].midtones, self.stf_params.channels[1].highlights),
            (self.stf_params.channels[2].shadows, self.stf_params.channels[2].midtones, self.stf_params.channels[2].highlights),
        ];
        let mut stretched = HistogramProcessor::apply_permanent_stretch_channels(&img, &params);
        stretched.metadata.file_name = format!("{}_Stretched.fits", img.metadata.file_name);
        self.images.push(Arc::new(stretched));
        self.active_image_index = self.images.len() - 1;
        self.image_counter += 1;
        self.stf_params.enabled = false; // Data is already stretched now
        self.status_message = crate::i18n::tr("Pysyvä venytys (Histogram Stretch) suoritettu.").to_string();
    }

    fn run_auto_crop(&mut self) {
        let Some(img) = self.active_image_arc() else { return };
        match img.auto_crop() {
            Ok(cropped) => {
                self.push_undo(crate::i18n::tr("Automaattinen rajaus"));
                let old_w = img.width;
                let old_h = img.height;
                let new_w = cropped.width;
                let new_h = cropped.height;
                self.images.push(Arc::new(cropped));
                self.active_image_index = self.images.len() - 1;
                self.image_counter += 1;
                if let Some(cropped_ref) = self.active_image() {
                    self.stf_params = StfParameters::compute_auto(cropped_ref, self.stf_params.linked_channels);
                }
                self.status_message = format!("Reunat rajattu onnistuneesti: {}×{} -> {}×{} px.", old_w, old_h, new_w, new_h);
            }
            Err(e) => {
                self.status_message = format!("Rajaus: {}", e);
            }
        }
    }

    fn run_apply_curves(&mut self) {
        let Some(img) = self.active_image_arc() else {
            self.status_message = crate::i18n::tr("Ei valittua kuvaa käyrien säätöön.").to_string();
            return;
        };

        if self.curves.is_identity() {
            self.status_message = crate::i18n::tr("Käyriä ei ole muutettu (1:1 linja).").to_string();
            return;
        }

        self.push_undo(crate::i18n::tr("Käyrien säätö (Curves)"));
        let mut modified = self.curves.apply_to_image(&img);

        let base_name = img
            .metadata
            .file_name
            .strip_suffix(".fits")
            .or_else(|| img.metadata.file_name.strip_suffix(".fit"))
            .or_else(|| img.metadata.file_name.strip_suffix(".xisf"))
            .unwrap_or(&img.metadata.file_name);
        modified.metadata.file_name = format!("{}_Curves.fits", base_name);

        self.images.push(Arc::new(modified));
        self.active_image_index = self.images.len() - 1;
        self.image_counter += 1;
        self.curves.reset_all();
        self.curves_selected_point = None;
        self.status_message = crate::i18n::tr("Käyrät sovellettu onnistuneesti kuvaan.").to_string();
    }

    fn toggle_manual_crop(&mut self) {
        if self.images.is_empty() {
            self.status_message = crate::i18n::tr("Avaa ensin kuva rajausta varten.").to_string();
            return;
        }
        self.manual_crop.is_active = !self.manual_crop.is_active;
        if self.manual_crop.is_active {
            self.status_message = crate::i18n::tr("Manuaalinen rajaus aktivoitu. Raahaa rajauslaatikkoa hiirellä tai säädä mittoja.").to_string();
        } else {
            self.status_message = crate::i18n::tr("Manuaalinen rajaustila suljettu.").to_string();
        }
    }

    fn apply_manual_crop(&mut self) {
        let Some(img) = self.active_image_arc() else { return; };
        let (px_x, px_y, px_w, px_h) = self.manual_crop.pixel_rect(img.width, img.height);
        if px_w < 4 || px_h < 4 {
            self.status_message = crate::i18n::tr("Rajausalue on liian pieni (vähintään 4x4 px).").to_string();
            return;
        }

        match img.crop(px_x, px_y, px_w, px_h) {
            Ok(cropped) => {
                self.push_undo(crate::i18n::tr("Manuaalinen rajaus"));
                self.manual_crop.is_active = false;
                let new_w = cropped.width;
                let new_h = cropped.height;
                self.images.push(Arc::new(cropped));
                self.active_image_index = self.images.len() - 1;
                self.image_counter += 1;
                if let Some(cropped_ref) = self.active_image() {
                    self.stf_params = StfParameters::compute_auto(cropped_ref, self.stf_params.linked_channels);
                }
                self.image_viewer.reset_view();
                self.status_message = format!("Kuva rajattu manuaalisesti: {} × {} px (X: {}, Y: {}).", new_w, new_h, px_x, px_y);
            }
            Err(e) => {
                self.status_message = format!("Rajausvirhe: {}", e);
            }
        }
    }

    fn cancel_manual_crop(&mut self) {
        self.manual_crop.is_active = false;
        self.status_message = crate::i18n::tr("Manuaalinen rajaus peruutettu.").to_string();
    }

    fn rotate_active_90_cw(&mut self) {
        let Some(img) = self.active_image_arc() else { return; };
        self.push_undo(crate::i18n::tr("Pyöritys 90° CW"));
        let rotated = img.rotate_90_cw();
        self.images.push(Arc::new(rotated));
        self.active_image_index = self.images.len() - 1;
        self.image_counter += 1;
        if let Some(r) = self.active_image() {
            self.stf_params = StfParameters::compute_auto(r, self.stf_params.linked_channels);
        }
        self.image_viewer.reset_view();
        self.status_message = crate::i18n::tr("Kuvaa pyöritetty 90° myötäpäivään.").to_string();
    }

    fn rotate_active_90_ccw(&mut self) {
        let Some(img) = self.active_image_arc() else { return; };
        self.push_undo(crate::i18n::tr("Pyöritys 90° CCW"));
        let rotated = img.rotate_90_ccw();
        self.images.push(Arc::new(rotated));
        self.active_image_index = self.images.len() - 1;
        self.image_counter += 1;
        if let Some(r) = self.active_image() {
            self.stf_params = StfParameters::compute_auto(r, self.stf_params.linked_channels);
        }
        self.image_viewer.reset_view();
        self.status_message = crate::i18n::tr("Kuvaa pyöritetty 90° vastapäivään.").to_string();
    }

    fn rotate_active_180(&mut self) {
        let Some(img) = self.active_image_arc() else { return; };
        self.push_undo(crate::i18n::tr("Pyöritys 180°"));
        let rotated = img.rotate_180();
        self.images.push(Arc::new(rotated));
        self.active_image_index = self.images.len() - 1;
        self.image_counter += 1;
        if let Some(r) = self.active_image() {
            self.stf_params = StfParameters::compute_auto(r, self.stf_params.linked_channels);
        }
        self.image_viewer.reset_view();
        self.status_message = crate::i18n::tr("Kuvaa pyöritetty 180°.").to_string();
    }

    fn flip_active_horizontal(&mut self) {
        let Some(img) = self.active_image_arc() else { return; };
        self.push_undo(crate::i18n::tr("Peilaus vaakasuunnassa"));
        let flipped = img.flip_horizontal();
        self.images.push(Arc::new(flipped));
        self.active_image_index = self.images.len() - 1;
        self.image_counter += 1;
        if let Some(r) = self.active_image() {
            self.stf_params = StfParameters::compute_auto(r, self.stf_params.linked_channels);
        }
        self.image_viewer.reset_view();
        self.status_message = crate::i18n::tr("Kuva peilattu vaakasuunnassa.").to_string();
    }

    fn flip_active_vertical(&mut self) {
        let Some(img) = self.active_image_arc() else { return; };
        self.push_undo(crate::i18n::tr("Peilaus pystysuunnassa"));
        let flipped = img.flip_vertical();
        self.images.push(Arc::new(flipped));
        self.active_image_index = self.images.len() - 1;
        self.image_counter += 1;
        if let Some(r) = self.active_image() {
            self.stf_params = StfParameters::compute_auto(r, self.stf_params.linked_channels);
        }
        self.image_viewer.reset_view();
        self.status_message = crate::i18n::tr("Kuva peilattu pystysuunnassa.").to_string();
    }

    fn rotate_active_arbitrary(&mut self, angle_degrees: f32, auto_crop: bool) {
        let Some(img) = self.active_image_arc() else { return; };
        if angle_degrees.abs() < 1e-4 {
            self.status_message = crate::i18n::tr("Kulma on 0.0°, kuvaa ei tarvitse pyörittää.").to_string();
            return;
        }

        self.push_undo(&format!("Vapaa pyöritys {:.1}°", angle_degrees));
        match img.rotate_arbitrary(angle_degrees, auto_crop) {
            Ok(rotated) => {
                let w = rotated.width;
                let h = rotated.height;
                self.images.push(Arc::new(rotated));
                self.active_image_index = self.images.len() - 1;
                self.image_counter += 1;
                if let Some(r) = self.active_image() {
                    self.stf_params = StfParameters::compute_auto(r, self.stf_params.linked_channels);
                }
                self.image_viewer.reset_view();
                self.status_message = format!("Kuvaa pyöritetty {:.1}° (koko {} × {} px).", angle_degrees, w, h);
            }
            Err(e) => {
                self.status_message = format!("Pyöritysvirhe: {}", e);
            }
        }
    }
}

impl eframe::App for PhotonForgeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Handle background stacking worker completion
        if self.is_stacking {
            ctx.request_repaint();

            if let Some(rx) = &self.stacking_rx {
                if let Ok((result, duration)) = rx.try_recv() {
                    self.is_stacking = false;
                    self.stacking_rx = None;
                    if let Ok(mut lock) = self.stacking_progress.lock() {
                        *lock = None;
                    }

                    match result {
                        Ok(stacked_imgs) => {
                            self.push_undo(crate::i18n::tr("Pinoaminen"));
                            let total_imgs = self.images.len();
                            let count = stacked_imgs.len();
                            for img in stacked_imgs {
                                self.images.push(Arc::new(img));
                            }
                            self.active_image_index = self.images.len() - 1;
                            self.image_counter += 1;
                            if let Some(img) = self.active_image() {
                                self.stf_params = StfParameters::compute_auto_with_target(
                                    img,
                                    self.stf_params.linked_channels,
                                    self.stf_params.target_bg,
                                );
                            }
                            self.status_message = format!(
                                "Pinoaminen valmis! ({} kuvaa -> {} master-kuva(a), kesto: {:.2} s)",
                                total_imgs,
                                count,
                                duration.as_secs_f32()
                            );
                        }
                        Err(e) => {
                            self.status_message = format!("Pinoamisvirhe: {}", e);
                        }
                    }
                }
            }
        }

        // Handle background file loading completion
        if self.is_loading_files {
            ctx.request_repaint();

            if let Some(rx) = &self.load_rx {
                if let Ok(result) = rx.try_recv() {
                    self.is_loading_files = false;
                    self.load_rx = None;
                    if let Ok(mut lock) = self.load_progress.lock() {
                        *lock = None;
                    }

                    match result {
                        Ok(new_imgs) => {
                            let count = new_imgs.len();
                            if count > 0 {
                                self.push_undo(crate::i18n::tr("Kuvien avaaminen"));
                                for img in new_imgs {
                                    self.images.push(Arc::new(img));
                                }
                                self.active_image_index = self.images.len() - 1;
                                self.image_counter += 1;
                                if let Some(img) = self.active_image() {
                                    self.stf_params = StfParameters::compute_auto(
                                        img,
                                        self.stf_params.linked_channels,
                                    );
                                }
                                self.image_viewer.reset_view();
                                self.status_message = format!("Ladattu onnistuneesti {} kuvaa.", count);
                            } else {
                                self.status_message = crate::i18n::tr("Ei uusia kuvia ladattu.").to_string();
                            }
                        }
                        Err(e) => {
                            self.status_message = e;
                        }
                    }
                }
            }
        }

        // Handle background SyQon AI worker completion
        if self.is_syqon_running {
            ctx.request_repaint();

            if let Some(rx) = &self.syqon_rx {
                if let Ok(result) = rx.try_recv() {
                    self.is_syqon_running = false;
                    self.syqon_rx = None;
                    if let Ok(mut lock) = self.syqon_progress.lock() {
                        *lock = None;
                    }

                    match result {
                        Ok(syq_res) => {
                            self.push_undo(&syq_res.description);
                            let desc = syq_res.description.clone();
                            if !self.images.is_empty() && self.active_image_index < self.images.len() {
                                self.images[self.active_image_index] = Arc::new(syq_res.primary);
                            } else {
                                self.images.push(Arc::new(syq_res.primary));
                                self.active_image_index = self.images.len() - 1;
                            }
                            if let Some(sec) = syq_res.secondary {
                                self.images.push(Arc::new(sec));
                            }
                            self.image_counter += 1;
                            if let Some(img) = self.active_image() {
                                self.stf_params = StfParameters::compute_auto(
                                    img,
                                    self.stf_params.linked_channels,
                                );
                            }
                            self.status_message = desc;
                        }
                        Err(e) => {
                            self.status_message = format!("SyQon AI -virhe: {}", e);
                        }
                    }
                }
            }
        }

        // Support drag-and-drop of files and folders directly onto the window
        let dropped_files = ctx.input(|i| i.raw.dropped_files.clone());
        if !dropped_files.is_empty() && !self.is_loading_files {
            let paths: Vec<PathBuf> = dropped_files
                .into_iter()
                .filter_map(|df| df.path)
                .collect();
            if !paths.is_empty() {
                self.start_loading_files(paths);
            }
        }

        // Keyboard shortcuts
        if !ctx.wants_keyboard_input() {
            let cmd_or_ctrl = ctx.input(|i| i.modifiers.command || i.modifiers.ctrl);
            let shift = ctx.input(|i| i.modifiers.shift);

            // Undo: Cmd+Z or Ctrl+Z
            if cmd_or_ctrl && !shift && ctx.input(|i| i.key_pressed(egui::Key::Z)) {
                self.undo();
            }
            // Redo: Cmd+Shift+Z or Ctrl+Shift+Z or Cmd+Y or Ctrl+Y
            else if (cmd_or_ctrl && shift && ctx.input(|i| i.key_pressed(egui::Key::Z)))
                || (cmd_or_ctrl && ctx.input(|i| i.key_pressed(egui::Key::Y)))
            {
                self.redo();
            }
            // Cmd+N or Ctrl+N to start over
            else if cmd_or_ctrl && ctx.input(|i| i.key_pressed(egui::Key::N)) {
                if !self.images.is_empty() {
                    self.show_reset_dialog = true;
                }
            }
        }

        // Dark theme styling
        ctx.set_visuals(egui::Visuals::dark());

        // Top Header & Toolbar
        egui::TopBottomPanel::top("top_panel")
            .frame(
                egui::Frame::side_top_panel(&ctx.style())
                    .fill(Color32::from_rgb(18, 21, 28))
                    .inner_margin(egui::Margin::symmetric(10, 6)),
            )
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);

                    ui.heading(
                        RichText::new("⚡ PhotonForge")
                            .color(Color32::from_rgb(80, 200, 255))
                            .strong(),
                    );
                    ui.label(RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).color(Color32::from_rgb(150, 160, 180)));

                    let lang_label = match crate::i18n::current_lang() {
                        crate::i18n::UiLang::English => "🇬🇧 EN",
                        crate::i18n::UiLang::Finnish => "🇫🇮 FI",
                    };
                    if ui
                        .button(RichText::new(lang_label).strong())
                        .on_hover_text("Switch UI Language: English (EN) / Suomi (FI)")
                        .clicked()
                    {
                        crate::i18n::toggle_lang();
                    }

                    ui.separator();

                    let sidebar_icon = if self.show_sidebar { crate::i18n::tr("◧ Sivupalkki") } else { crate::i18n::tr("◨ Sivupalkki") };
                    if ui
                        .selectable_label(self.show_sidebar, sidebar_icon)
                        .on_hover_text(if self.show_sidebar { crate::i18n::tr("Piilota sivupalkki (koko ruutu tähtikartalle)") } else { crate::i18n::tr("Näytä sivupalkki") })
                        .clicked()
                    {
                        self.show_sidebar = !self.show_sidebar;
                    }

                    ui.separator();

                    if ui
                        .add_enabled(!self.is_loading_files, egui::Button::new(crate::i18n::tr("📂 Avaa kuvia")))
                        .on_hover_text(crate::i18n::tr("Avaa kuvia (FITS, XISF, TIFF, PNG) tai vedä tiedostot suoraan ikkunaan"))
                        .clicked()
                    {
                        self.open_files_dialog();
                    }

                    if ui.button(crate::i18n::tr("💾 Tallenna")).clicked() {
                        self.save_current_image_dialog();
                    }

                    if ui
                        .add_enabled(
                            !self.images.is_empty(),
                            egui::Button::new(crate::i18n::tr("🔄 Aloita alusta")),
                        )
                        .on_hover_text(crate::i18n::tr("Tyhjennä työtila ja aloita alusta uudella kuvasarjalla (Cmd+N)"))
                        .clicked()
                    {
                        self.show_reset_dialog = true;
                    }

                    ui.separator();

                    let undo_tip = self
                        .undo_stack
                        .last()
                        .map(|s| format!("Kumoa: {} (Cmd+Z)", s.description))
                        .unwrap_or_else(|| crate::i18n::tr("Ei kumottavaa (Cmd+Z)").to_string());
                    if ui
                        .add_enabled(!self.undo_stack.is_empty(), egui::Button::new(crate::i18n::tr("↩ Kumoa")))
                        .on_hover_text(undo_tip)
                        .clicked()
                    {
                        self.undo();
                    }

                    let redo_tip = self
                        .redo_stack
                        .last()
                        .map(|s| format!("Tee uudelleen: {} (Cmd+Shift+Z)", s.description))
                        .unwrap_or_else(|| crate::i18n::tr("Ei uudelleen tehtävää (Cmd+Shift+Z)").to_string());
                    if ui
                        .add_enabled(!self.redo_stack.is_empty(), egui::Button::new(crate::i18n::tr("↪ Tee uudelleen")))
                        .on_hover_text(redo_tip)
                        .clicked()
                    {
                        self.redo();
                    }

                    ui.separator();

                    if ui
                        .button(
                            RichText::new("⚡ Auto-STF")
                                .color(Color32::from_rgb(255, 215, 0))
                                .strong(),
                        )
                        .on_hover_text(crate::i18n::tr("Reaaliaikainen Screen Transfer Function -automaattivenytys"))
                        .clicked()
                    {
                        self.run_auto_stf();
                    }

                    let (link_label, hover_tip) = if self.stf_params.linked_channels {
                        (crate::i18n::tr("🔗 Linkitetty"), crate::i18n::tr("Kanavat linkitetty (raakavärit). Klikkaa neutraloidaksesi taustan."))
                    } else {
                        (crate::i18n::tr("🔓 Neutraali tausta"), crate::i18n::tr("Taustan neutralointi aktiivinen. Klikkaa linkittääksesi kanavat."))
                    };
                    if ui.button(link_label).on_hover_text(hover_tip).clicked() {
                        self.stf_params.linked_channels = !self.stf_params.linked_channels;
                        self.run_auto_stf();
                    }

                    // Background level quick presets
                    if ui.button(crate::i18n::tr("🌌 Syvä (0.08)")).on_hover_text(crate::i18n::tr("Syvä avaruustausta (PixInsight-tyyli)")).clicked() {
                        self.stf_params.target_bg = 0.08;
                        self.run_auto_stf();
                    }
                    if ui.button(crate::i18n::tr("🪐 Standardi (0.10)")).on_hover_text(crate::i18n::tr("Normaali avaruustausta (0.10)")).clicked() {
                        self.stf_params.target_bg = 0.10;
                        self.run_auto_stf();
                    }

                    ui.separator();

                    let crop_btn = if self.manual_crop.is_active {
                        egui::Button::new(RichText::new(crate::i18n::tr("✂️ Rajaa (PÄÄLLÄ)")).color(Color32::from_rgb(100, 255, 150)).strong())
                    } else {
                        egui::Button::new(crate::i18n::tr("✂️ Rajaa"))
                    };
                    if ui.add_enabled(!self.images.is_empty(), crop_btn)
                        .on_hover_text(crate::i18n::tr("Kytke interaktiivinen manuaalinen rajaus päälle/pois"))
                        .clicked()
                    {
                        self.toggle_manual_crop();
                    }

                    if ui.add_enabled(!self.images.is_empty(), egui::Button::new(crate::i18n::tr("🔄 Pyöritä...")))
                        .on_hover_text(crate::i18n::tr("Kuvan pyöritys (90° CW/CCW, 180°, peilaus ja vapaa kulma)"))
                        .clicked()
                    {
                        self.show_rotation_dialog = true;
                    }

                    if ui.add_enabled(!self.images.is_empty(), egui::Button::new(crate::i18n::tr("✂️ Rajaa reunat")))
                        .on_hover_text(crate::i18n::tr("Leikkaa pois tähtikohdistuksen ja pinoamisen mustat reunat (Auto-Crop)"))
                        .clicked()
                    {
                        self.run_auto_crop();
                    }

                    if ui.button(crate::i18n::tr("🌐 ABE Tausta"))
                        .on_hover_text(crate::i18n::tr("Automaattinen taustan tasoitus ja valosaasteen neutralointi"))
                        .clicked()
                    {
                        self.run_abe();
                    }

                    if ui.button("🎯 SPCC / PCC")
                        .on_hover_text(crate::i18n::tr("Fotometrinen ja spektrofotometrinen tähtien värikalibrointi (PCC / SPCC)"))
                        .clicked()
                    {
                        self.run_pcc();
                    }

                    let wcs_btn_text = if self.annotation_config.enabled {
                        RichText::new(crate::i18n::tr("🧭 WCS & Kohteet (PÄÄLLÄ)"))
                            .color(Color32::from_rgb(100, 255, 160))
                            .strong()
                    } else {
                        RichText::new(crate::i18n::tr("🧭 WCS & Kohteet"))
                    };
                    if ui
                        .add_enabled(!self.images.is_empty(), egui::Button::new(wcs_btn_text))
                        .on_hover_text(crate::i18n::tr("Plate Solve & WCS-annotaatiot: näytä kuvan päällä RA/Dec-koordinaattiruudukko, DSO-kohteet ja tunnistetut tähdet"))
                        .clicked()
                    {
                        self.toggle_wcs_annotations();
                    }

                    if ui.button("🟢 SCNR")
                        .on_hover_text(crate::i18n::tr("Vihreän kohinan poisto"))
                        .clicked()
                    {
                        self.run_scnr();
                    }

                    if ui.button("⚡ Dual-Band")
                        .on_hover_text(crate::i18n::tr("Korosta Ha (punainen) ja [OIII] (syaani) sumuvärejä"))
                        .clicked()
                    {
                        self.run_dual_band_boost();
                    }

                    if ui.button(crate::i18n::tr("🌌 Plejadit / Heijastus"))
                        .on_hover_text(crate::i18n::tr("Korosta heijastussumujen sinisiä sävyjä (Plejadit, Iris)"))
                        .clicked()
                    {
                        self.run_reflection_boost();
                    }

                    if ui.button(crate::i18n::tr("✨ Pienennä tähtiä"))
                        .on_hover_text(crate::i18n::tr("Pienennä tähtien kokoa ja korosta sumurakenteita (Star Reduction)"))
                        .clicked()
                    {
                        self.run_star_reduction();
                    }

                    if ui.button(crate::i18n::tr("🌈 Kanavat"))
                        .on_hover_text(crate::i18n::tr("Kanavayhdistys (LRGB / SHO / HOO / HSO)"))
                        .clicked()
                    {
                        self.active_tab = ActiveTab::ChannelCombination;
                    }

                    let curves_text = if self.show_curves_window {
                        RichText::new(crate::i18n::tr("📈 Käyrät")).color(Color32::from_rgb(80, 200, 255)).strong()
                    } else {
                        RichText::new(crate::i18n::tr("📈 Käyrät"))
                    };
                    if ui.button(curves_text)
                        .on_hover_text(crate::i18n::tr("Avaa interaktiivinen käyrätyökalu (Curves Transformation)"))
                        .clicked()
                    {
                        self.show_curves_window = !self.show_curves_window;
                    }

                    let syqon_text = if self.syqon_cli_available {
                        RichText::new("✨ SyQon AI").color(Color32::from_rgb(140, 220, 255)).strong()
                    } else if self.syqon_simulate {
                        RichText::new("✨ SyQon AI (Sim)").color(Color32::from_rgb(255, 200, 80)).strong()
                    } else {
                        RichText::new("✨ SyQon AI")
                    };
                    if ui.button(syqon_text)
                        .on_hover_text(crate::i18n::tr("SyQon Neural AI: Tähtien poisto (Axiom), korjaus & deblur (Parallax), kohinanpoisto (Prism)"))
                        .clicked()
                    {
                        self.active_tab = ActiveTab::Processing;
                    }

                    ui.separator();

                    // Telescope quick status button
                    let telem = self.seestar_client.get_telemetry();
                    let scope_name = telem.scope_model.display_short();
                    let (status_text, status_col) = if telem.connected {
                        if !telem.is_simulation {
                            (
                                format!("🔭 {}: {} (🔋 {}%)", scope_name, crate::i18n::tr(telem.state.display_fi()), telem.battery_pct),
                                Color32::from_rgb(100, 255, 140),
                            )
                        } else {
                            (
                                format!("🎮 {}: {} (🔋 {}%)", scope_name, crate::i18n::tr("Simulaatio"), telem.battery_pct),
                                Color32::from_rgb(120, 210, 255),
                            )
                        }
                    } else {
                        (format!("🔭 {}: {}", scope_name, crate::i18n::tr("Ei yhteyttä")), Color32::from_rgb(160, 160, 170))
                    };
                    if ui.button(RichText::new(status_text).color(status_col)).on_hover_text(crate::i18n::tr("Avaa kaukoputkiohjaus, tähtikartta ja yösekvensseri")).clicked() {
                        self.active_tab = ActiveTab::Telescope;
                    }

                    if self.active_tab == ActiveTab::Telescope {
                        ui.separator();
                        let is_map = self.telescope_view_state.center_view == TelescopeCenterView::StarMap;
                        let is_seq = self.telescope_view_state.center_view == TelescopeCenterView::Sequencer;
                        let is_img = self.telescope_view_state.center_view == TelescopeCenterView::ImageViewer;

                        if ui.selectable_label(is_map, crate::i18n::tr("🌌 Tähtikartta")).clicked() {
                            self.telescope_view_state.center_view = TelescopeCenterView::StarMap;
                        }
                        if ui.selectable_label(is_seq, crate::i18n::tr("📅 Yösekvensseri")).clicked() {
                            self.telescope_view_state.center_view = TelescopeCenterView::Sequencer;
                        }
                        if ui.selectable_label(is_img, crate::i18n::tr("🖼 Kuvanäyttö")).clicked() {
                            self.telescope_view_state.center_view = TelescopeCenterView::ImageViewer;
                        }
                    }

                    ui.separator();

                    if ui.button(crate::i18n::tr("🔍 Sovita")).on_hover_text(crate::i18n::tr("Sovita kuva ikkunaan")).clicked() {
                        self.image_viewer.reset_view();
                    }

                    if ui.button("1:1").on_hover_text(crate::i18n::tr("Näytä kuva 100% koossa")).clicked() {
                        self.image_viewer.set_1_to_1();
                    }
                });
            });

        // Bottom Status Bar
        egui::TopBottomPanel::bottom("bottom_panel")
            .frame(egui::Frame::side_top_panel(&ctx.style()).fill(Color32::from_rgb(14, 16, 22)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&self.status_message).color(Color32::from_rgb(200, 210, 225)));

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some((x, y, vals)) = &self.hovered_pixel_info {
                            let val_str = if vals.len() == 1 {
                                format!("{:.4}", vals[0])
                            } else if vals.len() >= 3 {
                                format!("R:{:.3} G:{:.3} B:{:.3}", vals[0], vals[1], vals[2])
                            } else {
                                "".to_string()
                            };
                            ui.label(
                                RichText::new(format!("X: {}  Y: {}  |  {}", x, y, val_str))
                                    .color(Color32::from_rgb(100, 220, 255)),
                            );
                        }

                        if let Some(img) = self.active_image() {
                            let ch_str = if img.channels == 1 { "Mono" } else { "RGB" };
                            ui.label(format!("{} × {} px ({})", img.width, img.height, ch_str));
                        }
                    });
                });
            });

        // Left Controls Panel
        if self.show_sidebar {
            egui::SidePanel::left("left_sidebar_panel_v4")
                .default_width(430.0)
                .width_range(370.0..=750.0)
                .resizable(true)
                .frame(egui::Frame::side_top_panel(&ctx.style()).fill(Color32::from_rgb(22, 25, 34)))
                .show(ctx, |ui| {
                    ui.add_space(6.0);

                    // Tab buttons
                    ui.scope(|ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal_wrapped(|ui| {
                            ui.selectable_value(&mut self.active_tab, ActiveTab::Files, crate::i18n::tr("📁 Kuvat"));
                            ui.selectable_value(&mut self.active_tab, ActiveTab::Stf, "⚡ STF");
                            ui.selectable_value(&mut self.active_tab, ActiveTab::Stacking, crate::i18n::tr("🌌 Pinoaminen"));
                            ui.selectable_value(&mut self.active_tab, ActiveTab::LiveStacking, crate::i18n::tr("⚡ Live-pino"));
                            ui.selectable_value(&mut self.active_tab, ActiveTab::ChannelCombination, crate::i18n::tr("🌈 Kanavat"));
                            ui.selectable_value(&mut self.active_tab, ActiveTab::Processing, crate::i18n::tr("🎨 Käsittely"));
                            ui.selectable_value(&mut self.active_tab, ActiveTab::Telescope, crate::i18n::tr("🔭 Kaukoputki & Kartta"));
                        });
                    });

                    ui.separator();

                    egui::ScrollArea::vertical()
                        .id_salt("main_sidebar_scroll")
                        .auto_shrink([false, false])
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                        .show(ui, |ui| {
                            ui.set_max_width(ui.available_width());
                            match self.active_tab {
                                ActiveTab::Files => self.ui_files_tab(ui),
                                ActiveTab::Stf => self.ui_stf_tab(ui),
                                ActiveTab::Stacking => self.ui_stacking_tab(ui),
                                ActiveTab::LiveStacking => self.ui_live_stacking_sidebar(ui),
                                ActiveTab::ChannelCombination => self.ui_channel_combination_tab(ui),
                                ActiveTab::Processing => self.ui_processing_tab(ui),
                                ActiveTab::Telescope => {
                                    TelescopeView::ui_sidebar(
                                        ui,
                                        &mut self.telescope_view_state,
                                        &self.seestar_client,
                                        &self.night_sequencer,
                                        &self.live_ingest,
                                        &self.sky_catalog,
                                        self.sky_lat,
                                        self.sky_lon,
                                    );

                                    if self.telescope_view_state.request_mosaic_stitcher {
                                        self.telescope_view_state.request_mosaic_stitcher = false;
                                        self.active_tab = ActiveTab::Stacking;
                                        self.stacking_mode = StackingTabMode::Mosaic;
                                    }
                                }
                            }
                        });
                });
        }

        // Center Panel (Star Map, Sequencer, Live Stacker, or Image Canvas)
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.style()).fill(Color32::from_rgb(10, 12, 16)))
            .show(ctx, |ui| {
                if self.active_tab == ActiveTab::LiveStacking {
                    self.ui_live_stacking_canvas(ctx, ui);
                } else if self.active_tab == ActiveTab::Telescope {
                    match self.telescope_view_state.center_view {
                        TelescopeCenterView::StarMap => {
                            TelescopeView::ui_star_chart_canvas(
                                ui,
                                &mut self.telescope_view_state,
                                &self.seestar_client,
                                &self.sky_catalog,
                                self.sky_lat,
                                self.sky_lon,
                            );
                        }
                        TelescopeCenterView::Sequencer => {
                            TelescopeView::ui_sequencer_center_view(
                                ui,
                                &mut self.telescope_view_state,
                                &self.night_sequencer,
                                &self.seestar_client,
                                self.sky_lat,
                                self.sky_lon,
                            );
                        }
                        TelescopeCenterView::ImageViewer => {
                            if self.images.is_empty() {
                                self.ui_empty_workspace(ui);
                            } else {
                                if self.annotation_config.enabled {
                                    let needs_solve = self
                                        .images
                                        .get(self.active_image_index)
                                        .map(|im| {
                                            self.wcs_solved_image_key
                                                != Some((self.active_image_index, im.width, im.height))
                                        })
                                        .unwrap_or(false);
                                    if needs_solve {
                                        self.solve_active_image_wcs();
                                    }
                                }
                                let img = self.images.get(self.active_image_index).map(|arc| arc.as_ref());
                                let curves_ref = if self.curves_live_preview && !self.curves.is_identity() {
                                    Some(&self.curves)
                                } else {
                                    None
                                };
                                let ann_ref = self
                                    .wcs_solution
                                    .as_ref()
                                    .map(|w| (&self.annotation_config, w));
                                let (pixel_info, crop_action) = self.image_viewer.show(
                                    ui,
                                    img,
                                    &self.stf_params,
                                    self.image_counter + self.active_image_index * 1000,
                                    curves_ref,
                                    Some(&mut self.manual_crop),
                                    ann_ref,
                                );
                                self.hovered_pixel_info = pixel_info;
                                match crop_action {
                                    CropAction::Apply => self.apply_manual_crop(),
                                    CropAction::Cancel => self.cancel_manual_crop(),
                                    CropAction::Reset => self.manual_crop.reset_to_full(),
                                    CropAction::None => {}
                                }
                            }
                        }
                    }
                } else if self.images.is_empty() {
                    self.ui_empty_workspace(ui);
                } else {
                    if self.annotation_config.enabled {
                        let needs_solve = self
                            .images
                            .get(self.active_image_index)
                            .map(|im| {
                                self.wcs_solved_image_key
                                    != Some((self.active_image_index, im.width, im.height))
                            })
                            .unwrap_or(false);
                        if needs_solve {
                            self.solve_active_image_wcs();
                        }
                    }
                    let img = self.images.get(self.active_image_index).map(|arc| arc.as_ref());
                    let curves_ref = if self.curves_live_preview && !self.curves.is_identity() {
                        Some(&self.curves)
                    } else {
                        None
                    };
                    let ann_ref = self
                        .wcs_solution
                        .as_ref()
                        .map(|w| (&self.annotation_config, w));
                    let (pixel_info, crop_action) = self.image_viewer.show(
                        ui,
                        img,
                        &self.stf_params,
                        self.image_counter + self.active_image_index * 1000,
                        curves_ref,
                        Some(&mut self.manual_crop),
                        ann_ref,
                    );
                    self.hovered_pixel_info = pixel_info;
                    match crop_action {
                        CropAction::Apply => self.apply_manual_crop(),
                        CropAction::Cancel => self.cancel_manual_crop(),
                        CropAction::Reset => self.manual_crop.reset_to_full(),
                        CropAction::None => {}
                    }
                }
            });

        // Floating progress modal while stacking in background
        if self.is_stacking {
            egui::Window::new(crate::i18n::tr("🌌 Pinoaminen käynnissä"))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .frame(egui::Frame::window(&ctx.style()).fill(Color32::from_rgba_premultiplied(18, 22, 32, 245)))
                .show(ctx, |ui| {
                    ui.set_min_width(380.0);
                    ui.vertical_centered(|ui| {
                        ui.add_space(6.0);
                        ui.heading(RichText::new("⚡ PhotonForge Stacker").color(Color32::from_rgb(80, 200, 255)).strong());
                        ui.add_space(4.0);

                        if let Ok(lock) = self.stacking_progress.lock() {
                            if let Some(p) = lock.as_ref() {
                                ui.label(RichText::new(&p.stage).strong().size(15.0).color(Color32::from_rgb(255, 220, 100)));
                                ui.label(RichText::new(&p.detail).size(13.0).color(Color32::from_rgb(180, 190, 210)));
                                ui.add_space(8.0);
                                ui.add(
                                    egui::ProgressBar::new(p.fraction)
                                        .show_percentage()
                                        .animate(true),
                                );
                            } else {
                                ui.spinner();
                                ui.label(crate::i18n::tr("Alustetaan rinnakkaispinoamista..."));
                            }
                        }

                        ui.add_space(12.0);
                        if ui.button(RichText::new(crate::i18n::tr("⛔ Peruuta pinoaminen")).color(Color32::from_rgb(255, 100, 100)).strong()).clicked() {
                            self.stacking_cancel.store(true, Ordering::SeqCst);
                            self.status_message = crate::i18n::tr("Peruutetaan pinoamista...").to_string();
                        }
                        ui.add_space(4.0);
                    });
                });
        }

        // Floating progress modal while loading images in background
        if self.is_loading_files {
            egui::Window::new(crate::i18n::tr("📥 Ladataan kuvia..."))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .frame(egui::Frame::window(&ctx.style()).fill(Color32::from_rgba_premultiplied(18, 22, 32, 245)))
                .show(ctx, |ui| {
                    ui.set_min_width(420.0);
                    ui.vertical_centered(|ui| {
                        ui.add_space(6.0);
                        ui.heading(RichText::new(crate::i18n::tr("📂 Kuvien nopea rinnakkaistuonti")).color(Color32::from_rgb(80, 200, 255)).strong());
                        ui.add_space(4.0);

                        if let Ok(lock) = self.load_progress.lock() {
                            if let Some(p) = lock.as_ref() {
                                ui.label(
                                    RichText::new(format!(
                                        "Tuodaan kuvaa {} / {} ({:.0}%)",
                                        p.current,
                                        p.total,
                                        p.fraction * 100.0
                                    ))
                                    .strong()
                                    .size(15.0)
                                    .color(Color32::from_rgb(255, 220, 100)),
                                );
                                ui.label(
                                    RichText::new(&p.filename)
                                        .size(13.0)
                                        .color(Color32::from_rgb(180, 190, 210)),
                                );
                                ui.add_space(8.0);
                                ui.add(
                                    egui::ProgressBar::new(p.fraction)
                                        .show_percentage()
                                        .animate(true),
                                );
                            } else {
                                ui.spinner();
                                ui.label(crate::i18n::tr("Valmistellaan tiedostoja..."));
                            }
                        }

                        ui.add_space(12.0);
                        if ui
                            .button(
                                RichText::new(crate::i18n::tr("⛔ Peruuta tuonti"))
                                    .color(Color32::from_rgb(255, 100, 100))
                                    .strong(),
                            )
                            .clicked()
                        {
                            self.load_cancel.store(true, Ordering::SeqCst);
                            self.status_message = crate::i18n::tr("Peruutetaan kuvien tuontia...").to_string();
                        }
                        ui.add_space(4.0);
                    });
                });
        }

        // Floating progress modal while SyQon AI is running in background
        if self.is_syqon_running {
            egui::Window::new(crate::i18n::tr("✨ SyQon Neural AI -käsittely"))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .frame(egui::Frame::window(&ctx.style()).fill(Color32::from_rgba_premultiplied(18, 22, 32, 245)))
                .show(ctx, |ui| {
                    ui.set_min_width(400.0);
                    ui.vertical_centered(|ui| {
                        ui.add_space(6.0);
                        ui.heading(RichText::new("🧠 SyQon Neural Engine").color(Color32::from_rgb(140, 220, 255)).strong());
                        ui.add_space(4.0);

                        if let Ok(lock) = self.syqon_progress.lock() {
                            if let Some(p) = lock.as_ref() {
                                ui.label(RichText::new(&p.stage).strong().size(15.0).color(Color32::from_rgb(255, 220, 100)));
                                ui.label(RichText::new(&p.detail).size(13.0).color(Color32::from_rgb(180, 190, 210)));
                                ui.add_space(8.0);
                                ui.add(
                                    egui::ProgressBar::new(p.fraction)
                                        .show_percentage()
                                        .animate(true),
                                );
                            } else {
                                ui.spinner();
                                ui.label(crate::i18n::tr("Käynnistetään SyQon-neuroverkkoa..."));
                            }
                        }

                        ui.add_space(12.0);
                        if ui.button(RichText::new(crate::i18n::tr("⛔ Peruuta käsittely")).color(Color32::from_rgb(255, 100, 100)).strong()).clicked() {
                            self.syqon_cancel.store(true, Ordering::SeqCst);
                            self.status_message = crate::i18n::tr("Peruutetaan SyQon AI -käsittelyä...").to_string();
                        }
                        ui.add_space(4.0);
                    });
                });
        }

        // Reset confirmation modal dialog
        if self.show_reset_dialog {
            egui::Window::new(crate::i18n::tr("🔄 Aloita alusta"))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .frame(egui::Frame::window(&ctx.style()).fill(Color32::from_rgba_premultiplied(22, 26, 36, 250)))
                .show(ctx, |ui| {
                    ui.set_min_width(340.0);
                    ui.vertical_centered(|ui| {
                        ui.add_space(4.0);
                        ui.heading(RichText::new(crate::i18n::tr("⚠️ Tyhjennetäänkö työtila?")).color(Color32::from_rgb(255, 200, 80)));
                        ui.add_space(8.0);
                        ui.label(format!(
                            "Haluatko varmasti aloittaa alusta? Kaikki muistissa olevat kuvat ({} kpl) poistetaan.",
                            self.images.len()
                        ));
                        ui.label(RichText::new(crate::i18n::tr("Varmista, että olet tallentanut valmiin lopputuloksen!")).small().color(Color32::from_rgb(180, 190, 210)));
                        ui.add_space(14.0);

                        ui.horizontal(|ui| {
                            if ui.button(RichText::new(crate::i18n::tr("❌ Peruuta")).size(14.0)).clicked() {
                                self.show_reset_dialog = false;
                            }
                            ui.add_space(20.0);
                            if ui.button(
                                RichText::new(crate::i18n::tr("🗑 Kyllä, aloita alusta"))
                                    .color(Color32::from_rgb(255, 90, 90))
                                    .strong()
                                    .size(14.0),
                            ).clicked() {
                                self.reset_workspace();
                            }
                        });
                        ui.add_space(4.0);
                    });
                });
        }

        // Floating progress modal while SyQon AI is running in background
        if self.is_syqon_running {
            egui::Window::new(crate::i18n::tr("✨ SyQon Neural AI -käsittely"))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .frame(egui::Frame::window(&ctx.style()).fill(Color32::from_rgba_premultiplied(18, 22, 32, 245)))
                .show(ctx, |ui| {
                    ui.set_min_width(400.0);
                    ui.vertical_centered(|ui| {
                        ui.add_space(6.0);
                        ui.heading(RichText::new("🧠 SyQon Neural Engine").color(Color32::from_rgb(140, 220, 255)).strong());
                        ui.add_space(4.0);

                        if let Ok(lock) = self.syqon_progress.lock() {
                            if let Some(p) = lock.as_ref() {
                                ui.label(RichText::new(&p.stage).strong().size(15.0).color(Color32::from_rgb(255, 220, 100)));
                                ui.label(RichText::new(&p.detail).size(13.0).color(Color32::from_rgb(180, 190, 210)));
                                ui.add_space(8.0);
                                ui.add(
                                    egui::ProgressBar::new(p.fraction)
                                        .show_percentage()
                                        .animate(true),
                                );
                            } else {
                                ui.spinner();
                                ui.label(crate::i18n::tr("Käynnistetään SyQon-neuroverkkoa..."));
                            }
                        }

                        ui.add_space(12.0);
                        if ui.button(RichText::new(crate::i18n::tr("⛔ Peruuta käsittely")).color(Color32::from_rgb(255, 100, 100)).strong()).clicked() {
                            self.syqon_cancel.store(true, Ordering::SeqCst);
                            self.status_message = crate::i18n::tr("Peruutetaan SyQon AI -käsittelyä...").to_string();
                        }
                        ui.add_space(4.0);
                    });
                });
        }

        // Floating Curves Transformation Window
        let mut show_curves = self.show_curves_window;
        if show_curves {
            egui::Window::new(crate::i18n::tr("📈 Käyrät (Curves Transformation)"))
                .open(&mut show_curves)
                .resizable(true)
                .default_width(320.0)
                .show(ctx, |ui| {
                    self.ui_curves_editor(ui);
                });
            self.show_curves_window = show_curves;
        }

        // Floating Rotation Window / Dialog
        if self.show_rotation_dialog {
            egui::Window::new(crate::i18n::tr("🔄 Kuvan pyöritys ja peilaus"))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .frame(egui::Frame::window(&ctx.style()).fill(Color32::from_rgba_premultiplied(20, 24, 34, 250)))
                .show(ctx, |ui| {
                    ui.set_min_width(380.0);
                    ui.vertical_centered(|ui| {
                        ui.add_space(4.0);
                        ui.heading(RichText::new(crate::i18n::tr("🔄 Kuvan pyöritys ja peilaus")).color(Color32::from_rgb(80, 200, 255)).strong());
                        ui.add_space(8.0);
                    });

                    ui.group(|ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(crate::i18n::tr("Pikapyöritys 90° / 180° ja peilaus:")).strong());
                        ui.add_space(4.0);
                        ui.horizontal_wrapped(|ui| {
                            if ui.button(crate::i18n::tr("↷ 90° Myötäpäivään")).clicked() {
                                self.rotate_active_90_cw();
                                self.show_rotation_dialog = false;
                            }
                            if ui.button(crate::i18n::tr("↶ 90° Vastapäivään")).clicked() {
                                self.rotate_active_90_ccw();
                                self.show_rotation_dialog = false;
                            }
                            if ui.button("↻ 180°").clicked() {
                                self.rotate_active_180();
                                self.show_rotation_dialog = false;
                            }
                        });
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            if ui.button(crate::i18n::tr("⇄ Peilaa vaakasuunnassa (Flip H)")).clicked() {
                                self.flip_active_horizontal();
                                self.show_rotation_dialog = false;
                            }
                            if ui.button(crate::i18n::tr("⇅ Peilaa pystysuunnassa (Flip V)")).clicked() {
                                self.flip_active_vertical();
                                self.show_rotation_dialog = false;
                            }
                        });
                    });

                    ui.add_space(8.0);

                    ui.group(|ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(crate::i18n::tr("Vapaa kulma (Arbitrary Rotation):")).strong());
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(crate::i18n::tr("Kulma:"));
                            ui.add(egui::Slider::new(&mut self.rotation_angle, -180.0..=180.0).suffix("°").step_by(0.1));
                        });
                        ui.horizontal_wrapped(|ui| {
                            if ui.small_button("-90°").clicked() { self.rotation_angle = -90.0; }
                            if ui.small_button("-45°").clicked() { self.rotation_angle = -45.0; }
                            if ui.small_button("-1.0°").clicked() { self.rotation_angle -= 1.0; }
                            if ui.small_button("-0.1°").clicked() { self.rotation_angle -= 0.1; }
                            if ui.small_button("0.0°").clicked() { self.rotation_angle = 0.0; }
                            if ui.small_button("+0.1°").clicked() { self.rotation_angle += 0.1; }
                            if ui.small_button("+1.0°").clicked() { self.rotation_angle += 1.0; }
                            if ui.small_button("+45°").clicked() { self.rotation_angle = 45.0; }
                            if ui.small_button("+90°").clicked() { self.rotation_angle = 90.0; }
                        });
                        ui.checkbox(&mut self.rotation_auto_crop, crate::i18n::tr("✂️ Rajaa mustat reunat automaattisesti"));
                        ui.add_space(4.0);
                        if ui.add(egui::Button::new(RichText::new(crate::i18n::tr("🔄 Sovella vapaa pyöritys")).color(Color32::from_rgb(100, 255, 150)).strong()).min_size(Vec2::new(ui.available_width(), 26.0))).clicked() {
                            let angle = self.rotation_angle;
                            let auto_crop = self.rotation_auto_crop;
                            self.rotate_active_arbitrary(angle, auto_crop);
                            self.show_rotation_dialog = false;
                        }
                    });

                    ui.add_space(8.0);
                    ui.vertical_centered(|ui| {
                        if ui.button(RichText::new(crate::i18n::tr("Sulje")).size(14.0)).clicked() {
                            self.show_rotation_dialog = false;
                        }
                    });
                    ui.add_space(4.0);
                });
        }
    }
}

impl PhotonForgeApp {
    fn ui_empty_workspace(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            let total_h = ui.available_height();
            ui.add_space((total_h * 0.15).max(30.0));

            ui.heading(
                RichText::new("⚡ PhotonForge")
                    .size(28.0)
                    .color(Color32::from_rgb(100, 210, 255))
                    .strong(),
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new(crate::i18n::tr("Ei avattuja kuvia työtilassa."))
                    .size(16.0)
                    .color(Color32::from_rgb(210, 225, 245)),
            );
            ui.label(
                RichText::new(crate::i18n::tr("Avaa kuvia tiedostovalitsimesta tai raahaa ne suoraan tähän ikkunaan."))
                    .small()
                    .color(Color32::from_rgb(160, 175, 195)),
            );

            ui.add_space(16.0);

            if ui
                .add(
                    egui::Button::new(
                        RichText::new(crate::i18n::tr("📂 Avaa kuvatiedostoja..."))
                            .size(15.0)
                            .strong()
                            .color(Color32::from_rgb(50, 255, 160)),
                    )
                    .min_size(Vec2::new(220.0, 38.0)),
                )
                .clicked()
            {
                self.open_files_dialog();
            }

            ui.add_space(20.0);

            ui.group(|ui| {
                ui.set_max_width(520.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(crate::i18n::tr("Tuetut tiedostomuodot:"))
                            .strong()
                            .color(Color32::from_rgb(255, 215, 120)),
                    );
                    ui.add_space(4.0);
                    ui.label(crate::i18n::tr("• FITS-tähtikuvat (.fits, .fit, 8/16/32-bit ja IEEE float)"));
                    ui.label(crate::i18n::tr("• PixInsight -kuvatiedostot (.xisf)"));
                    ui.label(crate::i18n::tr("• 16-bittiset TIFF-kuvat (.tif, .tiff)"));
                    ui.label(crate::i18n::tr("• PNG-kuvat (.png)"));

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(4.0);

                    ui.label(
                        RichText::new(crate::i18n::tr("Tuetut kaukoputket ja laitteet:"))
                            .strong()
                            .color(Color32::from_rgb(140, 220, 255)),
                    );
                    ui.add_space(4.0);
                    ui.label(crate::i18n::tr("• ZWO Seestar S50, S50 Pro, S30 ja S30 Pro (Alpaca + SMB)"));
                    ui.label(crate::i18n::tr("• Celestron Origin RASA (Alpaca REST)"));
                    ui.label(crate::i18n::tr("• ASCOM Alpaca -jalustat (Sky-Watcher, Pegasus Astro, AM5 jne.)"));
                });
            });
        });
    }

    fn ui_files_tab(&mut self, ui: &mut egui::Ui) {
        ui.set_width(ui.available_width());
        ui.heading(crate::i18n::tr("📁 Kuvat"));
        let count_str = if self.images.is_empty() {
            crate::i18n::tr("Ei kuvia työtilassa").to_string()
        } else {
            format!("{}: {}", crate::i18n::tr("Kuvia muistissa"), self.images.len())
        };
        ui.label(RichText::new(count_str).color(Color32::from_rgb(160, 180, 205)));

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.is_loading_files, egui::Button::new(crate::i18n::tr("➕ Lisää kuvia")))
                .on_hover_text(crate::i18n::tr("Valitse kuvatiedostoja tai kansioita"))
                .clicked()
            {
                self.open_files_dialog();
            }

            if ui
                .add_enabled(!self.images.is_empty() && !self.is_loading_files, egui::Button::new(crate::i18n::tr("🗑 Tyhjennä kaikki")))
                .on_hover_text(crate::i18n::tr("Poista kaikki kuvat muistista ja aloita alusta"))
                .clicked()
            {
                self.show_reset_dialog = true;
            }
        });

        if self.images.is_empty() && !self.is_loading_files {
            ui.add_space(8.0);
            ui.group(|ui| {
                ui.set_width(ui.available_width());
                ui.vertical(|ui| {
                    ui.label(RichText::new(crate::i18n::tr("Kuvalista on tyhjä.")).color(Color32::from_rgb(255, 200, 100)));
                    ui.add_space(2.0);
                    ui.label(RichText::new(crate::i18n::tr("Avaa kuvia 'Lisää kuvia' -napilla tai raahaa tiedostot tähän ikkunaan.")).small().color(Color32::from_rgb(160, 175, 195)));
                });
            });
        }

        if self.is_loading_files {
            ui.add_space(6.0);
            ui.group(|ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.spinner();
                    if let Ok(lock) = self.load_progress.lock() {
                        if let Some(p) = lock.as_ref() {
                            ui.label(RichText::new(format!("{} ({}/{})...", crate::i18n::tr("Tuodaan kuvia"), p.current, p.total)).strong());
                        } else {
                            ui.label(crate::i18n::tr("Tuodaan kuvia rinnakkain..."));
                        }
                    }
                });
            });
        }

        ui.add_space(8.0);

        let mut remove_index = None;
        let mut select_index = None;
        let mut change_filter = None;

        for (i, img) in self.images.iter().enumerate() {
            let is_selected = i == self.active_image_index;

            ui.group(|ui| {
                ui.set_width(ui.available_width());

                let badge = if let Some(ref filt) = img.metadata.filter_name {
                    if img.channels == 3 {
                        format!("RGB:{}", filt)
                    } else {
                        format!("Mono:{}", filt)
                    }
                } else if img.channels == 3 {
                    if let Some(b) = &img.metadata.bayer_pattern {
                        format!("RGB:{}", b)
                    } else {
                        "RGB".to_string()
                    }
                } else if let Some(b) = &img.metadata.bayer_pattern {
                    format!("CFA:{}", b)
                } else {
                    "Mono".to_string()
                };

                let badge_color = match img.channels {
                    1 => Color32::from_rgb(180, 200, 230),
                    _ => Color32::from_rgb(100, 220, 255),
                };

                ui.horizontal(|ui| {
                    let action_space = 28.0;
                    let title_w = (ui.available_width() - action_space).max(80.0);

                    ui.allocate_ui_with_layout(
                        Vec2::new(title_w, 20.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            let prefix = format!("{}. [{}] ", i + 1, badge);
                            ui.label(RichText::new(prefix).color(badge_color).strong().size(12.0));

                            let display_name = if img.metadata.file_name.chars().count() > 32 {
                                format!("{}...", img.metadata.file_name.chars().take(30).collect::<String>())
                            } else {
                                img.metadata.file_name.clone()
                            };

                            if ui.selectable_label(is_selected, display_name).on_hover_text(&img.metadata.file_name).clicked() {
                                select_index = Some(i);
                            }
                        },
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("❌").on_hover_text(crate::i18n::tr("Poista kuva listalta")).clicked() {
                            remove_index = Some(i);
                        }
                    });
                });

                if is_selected {
                    ui.separator();
                    ui.label(format!("{}: {} × {} px", crate::i18n::tr("Koko"), img.width, img.height));
                    let chan_desc = if img.channels == 3 {
                        if let Some(b) = &img.metadata.bayer_pattern {
                            format!("3 (RGB, {} {})", crate::i18n::tr("kuvio:"), b)
                        } else {
                            "3 (RGB)".to_string()
                        }
                    } else if let Some(b) = &img.metadata.bayer_pattern {
                        format!("1 (CFA Bayer: {})", b)
                    } else {
                        format!("1 ({})", crate::i18n::tr("Mono"))
                    };
                    ui.label(format!("{}: {}", crate::i18n::tr("Kanavat"), chan_desc));
                    if let Some(exp) = img.metadata.exposure_time {
                        ui.label(format!("{}: {:.1} s", crate::i18n::tr("Valotusaika"), exp));
                    }
                    if let Some(cam) = &img.metadata.camera {
                        ui.label(format!("{}: {}", crate::i18n::tr("Kamera"), cam));
                    }

                    // Filter assignment selector
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(crate::i18n::tr("Suodatin:")).strong());
                        let filters = [
                            AstroFilter::Luminance,
                            AstroFilter::Red,
                            AstroFilter::Green,
                            AstroFilter::Blue,
                            AstroFilter::HAlpha,
                            AstroFilter::OIII,
                            AstroFilter::SII,
                        ];
                        for f in filters {
                            let is_curr = img.metadata.filter_name.as_deref() == Some(f.badge());
                            if ui.selectable_label(is_curr, f.badge()).clicked() {
                                change_filter = Some((i, f.badge().to_string()));
                            }
                        }
                    });
                }
            });
            ui.add_space(2.0);
        }

        if let Some((idx, filt)) = change_filter {
            if let Some(img) = self.images.get(idx) {
                let mut updated = (**img).clone();
                updated.metadata.filter_name = Some(filt);
                self.images[idx] = Arc::new(updated);
                self.image_counter += 1;
            }
        }

        if let Some(i) = select_index {
            self.active_image_index = i;
            self.image_counter += 1;
            if let Some(img) = self.active_image() {
                self.stf_params = StfParameters::compute_auto(img, self.stf_params.linked_channels);
            }
        }

        if let Some(i) = remove_index {
            self.push_undo(crate::i18n::tr("Kuvan poisto"));
            self.images.remove(i);
            if self.active_image_index >= self.images.len() && !self.images.is_empty() {
                self.active_image_index = self.images.len() - 1;
            }
            self.image_counter += 1;
        }

        if self.images.len() >= 2 {
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                if ui.button(crate::i18n::tr("🌌 Siirry pinoamiseen")).clicked() {
                    self.active_tab = ActiveTab::Stacking;
                }
                if ui.button(crate::i18n::tr("🌈 Siirry kanavayhdistykseen")).clicked() {
                    self.active_tab = ActiveTab::ChannelCombination;
                }
            });
        }
    }

    fn ui_stf_tab(&mut self, ui: &mut egui::Ui) {
        let tab_w = ui.available_width();
        ui.set_width(tab_w);
        ui.set_max_width(tab_w);
        ui.heading(crate::i18n::tr("⚡ Näyttövenytys (STF)"));
        ui.label(RichText::new(crate::i18n::tr("Reaaliaikainen näyttövenytys muuttamatta alkuperäistä raw-dataa.")).small().color(Color32::from_rgb(160, 180, 210)));

        ui.add_space(8.0);
        ui.group(|ui| {
            let gw = ui.available_width();
            ui.set_width(gw);
            ui.set_max_width(gw);
            ui.spacing_mut().slider_width = (gw - 76.0).max(100.0);
            ui.strong(crate::i18n::tr("⚙️ Automaattinen venytys:"));
            ui.add_space(4.0);
            ui.checkbox(&mut self.stf_params.enabled, crate::i18n::tr("STF käytössä"));
            ui.checkbox(&mut self.stf_params.linked_channels, crate::i18n::tr("Linkitetyt RGB-kanavat"));

            ui.add_space(4.0);
            ui.label(crate::i18n::tr("Taustan tavoitetummuus:"));
            if ui.add(egui::Slider::new(&mut self.stf_params.target_bg, 0.04..=0.25).step_by(0.01).trailing_fill(true)).changed() {
                self.run_auto_stf();
            }

            ui.add_space(4.0);
            ui.label(crate::i18n::tr("Pika-asetukset:"));
            ui.horizontal_wrapped(|ui| {
                if ui.button(crate::i18n::tr("🌌 Syvä (0.08)")).on_hover_text(crate::i18n::tr("Syvä avaruus, tumma taivas (0.08)")).clicked() {
                    self.stf_params.target_bg = 0.08;
                    self.run_auto_stf();
                }
                if ui.button(crate::i18n::tr("🪐 Standardi (0.10)")).on_hover_text(crate::i18n::tr("Normaali tasapainoinen tausta (0.10)")).clicked() {
                    self.stf_params.target_bg = 0.10;
                    self.run_auto_stf();
                }
                if ui.button(crate::i18n::tr("🔭 Vaalea (0.20)")).on_hover_text(crate::i18n::tr("Vaalea tausta himmeille sumuille (0.20)")).clicked() {
                    self.stf_params.target_bg = 0.20;
                    self.run_auto_stf();
                }
            });

            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if ui.add(egui::Button::new(RichText::new(crate::i18n::tr("⚡ Laske automaattivenytys")).strong()).min_size(Vec2::new(140.0, 24.0))).clicked() {
                    self.run_auto_stf();
                }
                if ui.add(egui::Button::new(crate::i18n::tr("Palauta oletukset")).min_size(Vec2::new(90.0, 24.0))).clicked() {
                    self.stf_params.channels = [ChannelStf::default(), ChannelStf::default(), ChannelStf::default()];
                    self.image_counter += 1;
                }
            });
        });

        ui.add_space(8.0);
        ui.group(|ui| {
            let gw = ui.available_width();
            ui.set_width(gw);
            ui.set_max_width(gw);
            ui.spacing_mut().slider_width = (gw - 76.0).max(100.0);
            ui.strong(crate::i18n::tr("🎚 Manuaaliset säätimet:"));
            ui.add_space(4.0);

            let ch = &mut self.stf_params.channels[0];
            let mut changed = false;

            ui.label(crate::i18n::tr("Varjot:"));
            if ui.add(egui::Slider::new(&mut ch.shadows, 0.0..=0.5).step_by(0.001).trailing_fill(true)).changed() {
                changed = true;
            }

            ui.label(crate::i18n::tr("Keskisävyt:"));
            if ui.add(egui::Slider::new(&mut ch.midtones, 0.001..=0.999).step_by(0.001).logarithmic(true).trailing_fill(true)).changed() {
                changed = true;
            }

            ui.label(crate::i18n::tr("Kirkkaat sävyt:"));
            if ui.add(egui::Slider::new(&mut ch.highlights, 0.5..=1.0).step_by(0.001).trailing_fill(true)).changed() {
                changed = true;
            }

            if changed {
                let sync_ch = *ch;
                self.stf_params.channels = [sync_ch, sync_ch, sync_ch];
                self.image_counter += 1;
            }

            ui.add_space(6.0);
            let btn_w = ui.available_width();
            if ui.add_sized(
                [btn_w, 26.0],
                egui::Button::new(RichText::new(crate::i18n::tr("💾 Tee venytyksestä pysyvä (Histogrammi)")).strong().color(Color32::from_rgb(100, 220, 255))),
            ).clicked() {
                self.apply_permanent_stf();
            }
        });
    }

    fn ui_stacking_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.stacking_mode, StackingTabMode::Standard, crate::i18n::tr("🌌 Pinoaminen"));
            ui.selectable_value(&mut self.stacking_mode, StackingTabMode::Mosaic, crate::i18n::tr("🧩 Mosaiikit"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(RichText::new(crate::i18n::tr("⚡ Live-pino")).color(Color32::from_rgb(100, 220, 255))).clicked() {
                    self.active_tab = ActiveTab::LiveStacking;
                }
            });
        });
        ui.separator();

        match self.stacking_mode {
            StackingTabMode::Standard => self.ui_stacking_subtab_standard(ui),
            StackingTabMode::Mosaic => self.ui_stacking_subtab_mosaic(ui),
        }
    }

    fn ui_stacking_subtab_standard(&mut self, ui: &mut egui::Ui) {
        ui.set_width(ui.available_width());
        ui.heading(crate::i18n::tr("🌌 Kuvien pinoaminen"));
        ui.label(format!("{}: {}", crate::i18n::tr("Pinottavia kuvia"), self.images.len()));

        let mut filter_counts: BTreeMap<String, usize> = BTreeMap::new();
        for img in &self.images {
            let f = img.metadata.filter_name.clone().unwrap_or_else(|| crate::i18n::tr("Tuntematon").to_string());
            *filter_counts.entry(f).or_insert(0) += 1;
        }

        if filter_counts.len() > 1 {
            ui.add_space(6.0);
            ui.group(|ui| {
                ui.set_width(ui.available_width());
                ui.strong(crate::i18n::tr("🏷 Havaitut suodatinryhmät:"));
                let mut summary = Vec::new();
                for (filt, count) in &filter_counts {
                    summary.push(format!("{}: {}", filt, count));
                }
                ui.label(summary.join("  |  "));

                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(crate::i18n::tr("Pinoamiskohde:"));
                    let all_label = format!("{} ({})", crate::i18n::tr("Kaikki"), self.images.len());
                    ui.selectable_value(&mut self.selected_stack_filter, None, all_label);
                    for (filt, count) in &filter_counts {
                        let label = format!("{} ({})", filt, count);
                        ui.selectable_value(&mut self.selected_stack_filter, Some(filt.clone()), label);
                    }
                });

                let valid_groups_count = filter_counts.values().filter(|&&c| c >= 2).count();
                if valid_groups_count > 1 && !self.is_stacking {
                    ui.add_space(6.0);
                    if ui
                        .add(egui::Button::new(
                            RichText::new(format!(
                                "🚀 {} ({} {})",
                                crate::i18n::tr("Pinoa kaikki suodatinryhmät sarjana"),
                                valid_groups_count,
                                crate::i18n::tr("ryhmää")
                            ))
                            .color(Color32::from_rgb(100, 220, 255))
                            .strong(),
                        ).min_size(Vec2::new(ui.available_width(), 26.0)))
                        .on_hover_text(crate::i18n::tr("Pinotaan kukin suodatinryhmä automaattisesti erilliseksi master-kuvaksi"))
                        .clicked()
                    {
                        self.run_batch_stacking();
                    }
                }
            });
        }

        ui.add_space(8.0);
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.strong(crate::i18n::tr("⚙️ Kohdistus & Normalisointi:"));

            ui.checkbox(&mut self.stacking_config.align_stars, crate::i18n::tr("✨ Tähtien automaattinen kohdistus"));
            ui.label(RichText::new(crate::i18n::tr("Korjaa kentänkiertymän ja jalustan seurantavirheet ennen pinoamista.")).small().color(Color32::from_rgb(140, 170, 200)));

            ui.checkbox(&mut self.stacking_config.normalize_background, crate::i18n::tr("🌐 Taustataivaan tasoitus"));
            ui.label(RichText::new(crate::i18n::tr("Tasaa taustataivaan valoisuuden eri öiltä otetuissa kuvissa.")).small().color(Color32::from_rgb(140, 170, 200)));

            ui.checkbox(&mut self.stacking_config.auto_crop_borders, crate::i18n::tr("✂️ Automaattinen reunojen rajaus"));
            ui.label(RichText::new(crate::i18n::tr("Leikkaa automaattisesti pois osittaisen peiton epätasaiset reunat.")).small().color(Color32::from_rgb(140, 170, 200)));

            ui.add_space(4.0);
            let mut pct = (self.stacking_config.min_coverage_ratio * 100.0).round() as i32;
            if ui.add(egui::Slider::new(&mut pct, 0..=100).prefix(crate::i18n::tr("Minimipeitto: ")).suffix("%")).changed() {
                self.stacking_config.min_coverage_ratio = pct as f32 / 100.0;
            }
        });

        ui.add_space(8.0);
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.strong(crate::i18n::tr("📊 Pinoamismenetelmä ja hylkäys:"));
            ui.add_space(2.0);
            ui.radio_value(&mut self.stacking_config.method, StackingMethod::SigmaClipping, crate::i18n::tr("Sigma-hylkäys (Suositus)"));
            ui.radio_value(&mut self.stacking_config.method, StackingMethod::Average, crate::i18n::tr("Keskiarvo"));
            ui.radio_value(&mut self.stacking_config.method, StackingMethod::Median, crate::i18n::tr("Mediaani"));
            ui.radio_value(&mut self.stacking_config.method, StackingMethod::MinMax, crate::i18n::tr("Minimi/Maksimi -hylkäys"));

            if self.stacking_config.method == StackingMethod::SigmaClipping {
                ui.separator();
                ui.label(crate::i18n::tr("Hylkäysrajat (Sigma):"));
                ui.add(egui::Slider::new(&mut self.stacking_config.sigma_low, 1.0..=5.0).prefix(crate::i18n::tr("Alaraja (Sigma): ")).text(crate::i18n::tr("(tummat pisteet)")));
                ui.add(egui::Slider::new(&mut self.stacking_config.sigma_high, 1.0..=5.0).prefix(crate::i18n::tr("Yläraja (Sigma): ")).text(crate::i18n::tr("(satelliitit ja tähdet)")));
            }
        });

        ui.add_space(8.0);
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.strong(crate::i18n::tr("🔍 Resoluution skaalaus (Drizzle):"));
            ui.horizontal_wrapped(|ui| {
                ui.radio_value(&mut self.stacking_config.drizzle_scale, 1, crate::i18n::tr("1x Normaali"));
                ui.radio_value(&mut self.stacking_config.drizzle_scale, 2, crate::i18n::tr("2x Drizzle (Kaksinkertainen resoluutio)"));
            });
            ui.label(RichText::new(crate::i18n::tr("2x Drizzle muodostaa alipikselisiirtymistä kaksinkertaisen resoluution ja pyöreämmät tähdet.")).small().color(Color32::from_rgb(140, 170, 200)));
        });

        ui.add_space(16.0);
        if self.is_stacking {
            ui.group(|ui| {
                ui.set_width(ui.available_width());
                ui.heading(RichText::new(crate::i18n::tr("⏳ Pinoaminen käynnissä...")).color(Color32::from_rgb(100, 220, 255)));
                if let Ok(lock) = self.stacking_progress.lock() {
                    if let Some(p) = lock.as_ref() {
                        ui.label(RichText::new(&p.stage).strong().color(Color32::from_rgb(255, 220, 100)));
                        ui.label(RichText::new(&p.detail).small());
                        ui.add(egui::ProgressBar::new(p.fraction).show_percentage().animate(true));
                    }
                }
                ui.add_space(8.0);
                if ui.button(RichText::new(crate::i18n::tr("⛔ Peruuta pinoaminen")).color(Color32::from_rgb(255, 100, 100))).clicked() {
                    self.stacking_cancel.store(true, Ordering::SeqCst);
                    self.status_message = crate::i18n::tr("Peruutetaan pinoamista...").to_string();
                }
            });
        } else {
            let stack_btn_label = if let Some(ref f) = self.selected_stack_filter {
                let cnt = filter_counts.get(f).copied().unwrap_or(0);
                format!("🚀 {}: {} ({} {})", crate::i18n::tr("Suorita pinoaminen"), f, cnt, crate::i18n::tr("kuvaa"))
            } else {
                format!("🚀 {} ({} {})", crate::i18n::tr("Suorita pinoaminen"), self.images.len(), crate::i18n::tr("kuvaa"))
            };

            let can_stack = if let Some(ref f) = self.selected_stack_filter {
                filter_counts.get(f).copied().unwrap_or(0) >= 2
            } else {
                self.images.len() >= 2
            };

            if ui
                .add_enabled(
                    can_stack,
                    egui::Button::new(
                        RichText::new(stack_btn_label)
                            .color(Color32::from_rgb(50, 255, 150))
                            .strong()
                            .size(15.0),
                    ).min_size(Vec2::new(ui.available_width(), 32.0)),
                )
                .clicked()
            {
                self.run_stacking();
            }
        }
    }

    fn ui_stacking_subtab_mosaic(&mut self, ui: &mut egui::Ui) {
        ui.set_width(ui.available_width());
        ui.heading(crate::i18n::tr("🧩 Mosaiikkien yhdistäminen"));
        ui.label(RichText::new(crate::i18n::tr("Yhdistää useat eri taivaanaloihin osoittavat ruudut laajennetulle yhteiselle kankaalle tähtikohdistuksen ja saumahäivytyksen avulla.")).small().color(Color32::from_rgb(160, 180, 210)));

        ui.add_space(8.0);
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.strong(format!("{}: {}", crate::i18n::tr("Valmiit ruudut muistissa"), self.images.len()));
            if self.images.is_empty() {
                ui.label(RichText::new(crate::i18n::tr("Lataa mosaiikkiruudut (FITS/TIFF) yläpalkin 'Avaa kuvia' -painikkeella.")).color(Color32::from_rgb(255, 200, 100)));
            } else {
                for (idx, img) in self.images.iter().enumerate() {
                    let name = if img.metadata.file_name.is_empty() {
                        format!("{} {}", crate::i18n::tr("Kuva"), idx + 1)
                    } else {
                        img.metadata.file_name.clone()
                    };
                    ui.label(format!("• {} ({} × {} px)", name, img.width, img.height));
                }
            }
        });

        ui.add_space(8.0);
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.strong(crate::i18n::tr("⚙️ Yhdistämisasetukset:"));

            ui.add_space(4.0);
            ui.add(egui::Slider::new(&mut self.mosaic_stitch_config.feather_radius_px, 10.0..=120.0).prefix(crate::i18n::tr("Sauman häivytys: ")).suffix(" px"));
            ui.label(RichText::new(crate::i18n::tr("Pehmentää ruutujen väliset saumat liukuvasti, jottei paneelien rajakohtia erota.")).small().color(Color32::from_rgb(140, 170, 200)));

            ui.add_space(4.0);
            ui.checkbox(&mut self.mosaic_stitch_config.normalize_background, crate::i18n::tr("🌐 Taustataivaan tasoitus"));
            ui.label(RichText::new(crate::i18n::tr("Tasaa taustataivaan valoisuuserot eri paneelien välillä.")).small().color(Color32::from_rgb(140, 170, 200)));

            ui.add_space(4.0);
            ui.checkbox(&mut self.mosaic_stitch_config.auto_crop, crate::i18n::tr("✂️ Automaattinen rajaus"));
            ui.label(RichText::new(crate::i18n::tr("Leikkaa epätasaiset ulkoreunat pois valmiista mosaiikista.")).small().color(Color32::from_rgb(140, 170, 200)));
        });

        ui.add_space(16.0);
        if self.is_stacking {
            ui.group(|ui| {
                ui.set_width(ui.available_width());
                ui.heading(RichText::new(crate::i18n::tr("⏳ Mosaiikkia yhdistetään...")).color(Color32::from_rgb(100, 220, 255)));
                if let Ok(lock) = self.stacking_progress.lock() {
                    if let Some(p) = lock.as_ref() {
                        ui.label(RichText::new(&p.stage).strong().color(Color32::from_rgb(255, 220, 100)));
                        ui.label(RichText::new(&p.detail).small());
                        ui.add(egui::ProgressBar::new(p.fraction).show_percentage().animate(true));
                    }
                }
                ui.add_space(8.0);
                if ui.button(RichText::new(crate::i18n::tr("⛔ Peruuta")).color(Color32::from_rgb(255, 100, 100))).clicked() {
                    self.stacking_cancel.store(true, Ordering::SeqCst);
                    self.status_message = crate::i18n::tr("Peruutetaan mosaiikin yhdistämistä...").to_string();
                }
            });
        } else {
            let can_stitch = self.images.len() >= 2;
            let btn_label = if can_stitch {
                format!("🧩 {} ({} {})", crate::i18n::tr("Yhdistä mosaiikki"), self.images.len(), crate::i18n::tr("ruutua"))
            } else {
                crate::i18n::tr("🧩 Yhdistä mosaiikki (vaatii väh. 2 kuvaa)").to_string()
            };

            if ui.add_enabled(
                can_stitch,
                egui::Button::new(RichText::new(btn_label).color(Color32::from_rgb(80, 255, 180)).strong().size(15.0))
                    .min_size(Vec2::new(ui.available_width(), 32.0)),
            ).clicked() {
                self.run_mosaic_stitching();
            }
        }
    }

    fn render_image_combo(images: &[Arc<AstroImage>], ui: &mut egui::Ui, id_salt: &str, selected_index: &mut Option<usize>) {
        let current_text = match *selected_index {
            Some(idx) => {
                if let Some(img) = images.get(idx) {
                    let filt = img.metadata.filter_name.as_deref().unwrap_or("-");
                    let fname = if img.metadata.file_name.chars().count() > 22 {
                        format!("{}...", img.metadata.file_name.chars().take(20).collect::<String>())
                    } else {
                        img.metadata.file_name.clone()
                    };
                    format!("{}. [{}] {}", idx + 1, filt, fname)
                } else {
                    crate::i18n::tr("-- Valitse kuva --").to_string()
                }
            }
            None => crate::i18n::tr("-- Valitse kuva --").to_string(),
        };

        let combo_w = (ui.available_width() - 8.0).max(180.0);
        egui::ComboBox::from_id_salt(id_salt)
            .selected_text(current_text)
            .width(combo_w)
            .show_ui(ui, |ui| {
                ui.selectable_value(selected_index, None, crate::i18n::tr("-- Ei valittu --"));
                for (i, img) in images.iter().enumerate() {
                    let filt = img.metadata.filter_name.as_deref().unwrap_or("-");
                    let text = format!("{}. [{}] {}", i + 1, filt, img.metadata.file_name);
                    ui.selectable_value(selected_index, Some(i), text);
                }
            });
    }

    fn ui_live_stacking_sidebar(&mut self, ui: &mut egui::Ui) {
        ui.set_width(ui.available_width());
        let stats = self.live_stacker.get_stats();
        let mut cfg = self.live_stacker.get_config();

        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.heading(RichText::new(crate::i18n::tr("⚡ Live-pinoaminen")).color(Color32::from_rgb(100, 220, 255)).strong());
                if stats.is_running {
                    ui.label(RichText::new(crate::i18n::tr("● AKTIIVINEN")).color(Color32::from_rgb(80, 255, 140)).strong());
                } else {
                    ui.label(RichText::new(crate::i18n::tr("● TAUKO")).color(Color32::from_rgb(255, 200, 80)).strong());
                }
            });

            ui.add_space(4.0);

            // Controls: Pause/Resume, Reset
            ui.horizontal(|ui| {
                if stats.is_running {
                    if ui.button(crate::i18n::tr("⏸ Aseta tauolle")).clicked() {
                        self.live_stacker.set_running(false);
                    }
                } else {
                    if ui.button(crate::i18n::tr("▶ Jatka pinoa")).clicked() {
                        self.live_stacker.set_running(true);
                    }
                }

                if ui.button(crate::i18n::tr("🔄 Nollaa pino")).clicked() {
                    self.live_stacker.reset();
                    self.live_stack_counter += 1;
                }
            });

            ui.add_space(4.0);

            // Export buttons
            ui.horizontal(|ui| {
                if ui.button(crate::i18n::tr("💾 Tallenna FITS")).on_hover_text(crate::i18n::tr("Tallenna integroitu 32-bit lineaarinen FITS-kuva")).clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_file_name("LiveStack_Master.fits")
                        .add_filter("FITS", &["fits", "fit"])
                        .save_file()
                    {
                        match self.live_stacker.export_fits(&path) {
                            Ok(_) => self.status_message = format!("Master-pino tallennettu: {}", path.display()),
                            Err(e) => self.status_message = format!("Tallennusvirhe: {}", e),
                        }
                    }
                }

                if ui.button(crate::i18n::tr("🖼 Tallenna PNG")).on_hover_text(crate::i18n::tr("Tallenna venytetty kuva PNG-muodossa")).clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_file_name("LiveStack_View.png")
                        .add_filter("PNG", &["png"])
                        .save_file()
                    {
                        match self.live_stacker.export_png(&path, true) {
                            Ok(_) => self.status_message = format!("Kuva tallennettu: {}", path.display()),
                            Err(e) => self.status_message = format!("Tallennusvirhe: {}", e),
                        }
                    }
                }
            });

            ui.add_space(4.0);

            // Ingest folder button
            if ui.button(crate::i18n::tr("📂 Lue ja pinoa kansiosta")).on_hover_text(crate::i18n::tr("Lue ja pinoa kansion kaikki kuvatiedostot")).clicked() {
                if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                    let count = self.live_stacker.ingest_folder(&folder);
                    self.status_message = format!("Aloitettu {} kuvan käsittely kansiosta: {}", count, folder.display());
                }
            }
        });

        ui.add_space(8.0);

        // Statistics Group
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.strong(crate::i18n::tr("📊 Pinon tilastot"));
            ui.add_space(2.0);

            let total_mins = (stats.total_exposure_secs / 60.0).floor() as u32;
            let total_secs = (stats.total_exposure_secs % 60.0) as u32;

            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("⏱ Kokonaisvalotusaika:"));
                ui.label(RichText::new(format!("{} min {:02} s", total_mins, total_secs)).color(Color32::from_rgb(255, 220, 100)).strong());
            });

            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("📥 Pinottu / Vastaanotettu:"));
                ui.label(RichText::new(format!("{} / {} ruutua", stats.stacked_count, stats.total_received)).strong());
            });

            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("📈 Kohinasuhteen parannus (SNR):"));
                ui.label(RichText::new(format!("{:.1}× (+{:.0}%)", stats.snr_gain_factor, (stats.snr_gain_factor - 1.0) * 100.0)).color(Color32::from_rgb(100, 255, 140)).strong());
            });

            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("📐 Resoluutio:"));
                let (w, h) = stats.output_resolution;
                let res_text = if w > 0 && h > 0 {
                    if stats.drizzle_scale > 1 {
                        format!("{} × {} px ({}x Drizzle)", w, h, stats.drizzle_scale)
                    } else {
                        format!("{} × {} px (1x)", w, h)
                    }
                } else {
                    "-".to_string()
                };
                ui.label(RichText::new(res_text).color(Color32::from_rgb(130, 220, 255)).strong());
            });

            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("⭐ Tähdet (referenssi / uusin):"));
                ui.label(format!("{} / {}", stats.detected_stars_reference, stats.detected_stars_last_frame));
            });

            if stats.rejected_count > 0 {
                let rej_pct = (stats.rejected_count as f32 / stats.total_received.max(1) as f32) * 100.0;
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("❌ Hylätyt ruudut:"));
                    ui.colored_label(Color32::from_rgb(255, 120, 100), format!("{} ({:.1}%)", stats.rejected_count, rej_pct));
                });
                if !stats.last_rejection_reason.is_empty() {
                    ui.label(RichText::new(format!("{}: {}", crate::i18n::tr("Syy"), stats.last_rejection_reason)).small().color(Color32::from_rgb(255, 150, 130)));
                }
            }

            if !stats.last_frame_name.is_empty() {
                ui.separator();
                ui.label(RichText::new(format!("{}: {} ({})", crate::i18n::tr("Viimeisin ruutu"), stats.last_frame_name, stats.last_frame_time)).small().color(Color32::from_rgb(160, 180, 210)));
            }

            if !stats.status_message.is_empty() {
                ui.separator();
                ui.label(RichText::new(&stats.status_message).small().color(Color32::from_rgb(130, 220, 180)));
            }
        });

        ui.add_space(8.0);

        // Settings Group
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.strong(crate::i18n::tr("⚙ Live-pinoamisen asetukset"));
            ui.add_space(2.0);

            let mut cfg_changed = false;
            cfg_changed |= ui.checkbox(&mut cfg.align_frames, crate::i18n::tr("Tähtien automaattinen kohdistus")).on_hover_text(crate::i18n::tr("Kohdistaa osakuvat tähtiin sub-pikselin tarkkuudella")).changed();
            cfg_changed |= ui.checkbox(&mut cfg.auto_stretch, crate::i18n::tr("Automaattinen STF-venytys")).on_hover_text(crate::i18n::tr("Säätää kontrastin automaattisesti jokaisen ruudun jälkeen")).changed();

            ui.horizontal(|ui| {
                ui.label("Drizzle:");
                let old_drizzle = cfg.drizzle_scale;
                ui.radio_value(&mut cfg.drizzle_scale, 1, crate::i18n::tr("1x Normaali")).on_hover_text(crate::i18n::tr("Tavallinen sensorin natiiviresoluutio"));
                ui.radio_value(&mut cfg.drizzle_scale, 2, "2x Drizzle").on_hover_text(crate::i18n::tr("2x Super-Resolution sub-pikselin tarkkuudella (kaksinkertainen resoluutio)"));
                if cfg.drizzle_scale != old_drizzle {
                    cfg_changed = true;
                    self.live_stacker.reset();
                    self.live_stack_counter += 1;
                    self.status_message = format!("Drizzle-tila vaihdettu ({}x) – live-pino nollattu.", cfg.drizzle_scale);
                }
            });

            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Minimi tähtimäärä:"));
                cfg_changed |= ui.add(egui::DragValue::new(&mut cfg.min_stars).range(3..=30)).changed();
            });

            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Osavalotusaika:"));
                cfg_changed |= ui.add(egui::DragValue::new(&mut cfg.default_sub_exposure_secs).range(1.0..=300.0).suffix(" s")).changed();
            });

            if cfg_changed {
                self.live_stacker.update_config(|c| *c = cfg);
            }
        });
    }

    fn ui_live_stacking_canvas(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let live_img = self.live_stacker.get_current_image();
        let stats = self.live_stacker.get_stats();

        // Top summary bar inside canvas
        ui.horizontal(|ui| {
            ui.heading(RichText::new(crate::i18n::tr("⚡ Live-pino (Reaaliaikainen pinoaja)")).color(Color32::from_rgb(80, 200, 255)).strong());
            ui.separator();
            if stats.stacked_count > 0 {
                let total_mins = (stats.total_exposure_secs / 60.0).floor() as u32;
                let total_secs = (stats.total_exposure_secs % 60.0) as u32;
                ui.label(RichText::new(format!("⏱ {}m {:02}s ({} ruutua)", total_mins, total_secs, stats.stacked_count)).color(Color32::from_rgb(255, 230, 100)).strong());
                ui.separator();
                ui.label(RichText::new(format!("📈 SNR: {:.1}×", stats.snr_gain_factor)).color(Color32::from_rgb(100, 255, 140)).strong());
                ui.separator();
                ui.label(RichText::new(format!("⭐ {} tähteä", stats.detected_stars_last_frame)).color(Color32::from_rgb(200, 220, 255)));
                if stats.drizzle_scale > 1 {
                    ui.separator();
                    ui.label(RichText::new(format!("✨ {}x Drizzle", stats.drizzle_scale)).color(Color32::from_rgb(130, 240, 255)).strong());
                }
            } else {
                ui.label(RichText::new(crate::i18n::tr("Odottaa ensimmäistä valotusta...")).color(Color32::from_rgb(160, 170, 190)));
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(crate::i18n::tr("🔍 Nollaa näkymä")).clicked() {
                    self.live_image_viewer.reset_view();
                }
                if ui.button("1:1").clicked() {
                    self.live_image_viewer.set_1_to_1();
                }
            });
        });

        ui.separator();

        if let Some(img) = live_img {
            let stf = self.live_stacker.get_stf_params();
            self.live_image_viewer.update_texture(
                ctx,
                img.as_ref(),
                &stf,
                self.live_stack_counter + stats.stacked_count * 10,
                None,
            );
            self.hovered_pixel_info = self.live_image_viewer.show(
                ui,
                Some(img.as_ref()),
                &stf,
                self.live_stack_counter + stats.stacked_count * 10,
                None,
                None,
                None,
            ).0;
        } else {
            // Empty state placeholder
            let available_size = ui.available_size();
            let (rect, _) = ui.allocate_exact_size(available_size, egui::Sense::hover());
            ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(12, 14, 18));

            let center = rect.center();
            ui.painter().text(
                Pos2::new(center.x, center.y - 20.0),
                egui::Align2::CENTER_CENTER,
                crate::i18n::tr("⚡ Live-pinoaminen valmiina"),
                egui::FontId::proportional(22.0),
                Color32::from_rgb(100, 200, 255),
            );
            ui.painter().text(
                Pos2::new(center.x, center.y + 15.0),
                egui::Align2::CENTER_CENTER,
                crate::i18n::tr("Kuvat pinotaan ja päivittyvät ruudulle automaattisesti heti kun Seestar tallentaa niitä."),
                egui::FontId::proportional(14.0),
                Color32::from_rgb(160, 170, 190),
            );
            ui.painter().text(
                Pos2::new(center.x, center.y + 40.0),
                egui::Align2::CENTER_CENTER,
                crate::i18n::tr("Voit myös painaa vasemmalta 'Pinoa kansiosta' testataksesi olemassa olevilla tiedostoilla."),
                egui::FontId::proportional(13.0),
                Color32::from_rgb(120, 140, 170),
            );
        }
    }

    fn ui_channel_combination_tab(&mut self, ui: &mut egui::Ui) {
        ui.set_width(ui.available_width());
        ui.heading(crate::i18n::tr("🌈 Kanavayhdistys (LRGB & SHO / HOO / HSO)"));
        ui.label(
            RichText::new(crate::i18n::tr("Yhdistä monokromisuodattimet (R, G, B, L, Ha, OIII, SII) täyden värispektrin värikuvaksi."))
                .small()
                .color(Color32::from_rgb(160, 180, 210)),
        );

        ui.add_space(8.0);

        // One-click preset palette buttons
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.strong(crate::i18n::tr("🎨 Valmiit väripaletit:"));
            ui.horizontal_wrapped(|ui| {
                if ui.button(RichText::new("🔭 LRGB / RGBL").strong()).on_hover_text(crate::i18n::tr("Täysi LRGB: R, G, B ja luminanssi L")).clicked() {
                    self.apply_palette_preset("LRGB");
                }
                if ui.button(RichText::new("🌌 SHO (Hubble)").color(Color32::from_rgb(255, 200, 100)).strong())
                    .on_hover_text(crate::i18n::tr("Hubble-paletti: R = SII, G = Ha, B = OIII"))
                    .clicked()
                {
                    self.apply_palette_preset("SHO");
                }
                if ui.button(RichText::new("🔴 HOO (Bicolor)").color(Color32::from_rgb(100, 230, 255)).strong())
                    .on_hover_text(crate::i18n::tr("Kaksoiskaistapaletti: R = Ha, G = OIII, B = OIII"))
                    .clicked()
                {
                    self.apply_palette_preset("HOO");
                }
                if ui.button(RichText::new("⚡ HSO").strong())
                    .on_hover_text(crate::i18n::tr("HSO-paletti: R = Ha, G = SII, B = OIII"))
                    .clicked()
                {
                    self.apply_palette_preset("HSO");
                }
            });
        });

        ui.add_space(8.0);

        // Channel Selectors
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.strong(crate::i18n::tr("📥 Valitse kanavat:"));

            // Red Channel
            ui.horizontal(|ui| {
                ui.label(RichText::new("🔴 R:").color(Color32::from_rgb(255, 100, 100)).strong());
                Self::render_image_combo(&self.images, ui, "combo_r", &mut self.channel_r_index);
            });

            // Green Channel
            ui.horizontal(|ui| {
                ui.label(RichText::new("🟢 G:").color(Color32::from_rgb(100, 255, 120)).strong());
                Self::render_image_combo(&self.images, ui, "combo_g", &mut self.channel_g_index);
            });

            // Blue Channel
            ui.horizontal(|ui| {
                ui.label(RichText::new("🔵 B:").color(Color32::from_rgb(100, 180, 255)).strong());
                Self::render_image_combo(&self.images, ui, "combo_b", &mut self.channel_b_index);
            });

            // Luminance Channel (Optional)
            ui.horizontal(|ui| {
                ui.label(RichText::new("⚪ L:").color(Color32::from_rgb(230, 240, 255)).strong());
                Self::render_image_combo(&self.images, ui, "combo_l", &mut self.channel_l_index);
                if self.channel_l_index.is_some() {
                    if ui.small_button("❌").on_hover_text(crate::i18n::tr("Poista luminanssi käytöstä")).clicked() {
                        self.channel_l_index = None;
                    }
                }
            });
        });

        ui.add_space(8.0);

        // Channel Weights & Settings
        ui.group(|ui| {
            let gw = ui.available_width();
            ui.set_width(gw);
            ui.set_max_width(gw);
            ui.spacing_mut().slider_width = (gw - 76.0).max(100.0);
            ui.strong(crate::i18n::tr("⚖️ Kanavapainotukset ja asetukset:"));

            ui.label(RichText::new(crate::i18n::tr("Punaisen paino (R):")).small());
            ui.add(egui::Slider::new(&mut self.channel_combiner_config.r_weight, 0.0..=2.0).trailing_fill(true));
            ui.label(RichText::new(crate::i18n::tr("Vihreän paino (G):")).small());
            ui.add(egui::Slider::new(&mut self.channel_combiner_config.g_weight, 0.0..=2.0).trailing_fill(true));
            ui.label(RichText::new(crate::i18n::tr("Sinisen paino (B):")).small());
            ui.add(egui::Slider::new(&mut self.channel_combiner_config.b_weight, 0.0..=2.0).trailing_fill(true));

            if self.channel_l_index.is_some() {
                ui.separator();
                ui.label(RichText::new(crate::i18n::tr("Luminanssisäädöt:")).strong().color(Color32::from_rgb(200, 220, 255)));
                ui.label(RichText::new(crate::i18n::tr("Luminanssipaino (L):")).small());
                ui.add(egui::Slider::new(&mut self.channel_combiner_config.l_weight, 0.0..=1.0).trailing_fill(true));
                ui.label(RichText::new(crate::i18n::tr("Värikylläisyyden korostus:")).small());
                ui.add(egui::Slider::new(&mut self.channel_combiner_config.saturation_boost, 0.5..=3.0).trailing_fill(true));
            }

            ui.separator();
            ui.checkbox(&mut self.channel_combiner_config.align_stars, crate::i18n::tr("✨ Kohdista tähdet automaattisesti"));
            ui.label(RichText::new(crate::i18n::tr("Kohdistaa kanavat automaattisesti punaisen kanavan koordinaatistoon.")).small().color(Color32::from_rgb(140, 170, 200)));

            ui.checkbox(&mut self.channel_combiner_config.linear_fit, crate::i18n::tr("🌐 Tasaa kanavien taustatasot"));
            ui.label(RichText::new(crate::i18n::tr("Tasaa kanavien taustat ja sirontatasot ennen yhdistämistä värivääristymien välttämiseksi.")).small().color(Color32::from_rgb(140, 170, 200)));
        });

        ui.add_space(14.0);

        let can_combine = self.channel_r_index.is_some() && self.channel_g_index.is_some() && self.channel_b_index.is_some();
        let btn_text = if self.channel_l_index.is_some() {
            crate::i18n::tr("🚀 Yhdistä LRGB-värikuvaksi")
        } else {
            crate::i18n::tr("🚀 Yhdistä RGB-värikuvaksi")
        };

        let btn_w = ui.available_width();
        if ui
            .add_enabled(
                can_combine,
                egui::Button::new(RichText::new(btn_text).color(Color32::from_rgb(50, 255, 150)).strong().size(15.0))
                    .min_size(Vec2::new(btn_w, 32.0)),
            )
            .clicked()
        {
            self.run_channel_combination();
        }

        if !can_combine {
            ui.label(RichText::new(crate::i18n::tr("⚠️ Valitse vähintään R-, G- ja B-kanavat yltä.")).small().color(Color32::from_rgb(255, 180, 80)));
        }
    }

    fn ui_syqon_section(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            let gw = ui.available_width();
            ui.set_width(gw);
            ui.set_max_width(gw);
            ui.spacing_mut().slider_width = (gw - 86.0).max(100.0);
            ui.horizontal(|ui| {
                ui.heading(RichText::new(crate::i18n::tr("🌌 SyQon-tekoälytyökalut")).color(Color32::from_rgb(140, 220, 255)).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.syqon_cli_available {
                        let ver_str = self.syqon_cli_version.as_deref().unwrap_or("v1.0");
                        ui.label(RichText::new(format!("● {}", ver_str)).color(Color32::from_rgb(80, 255, 120)).small().strong())
                            .on_hover_text(crate::i18n::tr("SyQon CLI havaittu järjestelmästä."));
                    } else if self.syqon_simulate {
                        ui.label(RichText::new(crate::i18n::tr("● Simulaatio")).color(Color32::from_rgb(255, 200, 80)).small().strong());
                    } else {
                        ui.label(RichText::new(crate::i18n::tr("○ Ei asennettu")).color(Color32::from_rgb(255, 160, 100)).small())
                            .on_hover_text(crate::i18n::tr("SyQon Studioa ei löytynyt vakiopoluista."));
                    }
                });
            });

            if !self.syqon_cli_available && !self.syqon_simulate {
                ui.add_space(2.0);
                ui.label(RichText::new(crate::i18n::tr("SyQon tarjoaa ilmaiset paikalliset tekoälymallit tähtien poistoon, optiikan korjaukseen ja kohinanpoistoon.")).small().color(Color32::from_rgb(180, 190, 210)));
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.button(RichText::new(crate::i18n::tr("🌐 Lataa ilmainen SyQon Studio")).color(Color32::from_rgb(100, 220, 255)).strong()).clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab("https://syqon.eu"));
                    }
                    if ui.button(crate::i18n::tr("🔄 Tarkista asennus")).clicked() {
                        self.syqon_cli_available = SyQonEngine::is_available();
                        self.syqon_cli_version = SyQonEngine::get_version();
                    }
                });
                ui.add_space(2.0);
                ui.checkbox(&mut self.syqon_simulate, crate::i18n::tr("🧪 Testaa simulaatiotilassa (ilman ulkoista binääriä)"));
                ui.add_space(4.0);
            }

            // Interactive Studio Editor button (-gui)
            let has_img = self.active_image().is_some();
            let is_enabled = has_img && (self.syqon_cli_available || self.syqon_simulate);
            let btn_w = ui.available_width();
            let gui_btn = ui.add_enabled(
                is_enabled,
                egui::Button::new(RichText::new(crate::i18n::tr("⇱ Avaa SyQon Studio -editori")).color(Color32::from_rgb(120, 230, 200)).strong())
                    .min_size(Vec2::new(btn_w, 26.0)),
            );
            if gui_btn.on_hover_text(crate::i18n::tr("Avaa aktiivisen kuvan SyQonin kelluvassa kevyessä GUI-editorissa (ROI, Autostretch, Done). Tallennettu tulos päivittyy suoraan kuvalistalle.")).clicked() {
                self.start_syqon_task(SyQonTask::InteractiveGui);
            }

            ui.add_space(4.0);

            // 1. Axiom Star Separation
            ui.collapsing(crate::i18n::tr("✨ Axiom: Tähtien poisto ja erottelu"), |ui| {
                let cw = ui.available_width();
                ui.set_max_width(cw);
                ui.horizontal_wrapped(|ui| {
                    ui.label(crate::i18n::tr("Malli:"));
                    ui.radio_value(&mut self.syqon_axiom_model, SyQonModel::AxiomMini, crate::i18n::tr("Axiom Mini (Ilmainen)"));
                    ui.radio_value(&mut self.syqon_axiom_model, SyQonModel::AxiomV3, crate::i18n::tr("Axiom V3 (Täysi)"));
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label(crate::i18n::tr("Venytyksen tila:"));
                    ui.selectable_value(&mut self.syqon_axiom_stretch, "auto".to_string(), "Auto");
                    ui.selectable_value(&mut self.syqon_axiom_stretch, "linear".to_string(), crate::i18n::tr("Lineaarinen"));
                    ui.selectable_value(&mut self.syqon_axiom_stretch, "none".to_string(), crate::i18n::tr("Ei mitään"));
                });
                ui.checkbox(&mut self.syqon_axiom_stars_only, crate::i18n::tr("🌟 Erottele tähdet omaksi kuvakseen"));
                ui.add_space(4.0);
                let bw = ui.available_width();
                let btn = ui.add_enabled(
                    is_enabled,
                    egui::Button::new(RichText::new(crate::i18n::tr("🚀 Poista tähdet (Axiom)")).color(Color32::from_rgb(100, 255, 160)).strong())
                        .min_size(Vec2::new(bw, 24.0)),
                );
                if btn.clicked() {
                    let task = SyQonTask::StarSeparation {
                        model: self.syqon_axiom_model,
                        stretch: self.syqon_axiom_stretch.clone(),
                        generate_stars_only: self.syqon_axiom_stars_only,
                    };
                    self.start_syqon_task(task);
                }
            });

            // 2. Parallax Stellar Correction, Reduction & Deblur
            ui.collapsing(crate::i18n::tr("🔭 Parallax: Tähtien korjaus ja terävöitys"), |ui| {
                let cw = ui.available_width();
                ui.set_max_width(cw);
                ui.spacing_mut().slider_width = (cw - 76.0).max(90.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(crate::i18n::tr("Malli:"));
                    ui.radio_value(&mut self.syqon_parallax_model, SyQonModel::ParallaxNano, crate::i18n::tr("Nano (Ilmainen)"));
                    ui.radio_value(&mut self.syqon_parallax_model, SyQonModel::ParallaxAesthetics, "Aesthetics");
                    ui.radio_value(&mut self.syqon_parallax_model, SyQonModel::ParallaxClassic, "Classic");
                });
                ui.checkbox(&mut self.syqon_parallax_correction, crate::i18n::tr("🛡 Korjaa optiikan tähtivääristymät"));
                ui.checkbox(&mut self.syqon_parallax_reduction, crate::i18n::tr("✨ Pienennä tähtiä"));
                if self.syqon_parallax_reduction {
                    ui.label(RichText::new(crate::i18n::tr("Pienennyksen taso (0–10):")).small());
                    ui.add(egui::Slider::new(&mut self.syqon_parallax_reduction_level, 0..=10).trailing_fill(true));
                }
                ui.checkbox(&mut self.syqon_parallax_deblur, crate::i18n::tr("🔍 Terävöitä kohteita (Deblur)"));
                if self.syqon_parallax_deblur {
                    ui.label(RichText::new(crate::i18n::tr("Terävöityksen voimakkuus:")).small());
                    ui.add(egui::Slider::new(&mut self.syqon_parallax_deblur_strength, 0.0..=1.0).step_by(0.05).trailing_fill(true));
                }
                ui.add_space(4.0);
                let bw = ui.available_width();
                let btn = ui.add_enabled(
                    is_enabled,
                    egui::Button::new(RichText::new(crate::i18n::tr("🚀 Suorita Parallax-parannus")).color(Color32::from_rgb(100, 255, 160)).strong())
                        .min_size(Vec2::new(bw, 24.0)),
                );
                if btn.clicked() {
                    let task = SyQonTask::Parallax {
                        model: self.syqon_parallax_model,
                        correction: self.syqon_parallax_correction,
                        reduction: self.syqon_parallax_reduction,
                        reduction_level: self.syqon_parallax_reduction_level,
                        deblur: self.syqon_parallax_deblur,
                        deblur_strength: self.syqon_parallax_deblur_strength,
                    };
                    self.start_syqon_task(task);
                }
            });

            // 3. Prism Denoise
            ui.collapsing(crate::i18n::tr("🌊 Prism: Syvä tekoälykohinanpoisto"), |ui| {
                let cw = ui.available_width();
                ui.set_max_width(cw);
                ui.spacing_mut().slider_width = (cw - 76.0).max(90.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(crate::i18n::tr("Malli:"));
                    ui.radio_value(&mut self.syqon_prism_model, SyQonModel::PrismEssential, crate::i18n::tr("Prism Essential (Mukana)"));
                    ui.radio_value(&mut self.syqon_prism_model, SyQonModel::PrismMax, "Prism Max");
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label(crate::i18n::tr("Dynaaminen alue:"));
                    ui.selectable_value(&mut self.syqon_prism_domain, "auto".to_string(), "Auto");
                    ui.selectable_value(&mut self.syqon_prism_domain, "linear".to_string(), crate::i18n::tr("Lineaarinen"));
                    ui.selectable_value(&mut self.syqon_prism_domain, "nonlinear".to_string(), crate::i18n::tr("Epälineaarinen"));
                });
                ui.label(RichText::new(crate::i18n::tr("Voimakkuus:")).small());
                ui.add(egui::Slider::new(&mut self.syqon_prism_application, 0.1..=1.0).step_by(0.05).trailing_fill(true));
                ui.add_space(4.0);
                let bw = ui.available_width();
                let btn = ui.add_enabled(
                    is_enabled,
                    egui::Button::new(RichText::new(crate::i18n::tr("🚀 Suorita Prism-kohinanpoisto")).color(Color32::from_rgb(100, 255, 160)).strong())
                        .min_size(Vec2::new(bw, 24.0)),
                );
                if btn.clicked() {
                    let task = SyQonTask::Denoise {
                        model: self.syqon_prism_model,
                        domain: self.syqon_prism_domain.clone(),
                        application: self.syqon_prism_application,
                    };
                    self.start_syqon_task(task);
                }
            });

            // 4. Deep Gradient
            ui.collapsing(crate::i18n::tr("🌌 Deep Gradient: Tekoälytaustatasaus"), |ui| {
                let cw = ui.available_width();
                ui.set_max_width(cw);
                ui.label(crate::i18n::tr("Poistaa valosaasteen ja vinjetoinnin tekoälymallilla taustasta."));
                ui.checkbox(&mut self.syqon_gradient_save, crate::i18n::tr("Tallenna gradientti erillisenä kuvana"));
                ui.add_space(4.0);
                let bw = ui.available_width();
                let btn = ui.add_enabled(
                    is_enabled,
                    egui::Button::new(RichText::new(crate::i18n::tr("🚀 Poista taustagradientti tekoälyllä")).color(Color32::from_rgb(100, 255, 160)).strong())
                        .min_size(Vec2::new(bw, 24.0)),
                );
                if btn.clicked() {
                    let task = SyQonTask::Gradient {
                        save_gradient: self.syqon_gradient_save,
                    };
                    self.start_syqon_task(task);
                }
            });
        });
    }

    fn ui_processing_tab(&mut self, ui: &mut egui::Ui) {
        let tab_w = ui.available_width();
        ui.set_width(tab_w);
        ui.set_max_width(tab_w);
        ui.spacing_mut().slider_width = (tab_w - 88.0).max(100.0);
        ui.heading(crate::i18n::tr("Jälkikäsittelytyökalut"));

        // 0. SyQon Neural AI Suite
        self.ui_syqon_section(ui);
        ui.add_space(8.0);

        // 1. Crop (Manuaalinen ja automaattinen rajaus)
        ui.group(|ui| {
            let gw = ui.available_width();
            ui.set_width(gw);
            ui.set_max_width(gw);
            ui.horizontal(|ui| {
                ui.strong(crate::i18n::tr("✂️ Kuvan rajaus"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let toggle_txt = if self.manual_crop.is_active { crate::i18n::tr("Rajaus PÄÄLLÄ") } else { crate::i18n::tr("Kytke rajaustila") };
                    if ui.selectable_label(self.manual_crop.is_active, toggle_txt).clicked() {
                        self.toggle_manual_crop();
                    }
                });
            });

            if let Some(img) = self.active_image() {
                let img_w = img.width;
                let img_h = img.height;
                let (mut px_x, mut px_y, mut px_w, mut px_h) = self.manual_crop.pixel_rect(img_w, img_h);

                ui.label(RichText::new(format!("{}: {} × {} px", crate::i18n::tr("Kuvan koko"), img_w, img_h)).small());

                if self.manual_crop.is_active {
                    ui.colored_label(Color32::from_rgb(100, 255, 150), crate::i18n::tr("💡 Raahaa hiirellä rajausaluetta kuvassa tai säädä mitat alta:"));

                    ui.horizontal_wrapped(|ui| {
                        ui.label("X:");
                        if ui.add(egui::DragValue::new(&mut px_x).range(0..=img_w.saturating_sub(4))).changed() {
                            self.manual_crop.set_pixel_rect(px_x, px_y, px_w, px_h, img_w, img_h);
                        }
                        ui.label("Y:");
                        if ui.add(egui::DragValue::new(&mut px_y).range(0..=img_h.saturating_sub(4))).changed() {
                            self.manual_crop.set_pixel_rect(px_x, px_y, px_w, px_h, img_w, img_h);
                        }
                    });

                    ui.horizontal_wrapped(|ui| {
                        ui.label(crate::i18n::tr("Leveys:"));
                        if ui.add(egui::DragValue::new(&mut px_w).range(4..=img_w.saturating_sub(px_x))).changed() {
                            self.manual_crop.set_pixel_rect(px_x, px_y, px_w, px_h, img_w, img_h);
                        }
                        ui.label(crate::i18n::tr("Korkeus:"));
                        if ui.add(egui::DragValue::new(&mut px_h).range(4..=img_h.saturating_sub(px_y))).changed() {
                            self.manual_crop.set_pixel_rect(px_x, px_y, px_w, px_h, img_w, img_h);
                        }
                    });

                    ui.horizontal_wrapped(|ui| {
                        ui.label(crate::i18n::tr("Kuvasuhde:"));
                        for ar in [CropAspectRatio::Free, CropAspectRatio::Square1x1, CropAspectRatio::Wide16x9, CropAspectRatio::Standard4x3, CropAspectRatio::Photo3x2] {
                            if ui.selectable_label(self.manual_crop.aspect_ratio == ar, ar.label()).clicked() {
                                self.manual_crop.aspect_ratio = ar;
                                if let Some(target_ar) = ar.ratio() {
                                    let norm_ar = target_ar * (img_h as f32 / img_w as f32);
                                    self.manual_crop.w = (self.manual_crop.h * norm_ar).clamp(0.02, 1.0 - self.manual_crop.x);
                                }
                            }
                        }
                    });

                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        if ui.button(RichText::new(crate::i18n::tr("✂️ Käytä rajaus (Enter)")).color(Color32::from_rgb(100, 255, 150)).strong()).clicked() {
                            self.apply_manual_crop();
                        }
                        if ui.button(RichText::new(crate::i18n::tr("❌ Peruuta (Esc)")).color(Color32::from_rgb(255, 120, 120))).clicked() {
                            self.cancel_manual_crop();
                        }
                        if ui.button(crate::i18n::tr("↺ Koko kuva")).clicked() {
                            self.manual_crop.reset_to_full();
                        }
                    });
                } else {
                    let bw = ui.available_width();
                    if ui.add_sized([bw, 24.0], egui::Button::new(RichText::new(crate::i18n::tr("✂️ Avaa manuaalinen rajaus (interaktiivinen)")).strong())).clicked() {
                        self.toggle_manual_crop();
                    }
                }

                ui.separator();
                let bw = ui.available_width();
                if ui.add_sized([bw, 24.0], egui::Button::new(crate::i18n::tr("✂️ Rajaa mustat reunat automaattisesti (Auto-Crop)"))).clicked() {
                    self.run_auto_crop();
                }
            } else {
                ui.label(crate::i18n::tr("Avaa kuva käyttääksesi rajausta."));
            }
        });

        ui.add_space(8.0);

        // 2. Rotation & Flip (Kuvan pyöritys ja peilaus)
        ui.group(|ui| {
            let gw = ui.available_width();
            ui.set_width(gw);
            ui.set_max_width(gw);
            ui.spacing_mut().slider_width = (gw - 76.0).max(100.0);
            ui.strong(crate::i18n::tr("🔄 Kuvan pyöritys ja peilaus"));
            ui.label(crate::i18n::tr("Käännä tai peilaa kuva optiikan tai sommittelun mukaisesti."));

            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                if ui.button("↷ 90° CW").on_hover_text(crate::i18n::tr("Pyöritä 90° myötäpäivään")).clicked() {
                    self.rotate_active_90_cw();
                }
                if ui.button("↶ 90° CCW").on_hover_text(crate::i18n::tr("Pyöritä 90° vastapäivään")).clicked() {
                    self.rotate_active_90_ccw();
                }
                if ui.button("↻ 180°").on_hover_text(crate::i18n::tr("Pyöritä 180°")).clicked() {
                    self.rotate_active_180();
                }
                if ui.button(crate::i18n::tr("⇄ Peilaa V")).on_hover_text(crate::i18n::tr("Peilaa vaakasuunnassa")).clicked() {
                    self.flip_active_horizontal();
                }
                if ui.button(crate::i18n::tr("⇅ Peilaa P")).on_hover_text(crate::i18n::tr("Peilaa pystysuunnassa")).clicked() {
                    self.flip_active_vertical();
                }
            });

            ui.add_space(6.0);
            ui.label(RichText::new(crate::i18n::tr("Vapaa kiertokulma:")).small());
            ui.add(egui::Slider::new(&mut self.rotation_angle, -180.0..=180.0).suffix("°").step_by(0.1));
            ui.horizontal_wrapped(|ui| {
                if ui.small_button("-90°").clicked() { self.rotation_angle = -90.0; }
                if ui.small_button("-45°").clicked() { self.rotation_angle = -45.0; }
                if ui.small_button("-1.0°").clicked() { self.rotation_angle -= 1.0; }
                if ui.small_button("-0.1°").clicked() { self.rotation_angle -= 0.1; }
                if ui.small_button("0.0°").clicked() { self.rotation_angle = 0.0; }
                if ui.small_button("+0.1°").clicked() { self.rotation_angle += 0.1; }
                if ui.small_button("+1.0°").clicked() { self.rotation_angle += 1.0; }
                if ui.small_button("+45°").clicked() { self.rotation_angle = 45.0; }
                if ui.small_button("+90°").clicked() { self.rotation_angle = 90.0; }
            });
            ui.checkbox(&mut self.rotation_auto_crop, crate::i18n::tr("✂️ Rajaa mustat reunat automaattisesti"));
            ui.add_space(4.0);
            let bw = ui.available_width();
            if ui.add_sized([bw, 24.0], egui::Button::new(RichText::new(crate::i18n::tr("🔄 Sovella vapaa pyöritys")).color(Color32::from_rgb(100, 255, 150)).strong())).clicked() {
                let angle = self.rotation_angle;
                let auto_crop = self.rotation_auto_crop;
                self.rotate_active_arbitrary(angle, auto_crop);
            }
        });

        ui.add_space(8.0);

        // Curves Transformation
        ui.group(|ui| {
            let gw = ui.available_width();
            ui.set_width(gw);
            ui.set_max_width(gw);
            ui.horizontal(|ui| {
                ui.strong(crate::i18n::tr("📈 Käyrätyökalu"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(crate::i18n::tr("⇱ Avaa ikkunaan")).on_hover_text(crate::i18n::tr("Avaa käyrätyökalu kelluvana ikkunana kuvan päälle")).clicked() {
                        self.show_curves_window = true;
                    }
                });
            });
            ui.label(crate::i18n::tr("Monikanavainen PCHIP-käyrätyökalu (RGB, R, G, B, S)."));
            self.ui_curves_editor(ui);
        });

        ui.add_space(8.0);

        // Debayer
        ui.group(|ui| {
            let gw = ui.available_width();
            ui.set_width(gw);
            ui.set_max_width(gw);
            ui.strong(crate::i18n::tr("🟣 Värimatriisin purku (Debayer)"));
            ui.label(crate::i18n::tr("Purkaa värikennon (Seestar, OSC, DSLR) CFA-matriisin värikuvaksi."));

            if let Some(img) = self.active_image() {
                if img.channels == 3 {
                    let info = if let Some(b) = &img.metadata.bayer_pattern {
                        format!("Aktiivinen kuva on RGB-värikuva (Bayer: {}).", b)
                    } else {
                        crate::i18n::tr("Aktiivinen kuva on 3-kanavainen RGB.").to_string()
                    };
                    ui.colored_label(Color32::from_rgb(100, 255, 150), info);
                } else {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(crate::i18n::tr("Kuvio:"));
                        ui.radio_value(&mut self.selected_bayer_pattern, BayerPattern::Grbg, "GRBG (Seestar)");
                        ui.radio_value(&mut self.selected_bayer_pattern, BayerPattern::Rggb, "RGGB (ZWO)");
                        ui.radio_value(&mut self.selected_bayer_pattern, BayerPattern::Bggr, "BGGR");
                        ui.radio_value(&mut self.selected_bayer_pattern, BayerPattern::Gbrg, "GBRG");
                    });

                    let bw = ui.available_width();
                    if ui.add_sized([bw, 24.0], egui::Button::new(crate::i18n::tr("🚀 Pura Bayer-värit (Debayer)"))).clicked() {
                        self.run_manual_debayer();
                    }
                }
            } else {
                ui.label(crate::i18n::tr("Ei valittua kuvaa."));
            }
        });

        ui.add_space(8.0);

        // ABE
        ui.group(|ui| {
            let gw = ui.available_width();
            ui.set_width(gw);
            ui.set_max_width(gw);
            ui.spacing_mut().slider_width = (gw - 76.0).max(100.0);
            ui.strong(crate::i18n::tr("🌐 Automaattinen taustan tasoitus (ABE)"));
            ui.label(crate::i18n::tr("Poistaa valosaasteen, neutraloi taustataivaan ja tasoittaa vinjetoinnin."));

            ui.horizontal_wrapped(|ui| {
                ui.label(crate::i18n::tr("Polynomi:"));
                ui.radio_value(&mut self.abe_degree, 1, "1 (Lin)");
                ui.radio_value(&mut self.abe_degree, 2, crate::i18n::tr("2 (Kaari)"));
                ui.radio_value(&mut self.abe_degree, 3, crate::i18n::tr("3 (Syvä)"));
                ui.radio_value(&mut self.abe_degree, 4, crate::i18n::tr("4 (Korkea)"));
            });

            ui.horizontal_wrapped(|ui| {
                ui.label(crate::i18n::tr("Toimintatila:"));
                ui.radio_value(&mut self.abe_mode, ExtractionMode::Subtraction, crate::i18n::tr("Vähennys (Valosaaste)"));
                ui.radio_value(&mut self.abe_mode, ExtractionMode::Division, crate::i18n::tr("Jako (Vinjetointi)"));
            });

            ui.label(RichText::new(crate::i18n::tr("Toleranssi:")).small());
            ui.add(egui::Slider::new(&mut self.abe_deviation, 1.5..=6.0).step_by(0.1).trailing_fill(true));

            let bw = ui.available_width();
            if ui.add_sized([bw, 24.0], egui::Button::new(crate::i18n::tr("Suorita ABE-gradientinpoisto"))).clicked() {
                self.run_abe();
            }
        });

        ui.add_space(8.0);

        // SCNR
        ui.group(|ui| {
            let gw = ui.available_width();
            ui.set_width(gw);
            ui.set_max_width(gw);
            ui.spacing_mut().slider_width = (gw - 76.0).max(100.0);
            ui.strong(crate::i18n::tr("🟢 Vihreän kohinan poisto (SCNR)"));
            ui.label(crate::i18n::tr("Poistaa avaruuskuvista luonnottoman vihertävän sävyn."));

            ui.label(RichText::new(crate::i18n::tr("Voimakkuus:")).small());
            ui.add(egui::Slider::new(&mut self.scnr_amount, 0.0..=1.0).trailing_fill(true));

            let bw = ui.available_width();
            if ui.add_sized([bw, 24.0], egui::Button::new(crate::i18n::tr("Poista viherkohina"))).clicked() {
                self.run_scnr();
            }
        });

        ui.add_space(8.0);

        // Dual-Band Boost
        ui.group(|ui| {
            let gw = ui.available_width();
            ui.set_width(gw);
            ui.set_max_width(gw);
            ui.spacing_mut().slider_width = (gw - 76.0).max(100.0);
            ui.strong(crate::i18n::tr("⚡ Kapeakaistakorostus (Ha / [OIII])"));
            ui.label(crate::i18n::tr("Erityisesti Seestar S30 ja OSC-kaksoiskaistasuotimille (Ha punainen siipisumu, OIII syaani ydin)."));

            ui.label(RichText::new(crate::i18n::tr("H-alfa (Punainen sumu):")).small());
            ui.add(egui::Slider::new(&mut self.ha_boost, 0.8..=2.5).trailing_fill(true));
            ui.label(RichText::new(crate::i18n::tr("[OIII] (Syaaninsininen ydin):")).small());
            ui.add(egui::Slider::new(&mut self.oiii_boost, 0.8..=2.5).trailing_fill(true));

            let bw = ui.available_width();
            if ui.add_sized([bw, 24.0], egui::Button::new(crate::i18n::tr("🚀 Korosta kapeakaistavärejä"))).clicked() {
                self.run_dual_band_boost();
            }
        });

        ui.add_space(8.0);

        // Reflection Nebula Boost (Pleiades / Blue Dust)
        ui.group(|ui| {
            let gw = ui.available_width();
            ui.set_width(gw);
            ui.set_max_width(gw);
            ui.spacing_mut().slider_width = (gw - 76.0).max(100.0);
            ui.strong(crate::i18n::tr("🌌 Heijastussumun korostus (Plejadit / Sininen)"));
            ui.label(crate::i18n::tr("Korostaa tähtien heijastussumujen sinisiä ja syaaneja pölypilviä (M45 Plejadit, Iris, Witch Head)."));

            ui.label(RichText::new(crate::i18n::tr("Sininen heijastus (B):")).small());
            ui.add(egui::Slider::new(&mut self.reflection_blue_boost, 1.0..=4.0).trailing_fill(true));
            ui.label(RichText::new(crate::i18n::tr("Syaanipöly (G):")).small());
            ui.add(egui::Slider::new(&mut self.reflection_cyan_boost, 1.0..=2.5).trailing_fill(true));
            ui.label(RichText::new(crate::i18n::tr("Värikylläisyys:")).small());
            ui.add(egui::Slider::new(&mut self.reflection_saturation, 1.0..=3.0).trailing_fill(true));

            let bw = ui.available_width();
            if ui.add_sized([bw, 24.0], egui::Button::new(crate::i18n::tr("🚀 Korosta heijastussumua"))).clicked() {
                self.run_reflection_boost();
            }
        });

        ui.add_space(8.0);

        // Arcsinh Stretch
        ui.group(|ui| {
            let gw = ui.available_width();
            ui.set_width(gw);
            ui.set_max_width(gw);
            ui.spacing_mut().slider_width = (gw - 76.0).max(100.0);
            ui.strong(crate::i18n::tr("📈 Väriä säilyttävä venytys (Arcsinh)"));
            ui.label(crate::i18n::tr("Nostaa himmeät sumut esiin säilyttäen täyden värikylläisyyden."));

            ui.label(RichText::new(crate::i18n::tr("Venytyskerroin:")).small());
            ui.add(egui::Slider::new(&mut self.arcsinh_stretch_factor, 5.0..=100.0).logarithmic(true).trailing_fill(true));
            ui.label(RichText::new(crate::i18n::tr("Musta piste:")).small());
            ui.add(egui::Slider::new(&mut self.arcsinh_black_point, 0.0..=0.20).step_by(0.005).trailing_fill(true));

            let bw = ui.available_width();
            if ui.add_sized([bw, 24.0], egui::Button::new(crate::i18n::tr("🚀 Suorita Arcsinh-venytys"))).clicked() {
                self.run_arcsinh_stretch();
            }
        });

        ui.add_space(8.0);

        // Plate Solve & WCS Celestial Annotation Overlay
        ui.group(|ui| {
            let gw = ui.available_width();
            ui.set_width(gw);
            ui.set_max_width(gw);
            ui.spacing_mut().slider_width = (gw - 76.0).max(100.0);
            ui.horizontal(|ui| {
                ui.strong(crate::i18n::tr("🧭 Tähtikentän tunnistus & Kohteet"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let toggle_lbl = if self.annotation_config.enabled {
                        RichText::new(crate::i18n::tr("● PÄÄLLÄ")).color(Color32::from_rgb(80, 255, 140)).strong()
                    } else {
                        RichText::new(crate::i18n::tr("○ POIS")).color(Color32::from_rgb(160, 175, 195))
                    };
                    if ui.selectable_label(self.annotation_config.enabled, toggle_lbl).clicked() {
                        self.toggle_wcs_annotations();
                    }
                });
            });
            ui.label(crate::i18n::tr("Ratkaisee kuvan tähtikentän (WCS) ja piirtää kuvan päälle RA/Dec-koordinaattiruudukon, DSO-kohteet ja tunnistetut tähdet."));

            ui.add_space(4.0);
            let combo_w = (gw - 12.0).max(150.0);
            let selected_target_label = match self.wcs_manual_target_idx {
                Some(idx) => self
                    .sky_catalog
                    .get(idx)
                    .map(|o| format!("{} – {}", o.id, o.name))
                    .unwrap_or_else(|| crate::i18n::tr("Automaattinen (FITS / Tiedostonimi)").to_string()),
                None => crate::i18n::tr("Automaattinen (FITS / Tiedostonimi)").to_string(),
            };
            ui.label(RichText::new(crate::i18n::tr("Kentän kohde / keskipiste:")).small());
            let mut target_changed = false;
            egui::ComboBox::from_id_salt("wcs_target_override")
                .selected_text(selected_target_label)
                .width(combo_w)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_value(
                            &mut self.wcs_manual_target_idx,
                            None,
                            crate::i18n::tr("🔍 Automaattinen (FITS-otsikko / Tiedostonimi)"),
                        )
                        .changed()
                    {
                        target_changed = true;
                    }
                    for (idx, obj) in self.sky_catalog.iter().enumerate() {
                        let lbl = format!("{} {} – {}", obj.obj_type.icon(), obj.id, obj.name);
                        if ui
                            .selectable_value(&mut self.wcs_manual_target_idx, Some(idx), lbl)
                            .changed()
                        {
                            target_changed = true;
                        }
                    }
                });
            if target_changed && !self.images.is_empty() {
                self.annotation_config.enabled = true;
                self.solve_active_image_wcs();
            }

            ui.add_space(2.0);
            ui.checkbox(&mut self.annotation_config.show_grid, crate::i18n::tr("🌐 RA / Dec -koordinaattiruudukko"));
            ui.checkbox(&mut self.annotation_config.show_dso_objects, crate::i18n::tr("🌀 DSO-katalogikohteet (M / NGC / IC)"));
            ui.checkbox(&mut self.annotation_config.show_detected_stars, crate::i18n::tr("⭐ Tunnistetut tähdet (#1..#N)"));
            ui.checkbox(&mut self.annotation_config.show_compass, crate::i18n::tr("🧭 Pohjois-/Itä-kompassiruusu (N/E)"));

            if let Some(sol) = &mut self.wcs_solution {
                ui.add_space(2.0);
                ui.label(RichText::new(crate::i18n::tr("Kentän kiertokulma:")).small());
                let mut rot_f32 = sol.rotation_deg as f32;
                if ui
                    .add(egui::Slider::new(&mut rot_f32, -180.0..=180.0).suffix("°").step_by(0.5))
                    .changed()
                {
                    sol.rotation_deg = rot_f32 as f64;
                    sol.populate_catalog_objects(&self.sky_catalog);
                }

                ui.label(RichText::new(crate::i18n::tr("Pikseliskaala (\"/px):")).small());
                let mut scale_f32 = sol.arcsec_per_px as f32;
                if ui
                    .add(egui::Slider::new(&mut scale_f32, 0.5..=12.0).suffix("\"/px").step_by(0.05))
                    .changed()
                {
                    sol.arcsec_per_px = scale_f32 as f64;
                    sol.populate_catalog_objects(&self.sky_catalog);
                }
            }

            ui.add_space(4.0);
            let bw = ui.available_width();
            if ui
                .add_sized(
                    [bw, 25.0],
                    egui::Button::new(
                        RichText::new(crate::i18n::tr("🧭 Ratkaise kenttä & Näytä kohteet"))
                            .color(Color32::from_rgb(100, 230, 255))
                            .strong(),
                    ),
                )
                .clicked()
            {
                self.annotation_config.enabled = true;
                self.solve_active_image_wcs();
            }

            if let Some(sol) = &self.wcs_solution {
                let (fov_w, fov_h) = sol.fov_deg();
                ui.add_space(4.0);
                ui.separator();
                ui.label(
                    RichText::new(format!(
                        "📍 {}  {}  |  FOV {:.2}° × {:.2}°",
                        format_ra(sol.center_ra_hours),
                        format_dec(sol.center_dec_deg),
                        fov_w,
                        fov_h
                    ))
                    .small()
                    .color(Color32::from_rgb(140, 220, 255)),
                );
                ui.label(
                    RichText::new(format!(
                        "⭐ {} tähteä, {} DSO-kohdetta  |  {}",
                        sol.detected_stars.len(),
                        sol.matched_objects.len(),
                        sol.solver_source
                    ))
                    .small()
                    .color(Color32::from_rgb(180, 240, 190)),
                );
            }
        });

        ui.add_space(8.0);

        // PCC / SPCC (Photometric & Spectrophotometric Color Calibration)
        ui.group(|ui| {
            let gw = ui.available_width();
            ui.set_width(gw);
            ui.set_max_width(gw);
            ui.spacing_mut().slider_width = (gw - 76.0).max(100.0);
            ui.strong(crate::i18n::tr("🎯 Fotometrinen värikalibrointi (PCC / SPCC)"));
            ui.label(crate::i18n::tr("Kalibroi kuvan värit automaattisesti saturoitumattomien tähtien fotometrian, mustan kappaleen säteilykäyrän ja kennon/suotimen spektrivasteen perusteella."));

            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(crate::i18n::tr("Menetelmä:"));
                ui.radio_value(&mut self.pcc_config.mode, PccMode::Spcc, crate::i18n::tr("SPCC (Spektrofotometrinen)"));
                ui.radio_value(&mut self.pcc_config.mode, PccMode::Pcc, crate::i18n::tr("PCC (Fotometrinen)"));
            });

            let combo_w = (gw - 12.0).max(150.0);
            ui.label(RichText::new(crate::i18n::tr("Valkopisteen referenssi:")).small());
            egui::ComboBox::from_id_salt("pcc_white_ref")
                .selected_text(self.pcc_config.white_reference.label())
                .width(combo_w)
                .show_ui(ui, |ui| {
                    for wref in [
                        WhiteReference::AverageSpiralGalaxy,
                        WhiteReference::G2VStar,
                        WhiteReference::A0VVega,
                        WhiteReference::EllipticalGalaxy,
                    ] {
                        ui.selectable_value(&mut self.pcc_config.white_reference, wref, wref.label());
                    }
                });

            if self.pcc_config.mode == PccMode::Spcc {
                ui.add_space(2.0);
                ui.label(RichText::new(crate::i18n::tr("Kameran kennoprofiili:")).small());
                egui::ComboBox::from_id_salt("spcc_sensor_prof")
                    .selected_text(self.pcc_config.sensor.label())
                    .width(combo_w)
                    .show_ui(ui, |ui| {
                        for s in [
                            SensorProfile::SeestarS50Imx462,
                            SensorProfile::SeestarS30Imx662,
                            SensorProfile::SonyImx585,
                            SensorProfile::SonyImx678,
                            SensorProfile::SonyImx415,
                            SensorProfile::SonyImx676,
                            SensorProfile::SonyImx178,
                            SensorProfile::SonyImx533Imx571,
                            SensorProfile::GenericOsc,
                        ] {
                            ui.selectable_value(&mut self.pcc_config.sensor, s, s.label());
                        }
                    });

                ui.label(RichText::new(crate::i18n::tr("Suodatinprofiili:")).small());
                egui::ComboBox::from_id_salt("spcc_filter_prof")
                    .selected_text(self.pcc_config.filter.label())
                    .width(combo_w)
                    .show_ui(ui, |ui| {
                        for f in [
                            SpccFilterProfile::UvIrCut,
                            SpccFilterProfile::SeestarDualBandLp,
                            SpccFilterProfile::NarrowDualBand,
                        ] {
                            ui.selectable_value(&mut self.pcc_config.filter, f, f.label());
                        }
                    });

                if let Some(img) = self.active_image() {
                    let meta_clone = img.metadata.clone();
                    let bw = ui.available_width();
                    if ui
                        .add_sized([bw, 22.0], egui::Button::new(crate::i18n::tr("🔍 Tunnista kenno & suodin FITS-otsikosta")))
                        .clicked()
                    {
                        self.status_message = self.pcc_config.auto_detect_from_metadata(&meta_clone);
                    }
                }
            }

            ui.add_space(2.0);
            ui.checkbox(
                &mut self.pcc_config.neutralize_background,
                crate::i18n::tr("🌐 Neutraloi taustataivas"),
            );

            ui.add_space(4.0);
            let bw = ui.available_width();
            if ui
                .add_sized(
                    [bw, 26.0],
                    egui::Button::new(
                        RichText::new(crate::i18n::tr("🎯 Suorita fotometrinen värikalibrointi"))
                            .color(Color32::from_rgb(100, 255, 160))
                            .strong(),
                    ),
                )
                .clicked()
            {
                self.run_pcc();
            }

            if let Some(res) = &self.last_pcc_result {
                ui.add_space(4.0);
                ui.separator();
                ui.label(
                    RichText::new(format!(
                        "⭐ Tähtiotos: {} / {} saturoitumatonta tähteä (RMS {:.3})",
                        res.valid_stars, res.detected_stars, res.fit_rms
                    ))
                    .small()
                    .color(Color32::from_rgb(140, 220, 255)),
                );
                ui.label(
                    RichText::new(format!(
                        "⚖️ Kertoimet: R={:.3}×  G={:.3}×  B={:.3}×  |  T_keskim ≈ {:.0} K",
                        res.k_r, res.k_g, res.k_b, res.mean_star_temp_k
                    ))
                    .small()
                    .color(Color32::from_rgb(180, 240, 190)),
                );
            }
        });

        ui.add_space(8.0);

        // White Balance
        ui.group(|ui| {
            let gw = ui.available_width();
            ui.set_width(gw);
            ui.set_max_width(gw);
            ui.spacing_mut().slider_width = (gw - 76.0).max(100.0);
            ui.strong(crate::i18n::tr("⚖️ Valkotasapaino"));
            ui.label(crate::i18n::tr("Hienosäädä tähtien ja sumun väritasapainoa."));

            ui.label(RichText::new(crate::i18n::tr("Punainen (R):")).small());
            ui.add(egui::Slider::new(&mut self.wb_r, 0.5..=2.0).trailing_fill(true));
            ui.label(RichText::new(crate::i18n::tr("Vihreä (G):")).small());
            ui.add(egui::Slider::new(&mut self.wb_g, 0.5..=2.0).trailing_fill(true));
            ui.label(RichText::new(crate::i18n::tr("Sininen (B):")).small());
            ui.add(egui::Slider::new(&mut self.wb_b, 0.5..=2.0).trailing_fill(true));

            ui.horizontal_wrapped(|ui| {
                if ui.button(crate::i18n::tr("Aseta väritasapaino")).clicked() {
                    self.run_color_balance();
                }
                if ui.button(crate::i18n::tr("Palauta 1.0")).clicked() {
                    self.wb_r = 1.0;
                    self.wb_g = 1.0;
                    self.wb_b = 1.0;
                }
            });
        });

        ui.add_space(8.0);

        // Saturation
        ui.group(|ui| {
            let gw = ui.available_width();
            ui.set_width(gw);
            ui.set_max_width(gw);
            ui.spacing_mut().slider_width = (gw - 76.0).max(100.0);
            ui.strong(crate::i18n::tr("🎨 Värikylläisyys"));
            ui.label(crate::i18n::tr("Korostaa sumujen ja galaksien värejä."));

            ui.label(RichText::new(crate::i18n::tr("Kylläisyyskerroin:")).small());
            ui.add(egui::Slider::new(&mut self.saturation_factor, 0.5..=3.0).trailing_fill(true));

            let bw = ui.available_width();
            if ui.add_sized([bw, 24.0], egui::Button::new(crate::i18n::tr("Kylläisyyskorostus"))).clicked() {
                self.run_saturation();
            }
        });

        ui.add_space(8.0);

        // Star Reduction
        ui.group(|ui| {
            let gw = ui.available_width();
            ui.set_width(gw);
            ui.set_max_width(gw);
            ui.spacing_mut().slider_width = (gw - 76.0).max(100.0);
            ui.strong(crate::i18n::tr("✨ Tähtien pienennys"));
            ui.label(crate::i18n::tr("Pienentää tähtien kokoa ja kirkkautta tuoden sumurakenteet paremmin esiin."));

            ui.label(RichText::new(crate::i18n::tr("Voimakkuus:")).small());
            ui.add(egui::Slider::new(&mut self.star_reduction_amount, 0.1..=1.0).trailing_fill(true));
            ui.horizontal_wrapped(|ui| {
                ui.label(crate::i18n::tr("Kokoluokka:"));
                ui.radio_value(&mut self.star_reduction_radius, 1, crate::i18n::tr("Pienet tähdet (Säde 1)"));
                ui.radio_value(&mut self.star_reduction_radius, 2, crate::i18n::tr("Keskikokoiset tähdet (Säde 2)"));
            });

            let bw = ui.available_width();
            if ui.add_sized([bw, 24.0], egui::Button::new(crate::i18n::tr("✨ Pienennä tähtiä"))).clicked() {
                self.run_star_reduction();
            }
        });
    }

    fn ui_curves_editor(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());

            // Channel selector
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(crate::i18n::tr("Kanava:")).strong());
                for &ch in &[
                    CurvesChannel::Rgb,
                    CurvesChannel::Red,
                    CurvesChannel::Green,
                    CurvesChannel::Blue,
                    CurvesChannel::Saturation,
                ] {
                    let (r, g, b) = ch.color_rgb();
                    let is_active = self.curves_channel == ch;
                    let has_mod = !self.curves.get_curve(ch).is_identity();
                    let label = if has_mod {
                        format!("{} *", ch.name())
                    } else {
                        ch.name().to_string()
                    };

                    let btn_text = if is_active {
                        RichText::new(label).color(Color32::from_rgb(r, g, b)).strong()
                    } else {
                        RichText::new(label).color(Color32::from_rgb(r / 2 + 50, g / 2 + 50, b / 2 + 50))
                    };

                    if ui.selectable_label(is_active, btn_text).clicked() {
                        self.curves_channel = ch;
                        self.curves_selected_point = None;
                    }
                }
            });

            ui.add_space(4.0);

            // Curve plot area
            let avail_w = ui.available_width().max(180.0);
            let plot_w = avail_w.clamp(200.0, 380.0);
            let plot_size = Vec2::new(plot_w, (plot_w * 0.85).clamp(180.0, 300.0));
            let (response, painter) = ui.allocate_painter(plot_size, egui::Sense::click_and_drag());
            let rect = response.rect;

            // Background
            painter.rect_filled(rect, 4.0, Color32::from_rgb(14, 17, 24));
            painter.rect_stroke(
                rect,
                4.0,
                egui::Stroke::new(1.0_f32, Color32::from_rgb(40, 48, 65)),
                egui::StrokeKind::Inside,
            );

            // Grid lines (25%, 50%, 75%)
            let grid_color = Color32::from_rgb(32, 38, 52);
            for frac in [0.25f32, 0.50, 0.75] {
                let x = rect.left() + frac * rect.width();
                painter.line_segment(
                    [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                    egui::Stroke::new(1.0_f32, grid_color),
                );
                let y = rect.bottom() - frac * rect.height();
                painter.line_segment(
                    [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                    egui::Stroke::new(1.0_f32, grid_color),
                );
            }

            // Diagonal reference line (0,0) -> (1,1)
            painter.line_segment(
                [egui::pos2(rect.left(), rect.bottom()), egui::pos2(rect.right(), rect.top())],
                egui::Stroke::new(1.0_f32, Color32::from_rgb(55, 62, 78)),
            );

            // Draw underlying histogram if active image exists
            if let Some(img) = self.active_image() {
                let hist = compute_preview_histogram(img, self.curves_channel);
                let bar_w = rect.width() / 64.0;
                for b in 0..64 {
                    let h = hist[b] * rect.height() * 0.75;
                    let x0 = rect.left() + b as f32 * bar_w;
                    let x1 = x0 + bar_w;
                    let y_top = rect.bottom() - h;
                    painter.rect_filled(
                        egui::Rect::from_min_max(egui::pos2(x0, y_top), egui::pos2(x1, rect.bottom())),
                        0.0,
                        Color32::from_rgba_unmultiplied(75, 95, 130, 45),
                    );
                }
            }

            // Draw inactive curves faintly
            for &ch in &[
                CurvesChannel::Rgb,
                CurvesChannel::Red,
                CurvesChannel::Green,
                CurvesChannel::Blue,
                CurvesChannel::Saturation,
            ] {
                if ch == self.curves_channel {
                    continue;
                }
                let other_curve = self.curves.get_curve(ch);
                if other_curve.is_identity() {
                    continue;
                }
                let (r, g, b) = ch.color_rgb();
                let faint_color = Color32::from_rgba_unmultiplied(r, g, b, 70);
                let steps = 60;
                let mut pts = Vec::with_capacity(steps + 1);
                for i in 0..=steps {
                    let x_norm = i as f32 / steps as f32;
                    let y_norm = other_curve.evaluate_pchip(x_norm);
                    pts.push(egui::pos2(
                        rect.left() + x_norm * rect.width(),
                        rect.bottom() - y_norm * rect.height(),
                    ));
                }
                painter.add(egui::Shape::line(pts, egui::Stroke::new(1.2_f32, faint_color)));
            }

            // Active curve
            let (r, g, b) = self.curves_channel.color_rgb();
            let active_color = Color32::from_rgb(r, g, b);

            let active_curve = self.curves.get_curve(self.curves_channel);
            let steps = 120;
            let mut curve_pts = Vec::with_capacity(steps + 1);
            for i in 0..=steps {
                let x_norm = i as f32 / steps as f32;
                let y_norm = active_curve.evaluate_pchip(x_norm);
                curve_pts.push(egui::pos2(
                    rect.left() + x_norm * rect.width(),
                    rect.bottom() - y_norm * rect.height(),
                ));
            }
            painter.add(egui::Shape::line(curve_pts, egui::Stroke::new(2.5_f32, active_color)));

            // Draw control points
            let current_points = self.curves.get_curve(self.curves_channel).points.clone();
            for (i, p) in current_points.iter().enumerate() {
                let pt_center = egui::pos2(
                    rect.left() + p.x * rect.width(),
                    rect.bottom() - p.y * rect.height(),
                );
                let is_selected = self.curves_selected_point == Some(i);
                let radius = if is_selected { 6.5 } else { 4.5 };
                let fill = if is_selected {
                    Color32::WHITE
                } else {
                    active_color
                };
                let stroke = egui::Stroke::new(1.5_f32, Color32::from_rgb(14, 17, 24));
                painter.circle(pt_center, radius, fill, stroke);
            }

            // User input handling (click to add, drag, right-click to delete)
            let channel = self.curves_channel;
            let curve = self.curves.get_curve_mut(channel);
            if let Some(mouse_pos) = response.interact_pointer_pos() {
                let norm_x = ((mouse_pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                let norm_y = ((rect.bottom() - mouse_pos.y) / rect.height()).clamp(0.0, 1.0);

                let mut nearest_idx = None;
                let mut min_dist = f32::MAX;
                for (i, p) in curve.points.iter().enumerate() {
                    let px_pos = egui::pos2(
                        rect.left() + p.x * rect.width(),
                        rect.bottom() - p.y * rect.height(),
                    );
                    let dist = mouse_pos.distance(px_pos);
                    if dist < min_dist {
                        min_dist = dist;
                        nearest_idx = Some(i);
                    }
                }

                if response.drag_started() {
                    if min_dist < 16.0 {
                        self.curves_selected_point = nearest_idx;
                    } else if norm_x > 0.02 && norm_x < 0.98 {
                        let new_idx = curve.add_point(norm_x, norm_y);
                        self.curves_selected_point = Some(new_idx);
                    }
                } else if response.dragged() {
                    if let Some(sel) = self.curves_selected_point {
                        if sel < curve.points.len() {
                            if sel == 0 {
                                curve.points[sel].x = 0.0;
                                curve.points[sel].y = norm_y;
                            } else if sel == curve.points.len() - 1 {
                                curve.points[sel].x = 1.0;
                                curve.points[sel].y = norm_y;
                            } else {
                                let min_x = curve.points[sel - 1].x + 0.01;
                                let max_x = curve.points[sel + 1].x - 0.01;
                                curve.points[sel].x = norm_x.clamp(min_x, max_x);
                                curve.points[sel].y = norm_y;
                            }
                        }
                    }
                }

                if response.secondary_clicked() && min_dist < 16.0 {
                    if let Some(idx) = nearest_idx {
                        if curve.remove_point(idx) {
                            self.curves_selected_point = None;
                        }
                    }
                }
            }

            ui.add_space(4.0);

            // Readouts & Point deletion
            let curve = self.curves.get_curve_mut(self.curves_channel);
            ui.horizontal_wrapped(|ui| {
                if let Some(sel) = self.curves_selected_point {
                    if sel < curve.points.len() {
                        let pt = curve.points[sel];
                        ui.label(
                            RichText::new(format!("Piste {}: In: {:.2} | Out: {:.2}", sel + 1, pt.x, pt.y))
                                .size(11.5)
                                .color(Color32::from_rgb(200, 210, 230)),
                        );
                    }
                } else {
                    ui.label(
                        RichText::new(crate::i18n::tr("Klikkaa käyrää lisätäksesi pisteitä."))
                            .size(11.0)
                            .color(Color32::from_rgb(140, 150, 170)),
                    );
                }

                if let Some(sel) = self.curves_selected_point {
                    let can_delete = sel > 0 && sel < curve.points.len() - 1;
                    if ui
                        .add_enabled(can_delete, egui::Button::new(crate::i18n::tr("🗑 Poista")))
                        .on_hover_text(crate::i18n::tr("Poista valittu kontrollipiste (tai hiiren oikealla)"))
                        .clicked()
                    {
                        curve.remove_point(sel);
                        self.curves_selected_point = None;
                    }
                }

                if ui.button(crate::i18n::tr("↺ Tyhjennä")).on_hover_text(crate::i18n::tr("Palauta aktiivinen kanava suoraksi")).clicked() {
                    curve.reset();
                    self.curves_selected_point = None;
                }
            });

            ui.add_space(6.0);

            // Presets
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(crate::i18n::tr("Esiasetukset:")).strong());

                if ui.button(crate::i18n::tr("📈 S-käyrä")).on_hover_text(crate::i18n::tr("Klassinen kontrastia nostava S-käyrä")).clicked() {
                    *self.curves.get_curve_mut(self.curves_channel) = SplineCurve::preset_s_curve();
                    self.curves_selected_point = None;
                }

                if ui.button(crate::i18n::tr("💡 Keskisävyt")).on_hover_text(crate::i18n::tr("Nosta himmeitä sumuja ja heijastuksia")).clicked() {
                    *self.curves.get_curve_mut(self.curves_channel) = SplineCurve::preset_lift_midtones();
                    self.curves_selected_point = None;
                }

                if ui.button(crate::i18n::tr("🌌 Tausta")).on_hover_text(crate::i18n::tr("Laskee varjoja ja syventää avaruustaustaa")).clicked() {
                    *self.curves.get_curve_mut(self.curves_channel) = SplineCurve::preset_darken_shadows();
                    self.curves_selected_point = None;
                }

                if ui.button(crate::i18n::tr("⚡ Kontrasti")).clicked() {
                    *self.curves.get_curve_mut(self.curves_channel) = SplineCurve::preset_high_contrast();
                    self.curves_selected_point = None;
                }

                if self.curves_channel == CurvesChannel::Saturation {
                    if ui.button(crate::i18n::tr("✨ Kylläisyys+")).on_hover_text(crate::i18n::tr("Nosta sumujen värikylläisyyttä suojaten taustaa")).clicked() {
                        *self.curves.get_curve_mut(CurvesChannel::Saturation) = SplineCurve::preset_saturation_boost();
                        self.curves_selected_point = None;
                    }
                }

                if ui.button(crate::i18n::tr("🔄 Palauta kaikki")).on_hover_text(crate::i18n::tr("Palauta kaikki kanavat (RGB, R, G, B, S)")).clicked() {
                    self.curves.reset_all();
                    self.curves_selected_point = None;
                }
            });

            ui.add_space(6.0);

            // Live preview checkbox & Apply
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.curves_live_preview, crate::i18n::tr("👁 Esikatselu"));

                let has_curves = !self.curves.is_identity();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(has_curves, egui::Button::new(RichText::new(crate::i18n::tr("🚀 Sovella kuvaan")).strong().color(Color32::from_rgb(100, 255, 180))))
                        .on_hover_text(crate::i18n::tr("Sovella käyrät pysyvästi 32-bittiseen HDR-kuvaan (luo uuden käsitellyn kuvan)"))
                        .clicked()
                    {
                        self.run_apply_curves();
                    }
                });
            });
        });
    }
}

fn compute_preview_histogram(img: &AstroImage, channel: CurvesChannel) -> [f32; 64] {
    let mut hist = [0u32; 64];
    let total_pixels = img.width * img.height;
    if total_pixels == 0 {
        return [0.0; 64];
    }
    let step = (total_pixels / 30000).max(1);
    let ch_idx = match channel {
        CurvesChannel::Red => 0,
        CurvesChannel::Green => 1.min(img.channels - 1),
        CurvesChannel::Blue => 2.min(img.channels - 1),
        _ => 0,
    };
    let is_rgb = img.channels == 3;
    for i in (0..total_pixels).step_by(step) {
        let val = if is_rgb && (channel == CurvesChannel::Rgb || channel == CurvesChannel::Saturation) {
            let r = img.data[i * 3];
            let g = img.data[i * 3 + 1];
            let b = img.data[i * 3 + 2];
            0.2126 * r + 0.7152 * g + 0.0722 * b
        } else {
            let idx = i * img.channels + ch_idx;
            if idx < img.data.len() {
                img.data[idx]
            } else {
                0.0
            }
        };
        let bin = ((val.clamp(0.0, 1.0) * 63.0) as usize).min(63);
        hist[bin] += 1;
    }
    let mut normalized = [0.0f32; 64];
    let max = *hist.iter().max().unwrap_or(&1).max(&1) as f32;
    for b in 0..64 {
        normalized[b] = hist[b] as f32 / max;
    }
    normalized
}
