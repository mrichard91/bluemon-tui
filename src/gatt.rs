use btleplug::api::{Central, Peripheral as _};
use btleplug::platform::Adapter;
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

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
    #[serde(default)]
    pub pnp_id: Option<String>,
    pub probed_at: String,
}

pub enum ProbeRequest {
    Probe { mac: String },
}

pub enum ProbeResult {
    Success { mac: String, info: GattDeviceInfo },
    #[allow(dead_code)]
    Failed { mac: String, error: String },
}

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

/// Long-lived GATT probe task. Receives ProbeRequests, connects to devices,
/// reads Device Information Service characteristics, and sends results back.
pub async fn probe_loop(
    adapter: Adapter,
    mut rx: mpsc::UnboundedReceiver<ProbeRequest>,
    tx: mpsc::UnboundedSender<ProbeResult>,
) {
    while let Some(req) = rx.recv().await {
        let ProbeRequest::Probe { mac } = req;
        let result = probe_device(&adapter, &mac).await;
        let _ = tx.send(result);
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

    // Find the peripheral matching target address
    let mut peripheral = None;
    for p in peripherals {
        if let Ok(Some(props)) = p.properties().await {
            if props.address == target {
                peripheral = Some(p);
                break;
            }
        }
    }

    let peripheral = match peripheral {
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
        probed_at: Local::now().to_rfc3339(),
    };

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
                        let src = if vendor_src == 1 { "BT" } else { "USB" };
                        info.pnp_id =
                            Some(format!("{src}:{vendor_id:04X}:{product_id:04X}:{version:04X}"));
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
