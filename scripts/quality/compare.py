#!/usr/bin/env python3
"""compare.py — deterministic baseline-vs-current comparator for the AI Quality Ratchet.

Usage:
    python3 compare.py --mode report-only|enforce \\
        path/to/baseline.json \\
        path/to/current-metrics.json \\
        [--report path/to/quality-report.md]

Modes (per plan 0060 §"Design invariants" #2 — Michel ajuste #1):
    --mode report-only  : ALWAYS exit 0. Writes quality-report.md with
                          PASS/WARN/REGRESSION rows. Used by PR-1 / PR-2 / PR-3
                          workflows where the ratchet is observation-only.
    --mode enforce      : Exit 1 if any REGRESSION row exists. Used by PR-4+
                          when the ratchet becomes blocking. Always writes
                          the same quality-report.md.

Determinism: pure data comparison. No network, no IA call, no shell-out.
Same input → same output, byte-identical.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any

# Windows consoles default to cp1252; the report uses ✅/❌/⚠️ emoji.
# Reconfigure stdout/stderr to UTF-8 so `print()` can't blow up here. The
# file IO already uses encoding="utf-8" explicitly.
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8")  # type: ignore[attr-defined]
if hasattr(sys.stderr, "reconfigure"):
    sys.stderr.reconfigure(encoding="utf-8")  # type: ignore[attr-defined]

# Tolerance for float comparisons (coverage %, etc.). Avoids 0.05% noise
# triggering false regressions. Threshold is configurable per .quality/thresholds.toml
# but we hardcode here to keep the comparator single-purpose. PR-2 may expose this.
DEFAULT_TOLERANCE = 0.1


def load_json(path: Path) -> dict:
    with path.open("r", encoding="utf-8") as fh:
        return json.load(fh)


def coalesce(value: Any, fallback: Any) -> Any:
    return fallback if value is None else value


def compare_max_file_lines(baseline: dict, current: dict, tolerance_pct: float) -> dict:
    b = baseline.get("max_file_lines", 0)
    c = current.get("max_file_lines", 0)
    if c > b:
        return {
            "metric": "max_file_lines",
            "status": "REGRESSION",
            "baseline": b,
            "current": c,
            "delta": c - b,
            "fix": (
                f"Modularize {current.get('max_file_path', '<unknown>')} "
                f"(now {c} lines). Quebre em arquivos menores por responsabilidade. "
                "NÃO ignore o limite criando arquivos redundantes."
            ),
        }
    return {
        "metric": "max_file_lines",
        "status": "PASS",
        "baseline": b,
        "current": c,
    }


def compare_files_over_threshold(
    baseline: dict, current: dict, key: str, label: str
) -> dict:
    b = baseline.get(key, 0)
    c = current.get(key, 0)
    if c > b:
        return {
            "metric": key,
            "status": "REGRESSION",
            "baseline": b,
            "current": c,
            "delta": c - b,
            "fix": (
                f"Quantidade de arquivos {label} subiu de {b} para {c}. "
                "Modularize o(s) arquivo(s) que cruzaram o limite — veja `top_15` "
                "no current-metrics.json."
            ),
        }
    return {"metric": key, "status": "PASS", "baseline": b, "current": c}


def compare_coverage(baseline: dict, current: dict, tolerance_pct: float) -> dict:
    b_cov = baseline.get("coverage") or {}
    c_cov = current.get("coverage") or {}

    b_pct = b_cov.get("coverage_pct")
    c_pct = c_cov.get("coverage_pct")

    if c_cov.get("status") != "present":
        return {
            "metric": "coverage_pct",
            "status": "WARN",
            "baseline": b_pct,
            "current": None,
            "note": (
                f"Coverage não coletada neste run "
                f"(reason: {c_cov.get('reason', c_cov.get('status', 'unknown'))})."
            ),
        }

    if b_cov.get("status") != "present" or b_pct is None:
        return {
            "metric": "coverage_pct",
            "status": "PASS",
            "baseline": None,
            "current": c_pct,
            "note": "Baseline ainda não coletou coverage; current registrada para futuro.",
        }

    if c_pct + tolerance_pct < b_pct:
        return {
            "metric": "coverage_pct",
            "status": "REGRESSION",
            "baseline": b_pct,
            "current": c_pct,
            "delta": round(c_pct - b_pct, 2),
            "fix": (
                "Cobertura de testes caiu. Escreva testes para o código que "
                "VOCÊ adicionou neste PR. Não remova testes existentes."
            ),
        }
    return {"metric": "coverage_pct", "status": "PASS", "baseline": b_pct, "current": c_pct}


def compare_audit(baseline: dict, current: dict) -> list[dict]:
    rows: list[dict] = []
    b = baseline.get("audit") or {}
    c = current.get("audit") or {}

    if c.get("status") != "present":
        rows.append({
            "metric": "audit",
            "status": "WARN",
            "baseline": b.get("total"),
            "current": None,
            "note": f"Audit não coletada (reason: {c.get('reason', 'unknown')}).",
        })
        return rows

    # Critical = absolute zero (anti-fraud: cannot be added even if baseline had some).
    c_critical = c.get("critical", 0)
    b_critical = b.get("critical", 0) or 0
    if c_critical > 0:
        rows.append({
            "metric": "audit_critical",
            "status": "REGRESSION",
            "baseline": b_critical,
            "current": c_critical,
            "fix": "Vulnerabilidade CRÍTICA detectada. Atualize ou substitua a dep. NÃO use --force.",
        })
    else:
        rows.append({
            "metric": "audit_critical",
            "status": "PASS",
            "baseline": b_critical,
            "current": c_critical,
        })

    # High: not allowed to grow.
    b_high = b.get("high", 0) or 0
    c_high = c.get("high", 0)
    if c_high > b_high:
        rows.append({
            "metric": "audit_high",
            "status": "REGRESSION",
            "baseline": b_high,
            "current": c_high,
            "delta": c_high - b_high,
            "fix": "Nova vulnerabilidade HIGH em relação ao baseline. Atualize ou substitua a dep.",
        })
    else:
        rows.append({
            "metric": "audit_high",
            "status": "PASS",
            "baseline": b_high,
            "current": c_high,
        })

    return rows


def compare_clippy(baseline: dict, current: dict) -> dict:
    b = baseline.get("clippy") or {}
    c = current.get("clippy") or {}

    if c.get("status") != "present":
        return {
            "metric": "clippy_warnings",
            "status": "WARN",
            "baseline": b.get("clippy_warnings"),
            "current": None,
            "note": (
                f"Clippy não coletado neste run (reason: {c.get('reason', 'unknown')}). "
                "Em fast mode é esperado — `ci.yml` já roda clippy hard-blocking."
            ),
        }

    b_warn = (b.get("clippy_warnings") if b.get("status") == "present" else None) or 0
    c_warn = c.get("clippy_warnings", 0)
    if c_warn > b_warn:
        return {
            "metric": "clippy_warnings",
            "status": "REGRESSION",
            "baseline": b_warn,
            "current": c_warn,
            "delta": c_warn - b_warn,
            "fix": "Novos warnings clippy. Corrija TODOS os novos antes de commitar. NÃO use #[allow].",
        }
    return {
        "metric": "clippy_warnings",
        "status": "PASS",
        "baseline": b_warn,
        "current": c_warn,
    }


def render_markdown(rows: list[dict], baseline: dict, current: dict, mode: str) -> str:
    out: list[str] = []
    out.append("# Quality Ratchet Report")
    out.append("")
    out.append(f"<!-- quality-ratchet-comment -->")
    out.append("")
    out.append(f"**Mode:** `{mode}`")
    out.append(f"**Baseline frozen:** {baseline.get('frozenAt', 'unknown')}")
    out.append(f"**Current collected:** {current.get('collected_at', 'unknown')}")
    out.append(f"**Current SHA:** `{current.get('git_sha', 'unknown')[:12]}`")
    out.append(f"**Collect mode:** `{current.get('collect_mode', 'unknown')}`")
    out.append("")

    out.append("## Resumo")
    out.append("")
    out.append("| Métrica | Baseline | Current | Status |")
    out.append("|---|---|---|---|")
    for row in rows:
        sym = {"PASS": "✅", "WARN": "⚠️", "REGRESSION": "❌"}.get(row["status"], "?")
        out.append(
            f"| `{row['metric']}` | {row.get('baseline', 'n/a')} | "
            f"{row.get('current', 'n/a')} | {sym} {row['status']} |"
        )
    out.append("")

    regressions = [r for r in rows if r["status"] == "REGRESSION"]
    warns = [r for r in rows if r["status"] == "WARN"]

    if regressions:
        out.append("## ❌ Regressões Detectadas")
        out.append("")
        for r in regressions:
            out.append(f"### `{r['metric']}`")
            out.append("")
            out.append(f"- **Baseline:** {r.get('baseline')}")
            out.append(f"- **Current:**  {r.get('current')}")
            if "delta" in r:
                out.append(f"- **Delta:**    {r['delta']:+}")
            out.append(f"- **Fix:**      {r.get('fix', '(no fix hint)')}")
            out.append("")
        if mode == "enforce":
            out.append("> Mode `enforce` → exit 1. CI bloqueado.")
        else:
            out.append("> Mode `report-only` → exit 0. CI **não** bloqueado, mas regressão registrada.")
        out.append("")

    if warns:
        out.append("## ⚠️ Warnings (métrica não coletada / sem baseline)")
        out.append("")
        for w in warns:
            out.append(f"- `{w['metric']}`: {w.get('note', '(no note)')}")
        out.append("")

    if not regressions and not warns:
        out.append("## ✅ Todos os gates passaram")
        out.append("")
        for r in rows:
            out.append(f"- `{r['metric']}`: baseline {r.get('baseline')} / current {r.get('current')}")
        out.append("")

    out.append("## Próximo passo")
    out.append("")
    if regressions:
        out.append(
            "Resolva **uma regressão por commit** (não tudo de uma vez). "
            "Veja `.claude/commands/quality-babysit.md` (auto-loop até N=5) — "
            "em PR-1 o modo é manual-only."
        )
    else:
        out.append(
            "Nada a fazer agora. Métricas iguais ou melhores que o baseline."
        )
    out.append("")

    out.append("---")
    out.append(
        "_Generated by `scripts/quality/compare.py`. Determinístico — mesma "
        "entrada = mesma saída. Sem chamada de IA._"
    )
    return "\n".join(out) + "\n"


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path, help="path to .quality/baseline.json")
    parser.add_argument("current", type=Path, help="path to current-metrics.json")
    parser.add_argument(
        "--mode",
        choices=("report-only", "enforce"),
        required=True,
        help="report-only always exits 0; enforce exits 1 on REGRESSION",
    )
    parser.add_argument(
        "--report",
        type=Path,
        default=Path("quality-report.md"),
        help="output markdown report path (default: ./quality-report.md)",
    )
    parser.add_argument(
        "--tolerance-pct",
        type=float,
        default=DEFAULT_TOLERANCE,
        help=f"absolute tolerance for coverage (default: {DEFAULT_TOLERANCE})",
    )
    args = parser.parse_args(argv[1:])

    baseline = load_json(args.baseline)
    current = load_json(args.current)

    rows: list[dict] = []
    rows.append(compare_max_file_lines(baseline, current, args.tolerance_pct))
    rows.append(compare_files_over_threshold(baseline, current, "files_over_700", ">700 linhas"))
    rows.append(compare_files_over_threshold(baseline, current, "files_over_1500", ">1500 linhas"))
    rows.append(compare_files_over_threshold(baseline, current, "files_over_2500", ">2500 linhas"))
    rows.append(compare_coverage(baseline, current, args.tolerance_pct))
    rows.extend(compare_audit(baseline, current))
    rows.append(compare_clippy(baseline, current))

    report = render_markdown(rows, baseline, current, args.mode)
    args.report.write_text(report, encoding="utf-8")
    print(report)

    has_regression = any(r["status"] == "REGRESSION" for r in rows)
    if args.mode == "enforce" and has_regression:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
