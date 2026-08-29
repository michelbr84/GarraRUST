#!/usr/bin/env python3
"""Agrega alertas de code scanning por regra, separando produção de teste.

Existe porque `GET /repos/{owner}/{repo}/code-scanning/alerts` exige o escopo
`security_events`, que tokens de integração normalmente não têm. O
`GITHUB_TOKEN` de um job com `permissions: security-events: read` tem — por isso
este script é chamado por `.github/workflows/codeql-triage.yml`.

Motivação concreta: em 2026-08-28 o bundle do CodeQL nos runners subiu de 2.26.3
para 2.26.4 e o extractor de Rust saiu de 118 arquivos extraídos sem erro para
422 (de 425). A onda de alertas resultante só era diagnosticável clicando no
Security tab, um a um. Este script transforma isso em uma tabela.

Uso:
    GITHUB_TOKEN=... python3 scripts/security/codeql-alert-report.py \
        --repo michelbr84/GarraRUST --severity critical --state open

Saídas:
    --json-out    lista crua de alertas (default: codeql-alerts.json)
    --md-out      relatório markdown (default: stdout)

Exit codes:
    0  sucesso
    1  erro de uso
    2  falha de API (inclui 403 quando falta o escopo security_events)
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

API_ROOT = os.environ.get("GITHUB_API_URL", "https://api.github.com")
PER_PAGE = 100
MAX_PAGES = 100  # 10k alertas — teto de sanidade, não um limite esperado.
PRODUCTION_ROWS_SHOWN = 50

# Caminhos que são alvo de teste dedicado (arquivo inteiro é teste).
TEST_PATH_RE = re.compile(
    r"(^|/)tests?/"  # crates/foo/tests/bar.rs, tests/playwright/...
    r"|(^|/)benches/"
    r"|_test\.[A-Za-z0-9]+$"
    r"|\.spec\.[A-Za-z0-9]+$"
)
CFG_TEST_RE = re.compile(r"^\s*#\[cfg\(test\)\]")


def fail(msg: str, code: int = 2) -> None:
    print(f"error: {msg}", file=sys.stderr)
    raise SystemExit(code)


def fetch_alerts(repo: str, token: str, state: str, severity: str, tool: str) -> list[dict]:
    """Pagina a lista de alertas. Falha explicitamente em 403 (escopo faltando)."""
    alerts: list[dict] = []
    for page in range(1, MAX_PAGES + 1):
        query = {"per_page": str(PER_PAGE), "page": str(page)}
        if state:
            query["state"] = state
        if severity:
            query["severity"] = severity
        if tool:
            query["tool_name"] = tool
        url = f"{API_ROOT}/repos/{repo}/code-scanning/alerts?{urllib.parse.urlencode(query)}"
        req = urllib.request.Request(
            url,
            headers={
                "Accept": "application/vnd.github+json",
                "Authorization": f"Bearer {token}",
                "X-GitHub-Api-Version": "2022-11-28",
                "User-Agent": "garraia-codeql-triage",
            },
        )
        try:
            with urllib.request.urlopen(req, timeout=30) as resp:
                batch = json.load(resp)
        except urllib.error.HTTPError as exc:
            body = exc.read().decode("utf-8", "replace")[:400]
            if exc.code == 403:
                fail(
                    "403 ao listar alertas. O token não tem o escopo "
                    "`security_events`, ou code scanning não está habilitado "
                    f"neste repositório. Resposta: {body}"
                )
            fail(f"HTTP {exc.code} ao listar alertas: {body}")
        except urllib.error.URLError as exc:
            fail(f"falha de rede ao listar alertas: {exc.reason}")

        if not isinstance(batch, list):
            fail(f"resposta inesperada da API (esperava lista): {str(batch)[:200]}")
        alerts.extend(batch)
        if len(batch) < PER_PAGE:
            break
    return alerts


def cfg_test_ranges(path: Path) -> list[tuple[int, int]]:
    """Intervalos de linha (1-based, inclusivos) cobertos por `#[cfg(test)]`.

    Conta chaves a partir da primeira `{` após o atributo. Ignora chaves dentro
    de string/char/comentário — é heurística de dimensionamento, não parser.
    """
    try:
        lines = path.read_text(encoding="utf-8", errors="replace").split("\n")
    except OSError:
        return []

    ranges: list[tuple[int, int]] = []
    for i, line in enumerate(lines):
        if not CFG_TEST_RE.match(line):
            continue
        depth = 0
        started = False
        end = len(lines)
        for j in range(i, len(lines)):
            for ch in lines[j]:
                if ch == "{":
                    depth += 1
                    started = True
                elif ch == "}":
                    depth -= 1
            if started and depth <= 0:
                end = j + 1
                break
        ranges.append((i + 1, end))
    return ranges


def classify(alert: dict, repo_root: Path, cache: dict[str, list[tuple[int, int]]]) -> str:
    """'producao' | 'teste' | 'desconhecido'."""
    loc = (alert.get("most_recent_instance") or {}).get("location") or {}
    path = loc.get("path") or ""
    line = loc.get("start_line") or 0
    if not path:
        return "desconhecido"
    if TEST_PATH_RE.search(path):
        return "teste"
    if path.endswith(".rs") and line:
        if path not in cache:
            cache[path] = cfg_test_ranges(repo_root / path)
        for lo, hi in cache[path]:
            if lo <= line <= hi:
                return "teste"
    return "producao"


def build_report(alerts: list[dict], repo_root: Path, severity: str, state: str) -> str:
    cache: dict[str, list[tuple[int, int]]] = {}
    buckets: dict[tuple[str, str], dict] = {}
    classified: list[tuple[dict, str]] = []

    for alert in alerts:
        kind = classify(alert, repo_root, cache)
        classified.append((alert, kind))
        rule = alert.get("rule") or {}
        rule_id = rule.get("id") or "(sem rule id)"
        sev = rule.get("security_severity_level") or rule.get("severity") or "?"
        row = buckets.setdefault(
            (rule_id, sev),
            {"rule": rule_id, "sev": sev, "total": 0, "producao": 0, "teste": 0, "desconhecido": 0},
        )
        row["total"] += 1
        row[kind] += 1

    rows = sorted(buckets.values(), key=lambda r: (-r["total"], r["rule"]))
    totals = {
        k: sum(r[k] for r in rows) for k in ("total", "producao", "teste", "desconhecido")
    } if rows else {"total": 0, "producao": 0, "teste": 0, "desconhecido": 0}

    out: list[str] = []
    out.append(f"## Code scanning — severidade `{severity or '(todas)'}`, estado `{state or '(todos)'}`")
    out.append("")
    out.append(
        f"**{totals['total']}** alertas: {totals['producao']} em produção, "
        f"{totals['teste']} em código de teste, {totals['desconhecido']} não classificados."
    )
    out.append("")
    if not rows:
        out.append("Nenhum alerta para os filtros informados.")
        return "\n".join(out) + "\n"

    out.append("| Regra | Sev | Total | Produção | Teste | ? |")
    out.append("|---|---|---:|---:|---:|---:|")
    for r in rows:
        out.append(
            f"| `{r['rule']}` | {r['sev']} | {r['total']} | "
            f"{r['producao']} | {r['teste']} | {r['desconhecido']} |"
        )
    out.append("")

    prod = [(a, k) for a, k in classified if k == "producao"]
    out.append(f"<details><summary>Alertas em produção ({len(prod)})</summary>")
    out.append("")
    out.append("| # | Regra | Arquivo:linha |")
    out.append("|---|---|---|")
    for alert, _ in prod[:PRODUCTION_ROWS_SHOWN]:
        loc = (alert.get("most_recent_instance") or {}).get("location") or {}
        rule_id = (alert.get("rule") or {}).get("id") or "?"
        out.append(
            f"| {alert.get('number')} | `{rule_id}` | "
            f"`{loc.get('path')}:{loc.get('start_line')}` |"
        )
    if len(prod) > PRODUCTION_ROWS_SHOWN:
        out.append(f"| … | | _{len(prod) - PRODUCTION_ROWS_SHOWN} linhas omitidas — ver o JSON_ |")
    out.append("")
    out.append("</details>")
    out.append("")
    out.append(
        "A classificação produção-vs-teste é heurística (caminho de teste, ou "
        "linha dentro de um `#[cfg(test)]` inline) e serve para dimensionar a "
        "onda — não dispensa triagem individual."
    )
    return "\n".join(out) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--repo", default=os.environ.get("GITHUB_REPOSITORY", "michelbr84/GarraRUST"))
    parser.add_argument("--severity", default="critical", help="critical|high|medium|low|warning|note|error; vazio = todas")
    parser.add_argument("--state", default="open", help="open|closed|dismissed|fixed; vazio = todos")
    parser.add_argument("--tool", default="CodeQL", help="nome da ferramenta; vazio = todas")
    parser.add_argument("--json-out", default="codeql-alerts.json")
    parser.add_argument("--md-out", default="", help="arquivo markdown; vazio = stdout")
    parser.add_argument("--repo-root", default=".", help="raiz do checkout, para ler os #[cfg(test)]")
    args = parser.parse_args()

    if "/" not in args.repo:
        print("error: --repo deve ser owner/name", file=sys.stderr)
        return 1

    token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")
    if not token:
        print("error: defina GITHUB_TOKEN (precisa do escopo security_events)", file=sys.stderr)
        return 1

    alerts = fetch_alerts(args.repo, token, args.state, args.severity, args.tool)
    Path(args.json_out).write_text(json.dumps(alerts, indent=2), encoding="utf-8")
    print(f"alertas coletados: {len(alerts)} -> {args.json_out}", file=sys.stderr)

    report = build_report(alerts, Path(args.repo_root), args.severity, args.state)
    if args.md_out:
        Path(args.md_out).write_text(report, encoding="utf-8")
    else:
        sys.stdout.write(report)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
