//! User configuration loaded from `~/.config/bluemon-tui/config.toml`.
//!
//! All fields are optional with sane defaults. Env vars override config file values,
//! and CLI args override everything.

use serde::Deserialize;
use std::path::PathBuf;

/// User-facing configuration.
///
/// API keys are intentionally excluded — use the K key in the TUI (persists to DB)
/// or the OPENAI_API_KEY env var instead.
#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    /// OpenAI model for chat analysis.
    pub openai_model: String,
    /// BLE adapter index (0 = first adapter).
    pub adapter: usize,
    /// Scan cycle duration in seconds.
    pub scan_duration: u64,
    /// Path loss exponent for distance estimation.
    /// 2.0 = free space/outdoor, 3.0 = typical indoor, 4.0 = dense walls.
    pub path_loss_n: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            openai_model: "gpt-5.4-mini".to_string(),
            adapter: 0,
            scan_duration: 3,
            path_loss_n: 3.0,
        }
    }
}

/// Config directory path (`~/.config/bluemon-tui/`).
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("bluemon-tui")
}

/// Load config from `~/.config/bluemon-tui/config.toml`, falling back to defaults.
/// Env vars override config file values where applicable.
pub fn load() -> Config {
    let path = config_dir().join("config.toml");
    let mut cfg = match std::fs::read_to_string(&path) {
        Ok(contents) => toml::from_str::<Config>(&contents).unwrap_or_else(|e| {
            eprintln!("Warning: failed to parse {}: {e}", path.display());
            Config::default()
        }),
        Err(_) => Config::default(),
    };

    // Env vars override config file
    if let Ok(model) = std::env::var("OPENAI_MODEL") {
        if !model.is_empty() {
            cfg.openai_model = model;
        }
    }
    if let Ok(n) = std::env::var("BLE_PATH_LOSS_N") {
        if let Ok(v) = n.parse::<f64>() {
            cfg.path_loss_n = v;
        }
    }

    cfg
}

/// Generate a default config.toml template with all options commented out.
pub fn generate_template() -> String {
    r#"# Bluemon TUI Configuration
# Place this file at ~/.config/bluemon-tui/config.toml

# OpenAI model for chat analysis
# openai_model = "gpt-5.4-mini"

# OpenAI API key: set via K key in the TUI (saved to DB) or OPENAI_API_KEY env var.
# Not stored in this file for security.

# BLE adapter index (0 = first adapter)
# adapter = 0

# Scan cycle duration in seconds
# scan_duration = 3

# Path loss exponent for BLE distance estimation
# 2.0 = free space/outdoor, 3.0 = typical indoor, 4.0 = dense walls
# path_loss_n = 3.0
"#
    .to_string()
}
