use crate::app::DeviceInfo;
use crate::classifier::DeviceType;
use crate::continuity::ContinuityData;
use crate::gatt::GattDeviceInfo;
use chrono::{DateTime, Local};
use rusqlite::{params, Connection};
use std::collections::HashMap;

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
             service_uuids TEXT DEFAULT ''
         );

         CREATE INDEX IF NOT EXISTS idx_obs_mac ON observations(mac);
         CREATE INDEX IF NOT EXISTS idx_obs_seen ON observations(seen_at);",
    )?;

    // Migrate older databases that lack newer columns
    let has_tx_power: bool = conn
        .prepare("SELECT tx_power FROM devices LIMIT 0")
        .is_ok();
    if !has_tx_power {
        conn.execute_batch("ALTER TABLE devices ADD COLUMN tx_power INTEGER;")?;
    }
    let has_last_rssi: bool = conn
        .prepare("SELECT last_rssi FROM devices LIMIT 0")
        .is_ok();
    if !has_last_rssi {
        conn.execute_batch("ALTER TABLE devices ADD COLUMN last_rssi INTEGER;")?;
    }

    // Migrate older databases that lack the fingerprint column
    let has_fingerprint: bool = conn
        .prepare("SELECT fingerprint FROM devices LIMIT 0")
        .is_ok();
    if !has_fingerprint {
        conn.execute_batch("ALTER TABLE devices ADD COLUMN fingerprint TEXT DEFAULT '';")?;
    }

    // Migrate older databases that lack enrichment columns
    if conn
        .prepare("SELECT continuity_json FROM devices LIMIT 0")
        .is_err()
    {
        conn.execute_batch(
            "ALTER TABLE devices ADD COLUMN continuity_json TEXT DEFAULT '';
             ALTER TABLE devices ADD COLUMN gatt_info_json TEXT DEFAULT '';
             ALTER TABLE devices ADD COLUMN fast_pair_model TEXT DEFAULT '';",
        )?;
    }

    seed_reference_data(&conn)?;

    Ok(conn)
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
        (include_str!("../data/service_uuids.csv"), "ref_service_uuids", 2),
        (include_str!("../data/fast_pair_models.csv"), "ref_fast_pair_models", 2),
        (include_str!("../data/bt_company_ids.csv"), "ref_bt_company_ids", 3),
        (include_str!("../data/ibeacon_uuids.csv"), "ref_ibeacon_uuids", 2),
        (include_str!("../data/airpods_models.csv"), "ref_airpods_models", 2),
        (include_str!("../data/homekit_categories.csv"), "ref_homekit_categories", 2),
        (include_str!("../data/nearby_actions.csv"), "ref_nearby_actions", 2),
    ];

    for &(csv, table, cols) in csvs.iter() {
        let placeholders = (1..=cols).map(|n| format!("?{n}")).collect::<Vec<_>>().join(", ");
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
                2 => { stmt.execute(params![fields[0], fields[1]])?; }
                3 => { stmt.execute(params![fields[0], fields[1], fields[2]])?; }
                _ => {}
            }
        }
    }

    Ok(())
}

pub fn load_devices(conn: &Connection) -> anyhow::Result<HashMap<String, DeviceInfo>> {
    let mut stmt = conn.prepare(
        "SELECT mac, name, vendor, device_type, is_randomized,
                first_seen, last_seen, note, service_uuids, sightings, tx_power, fingerprint,
                continuity_json, gatt_info_json, fast_pair_model, last_rssi
         FROM devices",
    )?;
    let mut devices = HashMap::new();
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, bool>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, u32>(9)?,
            row.get::<_, Option<i16>>(10)?,
            row.get::<_, Option<String>>(11)?,
            row.get::<_, Option<String>>(12)?,
            row.get::<_, Option<String>>(13)?,
            row.get::<_, Option<String>>(14)?,
            row.get::<_, Option<i16>>(15)?,
        ))
    })?;

    for row in rows {
        let (mac, name, vendor, dtype_str, is_randomized, first_str, last_str, note, uuids_str, sightings, tx_power, fingerprint, continuity_json, gatt_info_json, fast_pair_model, last_rssi) = row?;
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
        let continuity = continuity_json
            .as_deref()
            .filter(|s| !s.is_empty())
            .and_then(|s| serde_json::from_str::<ContinuityData>(s).ok());
        let gatt_info = gatt_info_json
            .as_deref()
            .filter(|s| !s.is_empty())
            .and_then(|s| serde_json::from_str::<GattDeviceInfo>(s).ok());
        let fast_pair_model = fast_pair_model.filter(|s| !s.is_empty());

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
                continuity,
                gatt_info,
                fast_pair_model,
            },
        );
    }

    Ok(devices)
}

/// Observation data captured at scan time for batch writing.
pub struct PendingObs {
    pub mac: String,
    pub rssi: Option<i16>,
    pub name: Option<String>,
    pub service_uuids: String,
}

