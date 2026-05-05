#!/usr/bin/env python3
"""Parse cargo-llvm-cov lcov.info output → coverage JSON.

Reads an lcov.info file (lines hit / lines found per source file) and emits a
JSON summary with total line coverage percentage.

Usage:
    python3 parse-llvm-cov.py path/to/lcov.info

Output JSON shape:
    {
      "coverage_pct": 70.0,
      "lines_total": 10,
      "lines_hit": 7,
      "files": 3,
      "status": "present"
    }

If the input file is missing or empty, exits 0 with status="not_collected_this_run".
This is the best-effort contract from plan 0064 — the ratchet workflow will
mark coverage as not collected rather than failing.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path


def parse_lcov(path: Path) -> dict:
    if not path.exists() or path.stat().st_size == 0:
        return {
            "coverage_pct": None,
            "lines_total": 0,
            "lines_hit": 0,
            "files": 0,
            "status": "not_collected_this_run",
        }

    lines_total = 0
    lines_hit = 0
    files = 0

    with path.open("r", encoding="utf-8") as fh:
        for raw in fh:
            line = raw.strip()
            if line.startswith("SF:"):
                files += 1
            elif line.startswith("LF:"):
                lines_total += int(line[3:])
            elif line.startswith("LH:"):
                lines_hit += int(line[3:])

    pct = round(lines_hit / lines_total * 100, 2) if lines_total > 0 else 0.0

    return {
        "coverage_pct": pct,
        "lines_total": lines_total,
        "lines_hit": lines_hit,
        "files": files,
        "status": "present",
    }


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print(f"usage: {argv[0]} path/to/lcov.info", file=sys.stderr)
        return 2
    summary = parse_lcov(Path(argv[1]))
    json.dump(summary, sys.stdout, indent=2, sort_keys=True)
    print()
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
