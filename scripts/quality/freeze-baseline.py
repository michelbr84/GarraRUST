#!/usr/bin/env python3
"""freeze-baseline.py — propose a new baseline from current metrics.

NEVER commits. Always writes `.quality/baseline.proposed.json` for human review.
The Quality Ratchet's anti-fraud invariant (plan 0064 §11.5) requires that
baseline updates only happen via:
    1. Human inspection of `.quality/baseline.proposed.json`.
    2. Explicit move/rename to `.quality/baseline.json`.
    3. Manual commit.

Usage:
    bash scripts/quality/collect-metrics.sh > current-metrics.json
    python3 scripts/quality/freeze-baseline.py current-metrics.json
    # → writes .quality/baseline.proposed.json
    # → DOES NOT modify .quality/baseline.json

Ratchet semantics: when current is BETTER than the existing baseline (e.g.
coverage goes up, files_over_700 goes down, max_file_lines decreases), the
proposal preserves the BETTER value. When current is WORSE, the proposal
preserves the BASELINE value (so the proposed file is never a regression).
This makes it safe to run after every push to main as a "what would the
new baseline look like" preview.
"""

from __future__ import annotations

import argparse
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_BASELINE = REPO_ROOT / ".quality" / "baseline.json"
DEFAULT_PROPOSED = REPO_ROOT / ".quality" / "baseline.proposed.json"


def load_or_default_baseline(path: Path) -> dict:
    if path.exists() and path.stat().st_size > 0:
        return json.loads(path.read_text(encoding="utf-8"))
    # First-time seed: file-size sentinels start permissive (10^9) so any real
    # current is immediately ratcheted to a smaller value. Coverage / audit /
    # clippy start with status="not_collected_yet" so compare.py's report-only
    # mode treats them as "no baseline yet, current registered for future"
    # rather than a hard floor.
    return {
        "schema_version": "1.0",
        "max_file_lines": 10**9,
        "files_over_700": 10**9,
        "files_over_1500": 10**9,
        "files_over_2500": 10**9,
        "coverage": {"coverage_pct": None, "status": "not_collected_yet"},
        "audit": {
            "critical": 0,
            "high": 0,
            "medium": 0,
            "low": 0,
            "informational": 0,
            "unknown": 0,
            "total": 0,
            "status": "not_collected_yet",
        },
        "clippy": {
            "clippy_warnings": None,
            "non_clippy_warnings": None,
            "errors": None,
            "status": "not_collected_yet",
        },
        "_doc": "Synthetic seed baseline used by freeze-baseline.py when no real baseline exists yet.",
    }


def ratchet_int_min(b: int | None, c: int | None) -> int | None:
    """Return the SMALLER (better) of two ints; None-tolerant."""
    if b is None:
        return c
    if c is None:
        return b
    return min(b, c)


def ratchet_int_max(b: int | None, c: int | None) -> int | None:
    """Return the LARGER (better) of two ints — used for coverage."""
    if b is None:
        return c
    if c is None:
        return b
    return max(b, c)


