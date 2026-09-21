#!/usr/bin/env python3
"""compare.py — deterministic baseline-vs-current comparator for the AI Quality Ratchet.

Usage:
    python3 compare.py --mode report-only|enforce \\
        path/to/baseline.json \\
        path/to/current-metrics.json \\
        [--base path/to/base-metrics.json] \\
        [--report path/to/quality-report.md]

Modes (per plan 0064 §"Design invariants" #2 — owner ajuste #1):
    --mode report-only  : ALWAYS exit 0. Writes quality-report.md with
                          PASS/WARN/REGRESSION rows. Used by PR-1 / PR-2 / PR-3
                          workflows where the ratchet is observation-only.
    --mode enforce      : Exit 1 if any REGRESSION row exists. Used by PR-4+
                          when the ratchet becomes blocking. Always writes
                          the same quality-report.md.

Per-PR signal (#1254):
    --base              : metrics collected at the PR merge-base (same shape as
                          current-metrics.json). When present, every table row
                          gains a `Δ nesta PR` column (current - base) and the
                          report opens with a verdict that lists ONLY what got
                          worse inside this PR. The baseline comparison stays
                          as-is (relabelled "vs baseline (<date>) — ver #1254").
                          `--base` never changes the exit code: report-only is
                          still always 0 and enforce still looks at the
                          baseline only. Without `--base` the report is
                          byte-identical to the historical one.
                          Each regression vs baseline is tagged with what the
                          merge-base says about it: `nova nesta PR` (worse than
                          the base), `pre-existente no merge-base` (same value
                          at the base) or `nao mensuravel no merge-base` (the
                          metric was not collected at the base — coverage in
                          CI, where lcov.info only exists for the head
                          checkout). The third state turns the verdict from ✅
                          into ⚠️: the report never claims a regression is
                          pre-existing when it was never measured there.

Stale baseline (#1254, criterio 3):
    When `frozenAt` (baseline) is more than BASELINE_MAX_AGE_DAYS before
    `collected_at` (current) a WARN row `baseline_age_days` is added. The age
    is computed from those two ISO-8601 UTC fields only — never from the wall
    clock — so the same inputs always yield the same report.

Determinism: pure data comparison. No network, no IA call, no shell-out.
Same input → same output, byte-identical.
"""

from __future__ import annotations

import argparse
import json
import sys
from datetime import datetime, timezone
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

# Above this many days between baseline.frozenAt and current.collected_at the
# baseline is considered stale and the report says so (#1254). Re-baselining
# is the owner's call (R5) — the comparator only reports.
BASELINE_MAX_AGE_DAYS = 90
STALE_BASELINE_ISSUE = "#1254"


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


def build_rows(reference: dict, current: dict, tolerance_pct: float) -> list[dict]:
    """All metric rows of `current` measured against `reference`.

    `reference` is the frozen baseline in the historical comparison and the PR
    merge-base metrics in the per-PR comparison (#1254) — same rules, same
    tolerance, so "regression vs baseline" and "regression in this PR" can
    never disagree about what counts as worse.
    """
    rows: list[dict] = []
    rows.append(compare_max_file_lines(reference, current, tolerance_pct))
    rows.append(compare_files_over_threshold(reference, current, "files_over_700", ">700 linhas"))
    rows.append(compare_files_over_threshold(reference, current, "files_over_1500", ">1500 linhas"))
    rows.append(compare_files_over_threshold(reference, current, "files_over_2500", ">2500 linhas"))
    rows.append(compare_coverage(reference, current, tolerance_pct))
    rows.extend(compare_audit(reference, current))
    rows.append(compare_clippy(reference, current))
    return rows


# --- stale baseline (#1254, criterio 3) -------------------------------------


def parse_utc_timestamp(value: Any) -> datetime | None:
    """ISO-8601 → aware UTC datetime, or None when absent/unparseable.

    Accepts the `Z` suffix the collector and freeze-baseline emit (Python
    < 3.11 `fromisoformat` does not) and treats a naive timestamp as UTC,
    which is what the project convention declares for these fields.
    """
    if not isinstance(value, str) or not value.strip():
        return None
    text = value.strip()
    if text.endswith(("Z", "z")):
        text = text[:-1] + "+00:00"
    try:
        parsed = datetime.fromisoformat(text)
    except ValueError:
        return None
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=timezone.utc)
    return parsed.astimezone(timezone.utc)


