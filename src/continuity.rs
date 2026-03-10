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
fn parse_airpods(payload: &[u8], extended: bool) -> ContinuityData {
    let device_model = u16::from_be_bytes([payload[0], payload[1]]);

    let bat_byte = payload[2];
    let raw_left = (bat_byte >> 4) & 0x0F;
    let raw_right = bat_byte & 0x0F;
    let battery_left = if raw_left <= 10 {
        Some(raw_left)
    } else {
        None
    };
    let battery_right = if raw_right <= 10 {
        Some(raw_right)
    } else {
        None
    };

    let case_charge = payload[3];
    let raw_case = (case_charge >> 4) & 0x0F;
    let battery_case = if raw_case <= 10 {
        Some(raw_case)
    } else {
        None
    };
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
