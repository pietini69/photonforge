use crate::core::fits::FitsParser;
use crate::core::image::AstroImage;
use crate::processing::background::{BackgroundExtractor, ExtractionMode};
use crate::processing::star_reduction::StarReduction;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Supported SyQon Neural AI Models
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyQonModel {
    /// Free fast star separation (Axiom Mini)
    AxiomMini,
    /// Licensed full star separation (Axiom V3)
    AxiomV3,
    /// Free stellar repair, reduction and deconvolution (Parallax Nano)
    ParallaxNano,
    /// Premium aesthetic stellar repair, reduction and deblur (Parallax Aesthetics)
    ParallaxAesthetics,
    /// Premium classic stellar repair, reduction and deblur (Parallax Classic)
    ParallaxClassic,
    /// Essential deep denoise and restoration for linear RGB / mono (Prism Essential)
    PrismEssential,
    /// Deep Max non-linear / linear restoration (Prism Max)
    PrismMax,
    /// AI background gradient extraction and removal (Deep Gradient)
    DeepGradient,
}

#[allow(dead_code)]
impl SyQonModel {
    pub fn cli_model_id(&self) -> &'static str {
        match self {
            Self::AxiomMini => "axiom-mini",
            Self::AxiomV3 => "axiom",
            Self::ParallaxNano => "parallax-nano",
            Self::ParallaxAesthetics | Self::ParallaxClassic => "parallax",
            Self::PrismEssential => "prism-essential",
            Self::PrismMax => "prism-max",
            Self::DeepGradient => "deep-gradient",
        }
    }

    pub fn family(&self) -> Option<&'static str> {
        match self {
            Self::ParallaxAesthetics => Some("aesthetics"),
            Self::ParallaxClassic => Some("classic"),
            _ => None,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::AxiomMini => "Axiom Mini (Ilmainen tähtien poisto)",
            Self::AxiomV3 => "Axiom V3 (Täysi tähtien erottelu)",
            Self::ParallaxNano => "Parallax Nano (Ilmainen tähtikorjaus & deblur)",
            Self::ParallaxAesthetics => "Parallax Aesthetics (Visuaalinen tähtiparannus)",
            Self::ParallaxClassic => "Parallax Classic (Klassinen pisteparannus)",
            Self::PrismEssential => "Prism Essential (AI-kohinanpoisto)",
            Self::PrismMax => "Prism Max (Syvä kohinanpoisto)",
            Self::DeepGradient => "Deep Gradient (AI-taustatasoitus)",
        }
    }

    pub fn is_free(&self) -> bool {
        matches!(self, Self::AxiomMini | Self::ParallaxNano | Self::PrismEssential)
    }
}

/// Parameters for SyQon execution
#[derive(Clone, Debug)]
pub enum SyQonTask {
    StarSeparation {
        model: SyQonModel,
        stretch: String, // "auto", "none", "linear"
        generate_stars_only: bool,
    },
    Parallax {
        model: SyQonModel,
        correction: bool,
        reduction: bool,
        reduction_level: u32, // 0..=10
        deblur: bool,
        deblur_strength: f32, // 0.0..=1.0
    },
    Denoise {
        model: SyQonModel,
        domain: String, // "auto", "linear", "nonlinear"
        application: f32, // 0.0..=1.0
    },
    Gradient {
        save_gradient: bool,
    },
    InteractiveGui,
}

#[derive(Clone, Debug)]
pub struct SyQonProgress {
    pub stage: String,
    pub detail: String,
    pub fraction: f32,
}

pub struct SyQonResult {
    pub primary: AstroImage,
    pub secondary: Option<AstroImage>, // E.g. stars-only or extracted gradient
    pub description: String,
}

pub struct SyQonEngine;