def baseline_age_days(baseline: dict, current: dict) -> int | None:
    """Whole days between baseline.frozenAt and current.collected_at.

    Deterministic on purpose: both instants come from the input files, never
    from the wall clock, so re-running on the same JSON gives the same report.
    """
    frozen = parse_utc_timestamp(baseline.get("frozenAt"))
    collected = parse_utc_timestamp(current.get("collected_at"))
    if frozen is None or collected is None:
        return None
    return (collected - frozen).days


def compare_baseline_age(
    baseline: dict, current: dict, max_age_days: int = BASELINE_MAX_AGE_DAYS
) -> dict | None:
    """WARN row when the baseline is stale or its age cannot be computed.

    Returns None when the baseline is fresh enough, so the historical report
    stays byte-identical in that case. Unknown age is reported (fail-closed):
    a baseline whose date cannot be read must not pass as fresh.
    """
    age = baseline_age_days(baseline, current)
    if age is None:
        return {
            "metric": "baseline_age_days",
            "status": "WARN",
            "baseline": max_age_days,
            "current": None,
            "note": (
                "Idade do baseline nao calculavel: `frozenAt` do baseline ou "
                "`collected_at` do current ausente/invalido (esperado ISO-8601 UTC). "
                f"Ver {STALE_BASELINE_ISSUE}."
            ),
        }
    if age > max_age_days:
        return {
            "metric": "baseline_age_days",
            "status": "WARN",
            "baseline": max_age_days,
            "current": age,
            "note": (
                f"Baseline congelado ha {age} dias (frozenAt {baseline.get('frozenAt')} "
                f"vs collected_at {current.get('collected_at')}), acima do teto de "
                f"{max_age_days}. Enquanto nao houver re-baseline, a secao 'vs baseline' "
                "repete o mesmo diagnostico em toda PR — use o veredito 'nesta PR' como "
                f"sinal. Re-baseline e decisao do dono; ver {STALE_BASELINE_ISSUE}."
            ),
        }
    return None


# --- per-PR delta (#1254) ----------------------------------------------------


def is_number(value: Any) -> bool:
    return isinstance(value, (int, float)) and not isinstance(value, bool)


def pr_deltas(rows_vs_base: list[dict]) -> dict[str, dict]:
    """metric -> {delta, measurable, worsened, base, current} vs the merge-base rows.

    `measurable` is whether both sides are numbers. When the metric was not
    collected at the merge-base (coverage in CI: lcov.info is downloaded into
    the head checkout only, never under /tmp/base) `delta` is None and NOTHING
    can be said about what this PR did to it — neither "nova" nor
    "pre-existente". `unmeasurable_regressions` turns that into a ⚠️.

    `worsened` reuses the row status (same rules as the baseline comparison)
    but additionally requires the value to have actually moved: an
    `audit_critical` that was already 1 at the merge-base is a REGRESSION vs
    anything (absolute zero) yet is not *new* in this PR. A REGRESSION whose
    delta cannot be computed counts as worsened (fail-closed).
    """
    out: dict[str, dict] = {}
    for row in rows_vs_base:
        base_value = row.get("baseline")
        current_value = row.get("current")
        measurable = is_number(base_value) and is_number(current_value)
        delta: int | float | None = None
        if measurable:
            delta = current_value - base_value
            if isinstance(delta, float):
                delta = round(delta, 2)
        worsened = row["status"] == "REGRESSION" and (delta is None or delta != 0)
        out[row["metric"]] = {
            "delta": delta,
            "measurable": measurable,
            "worsened": worsened,
            "base": base_value,
            "current": current_value,
        }
    return out


def fmt_delta(delta: int | float | None) -> str:
    if delta is None:
        return "n/a"
    if delta == 0:
        return "0"
    return f"{delta:+}"


UNMEASURABLE_TAG = "nao mensuravel no merge-base: metrica nao coletada no base"


