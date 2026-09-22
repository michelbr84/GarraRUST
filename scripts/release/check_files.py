#!/usr/bin/env python3
"""Todo asset que o `release.yml` coloca em `release/` precisa estar no upload.

Na v0.4.4 o passo de staging copiou `garraia-linux-aarch64.AppImage` para
`release/` e o `SHA256SUMS` o listou, mas a lista `files:` do
`softprops/action-gh-release` nunca teve a linha dele. O glob `*.sha256`
subiu so o sidecar, e a release saiu com um `.sha256` apontando para um
arquivo que nao existia (o asset foi anexado a mao depois, com o mesmo hash).
`fail_on_unmatched_files` e `false` de proposito — builds best-effort faltam
sem derrubar a release —, entao o esquecimento inverso (listado e nao
produzido) e aceito, mas este (produzido e nao listado) nao aparece em lugar
nenhum. Este script pega esse caso no PR, antes do tag.

O que ele le do workflow, sem executar nada:

- nomes que o staging copia para `release/`: os itens de cada
  `for pkg in A B C; do` e todo `cp <caminho>/<nome> release/` com nome
  literal (sem `$`);
- os padroes da lista `files:` do passo `softprops/action-gh-release`.

Cada nome copiado tem de casar com algum padrao (`fnmatch`, o mesmo tipo de
glob que o softprops expande). Sem dependencia fora da stdlib: roda em
qualquer runner sem `pip install`.

Uso: python3 scripts/release/check_files.py [.github/workflows/release.yml]
Saida 0 quando tudo casa; 1 listando o que ficou de fora.
"""

from __future__ import annotations

import fnmatch
import re
import sys
from pathlib import Path

DEFAULT_WORKFLOW = Path(__file__).resolve().parents[2] / ".github" / "workflows" / "release.yml"

_FOR_PKG = re.compile(r"for\s+pkg\s+in\s+([^;]+);\s*do")
_CP_LITERAL = re.compile(r"\bcp\s+(?:\S*/)?([A-Za-z0-9._-]+)\s+release/?(?:\s|$)")


def nomes_copiados(texto: str) -> set[str]:
    """Nomes de asset que o staging poe em `release/`."""
    nomes: set[str] = set()
    for linha in texto.splitlines():
        m = _FOR_PKG.search(linha)
        if m:
            nomes.update(n for n in m.group(1).split() if "$" not in n)
            continue
        m = _CP_LITERAL.search(linha)
        if m and "$" not in m.group(1):
            nomes.add(m.group(1))
    return nomes


def padroes_do_upload(texto: str) -> list[str]:
    """Padroes da lista `files:` do passo `softprops/action-gh-release`."""
    linhas = texto.splitlines()
    try:
        inicio = next(
            i for i, l in enumerate(linhas) if re.match(r"\s*(-\s+)?uses:\s*softprops/action-gh-release", l)
        )
    except StopIteration:
        return []
    padroes: list[str] = []
    dentro = False
    indent_bloco = None
    for linha in linhas[inicio + 1 :]:
        if not dentro:
            if re.match(r"\s*files:\s*\|\s*$", linha):
                dentro = True
            elif re.match(r"\s*-\s+(name|uses):", linha):
                break
            continue
        if not linha.strip():
            continue
        indent = len(linha) - len(linha.lstrip())
        if indent_bloco is None:
            indent_bloco = indent
        if indent < indent_bloco:
            break
        padroes.append(linha.strip())
    return padroes


def fora_do_upload(texto: str) -> list[str]:
    """Nomes copiados para `release/` que nenhum padrao do upload cobre."""
    padroes = padroes_do_upload(texto)
    return sorted(
        nome
        for nome in nomes_copiados(texto)
        if not any(fnmatch.fnmatch(f"release/{nome}", p) for p in padroes)
    )


def main(argv: list[str]) -> int:
    caminho = Path(argv[1]) if len(argv) > 1 else DEFAULT_WORKFLOW
    texto = caminho.read_text(encoding="utf-8")
    if not padroes_do_upload(texto):
        print(f"{caminho}: passo softprops/action-gh-release com `files:` nao encontrado")
        return 1
    faltando = fora_do_upload(texto)
    if faltando:
        print(f"{caminho}: copiados para release/ mas fora da lista `files:` do upload:")
        for nome in faltando:
            print(f"  release/{nome}")
        return 1
    print(f"{caminho}: todo asset copiado para release/ esta na lista de upload")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
