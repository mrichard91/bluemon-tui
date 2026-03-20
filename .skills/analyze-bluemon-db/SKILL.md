---
name: analyze-bluemon-db
description: Query and interpret the bluemon-tui SQLite scan database. Use when asked to answer questions about nearby Bluetooth devices, device counts, repeat visits, dwell time, Apple Continuity or Google Fast Pair data, GATT enrichment, or transient and walker patterns by running `scripts/query-db.py` and, when explicitly needed, `scripts/probe-gatt.py`.
---

# Analyze Bluemon DB

Use this skill to answer questions about the local Bluetooth scan history with targeted SQL, not broad row dumps. Keep the top-level workflow here and load the reference files only when you need schema detail or query patterns.

## Quick Start

- Run exploratory queries with `python3 scripts/query-db.py "SELECT ..." --format table`.
- Switch to `--format json` only when you need to post-process the result.
- The default database path is `~/.local/share/bluemon-tui/bluemon.db`; pass `--db` only if the user is working against a different file.
- Read [references/schema.md](references/schema.md) for table, column, JSON, and lookup-table details.
- Read [references/query-patterns.md](references/query-patterns.md) for counting devices, visit-pattern analysis, Apple/audio enrichment, and transient-device heuristics.

## Workflow

1. Translate the user question into the narrowest useful SQL query.
2. Prefer aggregates, grouping, and date filters over `SELECT *` or raw observation dumps.
3. Use `COALESCE(NULLIF(fingerprint, ''), mac)` as the physical-device key unless the user specifically wants raw MAC-level detail.
4. When querying time-series behavior, prefer `observations` and use its `fingerprint` column with the same fallback logic.
5. Resolve IDs and protocol fields into human-readable names before answering by using `ref_*` tables or `json_extract()`.
6. If the first query only identifies a candidate device or fingerprint, run one focused follow-up query and then answer.
7. Present results in terminal-friendly markdown: narrow tables, short headers, bullets when there are only a few facts.

## Guardrails

- `scripts/query-db.py` accepts only top-level `SELECT` statements. Do not use `WITH` CTEs here.
- The tool caps results at 500 rows by default. Tighten the query instead of increasing raw output.
- `devices` is one row per MAC, not one row per physical device.
- Some rows have an empty `fingerprint`; never group on bare `fingerprint` alone.
- Use `'localtime'` when extracting dates or hours for user-facing local-time analysis.

## Active GATT Probing

- Run `python3 scripts/probe-gatt.py <MAC>` only when passive data is insufficient and the user needs to identify a specific reachable device.
- This probe writes enriched data back to the database. Do not use it for broad exploratory analysis.
