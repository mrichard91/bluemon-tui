use serde::{Deserialize, Serialize};

/// Parsed Apple Continuity protocol data from manufacturer data (company 0x004C).
///
/// The Continuity protocol uses Type-Length-Value encoding. We parse the first
/// TLV entry and extract structured fields for each known message type.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContinuityData {
    IBeacon {
        uuid: String,
        major: u16,
        minor: u16,
        measured_power: i8,
    },
    AirDrop {
        contact_hash: String,
    },
    HomeKit {
        device_category: u8,
        state: u8,
    },
    AirPods {
        device_model: u16,
        battery_left: Option<u8>,
        battery_right: Option<u8>,
        battery_case: Option<u8>,
        charging_left: bool,
        charging_right: bool,
        charging_case: bool,
        lid_open: bool,
    },
    AirPlay {
        flags: u8,
        config_seed: u8,
    },
    Handoff {
        activity_type: u16,
        payload_hash: String,
    },
    NearbyInfo {
        activity_level: u8,
        wifi_on: bool,
        os_version_hint: u8,
        device_model: u8,
    },
    NearbyAction {
        action_type: u8,
        flags: u8,
    },
    AirPodsExtended {
        device_model: u16,
        battery_left: Option<u8>,
        battery_right: Option<u8>,
        battery_case: Option<u8>,
        charging_left: bool,
        charging_right: bool,
        charging_case: bool,
        lid_open: bool,
    },
    FindMy {
        status: u8,
    },
    Unknown {
        type_byte: u8,
        raw: String,
    },
}

impl ContinuityData {
    /// Parse Apple Continuity protocol data from manufacturer data bytes.
    ///
    /// The data format is TLV: type (1 byte), length (1 byte), value (length bytes).
    /// We parse the first TLV entry. Defensive: never panics on short data.
    pub fn parse(data: &[u8]) -> Option<ContinuityData> {
        if data.len() < 2 {
            return None;
        }
        let type_byte = data[0];
        let length = data[1] as usize;
        let payload = if data.len() >= 2 + length {
            &data[2..2 + length]
        } else {
            &data[2..]
        };

        Some(match type_byte {
            0x02 if payload.len() >= 21 => {
                let uuid = format!(
                    "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
                    payload[0], payload[1], payload[2], payload[3],
                    payload[4], payload[5],
                    payload[6], payload[7],
                    payload[8], payload[9],
                    payload[10], payload[11], payload[12], payload[13], payload[14], payload[15],
                );
                let major = u16::from_be_bytes([payload[16], payload[17]]);
                let minor = u16::from_be_bytes([payload[18], payload[19]]);
                let measured_power = payload[20] as i8;
                ContinuityData::IBeacon {
                    uuid,
                    major,
                    minor,
                    measured_power,
                }
            }

            0x05 if payload.len() >= 2 => ContinuityData::AirDrop {
                contact_hash: format!("{:02X}{:02X}", payload[0], payload[1]),
            },

            0x06 if payload.len() >= 2 => ContinuityData::HomeKit {
                device_category: payload[0],
                state: payload[1],
            },

            0x07 if payload.len() >= 5 => parse_airpods(payload, false),

            0x09 if payload.len() >= 2 => ContinuityData::AirPlay {
                flags: payload[0],
                config_seed: payload[1],
            },

            0x0C if payload.len() >= 4 => {
                let activity_type = u16::from_be_bytes([payload[0], payload[1]]);
                let payload_hash = format!("{:02X}{:02X}", payload[2], payload[3]);
                ContinuityData::Handoff {
                    activity_type,
                    payload_hash,
                }
            }

            0x0F if payload.len() >= 2 => {
                let status_flags = payload[0];
                let activity_level = status_flags & 0x03;
                let wifi_on = (status_flags & 0x04) != 0;
                let os_version_hint = payload[1] >> 4;
                let device_model = payload[1] & 0x0F;
                ContinuityData::NearbyInfo {
                    activity_level,
                    wifi_on,
                    os_version_hint,
                    device_model,
                }
            }

            0x10 if payload.len() >= 2 => ContinuityData::NearbyAction {
                action_type: payload[0],
                flags: payload[1],
            },

            0x12 if payload.len() >= 5 => parse_airpods(payload, true),

            0x19 if !payload.is_empty() => ContinuityData::FindMy {
                status: payload[0],
            },

            _ => {
                let raw = data
                    .iter()
                    .map(|b| format!("{:02X}", b))
                    .collect::<Vec<_>>()
                    .join(" ");
                ContinuityData::Unknown { type_byte, raw }
            }
        })
    }

