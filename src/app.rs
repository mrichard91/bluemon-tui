use crate::chat::ChatState;
use crate::classifier::{self as cls, DeviceType};
use crate::continuity::ContinuityData;
use crate::gatt::{self, GattDeviceInfo};
use crate::scanner::ScanResult;
use crate::{continuity, fast_pair};
use chrono::{DateTime, Datelike, Local};
use ratatui::widgets::TableState;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use tokio::sync::mpsc;

/// Path loss exponent for log-distance model. Higher = faster signal decay.
/// - 2.0: free space (outdoors, line-of-sight)
/// - 3.0: typical indoor (default)
/// - 3.5–4.0: dense indoor (multiple walls/floors)
/// Override via BLE_PATH_LOSS_N in .env.
static PATH_LOSS_N: LazyLock<f64> = LazyLock::new(|| {
    std::env::var("BLE_PATH_LOSS_N")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3.0)
});

#[derive(Clone)]
pub struct DeviceInfo {
    pub mac: String,
    pub name: Option<String>,
    pub rssi: Option<i16>,
    pub tx_power: Option<i16>,
    pub vendor: Option<String>,
    pub device_type: DeviceType,
    pub service_uuids: Vec<String>,
    pub sightings: u32,
    pub first_seen: DateTime<Local>,
    pub last_seen: DateTime<Local>,
    pub is_randomized: bool,
    pub note: Option<String>,
    pub fingerprint: String,
    pub manufacturer_data: HashMap<u16, Vec<u8>>,
    pub continuity: Option<ContinuityData>,
    pub gatt_info: Option<GattDeviceInfo>,
    pub fast_pair_model: Option<String>,
    pub device_class: Option<u32>,
    pub addr_type: Option<cls::BleAddrType>,
}

#[derive(Clone)]
pub struct AggregatedDevice {
    pub fingerprint: String,
    pub representative_mac: String,
    pub mac_count: usize,
    pub name: Option<String>,
    pub rssi: Option<i16>,
    pub tx_power: Option<i16>,
    pub vendor: Option<String>,
    pub device_type: DeviceType,
    pub sightings: u32,
    pub first_seen: DateTime<Local>,
    pub last_seen: DateTime<Local>,
    pub note: Option<String>,
    pub service_uuids: Vec<String>,
    pub is_randomized: bool,
    pub addr_type: Option<cls::BleAddrType>,
    pub device_class: Option<u32>,
    pub continuity_summary: Option<String>,
    pub ibeacon_measured_power: Option<i8>,
    pub gatt_info: Option<GattDeviceInfo>,
    pub fast_pair_model: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Distance,
    Type,
    Mac,
    Vendor,
    Name,
    Rssi,
    Sightings,
    FirstSeen,
    LastSeen,
}

impl SortColumn {
    pub fn next(self) -> Self {
        match self {
            Self::Distance => Self::Type,
            Self::Type => Self::Mac,
            Self::Mac => Self::Vendor,
            Self::Vendor => Self::Name,
            Self::Name => Self::Rssi,
            Self::Rssi => Self::Sightings,
            Self::Sightings => Self::FirstSeen,
            Self::FirstSeen => Self::LastSeen,
            Self::LastSeen => Self::Distance,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Distance => "Dist",
            Self::Type => "Type",
            Self::Mac => "MAC",
            Self::Vendor => "Vendor",
            Self::Name => "Name",
            Self::Rssi => "RSSI",
            Self::Sightings => "Seen",
            Self::FirstSeen => "First Seen",
            Self::LastSeen => "Last Seen",
        }
    }
}

/// Estimate distance in meters from RSSI using log-distance path loss model.
///
/// Reference power at 1m:
/// - iBeacon measured_power is used directly (already calibrated at 1m).
/// - BLE tx_power is the transmitter output power, NOT a calibrated 1m RSSI,
///   so we ignore it and fall back to the default.
/// - Default: -59 dBm (typical BLE received power at 1m).
///
/// Formula: distance = 10 ^ ((ref_power - rssi) / (10 * n))
pub fn estimate_distance(rssi: i16, _tx_power: Option<i16>, ibeacon_power: Option<i8>) -> f64 {
    let ref_power = match ibeacon_power {
        Some(ibp) => ibp as f64,
        None => -59.0,
    };
    10_f64.powf((ref_power - rssi as f64) / (10.0 * *PATH_LOSS_N))
}

