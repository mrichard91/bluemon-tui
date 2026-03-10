#!/usr/bin/env python3
"""Read-only query tool for the bluemon Bluetooth scan database.

Usage: query-db.py "SELECT ..." [--db PATH] [--limit N] [--format json|csv|table]
"""
import argparse
import json
import os
import sqlite3
import sys


DEFAULT_DB = os.path.expanduser("~/.local/share/bluemon-tui/bluemon.db")


def main():
    parser = argparse.ArgumentParser(description="Query the bluemon scan database (read-only)")
    parser.add_argument("query", help="SQL SELECT query to execute")
    parser.add_argument("--db", default=DEFAULT_DB, help="Path to bluemon.db")
    parser.add_argument("--limit", type=int, default=500, help="Max rows to return (default 500)")
    parser.add_argument("--format", choices=["json", "csv", "table"], default="json", help="Output format")
    args = parser.parse_args()

    query = args.query.strip()
    if not query.upper().startswith("SELECT"):
        print(json.dumps({"error": "Only SELECT queries are allowed."}))
        sys.exit(1)

    if not os.path.exists(args.db):
        print(json.dumps({"error": f"Database not found: {args.db}"}))
        sys.exit(1)

    try:
        conn = sqlite3.connect(f"file:{args.db}?mode=ro", uri=True)
        conn.row_factory = sqlite3.Row
        cursor = conn.execute(query)
        columns = [desc[0] for desc in cursor.description] if cursor.description else []
        rows = []
        for i, row in enumerate(cursor):
            if i >= args.limit:
                break
            rows.append(dict(row))
        conn.close()
    except Exception as e:
        print(json.dumps({"error": str(e)}))
        sys.exit(1)

    if args.format == "json":
        print(json.dumps({"row_count": len(rows), "columns": columns, "rows": rows}, indent=2))
    elif args.format == "csv":
        if columns:
            print(",".join(columns))
        for row in rows:
            print(",".join(str(row.get(c, "")) for c in columns))
    elif args.format == "table":
        if not rows:
            print("(no rows)")
            return
        widths = {c: max(len(c), max((len(str(r.get(c, ""))) for r in rows), default=0)) for c in columns}
        header = " | ".join(c.ljust(widths[c]) for c in columns)
        sep = "-+-".join("-" * widths[c] for c in columns)
        print(header)
        print(sep)
        for row in rows:
            print(" | ".join(str(row.get(c, "")).ljust(widths[c]) for c in columns))


if __name__ == "__main__":
    main()
