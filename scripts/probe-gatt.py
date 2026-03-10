#!/usr/bin/env python3
"""Probe a BLE device's GATT services and store results.

Usage: python3 scripts/probe-gatt.py <MAC> [--db PATH] [--timeout 10]
"""
import argparse
import asyncio
import json
import os
import sqlite3
import struct
import sys
from datetime import datetime, timezone

from bleak import BleakClient

DEFAULT_DB = os.path.expanduser("~/.local/share/bluemon-tui/bluemon.db")

# BLE service UUIDs
DIS_SERVICE_UUID = "0000180a-0000-1000-8000-00805f9b34fb"
BATTERY_SERVICE_UUID = "0000180f-0000-1000-8000-00805f9b34fb"

# Device Information Service (0x180A) characteristic UUIDs
DIS_CHARS = {
    "00002a29-0000-1000-8000-00805f9b34fb": "manufacturer_name",
    "00002a24-0000-1000-8000-00805f9b34fb": "model_number",
    "00002a26-0000-1000-8000-00805f9b34fb": "firmware_revision",
    "00002a27-0000-1000-8000-00805f9b34fb": "hardware_revision",
    "00002a28-0000-1000-8000-00805f9b34fb": "software_revision",
}
PNP_ID_UUID = "00002a50-0000-1000-8000-00805f9b34fb"
BATTERY_LEVEL_UUID = "00002a19-0000-1000-8000-00805f9b34fb"


async def probe(mac: str, timeout: float) -> dict:
    info = {}
    async with BleakClient(mac, timeout=timeout) as client:
        for service in client.services:
            if service.uuid == DIS_SERVICE_UUID:
                for char in service.characteristics:
                    if "read" not in char.properties:
                        continue
                    # PnP ID is binary, handle separately
                    if char.uuid == PNP_ID_UUID:
                        try:
                            data = await client.read_gatt_char(char)
                            if len(data) >= 7:
                                src, vid, pid, ver = struct.unpack("<BHHH", data[:7])
                                prefix = "BT" if src == 1 else "USB"
                                info["pnp_id"] = f"{prefix}:{vid:04X}:{pid:04X}:{ver:04X}"
                        except Exception:
                            pass
                        continue
                    field = DIS_CHARS.get(char.uuid)
                    if field:
                        try:
                            data = await client.read_gatt_char(char)
                            value = data.decode("utf-8", errors="replace").strip()
                            if value:
                                info[field] = value
                        except Exception:
                            pass

            elif service.uuid == BATTERY_SERVICE_UUID:
                for char in service.characteristics:
                    if char.uuid == BATTERY_LEVEL_UUID and "read" in char.properties:
                        try:
                            data = await client.read_gatt_char(char)
                            if data:
                                info["battery_level"] = data[0]
                        except Exception:
                            pass
    return info


def store_result(db_path: str, mac: str, gatt_json: str):
    if not os.path.exists(db_path):
        return
    conn = sqlite3.connect(db_path)
    conn.execute(
        "UPDATE devices SET gatt_info_json = ? WHERE mac = ?",
        (gatt_json, mac),
    )
    conn.commit()
    conn.close()


def main():
    parser = argparse.ArgumentParser(description="Probe BLE device GATT services")
    parser.add_argument("mac", help="Device MAC address (e.g. AA:BB:CC:DD:EE:FF)")
    parser.add_argument("--db", default=DEFAULT_DB, help="Path to bluemon.db")
    parser.add_argument("--timeout", type=float, default=10, help="Connection timeout in seconds (default 10)")
    args = parser.parse_args()

    mac = args.mac.upper()
    try:
        info = asyncio.run(probe(mac, args.timeout))
        info["probed_at"] = datetime.now(timezone.utc).astimezone().isoformat()
        gatt_json = json.dumps(info)
        store_result(args.db, mac, gatt_json)
        result = {"mac": mac, "status": "ok", **info}
    except Exception as e:
        result = {"mac": mac, "status": "error", "error": str(e)}

    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