impl SyQonEngine {
    /// Discovers the path to `syqon-cli` on macOS / Windows / Linux
    pub fn find_cli() -> Option<PathBuf> {
        // 1. Explicit environment variable
        if let Ok(p) = std::env::var("SYQON_CLI_PATH") {
            let path = PathBuf::from(p);
            if path.exists() {
                return Some(path);
            }
        }

        // 2. macOS System standard app bundle
        let sys_app = PathBuf::from("/Applications/SyQon Studio.app/Contents/MacOS/syqon-cli");
        if sys_app.exists() {
            return Some(sys_app);
        }

        // 3. macOS User applications bundle
        if let Ok(home) = std::env::var("HOME") {
            let user_app = PathBuf::from(home).join("Applications/SyQon Studio.app/Contents/MacOS/syqon-cli");
            if user_app.exists() {
                return Some(user_app);
            }
        }

        // 4. Windows LocalAppData
        #[cfg(target_os = "windows")]
        if let Ok(local_app) = std::env::var("LOCALAPPDATA") {
            let win_path = PathBuf::from(local_app).join("Programs/SyQon Studio/syqon-cli.exe");
            if win_path.exists() {
                return Some(win_path);
            }
        }

        // 5. Look in PATH
        if let Ok(path_var) = std::env::var("PATH") {
            for dir in std::env::split_paths(&path_var) {
                let candidate = dir.join("syqon-cli");
                if candidate.exists() {
                    return Some(candidate);
                }
                #[cfg(target_os = "windows")]
                {
                    let win_candidate = dir.join("syqon-cli.exe");
                    if win_candidate.exists() {
                        return Some(win_candidate);
                    }
                }
            }
        }

        None
    }

    /// Returns true if `syqon-cli` executable exists on the system
    pub fn is_available() -> bool {
        Self::find_cli().is_some()
    }

    /// Queries `syqon-cli --version`
    pub fn get_version() -> Option<String> {
        let cli = Self::find_cli()?;
        let output = Command::new(cli)
            .arg("--version")
            .output()
            .ok()?;
        if output.status.success() {
            Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            None
        }
    }

    /// Parses a progress line from syqon-cli stderr, e.g. "axiom-mini 45%" or "45%"
    pub fn parse_progress_line(line: &str) -> Option<(String, f32)> {
        let trimmed = line.trim();
        if let Some(pct_idx) = trimmed.find('%') {
            let before_pct = &trimmed[..pct_idx];
            if let Some(last_token) = before_pct.split_whitespace().last() {
                if let Ok(num) = last_token.parse::<f32>() {
                    let fraction = (num / 100.0).clamp(0.0, 1.0);
                    let prefix = before_pct[..before_pct.len() - last_token.len()].trim();
                    let stage = if prefix.is_empty() {
                        "Käsitellään".to_string()
                    } else {
                        prefix.to_string()
                    };
                    return Some((stage, fraction));
                }
            }
        }
        None
    }

