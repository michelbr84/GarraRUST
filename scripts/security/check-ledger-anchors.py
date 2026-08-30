#!/usr/bin/env python3
"""Confere que cada entrada do ledger ainda aponta para o statement que descreve.

Existe porque o ledger em `docs/security/codeql-suppressions.json` e chaveado por
`(rule_id, path, line)` — e `line` **apodrece em silencio**. Qualquer commit que
insira ou remova linhas acima de um sink desloca a ancora, e a entrada passa a
descrever outro statement. O `codeql-reapply-dismissals.sh` detecta isso
fail-closed (exit 2), mas so quando alguem dispara o workflow a mao: nem ele nem
o `codeql-rekey-ledger.py` rodam na trilha de PR. Este script fecha essa lacuna,
e por isso e **offline**: sem rede, sem `GITHUB_TOKEN`, sem escopo
`security_events`. So le dois arquivos versionados e o codigo-fonte.

Dois casos reais que motivaram o script:

* O alerta #113 passou tres meses apontando para o statement errado
  (`wizard/mod.rs:640`, o `eprintln!` do "vault open failed") enquanto o alerta
  vivo estava no `println!` de `:673`. Ninguem percebeu porque nada comparava a
  ancora com o codigo. Corrigido a mao em 2026-08-30.
* O PR #882 (Copilot Autofix do alerta 162) inseriu 18 linhas em
  `skins_handler.rs` e deslocou as cinco ancoras 84/104/111/134/141 que quatro
  entradas do ledger registram. O CI so reclamou do erro de compilacao que o
  mesmo commit trazia de brinde; a dessincronizacao do ledger teria passado
  despercebida se ele compilasse.

O que e conferido, por entrada:

1. **Ancora vs codigo** — a linha `line` de `path`, com `.strip()`, tem que ser
   identica ao campo `sink_snippet`.
2. **Ancora vs `.md`** — a coluna `File:line` da linha correspondente no ledger
   markdown tem que casar com `path:line` do `.json`, e a coluna `Rule` com
   `rule_id`. O `--check-md` do `codeql-reapply-dismissals.sh` **nao** cobre
   isso: ele so compara o conjunto de numeros de alerta entre os dois arquivos.

REGRA ANTI-FRAUDE, e a razao de o snippet ser versionado em vez de derivado:

    NUNCA edite `sink_snippet` para fazer este check passar. O snippet e o
    registro do que a justificativa revisada em PR descreve. Se a linha se
    moveu, ha exatamente dois caminhos legitimos, os dois com diff revisado por
    humano: reapontar a entrada (linha nova + snippet novo, conferindo que o
    sink e mesmo o mesmo) ou reauditar a supressao. Reescrever o snippet para
    casar com o que quer que esteja na linha hoje e a mesma fraude que editar
    `.quality/baseline.json` a mao — ver CLAUDE.md, secao AI Quality Ratchet.

Uso:
    python3 scripts/security/check-ledger-anchors.py
    python3 scripts/security/check-ledger-anchors.py --ledger caminho/ledger.json

Exit codes (a convencao 2/3/4/5 ja esta ocupada pelos scripts irmaos):
    0  todas as entradas casam
    5  falha de precondicao (ledger ausente/malformado, arquivo-fonte sumido,
       linha fora de faixa, entrada sem `sink_snippet`)
    6  drift: alguma entrada nao aponta mais para o statement que descreve
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

DEFAULT_JSON = "docs/security/codeql-suppressions.json"
DEFAULT_MD = "docs/security/codeql-suppressions.md"

EXIT_OK = 0
EXIT_PRECONDITION = 5
EXIT_DRIFT = 6

# Linha canonica do ledger markdown. Casa apenas as tres primeiras celulas, para
# nao tropecar num `|` dentro da justificativa (que e prosa livre com codigo):
#   | <a id="alert-162"></a>[#162](.../162) | `rule` | `path:line` | ...
MD_ROW = re.compile(
    r'^\|\s*<a id="alert-(?P<number>\d+)"></a>[^|]*\|(?P<rule>[^|]*)\|(?P<fileline>[^|]*)\|'
)


def fail(msg: str, code: int = EXIT_PRECONDITION) -> None:
    print(f"error: {msg}", file=sys.stderr)
    raise SystemExit(code)


def unbacktick(cell: str) -> str:
    """Normaliza uma celula de tabela markdown: tira espacos e crases."""
    return cell.strip().strip("`").strip()


def parse_md(md_path: Path) -> dict[int, tuple[str, str]]:
    """Mapeia numero do alerta -> (rule_id, 'path:line') a partir do ledger .md."""
    rows: dict[int, tuple[str, str]] = {}
    for raw in md_path.read_text(encoding="utf-8").splitlines():
        m = MD_ROW.match(raw)
        if not m:
            continue
        number = int(m.group("number"))
        if number in rows:
            fail(f"ledger .md tem duas linhas para o alerta #{number}")
        rows[number] = (unbacktick(m.group("rule")), unbacktick(m.group("fileline")))
    return rows


def main() -> int:
    ap = argparse.ArgumentParser(
        description="Confere as ancoras (path, line, sink_snippet) do ledger CodeQL."
    )
    ap.add_argument("--ledger", default=DEFAULT_JSON, help=f"default: {DEFAULT_JSON}")
    ap.add_argument("--md", default=DEFAULT_MD, help=f"default: {DEFAULT_MD}")
    ap.add_argument(
        "--root",
        default=".",
        help="raiz do repo, para resolver os `path` das entradas (default: .)",
    )
    args = ap.parse_args()

    ledger_path = Path(args.ledger)
    md_path = Path(args.md)
    root = Path(args.root)

    if not ledger_path.is_file():
        fail(f"ledger nao encontrado: {ledger_path}")
    if not md_path.is_file():
        fail(f"ledger markdown nao encontrado: {md_path}")

    try:
        doc = json.loads(ledger_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        fail(f"ledger malformado ({ledger_path}): {exc}")

    entries = doc.get("entries")
    if not isinstance(entries, list):
        fail(f"ledger sem lista `entries`: {ledger_path}")

    md_rows = parse_md(md_path)

    # Cache de arquivo -> linhas, para nao reler o mesmo fonte 11 vezes.
    sources: dict[str, list[str]] = {}
    drift: list[str] = []

    for entry in entries:
        number = entry.get("alert_number")
        rel = entry.get("path")
        line_no = entry.get("line")
        snippet = entry.get("sink_snippet")

        if number is None or rel is None or line_no is None:
            fail(f"entrada incompleta (alert_number/path/line): {entry!r}")
        if snippet is None:
            fail(
                f"#{number}: entrada sem `sink_snippet`. O ledger precisa estar em "
                f"schema_version >= 1.1.0 — rode a migracao antes deste check."
            )

        if rel not in sources:
            src = root / rel
            if not src.is_file():
                fail(f"#{number}: arquivo do ledger nao existe: {src}")
            sources[rel] = src.read_text(encoding="utf-8").splitlines()
        lines = sources[rel]

        if not 0 < line_no <= len(lines):
            fail(
                f"#{number}: linha {line_no} fora de faixa em {rel} "
                f"({len(lines)} linhas)"
            )

        actual = lines[line_no - 1].strip()
        if actual != snippet:
            drift.append(
                f"#{number} ({entry.get('rule_id', '?')}) {rel}:{line_no}\n"
                f"      ledger : {snippet}\n"
                f"      codigo : {actual}"
            )
            continue

        # A entrada casa com o codigo; agora confere o espelho no .md.
        md_row = md_rows.get(number)
        if md_row is None:
            drift.append(
                f"#{number} ({entry.get('rule_id', '?')}) {rel}:{line_no}\n"
                f"      sem linha correspondente em {md_path}"
            )
            continue
        md_rule, md_fileline = md_row
        expected_fileline = f"{rel}:{line_no}"
        if md_fileline != expected_fileline:
            drift.append(
                f"#{number} .md dessincronizado\n"
                f"      .json : {expected_fileline}\n"
                f"      .md   : {md_fileline}"
            )
        elif md_rule != entry.get("rule_id"):
            drift.append(
                f"#{number} .md com rule_id divergente\n"
                f"      .json : {entry.get('rule_id')}\n"
                f"      .md   : {md_rule}"
            )

    if drift:
        print(
            f"error: {len(drift)} entrada(s) do ledger nao apontam mais para o "
            f"statement que descrevem\n",
            file=sys.stderr,
        )
        for item in drift:
            print(f"  {item}\n", file=sys.stderr)
        print(
            "Reaponte a entrada (linha nova + sink_snippet novo, conferindo que o\n"
            "sink e o mesmo) ou reaudite a supressao. NUNCA edite sink_snippet so\n"
            "para o check passar — ver a docstring deste script.",
            file=sys.stderr,
        )
        return EXIT_DRIFT

    print(f"ok: {len(entries)} entradas do ledger conferem com o codigo e com o .md")
    return EXIT_OK


if __name__ == "__main__":
    raise SystemExit(main())
