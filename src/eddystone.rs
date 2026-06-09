//! Eddystone beacon protocol parser.
//!
//! Eddystone payloads ride in BLE service data for service UUID 0xFEAA.
//! The first byte is a frame type:
//!   0x00 = UID  — namespace + instance identifier
//!   0x10 = URL  — compressed URL
//!   0x20 = TLM  — telemetry (battery voltage, temperature, advert count, uptime)
//!   0x30 = EID  — ephemeral identifier (rotating)
//!
//! See https://github.com/google/eddystone for the spec.

use serde::{Deserialize, Serialize};

/// Parsed Eddystone frame.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "frame")]
pub enum Eddystone {
    /// 10-byte namespace + 6-byte instance ID, plus 1-byte calibrated tx power.
    Uid {
        tx_power: i8,
        namespace: String,
        instance: String,
    },
    /// Compressed URL prefix + suffix bytes.
    Url { tx_power: i8, url: String },
    /// Unencrypted telemetry: battery voltage (mV), temperature (°C),
    /// advertising packet count, uptime (deciseconds → seconds).
    Tlm {
        battery_mv: u16,
        temperature_c: Option<f32>,
        advertising_count: u32,
        uptime_secs: u32,
    },
    /// Ephemeral 8-byte rotating identifier.
    Eid { tx_power: i8, eid: String },
}

impl Eddystone {
    /// Parse an Eddystone service-data payload. Returns None for unknown frame
    /// types or short data.
    pub fn parse(data: &[u8]) -> Option<Eddystone> {
        let frame = *data.first()?;
        match frame {
            0x00 if data.len() >= 18 => Some(Eddystone::Uid {
                tx_power: data[1] as i8,
                namespace: hex_string(&data[2..12]),
                instance: hex_string(&data[12..18]),
            }),
            0x10 if data.len() >= 4 => Some(Eddystone::Url {
                tx_power: data[1] as i8,
                url: decode_url(data[2], &data[3..]),
            }),
            0x20 if data.len() >= 14 => {
                // Layout: version (1) | battery_mv (2 BE) | temp (2 fixed-point 8.8)
                //       | adv_count (4 BE) | uptime (4 BE, units of 100 ms)
                let battery_mv = u16::from_be_bytes([data[2], data[3]]);
                let temp_raw = i16::from_be_bytes([data[4], data[5]]);
                // 0x8000 sentinel = unsupported temperature reading.
                let temperature_c = if temp_raw == i16::MIN {
                    None
                } else {
                    Some(temp_raw as f32 / 256.0)
                };
                let advertising_count =
                    u32::from_be_bytes([data[6], data[7], data[8], data[9]]);
                let uptime_decisec =
                    u32::from_be_bytes([data[10], data[11], data[12], data[13]]);
                Some(Eddystone::Tlm {
                    battery_mv,
                    temperature_c,
                    advertising_count,
                    uptime_secs: uptime_decisec / 10,
                })
            }
            0x30 if data.len() >= 10 => Some(Eddystone::Eid {
                tx_power: data[1] as i8,
                eid: hex_string(&data[2..10]),
            }),
            _ => None,
        }
    }

    /// Compact one-line summary suitable for the detail view.
    pub fn summary(&self) -> String {
        match self {
            Eddystone::Uid {
                namespace,
                instance,
                tx_power,
            } => {
                format!("Eddystone-UID ns:{namespace} id:{instance} tx:{tx_power}")
            }
            Eddystone::Url { url, tx_power } => {
                format!("Eddystone-URL {url} tx:{tx_power}")
            }
            Eddystone::Tlm {
                battery_mv,
                temperature_c,
                advertising_count,
                uptime_secs,
            } => {
                let temp = temperature_c
                    .map(|t| format!("{t:.1}°C"))
                    .unwrap_or_else(|| "?°C".into());
                format!(
                    "Eddystone-TLM batt:{battery_mv}mV {temp} adv:{advertising_count} up:{uptime_secs}s"
                )
            }
            Eddystone::Eid { eid, tx_power } => {
                format!("Eddystone-EID {eid} tx:{tx_power}")
            }
        }
    }
}

fn hex_string(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>()
}

