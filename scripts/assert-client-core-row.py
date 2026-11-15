#!/usr/bin/env python3
"""CI-only, read-only direct SQLite proof for Windows native client registration.

This deliberately queries core_clients instead of trusting the React view or
Tauri's hydrated state; it can detect a missing relational row hidden by a UI.
Never accept an operational database path.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import sqlite3
import sys


def verify(args: argparse.Namespace) -> dict[str, object]:
    if os.environ.get("GITHUB_ACTIONS") != "true":
        raise AssertionError("Refusing SQL audit outside disposable GitHub Actions")
    appdata = os.environ.get("APPDATA")
    if not appdata:
        raise AssertionError("APPDATA is not defined")
    expected = (
        Path(appdata)
        / "com.bandostand.bandohub.tauri"
        / "developer-test"
        / "bando-hub-test.sqlite"
    ).resolve()
    database = Path(args.database).resolve(strict=True)
    if os.path.normcase(str(database)) != os.path.normcase(str(expected)):
        raise AssertionError("Only the isolated developer-test SQLite database may be read")

    # URI mode=ro cannot create a missing DB or alter the user data. SQLite's
    # own connection reads committed WAL frames while Tauri is still running.
    with sqlite3.connect(database.as_uri() + "?mode=ro", uri=True, timeout=10.0) as db:
        db.row_factory = sqlite3.Row
        db.execute("PRAGMA query_only = ON")
        integrity = db.execute("PRAGMA quick_check").fetchone()
        if integrity is None or integrity[0] != "ok":
            raise AssertionError(f"TEST SQLite quick_check failed: {integrity}")
        schema = db.execute(
            "SELECT version FROM core_schema WHERE key = 'business-core'"
        ).fetchone()
        if schema is None or int(schema["version"]) < 1:
            raise AssertionError("Missing relational core_schema version")
        total = int(db.execute("SELECT COUNT(*) FROM core_clients").fetchone()[0])
        found = db.execute(
            "SELECT id, name, tier, address, payload "
            "FROM core_clients WHERE name = ?",
            (args.name,),
        ).fetchall()

    if args.phase == "before":
        if total != 0 or found:
            raise AssertionError(f"Expected clean TEST clients before UI save; found {total}")
        return {
            "phase": args.phase, "sql": "SELECT FROM core_clients",
            "mode": "read-only", "schemaVersion": int(schema["version"]),
            "totalClientRows": total, "matchingRows": 0, "id": None,
        }

    if total != 1 or len(found) != 1:
        raise AssertionError(
            f"Expected exactly one committed core_clients row; total={total}, "
            f"matching={len(found)}"
        )
    row = found[0]
    payload = json.loads(row["payload"])
    if not isinstance(payload, dict):
        raise AssertionError("core_clients.payload is not a JSON object")
    expected_values = {
        "id": str(row["id"]),
        "name": args.name,
        "address": args.address,
        "phone": args.phone,
    }
    for field, expected_value in expected_values.items():
        actual = payload.get(field)
        if actual != expected_value:
            raise AssertionError(
                f"core_clients payload {field!r} mismatch: "
                f"expected {expected_value!r}, actual {actual!r}"
            )
    if row["name"] != args.name or row["address"] != args.address:
        raise AssertionError("core_clients indexed name/address differ from payload")
    if not row["tier"] or not row["id"]:
        raise AssertionError("core_clients row is missing a tier or stable ID")
    if args.expected_id and str(row["id"]) != args.expected_id:
        raise AssertionError(
            f"core_clients ID changed after process restart: "
            f"{args.expected_id!r} -> {row['id']!r}"
        )

    return {
        "phase": args.phase, "sql": "SELECT FROM core_clients",
        "mode": "read-only", "schemaVersion": int(schema["version"]),
        "totalClientRows": total, "matchingRows": len(found),
        "id": str(row["id"]), "name": str(row["name"]),
        "address": str(row["address"]), "phone": str(payload["phone"]),
        "payloadMatchesColumns": True, "idPreserved": bool(args.expected_id),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--database", required=True)
    parser.add_argument("--phase", required=True,
                        choices=("before", "saved", "closed", "reopened"))
    parser.add_argument("--name", required=True)
    parser.add_argument("--address", required=True)
    parser.add_argument("--phone", required=True)
    parser.add_argument("--expected-id", default="")
    args = parser.parse_args()
    try:
        result = verify(args)
    except (AssertionError, OSError, sqlite3.Error, ValueError, KeyError) as exc:
        print(f"CORE_CLIENT_SQL_FAILED: {exc}", file=sys.stderr)
        return 1
    print(json.dumps(result, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
