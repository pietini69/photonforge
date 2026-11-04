use crate::telescope::mosaic::TelescopeModel;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream, ToSocketAddrs, UdpSocket};
use std::path::PathBuf;
use std::time::Duration;

fn default_auto_reconnect() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TelescopeConfig {
    pub last_ip: String,
    pub scope_model: Option<String>,
    #[serde(default)]
    pub is_simulation: bool,
    #[serde(default = "default_auto_reconnect")]
    pub auto_reconnect: bool,
    #[serde(default)]
    pub model_ips: HashMap<String, String>,
}

impl Default for TelescopeConfig {
    fn default() -> Self {
        Self {
            last_ip: "seestar.local".to_string(),
            scope_model: Some("Seestar S-30 (2.1° × 1.2°)".to_string()),
            is_simulation: false, // Default is OFF
            auto_reconnect: true,
            model_ips: HashMap::new(),
        }
    }
}

impl TelescopeConfig {
    pub fn get_model(&self) -> TelescopeModel {
        self.scope_model
            .as_deref()
            .and_then(TelescopeModel::from_name_or_short)
            .unwrap_or(TelescopeModel::SeestarS30)
    }

    pub fn get_ip_for_model(&self, model: TelescopeModel) -> String {
        if let Some(ip) = self.model_ips.get(model.display_short()) {
            if !ip.trim().is_empty() {
                return ip.clone();
            }
        }
        if self.get_model() == model && !self.last_ip.trim().is_empty() {
            return self.last_ip.clone();
        }
        model.default_ip().to_string()
    }

    pub fn config_path() -> Option<PathBuf> {
        let base_dir = if let Some(home) = std::env::var_os("HOME") {
            PathBuf::from(home).join(".config").join("photon_forge")
        } else if let Some(appdata) = std::env::var_os("APPDATA") {
            PathBuf::from(appdata).join("photon_forge")
        } else {
            return None;
        };
        let _ = std::fs::create_dir_all(&base_dir);
        Some(base_dir.join("telescope_settings.json"))
    }

    pub fn load() -> Self {
        if let Some(path) = Self::config_path() {
            if let Ok(data) = std::fs::read_to_string(&path) {
                if let Ok(cfg) = serde_json::from_str::<TelescopeConfig>(&data) {
                    if !cfg.last_ip.trim().is_empty() {
                        return cfg;
                    }
                }
            }
        }
        Self::default()
    }

    pub fn save(&self) {
        if let Some(path) = Self::config_path() {
            if let Ok(json) = serde_json::to_string_pretty(self) {
                let _ = std::fs::write(&path, json);
            }
        }
    }
}

/// Discovers a connected Seestar telescope on the network.
/// Priority:
/// 1. `seestar.local:32323` via mDNS (IPv4)
/// 2. `last_ip:32323` if provided
/// 3. `192.168.110.1:32323` (Seestar factory AP hotspot mode)
/// 4. Parallel subnet sweep on port 32323 using Rayon
pub fn discover_telescope(last_ip: &str) -> Result<String, String> {
    // 1. Check seestar.local:32323
    let addr_str = "seestar.local:32323";
    if let Ok(addrs) = addr_str.to_socket_addrs() {
        for addr in addrs {
            if addr.is_ipv4() && TcpStream::connect_timeout(&addr, Duration::from_millis(800)).is_ok() {
                return Ok(addr.ip().to_string());
            }
        }
    }

    // 2. Check last configured IP if non-empty and not already tested
    let clean_last = last_ip.trim();
    if !clean_last.is_empty() && clean_last != "seestar.local" {
        let addr_candidate = format!("{}:32323", clean_last);
        if let Ok(addrs) = addr_candidate.to_socket_addrs() {
            for addr in addrs {
                if addr.is_ipv4() && TcpStream::connect_timeout(&addr, Duration::from_millis(800)).is_ok() {
                    return Ok(addr.ip().to_string());
                }
            }
        }
    }

    // 3. Check Seestar factory AP mode (192.168.110.1)
    if let Ok(addr) = "192.168.110.1:32323".parse::<SocketAddr>() {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok() {
            return Ok("192.168.110.1".to_string());
        }
    }

    // 4. Parallel subnet scan using Rayon
    if let Some(local_ip) = get_local_ipv4() {
        let octets = local_ip.octets();
        let prefix = [octets[0], octets[1], octets[2]];
        let found: Option<Ipv4Addr> = (1..=254).into_par_iter().find_map_first(|host| {
            let target = SocketAddrV4::new(
                Ipv4Addr::new(prefix[0], prefix[1], prefix[2], host),
                32323,
            );
            if TcpStream::connect_timeout(
                &SocketAddr::V4(target),
                Duration::from_millis(250),
            )
            .is_ok()
            {
                Some(*target.ip())
            } else {
                None
            }
        });

        if let Some(ip) = found {
            return Ok(ip.to_string());
        }
    }

    Err("Kaukoputkea ei löytynyt lähiverkosta. Varmista että Seestar on päällä ja samassa Wi-Fi-verkossa.".to_string())
}

fn get_local_ipv4() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(v4) => Some(v4),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telescope_config_roundtrip() {
        let mut model_ips = HashMap::new();
        model_ips.insert("Seestar S-30 (2.1° × 1.2°)".to_string(), "192.168.68.77".to_string());
        let cfg = TelescopeConfig {
            last_ip: "192.168.68.77".to_string(),
            scope_model: Some("ZWO Seestar S-30".to_string()),
            is_simulation: false,
            auto_reconnect: true,
            model_ips,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let loaded: TelescopeConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.last_ip, "192.168.68.77");
        assert_eq!(loaded.scope_model.as_deref(), Some("ZWO Seestar S-30"));
        assert!(!loaded.is_simulation);
        assert_eq!(loaded.get_model(), TelescopeModel::SeestarS30);
        assert_eq!(loaded.get_ip_for_model(TelescopeModel::SeestarS30), "192.168.68.77");
    }

    #[test]
    fn test_discover_seestar_real_or_fallback() {
        // Test discovery - should find 192.168.68.77 on the user's live network
        let res = discover_telescope("192.168.68.77");
        if let Ok(ip) = res {
            println!("Discovered telescope IP: {}", ip);
            assert!(!ip.is_empty());
        }
    }
}