def unmeasurable_regressions(rows: list[dict], pr: dict[str, dict]) -> list[str]:
    """Metrics that regressed vs baseline but have no computable `Δ nesta PR`.

    The CI shape for coverage: lcov.info is downloaded into the head checkout
    only, so the merge-base worktree reports `not_collected_this_run` and the
    row is a REGRESSION vs baseline with `n/a` vs base. It is neither `nova`
    nor `pre-existente` — the report has to say it cannot tell. A metric the
    fail-closed path already flagged `worsened` is not repeated here; a
    regression with no `pr` entry at all counts as unmeasurable (fail-closed).
    """
    out: list[str] = []
    for row in rows:
        if row["status"] != "REGRESSION":
            continue
        info = pr.get(row["metric"])
        if info is None or (not info["measurable"] and not info["worsened"]):
            out.append(row["metric"])
    return out


def render_unmeasurable_note(unmeasurable: list[str]) -> str:
    listed = ", ".join(f"`{m}`" for m in unmeasurable)
    return (
        f"{listed}: regrediu vs baseline mas nao pode ser comparada ao merge-base "
        "(metrica nao coletada la) — pode ou nao ser desta PR."
    )


def render_pr_verdict(
    pr: dict[str, dict], base: dict, unmeasurable: list[str] | None = None
) -> list[str]:
    worsened = [(metric, info) for metric, info in pr.items() if info["worsened"]]
    unmeasurable = unmeasurable or []
    base_sha = str(base.get("git_sha", "unknown"))[:12]
    out: list[str] = []
    if worsened:
        parts = [
            f"{metric} {fmt_delta(info['delta'])} ({info['base']} → {info['current']})"
            for metric, info in worsened
        ]
        out.append(f"> ## ❌ REGRESSAO NOVA nesta PR: {', '.join(parts)}")
    elif unmeasurable:
        # Nothing measurable got worse, but a regression vs baseline could not
        # be attributed either way. Not a ✅: the reader must not read it as
        # "pre-existing".
        out.append("> ## ⚠️ Sem regressao nova mensuravel nesta PR")
    else:
        out.append("> ## ✅ Sem regressao nova nesta PR")
    if unmeasurable:
        out.append(">")
        out.append(f"> ⚠️ {render_unmeasurable_note(unmeasurable)}")
    out.append(">")
    out.append(
        f"> `Δ nesta PR` = current − merge-base `{base_sha}` (metricas coletadas em "
        f"{base.get('collected_at', 'unknown')}). A secao 'vs baseline' abaixo compara "
        f"com o baseline congelado e pode repetir regressoes pre-existentes — ver "
        f"{STALE_BASELINE_ISSUE}."
    )
    return out