/// Format estimated distance for display.
pub fn format_distance(rssi: Option<i16>, tx_power: Option<i16>, ibeacon_power: Option<i8>) -> String {
    let Some(rssi) = rssi else {
        return "?".to_string();
    };
    let dist = estimate_distance(rssi, tx_power, ibeacon_power);
    if dist < 1.0 {
        "<1m".to_string()
    } else if dist < 10.0 {
        format!("~{:.1}m", dist)
    } else {
        format!("~{}m", dist.round() as u32)
    }
}

pub struct App {
    pub devices: HashMap<String, DeviceInfo>,
    pub aggregated: HashMap<String, AggregatedDevice>,
    pub display_list: Vec<String>, // fingerprints in sorted order
    pub table_state: TableState,
    pub sort_column: SortColumn,
    pub sort_ascending: bool,
    pub filter: String,
    pub filter_mode: bool,
    pub note_mode: bool,
    pub note_input: String,
    pub note_mac: String, // MAC of device being noted
    pub api_key_mode: bool,
    pub api_key_input: String,
    pub scan_count: u32,
    pub start_time: DateTime<Local>,
    pub scanning: bool,
    /// Fingerprint → set of MACs sharing that fingerprint
    pub fingerprint_groups: HashMap<String, HashSet<String>>,
    pub chat_mode: bool,
    pub chat: ChatState,
    pub probe_tx: Option<mpsc::UnboundedSender<gatt::ProbeRequest>>,
    pub detail_mode: bool,
    pub detail_scroll: usize,
    pub probe_cooldowns: HashMap<String, DateTime<Local>>,
    /// Status message shown briefly after a probe attempt.
    pub probe_status: Option<(String, DateTime<Local>)>,
    /// Cached detail view data (populated when entering detail mode).
    pub detail_rssi_history: Vec<i16>,
    pub detail_hourly: [u32; 24],
    pub detail_rotation: Option<crate::db::MacRotationStats>,
    /// Per-fingerprint hourly activity for the table sparkline column.
    pub hourly_cache: HashMap<String, [u32; 24]>,
}

impl App {
    pub fn new(db_path: String) -> Self {
        Self {
            devices: HashMap::new(),
            aggregated: HashMap::new(),
            display_list: Vec::new(),
            table_state: TableState::default(),
            sort_column: SortColumn::LastSeen,
            sort_ascending: false,
            filter: String::new(),
            filter_mode: false,
            note_mode: false,
            note_input: String::new(),
            note_mac: String::new(),
            api_key_mode: false,
            api_key_input: String::new(),
            scan_count: 0,
            start_time: Local::now(),
            scanning: true,
            fingerprint_groups: HashMap::new(),
            chat_mode: false,
            chat: ChatState::new(db_path),
            probe_tx: None,
            detail_mode: false,
            detail_scroll: 0,
            probe_cooldowns: HashMap::new(),
            probe_status: None,
            detail_rssi_history: Vec::new(),
            detail_hourly: [0; 24],
            detail_rotation: None,
            hourly_cache: HashMap::new(),
        }
    }

