/// BLE Service UUID name resolution.
///
/// Maps 8-char hex prefixes to human-readable service names.
/// Covers standard GATT services, classic Bluetooth profiles,
/// LE Audio, and common vendor-specific UUIDs.
///
/// Users can override/extend via `~/.config/bluemon-tui/service_uuids.toml`.

use std::collections::HashMap;
use std::sync::OnceLock;

static USER_DB: OnceLock<HashMap<String, String>> = OnceLock::new();

const SERVICE_UUIDS: &[(&str, &str)] = &[
    // ── Standard GATT Services ──────────────────────────────────────────
    ("00001800", "Generic Access"),
    ("00001801", "Generic Attribute"),
    ("00001802", "Immediate Alert"),
    ("00001803", "Link Loss"),
    ("00001804", "Tx Power"),
    ("00001805", "Current Time"),
    ("00001806", "Reference Time Update"),
    ("00001807", "Next DST Change"),
    ("00001808", "Glucose"),
    ("00001809", "Health Thermometer"),
    ("0000180a", "Device Information"),
    ("0000180d", "Heart Rate"),
    ("0000180e", "Phone Alert Status"),
    ("0000180f", "Battery"),
    ("00001810", "Blood Pressure"),
    ("00001811", "Alert Notification"),
    ("00001812", "Human Interface Device"),
    ("00001813", "Scan Parameters"),
    ("00001814", "Running Speed & Cadence"),
    ("00001815", "Automation IO"),
    ("00001816", "Cycling Speed & Cadence"),
    ("00001818", "Cycling Power"),
    ("00001819", "Location & Navigation"),
    ("0000181a", "Environmental Sensing"),
    ("0000181b", "Body Composition"),
    ("0000181c", "User Data"),
    ("0000181d", "Weight Scale"),
    ("0000181e", "Bond Management"),
    ("0000181f", "Continuous Glucose"),
    ("00001820", "Internet Protocol Support"),
    ("00001821", "Indoor Positioning"),
    ("00001822", "Pulse Oximeter"),
    ("00001823", "HTTP Proxy"),
    ("00001824", "Transport Discovery"),
    ("00001825", "Object Transfer"),
    ("00001826", "Fitness Machine"),
    ("00001827", "Mesh Provisioning"),
    ("00001828", "Mesh Proxy"),
    ("00001829", "Reconnection Configuration"),
    // ── Classic Bluetooth Profiles (SDP UUIDs) ──────────────────────────
    ("00001101", "Serial Port"),
    ("00001103", "Dialup Networking"),
    ("00001104", "IrMC Sync"),
    ("00001105", "OBEX Object Push"),
    ("00001106", "OBEX File Transfer"),
    ("00001108", "Headset"),
    ("0000110a", "A2DP Source"),
    ("0000110b", "A2DP Sink"),
    ("0000110c", "AVRCP Target"),
    ("0000110d", "Advanced Audio"),
    ("0000110e", "AVRCP Controller"),
    ("00001112", "Headset AG"),
    ("00001115", "PAN PANU"),
    ("00001116", "PAN NAP"),
    ("00001117", "PAN GN"),
    ("00001118", "Direct Printing"),
    ("00001119", "Reference Printing"),
    ("0000111e", "Handsfree"),
    ("0000111f", "Handsfree AG"),
    ("00001124", "HID"),
    ("0000112d", "SIM Access"),
    ("0000112f", "Phonebook Access (PCE)"),
    ("00001130", "Phonebook Access (PSE)"),
    ("00001132", "Message Access (MAS)"),
    ("00001133", "Message Access (MNS)"),
    ("00001200", "PnP Information"),
    ("00001203", "Generic Audio"),
    // ── LE Audio (Bluetooth 5.2+) ───────────────────────────────────────
    ("0000184e", "Audio Stream Control"),
    ("0000184f", "Broadcast Audio Scan"),
    ("00001850", "Published Audio Capabilities"),
    ("00001851", "Basic Audio Profile"),
    ("00001852", "Broadcast Audio Announcement"),
    ("00001853", "Common Audio"),
    ("00001854", "Hearing Access"),
    ("00001855", "TMAS"),
    ("00001856", "Public Broadcast Announcement"),
    // ── Apple ────────────────────────────────────────────────────────────
    ("d0611e78", "Apple Continuity"),
    ("7905f431", "Apple ANCS"),
    ("89d3502b", "Apple Media Service"),
    ("9fa480e0", "Apple AirDrop"),
    ("0000fd6f", "Exposure Notification"),
    // ── Google / Android ────────────────────────────────────────────────
    ("0000fe9f", "Google Fast Pair"),
    ("0000fe2c", "Google Nearby"),
    ("0000feaa", "Eddystone"),
    // ── Smart Home / IoT ────────────────────────────────────────────────
    ("0000fef5", "Philips Hue"),
    ("0000fee7", "Tencent IoT"),
    ("0000feab", "Nokia Beacon"),
    ("0000fff0", "Xiaomi Mi"),
    ("0000fee0", "Xiaomi Mi Band"),
    ("cba20d00", "SwitchBot"),
    ("0000ffd0", "Tuya BLE"),
    ("0000fff9", "FIDO2 / WebAuthn"),
    // ── Trackers ────────────────────────────────────────────────────────
    ("0000feed", "Tile"),
    ("0000feec", "Tile (alt)"),
    ("0000febe", "Bose"),
    ("0000feea", "Swirl Networks"),
    // ── Matter / Thread ─────────────────────────────────────────────────
    ("0000fff6", "Matter / Thread"),
    // ── Microsoft ───────────────────────────────────────────────────────
    ("0000fd6e", "Microsoft Swift Pair"),
];

