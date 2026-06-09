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
    /// Reasoning effort level for chat: "low", "medium", or "high".
    pub reasoning_effort: String,
    /// BLE adapter index (0 = first adapter).
    pub adapter: usize,
    /// Scan cycle duration in seconds.
    pub scan_duration: u64,
    /// Path loss exponent for distance estimation.
    /// 2.0 = free space/outdoor, 3.0 = typical indoor, 4.0 = dense walls.
    pub path_loss_n: f64,
    /// Optional MQTT publisher for raw observations.
    pub mqtt: MqttConfig,
}

/// MQTT publisher configuration for forwarding raw observations to a broker.
#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct MqttConfig {
    /// Enable MQTT publishing of raw observations.
    pub enabled: bool,
    /// Broker hostname or IP.
    pub host: String,
    /// Broker TCP port.
    pub port: u16,
    /// Optional username for authenticated brokers.
    pub username: Option<String>,
    /// Optional password for authenticated brokers.
    pub password: Option<String>,
    /// Optional explicit MQTT client ID.
    pub client_id: Option<String>,
    /// Root topic prefix.
    pub topic_prefix: String,
    /// Logical channel / stream name.
    #[serde(alias = "chan_name")]
    pub channel_name: String,
    /// Collector / sensor name.
    pub sensor_name: String,
    /// Optional site / location label.
    pub site_name: Option<String>,
    /// MQTT keep-alive interval.
    pub keep_alive_secs: u64,
    /// MQTT QoS level: 0, 1, or 2.
    pub qos: u8,
    /// Publish retained messages.
    pub retain: bool,
}

impl Default for MqttConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            host: "127.0.0.1".to_string(),
            port: 1883,
            username: None,
            password: None,
            client_id: None,
            topic_prefix: "bluemon".to_string(),
            channel_name: "default".to_string(),
            sensor_name: "bluemon-tui".to_string(),
            site_name: None,
            keep_alive_secs: 30,
            qos: 0,
            retain: false,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            openai_model: "gpt-5.4-mini".to_string(),
            reasoning_effort: "high".to_string(),
            adapter: 0,
            scan_duration: 3,
            path_loss_n: 3.0,
            mqtt: MqttConfig::default(),
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
    if let Some(enabled) = env_bool("BLUEMON_MQTT_ENABLED") {
        cfg.mqtt.enabled = enabled;
    }
    if let Some(host) = env_nonempty_string("BLUEMON_MQTT_HOST") {
        cfg.mqtt.host = host;
    }
    if let Some(port) = env_parse::<u16>("BLUEMON_MQTT_PORT") {
        cfg.mqtt.port = port;
    }
    if let Some(value) = env_optional_string("BLUEMON_MQTT_USERNAME") {
        cfg.mqtt.username = value;
    }
    if let Some(value) = env_optional_string("BLUEMON_MQTT_PASSWORD") {
        cfg.mqtt.password = value;
    }
    if let Some(value) = env_optional_string("BLUEMON_MQTT_CLIENT_ID") {
        cfg.mqtt.client_id = value;
    }
    if let Some(prefix) = env_nonempty_string("BLUEMON_MQTT_TOPIC_PREFIX") {
        cfg.mqtt.topic_prefix = prefix;
    }
    if let Some(channel) = env_nonempty_string("BLUEMON_MQTT_CHANNEL_NAME") {
        cfg.mqtt.channel_name = channel;
    }
    if let Some(sensor) = env_nonempty_string("BLUEMON_MQTT_SENSOR_NAME") {
        cfg.mqtt.sensor_name = sensor;
    }
    if let Some(value) = env_optional_string("BLUEMON_MQTT_SITE_NAME") {
        cfg.mqtt.site_name = value;
    }
    if let Some(keep_alive_secs) = env_parse::<u64>("BLUEMON_MQTT_KEEP_ALIVE_SECS") {
        cfg.mqtt.keep_alive_secs = keep_alive_secs;
    }
    if let Some(qos) = env_parse::<u8>("BLUEMON_MQTT_QOS") {
        cfg.mqtt.qos = qos;
    }
    if let Some(retain) = env_bool("BLUEMON_MQTT_RETAIN") {
        cfg.mqtt.retain = retain;
    }

    cfg
}

fn env_nonempty_string(key: &str) -> Option<String> {
    std::env::var(key).ok().and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn env_optional_string(key: &str) -> Option<Option<String>> {
    std::env::var(key).ok().map(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn env_parse<T: std::str::FromStr>(key: &str) -> Option<T> {
    std::env::var(key).ok()?.trim().parse::<T>().ok()
}

fn env_bool(key: &str) -> Option<bool> {
    let value = std::env::var(key).ok()?;
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// Generate a default config.toml template with all options commented out.
pub fn generate_template() -> String {
    r#"# Bluemon TUI Configuration
# Place this file at ~/.config/bluemon-tui/config.toml

# OpenAI model for chat analysis
# openai_model = "gpt-5.4-mini"

# Reasoning effort level: "low", "medium", or "high"
# reasoning_effort = "high"

# OpenAI API key: set via K key in the TUI (saved to DB) or OPENAI_API_KEY env var.
# Not stored in this file for security.

# BLE adapter index (0 = first adapter)
# adapter = 0

# Scan cycle duration in seconds
# scan_duration = 3

# Path loss exponent for BLE distance estimation
# 2.0 = free space/outdoor, 3.0 = typical indoor, 4.0 = dense walls
# path_loss_n = 3.0

# MQTT broker output for raw, factual observations
# Messages publish to: <topic_prefix>/<channel_name>/<sensor_name>/observations
# [mqtt]
# enabled = true
# host = "127.0.0.1"
# port = 1883
# username = "collector"
# password = "secret"
# client_id = "bluemon-office-01"
# topic_prefix = "bluemon"
# channel_name = "office"
# sensor_name = "collector-01"
# site_name = "hq"
# keep_alive_secs = 30
# qos = 0
# retain = false
"#
    .to_string()
}