def render_markdown(
    rows: list[dict],
    baseline: dict,
    current: dict,
    mode: str,
    base: dict | None = None,
    pr: dict[str, dict] | None = None,
) -> str:
    with_base = base is not None and pr is not None
    frozen_date = str(baseline.get("frozenAt", "unknown"))[:10]
    new_in_pr = [m for m, info in (pr or {}).items() if info["worsened"]]
    unmeasurable = unmeasurable_regressions(rows, pr) if with_base else []

    out: list[str] = []
    out.append("# Quality Ratchet Report")
    out.append("")
    out.append(f"<!-- quality-ratchet-comment -->")
    out.append("")
    if with_base:
        out.extend(render_pr_verdict(pr, base, unmeasurable))
        out.append("")
    out.append(f"**Mode:** `{mode}`")
    out.append(f"**Baseline frozen:** {baseline.get('frozenAt', 'unknown')}")
    out.append(f"**Current collected:** {current.get('collected_at', 'unknown')}")
    out.append(f"**Current SHA:** `{current.get('git_sha', 'unknown')[:12]}`")
    if with_base:
        out.append(f"**Base (merge-base) SHA:** `{str(base.get('git_sha', 'unknown'))[:12]}`")
    out.append(f"**Collect mode:** `{current.get('collect_mode', 'unknown')}`")
    out.append("")

    out.append("## Resumo")
    out.append("")
    if with_base:
        out.append("| Métrica | Baseline | Current | Δ nesta PR | Status |")
        out.append("|---|---|---|---|---|")
    else:
        out.append("| Métrica | Baseline | Current | Status |")
        out.append("|---|---|---|---|")
    for row in rows:
        sym = {"PASS": "✅", "WARN": "⚠️", "REGRESSION": "❌"}.get(row["status"], "?")
        if with_base:
            info = pr.get(row["metric"])
            delta_cell = fmt_delta(info["delta"]) if info is not None else "n/a"
            out.append(
                f"| `{row['metric']}` | {row.get('baseline', 'n/a')} | "
                f"{row.get('current', 'n/a')} | {delta_cell} | {sym} {row['status']} |"
            )
        else:
            out.append(
                f"| `{row['metric']}` | {row.get('baseline', 'n/a')} | "
                f"{row.get('current', 'n/a')} | {sym} {row['status']} |"
            )
    out.append("")

    regressions = [r for r in rows if r["status"] == "REGRESSION"]
    warns = [r for r in rows if r["status"] == "WARN"]

    if regressions:
        if with_base:
            out.append(f"## ❌ Regressões vs baseline ({frozen_date}) — ver {STALE_BASELINE_ISSUE}")
        else:
            out.append("## ❌ Regressões Detectadas")
        out.append("")
        for r in regressions:
            out.append(f"### `{r['metric']}`")
            out.append("")
            out.append(f"- **Baseline:** {r.get('baseline')}")
            out.append(f"- **Current:**  {r.get('current')}")
            if "delta" in r:
                out.append(f"- **Delta:**    {r['delta']:+}")
            if with_base:
                info = pr.get(r["metric"])
                delta_cell = fmt_delta(info["delta"]) if info is not None else "n/a"
                if r["metric"] in new_in_pr:
                    tag = "nova nesta PR"
                elif r["metric"] in unmeasurable:
                    tag = UNMEASURABLE_TAG
                else:
                    tag = "pre-existente no merge-base"
                out.append(f"- **Δ nesta PR:** {delta_cell} ({tag})")
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
    if with_base and new_in_pr:
        listed = ", ".join(f"`{m}`" for m in new_in_pr)
        out.append(
            f"Resolva primeiro o que piorou **nesta PR** ({listed}) — "
            "**uma regressão por commit** (não tudo de uma vez). "
            "Veja `.claude/commands/quality-babysit.md` (auto-loop até N=5) — "
            "em PR-1 o modo é manual-only."
        )
        if unmeasurable:
            out.append("")
            out.append(f"⚠️ {render_unmeasurable_note(unmeasurable)}")
    elif with_base and unmeasurable:
        # `unmeasurable` is a subset of `regressions`, so there IS a regression
        # vs baseline — just not one this report can attribute to the PR.
        preexisting = [r["metric"] for r in regressions if r["metric"] not in unmeasurable]
        line = (
            "Nenhuma regressão nova **mensurável** nesta PR. "
            f"{render_unmeasurable_note(unmeasurable)} Confira essa métrica no que "
            "VOCÊ mudou antes de assumir que é pré-existente."
        )
        if preexisting:
            listed = ", ".join(f"`{m}`" for m in preexisting)
            line += (
                f" As demais ({listed}) já existiam no merge-base e não são desta PR — "
                f"re-baseline é decisão do dono, ver {STALE_BASELINE_ISSUE}."
            )
        out.append(line)
    elif with_base and regressions:
        out.append(
            "Nenhuma regressão nova nesta PR. As regressões listadas em "
            f"'vs baseline ({frozen_date})' já existiam no merge-base e não são "
            f"desta PR — re-baseline é decisão do dono, ver {STALE_BASELINE_ISSUE}."
        )
    elif regressions:
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
        "--base",
        type=Path,
        default=None,
        help=(
            "metrics collected at the PR merge-base (same shape as current). "
            "Adds the `Δ nesta PR` column and the per-PR verdict (#1254). "
            "Never affects the exit code."
        ),
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

    rows = build_rows(baseline, current, args.tolerance_pct)
    age_row = compare_baseline_age(baseline, current)
    if age_row is not None:
        rows.append(age_row)

    base: dict | None = None
    pr: dict[str, dict] | None = None
    if args.base is not None:
        base = load_json(args.base)
        pr = pr_deltas(build_rows(base, current, args.tolerance_pct))

    report = render_markdown(rows, baseline, current, args.mode, base=base, pr=pr)
    args.report.write_text(report, encoding="utf-8")
    print(report)

    # Exit code is decided by the baseline comparison only. `--base` is a
    # signal for humans reading the comment, not a gate (#1254).
    has_regression = any(r["status"] == "REGRESSION" for r in rows)
    if args.mode == "enforce" and has_regression:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
