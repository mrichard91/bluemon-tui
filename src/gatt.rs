//! GATT Device Information Service prober.
//!
//! Connects to a BLE device and reads characteristics from the Generic Access
//! Service (0x1800), Device Information Service (0x180A), and Battery Service
//! (0x180F). Runs as a long-lived background task that fans probe requests out
//! to a small pool of concurrent workers.

use btleplug::api::{Central, Peripheral as _};
use btleplug::platform::Adapter;
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Semaphore};
use uuid::Uuid;

/// Maximum number of concurrent GATT probes. Each probe holds a connection for
/// up to 10 seconds; running them serially blocks every queued request behind
/// the timeout of the one in flight.
const MAX_CONCURRENT_PROBES: usize = 3;

/// Device Information Service data read via GATT connection.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GattDeviceInfo {
    pub manufacturer_name: Option<String>,
    pub model_number: Option<String>,
    pub firmware_revision: Option<String>,
    pub hardware_revision: Option<String>,
    pub software_revision: Option<String>,
    #[serde(default)]
    pub battery_level: Option<u8>,
    /// Pretty-printed PnP ID (kept for display / backward compat).
    #[serde(default)]
    pub pnp_id: Option<String>,
    /// Source byte: 1 = Bluetooth SIG, 2 = USB Implementers Forum.
    #[serde(default)]
    pub pnp_vendor_id_source: Option<u8>,
    #[serde(default)]
    pub pnp_vendor_id: Option<u16>,
    #[serde(default)]
    pub pnp_product_id: Option<u16>,
    #[serde(default)]
    pub pnp_product_version: Option<u16>,
    /// 16-bit GATT appearance category (e.g. 0x0040 = Generic Phone).
    #[serde(default)]
    pub appearance: Option<u16>,
    /// Human-readable appearance name resolved from `appearance`.
    #[serde(default)]
    pub appearance_name: Option<String>,
    pub probed_at: String,
}

/// Request to probe a specific device by MAC address.
pub enum ProbeRequest {
    Probe { mac: String },
}

/// Result of a GATT probe attempt.
pub enum ProbeResult {
    Success {
        mac: String,
        info: GattDeviceInfo,
    },
    #[allow(dead_code)]
    Failed {
        mac: String,
        error: String,
    },
}

// BLE Generic Access Service UUIDs
const GAP_SERVICE: Uuid = ble_uuid(0x1800);
const APPEARANCE: Uuid = ble_uuid(0x2A01);

// BLE Device Information Service UUIDs
const DIS_SERVICE: Uuid = ble_uuid(0x180A);
const MANUFACTURER_NAME: Uuid = ble_uuid(0x2A29);
const MODEL_NUMBER: Uuid = ble_uuid(0x2A24);
const FIRMWARE_REVISION: Uuid = ble_uuid(0x2A26);
const HARDWARE_REVISION: Uuid = ble_uuid(0x2A27);
const SOFTWARE_REVISION: Uuid = ble_uuid(0x2A28);
const PNP_ID: Uuid = ble_uuid(0x2A50);

// BLE Battery Service UUIDs
const BATTERY_SERVICE: Uuid = ble_uuid(0x180F);
const BATTERY_LEVEL: Uuid = ble_uuid(0x2A19);

/// Convert a 16-bit BLE short UUID to a full 128-bit UUID.
/// BLE base UUID: 00000000-0000-1000-8000-00805F9B34FB
const fn ble_uuid(short: u16) -> Uuid {
    Uuid::from_u128(((short as u128) << 96) | 0x00000000_0000_1000_8000_00805F9B34FB)
}

/// Long-lived GATT probe dispatcher. Spawns each probe as its own task gated
/// by a Semaphore so a single slow probe can't stall the queue.
pub async fn probe_loop(
    adapter: Adapter,
    mut rx: mpsc::UnboundedReceiver<ProbeRequest>,
    tx: mpsc::UnboundedSender<ProbeResult>,
) {
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_PROBES));
    while let Some(req) = rx.recv().await {
        let ProbeRequest::Probe { mac } = req;
        let Ok(permit) = semaphore.clone().acquire_owned().await else {
            return; // semaphore closed (shouldn't happen)
        };
        let adapter = adapter.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let _permit = permit; // released on drop
            let result = probe_device(&adapter, &mac).await;
            let _ = tx.send(result);
        });
    }
}

