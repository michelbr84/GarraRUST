#!/usr/bin/env python3
"""Valida o YAML do frontmatter de `.claude/agents/*.md` (#1139).

Guarda de regressao do #1107: a `description` sem aspas de `test-engineer.md`
continha ": " (dois-pontos seguido de espaco), que o YAML interpreta como um
novo par chave/valor dentro do mesmo mapping -- `yaml.safe_load` falhava com
"mapping values are not allowed here". O #1138 corrigiu aquele arquivo a mao;
nada na trilha de CI garantia que o mesmo erro nao voltasse a acontecer, no
mesmo arquivo ou em outro. Este script fecha esse gap.

So o frontmatter (o trecho entre os dois primeiros `---`) e parseado como
YAML -- o corpo do agente e prosa em markdown, nao YAML, e nunca deve passar
por `yaml.safe_load`.

Uso:
    python3 scripts/ci/validate_agent_frontmatter.py

Exit 0 se todo arquivo tem frontmatter valido com os campos obrigatorios;
exit 1 e uma mensagem por arquivo com problema caso contrario.
"""

from __future__ import annotations

import glob
import sys

import yaml

REQUIRED_FIELDS = ("name", "description", "model")


def extract_frontmatter(text: str) -> str | None:
    """Retorna o bloco entre o primeiro par de `---`, ou None se nao houver."""
    if not text.startswith("---"):
        return None
    parts = text.split("---", 2)
    if len(parts) < 3:
        return None
    return parts[1]


def validate_file(path: str) -> list[str]:
    """Retorna a lista de erros encontrados em `path` (vazia se estiver OK)."""
    errors: list[str] = []
    text = open(path, encoding="utf-8").read()

    frontmatter = extract_frontmatter(text)
    if frontmatter is None:
        errors.append("nao comeca com um bloco `---frontmatter---`")
        return errors

    try:
        data = yaml.safe_load(frontmatter)
    except yaml.YAMLError as exc:
        errors.append(f"YAML invalido no frontmatter: {exc}")
        return errors

    if not isinstance(data, dict):
        errors.append(
            f"frontmatter deve ser um mapping YAML, recebeu {type(data).__name__}"
        )
        return errors

    for field in REQUIRED_FIELDS:
        value = data.get(field)
        if not isinstance(value, str) or not value.strip():
            errors.append(f"campo obrigatorio ausente ou vazio: '{field}'")

    return errors


def main() -> int:
    files = sorted(glob.glob(".claude/agents/*.md"))
    if not files:
        print("AVISO: nenhum arquivo encontrado em .claude/agents/*.md", file=sys.stderr)
        return 0

    had_error = False
    for path in files:
        errors = validate_file(path)
        if errors:
            had_error = True
            print(f"FAIL: {path}", file=sys.stderr)
            for err in errors:
                print(f"  - {err}", file=sys.stderr)
        else:
            print(f"OK: {path}")

    if had_error:
        return 1

    print(f"OK: {len(files)} frontmatter(s) de agente validado(s).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
