use crate::classifier::{self, DeviceType};
use crate::vendor;
use btleplug::api::{AddressType, Central, CentralEvent, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::{Adapter, Manager, PeripheralId};
use futures::stream::StreamExt;
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

/// Build a `ScanResult` from btleplug peripheral properties.
fn build_scan_result(props: btleplug::api::PeripheralProperties) -> ScanResult {
    let mac = props.address.to_string();
    let is_randomized = match props.address_type {
        Some(AddressType::Random) => true,
        Some(AddressType::Public) => false,
        None => classifier::is_randomized_mac(&mac),
    };
    let manufacturer_data = props.manufacturer_data;
    let device_class = props.class;
    let tx_power = props.tx_power_level;

    let oui_vendor = vendor::lookup_vendor(&mac);
    let vendor = oui_vendor.or_else(|| classifier::best_company_name(&manufacturer_data));

    let service_uuids: Vec<String> = props.services.iter().map(|u| u.to_string()).collect();
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

    ScanResult {
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
    }
}

/// Read properties from a peripheral by ID and send as a ScanResult.
async fn handle_peripheral_event(
    adapter: &Adapter,
    id: &PeripheralId,
    tx: &mpsc::UnboundedSender<ScanMessage>,
) {
    let peripheral = match adapter.peripheral(id).await {
        Ok(p) => p,
        Err(_) => return,
    };
    let props = match peripheral.properties().await {
        Ok(Some(p)) => p,
        _ => return,
    };
    let result = build_scan_result(props);
    let _ = tx.send(ScanMessage::Result(result));
}

pub async fn scan_loop(
    tx: mpsc::UnboundedSender<ScanMessage>,
    adapter: Adapter,
    scan_duration: Duration,
) {
    loop {
        // Get the event stream before starting the scan so we don't miss events
        let mut events = match adapter.events().await {
            Ok(e) => e,
            Err(e) => {
                let _ = tx.send(ScanMessage::Error(format!("Failed to get event stream: {e}")));
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        if let Err(e) = adapter.start_scan(ScanFilter::default()).await {
            let _ = tx.send(ScanMessage::Error(format!("Scan start failed: {e}")));
            tokio::time::sleep(Duration::from_secs(5)).await;
            continue;
        }

        let mut cycle_interval = tokio::time::interval(scan_duration);
        cycle_interval.tick().await; // consume the immediate first tick

        loop {
            tokio::select! {
                event = events.next() => {
                    match event {
                        Some(CentralEvent::DeviceDiscovered(id))
                        | Some(CentralEvent::DeviceUpdated(id)) => {
                            handle_peripheral_event(&adapter, &id, &tx).await;
                        }
                        Some(_) => {} // ignore other events
                        None => break, // stream ended, restart
                    }
                }
                _ = cycle_interval.tick() => {
                    if tx.send(ScanMessage::CycleComplete).is_err() {
                        return;
                    }
                }
            }
        }

        // Event stream ended — stop scan and restart
        let _ = adapter.stop_scan().await;
        let _ = tx.send(ScanMessage::Error("Event stream ended, restarting scan".into()));
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use btleplug::api::{AddressType, BDAddr};
    use std::collections::HashMap;
    use uuid::Uuid;

    fn make_props(
        mac: [u8; 6],
        address_type: Option<AddressType>,
        rssi: Option<i16>,
        name: Option<&str>,
        tx_power: Option<i16>,
    ) -> btleplug::api::PeripheralProperties {
        btleplug::api::PeripheralProperties {
            address: BDAddr::from(mac),
            address_type,
            local_name: name.map(|s| s.to_string()),
            rssi,
            tx_power_level: tx_power,
            manufacturer_data: HashMap::new(),
            services: Vec::new(),
            service_data: HashMap::new(),
            class: None,
        }
    }

    #[test]
    fn build_scan_result_captures_rssi() {
        let props = make_props(
            [0xAA, 0xBB, 0xCC, 0x11, 0x22, 0x33],
            Some(AddressType::Public),
            Some(-55),
            Some("TestDevice"),
            Some(4),
        );
        let result = build_scan_result(props);
        assert_eq!(result.rssi, Some(-55));
        assert_eq!(result.name.as_deref(), Some("TestDevice"));
        assert_eq!(result.tx_power, Some(4));
        assert_eq!(result.mac, "AA:BB:CC:11:22:33");
        assert!(!result.is_randomized);
    }

    #[test]
    fn build_scan_result_handles_none_rssi() {
        let props = make_props(
            [0x00, 0x11, 0x22, 0x33, 0x44, 0x55],
            Some(AddressType::Public),
            None,
            None,
            None,
        );
        let result = build_scan_result(props);
        assert_eq!(result.rssi, None);
        assert_eq!(result.name, None);
        assert_eq!(result.tx_power, None);
    }

    #[test]
    fn build_scan_result_randomized_address_type() {
        let props = make_props(
            [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC],
            Some(AddressType::Random),
            Some(-70),
            None,
            None,
        );
        let result = build_scan_result(props);
        assert!(result.is_randomized);
    }

    #[test]
    fn build_scan_result_no_address_type_falls_back_to_heuristic() {
        // MAC with bit 1 of first octet set → randomized
        let props = make_props(
            [0xCB, 0x34, 0x56, 0x78, 0x9A, 0xBC],
            None,
            Some(-80),
            None,
            None,
        );
        let result = build_scan_result(props);
        // The heuristic checks the second-least significant bit of the first octet
        assert_eq!(result.is_randomized, classifier::is_randomized_mac(&result.mac));
    }

    #[test]
    fn build_scan_result_service_uuids() {
        let heart_rate = Uuid::parse_str("0000180d-0000-1000-8000-00805f9b34fb").unwrap();
        let mut props = make_props(
            [0xAA, 0xBB, 0xCC, 0x11, 0x22, 0x33],
            Some(AddressType::Public),
            Some(-60),
            None,
            None,
        );
        props.services.push(heart_rate);
        let result = build_scan_result(props);
        assert_eq!(result.service_uuids.len(), 1);
        assert!(result.service_uuids[0].contains("180d"));
    }

    #[test]
    fn build_scan_result_manufacturer_data() {
        let mut props = make_props(
            [0xAA, 0xBB, 0xCC, 0x11, 0x22, 0x33],
            Some(AddressType::Public),
            Some(-45),
            None,
            None,
        );
        props.manufacturer_data.insert(0x004C, vec![0x01, 0x02]); // Apple
        let result = build_scan_result(props);
        assert!(result.manufacturer_data.contains_key(&0x004C));
        assert_eq!(result.vendor.as_deref(), Some("Apple"));
    }

    #[test]
    fn build_scan_result_fingerprint_stable() {
        let props1 = make_props(
            [0xAA, 0xBB, 0xCC, 0x11, 0x22, 0x33],
            Some(AddressType::Public),
            Some(-50),
            Some("MyDevice"),
            Some(4),
        );
        let props2 = make_props(
            [0xAA, 0xBB, 0xCC, 0x11, 0x22, 0x33],
            Some(AddressType::Public),
            Some(-70), // different RSSI
            Some("MyDevice"),
            Some(4),
        );
        let r1 = build_scan_result(props1);
        let r2 = build_scan_result(props2);
        // Fingerprint should be the same — RSSI doesn't affect it
        assert_eq!(r1.fingerprint, r2.fingerprint);
    }
}