    /// One-line human-readable summary for TUI display.
    pub fn summary(&self) -> String {
        match self {
            ContinuityData::IBeacon {
                uuid,
                major,
                minor,
                measured_power,
            } => {
                format!("iBeacon {uuid} M:{major} m:{minor} P:{measured_power}")
            }

            ContinuityData::AirDrop { contact_hash } => {
                format!("AirDrop contact:{contact_hash}")
            }

            ContinuityData::HomeKit {
                device_category,
                state,
            } => {
                let cat = homekit_category_name(*device_category);
                format!("HomeKit {cat} state:{state:#04X}")
            }

            ContinuityData::AirPods {
                device_model,
                battery_left,
                battery_right,
                battery_case,
                charging_left,
                charging_right,
                charging_case,
                lid_open,
            }
            | ContinuityData::AirPodsExtended {
                device_model,
                battery_left,
                battery_right,
                battery_case,
                charging_left,
                charging_right,
                charging_case,
                lid_open,
            } => {
                let model = airpods_model_name(*device_model);
                let mut parts = vec![model.to_string()];
                if let Some(b) = battery_left {
                    parts.push(format!("L:{}%", b * 10));
                }
                if let Some(b) = battery_right {
                    parts.push(format!("R:{}%", b * 10));
                }
                if let Some(b) = battery_case {
                    parts.push(format!("C:{}%", b * 10));
                }
                let mut flags = Vec::new();
                if *charging_left {
                    flags.push("L+");
                }
                if *charging_right {
                    flags.push("R+");
                }
                if *charging_case {
                    flags.push("C+");
                }
                if *lid_open {
                    flags.push("lid");
                }
                if !flags.is_empty() {
                    parts.push(format!("[{}]", flags.join("")));
                }
                parts.join(" ")
            }

            ContinuityData::AirPlay {
                flags,
                config_seed,
            } => {
                format!("AirPlay flags:{flags:#04X} seed:{config_seed}")
            }

            ContinuityData::Handoff {
                activity_type,
                payload_hash,
            } => {
                format!("Handoff activity:{activity_type:#06X} hash:{payload_hash}")
            }

            ContinuityData::NearbyInfo {
                activity_level,
                wifi_on,
                os_version_hint,
                device_model,
            } => {
                let activity = match activity_level {
                    0 => "idle",
                    1 => "active",
                    2 => "screen-on",
                    3 => "screen-off",
                    _ => "?",
                };
                let wifi = if *wifi_on { "WiFi" } else { "noWiFi" };
                format!("Nearby {activity} {wifi} os:{os_version_hint} dev:{device_model}")
            }

            ContinuityData::NearbyAction {
                action_type,
                flags,
            } => {
                let action = nearby_action_name(*action_type);
                format!("Action: {action} flags:{flags:#04X}")
            }

            ContinuityData::FindMy { status } => {
                format!("FindMy status:{status:#04X}")
            }

            ContinuityData::Unknown { type_byte, raw } => {
                format!("Apple type:{type_byte:#04X} {raw}")
            }
        }
    }
}

/// Parse AirPods/Beats proximity data from Continuity payload.
///
/// Layout (type 0x07 and 0x12):
///   [0:2] device model (big-endian u16)
///   [2]   battery: left (high nibble), right (low nibble); 0xF = unknown
///   [3]   case battery (high nibble, 0xF = unknown), charging flags (low nibble)
///   [4]   lid open (bit 0)
/// Decode a 4-bit battery nibble (0–10 valid, >10 means unknown).
fn decode_battery_nibble(nibble: u8) -> Option<u8> {
    if nibble <= 10 { Some(nibble) } else { None }
}

fn parse_airpods(payload: &[u8], extended: bool) -> ContinuityData {
    let device_model = u16::from_be_bytes([payload[0], payload[1]]);

    let bat_byte = payload[2];
    let battery_left = decode_battery_nibble((bat_byte >> 4) & 0x0F);
    let battery_right = decode_battery_nibble(bat_byte & 0x0F);

    let case_charge = payload[3];
    let battery_case = decode_battery_nibble((case_charge >> 4) & 0x0F);
    let charge_flags = case_charge & 0x0F;
    let charging_left = (charge_flags & 0x04) != 0;
    let charging_right = (charge_flags & 0x02) != 0;
    let charging_case = (charge_flags & 0x01) != 0;

    let lid_open = if payload.len() > 4 {
        (payload[4] & 0x01) != 0
    } else {
        false
    };

    if extended {
        ContinuityData::AirPodsExtended {
            device_model,
            battery_left,
            battery_right,
            battery_case,
            charging_left,
            charging_right,
            charging_case,
            lid_open,
        }
    } else {
        ContinuityData::AirPods {
            device_model,
            battery_left,
            battery_right,
            battery_case,
            charging_left,
            charging_right,
            charging_case,
            lid_open,
        }
    }
}

