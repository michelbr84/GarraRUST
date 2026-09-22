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

Audited re-baseline (#1254): `--adopt-current-file-metrics --reason '#NNN'`
is the one sanctioned way to RELAX the file-size metrics when the baseline has
gone stale (main far past it). Rules, all fail-closed:
    - Only max_file_lines, max_file_path, files_over_{700,1500,2500} and
      total_rs_files take the current values. Audit, coverage and clippy keep
      the strict ratchet above, and audit.critical stays 0.
    - --reason is required and must reference an issue (`#123`; not `#0`,
      `foo#1` or `#12abc`).
    - current-metrics.json must carry git_sha and collected_at; they are
      copied to source_git_sha / source_collected_at next to adopted_reason,
      so a reviewer can reproduce the file from that commit.
    - It still writes only the proposed file: --seed is rejected, and so is an
      --out that points at --baseline-in.
Without the flag the behavior is exactly the strict ratchet described above.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_BASELINE = REPO_ROOT / ".quality" / "baseline.json"
DEFAULT_PROPOSED = REPO_ROOT / ".quality" / "baseline.proposed.json"

# #1254: the metrics an audited re-baseline may adopt from current. Nothing
# security- or quality-related (audit, coverage, clippy) is ever on this list.
ADOPTABLE_FILE_METRICS = (
    "max_file_lines",
    "max_file_path",
    "files_over_700",
    "files_over_1500",
    "files_over_2500",
    "total_rs_files",
)
# Provenance keys written only by an audited adoption.
ADOPTION_KEYS = ("adopted_reason", "source_git_sha", "source_collected_at")
# An issue reference is `#` + a number with no leading zero, standing on its
# own: `#0`, `foo#1` and `#12abc` do not count as referencing an issue.
REASON_ISSUE_RE = re.compile(r"(?<![\w#])#[1-9]\d*\b")


class AdoptionError(ValueError):
    """The audited adoption cannot be done safely; nothing must be written."""


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


def adopt_current_file_metrics(proposed: dict, current: dict, reason: str) -> dict:
    """#1254: overwrite ONLY the file-size metrics with the measured current.

    Fail-closed: a missing metric or missing provenance raises AdoptionError
    instead of adopting a partial or unreproducible baseline.
    """
    if not REASON_ISSUE_RE.search(reason or ""):
        raise AdoptionError("--reason must reference an issue, e.g. '#1254'")
    missing = [k for k in ADOPTABLE_FILE_METRICS if current.get(k) is None]
    if missing:
        raise AdoptionError(
            "current metrics lack " + ", ".join(missing) + "; refusing a partial adoption"
        )
    sha = current.get("git_sha")
    collected_at = current.get("collected_at")
    if not sha or not collected_at:
        raise AdoptionError(
            "current metrics lack git_sha/collected_at; the adoption would not be reproducible"
        )
    out = dict(proposed)
    for key in ADOPTABLE_FILE_METRICS:
        out[key] = current[key]
    out["adopted_reason"] = reason
    out["source_git_sha"] = sha
    out["source_collected_at"] = collected_at
    return out


def freeze(baseline: dict, current: dict) -> dict:
    out = dict(baseline)  # copy
    # Provenance of an earlier audited adoption does not describe a strict
    # ratchet run: drop it so a later proposal never claims a source it lacks.
    for key in ADOPTION_KEYS:
        out.pop(key, None)

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
    parser.add_argument(
        "--adopt-current-file-metrics",
        action="store_true",
        help="audited re-baseline (#1254): take ONLY the file-size metrics from current "
             "(audit/coverage/clippy stay ratcheted); requires --reason '#NNN'",
    )
    parser.add_argument(
        "--reason",
        default=None,
        help="issue reference justifying --adopt-current-file-metrics, e.g. '#1254'",
    )
    args = parser.parse_args(argv[1:])

    if args.adopt_current_file_metrics:
        if args.seed:
            parser.error("--adopt-current-file-metrics cannot be combined with --seed")
        if not args.reason or not REASON_ISSUE_RE.search(args.reason):
            parser.error("--adopt-current-file-metrics requires --reason referencing an issue (#NNN)")
        if args.out.resolve() == args.baseline_in.resolve():
            parser.error("--adopt-current-file-metrics never writes --baseline-in; pick another --out")
    elif args.reason is not None:
        parser.error("--reason only applies to --adopt-current-file-metrics")

    current = json.loads(args.current.read_text(encoding="utf-8"))
    baseline = load_or_default_baseline(args.baseline_in)
    proposed = freeze(baseline, current)
    if args.adopt_current_file_metrics:
        try:
            proposed = adopt_current_file_metrics(proposed, current, args.reason)
        except AdoptionError as e:
            print(f"error: {e}", file=sys.stderr)
            return 2

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
