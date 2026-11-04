use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct LiveIngestConfig {
    pub enabled: bool,
    pub source_path: PathBuf,
    pub destination_path: PathBuf,
    pub auto_live_stack: bool,
    #[allow(dead_code)]
    pub auto_load_into_app: bool,
}

impl Default for LiveIngestConfig {
    fn default() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/Shared".to_string());
        let default_dest = PathBuf::from(home).join("Pictures").join("PhotonForge_Seestar");

        Self {
            enabled: true,
            source_path: PathBuf::from("/Volumes/EMMC Images/MyWorks"),
            destination_path: default_dest,
            auto_live_stack: true,
            auto_load_into_app: false,
        }
    }
}

pub type DownloadCallback = Arc<dyn Fn(PathBuf) + Send + Sync>;

#[derive(Clone, Debug, Default)]
pub struct LiveIngestStatus {
    pub is_active: bool,
    pub source_found: bool,
    pub downloaded_count: usize,
    pub last_file_name: String,
    pub last_sync_time: String,
    pub status_message: String,
}

pub struct LiveIngestManager {
    pub config: Arc<Mutex<LiveIngestConfig>>,
    pub status: Arc<Mutex<LiveIngestStatus>>,
    download_callback: Arc<Mutex<Option<DownloadCallback>>>,
    stop_signal: Arc<AtomicBool>,
}

impl Default for LiveIngestManager {
    fn default() -> Self {
        Self::new()
    }
}

impl LiveIngestManager {
    pub fn new() -> Self {
        let config = Arc::new(Mutex::new(LiveIngestConfig::default()));
        let status = Arc::new(Mutex::new(LiveIngestStatus {
            status_message: "Odottaa yhteyttä Seestarin verkkolevyyn (/Volumes/EMMC Images).".to_string(),
            ..Default::default()
        }));
        let download_callback: Arc<Mutex<Option<DownloadCallback>>> = Arc::new(Mutex::new(None));
        let stop_signal = Arc::new(AtomicBool::new(false));

        let cfg_clone = Arc::clone(&config);
        let stat_clone = Arc::clone(&status);
        let cb_clone = Arc::clone(&download_callback);
        let stop_clone = Arc::clone(&stop_signal);

        // Background worker to poll and copy new FITS files as they arrive
        thread::spawn(move || {
            let mut known_files: HashSet<PathBuf> = HashSet::new();
            let mut was_share_connected = false;

            while !stop_clone.load(Ordering::SeqCst) {
                let (enabled, src, dst, auto_stack) = {
                    let cfg = cfg_clone.lock().unwrap();
                    (cfg.enabled, cfg.source_path.clone(), cfg.destination_path.clone(), cfg.auto_live_stack)
                };

                if enabled {
                    // Check if Seestar network share / folder exists
                    let exists = src.exists() && src.is_dir();

                    {
                        let mut st = stat_clone.lock().unwrap();
                        if exists && !was_share_connected && st.downloaded_count > 0 {
                            st.status_message = "✓ Verkkolevy palautui! Jatketaan uusien FITS-ruutujen synkronointia.".to_string();
                        }
                        st.source_found = exists;
                        st.is_active = exists;
                    }

                    if exists {
                        was_share_connected = true;
                        // Scan for .fits / .fit / .fts files in source directory recursively
                        let mut found_fits = Vec::new();
                        scan_directory_for_fits(&src, &mut found_fits);

                        for file in found_fits {
                            if stop_clone.load(Ordering::SeqCst) {
                                break;
                            }

                            if !known_files.contains(&file) {
                                let src_len = fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
                                if src_len == 0 {
                                    // File is still empty / being initialized on the scope, retry on next poll
                                    continue;
                                }

                                // Compute relative path inside source to maintain target folder structure
                                let rel_path = file.strip_prefix(&src).unwrap_or(&file);
                                let target_dest = dst.join(rel_path);

                                if let Some(parent) = target_dest.parent() {
                                    let _ = fs::create_dir_all(parent);
                                }

                                // Check if target file already exists and has the same non-zero size
                                let already_complete = if target_dest.exists() {
                                    let dst_len = fs::metadata(&target_dest).map(|m| m.len()).unwrap_or(0);
                                    dst_len == src_len
                                } else {
                                    false
                                };

                                if already_complete {
                                    known_files.insert(file);
                                } else if let Ok(copied_bytes) = fs::copy(&file, &target_dest) {
                                    if copied_bytes == src_len {
                                        let file_name = file.file_name().unwrap_or_default().to_string_lossy().to_string();
                                        let now_str = chrono::Local::now().format("%H:%M:%S").to_string();

                                        let mut st = stat_clone.lock().unwrap();
                                        st.downloaded_count += 1;
                                        st.last_file_name = file_name.clone();
                                        st.last_sync_time = now_str.clone();
                                        st.status_message = format!("✓ Ladattu uusi ruutu Macille: {} ({})", file_name, now_str);

                                        // Trigger callback (e.g. Live Stacker)
                                        if auto_stack {
                                            let cb_opt = cb_clone.lock().unwrap().clone();
                                            if let Some(cb) = cb_opt {
                                                cb(target_dest.clone());
                                            }
                                        }

                                        known_files.insert(file);
                                    }
                                }
                            }
                        }
                    } else {
                        let mut st = stat_clone.lock().unwrap();
                        if was_share_connected {
                            st.status_message = format!(
                                "⚠ Yhteys verkkolevyyn '{}' katkesi – odotetaan yhteyden palautumista...",
                                src.display()
                            );
                        } else {
                            st.status_message = format!(
                                "Seestarin verkkolevy ei vielä yhdistetty polkuun '{}'. Yhdistä Finderissä 'smb://<IP>/EMMC Images'.",
                                src.display()
                            );
                        }
                    }
                } else {
                    let mut st = stat_clone.lock().unwrap();
                    st.is_active = false;
                    st.status_message = "Automaattinen kuvien lataus on pois päältä.".to_string();
                }

                thread::sleep(Duration::from_secs(2));
            }
        });

        Self {
            config,
            status,
            download_callback,
            stop_signal,
        }
    }