fn airpods_model_name(model: u16) -> &'static str {
    match model {
        0x0220 => "AirPods",
        0x0F20 => "AirPods 2",
        0x1320 => "AirPods 3",
        0x0E20 => "AirPods Pro",
        0x1420 => "AirPods Pro 2",
        0x0A20 => "AirPods Max",
        0x0520 => "Beats X",
        0x0620 => "Beats Solo3",
        0x0920 => "Beats Studio3",
        0x1020 => "Beats Flex",
        0x1120 => "Beats Solo Pro",
        0x0320 => "Powerbeats3",
        0x0B20 => "Powerbeats Pro",
        _ => "AirPods/Beats",
    }
}

fn homekit_category_name(cat: u8) -> &'static str {
    match cat {
        1 => "Other",
        2 => "Bridge",
        3 => "Fan",
        4 => "Garage",
        5 => "Light",
        6 => "Lock",
        7 => "Outlet",
        8 => "Switch",
        9 => "Thermostat",
        10 => "Sensor",
        11 => "Door",
        12 => "Window",
        13 => "WindowCover",
        14 => "ProgramSwitch",
        17 => "IP Camera",
        28 => "Sprinkler",
        29 => "Faucet",
        _ => "Unknown",
    }
}

/// Look up a known iBeacon UUID and return its vendor/product name.
pub fn ibeacon_uuid_name(uuid: &str) -> Option<&'static str> {
    match uuid {
        "b9407f30-f5f8-466e-aff9-25556b57fe6d" => Some("Estimote"),
        "f7826da6-4fa2-4e98-8024-bc5b71e0893e" => Some("Kontakt.io"),
        "2f234454-cf6d-4a0f-adf2-f4911ba9ffa6" => Some("Radius Networks"),
        "e2c56db5-dffb-48d2-b060-d0f5a71096e0" => Some("Apple iBeacon"),
        "fda50693-a4e2-4fb1-afcf-c0f0f63cb0e8" => Some("Tile"),
        "74278bda-b644-4520-8f0c-720eaf059935" => Some("Chipolo"),
        _ => None,
    }
}