    /// Executes a SyQon task. If `cli_path` is not available and `simulate_if_missing` is true,
    /// performs an in-memory simulated transformation so the UI flow can be used and tested.
    pub fn run(
        input: &AstroImage,
        task: &SyQonTask,
        on_progress: Option<Arc<dyn Fn(SyQonProgress) + Send + Sync>>,
        cancel_token: Arc<AtomicBool>,
        simulate_if_missing: bool,
    ) -> Result<SyQonResult, String> {
        let cli_path = Self::find_cli();

        if cli_path.is_none() {
            if simulate_if_missing {
                return Self::run_simulated(input, task, on_progress, cancel_token);
            } else {
                return Err("SyQon CLI ei ole asennettuna. Asenna SyQon Studio osoitteesta https://syqon.eu".to_string());
            }
        }

        let cli = cli_path.unwrap();
        let temp_dir = std::env::temp_dir();
        let timestamp = Instant::now().elapsed().as_nanos();
        let input_path = temp_dir.join(format!("pf_syqon_in_{}.fits", timestamp));
        let output_primary = temp_dir.join(format!("pf_syqon_out_{}.fits", timestamp));
        let output_secondary = temp_dir.join(format!("pf_syqon_sec_{}.fits", timestamp));

        // Write input image to 32-bit FITS
        if let Some(cb) = &on_progress {
            cb(SyQonProgress {
                stage: "Valmistellaan".to_string(),
                detail: "Tallennetaan 32-bit FITS väliaikaistiedostoon...".to_string(),
                fraction: 0.05,
            });
        }
        FitsParser::write_file(input, &input_path)
            .map_err(|e| format!("FITS-väliaikaistiedoston kirjoitus epäonnistui: {}", e))?;

        // Build command arguments
        let mut cmd = Command::new(&cli);
        let mut expect_secondary = false;

        match task {
            SyQonTask::InteractiveGui => {
                cmd.arg("-gui");
                cmd.arg(&input_path);
                cmd.arg(&output_primary);
                cmd.arg("--overwrite");
            }
            SyQonTask::StarSeparation { model, stretch, generate_stars_only } => {
                cmd.arg("--model").arg(model.cli_model_id());
                cmd.arg("--axiom-stretch").arg(stretch);
                if *generate_stars_only {
                    cmd.arg("--stars-output").arg(&output_secondary);
                    expect_secondary = true;
                }
                cmd.arg("--precision").arg("f32");
                cmd.arg("--overwrite");
                cmd.arg(&input_path);
                cmd.arg(&output_primary);
            }
            SyQonTask::Parallax { model, correction, reduction, reduction_level, deblur, deblur_strength } => {
                cmd.arg("--model").arg(model.cli_model_id());
                if let Some(fam) = model.family() {
                    cmd.arg("--family").arg(fam);
                }
                cmd.arg("--correction").arg(if *correction { "true" } else { "false" });
                cmd.arg("--reduction").arg(if *reduction { "true" } else { "false" });
                cmd.arg("--reduction-level").arg(reduction_level.to_string());
                cmd.arg("--deblur").arg(if *deblur { "true" } else { "false" });
                cmd.arg("--deblur-strength").arg(format!("{:.2}", deblur_strength));
                cmd.arg("--precision").arg("f32");
                cmd.arg("--overwrite");
                cmd.arg(&input_path);
                cmd.arg(&output_primary);
            }
            SyQonTask::Denoise { model, domain, application } => {
                cmd.arg("--model").arg(model.cli_model_id());
                cmd.arg("--domain").arg(domain);
                cmd.arg("--application").arg(format!("{:.2}", application));
                cmd.arg("--precision").arg("f32");
                cmd.arg("--overwrite");
                cmd.arg(&input_path);
                cmd.arg(&output_primary);
            }
            SyQonTask::Gradient { save_gradient } => {
                cmd.arg("--model").arg(SyQonModel::DeepGradient.cli_model_id());
                if *save_gradient {
                    cmd.arg("--gradient-output").arg(&output_secondary);
                    expect_secondary = true;
                }
                cmd.arg("--precision").arg("f32");
                cmd.arg("--overwrite");
                cmd.arg(&input_path);
                cmd.arg(&output_primary);
            }
        }

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| format!("SyQon CLI käynnistys epäonnistui: {}", e))?;

        // Monitor stderr stream for live progress percentages
        if let Some(stderr) = child.stderr.take() {
            let reader = BufReader::new(stderr);
            let on_progress_clone = on_progress.clone();
            let cancel_clone = Arc::clone(&cancel_token);

            std::thread::spawn(move || {
                for line_res in reader.lines() {
                    if cancel_clone.load(Ordering::SeqCst) {
                        break;
                    }
                    if let Ok(line) = line_res {
                        if let Some((stage, frac)) = Self::parse_progress_line(&line) {
                            if let Some(ref cb) = on_progress_clone {
                                cb(SyQonProgress {
                                    stage: format!("SyQon AI: {}", stage),
                                    detail: line.clone(),
                                    fraction: 0.1 + frac * 0.85,
                                });
                            }
                        }
                    }
                }
            });
        }

