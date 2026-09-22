"""Tests for freeze-baseline.py — strict ratchet plus the audited
`--adopt-current-file-metrics --reason '#NNN'` re-baseline (#1254)."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
FREEZE = ROOT / "freeze-baseline.py"


def _baseline() -> dict:
    return {
        "schema_version": "1.0",
        "frozenAt": "2026-05-05T11:20:45Z",
        "max_file_lines": 3240,
        "max_file_path": "crates/garraia-gateway/src/admin/handlers.rs",
        "files_over_700": 34,
        "files_over_1500": 7,
        "files_over_2500": 1,
        "total_rs_files": 334,
        "coverage": {"coverage_pct": 50.0, "status": "present"},
        "audit": {
            "critical": 0, "high": 1, "medium": 2, "low": 3,
            "informational": 0, "unknown": 0, "total": 6, "status": "present",
        },
        "clippy": {
            "clippy_warnings": 0, "non_clippy_warnings": 0, "errors": 0, "status": "present",
        },
    }


def _current_worse() -> dict:
    """Main far past the baseline in file size AND worse in audit/clippy/coverage."""
    return {
        "schema_version": "1.0",
        "collected_at": "2026-09-22T01:36:59Z",
        "git_sha": "2f9a882" + "0" * 33,
        "collect_mode": "fast",
        "max_file_lines": 8475,
        "max_file_path": "crates/garraia-agents/src/runtime.rs",
        "files_over_700": 103,
        "files_over_1500": 34,
        "files_over_2500": 14,
        "total_rs_files": 579,
        "coverage": {"coverage_pct": 40.0, "status": "present", "lines_total": 10, "lines_hit": 4},
        "audit": {
            "critical": 2, "high": 5, "medium": 6, "low": 7,
            "informational": 0, "unknown": 1, "total": 21, "status": "present",
        },
        "clippy": {
            "clippy_warnings": 9, "non_clippy_warnings": 4, "errors": 1, "status": "present",
        },
    }


def _run(tmp_path: Path, current: dict, *extra: str, baseline: dict | None = None):
    cur = tmp_path / "current.json"
    base = tmp_path / "baseline.json"
    out = tmp_path / "proposed.json"
    cur.write_text(json.dumps(current), encoding="utf-8")
    original = json.dumps(baseline if baseline is not None else _baseline())
    base.write_text(original, encoding="utf-8")
    proc = subprocess.run(
        [sys.executable, str(FREEZE), str(cur), "--baseline-in", str(base), "--out", str(out), *extra],
        capture_output=True, text=True, check=False,
    )
    # Invariant for every case: the input baseline is never modified.
    assert base.read_text(encoding="utf-8") == original
    return proc, out


def _load(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def test_default_run_keeps_strict_ratchet(tmp_path):
    proc, out = _run(tmp_path, _current_worse())
    assert proc.returncode == 0, proc.stderr
    p = _load(out)
    assert p["max_file_lines"] == 3240
    assert p["files_over_700"] == 34
    assert p["files_over_1500"] == 7
    assert p["files_over_2500"] == 1
    assert p["audit"]["critical"] == 0
    assert p["audit"]["high"] == 1
    assert p["clippy"]["clippy_warnings"] == 0
    assert p["coverage"]["coverage_pct"] == 50.0
    for key in ("adopted_reason", "source_git_sha", "source_collected_at"):
        assert key not in p


def test_default_run_drops_stale_adoption_provenance(tmp_path):
    base = _baseline()
    base.update(adopted_reason="#1254", source_git_sha="abc", source_collected_at="x")
    proc, out = _run(tmp_path, _current_worse(), baseline=base)
    assert proc.returncode == 0, proc.stderr
    p = _load(out)
    for key in ("adopted_reason", "source_git_sha", "source_collected_at"):
        assert key not in p


def test_adopt_takes_current_file_metrics(tmp_path):
    cur = _current_worse()
    proc, out = _run(tmp_path, cur, "--adopt-current-file-metrics", "--reason", "#1254")
    assert proc.returncode == 0, proc.stderr
    p = _load(out)
    assert p["max_file_lines"] == 8475
    assert p["max_file_path"] == "crates/garraia-agents/src/runtime.rs"
    assert p["files_over_700"] == 103
    assert p["files_over_1500"] == 34
    assert p["files_over_2500"] == 14
    assert p["total_rs_files"] == 579


def test_adopt_never_relaxes_audit_clippy_or_coverage(tmp_path):
    proc, out = _run(tmp_path, _current_worse(), "--adopt-current-file-metrics", "--reason", "#1254")
    assert proc.returncode == 0, proc.stderr
    p = _load(out)
    assert p["audit"]["critical"] == 0
    assert p["audit"]["high"] == 1
    assert p["audit"]["medium"] == 2
    assert p["audit"]["low"] == 3
    assert p["audit"]["unknown"] == 0
    assert p["audit"]["total"] == 6
    assert p["clippy"] == {
        "clippy_warnings": 0, "non_clippy_warnings": 0, "errors": 0, "status": "present",
    }
    assert p["coverage"]["coverage_pct"] == 50.0


def test_adopt_records_provenance(tmp_path):
    cur = _current_worse()
    proc, out = _run(tmp_path, cur, "--adopt-current-file-metrics", "--reason", "re-baseline #1254")
    assert proc.returncode == 0, proc.stderr
    p = _load(out)
    assert p["adopted_reason"] == "re-baseline #1254"
    assert p["source_git_sha"] == cur["git_sha"]
    assert p["source_collected_at"] == cur["collected_at"]


def test_adopt_without_reason_exits_nonzero_and_writes_nothing(tmp_path):
    proc, out = _run(tmp_path, _current_worse(), "--adopt-current-file-metrics")
    assert proc.returncode != 0
    assert not out.exists()


def test_adopt_with_reason_lacking_issue_ref_is_rejected(tmp_path):
    proc, out = _run(tmp_path, _current_worse(), "--adopt-current-file-metrics", "--reason", "baseline velho")
    assert proc.returncode != 0
    assert not out.exists()


@pytest.mark.parametrize("reason", ["#0", "foo#1", "#12abc", "##", "#007"])
def test_adopt_with_loose_issue_ref_is_rejected(tmp_path, reason):
    proc, out = _run(tmp_path, _current_worse(), "--adopt-current-file-metrics", "--reason", reason)
    assert proc.returncode != 0
    assert not out.exists()


@pytest.mark.parametrize("reason", ["#1254", "re-baseline (#1254)", "#7: velho"])
def test_adopt_with_real_issue_ref_is_accepted(tmp_path, reason):
    proc, out = _run(tmp_path, _current_worse(), "--adopt-current-file-metrics", "--reason", reason)
    assert proc.returncode == 0, proc.stderr
    assert out.exists()


def test_reason_without_adopt_is_rejected(tmp_path):
    proc, out = _run(tmp_path, _current_worse(), "--reason", "#1254")
    assert proc.returncode != 0
    assert not out.exists()


def test_adopt_combined_with_seed_is_rejected(tmp_path):
    proc, out = _run(
        tmp_path, _current_worse(), "--adopt-current-file-metrics", "--reason", "#1254", "--seed",
    )
    assert proc.returncode != 0
    assert not out.exists()


def test_adopt_refuses_out_pointing_at_baseline_in(tmp_path):
    cur = tmp_path / "current.json"
    base = tmp_path / "baseline.json"
    cur.write_text(json.dumps(_current_worse()), encoding="utf-8")
    original = json.dumps(_baseline())
    base.write_text(original, encoding="utf-8")
    proc = subprocess.run(
        [sys.executable, str(FREEZE), str(cur), "--baseline-in", str(base), "--out", str(base),
         "--adopt-current-file-metrics", "--reason", "#1254"],
        capture_output=True, text=True, check=False,
    )
    assert proc.returncode != 0
    assert base.read_text(encoding="utf-8") == original


def test_adopt_refuses_missing_provenance(tmp_path):
    cur = _current_worse()
    del cur["git_sha"]
    proc, out = _run(tmp_path, cur, "--adopt-current-file-metrics", "--reason", "#1254")
    assert proc.returncode != 0
    assert not out.exists()


def test_adopt_refuses_partial_file_metrics(tmp_path):
    cur = _current_worse()
    cur["files_over_2500"] = None
    proc, out = _run(tmp_path, cur, "--adopt-current-file-metrics", "--reason", "#1254")
    assert proc.returncode != 0
    assert not out.exists()
