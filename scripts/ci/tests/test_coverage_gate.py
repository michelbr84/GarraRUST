"""Tests for scripts/ci/coverage_gate.py (#1566, criterio 2)."""

from __future__ import annotations

import importlib.util
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "coverage_gate.py"
# Reuse the parser fixture: file1 LF=4 LH=3, file2 LF=2 LH=0, file3 LF=4 LH=4
# → 7/10 lines = 70.00%
FIXTURE = ROOT.parent / "quality" / "tests" / "fixtures" / "sample.lcov"


def _load_module():
    spec = importlib.util.spec_from_file_location("coverage_gate", SCRIPT)
    mod = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(mod)
    return mod


def test_shares_the_parser_with_freeze_baseline():
    """The gate must not carry a second implementation of coverage_pct."""
    mod = _load_module()
    assert mod.PARSER_PATH.name == "parse-llvm-cov.py"
    assert mod.PARSER_PATH.exists(), f"{mod.PARSER_PATH} moved; the gate would break"


def test_passes_when_coverage_is_above_floor():
    mod = _load_module()
    code, msg = mod.evaluate(FIXTURE, 65.0)
    assert code == 0
    assert "70.00%" in msg
    assert "7/10 lines" in msg


def test_floor_is_inclusive():
    """Exactly at the floor passes — the comparison is inclusive, matching
    the maintainer-approved aggregate guardrail (2026-10-07) and the
    ROADMAP's '>= 70%' phrasing for domain crates."""
    mod = _load_module()
    code, _ = mod.evaluate(FIXTURE, 70.0)
    assert code == 0


def test_fails_just_below_the_floor():
    mod = _load_module()
    code, msg = mod.evaluate(FIXTURE, 70.01)
    assert code == 1
    assert "below the floor" in msg


def test_missing_lcov_fails_closed(tmp_path):
    """An unmeasured run must not read as a passing run."""
    mod = _load_module()
    code, msg = mod.evaluate(tmp_path / "nope.lcov", 70.0)
    assert code == 1
    assert "not measurable" in msg


def test_empty_lcov_fails_closed(tmp_path):
    mod = _load_module()
    empty = tmp_path / "lcov.info"
    empty.write_text("", encoding="utf-8")
    code, msg = mod.evaluate(empty, 70.0)
    assert code == 1
    assert "not measurable" in msg


def test_zero_coverage_is_a_failure_not_an_error(tmp_path):
    """LF with no LH is measurable and simply 0% — must fail on the floor."""
    mod = _load_module()
    lcov = tmp_path / "lcov.info"
    lcov.write_text("SF:a.rs\nLF:10\nLH:0\nend_of_record\n", encoding="utf-8")
    code, msg = mod.evaluate(lcov, 70.0)
    assert code == 1
    assert "below the floor" in msg


def test_cli_exit_zero_above_floor():
    proc = subprocess.run(
        [sys.executable, str(SCRIPT), str(FIXTURE), "--floor", "70.0"],
        capture_output=True,
        text=True,
    )
    assert proc.returncode == 0, proc.stderr
    assert "OK: coverage" in proc.stdout


def test_cli_exit_one_below_floor():
    proc = subprocess.run(
        [sys.executable, str(SCRIPT), str(FIXTURE), "--floor", "90.0"],
        capture_output=True,
        text=True,
    )
    assert proc.returncode == 1
    assert "FAIL" in proc.stderr


def test_cli_rejects_out_of_range_floor():
    proc = subprocess.run(
        [sys.executable, str(SCRIPT), str(FIXTURE), "--floor", "101"],
        capture_output=True,
        text=True,
    )
    assert proc.returncode == 2
    assert "must be in [0, 100]" in proc.stderr