    pub fn set_on_file_downloaded<F>(&self, callback: F)
    where
        F: Fn(PathBuf) + Send + Sync + 'static,
    {
        let mut cb = self.download_callback.lock().unwrap();
        *cb = Some(Arc::new(callback));
    }

    pub fn set_model_source_path(&self, model: crate::telescope::mosaic::TelescopeModel) {
        let mut cfg = self.config.lock().unwrap();
        let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/Shared".to_string());
        match model {
            crate::telescope::mosaic::TelescopeModel::SeestarS50
            | crate::telescope::mosaic::TelescopeModel::SeestarS50Pro
            | crate::telescope::mosaic::TelescopeModel::SeestarS30
            | crate::telescope::mosaic::TelescopeModel::SeestarS30Pro => {
                cfg.source_path = PathBuf::from("/Volumes/EMMC Images/MyWorks");
                cfg.destination_path =
                    PathBuf::from(&home).join("Pictures").join("PhotonForge_Seestar");
            }
            crate::telescope::mosaic::TelescopeModel::Dwarf3
            | crate::telescope::mosaic::TelescopeModel::Dwarf2 => {
                cfg.source_path = PathBuf::from("/Volumes/DWARF/FITS");
                cfg.destination_path =
                    PathBuf::from(&home).join("Pictures").join("PhotonForge_Dwarf");
            }
            crate::telescope::mosaic::TelescopeModel::Vespera2
            | crate::telescope::mosaic::TelescopeModel::VesperaPro => {
                cfg.source_path = PathBuf::from("/Volumes/Vespera");
                cfg.destination_path =
                    PathBuf::from(&home).join("Pictures").join("PhotonForge_Vespera");
            }
            _ => {
                cfg.destination_path =
                    PathBuf::from(&home).join("Pictures").join("PhotonForge_Telescope");
            }
        }
    }

    pub fn open_seestar_smb(seestar_ip: &str) {
        let smb_url = format!("smb://{}/EMMC Images", seestar_ip);
        let _ = std::process::Command::new("open").arg(&smb_url).spawn();
    }

    #[allow(dead_code)]
    pub fn open_share_url(url: &str) {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
}

impl Drop for LiveIngestManager {
    fn drop(&mut self) {
        self.stop_signal.store(true, Ordering::SeqCst);
    }
}

fn scan_directory_for_fits(dir: &Path, results: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan_directory_for_fits(&path, results);
            } else if path.is_file() {
                if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                    let lower = ext.to_lowercase();
                    if lower == "fits" || lower == "fit" || lower == "fts" {
                        results.push(path);
                    }
                }
            }
        }
    }
}