/// Write a full scan cycle to the database in a single transaction:
/// insert observations and upsert device records.
pub fn write_cycle(
    conn: &Connection,
    devices: &HashMap<String, DeviceInfo>,
    observations: &[PendingObs],
) -> anyhow::Result<()> {
    let tx = conn.unchecked_transaction()?;
    let now = Local::now().to_rfc3339();

    {
        let mut obs_stmt = tx.prepare_cached(
            "INSERT INTO observations (mac, seen_at, rssi, name, service_uuids)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for obs in observations {
            obs_stmt.execute(params![obs.mac, now, obs.rssi, obs.name, obs.service_uuids])?;
        }
    }

    {
        let mut dev_stmt = tx.prepare_cached(
            "INSERT INTO devices (mac, name, vendor, device_type, is_randomized,
                                  first_seen, last_seen, note, service_uuids, sightings, tx_power, fingerprint,
                                  continuity_json, gatt_info_json, fast_pair_model, last_rssi)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
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
                 last_rssi = COALESCE(?16, last_rssi)",
        )?;

        let mut written = std::collections::HashSet::new();
        for obs in observations {
            if !written.insert(&obs.mac) {
                continue;
            }
            if let Some(d) = devices.get(&obs.mac) {
                let continuity_json = d
                    .continuity
                    .as_ref()
                    .and_then(|c| serde_json::to_string(c).ok())
                    .unwrap_or_default();
                let gatt_info_json = d
                    .gatt_info
                    .as_ref()
                    .and_then(|g| serde_json::to_string(g).ok())
                    .unwrap_or_default();
                let fast_pair_model = d.fast_pair_model.as_deref().unwrap_or("");

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
                ])?;
            }
        }
    }

    tx.commit()?;
    Ok(())
}

pub fn update_gatt_info(
    conn: &Connection,
    mac: &str,
    info: &GattDeviceInfo,
) -> anyhow::Result<()> {
    let json = serde_json::to_string(info)?;
    conn.execute(
        "UPDATE devices SET gatt_info_json = ?1 WHERE mac = ?2",
        params![json, mac],
    )?;
    Ok(())
}

pub fn update_note(conn: &Connection, mac: &str, note: &str) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE devices SET note = ?1 WHERE mac = ?2",
        params![note, mac],
    )?;
    Ok(())
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
             CREATE TABLE IF NOT EXISTS observations (id INTEGER PRIMARY KEY);"
        ).unwrap();
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
            continuity: None,
            gatt_info: None,
            fast_pair_model: Some("Pixel Buds".into()),
        }
    }

    #[test]
    fn write_and_load_round_trip() {
        let conn = open(":memory:").unwrap();

        let mut devices = HashMap::new();
        devices.insert("AA:BB:CC:DD:EE:01".into(), make_test_device("AA:BB:CC:DD:EE:01"));

        let observations = vec![PendingObs {
            mac: "AA:BB:CC:DD:EE:01".into(),
            rssi: Some(-65),
            name: Some("TestDevice".into()),
            service_uuids: "0000180a,0000180f".into(),
        }];

        write_cycle(&conn, &devices, &observations).unwrap();

        let loaded = load_devices(&conn).unwrap();
        assert_eq!(loaded.len(), 1);
        let dev = &loaded["AA:BB:CC:DD:EE:01"];
        assert_eq!(dev.name.as_deref(), Some("TestDevice"));
        assert_eq!(dev.vendor.as_deref(), Some("TestVendor"));
        assert_eq!(dev.device_type, DeviceType::Audio);
        assert!(dev.is_randomized);
        assert_eq!(dev.sightings, 3);
        assert_eq!(dev.tx_power, Some(4));
        assert_eq!(dev.rssi, Some(-65));
        assert_eq!(dev.fingerprint, "A1B2");
        assert_eq!(dev.fast_pair_model.as_deref(), Some("Pixel Buds"));
        assert_eq!(dev.service_uuids, vec!["0000180a", "0000180f"]);
        assert_eq!(dev.note.as_deref(), Some("test note"));
    }

    #[test]
    fn write_cycle_multiple_devices() {
        let conn = open(":memory:").unwrap();

        let mut devices = HashMap::new();
        devices.insert("MAC1".into(), make_test_device("MAC1"));
        devices.insert("MAC2".into(), make_test_device("MAC2"));

        let observations = vec![
            PendingObs { mac: "MAC1".into(), rssi: Some(-60), name: None, service_uuids: String::new() },
            PendingObs { mac: "MAC2".into(), rssi: Some(-70), name: None, service_uuids: String::new() },
        ];

        write_cycle(&conn, &devices, &observations).unwrap();
        let loaded = load_devices(&conn).unwrap();
        assert_eq!(loaded.len(), 2);
    }

    // ── update_note ──────────────────────────────────────────────────────

    #[test]
    fn update_note_persists() {
        let conn = open(":memory:").unwrap();
        let mut devices = HashMap::new();
        devices.insert("MAC1".into(), make_test_device("MAC1"));
        let obs = vec![PendingObs {
            mac: "MAC1".into(), rssi: Some(-60), name: None, service_uuids: String::new(),
        }];
        write_cycle(&conn, &devices, &obs).unwrap();

        update_note(&conn, "MAC1", "updated note").unwrap();

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
            mac: "MAC1".into(), rssi: Some(-60), name: None, service_uuids: String::new(),
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
            probed_at: "2025-06-15T14:30:00Z".into(),
        };
        update_gatt_info(&conn, "MAC1", &info).unwrap();

        let loaded = load_devices(&conn).unwrap();
        let gatt = loaded["MAC1"].gatt_info.as_ref().unwrap();
        assert_eq!(gatt.manufacturer_name.as_deref(), Some("Acme Corp"));
        assert_eq!(gatt.model_number.as_deref(), Some("Model X"));
    }
}
