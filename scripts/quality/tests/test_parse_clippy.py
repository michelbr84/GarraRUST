"""Tests for parse-clippy.py — fixture-driven."""

from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "parse-clippy.py"
FIXTURE = Path(__file__).parent / "fixtures" / "clippy-stream.jsonl"


def _load_module():
    spec = importlib.util.spec_from_file_location("parse_clippy", SCRIPT)
    mod = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(mod)
    return mod


def test_parse_stream():
    mod = _load_module()
    summary = mod.parse_jsonl_stream(FIXTURE.read_text())
    # fixture: 1 unused_variables warning (non-clippy) + 2 clippy warnings + 1 error
    assert summary["clippy_warnings"] == 2
    assert summary["non_clippy_warnings"] == 1
    assert summary["errors"] == 1
    assert summary["status"] == "present"


def test_cli_with_file():
    out = subprocess.run(
        [sys.executable, str(SCRIPT), str(FIXTURE)],
        capture_output=True,
        text=True,
        check=True,
    )
    payload = json.loads(out.stdout)
    assert payload["clippy_warnings"] == 2


def test_cli_with_stdin():
    out = subprocess.run(
        [sys.executable, str(SCRIPT)],
        input=FIXTURE.read_text(),
        capture_output=True,
        text=True,
        check=True,
    )
    payload = json.loads(out.stdout)
    assert payload["clippy_warnings"] == 2


def test_cli_with_empty_stdin():
    out = subprocess.run(
        [sys.executable, str(SCRIPT)],
        input="",
        capture_output=True,
        text=True,
        check=True,
    )
    payload = json.loads(out.stdout)
    assert payload["status"] == "not_collected_this_run"
