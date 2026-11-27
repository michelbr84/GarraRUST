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
    GITHUB_TOKEN=... python3 scripts/security/codeql-rekey-ledger.py --dry-run --check-main

O `GET /code-scanning/alerts` exige o escopo `security_events`, que tokens de
integração normalmente não têm. Rode pelo workflow `codeql-triage.yml` (que tem
`permissions: security-events: read`) ou com um PAT que carregue o escopo.

Exit codes:
    0  sucesso (com ou sem mudança)
    1  erro de uso
    2  ambíguo: a mesma chave (rule_id, path, line) casou com mais de um alerta
       aberto. Fail-closed — reaudite à mão, não adivinhe.
    3  só com `--check-main`: há entrada cujo alerta a análise de `main` não
       detecta mais (instância `fixed`, sem instância, ou alerta inexistente).
       Perde para o exit 2, que pede reauditoria mais urgente.
    5  falha de precondição (ledger ausente/malformado, token ausente, API).

O exit 2 tambem cobre **drift**: entrada cujo `alert_number` continua aberto mas
cujo alerta vivo esta em outra `(rule_id, path, line)`. Ate 2026-08-30 esse caso
era pulado em silencio — so o numero era comparado — e foi como o #113 passou
tres meses descrevendo o statement errado. Nao ha o que reapontar ali: o numero
ja esta certo, o que apodreceu foi a linha, e corrigir isso exige reler o codigo.

Entrada sem alerta aberto correspondente **não** é erro: é o estado saudável de
uma entrada já dispensada e sem duplicata. Ela é listada no resumo para o
re-audit de 90 dias (§3.4 do ledger) e o exit continua 0.

