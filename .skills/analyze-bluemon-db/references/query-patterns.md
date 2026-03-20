# Query Patterns

Use this file when you need concrete query shapes or analysis heuristics.

## Physical Device Key

Use this expression whenever the user means "device" rather than "MAC":

```sql
COALESCE(NULLIF(fingerprint, ''), mac)
```

In `observations`, prefer:

```sql
COALESCE(NULLIF(o.fingerprint, ''), o.mac)
```

## Common Queries

### Unique physical devices seen today

```sql
SELECT COUNT(DISTINCT COALESCE(NULLIF(fingerprint, ''), mac)) AS device_count
FROM devices
WHERE date(last_seen, 'localtime') = date('now', 'localtime');
```

### Top device types nearby now

```sql
SELECT device_type,
       COUNT(DISTINCT COALESCE(NULLIF(fingerprint, ''), mac)) AS cnt
FROM devices
WHERE last_seen >= datetime('now', 'localtime', '-5 minutes')
GROUP BY device_type
ORDER BY cnt DESC;
```

### Visit pattern for one device fingerprint

```sql
SELECT date(seen_at, 'localtime') AS day,
       MIN(time(seen_at, 'localtime')) AS first_seen,
       MAX(time(seen_at, 'localtime')) AS last_seen,
       COUNT(*) AS sightings
FROM observations
WHERE COALESCE(NULLIF(fingerprint, ''), mac) = 'A1B2'
GROUP BY day
ORDER BY day DESC
LIMIT 14;
```

### Dwell time and RSSI summary for candidate transient devices

```sql
SELECT COALESCE(NULLIF(o.fingerprint, ''), o.mac) AS dev_key,
       MIN(time(o.seen_at, 'localtime')) AS first_time,
       MAX(time(o.seen_at, 'localtime')) AS last_time,
       ROUND((julianday(MAX(o.seen_at)) - julianday(MIN(o.seen_at))) * 1440, 1) AS dwell_mins,
       ROUND(AVG(o.rssi), 0) AS avg_rssi,
       COUNT(*) AS obs
FROM observations o
GROUP BY dev_key
HAVING dwell_mins < 30
ORDER BY first_time;
```

### AirPods or Beats with battery levels

```sql
SELECT mac,
       name,
       json_extract(continuity_json, '$.type') AS ctype,
       json_extract(continuity_json, '$.battery_left') * 10 AS left_pct,
       json_extract(continuity_json, '$.battery_right') * 10 AS right_pct,
       json_extract(continuity_json, '$.battery_case') * 10 AS case_pct
FROM devices
WHERE json_extract(continuity_json, '$.type') IN ('AirPods', 'AirPodsExtended');
```

### Devices with Fast Pair names

```sql
SELECT mac, fast_pair_model, vendor, sightings
FROM devices
WHERE fast_pair_model != ''
ORDER BY sightings DESC;
```

## Analysis Heuristics

### Walker or passer-by candidates

Look for the combination of:

- short dwell time, often under 30 minutes
- relatively weak signal, often average RSSI below `-80`
- limited total observations or sightings
- repeat appearances at similar times across different days

Exclude likely non-walkers first:

- Apple HomeKit, iBeacon, and FindMy rows via `json_extract(continuity_json, '$.type')`
- obvious household devices with long dwell windows
- static infrastructure such as printers, TVs, smart-home gear, and network devices

### Apple-heavy environments

- Very common Apple fingerprints can represent neighbors, not one transient device.
- Cross-check dwell time and repeat days before labeling a device as a walker or tracker.
- Join lookup tables when continuity payloads expose model, category, or action IDs.

### Audio-device identification

- Check `fast_pair_model` first for Android ecosystem earbuds.
- Check `continuity_json` for AirPods and Beats.
- Use `gatt_info_json` only after an explicit active probe or when passive data is missing.

## Pitfalls

- Do not use CTEs with `scripts/query-db.py`; it rejects non-`SELECT` prefixes.
- Do not group on bare `fingerprint`; empty strings will collapse unrelated rows.
- Do not assume `devices.name` or `devices.vendor` is stable across all MACs in a fingerprint group; prefer the most recent row when a single representative is needed.
- Do not dump huge `observations` result sets. Aggregate first, then drill into a specific MAC or fingerprint if needed.