    pub fn upsert_device(&mut self, result: ScanResult) {
        let now = Local::now();
        let fp = result.fingerprint.clone();
        let mac = result.mac.clone();

        let entry = self.devices.entry(mac.clone()).or_insert_with(|| {
            DeviceInfo {
                mac: result.mac.clone(),
                name: None,
                rssi: None,
                tx_power: None,
                vendor: result.vendor.clone(),
                device_type: DeviceType::Unknown,
                service_uuids: Vec::new(),
                sightings: 0,
                first_seen: now,
                last_seen: now,
                is_randomized: result.is_randomized,
                note: None,
                fingerprint: fp.clone(),
                manufacturer_data: HashMap::new(),
                continuity: None,
                gatt_info: None,
                fast_pair_model: None,
                device_class: result.device_class,
                addr_type: cls::parse_addr_type(&result.mac),
            }
        });

        if result.name.is_some() {
            entry.name = result.name;
        }
        if result.vendor.is_some() {
            entry.vendor = result.vendor;
        }
        if result.rssi.is_some() {
            entry.rssi = result.rssi;
        }
        if result.tx_power.is_some() {
            entry.tx_power = result.tx_power;
        }
        entry.last_seen = now;
        entry.sightings += 1;
        if !result.service_uuids.is_empty() {
            entry.service_uuids = result.service_uuids;
        }
        if result.device_type != DeviceType::Unknown {
            entry.device_type = result.device_type;
        }
        if result.device_class.is_some() {
            entry.device_class = result.device_class;
        }

        // Store manufacturer data and parse enrichment fields
        if !result.manufacturer_data.is_empty() {
            entry.manufacturer_data = result.manufacturer_data.clone();
        }
        if let Some(apple_data) = result.manufacturer_data.get(&0x004C) {
            entry.continuity = continuity::ContinuityData::parse(apple_data);
        }
        if let Some(google_data) = result.manufacturer_data.get(&0x00E0) {
            if let Some(model) = fast_pair::lookup_model(google_data) {
                entry.fast_pair_model = Some(model);
            }
        }

        // Remove MAC from its old fingerprint group before updating
        let old_fp = std::mem::replace(&mut entry.fingerprint, fp.clone());
        if old_fp != fp {
            if let Some(old_group) = self.fingerprint_groups.get_mut(&old_fp) {
                old_group.remove(&mac);
                if old_group.is_empty() {
                    self.fingerprint_groups.remove(&old_fp);
                }
            }
            // Also remove from the MAC-as-fallback key (legacy empty fingerprint)
            if old_fp.is_empty() {
                if let Some(fallback_group) = self.fingerprint_groups.get_mut(&mac) {
                    fallback_group.remove(&mac);
                    if fallback_group.is_empty() {
                        self.fingerprint_groups.remove(&mac);
                    }
                }
            }
        }

        // Track which MACs share this fingerprint
        self.fingerprint_groups
            .entry(fp)
            .or_default()
            .insert(mac);
    }

    /// Rebuild fingerprint groups from current device data (e.g. after DB load).
    pub fn rebuild_fingerprint_groups(&mut self) {
        self.fingerprint_groups.clear();
        for (mac, dev) in &self.devices {
            // Legacy DB rows may have fingerprint = "" — use MAC as fallback
            // so they don't all collapse into one group.
            let fp = if dev.fingerprint.is_empty() {
                mac.clone()
            } else {
                dev.fingerprint.clone()
            };
            self.fingerprint_groups
                .entry(fp)
                .or_default()
                .insert(mac.clone());
        }
    }

    /// Build aggregated device entries from fingerprint groups.
    pub fn build_aggregated(&mut self) {
        self.aggregated.clear();
        for (fp, macs) in &self.fingerprint_groups {
            let mut devs: Vec<&DeviceInfo> = macs
                .iter()
                .filter_map(|m| self.devices.get(m))
                .collect();
            if devs.is_empty() {
                continue;
            }
            // Sort by last_seen descending to pick the most recent MAC
            devs.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));

            let most_recent = devs[0];
            // Name fallback: BLE advertised name -> Fast Pair model -> GATT model
            let name = devs
                .iter()
                .find_map(|d| d.name.clone())
                .or_else(|| devs.iter().find_map(|d| d.fast_pair_model.clone()))
                .or_else(|| {
                    devs.iter().find_map(|d| {
                        d.gatt_info
                            .as_ref()
                            .and_then(|g| g.model_number.clone())
                    })
                });
            let vendor = devs.iter().find_map(|d| d.vendor.clone());
            let device_type = devs
                .iter()
                .map(|d| d.device_type)
                .find(|dt| *dt != DeviceType::Unknown)
                .unwrap_or(DeviceType::Unknown);
            let sightings: u32 = devs.iter().map(|d| d.sightings).sum();
            let first_seen = devs.iter().map(|d| d.first_seen).min().unwrap();
            let last_seen = devs.iter().map(|d| d.last_seen).max().unwrap();
            let note = devs.iter().find_map(|d| d.note.clone());
            let is_randomized = devs.iter().any(|d| d.is_randomized);
            let addr_type = most_recent.addr_type;
            let device_class = devs.iter().find_map(|d| d.device_class);

