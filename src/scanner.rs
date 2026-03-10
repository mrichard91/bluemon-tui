use crate::classifier::{self, DeviceType};
use crate::vendor;
use btleplug::api::{AddressType, Central, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::{Adapter, Manager};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;

#[allow(dead_code)]
pub struct ScanResult {
    pub mac: String,
    pub name: Option<String>,
    pub rssi: Option<i16>,
    pub tx_power: Option<i16>,
    pub vendor: Option<String>,
    pub device_type: DeviceType,
    pub service_uuids: Vec<String>,
    pub is_randomized: bool,
    pub manufacturer_data: HashMap<u16, Vec<u8>>,
    pub device_class: Option<u32>,
    pub fingerprint: String,
}

pub enum ScanMessage {
    Result(ScanResult),
    CycleComplete,
    Error(String),
}

/// Create a BLE adapter by index. Shared between scanner and GATT prober.
pub async fn get_adapter(adapter_index: usize) -> anyhow::Result<Adapter> {
    let manager = Manager::new().await?;
    let adapters = manager.adapters().await?;
    adapters
        .into_iter()
        .nth(adapter_index)
        .ok_or_else(|| anyhow::anyhow!("Adapter index {adapter_index} not found"))
}

pub async fn scan_loop(
    tx: mpsc::UnboundedSender<ScanMessage>,
    adapter: Adapter,
    scan_duration: Duration,
) {
    loop {
        if let Err(e) = adapter.start_scan(ScanFilter::default()).await {
            let _ = tx.send(ScanMessage::Error(format!("Scan start failed: {e}")));
            tokio::time::sleep(Duration::from_secs(5)).await;
            continue;
        }

        tokio::time::sleep(scan_duration).await;
        let _ = adapter.stop_scan().await;

        let peripherals = match adapter.peripherals().await {
            Ok(p) => p,
            Err(e) => {
                let _ = tx.send(ScanMessage::Error(format!("Failed to get peripherals: {e}")));
                continue;
            }
        };

        for peripheral in peripherals {
            let props = match peripheral.properties().await {
                Ok(Some(p)) => p,
                _ => continue,
            };

            let mac = props.address.to_string();
            // Prefer btleplug's address_type (from the controller) over bit-heuristic
            let is_randomized = match props.address_type {
                Some(AddressType::Random) => true,
                Some(AddressType::Public) => false,
                None => classifier::is_randomized_mac(&mac),
            };
            let manufacturer_data = props.manufacturer_data;
            let device_class = props.class;
            let tx_power = props.tx_power_level;

            // Always try OUI lookup — some randomized MACs still carry a real OUI prefix.
            // Layer manufacturer_data company name on top as fallback.
            let oui_vendor = vendor::lookup_vendor(&mac);

            let vendor = oui_vendor.or_else(|| {
                classifier::best_company_name(&manufacturer_data)
            });

            let service_uuids: Vec<String> =
                props.services.iter().map(|u| u.to_string()).collect();

            let name = props.local_name;

            let device_type = classifier::classify_device(
                vendor.as_deref(),
                name.as_deref(),
                &service_uuids,
                &manufacturer_data,
                device_class,
            );

            let fingerprint = classifier::compute_fingerprint(
                name.as_deref(),
                &service_uuids,
                &manufacturer_data,
                tx_power,
            );

            let result = ScanResult {
                mac,
                name,
                rssi: props.rssi,
                tx_power,
                vendor,
                device_type,
                service_uuids,
                is_randomized,
                manufacturer_data,
                device_class,
                fingerprint,
            };

            if tx.send(ScanMessage::Result(result)).is_err() {
                return;
            }
        }

        if tx.send(ScanMessage::CycleComplete).is_err() {
            return;
        }
    }
}
