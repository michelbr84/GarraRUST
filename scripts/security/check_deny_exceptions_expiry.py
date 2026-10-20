#!/usr/bin/env python3
"""Verifica o prazo das excecoes temporarias do `deny.toml`.

O cargo-deny 0.20.2 nao aceita chave de expiracao em `advisories.ignore`
(erro `unexpected-keys`). Por isso o prazo fica no campo `reason` de cada
entrada em forma de tabela, no formato `EXPIRA AAAA-MM-DD`, e este script
falha quando:

* uma entrada em forma de tabela nao traz `EXPIRA AAAA-MM-DD` no reason;
* a data ja passou (excecao vencida, precisa ser renovada ou removida).

Entradas em forma de string (as excecoes antigas de `unmaintained`) nao sao
verificadas aqui: elas nao tem prazo registrado.

Uso:
    python3 scripts/security/check_deny_exceptions_expiry.py [caminho/deny.toml] [--today AAAA-MM-DD]

Saida 0 = tudo dentro do prazo; 1 = excecao vencida ou sem prazo.
"""

from __future__ import annotations

import argparse
import datetime as dt
import re
import sys
import tomllib
from pathlib import Path

PADRAO_PRAZO = re.compile(r"EXPIRA (\d{4}-\d{2}-\d{2})")


def verificar(conteudo: str, hoje: dt.date) -> list[str]:
    """Devolve a lista de problemas encontrados (vazia = ok)."""
    dados = tomllib.loads(conteudo)
    problemas: list[str] = []
    entradas = dados.get("advisories", {}).get("ignore", [])
    for item in entradas:
        if not isinstance(item, dict):
            continue  # string = excecao permanente antiga, fora do escopo
        ident = item.get("id", "?")
        motivo = item.get("reason", "")
        m = PADRAO_PRAZO.search(motivo)
        if not m:
            problemas.append(f"{ident}: sem prazo 'EXPIRA AAAA-MM-DD' no reason")
            continue
        try:
            prazo = dt.date.fromisoformat(m.group(1))
        except ValueError:
            problemas.append(f"{ident}: data invalida {m.group(1)!r}")
            continue
        if hoje > prazo:
            problemas.append(f"{ident}: excecao VENCIDA em {prazo.isoformat()} (hoje {hoje.isoformat()})")
    return problemas


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("deny", nargs="?", default="deny.toml")
    ap.add_argument("--today", help="data de referencia AAAA-MM-DD (padrao: hoje)")
    args = ap.parse_args(argv)
    hoje = dt.date.fromisoformat(args.today) if args.today else dt.date.today()
    problemas = verificar(Path(args.deny).read_text(encoding="utf-8"), hoje)
    if problemas:
        for p in problemas:
            print(f"ERRO: {p}")
        return 1
    print(f"ok: excecoes temporarias dentro do prazo em {hoje.isoformat()}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
