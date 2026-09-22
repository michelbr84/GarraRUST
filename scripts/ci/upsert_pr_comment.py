#!/usr/bin/env python3
"""Cria ou EDITA o comentario de bot de um PR, achado por marcador (#1228, E).

O job de cobertura do `ci.yml` e o `quality-ratchet.yml` postavam um
comentario NOVO a cada push (`gh pr comment`). Num PR com dez pushes eram
vinte comentarios quase iguais, e o que importava — a conversa de review —
sumia no meio deles. Este script mantem um comentario so por marcador:

    python3 scripts/ci/upsert_pr_comment.py \\
        --repo dono/repo --pr 123 \\
        --marker '<!-- coverage-comment -->' --body-file pr-comment.md

Regras:

- O comentario editado tem de conter o marcador **e** ter sido escrito pelo
  bot do Actions (`github-actions[bot]`). So o marcador nao basta: qualquer
  pessoa pode colar `<!-- coverage-comment -->` num comentario proprio, e o
  bot passaria a sobrescrever o texto dela.
- Havendo mais de um (o historico de antes deste script), edita o mais
  recente e deixa os antigos como estao — apagar comentario nao e papel de CI.
- O corpo sempre carrega o marcador: se o arquivo nao o tiver, ele entra na
  primeira linha. Sem isso o proximo push nao acharia o comentario e voltaria
  a criar um novo.

Fala com a API so pelo `gh api` (token no `GH_TOKEN` do ambiente, nunca em
argumento). Exit 0 = criado ou editado; 1 = falha da API; 2 = erro de uso.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path
from typing import Callable, Iterable, Optional

AUTOR_DO_BOT = "github-actions[bot]"

# Um "runner" recebe os argumentos do `gh` e o stdin, e devolve o stdout.
# Injetavel para o teste nao precisar de rede.
Runner = Callable[[list[str], Optional[str]], str]


def gh_runner(args: list[str], stdin: Optional[str]) -> str:
    resultado = subprocess.run(
        ["gh", *args],
        input=stdin,
        capture_output=True,
        text=True,
        check=False,
    )
    if resultado.returncode != 0:
        # O stderr do `gh` nao carrega o token; o corpo do comentario tambem
        # nao passa por aqui (vai por stdin).
        raise RuntimeError(f"gh {args[0]} falhou ({resultado.returncode}): {resultado.stderr.strip()}")
    return resultado.stdout


def parse_paginas(texto: str) -> list[dict]:
    """O `gh api --paginate` concatena os arrays de cada pagina (`[...][...]`).
    Decodifica valor a valor e achata."""
    decoder = json.JSONDecoder()
    itens: list[dict] = []
    i = 0
    while i < len(texto):
        while i < len(texto) and texto[i].isspace():
            i += 1
        if i >= len(texto):
            break
        valor, i = decoder.raw_decode(texto, i)
        if isinstance(valor, list):
            itens.extend(v for v in valor if isinstance(v, dict))
        elif isinstance(valor, dict):
            itens.append(valor)
    return itens


def escolher(comentarios: Iterable[dict], marker: str, autor: str = AUTOR_DO_BOT) -> Optional[int]:
    """O id do comentario a editar, ou `None` para criar um novo."""
    candidatos = [
        c
        for c in comentarios
        if isinstance(c.get("id"), int)
        and marker in (c.get("body") or "")
        and ((c.get("user") or {}).get("login") == autor)
    ]
    if not candidatos:
        return None
    return max(candidatos, key=lambda c: c["id"])["id"]


def garantir_marcador(corpo: str, marker: str) -> str:
    if marker in corpo:
        return corpo
    return f"{marker}\n{corpo}"


def upsert(repo: str, pr: int, marker: str, corpo: str, runner: Runner = gh_runner) -> str:
    """Cria ou edita. Devolve `"created"` ou `"updated:<id>"`."""
    corpo = garantir_marcador(corpo, marker)
    listagem = runner(["api", "--paginate", f"repos/{repo}/issues/{pr}/comments"], None)
    alvo = escolher(parse_paginas(listagem), marker)
    payload = json.dumps({"body": corpo})
    if alvo is None:
        runner(["api", "-X", "POST", f"repos/{repo}/issues/{pr}/comments", "--input", "-"], payload)
        return "created"
    runner(["api", "-X", "PATCH", f"repos/{repo}/issues/comments/{alvo}", "--input", "-"], payload)
    return f"updated:{alvo}"


def main(argv: list[str] | None = None, runner: Runner = gh_runner) -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--repo", required=True)
    p.add_argument("--pr", required=True, type=int)
    p.add_argument("--marker", required=True)
    p.add_argument("--body-file", required=True, type=Path)
    args = p.parse_args(argv)

    marker = args.marker.strip()
    if not (marker.startswith("<!--") and marker.endswith("-->")):
        print("erro: --marker tem de ser um comentario HTML (<!-- ... -->)", file=sys.stderr)
        return 2
    if "/" not in args.repo or args.pr <= 0:
        print("erro: --repo dono/nome e --pr positivo", file=sys.stderr)
        return 2
    try:
        corpo = args.body_file.read_text(encoding="utf-8")
    except OSError as e:
        print(f"erro: nao li {args.body_file}: {e}", file=sys.stderr)
        return 2

    try:
        resultado = upsert(args.repo, args.pr, marker, corpo, runner)
    except (RuntimeError, json.JSONDecodeError) as e:
        print(f"erro: {e}", file=sys.stderr)
        return 1
    print(f"comentario {resultado}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
