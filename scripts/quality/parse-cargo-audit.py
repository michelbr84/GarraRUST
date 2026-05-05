#!/usr/bin/env python3
"""Parse `cargo audit --json` output → severity counts.

Usage:
    cargo audit --json | python3 parse-cargo-audit.py
    python3 parse-cargo-audit.py path/to/cargo-audit.json

Output JSON shape:
    {
      "critical": 0,
      "high": 1,
      "medium": 1,
      "low": 0,
      "informational": 0,
      "unknown": 1,
      "total": 3,
      "status": "present"
    }

The "unknown" bucket catches advisories where the `severity` field is missing
(some `cargo audit` outputs omit severity for advisories that lack a CVSS
score). Missing input file or unparseable JSON yields status="not_collected_this_run".
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

KNOWN_SEVERITIES = ("critical", "high", "medium", "low", "informational")


def parse_audit(payload: dict) -> dict:
    counts = {sev: 0 for sev in KNOWN_SEVERITIES}
    counts["unknown"] = 0

    vulns = (payload.get("vulnerabilities") or {}).get("list") or []
    for vuln in vulns:
        adv = vuln.get("advisory") or {}
        sev = (adv.get("severity") or "").lower()
        if sev in counts:
            counts[sev] += 1
        else:
            counts["unknown"] += 1

    counts["total"] = sum(counts[k] for k in (*KNOWN_SEVERITIES, "unknown"))
    counts["status"] = "present"
    return counts


def empty_summary(reason: str) -> dict:
    out = {sev: 0 for sev in KNOWN_SEVERITIES}
    out["unknown"] = 0
    out["total"] = 0
    out["status"] = "not_collected_this_run"
    out["reason"] = reason
    return out


def main(argv: list[str]) -> int:
    if len(argv) > 2:
        print(f"usage: {argv[0]} [path/to/cargo-audit.json]", file=sys.stderr)
        return 2

    if len(argv) == 2:
        path = Path(argv[1])
        if not path.exists() or path.stat().st_size == 0:
            json.dump(empty_summary("file_missing"), sys.stdout, indent=2, sort_keys=True)
            print()
            return 0
        raw = path.read_text(encoding="utf-8")
    else:
        raw = sys.stdin.read()
        if not raw.strip():
            json.dump(empty_summary("stdin_empty"), sys.stdout, indent=2, sort_keys=True)
            print()
            return 0

    try:
        payload = json.loads(raw)
    except json.JSONDecodeError as exc:
        summary = empty_summary(f"unparseable_json: {exc.msg}")
        json.dump(summary, sys.stdout, indent=2, sort_keys=True)
        print()
        return 0

    summary = parse_audit(payload)
    json.dump(summary, sys.stdout, indent=2, sort_keys=True)
    print()
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