async fn probe_device(adapter: &Adapter, mac: &str) -> ProbeResult {
    let mac_str = mac.to_string();
    match tokio::time::timeout(Duration::from_secs(10), probe_inner(adapter, mac)).await {
        Ok(result) => result,
        Err(_) => ProbeResult::Failed {
            mac: mac_str,
            error: "Connection timeout (10s)".to_string(),
        },
    }
}

async fn probe_inner(adapter: &Adapter, mac: &str) -> ProbeResult {
    let mac_str = mac.to_string();
    let target: btleplug::api::BDAddr = match mac.parse() {
        Ok(addr) => addr,
        Err(e) => {
            return ProbeResult::Failed {
                mac: mac_str,
                error: format!("Invalid MAC: {e}"),
            }
        }
    };

    let peripherals = match adapter.peripherals().await {
        Ok(p) => p,
        Err(e) => {
            return ProbeResult::Failed {
                mac: mac_str,
                error: format!("List error: {e}"),
            }
        }
    };

    // Peripheral::address() is synchronous in btleplug 0.11+, so we can find by
    // address without awaiting properties() for every cached peripheral.
    let peripheral = match peripherals.into_iter().find(|p| p.address() == target) {
        Some(p) => p,
        None => {
            return ProbeResult::Failed {
                mac: mac_str,
                error: "Not found nearby".to_string(),
            }
        }
    };

    if let Err(e) = peripheral.connect().await {
        return ProbeResult::Failed {
            mac: mac_str,
            error: format!("Connect failed: {e}"),
        };
    }

    let result = read_gatt_info(&peripheral).await;
    let _ = peripheral.disconnect().await;

    match result {
        Ok(info) => ProbeResult::Success { mac: mac_str, info },
        Err(e) => ProbeResult::Failed {
            mac: mac_str,
            error: e,
        },
    }
}

async fn read_gatt_info(
    peripheral: &btleplug::platform::Peripheral,
) -> Result<GattDeviceInfo, String> {
    peripheral
        .discover_services()
        .await
        .map_err(|e| format!("Service discovery failed: {e}"))?;

    let services = peripheral.services();

    let mut info = GattDeviceInfo {
        manufacturer_name: None,
        model_number: None,
        firmware_revision: None,
        hardware_revision: None,
        software_revision: None,
        battery_level: None,
        pnp_id: None,
        pnp_vendor_id_source: None,
        pnp_vendor_id: None,
        pnp_product_id: None,
        pnp_product_version: None,
        appearance: None,
        appearance_name: None,
        probed_at: Local::now().to_rfc3339(),
    };

    // Read Generic Access Service (0x1800) — Appearance characteristic.
    if let Some(gap) = services.iter().find(|s| s.uuid == GAP_SERVICE) {
        for ch in &gap.characteristics {
            if ch.uuid == APPEARANCE {
                if let Ok(v) = peripheral.read(ch).await {
                    if v.len() >= 2 {
                        let appearance = u16::from_le_bytes([v[0], v[1]]);
                        info.appearance = Some(appearance);
                        info.appearance_name =
                            Some(appearance_name(appearance).to_string());
                    }
                }
            }
        }
    }

    // Read Device Information Service (0x180A)
    if let Some(dis) = services.iter().find(|s| s.uuid == DIS_SERVICE) {
        for ch in &dis.characteristics {
            if ch.uuid == PNP_ID {
                if let Ok(v) = peripheral.read(ch).await {
                    if v.len() >= 7 {
                        let vendor_src = v[0];
                        let vendor_id = u16::from_le_bytes([v[1], v[2]]);
                        let product_id = u16::from_le_bytes([v[3], v[4]]);
                        let version = u16::from_le_bytes([v[5], v[6]]);
                        let src = match vendor_src {
                            1 => "BT",
                            2 => "USB",
                            _ => "?",
                        };
                        info.pnp_id = Some(format!(
                            "{src}:{vendor_id:04X}:{product_id:04X}:{version:04X}"
                        ));
                        info.pnp_vendor_id_source = Some(vendor_src);
                        info.pnp_vendor_id = Some(vendor_id);
                        info.pnp_product_id = Some(product_id);
                        info.pnp_product_version = Some(version);
                    }
                }
                continue;
            }

            let value = match peripheral.read(ch).await {
                Ok(v) => {
                    let s = String::from_utf8_lossy(&v).trim().to_string();
                    if s.is_empty() {
                        continue;
                    }
                    s
                }
                Err(_) => continue,
            };

            if ch.uuid == MANUFACTURER_NAME {
                info.manufacturer_name = Some(value);
            } else if ch.uuid == MODEL_NUMBER {
                info.model_number = Some(value);
            } else if ch.uuid == FIRMWARE_REVISION {
                info.firmware_revision = Some(value);
            } else if ch.uuid == HARDWARE_REVISION {
                info.hardware_revision = Some(value);
            } else if ch.uuid == SOFTWARE_REVISION {
                info.software_revision = Some(value);
            }
        }
    }

    // Read Battery Service (0x180F)
    if let Some(bas) = services.iter().find(|s| s.uuid == BATTERY_SERVICE) {
        for ch in &bas.characteristics {
            if ch.uuid == BATTERY_LEVEL {
                if let Ok(v) = peripheral.read(ch).await {
                    if let Some(&level) = v.first() {
                        info.battery_level = Some(level);
                    }
                }
            }
        }
    }

    Ok(info)
}

