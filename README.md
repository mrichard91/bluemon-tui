# bluemon-tui

Interactive Bluetooth Low Energy (BLE) scanner with an htop-like terminal UI. Continuously scans for nearby BLE devices, classifies them by type, resolves vendor names, and persists everything to a local SQLite database.

## Features

- **Real-time BLE scanning** with configurable cycle duration
- **Device classification** by type (phone, audio, laptop, watch, smart home, etc.) using manufacturer data, service UUIDs, name patterns, and OUI lookup
- **MAC address fingerprinting** to correlate randomized addresses belonging to the same physical device
- **Apple Continuity protocol** decoding (iBeacon, AirPods battery, AirDrop, HomeKit, Nearby Info, FindMy)
- **Google Fast Pair** model identification
- **GATT probing** to read Device Information Service and battery level
- **Distance estimation** from RSSI / TX power / iBeacon calibrated power
- **AI chat** (OpenAI) for querying scan data with natural language
- **SQLite persistence** with observations table for time-series analysis
- **Optional MQTT export** of raw scan and GATT observations for centralized processing
- **Sortable/filterable table**, device detail popup, per-device notes
- **Reference data CSVs** loaded into SQLite at startup for extensibility

## Install

Requires Rust 1.70+ and a Linux system with BlueZ (BLE stack).

```bash
git clone https://github.com/mrichard91/bluemon-tui.git
cd bluemon-tui
cargo build --release
```

The binary will be at `target/release/bluemon-tui`.

On Linux you may need BLE permissions:

```bash
sudo setcap cap_net_raw,cap_net_admin+eip target/release/bluemon-tui
```

## Usage

```bash
# Run with defaults (adapter 0, 3s scan cycles)
./target/release/bluemon-tui

# Custom adapter and scan duration
./target/release/bluemon-tui --adapter 1 --scan-duration 5

# Custom database path
./target/release/bluemon-tui --db /path/to/my.db

# Generate a template service_uuids.toml for custom UUID names
./target/release/bluemon-tui --init-service-uuids

# Generate a config.toml template with MQTT options
./target/release/bluemon-tui --init-config
```

### MQTT Export

Enable the optional MQTT publisher in `~/.config/bluemon-tui/config.toml`:

```toml
[mqtt]
enabled = true
host = "127.0.0.1"
port = 1883
topic_prefix = "bluemon"
channel_name = "office"
sensor_name = "collector-01"
site_name = "hq"
qos = 0
retain = false
```

Messages publish to `bluemon/<channel_name>/<sensor_name>/observations` as JSON. The payload only includes factual collector output such as MAC, RSSI, TX power, service UUIDs, manufacturer data, device class, address type, and GATT probe results. Device classification and higher-level analysis are intentionally left to downstream consumers.

### AI Chat

Set `OPENAI_API_KEY` in your environment or a `.env` file to enable the built-in chat. Press `c` to open the chat panel and ask questions about your scan data in natural language.

```bash
cp .env.example .env
# Edit .env with your API key
```

## Key Bindings

| Key | Action |
|-----|--------|
| `q` | Quit |
| `j`/`k` or Up/Down | Scroll device list |
| `s` | Cycle sort column |
| `S` | Reverse sort order |
| `/` | Filter devices |
| `Enter` | Add/edit note on selected device |
| `d` | Toggle device detail popup |
| `p` | Probe selected device via GATT |
| `c` | Toggle AI chat panel |
| `Home`/`End` | Jump to top/bottom |

## Data CSVs

Reference/lookup data is shipped as CSV files in `data/` and loaded into SQLite `ref_*` tables on startup:

| File | Description |
|------|-------------|
| `service_uuids.csv` | BLE service UUID prefix to name mapping |
| `fast_pair_models.csv` | Google Fast Pair model ID to device name |
| `bt_company_ids.csv` | Bluetooth SIG company IDs to name and device type |
| `ibeacon_uuids.csv` | Known iBeacon UUIDs to vendor name |
| `airpods_models.csv` | Apple AirPods/Beats model IDs to name |
| `homekit_categories.csv` | HomeKit accessory category IDs to name |
| `nearby_actions.csv` | Apple Nearby Action type IDs to name |

You can add rows to these CSVs and rebuild to extend the lookup tables.

## Scripts

- `scripts/probe-gatt.py` - Standalone Python script to probe a device's GATT services (requires [bleak](https://github.com/hbldh/bleak))
- `scripts/query-db.py` - Read-only SQL query tool for the scan database

## Local Skills

Repository-specific agent skills live in `.skills/`. The Bluetooth database analysis workflow is packaged as `.skills/analyze-bluemon-db/` rather than `.claude/commands/`.

## Inspired By

This project was inspired by [bluehood](https://github.com/mrichard91/bluehood), a Bluetooth monitoring daemon with a web UI.

## License

MIT
