"""Tests for parse-llvm-cov.py — fixture-driven."""

from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "parse-llvm-cov.py"
FIXTURE = Path(__file__).parent / "fixtures" / "sample.lcov"


def _load_module():
    spec = importlib.util.spec_from_file_location("parse_llvm_cov", SCRIPT)
    mod = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(mod)
    return mod


def test_parse_lcov_fixture():
    mod = _load_module()
    summary = mod.parse_lcov(FIXTURE)
    # fixture: file1 LF=4 LH=3, file2 LF=2 LH=0, file3 LF=4 LH=4
    # total = 10 lines, 7 hit → 70.0%
    assert summary["lines_total"] == 10
    assert summary["lines_hit"] == 7
    assert summary["files"] == 3
    assert summary["coverage_pct"] == 70.0
    assert summary["status"] == "present"


def test_parse_lcov_missing_file(tmp_path):
    mod = _load_module()
    summary = mod.parse_lcov(tmp_path / "does-not-exist.info")
    assert summary["status"] == "not_collected_this_run"
    assert summary["coverage_pct"] is None


def test_parse_lcov_empty_file(tmp_path):
    mod = _load_module()
    empty = tmp_path / "empty.info"
    empty.touch()
    summary = mod.parse_lcov(empty)
    assert summary["status"] == "not_collected_this_run"


def test_cli_invocation_emits_json(tmp_path):
    out = subprocess.run(
        [sys.executable, str(SCRIPT), str(FIXTURE)],
        capture_output=True,
        text=True,
        check=True,
    )
    payload = json.loads(out.stdout)
    assert payload["coverage_pct"] == 70.0
    assert payload["status"] == "present"
