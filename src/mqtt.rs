//! MQTT publisher for forwarding raw scan and probe observations.

use crate::classifier;
use crate::config::MqttConfig;
use crate::gatt::GattDeviceInfo;
use crate::scanner::ScanResult;
use anyhow::{anyhow, bail};
use rumqttc::{AsyncClient, MqttOptions, QoS};
use serde::Serialize;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::time::Duration;

pub struct Publisher {
    client: AsyncClient,
    topic: String,
    qos: QoS,
    retain: bool,
    collector: CollectorMetadata,
}

#[derive(Clone, Serialize)]
struct CollectorMetadata {
    channel_name: String,
    sensor_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    site_name: Option<String>,
    adapter_index: usize,
    scan_duration_secs: u64,
}

#[derive(Serialize)]
struct ScanObservationPayload {
    schema: &'static str,
    observation_type: &'static str,
    observed_at: String,
    scan_cycle: u32,
    collector: CollectorMetadata,
    mac: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rssi: Option<i16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tx_power: Option<i16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    service_uuids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    manufacturer_data: Vec<ManufacturerData>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    service_data: Vec<ServiceData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    device_class: Option<u32>,
    is_randomized: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    address_type: Option<&'static str>,
}

#[derive(Serialize)]
struct GattObservationPayload {
    schema: &'static str,
    observation_type: &'static str,
    observed_at: String,
    collector: CollectorMetadata,
    mac: String,
    gatt: GattDeviceInfo,
}

#[derive(Serialize)]
struct ManufacturerData {
    company_id: u16,
    data_hex: String,
}

#[derive(Serialize)]
struct ServiceData {
    service_uuid: String,
    data_hex: String,
}

impl Publisher {
    pub fn new(
        cfg: &MqttConfig,
        adapter_index: usize,
        scan_duration_secs: u64,
    ) -> anyhow::Result<Option<Self>> {
        if !cfg.enabled {
            return Ok(None);
        }
        if cfg.host.trim().is_empty() {
            bail!("mqtt.host cannot be empty when MQTT publishing is enabled");
        }

        let qos = parse_qos(cfg.qos)?;
        let mut options = MqttOptions::new(client_id(cfg), cfg.host.trim().to_string(), cfg.port);
        options.set_keep_alive(Duration::from_secs(cfg.keep_alive_secs.max(1)));
        if let Some(username) = cfg
            .username
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            options.set_credentials(username, cfg.password.as_deref().unwrap_or(""));
        }

