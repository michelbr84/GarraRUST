#!/usr/bin/env python3
"""Junta os fragmentos de `changelog.d/` na secao [Unreleased] do CHANGELOG.md.

Cada PR cria um arquivo proprio em `changelog.d/<secao>/<nome>.md` em vez de
editar o CHANGELOG.md — arquivos diferentes nunca conflitam. Este script e o
passo de release que transforma os fragmentos numa secao de changelog de
verdade.

    python3 scripts/changelog/assemble.py           # imprime, nao toca em nada
    python3 scripts/changelog/assemble.py --check   # valida os fragmentos
    python3 scripts/changelog/assemble.py --write   # insere e apaga fragmentos

Deterministico: mesma entrada, mesma saida. Sem chamada de rede e sem IA.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

# Ordem do Keep a Changelog 1.1.0. Vale para secoes NOVAS; secoes que ja
# existem no CHANGELOG.md sao respeitadas onde estiverem.
SECTIONS = ["added", "changed", "deprecated", "removed", "fixed", "security"]

TITLES = {
    "added": "Added",
    "changed": "Changed",
    "deprecated": "Deprecated",
    "removed": "Removed",
    "fixed": "Fixed",
    "security": "Security",
}

UNRELEASED = "## [Unreleased]"


def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def collect(fragments_dir: Path) -> dict[str, list[tuple[Path, str]]]:
    """Le os fragmentos, agrupados por secao e ordenados por nome de arquivo."""
    found: dict[str, list[tuple[Path, str]]] = {}
    for section in SECTIONS:
        section_dir = fragments_dir / section
        if not section_dir.is_dir():
            continue
        entries = []
        for path in sorted(section_dir.glob("*.md")):
            text = path.read_text(encoding="utf-8").strip("\n")
            entries.append((path, text))
        if entries:
            found[section] = entries
    return found


def check(fragments_dir: Path) -> list[str]:
    """Devolve a lista de problemas. Vazia = tudo certo."""
    problems: list[str] = []

    known = set(SECTIONS)
    for child in sorted(fragments_dir.iterdir()):
        if child.is_dir() and child.name not in known:
            problems.append(
                f"{child}: pasta nao e uma secao valida "
                f"(use uma de: {', '.join(SECTIONS)})"
            )
        if child.is_file() and child.suffix == ".md" and child.name != "README.md":
            problems.append(
                f"{child}: fragmento solto na raiz — mova para "
                f"changelog.d/<secao>/{child.name}"
            )

    for section, entries in collect(fragments_dir).items():
        for path, text in entries:
            if not text.strip():
                problems.append(f"{path}: fragmento vazio")
            elif not text.lstrip().startswith("-"):
                problems.append(
                    f"{path}: fragmento deve comecar com um bullet markdown ('- ')"
                )
    return problems


def render(found: dict[str, list[tuple[Path, str]]]) -> str:
    """Renderiza os fragmentos como blocos markdown por secao."""
    blocks = []
    for section in SECTIONS:
        if section not in found:
            continue
        body = "\n".join(text for _, text in found[section])
        blocks.append(f"### {TITLES[section]}\n{body}")
    return "\n\n".join(blocks)


def _unreleased_bounds(lines: list[str]) -> tuple[int, int]:
    """(inicio, fim) do bloco [Unreleased]; fim e exclusivo."""
    try:
        start = next(i for i, ln in enumerate(lines) if ln.strip() == UNRELEASED)
    except StopIteration as exc:
        raise SystemExit(
            f"CHANGELOG.md nao tem uma secao '{UNRELEASED}' — nao sei onde inserir"
        ) from exc

    end = len(lines)
    for i in range(start + 1, len(lines)):
        if lines[i].startswith("## "):
            end = i
            break
    return start, end


def _section_end(lines: list[str], start: int, end: int, title: str) -> int | None:
    """Indice logo apos o ultimo conteudo de '### title', ou None se nao existe."""
    header = f"### {title}"
    try:
        idx = next(
            i for i in range(start, end) if lines[i].strip() == header
        )
    except StopIteration:
        return None

    tail = end
    for i in range(idx + 1, end):
        if lines[i].startswith("### "):
            tail = i
            break
    while tail > idx + 1 and not lines[tail - 1].strip():
        tail -= 1
    return tail


def insert(changelog: str, found: dict[str, list[tuple[Path, str]]]) -> str:
    """Insere os fragmentos no [Unreleased], respeitando secoes existentes."""
    lines = changelog.split("\n")

    for section in SECTIONS:
        if section not in found:
            continue
        title = TITLES[section]
        body = [text for _, text in found[section]]

        start, end = _unreleased_bounds(lines)
        at = _section_end(lines, start, end, title)
        if at is not None:
            # Secao ja existe: entra no fim dela, preservando a ordem atual.
            lines[at:at] = body
        else:
            # Secao nova: entra no fim do bloco [Unreleased].
            tail = end
            while tail > start + 1 and not lines[tail - 1].strip():
                tail -= 1
            lines[tail:tail] = ["", f"### {title}"] + body

    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--write",
        action="store_true",
        help="insere no CHANGELOG.md e apaga os fragmentos",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="so valida os fragmentos; exit 1 se houver problema",
    )
    parser.add_argument(
        "--fragments-dir",
        type=Path,
        default=None,
        help="diretorio dos fragmentos (default: changelog.d/ na raiz do repo)",
    )
    parser.add_argument(
        "--changelog",
        type=Path,
        default=None,
        help="caminho do CHANGELOG.md (default: raiz do repo)",
    )
    args = parser.parse_args()

    fragments_dir = args.fragments_dir or (repo_root() / "changelog.d")
    changelog_path = args.changelog or (repo_root() / "CHANGELOG.md")

    if not fragments_dir.is_dir():
        print(f"changelog.d nao encontrado em {fragments_dir}", file=sys.stderr)
        return 1

    problems = check(fragments_dir)
    if problems:
        for problem in problems:
            print(f"ERRO: {problem}", file=sys.stderr)
        return 1

    if args.check:
        found = collect(fragments_dir)
        total = sum(len(v) for v in found.values())
        print(f"OK: {total} fragmento(s) valido(s).")
        return 0

    found = collect(fragments_dir)
    if not found:
        print("Nenhum fragmento em changelog.d/ — nada a juntar.")
        return 0

    if not args.write:
        print(render(found))
        return 0

    updated = insert(changelog_path.read_text(encoding="utf-8"), found)
    changelog_path.write_text(updated, encoding="utf-8")

    removed = 0
    for entries in found.values():
        for path, _ in entries:
            path.unlink()
            removed += 1

    print(f"CHANGELOG.md atualizado; {removed} fragmento(s) consumido(s).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