        // Wait for process with cancellation check
        loop {
            if cancel_token.load(Ordering::SeqCst) {
                let _ = child.kill();
                let _ = std::fs::remove_file(&input_path);
                let _ = std::fs::remove_file(&output_primary);
                let _ = std::fs::remove_file(&output_secondary);
                return Err("Käyttäjä peruutti SyQon-käsittelyn.".to_string());
            }

            match child.try_wait() {
                Ok(Some(status)) => {
                    let _ = std::fs::remove_file(&input_path);

                    if status.code() == Some(20) {
                        // User cancelled in GUI editor
                        let _ = std::fs::remove_file(&output_primary);
                        let _ = std::fs::remove_file(&output_secondary);
                        return Err("Käyttäjä sulki SyQon-editorin ilman tallennusta.".to_string());
                    }

                    if !status.success() {
                        let _ = std::fs::remove_file(&output_primary);
                        let _ = std::fs::remove_file(&output_secondary);
                        return Err(format!("SyQon CLI päättyi virhekoodiin: {:?}", status.code()));
                    }
                    break;
                }
                Ok(None) => {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                Err(e) => {
                    let _ = std::fs::remove_file(&input_path);
                    return Err(format!("Virhe lapsiprosessin odotuksessa: {}", e));
                }
            }
        }

        // Load primary output
        if !output_primary.exists() {
            return Err("SyQon ei luonut tulostiedostoa.".to_string());
        }

        let mut primary_img = FitsParser::read_file(&output_primary)
            .map_err(|e| format!("Tulostiedoston lataus epäonnistui: {}", e))?;
        let _ = std::fs::remove_file(&output_primary);

        let mut secondary_img = None;
        if expect_secondary && output_secondary.exists() {
            if let Ok(mut sec) = FitsParser::read_file(&output_secondary) {
                sec.metadata.file_name = format!("{}_Stars.fits", input.metadata.file_name.trim_end_matches(".fits"));
                secondary_img = Some(sec);
            }
            let _ = std::fs::remove_file(&output_secondary);
        }

        let desc = match task {
            SyQonTask::StarSeparation { .. } => {
                primary_img.metadata.file_name = format!("{}_Starless.fits", input.metadata.file_name.trim_end_matches(".fits"));
                "SyQon Axiom: Tähdetön kuva luotu".to_string()
            }
            SyQonTask::Parallax { .. } => {
                primary_img.metadata.file_name = format!("{}_Parallax.fits", input.metadata.file_name.trim_end_matches(".fits"));
                "SyQon Parallax: Tähtien korjaus & deblur valmis".to_string()
            }
            SyQonTask::Denoise { .. } => {
                primary_img.metadata.file_name = format!("{}_Prism.fits", input.metadata.file_name.trim_end_matches(".fits"));
                "SyQon Prism: AI-kohinanpoisto valmis".to_string()
            }
            SyQonTask::Gradient { .. } => {
                primary_img.metadata.file_name = format!("{}_GradientClean.fits", input.metadata.file_name.trim_end_matches(".fits"));
                "SyQon Deep Gradient: Taustatasoitus valmis".to_string()
            }
            SyQonTask::InteractiveGui => {
                primary_img.metadata.file_name = format!("{}_SyQonStudio.fits", input.metadata.file_name.trim_end_matches(".fits"));
                "SyQon Studio: Interaktiivinen muokkaus tallennettu".to_string()
            }
        };

        Ok(SyQonResult {
            primary: primary_img,
            secondary: secondary_img,
            description: desc,
        })
    }

