"""Tests for parse-cargo-audit.py — fixture-driven."""

from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "parse-cargo-audit.py"
FIXTURES = Path(__file__).parent / "fixtures"
CLEAN = FIXTURES / "cargo-audit-clean.json"
VULNS = FIXTURES / "cargo-audit-vulns.json"


def _load_module():
    spec = importlib.util.spec_from_file_location("parse_cargo_audit", SCRIPT)
    mod = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(mod)
    return mod


def test_parse_clean():
    mod = _load_module()
    summary = mod.parse_audit(json.loads(CLEAN.read_text()))
    assert summary["total"] == 0
    assert summary["high"] == 0
    assert summary["critical"] == 0
    assert summary["status"] == "present"


def test_parse_with_vulns():
    mod = _load_module()
    summary = mod.parse_audit(json.loads(VULNS.read_text()))
    # fixture: 1 high + 1 medium + 1 advisory without severity (unknown)
    assert summary["high"] == 1
    assert summary["medium"] == 1
    assert summary["unknown"] == 1
    assert summary["critical"] == 0
    assert summary["total"] == 3
    assert summary["status"] == "present"


def test_cli_with_file_arg():
    out = subprocess.run(
        [sys.executable, str(SCRIPT), str(VULNS)],
        capture_output=True,
        text=True,
        check=True,
    )
    payload = json.loads(out.stdout)
    assert payload["total"] == 3
    assert payload["high"] == 1


def test_cli_missing_file_returns_zero(tmp_path):
    missing = tmp_path / "absent.json"
    out = subprocess.run(
        [sys.executable, str(SCRIPT), str(missing)],
        capture_output=True,
        text=True,
        check=True,
    )
    payload = json.loads(out.stdout)
    assert payload["status"] == "not_collected_this_run"
    assert payload["total"] == 0
