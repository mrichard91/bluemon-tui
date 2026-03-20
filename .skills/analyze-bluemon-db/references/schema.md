# Schema Reference

Use this file when you need table or column details while building a query.

## Query Tools

- Read-only query tool:
  `python3 scripts/query-db.py "SELECT ..." --format table`
- Active GATT probe:
  `python3 scripts/probe-gatt.py <MAC>`
- Default database path:
  `~/.local/share/bluemon-tui/bluemon.db`

## Core Tables

```sql
CREATE TABLE devices (
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
    tx_power INTEGER,
    fingerprint TEXT DEFAULT '',
    continuity_json TEXT DEFAULT '',
    gatt_info_json TEXT DEFAULT '',
    fast_pair_model TEXT DEFAULT '',
    last_rssi INTEGER,
    device_class INTEGER,
    addr_type TEXT DEFAULT ''
);

CREATE TABLE observations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    mac TEXT NOT NULL,
    seen_at TEXT NOT NULL,
    rssi INTEGER,
    name TEXT,
    service_uuids TEXT DEFAULT '',
    fingerprint TEXT DEFAULT ''
);
```

Indexes: `idx_obs_mac(mac)`, `idx_obs_seen(seen_at)`

`device_type` values:
`phone`, `tablet`, `laptop`, `computer`, `watch`, `audio`, `speaker`, `tv`, `vehicle`, `smart_home`, `wearable`, `gaming`, `camera`, `printer`, `network`, `unknown`

## Table Semantics

- `devices` stores aggregate state for each MAC.
- `observations` stores one row per sighting per scan cycle and is the right table for time-of-day, dwell, repeat-visit, and RSSI-trend analysis.
- `fingerprint` groups MACs that likely belong to the same physical device. Legacy rows can leave it empty, so fall back to `mac`.
- `note` is user-authored context.
- `service_uuids` is a comma-separated UUID list.

## Reference Tables

These tables are seeded from `data/*.csv` and should be used to resolve IDs into names:

```sql
SELECT prefix, name FROM ref_service_uuids WHERE prefix = '0000180a';
SELECT model_id, name FROM ref_fast_pair_models WHERE model_id = '000047';
SELECT company_id, name, device_type FROM ref_bt_company_ids WHERE company_id = '004c';
SELECT uuid, vendor FROM ref_ibeacon_uuids;
SELECT model_id, name FROM ref_airpods_models WHERE model_id = '0220';
SELECT category_id, name FROM ref_homekit_categories WHERE category_id = 5;
SELECT action_id, name FROM ref_nearby_actions WHERE action_id = '09';
```

## Enrichment Columns

### `continuity_json`

Apple Continuity protocol data. Query it with `json_extract()`.

Common `type` values:
`IBeacon`, `AirDrop`, `HomeKit`, `AirPods`, `AirPlay`, `Handoff`, `NearbyInfo`, `NearbyAction`, `AirPodsExtended`, `FindMy`

Useful fields:

- `$.type`
- `$.battery_left`, `$.battery_right`, `$.battery_case`
- `$.device_model`
- `$.device_category`
- `$.action_type`
- `$.uuid`, `$.major`, `$.minor`

### `gatt_info_json`

JSON written by active probing. Useful fields:

- `$.manufacturer_name`
- `$.model_number`
- `$.firmware_revision`
- `$.hardware_revision`
- `$.software_revision`
- `$.battery_level`
- `$.pnp_id`
- `$.probed_at`

### `fast_pair_model`

Resolved Google Fast Pair model name such as `Pixel Buds Pro`.

## Timestamp Handling

Timestamps are RFC3339 strings with timezone offsets, generated in local time and stored as text. For user-facing local-time analysis, use SQLite date/time helpers with `'localtime'`:

- `datetime('now', 'localtime')`
- `date(seen_at, 'localtime')`
- `time(seen_at, 'localtime')`
- `strftime('%H', seen_at, 'localtime')`
- `julianday(MAX(seen_at)) - julianday(MIN(seen_at))`
