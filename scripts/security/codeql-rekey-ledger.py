#!/usr/bin/env python3
"""Reaponta as entradas do ledger para os alertas de code scanning vivos.

Existe porque `alert_number` **não é uma chave estável**. Quando o escopo da
análise muda — bundle do CodeQL, `paths-ignore`, `cargo_cfg_overrides`,
categoria do SARIF — os `partialFingerprints` mudam junto e o GitHub deixa de
casar os achados com os alertas existentes: ele **cria alertas novos** para o
mesmo código, nas mesmas linhas, com números novos. Os dismissals antigos ficam
grudados nas duplicatas aposentadas e o Security tab volta a acusar tudo.

Foi exatamente o que aconteceu em 2026-08-29: as 16 entradas
`rust/path-injection` de GAR-490 (alertas #67-#82) continuavam `dismissed` — o
dry-run do `codeql-reapply-dismissals.sh` reportou `skipped: 23
(already-dismissed)` — enquanto 16 alertas NOVOS (#147-#162) apontavam para os
mesmos `(rule_id, path, line)` e estavam abertos.

A chave natural é `(rule_id, path, line)`, que é o que o
`codeql-reapply-dismissals.sh` já valida fail-closed antes de reaplicar. Este
script usa essa chave para reconciliar os números e reescreve o ledger nos dois
arquivos (`.json` e `.md`) em sincronia, para o `--check-md` continuar passando.

O que este script **não** faz, deliberadamente:

* Não emite `PATCH` nenhum. Ele não dispensa alerta — só corrige a numeração.
  Aplicar dismissal continua sendo trabalho do `codeql-reapply-dismissals.sh`,
  depois que um humano revisar este diff em PR. É o que mantém a regra §3.1
  ("no bulk suppression") de pé: a justificativa de cada entrada foi revisada
  uma vez e continua valendo; só o ponteiro muda.
* Não inventa entrada. Se um alerta aberto não corresponde a nenhuma entrada do
  ledger, ele é ignorado (e contado no resumo) — alerta novo é para triar à mão.
* Não remove entrada. Entrada sem alerta vivo é reportada, não apagada.

Uso:
    GITHUB_TOKEN=... python3 scripts/security/codeql-rekey-ledger.py --dry-run
    GITHUB_TOKEN=... python3 scripts/security/codeql-rekey-ledger.py --apply

O `GET /code-scanning/alerts` exige o escopo `security_events`, que tokens de
integração normalmente não têm. Rode pelo workflow `codeql-triage.yml` (que tem
`permissions: security-events: read`) ou com um PAT que carregue o escopo.

Exit codes:
    0  sucesso (com ou sem mudança)
    1  erro de uso
    2  ambíguo: a mesma chave (rule_id, path, line) casou com mais de um alerta
       aberto. Fail-closed — reaudite à mão, não adivinhe.
    5  falha de precondição (ledger ausente/malformado, token ausente, API).

O exit 2 tambem cobre **drift**: entrada cujo `alert_number` continua aberto mas
cujo alerta vivo esta em outra `(rule_id, path, line)`. Ate 2026-08-30 esse caso
era pulado em silencio — so o numero era comparado — e foi como o #113 passou
tres meses descrevendo o statement errado. Nao ha o que reapontar ali: o numero
ja esta certo, o que apodreceu foi a linha, e corrigir isso exige reler o codigo.

Entrada sem alerta aberto correspondente **não** é erro: é o estado saudável de
uma entrada já dispensada e sem duplicata. Ela é listada no resumo para o
re-audit de 90 dias (§3.4 do ledger) e o exit continua 0.
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
MAX_PAGES = 100

DEFAULT_JSON = "docs/security/codeql-suppressions.json"
DEFAULT_MD = "docs/security/codeql-suppressions.md"


def fail(msg: str, code: int = 5) -> None:
    print(f"error: {msg}", file=sys.stderr)
    raise SystemExit(code)


def fetch_open_alerts(repo: str, token: str) -> list[dict]:
    """Lista todos os alertas abertos. Mesma paginação do codeql-alert-report.py."""
    alerts: list[dict] = []
    for page in range(1, MAX_PAGES + 1):
        query = {"per_page": str(PER_PAGE), "page": str(page), "state": "open"}
        url = f"{API_ROOT}/repos/{repo}/code-scanning/alerts?{urllib.parse.urlencode(query)}"
        req = urllib.request.Request(
            url,
            headers={
                "Accept": "application/vnd.github+json",
                "Authorization": f"Bearer {token}",
                "X-GitHub-Api-Version": "2022-11-28",
                "User-Agent": "garraia-codeql-rekey",
            },
        )
        try:
            with urllib.request.urlopen(req, timeout=30) as resp:
                batch = json.load(resp)
        except urllib.error.HTTPError as exc:
            body = exc.read().decode("utf-8", "replace")[:400]
            if exc.code == 403:
                fail(
                    "403 ao listar alertas. O token nao tem o escopo "
                    f"`security_events`. Resposta: {body}"
                )
            fail(f"HTTP {exc.code} ao listar alertas: {body}")
        except urllib.error.URLError as exc:
            fail(f"falha de rede ao listar alertas: {exc.reason}")

        if not isinstance(batch, list):
            fail(f"resposta inesperada da API: {str(batch)[:200]}")
        alerts.extend(batch)
        if len(batch) < PER_PAGE:
            break
    return alerts


def alert_key(alert: dict) -> tuple[str, str, int]:
    loc = alert.get("most_recent_instance", {}).get("location", {})
    return (
        alert.get("rule", {}).get("id", ""),
        loc.get("path", ""),
        int(loc.get("start_line", 0)),
    )


def entry_key(entry: dict) -> tuple[str, str, int]:
    return (entry["rule_id"], entry["path"], int(entry["line"]))


def plan_rekey(
    entries: list[dict], open_alerts: list[dict]
) -> tuple[dict[int, int], list[dict], list[tuple], list[tuple]]:
    """Decide o remapeamento. Retorna (mapping, sem_correspondencia, ambiguos, drift).

    Puro — sem I/O — para ser testável.
    """
    by_key: dict[tuple, list[int]] = {}
    for a in open_alerts:
        by_key.setdefault(alert_key(a), []).append(int(a["number"]))

    open_by_number: dict[int, tuple] = {int(a["number"]): alert_key(a) for a in open_alerts}

    mapping: dict[int, int] = {}
    unmatched: list[dict] = []
    ambiguous: list[tuple] = []
    drifted: list[tuple] = []

    for entry in entries:
        current = int(entry["alert_number"])
        key = entry_key(entry)
        candidates = by_key.get(key, [])

        if current in open_by_number:
            live = open_by_number[current]
            if live == key:
                # Ja aponta para um alerta aberto NESTA chave: nada a fazer.
                continue
            # O numero segue aberto, mas o alerta se move para outro
            # (rule_id, path, line). Ate 2026-08-30 este arm so olhava o numero,
            # e a entrada passava calada — foi assim que o #113 ficou tres meses
            # apontando para wizard/mod.rs:640 e descrevendo um `eprintln!`
            # enquanto o alerta vivo estava em :673, noutro statement. Nao da
            # para reapontar: o numero ja esta certo, o que apodreceu foi a
            # linha e, com ela, possivelmente a justificativa. Isso e re-audit
            # humano, nao remapeamento.
            drifted.append((entry, live))
            continue
        if not candidates:
            # Sem duplicata aberta. Normal para entrada ja dispensada e quieta.
            unmatched.append(entry)
            continue
        if len(candidates) > 1:
            ambiguous.append((entry, sorted(candidates)))
            continue

        target = candidates[0]
        if target != current:
            mapping[current] = target

    return mapping, unmatched, ambiguous, drifted


def rewrite_md(md_text: str, mapping: dict[int, int]) -> str:
    """Reescreve ancora, link e cross-references de cada numero remapeado.

    Substitui so os tres padroes exatos que o ledger usa, nunca o numero solto —
    um sed global em "67" destruiria linhas e datas.
    """
    # Placeholder em duas fases evita colisao quando um numero novo e igual a um
    # numero antigo ainda nao processado (ex.: 67 -> 149 e 149 -> 200).
    def phase(text: str, fmt_from, fmt_to) -> str:
        for old, new in mapping.items():
            text = text.replace(fmt_from(old), fmt_to(new))
        return text

    tmp = "\x00REKEY%d\x00"
    patterns = [
        (lambda n: f'<a id="alert-{n}"></a>', lambda n: f'<a id="alert-{n}"></a>'),
        (lambda n: f"(#alert-{n})", lambda n: f"(#alert-{n})"),
        (lambda n: f"/code-scanning/{n})", lambda n: f"/code-scanning/{n})"),
        (lambda n: f"[#{n}](https://github.com/michelbr84/GarraRUST/security/code-scanning/",
         lambda n: f"[#{n}](https://github.com/michelbr84/GarraRUST/security/code-scanning/"),
    ]
    for frm, to in patterns:
        # fase 1: antigo -> placeholder
        for old in mapping:
            md_text = md_text.replace(frm(old), frm(tmp % old))
        # fase 2: placeholder -> novo
        for old, new in mapping.items():
            md_text = md_text.replace(frm(tmp % old), to(new))
    return md_text


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--repo", default=os.environ.get("GITHUB_REPOSITORY", "michelbr84/GarraRUST"))
    ap.add_argument("--ledger-json", default=DEFAULT_JSON)
    ap.add_argument("--ledger-md", default=DEFAULT_MD)
    mode = ap.add_mutually_exclusive_group()
    mode.add_argument("--dry-run", action="store_true", help="default: so mostra o que mudaria")
    mode.add_argument("--apply", action="store_true", help="reescreve o ledger")
    args = ap.parse_args()

    token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")
    if not token:
        fail("GITHUB_TOKEN (ou GH_TOKEN) nao definido", 1)

    json_path, md_path = Path(args.ledger_json), Path(args.ledger_md)
    if not json_path.is_file():
        fail(f"ledger JSON nao encontrado: {json_path}")
    if not md_path.is_file():
        fail(f"ledger MD nao encontrado: {md_path}")

    try:
        ledger = json.loads(json_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        fail(f"ledger JSON malformado: {exc}")

    entries = ledger.get("entries", [])
    open_alerts = fetch_open_alerts(args.repo, token)
    print(f"alertas abertos: {len(open_alerts)} | entradas no ledger: {len(entries)}")

    mapping, unmatched, ambiguous, drifted = plan_rekey(entries, open_alerts)

    if drifted:
        print("\nDRIFT — o numero segue aberto, mas o alerta mudou de (rule_id, path, line):")
        for entry, live in drifted:
            print(f"  #{entry['alert_number']}  ledger: {entry['rule_id']} @ "
                  f"{entry['path']}:{entry['line']}")
            print(f"        {' ' * len(str(entry['alert_number']))}  vivo:   "
                  f"{live[0]} @ {live[1]}:{live[2]}")
        fail(
            "reaudite a mao: a linha do ledger apodreceu e a justificativa pode "
            "estar descrevendo outro statement",
            2,
        )

    if ambiguous:
        print("\nAMBIGUO — mesma (rule_id, path, line) casa com varios alertas abertos:")
        for entry, cands in ambiguous:
            print(f"  #{entry['alert_number']} {entry['rule_id']} @ {entry['path']}:{entry['line']}"
                  f" -> candidatos {cands}")
        fail("reaudite a mao; o script nao adivinha", 2)

    matched_keys = {entry_key(e) for e in entries}
    orphan_alerts = [a for a in open_alerts if alert_key(a) not in matched_keys]

    if not mapping:
        print("\nnada a reapontar: toda entrada ja aponta para alerta aberto, "
              "ou nao tem duplicata aberta.")
    else:
        print(f"\nreapontamentos ({len(mapping)}):")
        by_num = {int(e["alert_number"]): e for e in entries}
        for old, new in sorted(mapping.items()):
            e = by_num[old]
            print(f"  #{old} -> #{new}   {e['rule_id']} @ {e['path']}:{e['line']}")

    if unmatched:
        print(f"\nsem alerta aberto correspondente ({len(unmatched)}) — "
              "esperado para entrada ja dispensada e sem duplicata:")
        for e in unmatched:
            print(f"  #{e['alert_number']}  {e['rule_id']} @ {e['path']}:{e['line']}")

    if orphan_alerts:
        print(f"\nalertas abertos SEM entrada no ledger ({len(orphan_alerts)}) — "
              "triagem manual, este script nao inventa justificativa:")
        for a in sorted(orphan_alerts, key=lambda x: int(x["number"])):
            rid, path, line = alert_key(a)
            print(f"  #{a['number']}  {rid} @ {path}:{line}")

    if mapping and args.apply:
        for e in entries:
            cur = int(e["alert_number"])
            if cur in mapping:
                e["alert_number"] = mapping[cur]
                e["ledger_md_anchor"] = f"alert-{mapping[cur]}"
        json_path.write_text(json.dumps(ledger, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        md_path.write_text(rewrite_md(md_path.read_text(encoding="utf-8"), mapping), encoding="utf-8")
        print(f"\naplicado: {json_path} e {md_path} reescritos. "
              "Rode `--check-md` e revise o diff em PR antes de dispensar.")
    elif mapping:
        print("\n(dry-run — nada foi escrito; use --apply)")

    return 0


if __name__ == "__main__":
    sys.exit(main())
