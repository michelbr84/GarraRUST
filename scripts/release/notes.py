#!/usr/bin/env python3
"""Extrai a prosa de abertura de uma versao do CHANGELOG para o corpo da release.

A secao inteira nao serve: a da v0.3.9 tem 1298 linhas e 90 KB, perto do teto
de 125 KB do corpo de release do GitHub e ilegivel como pagina. A prosa de
abertura — o que vem entre o `## [X.Y.Z]` e a primeira subsecao `###` — e o
resumo escrito para humano, e e ela que vai para a release. O detalhe fica no
CHANGELOG, linkado.

Nunca falha: sem secao, sem prosa ou sem arquivo, emite um corpo minimo. Uma
release nao pode ser bloqueada por causa das proprias notas.
"""
import re
import sys

def main() -> int:
    versao = sys.argv[1] if len(sys.argv) > 1 else ""
    caminho = sys.argv[2] if len(sys.argv) > 2 else "CHANGELOG.md"
    limpa = versao.lstrip("v")
    repo = "https://github.com/michelbr84/GarraRUST"
    rodape = (
        f"\n---\n\n**Changelog completo desta versao:** "
        f"[CHANGELOG.md]({repo}/blob/main/CHANGELOG.md)\n"
    )

    try:
        with open(caminho, encoding="utf-8") as f:
            s = f.read()
    except OSError:
        print(f"## {versao}\n{rodape}")
        return 0

    m = re.search(rf"^## \[{re.escape(limpa)}\][^\n]*\n", s, re.M)
    if not m:
        print(f"## {versao}\n{rodape}")
        return 0

    resto = s[m.end():]
    fim = re.search(r"^## \[", resto, re.M)
    secao = resto[: fim.start()] if fim else resto
    corte = re.search(r"^### ", secao, re.M)
    prosa = (secao[: corte.start()] if corte else secao).strip()

    print((prosa if prosa else f"## {versao}") + "\n" + rodape)
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