    /// Simulation fallback when syqon-cli is not installed or for offline UI testing
    pub fn run_simulated(
        input: &AstroImage,
        task: &SyQonTask,
        on_progress: Option<Arc<dyn Fn(SyQonProgress) + Send + Sync>>,
        cancel_token: Arc<AtomicBool>,
    ) -> Result<SyQonResult, String> {
        let steps = 10;
        for s in 1..=steps {
            if cancel_token.load(Ordering::SeqCst) {
                return Err("Käyttäjä peruutti käsittelyn.".to_string());
            }
            std::thread::sleep(std::time::Duration::from_millis(40));
            if let Some(cb) = &on_progress {
                let frac = s as f32 / steps as f32;
                cb(SyQonProgress {
                    stage: "SyQon AI (Simulaatio)".to_string(),
                    detail: format!("Suoritetaan neuroverkkoa... {}%", (frac * 100.0) as u32),
                    fraction: frac,
                });
            }
        }

        match task {
            SyQonTask::StarSeparation { generate_stars_only, .. } => {
                // Generate a starless approximation using heavy morphological erosion
                let starless = StarReduction::reduce_stars(input, 1.0, 2, 2)?;
                let mut starless_img = starless;
                starless_img.metadata.file_name = format!("{}_Starless.fits", input.metadata.file_name.trim_end_matches(".fits"));

                let stars_img = if *generate_stars_only {
                    // Difference between original and starless = stars
                    let mut diff = input.clone();
                    for (d, s) in diff.data.iter_mut().zip(starless_img.data.iter()) {
                        *d = (*d - *s).max(0.0);
                    }
                    diff.metadata.file_name = format!("{}_Stars.fits", input.metadata.file_name.trim_end_matches(".fits"));
                    Some(diff)
                } else {
                    None
                };

                Ok(SyQonResult {
                    primary: starless_img,
                    secondary: stars_img,
                    description: "SyQon Axiom (Simulaatio): Tähtien poisto valmis".to_string(),
                })
            }
            SyQonTask::Parallax { reduction_level, deblur_strength, .. } => {
                let radius = if *reduction_level > 5 { 2 } else { 1 };
                let amount = (*reduction_level as f32 / 10.0).clamp(0.2, 0.9);
                let mut res = StarReduction::reduce_stars(input, amount, radius, 1)?;
                // Gentle high-frequency boost if deblur is requested
                if *deblur_strength > 0.0 {
                    let w = res.width;
                    let h = res.height;
                    let ch = res.channels;
                    let orig = res.data.clone();
                    for y in 1..h - 1 {
                        for x in 1..w - 1 {
                            for c in 0..ch {
                                let idx = (y * w + x) * ch + c;
                                let center = orig[idx];
                                let left = orig[(y * w + (x - 1)) * ch + c];
                                let right = orig[(y * w + (x + 1)) * ch + c];
                                let up = orig[((y - 1) * w + x) * ch + c];
                                let down = orig[((y + 1) * w + x) * ch + c];
                                let laplacian = 4.0 * center - (left + right + up + down);
                                res.data[idx] = (center + laplacian * (*deblur_strength * 0.15)).clamp(0.0, 1.0);
                            }
                        }
                    }
                }
                res.metadata.file_name = format!("{}_Parallax.fits", input.metadata.file_name.trim_end_matches(".fits"));
                Ok(SyQonResult {
                    primary: res,
                    secondary: None,
                    description: "SyQon Parallax (Simulaatio): Tähtikorjaus & deblur valmis".to_string(),
                })
            }
            SyQonTask::Denoise { application, .. } => {
                // Fast 3x3 bilateral/box denoise filter
                let mut res = input.clone();
                let w = res.width;
                let h = res.height;
                let ch = res.channels;
                let orig = input.data.clone();
                let strength = application.clamp(0.1, 1.0);

                for y in 1..h - 1 {
                    for x in 1..w - 1 {
                        for c in 0..ch {
                            let idx = (y * w + x) * ch + c;
                            let center = orig[idx];
                            let mut sum = 0.0;
                            let mut weight_sum = 0.0;
                            for dy in -1..=1 {
                                for dx in -1..=1 {
                                    let n_idx = ((y as isize + dy) as usize * w + (x as isize + dx) as usize) * ch + c;
                                    let val = orig[n_idx];
                                    let diff = (val - center).abs();
                                    let weight = (-diff * 15.0).exp();
                                    sum += val * weight;
                                    weight_sum += weight;
                                }
                            }
                            let smoothed = sum / weight_sum.max(1e-5);
                            res.data[idx] = center * (1.0 - strength * 0.5) + smoothed * (strength * 0.5);
                        }
                    }
                }
                res.metadata.file_name = format!("{}_Prism.fits", input.metadata.file_name.trim_end_matches(".fits"));
                Ok(SyQonResult {
                    primary: res,
                    secondary: None,
                    description: "SyQon Prism (Simulaatio): AI-kohinanpoisto valmis".to_string(),
                })
            }
            SyQonTask::Gradient { save_gradient } => {
                let corrected = BackgroundExtractor::extract_with_params(input, 2, ExtractionMode::Subtraction, 3.0)?;
                let mut primary = corrected;
                primary.metadata.file_name = format!("{}_GradientClean.fits", input.metadata.file_name.trim_end_matches(".fits"));

                let secondary = if *save_gradient {
                    let mut grad = input.clone();
                    for (g, c) in grad.data.iter_mut().zip(primary.data.iter()) {
                        *g = (*g - *c + 0.1).clamp(0.0, 1.0);
                    }
                    grad.metadata.file_name = format!("{}_Gradient.fits", input.metadata.file_name.trim_end_matches(".fits"));
                    Some(grad)
                } else {
                    None
                };

                Ok(SyQonResult {
                    primary,
                    secondary,
                    description: "SyQon Deep Gradient (Simulaatio): Taustatasoitus valmis".to_string(),
                })
            }
            SyQonTask::InteractiveGui => {
                let mut res = input.clone();
                res.metadata.file_name = format!("{}_SyQonStudio.fits", input.metadata.file_name.trim_end_matches(".fits"));
                Ok(SyQonResult {
                    primary: res,
                    secondary: None,
                    description: "SyQon Studio (Simulaatio): Interaktiivinen muokkaus valmis".to_string(),
                })
            }
        }
    }
}
