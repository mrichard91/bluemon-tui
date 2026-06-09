//! SQLite persistence layer for device and observation data.
//!
//! Handles schema creation, migrations, reference data seeding from compiled-in CSVs,
//! and all read/write operations. Uses WAL mode for concurrent scan writes and chat reads.

use crate::app::{self, DeviceInfo};
use crate::classifier::{self, DeviceType};
use crate::continuity::ContinuityData;
use crate::gatt::GattDeviceInfo;
use crate::scanner::ScanResult;
use chrono::{DateTime, Local};
use rusqlite::{params, Connection};
use std::collections::HashMap;

/// Open (or create) the database and run migrations.
pub fn open(path: &str) -> anyhow::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;

         CREATE TABLE IF NOT EXISTS devices (
             mac TEXT PRIMARY KEY,
             name TEXT,
             vendor TEXT,
             device_type TEXT NOT NULL DEFAULT 'unknown',
             is_randomized INTEGER NOT NULL DEFAULT 0,
             first_seen TEXT NOT NULL,
             last_seen TEXT NOT NULL,
             note TEXT DEFAULT '',
             service_uuids TEXT DEFAULT '',
             sightings INTEGER NOT NULL DEFAULT 0,
             tx_power INTEGER
         );

         CREATE TABLE IF NOT EXISTS observations (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             mac TEXT NOT NULL,
             seen_at TEXT NOT NULL,
             rssi INTEGER,
             name TEXT,
             service_uuids TEXT DEFAULT '',
             fingerprint TEXT DEFAULT ''
         );

         CREATE TABLE IF NOT EXISTS app_settings (
             key TEXT PRIMARY KEY,
             value TEXT NOT NULL DEFAULT ''
         );

         CREATE INDEX IF NOT EXISTS idx_obs_mac ON observations(mac);
         CREATE INDEX IF NOT EXISTS idx_obs_seen ON observations(seen_at);",
    )?;

    // Migrate older databases that lack newer columns
    ensure_column(
        &conn,
        "devices",
        "tx_power",
        "ALTER TABLE devices ADD COLUMN tx_power INTEGER;",
    )?;
    ensure_column(
        &conn,
        "devices",
        "last_rssi",
        "ALTER TABLE devices ADD COLUMN last_rssi INTEGER;",
    )?;
    ensure_column(
        &conn,
        "devices",
        "fingerprint",
        "ALTER TABLE devices ADD COLUMN fingerprint TEXT DEFAULT '';",
    )?;
    ensure_column(
        &conn,
        "devices",
        "continuity_json",
        "ALTER TABLE devices ADD COLUMN continuity_json TEXT DEFAULT '';
         ALTER TABLE devices ADD COLUMN gatt_info_json TEXT DEFAULT '';
         ALTER TABLE devices ADD COLUMN fast_pair_model TEXT DEFAULT '';",
    )?;
    ensure_column(
        &conn,
        "devices",
        "device_class",
        "ALTER TABLE devices ADD COLUMN device_class INTEGER;",
    )?;
    ensure_column(
        &conn,
        "devices",
        "addr_type",
        "ALTER TABLE devices ADD COLUMN addr_type TEXT DEFAULT '';",
    )?;
    ensure_column(
        &conn,
        "devices",
        "service_data_json",
        "ALTER TABLE devices ADD COLUMN service_data_json TEXT DEFAULT '';",
    )?;
    ensure_column(
        &conn,
        "observations",
        "fingerprint",
        "ALTER TABLE observations ADD COLUMN fingerprint TEXT DEFAULT '';",
    )?;

    seed_reference_data(&conn)?;

    Ok(conn)
}

/// Add a column to a table if it doesn't already exist.
fn ensure_column(conn: &Connection, table: &str, column: &str, ddl: &str) -> anyhow::Result<()> {
    if conn
        .prepare(&format!("SELECT {column} FROM {table} LIMIT 0"))
        .is_err()
    {
        conn.execute_batch(ddl)?;
    }
    Ok(())
}