/// Resolve a 16-bit GATT Appearance value to a human-readable category.
///
/// Appearance is a Bluetooth-SIG-coded category: the upper 10 bits identify a
/// category (e.g. Phone, Computer, Watch) and the lower 6 bits a sub-type. We
/// map the most common categories; the sub-type is preserved in the raw value.
pub fn appearance_name(value: u16) -> &'static str {
    let category = value >> 6;
    match category {
        0x000 => "Unknown",
        0x001 => "Phone",
        0x002 => "Computer",
        0x003 => "Watch",
        0x004 => "Clock",
        0x005 => "Display",
        0x006 => "Remote Control",
        0x007 => "Eye-glasses",
        0x008 => "Tag",
        0x009 => "Keyring",
        0x00A => "Media Player",
        0x00B => "Barcode Scanner",
        0x00C => "Thermometer",
        0x00D => "Heart Rate Sensor",
        0x00E => "Blood Pressure",
        0x00F => "HID",
        0x010 => "Glucose Meter",
        0x011 => "Running/Walking Sensor",
        0x012 => "Cycling",
        0x013 => "Control Device",
        0x014 => "Network Device",
        0x015 => "Sensor",
        0x016 => "Light Fixtures",
        0x017 => "Fan",
        0x018 => "HVAC",
        0x019 => "Air Conditioning",
        0x01A => "Humidifier",
        0x01B => "Heating",
        0x01C => "Access Control",
        0x01D => "Motorized Device",
        0x01E => "Power Device",
        0x01F => "Light Source",
        0x020 => "Window Covering",
        0x021 => "Audio Sink",
        0x022 => "Audio Source",
        0x023 => "Motorized Vehicle",
        0x024 => "Domestic Appliance",
        0x025 => "Wearable Audio Device",
        0x026 => "Aircraft",
        0x027 => "AV Equipment",
        0x028 => "Display Equipment",
        0x029 => "Hearing Aid",
        0x02A => "Gaming",
        0x02B => "Signage",
        0x031 => "Pulse Oximeter",
        0x032 => "Weight Scale",
        0x033 => "Personal Mobility Device",
        0x034 => "Continuous Glucose Monitor",
        0x035 => "Insulin Pump",
        0x036 => "Medication Delivery",
        0x037 => "Spirometer",
        0x051 => "Outdoor Sports",
        _ => "Other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_categories_resolve() {
        // 0x0040 = Generic Phone (category 1, sub-type 0)
        assert_eq!(appearance_name(0x0040), "Phone");
        // 0x00C0 = Watch (category 3)
        assert_eq!(appearance_name(0x00C0), "Watch");
        // 0x03C0 = HID (category 0x0F)
        assert_eq!(appearance_name(0x03C0), "HID");
        // 0x0840 = Audio Sink (category 0x21)
        assert_eq!(appearance_name(0x0840), "Audio Sink");
        // Unknown / category 0
        assert_eq!(appearance_name(0x0000), "Unknown");
    }
}