def freeze(baseline: dict, current: dict) -> dict:
    out = dict(baseline)  # copy

    # File size: smaller is better
    out["max_file_lines"] = ratchet_int_min(
        baseline.get("max_file_lines"), current.get("max_file_lines")
    )
    out["max_file_path"] = current.get("max_file_path") or baseline.get("max_file_path")
    out["files_over_700"] = ratchet_int_min(
        baseline.get("files_over_700"), current.get("files_over_700")
    )
    out["files_over_1500"] = ratchet_int_min(
        baseline.get("files_over_1500"), current.get("files_over_1500")
    )
    out["files_over_2500"] = ratchet_int_min(
        baseline.get("files_over_2500"), current.get("files_over_2500")
    )
    out["total_rs_files"] = current.get("total_rs_files")  # informational

    # Coverage: bigger is better. Only update if both are "present".
    b_cov = baseline.get("coverage") or {}
    c_cov = current.get("coverage") or {}
    if b_cov.get("status") == "present" and c_cov.get("status") == "present":
        better_pct = ratchet_int_max(b_cov.get("coverage_pct"), c_cov.get("coverage_pct"))
        out["coverage"] = {
            "coverage_pct": better_pct,
            "status": "present",
            "lines_total": c_cov.get("lines_total"),
            "lines_hit": c_cov.get("lines_hit"),
        }
    elif c_cov.get("status") == "present":
        out["coverage"] = c_cov  # first time we see coverage; seed it
    else:
        out["coverage"] = b_cov

    # Audit: smaller is better. Only update if current is "present".
    b_aud = baseline.get("audit") or {}
    c_aud = current.get("audit") or {}
    if c_aud.get("status") == "present":
        out["audit"] = {
            "critical": 0,  # always 0 — never accept any
            "high": ratchet_int_min(b_aud.get("high"), c_aud.get("high")),
            "medium": ratchet_int_min(b_aud.get("medium"), c_aud.get("medium")),
            "low": ratchet_int_min(b_aud.get("low"), c_aud.get("low")),
            "informational": c_aud.get("informational", 0),
            "unknown": ratchet_int_min(b_aud.get("unknown"), c_aud.get("unknown")),
            "total": ratchet_int_min(b_aud.get("total"), c_aud.get("total")),
            "status": "present",
        }
    else:
        out["audit"] = b_aud

    # Clippy: smaller is better. Only update if current is "present".
    b_cl = baseline.get("clippy") or {}
    c_cl = current.get("clippy") or {}
    if c_cl.get("status") == "present":
        out["clippy"] = {
            "clippy_warnings": ratchet_int_min(
                b_cl.get("clippy_warnings"), c_cl.get("clippy_warnings")
            ),
            "non_clippy_warnings": ratchet_int_min(
                b_cl.get("non_clippy_warnings"), c_cl.get("non_clippy_warnings")
            ),
            "errors": ratchet_int_min(b_cl.get("errors"), c_cl.get("errors")),
            "status": "present",
        }
    else:
        out["clippy"] = b_cl

    out["frozenAt"] = datetime.now(tz=timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    out["schema_version"] = "1.0"
    out["_doc"] = (
        "Atualizado por scripts/quality/freeze-baseline.py. "
        "NÃO edite manualmente para 'passar' o ratchet — anti-fraud invariant "
        "(plan 0064 §Design invariants #7). Updates só via review humano + commit explícito."
    )

    return out


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("current", type=Path, help="path to current-metrics.json")
    parser.add_argument(
        "--baseline-in",
        type=Path,
        default=DEFAULT_BASELINE,
        help=f"input baseline path (default: {DEFAULT_BASELINE.relative_to(REPO_ROOT)})",
    )
    parser.add_argument(
        "--out",
        type=Path,
        default=DEFAULT_PROPOSED,
        help=f"output proposed-baseline path (default: {DEFAULT_PROPOSED.relative_to(REPO_ROOT)})",
    )
    parser.add_argument(
        "--seed",
        action="store_true",
        help="overwrite the input baseline directly (used ONCE for initial capture; "
             "after that, always go through .quality/baseline.proposed.json)",
    )
    args = parser.parse_args(argv[1:])

    current = json.loads(args.current.read_text(encoding="utf-8"))
    baseline = load_or_default_baseline(args.baseline_in)
    proposed = freeze(baseline, current)

    if args.seed:
        target = args.baseline_in
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(json.dumps(proposed, indent=2, sort_keys=True, ensure_ascii=False) + "\n", encoding="utf-8")
        print(f"[seed] wrote {target}")
    else:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(json.dumps(proposed, indent=2, sort_keys=True, ensure_ascii=False) + "\n", encoding="utf-8")
        print(f"[proposed] wrote {args.out}")
        print("Review the file, then `mv {0} .quality/baseline.json && git add .quality/baseline.json`".format(args.out))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