/// Load user UUID overrides from `~/.config/bluemon-tui/service_uuids.toml`.
///
/// No-op if the file doesn't exist. Called once at startup.
pub fn load_user_db() {
    let map = match dirs::config_dir() {
        Some(dir) => {
            let path = dir.join("bluemon-tui").join("service_uuids.toml");
            if path.exists() {
                match std::fs::read_to_string(&path) {
                    Ok(contents) => parse_toml(&contents),
                    Err(_) => HashMap::new(),
                }
            } else {
                HashMap::new()
            }
        }
        None => HashMap::new(),
    };
    let _ = USER_DB.set(map);
}

/// Parse a TOML string into a normalized UUID map.
fn parse_toml(contents: &str) -> HashMap<String, String> {
    #[derive(serde::Deserialize)]
    struct UuidFile {
        #[serde(default)]
        uuids: HashMap<String, String>,
    }

    let Ok(file) = toml::from_str::<UuidFile>(contents) else {
        return HashMap::new();
    };

    file.uuids
        .into_iter()
        .map(|(k, v)| {
            let normalized = k.to_ascii_lowercase();
            // Accept 4-char short UUIDs → pad to 8
            let key = if normalized.len() == 4 {
                format!("0000{normalized}")
            } else {
                normalized
            };
            (key, v)
        })
        .collect()
}

/// Resolve a service UUID to its human-readable name.
///
/// Checks user overrides first, then falls back to built-in database.
/// Accepts full UUIDs (`0000180a-0000-1000-8000-00805f9b34fb`)
/// or short forms (`0000180a`, `180a`). Matching is case-insensitive
/// on the first 8 hex characters.
pub fn resolve(uuid: &str) -> Option<String> {
    let normalized = uuid.to_ascii_lowercase().replace('-', "");
    let prefix = if normalized.len() >= 8 {
        &normalized[..8]
    } else if normalized.len() == 4 {
        // Short 16-bit UUID: pad to 0000xxxx
        let padded = format!("0000{normalized}");
        // Check user DB first
        if let Some(db) = USER_DB.get() {
            if let Some(name) = db.get(&padded) {
                return Some(name.clone());
            }
        }
        return SERVICE_UUIDS
            .iter()
            .find(|(p, _)| p[4..] == normalized)
            .map(|(_, name)| name.to_string());
    } else {
        return None;
    };

    // Check user DB first
    if let Some(db) = USER_DB.get() {
        if let Some(name) = db.get(prefix) {
            return Some(name.clone());
        }
    }

    SERVICE_UUIDS
        .iter()
        .find(|(p, _)| *p == prefix)
        .map(|(_, name)| name.to_string())
}

/// Resolve a list of UUIDs to a compact comma-separated string of service names.
///
/// Deduplicates names and joins with ", ".
pub fn resolve_compact(uuids: &[String]) -> String {
    let mut seen = std::collections::HashSet::new();
    let mut names: Vec<String> = Vec::new();
    for uuid in uuids {
        if let Some(name) = resolve(uuid) {
            if seen.insert(name.clone()) {
                names.push(name);
            }
        }
    }
    names.join(", ")
}

/// Format a UUID with its resolved name appended, if known.
///
/// Returns `"uuid (Name)"` for known UUIDs, or just `"uuid"` for unknown ones.
pub fn format_uuid(uuid: &str) -> String {
    match resolve(uuid) {
        Some(name) => format!("{uuid} ({name})"),
        None => uuid.to_string(),
    }
}

/// Generate a template TOML file with all built-in UUIDs as comments.
pub fn generate_template() -> String {
    let mut out = String::from(
        "# Bluemon TUI - User Service UUID Overrides\n\
         #\n\
         # Keys: 8-char hex prefix (e.g. \"0000fe9f\") or 4-char short (e.g. \"fe9f\")\n\
         # User entries are checked before built-in database.\n\
         #\n\
         # Built-in UUIDs (for reference):\n",
    );
    for (prefix, name) in SERVICE_UUIDS {
        out.push_str(&format!("# {prefix} = \"{name}\"\n"));
    }
    out.push_str("\n[uuids]\n");
    out
}