            let mut uuid_set = HashSet::new();
            let mut service_uuids = Vec::new();
            for d in &devs {
                for u in &d.service_uuids {
                    if uuid_set.insert(u.clone()) {
                        service_uuids.push(u.clone());
                    }
                }
            }

            let continuity_summary = devs
                .iter()
                .find_map(|d| d.continuity.as_ref().map(|c| c.summary()));
            let ibeacon_measured_power = devs.iter().find_map(|d| {
                if let Some(ContinuityData::IBeacon { measured_power, .. }) = &d.continuity {
                    Some(*measured_power)
                } else {
                    None
                }
            });
            let gatt_info = devs
                .iter()
                .find_map(|d| d.gatt_info.clone());
            let fast_pair_model = devs
                .iter()
                .find_map(|d| d.fast_pair_model.clone());

            self.aggregated.insert(
                fp.clone(),
                AggregatedDevice {
                    fingerprint: fp.clone(),
                    representative_mac: most_recent.mac.clone(),
                    mac_count: macs.len(),
                    name,
                    rssi: most_recent.rssi,
                    tx_power: most_recent.tx_power,
                    vendor,
                    device_type,
                    sightings,
                    first_seen,
                    last_seen,
                    note,
                    service_uuids,
                    is_randomized,
                    addr_type,
                    device_class,
                    continuity_summary,
                    ibeacon_measured_power,
                    gatt_info,
                    fast_pair_model,
                },
            );
        }
    }

    pub fn rebuild_sorted_list(&mut self) {
        self.build_aggregated();

        let filter_lower = self.filter.to_lowercase();
        let mut fps: Vec<String> = self
            .aggregated
            .iter()
            .filter(|(fp, d)| {
                if filter_lower.is_empty() {
                    return true;
                }
                // Search aggregated fields
                d.representative_mac.to_lowercase().contains(&filter_lower)
                    || d.name
                        .as_deref()
                        .map_or(false, |n| n.to_lowercase().contains(&filter_lower))
                    || d.vendor
                        .as_deref()
                        .map_or(false, |v| v.to_lowercase().contains(&filter_lower))
                    || d.device_type.label().to_lowercase().contains(&filter_lower)
                    || d.note
                        .as_deref()
                        .map_or(false, |n| n.to_lowercase().contains(&filter_lower))
                    || d.fingerprint.to_lowercase().contains(&filter_lower)
                    // Also search all MACs in the group
                    || self.fingerprint_groups.get(*fp).map_or(false, |macs| {
                        macs.iter().any(|m| m.to_lowercase().contains(&filter_lower))
                    })
            })
            .map(|(fp, _)| fp.clone())
            .collect();

        let aggregated = &self.aggregated;
        let col = self.sort_column;
        let asc = self.sort_ascending;

        fps.sort_by(|a, b| {
            let da = &aggregated[a];
            let db = &aggregated[b];
            let ord = match col {
                SortColumn::Distance => {
                    let dist_a = da.rssi.map(|r| estimate_distance(r, da.tx_power, da.ibeacon_measured_power));
                    let dist_b = db.rssi.map(|r| estimate_distance(r, db.tx_power, db.ibeacon_measured_power));
                    // Closest first (default), None always last; tie-break by last_seen (newest first)
                    let cmp = match (dist_a, dist_b) {
                        (Some(a), Some(b)) => a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
                            .then(db.last_seen.cmp(&da.last_seen)),
                        (Some(_), None) => std::cmp::Ordering::Less,
                        (None, Some(_)) => std::cmp::Ordering::Greater,
                        (None, None) => db.last_seen.cmp(&da.last_seen),
                    };
                    return if asc { cmp.reverse() } else { cmp };
                }
                SortColumn::Type => da.device_type.label().cmp(db.device_type.label()),
                SortColumn::Mac => da.representative_mac.cmp(&db.representative_mac),
                SortColumn::Vendor => da.vendor.cmp(&db.vendor),
                SortColumn::Name => da.name.cmp(&db.name),
                SortColumn::Rssi => da.rssi.cmp(&db.rssi),
                SortColumn::Sightings => da.sightings.cmp(&db.sightings),
                SortColumn::FirstSeen => da.first_seen.cmp(&db.first_seen),
                SortColumn::LastSeen => da.last_seen.cmp(&db.last_seen),
            };
            if asc { ord } else { ord.reverse() }
        });

        // Remember which fingerprint was selected so we can restore it
        let selected_fp = self
            .table_state
            .selected()
            .and_then(|i| self.display_list.get(i).cloned());

        self.display_list = fps;

        if self.display_list.is_empty() {
            self.table_state.select(None);
        } else if let Some(fp) = selected_fp {
            let new_idx = self.display_list.iter().position(|f| *f == fp);
            self.table_state.select(Some(new_idx.unwrap_or(0)));
        }
    }

    /// Enter note editing mode for the currently selected device.
    pub fn enter_note_mode(&mut self) {
        if let Some(idx) = self.table_state.selected() {
            if let Some(fp) = self.display_list.get(idx).cloned() {
                if let Some(agg) = self.aggregated.get(&fp) {
                    self.note_mac = agg.representative_mac.clone();
                    // Pre-populate with existing note from any MAC in the group
                    self.note_input = self
                        .fingerprint_groups
                        .get(&fp)
                        .and_then(|macs| {
                            macs.iter().find_map(|m| {
                                self.devices.get(m).and_then(|d| d.note.clone())
                            })
                        })
                        .unwrap_or_default();
                    self.note_mode = true;
                }
            }
        }
    }

    /// Save the current note input to the device. Returns the MAC if saved.
    pub fn save_note(&mut self) -> Option<String> {
        self.note_mode = false;
        let mac = std::mem::take(&mut self.note_mac);
        let note = std::mem::take(&mut self.note_input);

        let note_value = if note.is_empty() { None } else { Some(note) };
        let group_key = self.devices.get(&mac).map(|d| {
            if d.fingerprint.is_empty() {
                mac.clone()
            } else {
                d.fingerprint.clone()
            }
        })?;

        if let Some(macs) = self.fingerprint_groups.get(&group_key).cloned() {
            for group_mac in macs {
                if let Some(d) = self.devices.get_mut(&group_mac) {
                    d.note = note_value.clone();
                }
            }
        }
        Some(mac)
    }

    pub fn cancel_note(&mut self) {
        self.note_mode = false;
        self.note_input.clear();
        self.note_mac.clear();
    }

    pub fn enter_api_key_mode(&mut self) {
        self.api_key_mode = true;
        self.api_key_input.clear();
    }

    pub fn save_api_key(&mut self) -> Option<Option<String>> {
        self.api_key_mode = false;
        let key = std::mem::take(&mut self.api_key_input);
        let trimmed = key.trim().to_string();
        if trimmed.is_empty() {
            self.chat.set_api_key(None);
            Some(None)
        } else {
            self.chat.set_api_key(Some(trimmed.clone()));
            Some(Some(trimmed))
        }
    }

    pub fn cancel_api_key(&mut self) {
        self.api_key_mode = false;
        self.api_key_input.clear();
    }

    pub fn scroll_down(&mut self) {
        if self.display_list.is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => (i + 1).min(self.display_list.len() - 1),
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    pub fn scroll_up(&mut self) {
        if self.display_list.is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => i.saturating_sub(1),
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    pub fn scroll_top(&mut self) {
        if !self.display_list.is_empty() {
            self.table_state.select(Some(0));
        }
    }

    pub fn scroll_bottom(&mut self) {
        if !self.display_list.is_empty() {
            self.table_state.select(Some(self.display_list.len() - 1));
        }
    }

    pub fn cycle_sort(&mut self) {
        self.sort_column = self.sort_column.next();
        self.rebuild_sorted_list();
    }

    pub fn reverse_sort(&mut self) {
        self.sort_ascending = !self.sort_ascending;
        self.rebuild_sorted_list();
    }
}

/// Format a duration as a relative time string like "3s ago", "2m ago".
pub fn format_relative(dt: DateTime<Local>) -> String {
    let elapsed = Local::now().signed_duration_since(dt);
    let secs = elapsed.num_seconds();
    if secs < 60 {
        format!("{}s ago", secs)
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else {
        format!("{}h ago", secs / 3600)
    }
}

/// Format a timestamp compactly for the "First Seen" column.
pub fn format_compact(dt: DateTime<Local>) -> String {
    let now = Local::now();
    if dt.date_naive() == now.date_naive() {
        dt.format("%H:%M").to_string()
    } else if dt.year() == now.year() {
        dt.format("%b %d %H:%M").to_string()
    } else {
        dt.format("%Y-%m-%d").to_string()
    }
}

/// Format an uptime duration as H:MM:SS.
pub fn format_uptime(start: DateTime<Local>) -> String {
    let elapsed = Local::now().signed_duration_since(start);
    let total_secs = elapsed.num_seconds();
    let h = total_secs / 3600;
    let m = (total_secs % 3600) / 60;
    let s = total_secs % 60;
    format!("{h}:{m:02}:{s:02}")
}

/// Render a 24-char hourly activity sparkline from counts per hour (0–23).
pub fn format_hourly_sparkline(counts: &[u32; 24]) -> String {
    let max = *counts.iter().max().unwrap_or(&0) as f64;
    if max == 0.0 {
        return String::new();
    }
    let bars = [' ', '\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}', '\u{2588}'];
    counts.iter().map(|&c| {
        if c == 0 {
            ' '
        } else {
            let norm = (c as f64 / max * 8.0) as usize;
            bars[norm.clamp(1, 8)]
        }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::ScanResult;
    use chrono::Duration;
    use std::collections::HashMap;

    // ── estimate_distance ────────────────────────────────────────────────

    #[test]
    fn distance_default_ref_power() {
        // No tx_power, no ibeacon → default -59 dBm ref
        let d = estimate_distance(-59, None, None);
        assert!((d - 1.0).abs() < 0.01, "At ref power, distance should be ~1m, got {d}");
    }

    #[test]
    fn distance_tx_power_ignored() {
        // tx_power is not calibrated at 1m, so it should be ignored (uses default -59)
        let d_with_tx = estimate_distance(-59, Some(12), None);
        let d_without_tx = estimate_distance(-59, None, None);
        assert!((d_with_tx - d_without_tx).abs() < 0.01, "tx_power should not affect distance");
    }

    #[test]
    fn distance_ibeacon_takes_priority() {
        // ibeacon_power should override tx_power
        let d_ibeacon = estimate_distance(-70, Some(10), Some(-70));
        assert!((d_ibeacon - 1.0).abs() < 0.01, "iBeacon power should be used as ref");
    }

    #[test]
    fn distance_stronger_rssi_means_closer() {
        let close = estimate_distance(-50, None, None);
        let far = estimate_distance(-80, None, None);
        assert!(close < far);
    }

    // ── format_distance ──────────────────────────────────────────────────

    #[test]
    fn format_distance_none_rssi() {
        assert_eq!(format_distance(None, None, None), "?");
    }

    #[test]
    fn format_distance_close() {
        // RSSI very close to ref → <1m
        assert_eq!(format_distance(Some(-55), None, None), "<1m");
    }

    #[test]
    fn format_distance_medium() {
        let s = format_distance(Some(-70), None, None);
        assert!(s.starts_with('~'));
        assert!(s.ends_with('m'));
    }

    #[test]
    fn format_distance_far() {
        let s = format_distance(Some(-95), None, None);
        assert!(s.starts_with('~'));
        assert!(s.ends_with('m'));
    }

    // ── format_uptime ────────────────────────────────────────────────────

    #[test]
    fn format_uptime_zero() {
        let now = Local::now();
        let s = format_uptime(now);
        assert_eq!(s, "0:00:00");
    }

    #[test]
    fn format_uptime_one_hour() {
        let start = Local::now() - Duration::seconds(3661);
        let s = format_uptime(start);
        assert_eq!(s, "1:01:01");
    }

    // ── App::upsert_device ───────────────────────────────────────────────

    fn make_scan_result(mac: &str) -> ScanResult {
        ScanResult {
            mac: mac.to_string(),
            name: None,
            rssi: Some(-65),
            tx_power: None,
            vendor: Some("TestVendor".into()),
            device_type: DeviceType::Unknown,
            service_uuids: vec![],
            is_randomized: false,
            manufacturer_data: HashMap::new(),
            device_class: None,
            fingerprint: "ABCD".to_string(),
        }
    }

    #[test]
    fn upsert_new_device() {
        let mut app = App::new(":memory:".into());
        app.upsert_device(make_scan_result("AA:BB:CC:DD:EE:FF"));
        assert!(app.devices.contains_key("AA:BB:CC:DD:EE:FF"));
        assert_eq!(app.devices["AA:BB:CC:DD:EE:FF"].sightings, 1);
    }

    #[test]
    fn upsert_preserves_first_seen() {
        let mut app = App::new(":memory:".into());
        app.upsert_device(make_scan_result("AA:BB:CC:DD:EE:FF"));
        let first_seen = app.devices["AA:BB:CC:DD:EE:FF"].first_seen;

        // Second upsert should not change first_seen
        app.upsert_device(make_scan_result("AA:BB:CC:DD:EE:FF"));
        assert_eq!(app.devices["AA:BB:CC:DD:EE:FF"].first_seen, first_seen);
        assert_eq!(app.devices["AA:BB:CC:DD:EE:FF"].sightings, 2);
    }

    #[test]
    fn upsert_updates_name_when_some() {
        let mut app = App::new(":memory:".into());
        app.upsert_device(make_scan_result("AA:BB:CC:DD:EE:FF"));
        assert!(app.devices["AA:BB:CC:DD:EE:FF"].name.is_none());

        let mut r = make_scan_result("AA:BB:CC:DD:EE:FF");
        r.name = Some("MyDevice".into());
        app.upsert_device(r);
        assert_eq!(app.devices["AA:BB:CC:DD:EE:FF"].name.as_deref(), Some("MyDevice"));
    }

    #[test]
    fn upsert_does_not_clear_name_with_none() {
        let mut app = App::new(":memory:".into());
        let mut r = make_scan_result("AA:BB:CC:DD:EE:FF");
        r.name = Some("MyDevice".into());
        app.upsert_device(r);

        // Second upsert with name=None should preserve existing name
        app.upsert_device(make_scan_result("AA:BB:CC:DD:EE:FF"));
        assert_eq!(app.devices["AA:BB:CC:DD:EE:FF"].name.as_deref(), Some("MyDevice"));
    }

    #[test]
    fn upsert_tracks_fingerprint_groups() {
        let mut app = App::new(":memory:".into());
        app.upsert_device(make_scan_result("AA:BB:CC:DD:EE:01"));
        app.upsert_device(make_scan_result("AA:BB:CC:DD:EE:02"));
        // Both devices share fingerprint "ABCD"
        let group = app.fingerprint_groups.get("ABCD").unwrap();
        assert!(group.contains("AA:BB:CC:DD:EE:01"));
        assert!(group.contains("AA:BB:CC:DD:EE:02"));
    }

    // ── rebuild_fingerprint_groups ───────────────────────────────────────

    #[test]
    fn rebuild_fingerprint_groups_from_devices() {
        let mut app = App::new(":memory:".into());
        let now = Local::now();
        app.devices.insert("MAC1".into(), DeviceInfo {
            mac: "MAC1".into(), name: None, rssi: None, tx_power: None,
            vendor: None, device_type: DeviceType::Unknown, service_uuids: vec![],
            sightings: 1, first_seen: now, last_seen: now, is_randomized: false,
            note: None, fingerprint: "FP01".into(), manufacturer_data: HashMap::new(),
            continuity: None, gatt_info: None, fast_pair_model: None,
            device_class: None, addr_type: None,
        });
        app.devices.insert("MAC2".into(), DeviceInfo {
            mac: "MAC2".into(), name: None, rssi: None, tx_power: None,
            vendor: None, device_type: DeviceType::Unknown, service_uuids: vec![],
            sightings: 1, first_seen: now, last_seen: now, is_randomized: false,
            note: None, fingerprint: "FP01".into(), manufacturer_data: HashMap::new(),
            continuity: None, gatt_info: None, fast_pair_model: None,
            device_class: None, addr_type: None,
        });
        app.devices.insert("MAC3".into(), DeviceInfo {
            mac: "MAC3".into(), name: None, rssi: None, tx_power: None,
            vendor: None, device_type: DeviceType::Unknown, service_uuids: vec![],
            sightings: 1, first_seen: now, last_seen: now, is_randomized: false,
            note: None, fingerprint: "".into(), manufacturer_data: HashMap::new(),
            continuity: None, gatt_info: None, fast_pair_model: None,
            device_class: None, addr_type: None,
        });

        app.rebuild_fingerprint_groups();

        // MAC1 and MAC2 share FP01
        let group = app.fingerprint_groups.get("FP01").unwrap();
        assert_eq!(group.len(), 2);
        // MAC3 has empty fingerprint → uses MAC as key
        assert!(app.fingerprint_groups.get("MAC3").unwrap().contains("MAC3"));
    }

    // ── save_note / cancel_note ──────────────────────────────────────────

    #[test]
    fn save_note_attaches_to_device() {
        let mut app = App::new(":memory:".into());
        app.upsert_device(make_scan_result("AA:BB:CC:DD:EE:FF"));

        app.note_mode = true;
        app.note_mac = "AA:BB:CC:DD:EE:FF".into();
        app.note_input = "test note".into();
        let mac = app.save_note();
        assert_eq!(mac, Some("AA:BB:CC:DD:EE:FF".to_string()));
        assert_eq!(app.devices["AA:BB:CC:DD:EE:FF"].note.as_deref(), Some("test note"));
        assert!(!app.note_mode);
    }

    #[test]
    fn save_empty_note_clears() {
        let mut app = App::new(":memory:".into());
        app.upsert_device(make_scan_result("AA:BB:CC:DD:EE:FF"));
        app.devices.get_mut("AA:BB:CC:DD:EE:FF").unwrap().note = Some("old".into());

        app.note_mode = true;
        app.note_mac = "AA:BB:CC:DD:EE:FF".into();
        app.note_input = "".into();
        app.save_note();
        assert!(app.devices["AA:BB:CC:DD:EE:FF"].note.is_none());
    }

    // ── format_hourly_sparkline ───────────────────────────────────────────

    #[test]
    fn sparkline_empty_counts() {
        assert_eq!(format_hourly_sparkline(&[0; 24]), "");
    }

    #[test]
    fn sparkline_single_hour() {
        let mut counts = [0u32; 24];
        counts[10] = 5;
        let s = format_hourly_sparkline(&counts);
        assert_eq!(s.chars().count(), 24);
        // Hour 10 should be full bar, others should be spaces
        let chars: Vec<char> = s.chars().collect();
        assert_eq!(chars[10], '\u{2588}');
        assert_eq!(chars[0], ' ');
    }

    #[test]
    fn sparkline_all_equal() {
        let counts = [10u32; 24];
        let s = format_hourly_sparkline(&counts);
        assert_eq!(s.chars().count(), 24);
        // All bars should be full height
        assert!(s.chars().all(|c| c == '\u{2588}'));
    }

    #[test]
    fn cancel_note_clears_state() {
        let mut app = App::new(":memory:".into());
        app.note_mode = true;
        app.note_mac = "AA:BB:CC:DD:EE:FF".into();
        app.note_input = "test".into();
        app.cancel_note();
        assert!(!app.note_mode);
        assert!(app.note_input.is_empty());
        assert!(app.note_mac.is_empty());
    }
}