fn seed_reference_data(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS ref_service_uuids (
             prefix TEXT PRIMARY KEY,
             name TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS ref_fast_pair_models (
             model_id TEXT PRIMARY KEY,
             name TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS ref_bt_company_ids (
             company_id TEXT PRIMARY KEY,
             name TEXT NOT NULL,
             device_type TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS ref_ibeacon_uuids (
             uuid TEXT PRIMARY KEY,
             vendor TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS ref_airpods_models (
             model_id TEXT PRIMARY KEY,
             name TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS ref_homekit_categories (
             category_id INTEGER PRIMARY KEY,
             name TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS ref_nearby_actions (
             action_id TEXT PRIMARY KEY,
             name TEXT NOT NULL
         );",
    )?;

    let csvs: &[(&str, &str, usize)] = &[
        (
            include_str!("../data/service_uuids.csv"),
            "ref_service_uuids",
            2,
        ),
        (
            include_str!("../data/fast_pair_models.csv"),
            "ref_fast_pair_models",
            2,
        ),
        (
            include_str!("../data/bt_company_ids.csv"),
            "ref_bt_company_ids",
            3,
        ),
        (
            include_str!("../data/ibeacon_uuids.csv"),
            "ref_ibeacon_uuids",
            2,
        ),
        (
            include_str!("../data/airpods_models.csv"),
            "ref_airpods_models",
            2,
        ),
        (
            include_str!("../data/homekit_categories.csv"),
            "ref_homekit_categories",
            2,
        ),
        (
            include_str!("../data/nearby_actions.csv"),
            "ref_nearby_actions",
            2,
        ),
    ];

    for &(csv, table, cols) in csvs.iter() {
        let placeholders = (1..=cols)
            .map(|n| format!("?{n}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!("INSERT OR IGNORE INTO {table} VALUES ({placeholders})");
        let mut stmt = conn.prepare_cached(&sql)?;

        for line in csv.lines().skip(1) {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let fields: Vec<&str> = line.splitn(cols, ',').collect();
            if fields.len() < cols {
                continue;
            }
            match cols {
                2 => {
                    stmt.execute(params![fields[0], fields[1]])?;
                }
                3 => {
                    stmt.execute(params![fields[0], fields[1], fields[2]])?;
                }
                _ => {}
            }
        }
    }

    Ok(())
}

/// Deserialize an optional JSON string into a typed value, returning None on empty/invalid.
fn deserialize_json_field<T: serde::de::DeserializeOwned>(json_str: Option<String>) -> Option<T> {
    json_str
        .as_deref()
        .filter(|s| !s.is_empty())
        .and_then(|s| serde_json::from_str::<T>(s).ok())
}

/// Serialize an optional value to a JSON string, returning "" if None.
fn serialize_json_field<T: serde::Serialize>(value: Option<&T>) -> String {
    value
        .and_then(|v| serde_json::to_string(v).ok())
        .unwrap_or_default()
}

/// Decode `continuity_json` into a Vec, accepting either the new array form
/// `[{...}, {...}]` or the legacy single-object form `{...}` written before
/// multi-TLV support landed.
fn deserialize_continuity_field(json_str: Option<String>) -> Vec<ContinuityData> {
    let Some(s) = json_str.filter(|s| !s.is_empty()) else {
        return Vec::new();
    };
    if let Ok(v) = serde_json::from_str::<Vec<ContinuityData>>(&s) {
        return v;
    }
    if let Ok(one) = serde_json::from_str::<ContinuityData>(&s) {
        return vec![one];
    }
    Vec::new()
}

/// Serialize parsed service-data payloads to a JSON map of UUID → hex string.
fn serialize_service_data(map: &HashMap<String, Vec<u8>>) -> String {
    if map.is_empty() {
        return String::new();
    }
    let hex: HashMap<String, String> = map
        .iter()
        .map(|(uuid, bytes)| {
            let s = bytes
                .iter()
                .map(|b| format!("{:02X}", b))
                .collect::<String>();
            (uuid.clone(), s)
        })
        .collect();
    serde_json::to_string(&hex).unwrap_or_default()
}

/// Decode the JSON map written by `serialize_service_data` back into bytes.
fn deserialize_service_data(json_str: Option<String>) -> HashMap<String, Vec<u8>> {
    let Some(s) = json_str.filter(|s| !s.is_empty()) else {
        return HashMap::new();
    };
    let hex_map: HashMap<String, String> = match serde_json::from_str(&s) {
        Ok(m) => m,
        Err(_) => return HashMap::new(),
    };
    hex_map
        .into_iter()
        .filter_map(|(uuid, hex)| decode_hex(&hex).map(|bytes| (uuid, bytes)))
        .collect()
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for i in (0..s.len()).step_by(2) {
        out.push(u8::from_str_radix(&s[i..i + 2], 16).ok()?);
    }
    Some(out)
}

/// Load all devices from the database into a HashMap keyed by MAC address.
pub fn load_devices(conn: &Connection) -> anyhow::Result<HashMap<String, DeviceInfo>> {
    let mut stmt = conn.prepare(
        "SELECT mac, name, vendor, device_type, is_randomized,
                first_seen, last_seen, note, service_uuids, sightings, tx_power, fingerprint,
                continuity_json, gatt_info_json, fast_pair_model, last_rssi,
                device_class, addr_type, service_data_json
         FROM devices",
    )?;
    let mut devices = HashMap::new();
    let mut rows = stmt.query([])?;

    while let Some(row) = rows.next()? {
        let mac: String = row.get(0)?;
        let name: Option<String> = row.get(1)?;
        let vendor: Option<String> = row.get(2)?;
        let dtype_str: String = row.get(3)?;
        let is_randomized: bool = row.get(4)?;
        let first_str: String = row.get(5)?;
        let last_str: String = row.get(6)?;
        let note: Option<String> = row.get(7)?;
        let uuids_str: Option<String> = row.get(8)?;
        let sightings: u32 = row.get(9)?;
        let tx_power: Option<i16> = row.get(10)?;
        let fingerprint: Option<String> = row.get(11)?;
        let continuity_json: Option<String> = row.get(12)?;
        let gatt_info_json: Option<String> = row.get(13)?;
        let fast_pair_model: Option<String> = row.get(14)?;
        let last_rssi: Option<i16> = row.get(15)?;
        let device_class: Option<u32> = row.get(16)?;
        let addr_type_str: Option<String> = row.get(17)?;
        let service_data_json: Option<String> = row.get(18)?;

        let device_type = DeviceType::from_db(&dtype_str);
        let first_seen = DateTime::parse_from_rfc3339(&first_str)
            .map(|dt| dt.with_timezone(&Local))
            .unwrap_or_else(|_| Local::now());
        let last_seen = DateTime::parse_from_rfc3339(&last_str)
            .map(|dt| dt.with_timezone(&Local))
            .unwrap_or_else(|_| Local::now());
        let service_uuids: Vec<String> = uuids_str
            .unwrap_or_default()
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        let note = note.filter(|n| !n.is_empty());
        let continuity = deserialize_continuity_field(continuity_json);
        let gatt_info = deserialize_json_field::<GattDeviceInfo>(gatt_info_json);
        let fast_pair_model = fast_pair_model.filter(|s| !s.is_empty());
        let service_data = deserialize_service_data(service_data_json);
        let eddystone = crate::eddystone::from_service_data(&service_data);
        let vendor = classifier::refine_vendor(
            vendor.as_deref().filter(|s| !s.is_empty()),
            &continuity,
            fast_pair_model.as_deref(),
        );
        let addr_type = addr_type_str
            .as_deref()
            .filter(|s| !s.is_empty())
            .and_then(classifier::BleAddrType::from_db)
            .or_else(|| classifier::parse_addr_type(&mac));

        devices.insert(
            mac.clone(),
            DeviceInfo {
                mac,
                name,
                rssi: last_rssi,
                tx_power,
                vendor,
                device_type,
                service_uuids,
                sightings,
                first_seen,
                last_seen,
                is_randomized,
                note,
                fingerprint: fingerprint.unwrap_or_default(),
                manufacturer_data: HashMap::new(),
                service_data,
                continuity,
                eddystone,
                gatt_info,
                fast_pair_model,
                device_class,
                addr_type,
            },
        );
    }

    Ok(devices)
}

/// Observation data captured at scan time for batch writing.
pub struct PendingObs {
    pub mac: String,
    /// Signal strength in dBm (closer to 0 = stronger).
    pub rssi: Option<i16>,
    pub name: Option<String>,
    /// Comma-separated list of advertised BLE service UUIDs.
    pub service_uuids: String,
    /// 4-char hex hash identifying the physical device.
    pub fingerprint: String,
}

impl PendingObs {
    pub fn from_scan_result(result: &ScanResult) -> Self {
        Self {
            mac: result.mac.clone(),
            rssi: result.rssi,
            name: result.name.clone(),
            service_uuids: result.service_uuids.join(","),
            fingerprint: app::effective_key(&result.fingerprint, &result.mac),
        }
    }
}

/// Write a full scan cycle to the database in a single transaction:
/// insert observations and upsert device records.
#[allow(dead_code)]
pub fn write_cycle(
    conn: &Connection,
    devices: &HashMap<String, DeviceInfo>,
    observations: &[PendingObs],
) -> anyhow::Result<()> {
    write_cycle_at(conn, devices, observations, &Local::now().to_rfc3339())
}

/// Write a full scan cycle using a caller-supplied observation timestamp.
pub fn write_cycle_at(
    conn: &Connection,
    devices: &HashMap<String, DeviceInfo>,
    observations: &[PendingObs],
    seen_at: &str,
) -> anyhow::Result<()> {
    let tx = conn.unchecked_transaction()?;

    {
        let mut obs_stmt = tx.prepare_cached(
            "INSERT INTO observations (mac, seen_at, rssi, name, service_uuids, fingerprint)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        for obs in observations {
            obs_stmt.execute(params![
                obs.mac,
                seen_at,
                obs.rssi,
                obs.name,
                obs.service_uuids,
                obs.fingerprint,
            ])?;
        }
    }

    {
        let mut dev_stmt = tx.prepare_cached(
            "INSERT INTO devices (mac, name, vendor, device_type, is_randomized,
                                  first_seen, last_seen, note, service_uuids, sightings, tx_power, fingerprint,
                                  continuity_json, gatt_info_json, fast_pair_model, last_rssi,
                                  device_class, addr_type, service_data_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
             ON CONFLICT(mac) DO UPDATE SET
                 name = COALESCE(?2, name),
                 vendor = COALESCE(?3, vendor),
                 device_type = ?4,
                 last_seen = ?7,
                 service_uuids = CASE WHEN ?9 = '' THEN service_uuids ELSE ?9 END,
                 sightings = ?10,
                 tx_power = COALESCE(?11, tx_power),
                 fingerprint = ?12,
                 continuity_json = CASE WHEN ?13 = '' THEN continuity_json ELSE ?13 END,
                 gatt_info_json = CASE WHEN ?14 = '' THEN gatt_info_json ELSE ?14 END,
                 fast_pair_model = CASE WHEN ?15 = '' THEN fast_pair_model ELSE ?15 END,
                 last_rssi = COALESCE(?16, last_rssi),
                 device_class = COALESCE(?17, device_class),
                 addr_type = CASE WHEN ?18 = '' THEN addr_type ELSE ?18 END,
                 service_data_json = CASE WHEN ?19 = '' THEN service_data_json ELSE ?19 END",
        )?;

        let mut written = std::collections::HashSet::new();
        for obs in observations {
            if !written.insert(&obs.mac) {
                continue;
            }
            if let Some(d) = devices.get(&obs.mac) {
                let continuity_json = if d.continuity.is_empty() {
                    String::new()
                } else {
                    serde_json::to_string(&d.continuity).unwrap_or_default()
                };
                let gatt_info_json = serialize_json_field(d.gatt_info.as_ref());
                let fast_pair_model = d.fast_pair_model.as_deref().unwrap_or("");
                let service_data_json = serialize_service_data(&d.service_data);

                dev_stmt.execute(params![
                    d.mac,
                    d.name,
                    d.vendor,
                    d.device_type.to_db(),
                    d.is_randomized,
                    d.first_seen.to_rfc3339(),
                    d.last_seen.to_rfc3339(),
                    d.note.as_deref().unwrap_or(""),
                    d.service_uuids.join(","),
                    d.sightings,
                    d.tx_power,
                    d.fingerprint,
                    continuity_json,
                    gatt_info_json,
                    fast_pair_model,
                    d.rssi,
                    d.device_class,
                    d.addr_type.map(|a| a.to_db()).unwrap_or(""),
                    service_data_json,
                ])?;
            }
        }
    }

    tx.commit()?;
    Ok(())
}

/// Persist GATT Device Information Service data for a specific MAC.
pub fn update_gatt_info(conn: &Connection, mac: &str, info: &GattDeviceInfo) -> anyhow::Result<()> {
    let json = serde_json::to_string(info)?;
    conn.execute(
        "UPDATE devices SET gatt_info_json = ?1 WHERE mac = ?2",
        params![json, mac],
    )?;
    Ok(())
}

/// Update the note for all devices sharing a fingerprint (or a single MAC).
pub fn update_note_group(
    conn: &Connection,
    fingerprint: Option<&str>,
    mac: &str,
    note: &str,
) -> anyhow::Result<()> {
    if let Some(fp) = fingerprint {
        conn.execute(
            "UPDATE devices SET note = ?1 WHERE fingerprint = ?2",
            params![note, fp],
        )?;
    } else {
        conn.execute(
            "UPDATE devices SET note = ?1 WHERE mac = ?2",
            params![note, mac],
        )?;
    }
    Ok(())
}

/// Upsert a key/value setting; passing None deletes the key.
pub fn save_setting(conn: &Connection, key: &str, value: Option<&str>) -> anyhow::Result<()> {
    match value.filter(|v| !v.is_empty()) {
        Some(value) => {
            conn.execute(
                "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
        }
        None => {
            conn.execute("DELETE FROM app_settings WHERE key = ?1", params![key])?;
        }
    }
    Ok(())
}

/// Fetch recent RSSI readings for a set of MACs (ordered oldest → newest).
pub fn recent_rssi(conn: &Connection, macs: &[String], limit: usize) -> anyhow::Result<Vec<i16>> {
    if macs.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = macs.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT rssi FROM (
            SELECT rssi, seen_at FROM observations
            WHERE mac IN ({placeholders}) AND rssi IS NOT NULL
            ORDER BY seen_at DESC LIMIT ?
        ) ORDER BY seen_at ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut values: Vec<i16> = Vec::new();
    let params: Vec<Box<dyn rusqlite::types::ToSql>> = macs
        .iter()
        .map(|m| Box::new(m.clone()) as Box<dyn rusqlite::types::ToSql>)
        .chain(std::iter::once(
            Box::new(limit as i64) as Box<dyn rusqlite::types::ToSql>
        ))
        .collect();
    let refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut rows = stmt.query(refs.as_slice())?;
    while let Some(row) = rows.next()? {
        values.push(row.get(0)?);
    }
    Ok(values)
}

/// Time window applied to hourly-activity sparklines. The detail/table sparkline
/// is meant to show *recent* time-of-day patterns; widening this past a few weeks
/// blurs schedule changes and slows the query.
pub const ACTIVITY_WINDOW: &str = "-7 days";

/// Count distinct scan cycles per hour-of-day (0–23) for a set of MACs over the
/// last `ACTIVITY_WINDOW`. Counting cycles (not raw observations) keeps the
/// magnitude consistent with the in-memory cache, which adds 1 per fingerprint
/// per cycle regardless of how many MACs in the group fired in that cycle.
pub fn hourly_activity(conn: &Connection, macs: &[String]) -> anyhow::Result<[u32; 24]> {
    let mut counts = [0u32; 24];
    if macs.is_empty() {
        return Ok(counts);
    }
    let placeholders = macs.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT CAST(strftime('%H', seen_at, 'localtime') AS INTEGER) AS hour,
                COUNT(DISTINCT seen_at) AS cnt
         FROM observations
         WHERE mac IN ({placeholders})
           AND seen_at > datetime('now', '{ACTIVITY_WINDOW}')
         GROUP BY hour"
    );
    let mut stmt = conn.prepare(&sql)?;
    let params: Vec<Box<dyn rusqlite::types::ToSql>> = macs
        .iter()
        .map(|m| Box::new(m.clone()) as Box<dyn rusqlite::types::ToSql>)
        .collect();
    let refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut rows = stmt.query(refs.as_slice())?;
    while let Some(row) = rows.next()? {
        let hour: u32 = row.get(0)?;
        let count: u32 = row.get(1)?;
        if (hour as usize) < 24 {
            counts[hour as usize] = count;
        }
    }
    Ok(counts)
}

/// MAC address rotation statistics for a fingerprinted device group.
pub struct MacRotationStats {
    /// Total distinct MAC addresses in the group.
    pub total_macs: usize,
    /// Average minutes between consecutive MAC rotation events (None if ≤1 MAC).
    pub avg_rotation_mins: Option<f64>,
}

pub fn mac_rotation_stats(conn: &Connection, macs: &[String]) -> anyhow::Result<MacRotationStats> {
    if macs.len() <= 1 {
        return Ok(MacRotationStats {
            total_macs: macs.len(),
            avg_rotation_mins: None,
        });
    }
    let placeholders = macs.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT first_seen FROM devices WHERE mac IN ({placeholders}) ORDER BY first_seen ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let params: Vec<Box<dyn rusqlite::types::ToSql>> = macs
        .iter()
        .map(|m| Box::new(m.clone()) as Box<dyn rusqlite::types::ToSql>)
        .collect();
    let refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut rows = stmt.query(refs.as_slice())?;
    let mut timestamps: Vec<DateTime<Local>> = Vec::new();
    while let Some(row) = rows.next()? {
        let ts: String = row.get(0)?;
        if let Ok(dt) = DateTime::parse_from_rfc3339(&ts) {
            timestamps.push(dt.with_timezone(&Local));
        }
    }
    if timestamps.len() <= 1 {
        return Ok(MacRotationStats {
            total_macs: macs.len(),
            avg_rotation_mins: None,
        });
    }
    let total_mins: f64 = timestamps
        .windows(2)
        .map(|w| (w[1] - w[0]).num_seconds().abs() as f64 / 60.0)
        .sum();
    let avg = total_mins / (timestamps.len() - 1) as f64;
    Ok(MacRotationStats {
        total_macs: macs.len(),
        avg_rotation_mins: Some(avg),
    })
}

/// Load hourly activity counts for all devices, grouped by fingerprint, over
/// the last `ACTIVITY_WINDOW`. Counts distinct scan cycles per (fingerprint, hour)
/// so the magnitude matches the live in-memory cache, which adds 1 per
/// fingerprint per cycle.
pub fn bulk_hourly_activity(conn: &Connection) -> anyhow::Result<HashMap<String, [u32; 24]>> {
    let mut result: HashMap<String, [u32; 24]> = HashMap::new();
    let sql = format!(
        "SELECT COALESCE(NULLIF(o.fingerprint, ''), o.mac) AS key,
                CAST(strftime('%H', o.seen_at, 'localtime') AS INTEGER) AS hour,
                COUNT(DISTINCT o.seen_at) AS cnt
         FROM observations o
         WHERE o.seen_at > datetime('now', '{ACTIVITY_WINDOW}')
         GROUP BY key, hour"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let key: String = row.get(0)?;
        let hour: u32 = row.get(1)?;
        let cnt: u32 = row.get(2)?;
        if (hour as usize) < 24 {
            let entry = result.entry(key).or_insert([0u32; 24]);
            entry[hour as usize] = cnt;
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gatt::GattDeviceInfo;

    // ── open ─────────────────────────────────────────────────────────────

    #[test]
    fn open_creates_schema() {
        let conn = open(":memory:").unwrap();
        // Both tables should exist
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('devices', 'observations')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn open_idempotent() {
        // Opening twice on the same path should not fail
        let conn = open(":memory:").unwrap();
        // Re-run the same schema DDL (simulates a second open)
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS devices (mac TEXT PRIMARY KEY);
             CREATE TABLE IF NOT EXISTS observations (id INTEGER PRIMARY KEY);",
        )
        .unwrap();
    }

    // ── write_cycle + load_devices round-trip ────────────────────────────

    fn make_test_device(mac: &str) -> DeviceInfo {
        DeviceInfo {
            mac: mac.to_string(),
            name: Some("TestDevice".into()),
            rssi: Some(-65),
            tx_power: Some(4),
            vendor: Some("TestVendor".into()),
            device_type: DeviceType::Audio,
            service_uuids: vec!["0000180a".into(), "0000180f".into()],
            sightings: 3,
            first_seen: DateTime::parse_from_rfc3339("2025-06-15T14:30:00+00:00")
                .unwrap()
                .with_timezone(&Local),
            last_seen: DateTime::parse_from_rfc3339("2025-06-15T15:00:00+00:00")
                .unwrap()
                .with_timezone(&Local),
            is_randomized: true,
            note: Some("test note".into()),
            fingerprint: "A1B2".into(),
            manufacturer_data: HashMap::new(),
            service_data: HashMap::new(),
            continuity: Vec::new(),
            eddystone: None,
            gatt_info: None,
            fast_pair_model: Some("Pixel Buds".into()),
            device_class: Some(0x200404),
            addr_type: Some(classifier::BleAddrType::RandomStatic),
        }
    }

    #[test]
    fn write_and_load_round_trip() {
        let conn = open(":memory:").unwrap();

        let mut devices = HashMap::new();
        devices.insert(
            "AA:BB:CC:DD:EE:01".into(),
            make_test_device("AA:BB:CC:DD:EE:01"),
        );

        let observations = vec![PendingObs {
            mac: "AA:BB:CC:DD:EE:01".into(),
            rssi: Some(-65),
            name: Some("TestDevice".into()),
            service_uuids: "0000180a,0000180f".into(),
            fingerprint: "A1B2".into(),
        }];

        write_cycle(&conn, &devices, &observations).unwrap();

        let loaded = load_devices(&conn).unwrap();
        assert_eq!(loaded.len(), 1);
        let dev = &loaded["AA:BB:CC:DD:EE:01"];
        assert_eq!(dev.name.as_deref(), Some("TestDevice"));
        assert_eq!(dev.vendor.as_deref(), Some("Google"));
        assert_eq!(dev.device_type, DeviceType::Audio);
        assert!(dev.is_randomized);
        assert_eq!(dev.sightings, 3);
        assert_eq!(dev.tx_power, Some(4));
        assert_eq!(dev.rssi, Some(-65));
        assert_eq!(dev.fingerprint, "A1B2");
        assert_eq!(dev.fast_pair_model.as_deref(), Some("Pixel Buds"));
        assert_eq!(dev.service_uuids, vec!["0000180a", "0000180f"]);
        assert_eq!(dev.note.as_deref(), Some("test note"));
        assert_eq!(dev.device_class, Some(0x200404));
        assert_eq!(dev.addr_type, Some(classifier::BleAddrType::RandomStatic));
    }

    #[test]
    fn load_devices_refines_vendor_from_continuity() {
        let conn = open(":memory:").unwrap();

        let mut devices = HashMap::new();
        let mut dev = make_test_device("00:17:9A:11:22:33");
        dev.vendor = Some("D-Link".into());
        dev.fast_pair_model = None;
        dev.continuity = vec![ContinuityData::AirPods {
            device_model: 0x0220,
            battery_left: Some(8),
            battery_right: Some(10),
            battery_case: Some(5),
            charging_left: false,
            charging_right: false,
            charging_case: false,
            lid_open: true,
        }];
        devices.insert(dev.mac.clone(), dev);

        let observations = vec![PendingObs {
            mac: "00:17:9A:11:22:33".into(),
            rssi: Some(-65),
            name: Some("AirPods".into()),
            service_uuids: String::new(),
            fingerprint: "A1B2".into(),
        }];

        write_cycle(&conn, &devices, &observations).unwrap();

        let loaded = load_devices(&conn).unwrap();
        assert_eq!(loaded["00:17:9A:11:22:33"].vendor.as_deref(), Some("Apple"));
    }

    #[test]
    fn write_cycle_multiple_devices() {
        let conn = open(":memory:").unwrap();

        let mut devices = HashMap::new();
        devices.insert("MAC1".into(), make_test_device("MAC1"));
        devices.insert("MAC2".into(), make_test_device("MAC2"));

        let observations = vec![
            PendingObs {
                mac: "MAC1".into(),
                rssi: Some(-60),
                name: None,
                service_uuids: String::new(),
                fingerprint: "A1B2".into(),
            },
            PendingObs {
                mac: "MAC2".into(),
                rssi: Some(-70),
                name: None,
                service_uuids: String::new(),
                fingerprint: "A1B2".into(),
            },
        ];

        write_cycle(&conn, &devices, &observations).unwrap();
        let loaded = load_devices(&conn).unwrap();
        assert_eq!(loaded.len(), 2);
    }

    // ── update_note_group ────────────────────────────────────────────────

    #[test]
    fn update_note_persists() {
        let conn = open(":memory:").unwrap();
        let mut devices = HashMap::new();
        devices.insert("MAC1".into(), make_test_device("MAC1"));
        let obs = vec![PendingObs {
            mac: "MAC1".into(),
            rssi: Some(-60),
            name: None,
            service_uuids: String::new(),
            fingerprint: "A1B2".into(),
        }];
        write_cycle(&conn, &devices, &obs).unwrap();

        update_note_group(&conn, Some("A1B2"), "MAC1", "updated note").unwrap();

        let loaded = load_devices(&conn).unwrap();
        assert_eq!(loaded["MAC1"].note.as_deref(), Some("updated note"));
    }

    // ── update_gatt_info ─────────────────────────────────────────────────

    #[test]
    fn update_gatt_info_persists() {
        let conn = open(":memory:").unwrap();
        let mut devices = HashMap::new();
        devices.insert("MAC1".into(), make_test_device("MAC1"));
        let obs = vec![PendingObs {
            mac: "MAC1".into(),
            rssi: Some(-60),
            name: None,
            service_uuids: String::new(),
            fingerprint: "A1B2".into(),
        }];
        write_cycle(&conn, &devices, &obs).unwrap();

        let info = GattDeviceInfo {
            manufacturer_name: Some("Acme Corp".into()),
            model_number: Some("Model X".into()),
            firmware_revision: Some("1.0.0".into()),
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
            probed_at: "2025-06-15T14:30:00Z".into(),
        };
        update_gatt_info(&conn, "MAC1", &info).unwrap();

        let loaded = load_devices(&conn).unwrap();
        let gatt = loaded["MAC1"].gatt_info.as_ref().unwrap();
        assert_eq!(gatt.manufacturer_name.as_deref(), Some("Acme Corp"));
        assert_eq!(gatt.model_number.as_deref(), Some("Model X"));
    }

    // ── recent_rssi ─────────────────────────────────────────────────────

    fn seed_observations(conn: &Connection, mac: &str, rssi_values: &[i16]) {
        let mut devices = HashMap::new();
        devices.insert(mac.to_string(), make_test_device(mac));
        let obs = vec![PendingObs {
            mac: mac.to_string(),
            rssi: Some(-60),
            name: None,
            service_uuids: String::new(),
            fingerprint: "A1B2".into(),
        }];
        write_cycle(conn, &devices, &obs).unwrap();

        for (i, &rssi) in rssi_values.iter().enumerate() {
            let ts = format!("2025-06-15T14:{:02}:00+00:00", i);
            conn.execute(
                "INSERT INTO observations (mac, seen_at, rssi) VALUES (?1, ?2, ?3)",
                params![mac, ts, rssi],
            )
            .unwrap();
        }
    }

    #[test]
    fn recent_rssi_returns_values() {
        let conn = open(":memory:").unwrap();
        seed_observations(&conn, "MAC1", &[-60, -65, -70, -55]);
        let result = recent_rssi(&conn, &["MAC1".into()], 10).unwrap();
        // 5 total: 1 from write_cycle (current timestamp) + 4 manual inserts (2025 timestamps)
        assert_eq!(result.len(), 5);
        // All seeded values should be present
        assert!(result.contains(&-60));
        assert!(result.contains(&-65));
        assert!(result.contains(&-70));
        assert!(result.contains(&-55));
    }

    #[test]
    fn recent_rssi_respects_limit() {
        let conn = open(":memory:").unwrap();
        seed_observations(&conn, "MAC1", &[-60, -65, -70, -55, -80]);
        let result = recent_rssi(&conn, &["MAC1".into()], 3).unwrap();
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn recent_rssi_empty_macs() {
        let conn = open(":memory:").unwrap();
        let result = recent_rssi(&conn, &[], 10).unwrap();
        assert!(result.is_empty());
    }

    // ── hourly_activity ─────────────────────────────────────────────────

    #[test]
    fn hourly_activity_counts() {
        let conn = open(":memory:").unwrap();
        let mut devices = HashMap::new();
        devices.insert("MAC1".to_string(), make_test_device("MAC1"));
        let obs = vec![PendingObs {
            mac: "MAC1".into(),
            rssi: Some(-60),
            name: None,
            service_uuids: String::new(),
            fingerprint: "A1B2".into(),
        }];
        write_cycle(&conn, &devices, &obs).unwrap();

        // Insert observations at three distinct timestamps within the activity
        // window. The query counts COUNT(DISTINCT seen_at) per hour, so unique
        // timestamps are required.
        let now = Local::now();
        let timestamps: Vec<String> = (0..3)
            .map(|i| (now - chrono::Duration::minutes(i * 30)).to_rfc3339())
            .collect();
        for ts in &timestamps {
            conn.execute(
                "INSERT INTO observations (mac, seen_at, rssi) VALUES (?1, ?2, -60)",
                params!["MAC1", ts],
            )
            .unwrap();
        }

        let counts = hourly_activity(&conn, &["MAC1".into()]).unwrap();
        let total: u32 = counts.iter().sum();
        assert!(total >= 3, "Expected at least 3 observations, got {total}");
    }

    #[test]
    fn hourly_activity_empty_macs() {
        let conn = open(":memory:").unwrap();
        let counts = hourly_activity(&conn, &[]).unwrap();
        assert_eq!(counts, [0u32; 24]);
    }

    // ── mac_rotation_stats ──────────────────────────────────────────────

    #[test]
    fn mac_rotation_single_mac() {
        let conn = open(":memory:").unwrap();
        let mut devices = HashMap::new();
        devices.insert("MAC1".to_string(), make_test_device("MAC1"));
        let obs = vec![PendingObs {
            mac: "MAC1".into(),
            rssi: Some(-60),
            name: None,
            service_uuids: String::new(),
            fingerprint: "A1B2".into(),
        }];
        write_cycle(&conn, &devices, &obs).unwrap();

        let stats = mac_rotation_stats(&conn, &["MAC1".into()]).unwrap();
        assert_eq!(stats.total_macs, 1);
        assert!(stats.avg_rotation_mins.is_none());
    }

    #[test]
    fn mac_rotation_multiple_macs() {
        let conn = open(":memory:").unwrap();

        let mut d1 = make_test_device("MAC1");
        d1.first_seen = DateTime::parse_from_rfc3339("2025-06-15T10:00:00+00:00")
            .unwrap()
            .with_timezone(&Local);
        let mut d2 = make_test_device("MAC2");
        d2.first_seen = DateTime::parse_from_rfc3339("2025-06-15T11:00:00+00:00")
            .unwrap()
            .with_timezone(&Local);

        let mut devices = HashMap::new();
        devices.insert("MAC1".to_string(), d1);
        devices.insert("MAC2".to_string(), d2);
        let obs = vec![
            PendingObs {
                mac: "MAC1".into(),
                rssi: Some(-60),
                name: None,
                service_uuids: String::new(),
                fingerprint: "A1B2".into(),
            },
            PendingObs {
                mac: "MAC2".into(),
                rssi: Some(-60),
                name: None,
                service_uuids: String::new(),
                fingerprint: "A1B2".into(),
            },
        ];
        write_cycle(&conn, &devices, &obs).unwrap();

        let stats = mac_rotation_stats(&conn, &["MAC1".into(), "MAC2".into()]).unwrap();
        assert_eq!(stats.total_macs, 2);
        assert!(stats.avg_rotation_mins.is_some());
        let avg = stats.avg_rotation_mins.unwrap();
        assert!(
            (avg - 60.0).abs() < 1.0,
            "Expected ~60 min rotation, got {avg}"
        );
    }

    #[test]
    fn mac_rotation_empty_macs() {
        let conn = open(":memory:").unwrap();
        let stats = mac_rotation_stats(&conn, &[]).unwrap();
        assert_eq!(stats.total_macs, 0);
        assert!(stats.avg_rotation_mins.is_none());
    }
}