/// Decode an Eddystone-URL: a 1-byte scheme prefix + suffix bytes that may
/// contain compressed top-level-domain codes. See the URL frame spec.
fn decode_url(scheme: u8, body: &[u8]) -> String {
    let prefix = match scheme {
        0 => "http://www.",
        1 => "https://www.",
        2 => "http://",
        3 => "https://",
        _ => "",
    };
    let mut out = String::from(prefix);
    for &b in body {
        match b {
            0 => out.push_str(".com/"),
            1 => out.push_str(".org/"),
            2 => out.push_str(".edu/"),
            3 => out.push_str(".net/"),
            4 => out.push_str(".info/"),
            5 => out.push_str(".biz/"),
            6 => out.push_str(".gov/"),
            7 => out.push_str(".com"),
            8 => out.push_str(".org"),
            9 => out.push_str(".edu"),
            10 => out.push_str(".net"),
            11 => out.push_str(".info"),
            12 => out.push_str(".biz"),
            13 => out.push_str(".gov"),
            32..=126 => out.push(b as char),
            _ => out.push('?'),
        }
    }
    out
}

/// Service UUID prefix that carries Eddystone payloads.
pub fn is_eddystone_uuid(uuid: &str) -> bool {
    let normalized = uuid.to_ascii_lowercase().replace('-', "");
    normalized.starts_with("0000feaa")
}

/// Try to parse an Eddystone frame from a service_data map.
pub fn from_service_data(
    service_data: &std::collections::HashMap<String, Vec<u8>>,
) -> Option<Eddystone> {
    service_data
        .iter()
        .find(|(uuid, _)| is_eddystone_uuid(uuid))
        .and_then(|(_, payload)| Eddystone::parse(payload))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn parse_uid_frame() {
        // frame=0x00, tx=-12 (0xF4), namespace 10 bytes, instance 6 bytes
        let mut data = vec![0x00, 0xF4];
        data.extend_from_slice(&[0xAA; 10]);
        data.extend_from_slice(&[0xBB; 6]);
        match Eddystone::parse(&data) {
            Some(Eddystone::Uid {
                tx_power,
                namespace,
                instance,
            }) => {
                assert_eq!(tx_power, -12);
                assert_eq!(namespace, "aaaaaaaaaaaaaaaaaaaa");
                assert_eq!(instance, "bbbbbbbbbbbb");
            }
            other => panic!("Expected UID, got {:?}", other),
        }
    }

    #[test]
    fn parse_url_frame() {
        // frame=0x10, tx=-20 (0xEC), scheme=3 (https://), body="example" + 0x07 (.com)
        let mut data = vec![0x10, 0xEC, 0x03];
        data.extend_from_slice(b"example");
        data.push(0x07);
        match Eddystone::parse(&data) {
            Some(Eddystone::Url { url, .. }) => assert_eq!(url, "https://example.com"),
            other => panic!("Expected URL, got {:?}", other),
        }
    }

    #[test]
    fn parse_tlm_frame() {
        // frame=0x20, version=0, battery=3000mV (0x0BB8), temp=23.5°C (0x1780),
        // adv_count=42, uptime=600 deciseconds = 60s
        let data = vec![
            0x20, 0x00, 0x0B, 0xB8, 0x17, 0x80, 0x00, 0x00, 0x00, 0x2A, 0x00, 0x00,
            0x02, 0x58,
        ];
        match Eddystone::parse(&data) {
            Some(Eddystone::Tlm {
                battery_mv,
                temperature_c,
                advertising_count,
                uptime_secs,
            }) => {
                assert_eq!(battery_mv, 3000);
                assert!((temperature_c.unwrap() - 23.5).abs() < 0.01);
                assert_eq!(advertising_count, 42);
                assert_eq!(uptime_secs, 60);
            }
            other => panic!("Expected TLM, got {:?}", other),
        }
    }

    #[test]
    fn parse_unknown_frame_returns_none() {
        assert!(Eddystone::parse(&[0xFF, 0x00]).is_none());
        assert!(Eddystone::parse(&[]).is_none());
    }

    #[test]
    fn from_service_data_finds_eddystone() {
        let mut svc = HashMap::new();
        svc.insert(
            "0000feaa-0000-1000-8000-00805f9b34fb".to_string(),
            vec![0x10, 0xEC, 0x03, b'a', 0x07],
        );
        assert!(from_service_data(&svc).is_some());
    }
}