fn nearby_action_name(action: u8) -> &'static str {
    match action {
        0x01 => "Setup New Device",
        0x04 => "Setup Transfer",
        0x05 => "Setup Proximity",
        0x06 => "HomeKit Setup",
        0x07 => "Repair",
        0x08 => "Setup Connect",
        0x09 => "WiFi Password",
        0x0A => "iOS Setup",
        0x0B => "Handoff",
        0x0C => "WiFi Join",
        0x0D => "Tethering",
        0x0E => "HomePod Setup",
        _ => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── ContinuityData::parse ────────────────────────────────────────────

    #[test]
    fn parse_ibeacon() {
        // type=0x02, length=21, then 16-byte UUID + major(2) + minor(2) + power(1)
        let mut data = vec![0x02, 21];
        // UUID: all zeros
        data.extend_from_slice(&[0x00; 16]);
        // major=1, minor=2
        data.extend_from_slice(&[0x00, 0x01, 0x00, 0x02]);
        // measured power = -59 (0xC5 as i8)
        data.push(0xC5);
        let parsed = ContinuityData::parse(&data).unwrap();
        match parsed {
            ContinuityData::IBeacon { major, minor, measured_power, .. } => {
                assert_eq!(major, 1);
                assert_eq!(minor, 2);
                assert_eq!(measured_power, -59);
            }
            other => panic!("Expected IBeacon, got {:?}", other),
        }
    }

    #[test]
    fn parse_airpods_type_07() {
        // type=0x07, length=5, model=0x0220, battery=0x8A, case+charge=0x50, lid=0x01
        let data = vec![0x07, 0x05, 0x02, 0x20, 0x8A, 0x50, 0x01];
        let parsed = ContinuityData::parse(&data).unwrap();
        match parsed {
            ContinuityData::AirPods { device_model, battery_left, battery_right, lid_open, .. } => {
                assert_eq!(device_model, 0x0220);
                assert_eq!(battery_left, Some(8));
                assert_eq!(battery_right, Some(10));
                assert!(lid_open);
            }
            other => panic!("Expected AirPods, got {:?}", other),
        }
    }

    #[test]
    fn parse_airdrop() {
        let data = vec![0x05, 0x02, 0xAB, 0xCD];
        let parsed = ContinuityData::parse(&data).unwrap();
        match parsed {
            ContinuityData::AirDrop { contact_hash } => {
                assert_eq!(contact_hash, "ABCD");
            }
            other => panic!("Expected AirDrop, got {:?}", other),
        }
    }

    #[test]
    fn parse_handoff() {
        let data = vec![0x0C, 0x04, 0x01, 0x23, 0xAA, 0xBB];
        let parsed = ContinuityData::parse(&data).unwrap();
        match parsed {
            ContinuityData::Handoff { activity_type, payload_hash } => {
                assert_eq!(activity_type, 0x0123);
                assert_eq!(payload_hash, "AABB");
            }
            other => panic!("Expected Handoff, got {:?}", other),
        }
    }

    #[test]
    fn parse_nearby_info() {
        // status_flags=0x06 → activity=2 (screen-on), wifi=true; version byte=0x53
        let data = vec![0x0F, 0x02, 0x06, 0x53];
        let parsed = ContinuityData::parse(&data).unwrap();
        match parsed {
            ContinuityData::NearbyInfo { activity_level, wifi_on, os_version_hint, device_model } => {
                assert_eq!(activity_level, 2);
                assert!(wifi_on);
                assert_eq!(os_version_hint, 5);
                assert_eq!(device_model, 3);
            }
            other => panic!("Expected NearbyInfo, got {:?}", other),
        }
    }

    #[test]
    fn parse_nearby_action() {
        let data = vec![0x10, 0x02, 0x09, 0x01];
        let parsed = ContinuityData::parse(&data).unwrap();
        match parsed {
            ContinuityData::NearbyAction { action_type, flags } => {
                assert_eq!(action_type, 0x09);
                assert_eq!(flags, 0x01);
            }
            other => panic!("Expected NearbyAction, got {:?}", other),
        }
    }

    #[test]
    fn parse_findmy() {
        let data = vec![0x19, 0x01, 0x42];
        let parsed = ContinuityData::parse(&data).unwrap();
        match parsed {
            ContinuityData::FindMy { status } => assert_eq!(status, 0x42),
            other => panic!("Expected FindMy, got {:?}", other),
        }
    }

    #[test]
    fn parse_unknown_type() {
        let data = vec![0xFF, 0x02, 0xAA, 0xBB];
        let parsed = ContinuityData::parse(&data).unwrap();
        match parsed {
            ContinuityData::Unknown { type_byte, .. } => assert_eq!(type_byte, 0xFF),
            other => panic!("Expected Unknown, got {:?}", other),
        }
    }

    #[test]
    fn parse_too_short_returns_none() {
        assert!(ContinuityData::parse(&[]).is_none());
        assert!(ContinuityData::parse(&[0x07]).is_none());
    }

    // ── summary ──────────────────────────────────────────────────────────

    #[test]
    fn summary_airdrop() {
        let d = ContinuityData::AirDrop { contact_hash: "ABCD".into() };
        assert_eq!(d.summary(), "AirDrop contact:ABCD");
    }

    #[test]
    fn summary_findmy() {
        let d = ContinuityData::FindMy { status: 0x01 };
        assert!(d.summary().contains("FindMy"));
    }

    #[test]
    fn summary_nearby_action() {
        let d = ContinuityData::NearbyAction { action_type: 0x09, flags: 0x00 };
        assert!(d.summary().contains("WiFi Password"));
    }

    // ── airpods_model_name ───────────────────────────────────────────────

    #[test]
    fn airpods_known_model() {
        assert_eq!(airpods_model_name(0x0220), "AirPods");
        assert_eq!(airpods_model_name(0x1420), "AirPods Pro 2");
    }

    #[test]
    fn airpods_unknown_model() {
        assert_eq!(airpods_model_name(0xFFFF), "AirPods/Beats");
    }

    // ── homekit_category_name ────────────────────────────────────────────

    #[test]
    fn homekit_known_categories() {
        assert_eq!(homekit_category_name(5), "Light");
        assert_eq!(homekit_category_name(6), "Lock");
        assert_eq!(homekit_category_name(9), "Thermostat");
    }

    #[test]
    fn homekit_unknown_category() {
        assert_eq!(homekit_category_name(200), "Unknown");
    }

    // ── ibeacon_uuid_name ────────────────────────────────────────────────

    #[test]
    fn ibeacon_known_uuid() {
        assert_eq!(
            ibeacon_uuid_name("b9407f30-f5f8-466e-aff9-25556b57fe6d"),
            Some("Estimote")
        );
    }

    #[test]
    fn ibeacon_unknown_uuid() {
        assert_eq!(ibeacon_uuid_name("00000000-0000-0000-0000-000000000000"), None);
    }

    // ── nearby_action_name ───────────────────────────────────────────────

    #[test]
    fn nearby_action_known() {
        assert_eq!(nearby_action_name(0x01), "Setup New Device");
        assert_eq!(nearby_action_name(0x09), "WiFi Password");
    }

    #[test]
    fn nearby_action_unknown() {
        assert_eq!(nearby_action_name(0xFF), "Unknown");
    }
}