        let (client, mut eventloop) = AsyncClient::new(options, 100);
        tokio::spawn(async move {
            loop {
                if let Err(err) = eventloop.poll().await {
                    eprintln!("Warning: MQTT event loop error: {err}");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        });

        Ok(Some(Self {
            client,
            topic: observation_topic(cfg),
            qos,
            retain: cfg.retain,
            collector: CollectorMetadata {
                channel_name: cfg.channel_name.clone(),
                sensor_name: cfg.sensor_name.clone(),
                site_name: cfg.site_name.clone(),
                adapter_index,
                scan_duration_secs,
            },
        }))
    }

    pub async fn publish_scan_results(
        &self,
        scan_cycle: u32,
        observed_at: &str,
        results: &[ScanResult],
    ) -> anyhow::Result<()> {
        for result in results {
            let payload = ScanObservationPayload {
                schema: "bluemon.scan_observation.v1",
                observation_type: "ble_advertisement",
                observed_at: observed_at.to_string(),
                scan_cycle,
                collector: self.collector.clone(),
                mac: result.mac.clone(),
                name: result.name.clone(),
                rssi: result.rssi,
                tx_power: result.tx_power,
                service_uuids: result.service_uuids.clone(),
                manufacturer_data: manufacturer_data(&result.manufacturer_data),
                service_data: service_data(&result.service_data),
                device_class: result.device_class,
                is_randomized: result.is_randomized,
                address_type: classifier::parse_addr_type(&result.mac).map(|kind| kind.to_db()),
            };
            self.publish_json(&payload).await?;
        }
        Ok(())
    }

    pub async fn publish_gatt_observation(
        &self,
        mac: &str,
        info: &GattDeviceInfo,
    ) -> anyhow::Result<()> {
        let payload = GattObservationPayload {
            schema: "bluemon.gatt_observation.v1",
            observation_type: "gatt_probe",
            observed_at: info.probed_at.clone(),
            collector: self.collector.clone(),
            mac: mac.to_string(),
            gatt: info.clone(),
        };
        self.publish_json(&payload).await
    }

    async fn publish_json<T: Serialize>(&self, payload: &T) -> anyhow::Result<()> {
        let bytes = serde_json::to_vec(payload)?;
        self.client
            .publish(self.topic.clone(), self.qos, self.retain, bytes)
            .await
            .map_err(|err| anyhow!("failed to publish MQTT message: {err}"))
    }
}

fn parse_qos(value: u8) -> anyhow::Result<QoS> {
    match value {
        0 => Ok(QoS::AtMostOnce),
        1 => Ok(QoS::AtLeastOnce),
        2 => Ok(QoS::ExactlyOnce),
        _ => bail!("mqtt.qos must be 0, 1, or 2"),
    }
}

fn observation_topic(cfg: &MqttConfig) -> String {
    let mut topic = String::new();
    for segment in [
        cfg.topic_prefix.as_str(),
        cfg.channel_name.as_str(),
        cfg.sensor_name.as_str(),
        "observations",
    ] {
        let trimmed = segment.trim_matches('/');
        if trimmed.is_empty() {
            continue;
        }
        if !topic.is_empty() {
            topic.push('/');
        }
        topic.push_str(trimmed);
    }
    if topic.is_empty() {
        "observations".to_string()
    } else {
        topic
    }
}

fn client_id(cfg: &MqttConfig) -> String {
    cfg.client_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string())
        .unwrap_or_else(|| {
            let channel = sanitize_client_id(&cfg.channel_name);
            let sensor = sanitize_client_id(&cfg.sensor_name);
            format!("bluemon-{channel}-{sensor}")
        })
}

fn sanitize_client_id(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "collector".to_string()
    } else {
        trimmed.to_string()
    }
}

fn manufacturer_data(entries: &HashMap<u16, Vec<u8>>) -> Vec<ManufacturerData> {
    let mut data: Vec<ManufacturerData> = entries
        .iter()
        .map(|(&company_id, payload)| ManufacturerData {
            company_id,
            data_hex: hex_string(payload),
        })
        .collect();
    data.sort_by_key(|entry| entry.company_id);
    data
}

fn service_data(entries: &HashMap<String, Vec<u8>>) -> Vec<ServiceData> {
    let mut data: Vec<ServiceData> = entries
        .iter()
        .map(|(uuid, payload)| ServiceData {
            service_uuid: uuid.clone(),
            data_hex: hex_string(payload),
        })
        .collect();
    data.sort_by(|a, b| a.service_uuid.cmp(&b.service_uuid));
    data
}

fn hex_string(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut out, "{byte:02X}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observation_topic_uses_prefix_channel_and_sensor() {
        let cfg = MqttConfig {
            topic_prefix: "bluemon".into(),
            channel_name: "lab".into(),
            sensor_name: "collector-01".into(),
            ..MqttConfig::default()
        };
        assert_eq!(
            observation_topic(&cfg),
            "bluemon/lab/collector-01/observations"
        );
    }

    #[test]
    fn generated_client_id_is_sanitized() {
        let cfg = MqttConfig {
            channel_name: "HQ West".into(),
            sensor_name: "Sensor/01".into(),
            ..MqttConfig::default()
        };
        assert_eq!(client_id(&cfg), "bluemon-hq-west-sensor-01");
    }

    #[test]
    fn manufacturer_data_is_sorted_and_hex_encoded() {
        let mut entries = HashMap::new();
        entries.insert(0x00E0, vec![0x11, 0x22]);
        entries.insert(0x004C, vec![0xAA, 0xBB, 0xCC]);

        let payload = manufacturer_data(&entries);
        assert_eq!(payload.len(), 2);
        assert_eq!(payload[0].company_id, 0x004C);
        assert_eq!(payload[0].data_hex, "AABBCC");
        assert_eq!(payload[1].company_id, 0x00E0);
        assert_eq!(payload[1].data_hex, "1122");
    }
}
