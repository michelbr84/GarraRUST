"""Tests for compare.py — exercising both modes + regression fabrication."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
COMPARE = ROOT / "compare.py"
FIXTURES = Path(__file__).resolve().parent / "fixtures"


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


def _base_equal_to_baseline() -> dict:
    """Merge-base metrics: same shape as current, collected a bit earlier."""
    b = _current_equal_to_baseline()
    b["collected_at"] = "2026-05-05T00:30:00Z"
    b["git_sha"] = "cafebabe" * 5
    return b


def _run(baseline_path, current_path, mode, report_path, base_path=None):
    cmd = [sys.executable, str(COMPARE), str(baseline_path), str(current_path),
           "--mode", mode, "--report", str(report_path)]
    if base_path is not None:
        cmd += ["--base", str(base_path)]
    return subprocess.run(cmd, capture_output=True, text=True)


def _write(tmp_path, baseline, current, base=None):
    b = tmp_path / "baseline.json"
    c = tmp_path / "current.json"
    r = tmp_path / "report.md"
    b.write_text(json.dumps(baseline))
    c.write_text(json.dumps(current))
    if base is None:
        return b, c, r, None
    p = tmp_path / "base.json"
    p.write_text(json.dumps(base))
    return b, c, r, p


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


# --- #1254: sem --base o relatorio e identico ao de sempre -------------------


def test_report_without_base_is_byte_identical_to_golden(tmp_path):
    """The golden was produced by compare.py BEFORE #1254 on these exact inputs.

    One REGRESSION (max_file_lines) and one WARN (clippy not collected) so every
    section of the historical layout is exercised. Baseline is fresh (1 h), so
    no `baseline_age_days` row is expected either.
    """
    cur = _current_equal_to_baseline()
    cur["max_file_lines"] = 3500
    cur["max_file_path"] = "crates/foo/src/bigger.rs"
    cur["clippy"] = {
        "clippy_warnings": 0, "non_clippy_warnings": 0, "errors": 0,
        "status": "not_collected_this_run",
        "reason": "fast_mode_skips_clippy_use_full_for_offline_run",
    }
    b, c, r, _ = _write(tmp_path, _baseline(), cur)

    out = _run(b, c, "report-only", r)
    assert out.returncode == 0
    golden = (FIXTURES / "report-no-base-golden.md").read_text(encoding="utf-8")
    assert r.read_text(encoding="utf-8") == golden
    assert "Δ nesta PR" not in golden
    assert "nesta PR" not in golden
    assert "baseline_age_days" not in golden


# --- #1254 (A): delta vs merge-base ------------------------------------------


def test_base_positive_delta_is_new_regression(tmp_path):
    """files_over_700 goes 34 → 36 inside the PR: the verdict names it, with the delta."""
    cur = _current_equal_to_baseline()
    cur["files_over_700"] = 36
    b, c, r, p = _write(tmp_path, _baseline(), cur, _base_equal_to_baseline())

    out = _run(b, c, "report-only", r, p)
    assert out.returncode == 0  # report-only NEVER fails, --base included
    text = r.read_text(encoding="utf-8")
    assert "❌ REGRESSAO NOVA nesta PR: files_over_700 +2 (34 → 36)" in text
    assert "Sem regressao nova" not in text
    assert "| Métrica | Baseline | Current | Δ nesta PR | Status |" in text
    assert "| `files_over_700` | 34 | 36 | +2 | ❌ REGRESSION |" in text
    assert "| `files_over_1500` | 7 | 7 | 0 | ✅ PASS |" in text
    assert "**Base (merge-base) SHA:** `cafebabecafe`" in text
    # Baseline section still there, relabelled with the frozen date + issue.
    assert "## ❌ Regressões vs baseline (2026-05-05) — ver #1254" in text
    assert "Regressões Detectadas" not in text
    assert "- **Δ nesta PR:** +2 (nova nesta PR)" in text
    # Next step points at what is new in THIS PR.
    assert "Resolva primeiro o que piorou **nesta PR** (`files_over_700`)" in text


def test_base_zero_delta_with_worse_baseline_is_not_new(tmp_path):
    """The real #1235 case: baseline 34, merge-base 36, PR 36.

    Vs baseline it is a REGRESSION (and stays one), but the PR changed nothing,
    so the verdict must say so instead of repeating the same complaint.
    """
    cur = _current_equal_to_baseline()
    cur["files_over_700"] = 36
    base = _base_equal_to_baseline()
    base["files_over_700"] = 36
    b, c, r, p = _write(tmp_path, _baseline(), cur, base)

    out = _run(b, c, "report-only", r, p)
    assert out.returncode == 0
    text = r.read_text(encoding="utf-8")
    assert "> ## ✅ Sem regressao nova nesta PR" in text
    assert "REGRESSAO NOVA" not in text
    assert "| `files_over_700` | 34 | 36 | 0 | ❌ REGRESSION |" in text
    assert "## ❌ Regressões vs baseline (2026-05-05) — ver #1254" in text
    assert "- **Δ nesta PR:** 0 (pre-existente no merge-base)" in text
    assert "Nenhuma regressão nova nesta PR" in text

    # enforce semantics untouched: still decided by the baseline only.
    out_en = _run(b, c, "enforce", r, p)
    assert out_en.returncode == 1


def test_base_never_changes_exit_code(tmp_path):
    """New regression in the PR but still under the baseline: enforce stays green.

    baseline 34, merge-base 30, PR 32 — worse than the merge-base, better than
    the baseline. `--base` is a signal for the reader, never a gate.
    """
    cur = _current_equal_to_baseline()
    cur["files_over_700"] = 32
    base = _base_equal_to_baseline()
    base["files_over_700"] = 30
    b, c, r, p = _write(tmp_path, _baseline(), cur, base)

    out_en = _run(b, c, "enforce", r, p)
    assert out_en.returncode == 0
    text = r.read_text(encoding="utf-8")
    assert "❌ REGRESSAO NOVA nesta PR: files_over_700 +2 (30 → 32)" in text
    assert "| `files_over_700` | 34 | 32 | +2 | ✅ PASS |" in text
    assert "Regressões vs baseline" not in text  # nothing regressed vs baseline

    out_ro = _run(b, c, "report-only", r, p)
    assert out_ro.returncode == 0


def test_base_preexisting_audit_critical_is_not_new(tmp_path):
    """audit_critical is absolute zero vs baseline, but 1 → 1 is not new in this PR."""
    cur = _current_equal_to_baseline()
    cur["audit"] = dict(cur["audit"], critical=1, total=1)
    base = _base_equal_to_baseline()
    base["audit"] = dict(base["audit"], critical=1, total=1)
    b, c, r, p = _write(tmp_path, _baseline(), cur, base)

    out = _run(b, c, "enforce", r, p)
    assert out.returncode == 1  # absolute zero still bites vs baseline
    text = r.read_text(encoding="utf-8")
    assert "> ## ✅ Sem regressao nova nesta PR" in text
    assert "| `audit_critical` | 0 | 1 | 0 | ❌ REGRESSION |" in text


def test_base_new_audit_critical_is_new(tmp_path):
    cur = _current_equal_to_baseline()
    cur["audit"] = dict(cur["audit"], critical=1, total=1)
    b, c, r, p = _write(tmp_path, _baseline(), cur, _base_equal_to_baseline())

    _run(b, c, "report-only", r, p)
    text = r.read_text(encoding="utf-8")
    assert "❌ REGRESSAO NOVA nesta PR: audit_critical +1 (0 → 1)" in text


def test_base_coverage_regression_lists_negative_delta(tmp_path):
    cur = _current_equal_to_baseline()
    cur["coverage"] = {"coverage_pct": 48.5, "status": "present"}
    b, c, r, p = _write(tmp_path, _baseline(), cur, _base_equal_to_baseline())

    _run(b, c, "report-only", r, p)
    text = r.read_text(encoding="utf-8")
    assert "❌ REGRESSAO NOVA nesta PR: coverage_pct -1.5 (50.0 → 48.5)" in text
    assert "| `coverage_pct` | 50.0 | 48.5 | -1.5 | ❌ REGRESSION |" in text


def test_base_metric_not_collected_at_base_yields_na_delta(tmp_path):
    """The merge-base worktree has no lcov.info: coverage delta is n/a, not a regression."""
    base = _base_equal_to_baseline()
    base["coverage"] = {"coverage_pct": None, "status": "not_collected_this_run",
                        "reason": "lcov_missing"}
    b, c, r, p = _write(tmp_path, _baseline(), _current_equal_to_baseline(), base)

    out = _run(b, c, "report-only", r, p)
    assert out.returncode == 0
    text = r.read_text(encoding="utf-8")
    assert "> ## ✅ Sem regressao nova nesta PR" in text
    assert "| `coverage_pct` | 50.0 | 50.0 | n/a | ✅ PASS |" in text


# --- #1254 (B): baseline defasado --------------------------------------------


def test_stale_baseline_emits_warn_row(tmp_path):
    """frozenAt 2026-05-05 vs collected_at 2026-09-20 = 138 days > 90 → WARN."""
    cur = _current_equal_to_baseline()
    cur["collected_at"] = "2026-09-20T12:00:00Z"
    b, c, r, _ = _write(tmp_path, _baseline(), cur)

    out = _run(b, c, "enforce", r)
    assert out.returncode == 0  # WARN never blocks, even in enforce
    text = r.read_text(encoding="utf-8")
    assert "| `baseline_age_days` | 90 | 138 | ⚠️ WARN |" in text
    assert "Baseline congelado ha 138 dias" in text
    assert "#1254" in text
    assert "Todos os gates passaram" not in text


def test_fresh_baseline_has_no_age_row(tmp_path):
    """Exactly 90 days is not 'more than 90 days'."""
    cur = _current_equal_to_baseline()
    cur["collected_at"] = "2026-08-03T00:00:00Z"
    b, c, r, _ = _write(tmp_path, _baseline(), cur)

    _run(b, c, "report-only", r)
    text = r.read_text(encoding="utf-8")
    assert "baseline_age_days" not in text
    assert "Todos os gates passaram" in text


def test_stale_baseline_age_is_deterministic_and_clock_free(tmp_path):
    """Same inputs → same age, whatever day the comparator runs on."""
    cur = _current_equal_to_baseline()
    cur["collected_at"] = "2026-09-20T12:00:00Z"
    b, c, r, _ = _write(tmp_path, _baseline(), cur)

    _run(b, c, "report-only", r)
    first = r.read_text(encoding="utf-8")
    _run(b, c, "report-only", r)
    assert r.read_text(encoding="utf-8") == first


def test_unparseable_timestamps_warn_instead_of_passing_as_fresh(tmp_path):
    cur = _current_equal_to_baseline()
    del cur["collected_at"]
    b, c, r, _ = _write(tmp_path, _baseline(), cur)

    out = _run(b, c, "enforce", r)
    assert out.returncode == 0
    text = r.read_text(encoding="utf-8")
    assert "| `baseline_age_days` | 90 | None | ⚠️ WARN |" in text
    assert "Idade do baseline nao calculavel" in text


def test_stale_baseline_and_base_together(tmp_path):
    """The CI shape for a PR today: stale baseline + merge-base metrics."""
    cur = _current_equal_to_baseline()
    cur["collected_at"] = "2026-09-20T12:00:00Z"
    base = _base_equal_to_baseline()
    base["collected_at"] = "2026-09-20T11:50:00Z"
    b, c, r, p = _write(tmp_path, _baseline(), cur, base)

    out = _run(b, c, "report-only", r, p)
    assert out.returncode == 0
    text = r.read_text(encoding="utf-8")
    assert "> ## ✅ Sem regressao nova nesta PR" in text
    assert "| `baseline_age_days` | 90 | 138 | n/a | ⚠️ WARN |" in text
