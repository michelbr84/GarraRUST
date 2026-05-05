"""Tests for compare.py — exercising both modes + regression fabrication."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
COMPARE = ROOT / "compare.py"


def _baseline() -> dict:
    return {
        "schema_version": "1.0",
        "frozenAt": "2026-05-05T00:00:00Z",
        "max_file_lines": 3240,
        "max_file_path": "crates/foo/src/big.rs",
        "files_over_700": 34,
        "files_over_1500": 7,
        "files_over_2500": 1,
        "coverage": {"coverage_pct": 50.0, "status": "present"},
        "audit": {
            "critical": 0, "high": 0, "medium": 0, "low": 0,
            "informational": 0, "unknown": 0, "total": 0, "status": "present",
        },
        "clippy": {
            "clippy_warnings": 0, "non_clippy_warnings": 0, "errors": 0, "status": "present",
        },
    }


def _current_equal_to_baseline() -> dict:
    b = _baseline()
    b["collected_at"] = "2026-05-05T01:00:00Z"
    b["git_sha"] = "deadbeef" * 5
    b["collect_mode"] = "fast"
    b["total_rs_files"] = 334
    b["top_15"] = []
    return b


def _run(baseline_path, current_path, mode, report_path):
    return subprocess.run(
        [sys.executable, str(COMPARE), str(baseline_path), str(current_path),
         "--mode", mode, "--report", str(report_path)],
        capture_output=True, text=True,
    )


def test_pass_when_current_equals_baseline(tmp_path):
    b = tmp_path / "baseline.json"
    c = tmp_path / "current.json"
    r = tmp_path / "report.md"
    b.write_text(json.dumps(_baseline()))
    c.write_text(json.dumps(_current_equal_to_baseline()))

    out_ro = _run(b, c, "report-only", r)
    assert out_ro.returncode == 0
    assert r.read_text(encoding="utf-8").count("PASS") >= 5
    assert "Todos os gates passaram" in r.read_text(encoding="utf-8")

    out_en = _run(b, c, "enforce", r)
    assert out_en.returncode == 0


def test_regression_fails_only_in_enforce(tmp_path):
    b = tmp_path / "baseline.json"
    c = tmp_path / "current.json"
    r = tmp_path / "report.md"
    b.write_text(json.dumps(_baseline()))

    regressed = _current_equal_to_baseline()
    regressed["max_file_lines"] = 3500   # +260 over baseline
    regressed["max_file_path"] = "crates/foo/src/bigger.rs"
    c.write_text(json.dumps(regressed))

    out_en = _run(b, c, "enforce", r)
    assert out_en.returncode == 1
    assert "Regressões Detectadas" in r.read_text(encoding="utf-8")
    assert "max_file_lines" in r.read_text(encoding="utf-8")

    out_ro = _run(b, c, "report-only", r)
    assert out_ro.returncode == 0  # report-only NEVER fails
    assert "Regressões Detectadas" in r.read_text(encoding="utf-8")


def test_warn_when_metric_not_collected(tmp_path):
    b = tmp_path / "baseline.json"
    c = tmp_path / "current.json"
    r = tmp_path / "report.md"
    b.write_text(json.dumps(_baseline()))

    cur = _current_equal_to_baseline()
    cur["coverage"] = {
        "coverage_pct": None,
        "status": "not_collected_this_run",
        "reason": "lcov_missing",
    }
    c.write_text(json.dumps(cur))

    out = _run(b, c, "enforce", r)
    assert out.returncode == 0  # WARN does not block enforce
    text = r.read_text(encoding="utf-8")
    assert "WARN" in text
    assert "Coverage não coletada" in text


def test_audit_critical_is_absolute_zero(tmp_path):
    """Even if baseline somehow had critical=N, current critical>0 is ALWAYS regression."""
    b = tmp_path / "baseline.json"
    c = tmp_path / "current.json"
    r = tmp_path / "report.md"
    base = _baseline()
    c_dict = _current_equal_to_baseline()
    c_dict["audit"] = dict(c_dict["audit"], critical=1, total=1)
    b.write_text(json.dumps(base))
    c.write_text(json.dumps(c_dict))

    out = _run(b, c, "enforce", r)
    assert out.returncode == 1
    assert "audit_critical" in r.read_text(encoding="utf-8")
