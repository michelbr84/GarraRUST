"""Testes do agregador de fragmentos de changelog."""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

MODULE_PATH = Path(__file__).resolve().parents[1] / "assemble.py"
_spec = importlib.util.spec_from_file_location("assemble", MODULE_PATH)
assert _spec and _spec.loader
assemble = importlib.util.module_from_spec(_spec)
sys.modules["assemble"] = assemble
_spec.loader.exec_module(assemble)


def make_fragments(tmp_path: Path, files: dict[str, str]) -> Path:
    """Monta um changelog.d/ de mentira. Chaves sao 'secao/nome.md'."""
    root = tmp_path / "changelog.d"
    for section in assemble.SECTIONS:
        (root / section).mkdir(parents=True, exist_ok=True)
    for rel, content in files.items():
        (root / rel).write_text(content, encoding="utf-8")
    return root


CHANGELOG_COM_SECAO = """# Changelog

## [Unreleased]

### Fixed
- **Ja existia (#1).** Corpo.

## [0.3.9] - 2026-09-05

### Added
- **Antigo (#0).** Nao pode ser tocado.
"""


def test_collect_agrupa_por_secao_e_ordena_por_nome(tmp_path):
    root = make_fragments(
        tmp_path,
        {
            "fixed/972-b.md": "- **B (#972).** Corpo b.",
            "fixed/900-a.md": "- **A (#900).** Corpo a.",
            "added/950-cli.md": "- **CLI (#950).** Corpo cli.",
        },
    )
    found = assemble.collect(root)

    assert set(found) == {"fixed", "added"}
    # Ordem alfabetica por nome de arquivo — deterministica.
    assert [p.name for p, _ in found["fixed"]] == ["900-a.md", "972-b.md"]


def test_render_usa_a_ordem_do_keep_a_changelog(tmp_path):
    root = make_fragments(
        tmp_path,
        {
            "security/1-s.md": "- **S.** corpo.",
            "added/1-a.md": "- **A.** corpo.",
            "fixed/1-f.md": "- **F.** corpo.",
        },
    )
    saida = assemble.render(assemble.collect(root))

    assert saida.index("### Added") < saida.index("### Fixed")
    assert saida.index("### Fixed") < saida.index("### Security")


def test_insert_anexa_no_fim_de_secao_existente(tmp_path):
    root = make_fragments(tmp_path, {"fixed/2-novo.md": "- **Novo (#2).** Corpo."})
    saida = assemble.insert(CHANGELOG_COM_SECAO, assemble.collect(root))

    unreleased = saida.split("## [0.3.9]")[0]
    assert unreleased.count("### Fixed") == 1, "nao pode duplicar a secao"
    assert unreleased.index("Ja existia (#1)") < unreleased.index("Novo (#2)")


def test_insert_nao_toca_em_versao_ja_publicada(tmp_path):
    root = make_fragments(tmp_path, {"added/2-novo.md": "- **Novo (#2).** Corpo."})
    saida = assemble.insert(CHANGELOG_COM_SECAO, assemble.collect(root))

    publicada = saida.split("## [0.3.9]")[1]
    assert "Novo (#2)" not in publicada, "entrada caiu numa versao ja publicada"
    assert "Antigo (#0)" in publicada
    # E entrou no Unreleased, numa secao Added criada do zero.
    assert "Novo (#2)" in saida.split("## [0.3.9]")[0]


def test_check_reprova_fragmento_vazio_e_sem_bullet(tmp_path):
    root = make_fragments(
        tmp_path,
        {
            "fixed/1-vazio.md": "   \n",
            "fixed/2-sem-bullet.md": "isto nao e um bullet",
            "fixed/3-bom.md": "- **Bom.** corpo.",
        },
    )
    problemas = assemble.check(root)

    assert any("vazio" in p for p in problemas)
    assert any("bullet" in p for p in problemas)
    assert not any("3-bom" in p for p in problemas)


def test_check_reprova_pasta_desconhecida_e_fragmento_solto(tmp_path):
    root = make_fragments(tmp_path, {})
    (root / "inventada").mkdir()
    (root / "solto.md").write_text("- **Solto.** corpo.", encoding="utf-8")

    problemas = assemble.check(root)

    assert any("inventada" in p for p in problemas)
    assert any("solto.md" in p for p in problemas)


def test_check_aceita_o_readme_na_raiz(tmp_path):
    root = make_fragments(tmp_path, {})
    (root / "README.md").write_text("# doc", encoding="utf-8")

    assert assemble.check(root) == []
