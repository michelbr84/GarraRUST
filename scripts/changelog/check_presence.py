#!/usr/bin/env python3
"""Checa se um PR deixa um fragmento em `changelog.d/` (#1228, slice D2).

O job "Changelog fragments" do `ci.yml` valida o FORMATO dos fragmentos que
existem. Ele nao ve o PR que nao escreveu fragmento nenhum — e as notas de
release saem do CHANGELOG.md (regra 15 do CLAUDE.md), entao esse silencio so
aparecia no dia da release, quando ninguem lembra mais o que o PR fez.

Este script decide, a partir de dados que o workflow ja tem, se o PR passa:

    git -c core.quotePath=false diff --name-status --no-renames -z \\
        HEAD^1 HEAD > changed.txt
    python3 scripts/changelog/check_presence.py \\
        --event pull_request --labels-json '["bug"]' \\
        --author alguem --head-ref fix/x --same-repo true \\
        --name-status-file changed.txt

A lista vem com `-z` (registros separados por NUL): sem isso o git poe entre
aspas, com escape octal, qualquer caminho com acento ou aspas, e o fragmento
deixaria de ser reconhecido. A saida por linhas continua aceita.

Passa quando o PR ADICIONA ou MODIFICA um `changelog.d/<secao>/<nome>.md` de
secao valida (as mesmas do `assemble.py`, importadas de la — uma lista so).
Apagar um fragmento nao conta: e o que o PR de release faz, e ele tem isencao
propria.

Isencoes, nesta ordem:

- evento que nao e `pull_request` (`merge_group`, `push`): o fragmento ja foi
  cobrado no PR;
- label `no-changelog` — para mudanca que nao interessa a quem le as notas
  (CI, doc interna, refactor sem efeito visivel). E uma decisao humana e fica
  registrada no PR;
- autor `dependabot[bot]`;
- head `release/vX.Y.Z` vindo DESTE repositorio (`--same-repo true`): o PR
  de release APAGA os fragmentos ao junta-los. O nome da branch e escolhido
  pelo autor do PR, entao sozinho nao isenta: um fork chamando a branch de
  `release/qualquer` continua cobrado. `--same-repo` vem do workflow
  (`head.repo.full_name == github.repository`), que o autor nao controla.

Sem rede, sem segredo, sem ler titulo nem corpo do PR. Exit 0 = passa,
1 = falta fragmento, 2 = erro de uso.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import re
import sys
from pathlib import Path

LABEL_DE_ESCAPE = "no-changelog"
AUTORES_ISENTOS = frozenset({"dependabot[bot]"})
RELEASE_HEAD = re.compile(r"^release/v\d+\.\d+\.\d+$")

# Status do `git diff --name-status` que contam como "o PR escreveu isto".
STATUS_QUE_CONTAM = frozenset({"A", "M"})


def _sections() -> list[str]:
    """As secoes validas, lidas do `assemble.py` para as duas listas nunca
    divergirem."""
    caminho = Path(__file__).resolve().parent / "assemble.py"
    spec = importlib.util.spec_from_file_location("_assemble_para_presence", caminho)
    if spec is None or spec.loader is None:
        raise RuntimeError("assemble.py nao carregou")
    modulo = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(modulo)
    return list(modulo.SECTIONS)


SECTIONS = _sections()


def e_fragmento(caminho: str) -> bool:
    """`changelog.d/<secao valida>/<nome>.md`, exatamente um nivel, sem README."""
    partes = caminho.strip().split("/")
    if len(partes) != 3 or partes[0] != "changelog.d":
        return False
    _, secao, nome = partes
    return (
        secao in SECTIONS
        and nome.endswith(".md")
        and nome.lower() != "readme.md"
        and not nome.startswith(".")
    )


def _parse_z(texto: str) -> list[tuple[str, str]]:
    """Saida de `git diff --name-status -z`: `S\0caminho\0`, e `R`/`C` com
    dois caminhos (`R100\0origem\0destino\0`). O caminho sai cru, sem aspas
    nem escape — por isso o workflow usa `-z`."""
    campos = texto.split("\0")
    pares: list[tuple[str, str]] = []
    i = 0
    while i < len(campos):
        bruto = campos[i]
        i += 1
        if not bruto:
            continue
        status = bruto[:1]
        n = 2 if status in {"R", "C"} else 1
        caminhos = campos[i : i + n]
        i += n
        if len(caminhos) < n:
            break
        if status in {"R", "C"}:
            status = "A"
        pares.append((status, caminhos[-1]))
    return pares


def parse_name_status(texto: str) -> list[tuple[str, str]]:
    """Saida do `git diff --name-status --no-renames`, com ou sem `-z`.

    Com NUL no texto, parse de `-z` (o que o workflow produz). Sem, linhas
    `S<TAB>caminho`. Com `--no-renames` cada linha tem dois campos; `R`/`C` so
    apareceriam sem a flag, e ai o caminho que conta e o de destino (ultimo
    campo).
    """
    if "\0" in texto:
        return _parse_z(texto)
    pares: list[tuple[str, str]] = []
    for linha in texto.splitlines():
        if not linha.strip():
            continue
        campos = linha.split("\t")
        if len(campos) < 2:
            continue
        status = campos[0][:1]
        if status in {"R", "C"}:
            status = "A"
        pares.append((status, campos[-1]))
    return pares


def decide(
    changed: list[tuple[str, str]],
    labels: list[str],
    author: str,
    head_ref: str,
    event: str,
    same_repo: bool = False,
) -> tuple[bool, str]:
    """A decisao, pura. Devolve (passa, motivo)."""
    if event != "pull_request":
        return True, f"evento `{event}` isento: o fragmento e cobrado no PR"
    if LABEL_DE_ESCAPE in labels:
        return True, f"label `{LABEL_DE_ESCAPE}` presente"
    if author in AUTORES_ISENTOS:
        return True, f"autor `{author}` isento"
    if same_repo and RELEASE_HEAD.match(head_ref):
        return True, "PR de release (apaga os fragmentos ao junta-los)"
    escritos = [c for s, c in changed if s in STATUS_QUE_CONTAM and e_fragmento(c)]
    if escritos:
        return True, "fragmento(s): " + ", ".join(sorted(escritos))
    return False, (
        "este PR nao adiciona nenhum fragmento em changelog.d/<secao>/<nome>.md "
        f"(secoes: {', '.join(SECTIONS)}). Escreva um (ver changelog.d/README.md) "
        f"ou, se a mudanca nao interessa a quem le as notas de release, aplique a "
        f"label `{LABEL_DE_ESCAPE}`."
    )


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--event", required=True)
    p.add_argument("--labels-json", default="[]")
    p.add_argument("--author", default="")
    p.add_argument("--head-ref", default="")
    p.add_argument(
        "--same-repo",
        default="false",
        help="`true` quando a head do PR e deste repositorio (nao de fork)",
    )
    p.add_argument("--name-status-file", type=Path)
    args = p.parse_args(argv)
    same_repo = args.same_repo.strip().lower() == "true"

    try:
        labels = json.loads(args.labels_json or "[]")
    except json.JSONDecodeError:
        print("erro: --labels-json nao e JSON", file=sys.stderr)
        return 2
    # Fora de `pull_request` (merge_group) o `toJSON` do workflow devolve
    # `null`: nao ha PR, entao nao ha label.
    if labels is None:
        labels = []
    if not isinstance(labels, list) or not all(isinstance(x, str) for x in labels):
        print("erro: --labels-json tem de ser uma lista de strings", file=sys.stderr)
        return 2

    changed: list[tuple[str, str]] = []
    if args.name_status_file is not None:
        changed = parse_name_status(args.name_status_file.read_text(encoding="utf-8"))

    passa, motivo = decide(
        changed, labels, args.author, args.head_ref, args.event, same_repo
    )
    if passa:
        print(f"OK: {motivo}")
        return 0
    print(f"::error title=Changelog presence::{motivo}")
    print(f"FALHA: {motivo}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