Entrada cujo alerta **não é mais detectado** é outra coisa, e só a API de
instâncias enxerga (#1993, item 1). Um alerta dispensado nunca passa a `fixed`
(§5.2 do ledger): o `state` dele fica `dismissed` para sempre, e nem este
script (que só lista `open`) nem o `codeql-reapply-dismissals.sh` (que só trata
`state=fixed` como velho) percebiam. Em 2026-11-25 eram 14 de 43 entradas, a
mais antiga desde 2026-08-29, com tudo verde. A instância do alerta em
`refs/heads/main` (`GET /code-scanning/alerts/{n}/instances?ref=...`) carrega
o estado da análise: `fixed` quando o achado sumiu, `open` quando continua,
mesmo com o alerta dispensado. `--check-main` consulta essa instância para
cada entrada, lista as que não são mais detectadas e sai com exit 3. Uma
entrada assim não tem o que justificar: o caminho é aposentá-la do ledger
num PR, depois de conferir o código.
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


def _get_json(url: str, token: str, what: str) -> object:
    """GET autenticado na API. 404 devolve `None`; o resto falha com exit 5."""
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
            return json.load(resp)
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", "replace")[:400]
        if exc.code == 404:
            return None
        if exc.code == 403:
            fail(
                f"403 ao {what}. O token nao tem o escopo "
                f"`security_events`. Resposta: {body}"
            )
        fail(f"HTTP {exc.code} ao {what}: {body}")
    except urllib.error.URLError as exc:
        fail(f"falha de rede ao {what}: {exc.reason}")
    return None  # inalcancavel: `fail` levanta SystemExit


def fetch_open_alerts(repo: str, token: str) -> list[dict]:
    """Lista todos os alertas abertos. Mesma paginação do codeql-alert-report.py."""
    alerts: list[dict] = []
    for page in range(1, MAX_PAGES + 1):
        query = {"per_page": str(PER_PAGE), "page": str(page), "state": "open"}
        url = f"{API_ROOT}/repos/{repo}/code-scanning/alerts?{urllib.parse.urlencode(query)}"
        batch = _get_json(url, token, "listar alertas")
        if not isinstance(batch, list):
            fail(f"resposta inesperada da API: {str(batch)[:200]}")
        alerts.extend(batch)
        if len(batch) < PER_PAGE:
            break
    return alerts


def fetch_ref_instances(repo: str, token: str, number: int, ref: str) -> list[dict] | None:
    """Instâncias do alerta `number` em `ref`. `None` se o alerta não existe (404)."""
    instances: list[dict] = []
    for page in range(1, MAX_PAGES + 1):
        query = {"per_page": str(PER_PAGE), "page": str(page), "ref": ref}
        url = (
            f"{API_ROOT}/repos/{repo}/code-scanning/alerts/{number}/instances?"
            f"{urllib.parse.urlencode(query)}"
        )
        batch = _get_json(url, token, f"listar instancias do alerta #{number}")
        if batch is None:
            return None
        if not isinstance(batch, list):
            fail(f"resposta inesperada da API para #{number}: {str(batch)[:200]}")
        instances.extend(batch)
        if len(batch) < PER_PAGE:
            break
    return instances


def alert_span(alert: dict) -> tuple[str, str, int, int]:
    """`(rule_id, path, start_line, end_line)` do alerta vivo.

    O CodeQL reporta o **span do statement**: num `println!` multi-linha o
    `start_line` e a abertura e o `end_line` o fechamento. O ledger, por sua
    vez, ancora a linha do **sink** — derivada do `sink_snippet` pelo
    `check-ledger-anchors.py` —, que pode ser qualquer linha dentro desse
    span. Casar `start_line` exato (o que este script fazia ate 2026-09-25)
    deixava a duplicata do #173 (`whatsapp.rs`, span 1356-1358, sink em 1358)
    aberta como "sem entrada", enquanto o `codeql-reapply-dismissals.sh` ja
    aceitava a linha em qualquer ponto do span. Uma regra so, nos dois.
    """
    loc = alert.get("most_recent_instance", {}).get("location", {})
    start = int(loc.get("start_line", 0) or 0)
    end = int(loc.get("end_line", start) or start)
    if end < start:
        end = start
    return (alert.get("rule", {}).get("id", ""), loc.get("path", ""), start, end)


def alert_key(alert: dict) -> tuple[str, str, int]:
    """`(rule_id, path, start_line)` — a forma que o relatorio imprime."""
    rule, path, start, _end = alert_span(alert)
    return (rule, path, start)


def entry_key(entry: dict) -> tuple[str, str, int]:
    return (entry["rule_id"], entry["path"], int(entry["line"]))


def span_cobre(span: tuple[str, str, int, int], entry: dict) -> bool:
    """O alerta vivo `span` e o achado que a entrada do ledger descreve?"""
    rule, path, start, end = span
    return rule == entry["rule_id"] and path == entry["path"] and start <= int(entry["line"]) <= end


def plan_rekey(
    entries: list[dict], open_alerts: list[dict]
) -> tuple[dict[int, int], list[dict], list[tuple], list[tuple]]:
    """Decide o remapeamento. Retorna (mapping, sem_correspondencia, ambiguos, drift).

    Puro — sem I/O — para ser testável.
    """
    spans: list[tuple[int, tuple[str, str, int, int]]] = [
        (int(a["number"]), alert_span(a)) for a in open_alerts
    ]
    open_by_number: dict[int, tuple[str, str, int, int]] = dict(spans)

    mapping: dict[int, int] = {}
    unmatched: list[dict] = []
    ambiguous: list[tuple] = []
    drifted: list[tuple] = []

    for entry in entries:
        current = int(entry["alert_number"])
        # Candidatos: alertas abertos da mesma regra e arquivo cujo span
        # contem a linha do sink que o ledger ancora.
        candidates = [numero for numero, span in spans if span_cobre(span, entry)]

        if current in open_by_number:
            live = open_by_number[current]
            if span_cobre(live, entry):
                # Ja aponta para um alerta aberto que cobre este sink: nada a fazer.
                continue
            # A forma de tres campos (rule, path, start_line) e o que o
            # relatorio e os testes de drift leem.
            live = (live[0], live[1], live[2])
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


# Motivos de "nao detectado" — a string e o que o relatorio imprime.
UNDETECTED_FIXED = "instancia `fixed` no ref: a analise nao acusa mais o achado"
UNDETECTED_NO_INSTANCE = "nenhuma instancia no ref"
UNDETECTED_MISSING = "alerta inexistente (404)"


def classify_ref_instances(
    entries: list[dict], instances_by_number: dict[int, list[dict] | None]
) -> list[tuple[dict, str]]:
    """Entradas cujo alerta a analise do ref nao detecta mais, com o motivo.

    Puro — sem I/O. `instances_by_number[n]` e a lista de instancias do alerta
    `n` no ref (`None` = alerta inexistente). Um alerta pode ter mais de uma
    instancia no mesmo ref (categoria/analysis_key diferentes); basta UMA que
    nao esteja `fixed` para ele seguir detectado — `open` e `dismissed` contam
    como detectado, porque o estado da instancia de um alerta dispensado ainda
    acompanha a analise. Entrada sem consulta no mapa e ignorada, nunca
    presumida morta.
    """
    undetected: list[tuple[dict, str]] = []
    for entry in entries:
        number = int(entry["alert_number"])
        if number not in instances_by_number:
            continue
        instances = instances_by_number[number]
        if instances is None:
            undetected.append((entry, UNDETECTED_MISSING))
        elif not instances:
            undetected.append((entry, UNDETECTED_NO_INSTANCE))
        elif all(i.get("state") == "fixed" for i in instances):
            undetected.append((entry, UNDETECTED_FIXED))
    return undetected


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
    ap.add_argument(
        "--check-main",
        action="store_true",
        help="consulta a instancia de cada entrada no ref da --main-ref e sai com "
        "exit 3 se alguma nao for mais detectada (#1993)",
    )
    ap.add_argument("--main-ref", default="refs/heads/main")
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

    # Antes do drift/ambiguo: aqueles saem com exit 2 e esconderiam esta lista.
    undetected: list[tuple[dict, str]] = []
    if args.check_main:
        instances = {
            int(e["alert_number"]): fetch_ref_instances(
                args.repo, token, int(e["alert_number"]), args.main_ref
            )
            for e in entries
        }
        undetected = classify_ref_instances(entries, instances)
        if undetected:
            print(f"\nNAO DETECTADAS em {args.main_ref} ({len(undetected)}) — a entrada "
                  "nao suprime mais nada; confira o codigo e aposente-a do ledger em PR:")
            for e, motivo in undetected:
                print(f"  #{e['alert_number']}  {e['rule_id']} @ {e['path']}:{e['line']}"
                      f"  — {motivo}")
        else:
            print(f"\ntoda entrada segue detectada em {args.main_ref} "
                  f"({len(entries)} conferidas).")

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

    # Orfao = alerta aberto que nenhuma entrada descreve (mesma regra e
    # arquivo, sink dentro do span) — a mesma regra de casamento de cima.
    orphan_alerts = [
        a for a in open_alerts if not any(span_cobre(alert_span(a), e) for e in entries)
    ]

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

    if undetected:
        print(f"\nexit 3: {len(undetected)} entrada(s) nao detectada(s) em {args.main_ref}.")
        return 3
    return 0


if __name__ == "__main__":
    sys.exit(main())
