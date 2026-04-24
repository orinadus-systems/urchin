use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub vault_root: PathBuf,
    pub journal_path: PathBuf,
    pub cache_path: PathBuf,
    pub intake_port: u16,
    pub remote_host: Option<String>,
}

/// The on-disk TOML representation — all fields optional so partial files work.
#[derive(Debug, Deserialize, Default)]
struct FileConfig {
    vault_root: Option<PathBuf>,
    journal_path: Option<PathBuf>,
    cache_path: Option<PathBuf>,
    intake_port: Option<u16>,
    remote_host: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        let data_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
            .join("urchin");

        Self {
            vault_root: dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("~"))
                .join("brain"),
            journal_path: data_dir.join("journal").join("events.jsonl"),
            cache_path: data_dir.join("event-cache.jsonl"),
            intake_port: 18799,
            remote_host: None,
        }
    }
}

impl Config {
    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("urchin")
            .join("config.toml")
    }

    pub fn load() -> Self {
        let mut cfg = Self::default();

        // Layer 1: config file
        let config_path = Self::config_path();
        if config_path.exists() {
            if let Ok(raw) = std::fs::read_to_string(&config_path) {
                if let Ok(file_cfg) = toml::from_str::<FileConfig>(&raw) {
                    if let Some(v) = file_cfg.vault_root    { cfg.vault_root    = v; }
                    if let Some(v) = file_cfg.journal_path  { cfg.journal_path  = v; }
                    if let Some(v) = file_cfg.cache_path    { cfg.cache_path    = v; }
                    if let Some(v) = file_cfg.intake_port   { cfg.intake_port   = v; }
                    if let Some(v) = file_cfg.remote_host   { cfg.remote_host   = Some(v); }
                }
            }
        }

        // Layer 2: env var overrides
        if let Ok(v) = std::env::var("URCHIN_VAULT_ROOT")   { cfg.vault_root   = PathBuf::from(v); }
        if let Ok(v) = std::env::var("URCHIN_JOURNAL_PATH") { cfg.journal_path = PathBuf::from(v); }
        if let Ok(v) = std::env::var("URCHIN_INTAKE_PORT")  {
            cfg.intake_port = v.parse().unwrap_or(18799);
        }

        cfg
    }
}
