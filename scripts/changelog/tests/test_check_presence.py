"""Testes do gate de presenca de fragmento (#1228, slice D2)."""

from __future__ import annotations

import importlib.util
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[3]
MODULE_PATH = Path(__file__).resolve().parents[1] / "check_presence.py"
_spec = importlib.util.spec_from_file_location("check_presence", MODULE_PATH)
assert _spec and _spec.loader
presence = importlib.util.module_from_spec(_spec)
sys.modules["check_presence"] = presence
_spec.loader.exec_module(presence)

WORKFLOW = ROOT / ".github" / "workflows" / "changelog-presence.yml"


def pr(
    changed,
    labels=(),
    author="alguem",
    head_ref="fix/x",
    event="pull_request",
    same_repo=True,
):
    return presence.decide(
        list(changed), list(labels), author, head_ref, event, same_repo
    )


def test_fragmento_valido_passa():
    ok, motivo = pr([("M", "crates/x.rs"), ("A", "changelog.d/fixed/1-x.md")])
    assert ok, motivo
    assert "changelog.d/fixed/1-x.md" in motivo


def test_editar_fragmento_existente_passa():
    ok, _ = pr([("M", "changelog.d/added/1-x.md")])
    assert ok


def test_so_readme_falha():
    ok, motivo = pr([("M", "changelog.d/README.md")])
    assert not ok
    assert "no-changelog" in motivo


def test_readme_dentro_de_secao_nao_conta():
    ok, _ = pr([("A", "changelog.d/added/README.md")])
    assert not ok


def test_secao_invalida_falha():
    ok, _ = pr([("A", "changelog.d/novidades/1-x.md")])
    assert not ok


def test_subdiretorio_e_extensao_errada_falham():
    assert not pr([("A", "changelog.d/added/sub/1-x.md")])[0]
    assert not pr([("A", "changelog.d/added/1-x.txt")])[0]
    assert not pr([("A", "outro/changelog.d/added/1-x.md")])[0]


def test_so_apagar_fragmento_falha():
    ok, _ = pr([("D", "changelog.d/fixed/1-x.md"), ("M", "src/lib.rs")])
    assert not ok


def test_sem_mudanca_nenhuma_falha():
    assert not pr([])[0]


def test_label_no_changelog_passa():
    ok, motivo = pr([("M", "src/lib.rs")], labels=["ci", "no-changelog"])
    assert ok
    assert "no-changelog" in motivo


def test_outra_label_nao_isenta():
    assert not pr([("M", "src/lib.rs")], labels=["changelog", "no-changelog-please"])[0]


def test_dependabot_passa():
    assert pr([("M", "Cargo.lock")], author="dependabot[bot]")[0]


def test_release_passa_mesmo_apagando_fragmentos():
    ok, _ = pr([("D", "changelog.d/fixed/1-x.md")], head_ref="release/v0.4.5")
    assert ok


def test_release_de_fork_nao_isenta():
    """O nome da branch e do autor do PR: de fork, `release/*` nao isenta."""
    ok, motivo = pr(
        [("M", "src/lib.rs")], head_ref="release/v0.4.5", same_repo=False
    )
    assert not ok, motivo


def test_release_sem_versao_nao_isenta():
    for head in ("release/x", "release/v0.4", "release/v0.4.5-lixo", "release/"):
        ok, motivo = pr([("M", "src/lib.rs")], head_ref=head)
        assert not ok, f"{head}: {motivo}"


def test_main_same_repo_default_e_falso(tmp_path, capsys):
    changed = tmp_path / "changed.txt"
    changed.write_text("M\tsrc/lib.rs\n", encoding="utf-8")
    base = [
        "--event",
        "pull_request",
        "--head-ref",
        "release/v0.4.5",
        "--name-status-file",
        str(changed),
    ]
    assert presence.main(base) == 1
    assert presence.main(base + ["--same-repo", "false"]) == 1
    assert presence.main(base + ["--same-repo", "true"]) == 0
    capsys.readouterr()


def test_merge_group_e_push_passam():
    assert pr([], event="merge_group")[0]
    assert pr([], event="push")[0]


def test_parse_name_status():
    texto = "M\tcrates/a.rs\nA\tchangelog.d/fixed/1-x.md\nD\tvelho.md\n\nR100\ta.md\tchangelog.d/added/2-y.md\n"
    assert presence.parse_name_status(texto) == [
        ("M", "crates/a.rs"),
        ("A", "changelog.d/fixed/1-x.md"),
        ("D", "velho.md"),
        ("A", "changelog.d/added/2-y.md"),
    ]


def test_parse_name_status_z():
    texto = (
        "M\0crates/a.rs\0A\0changelog.d/added/1228-a\u00e7\u00e3o.md\0"
        "R100\0a.md\0changelog.d/fixed/2-y.md\0D\0velho.md\0"
    )
    assert presence.parse_name_status(texto) == [
        ("M", "crates/a.rs"),
        ("A", "changelog.d/added/1228-a\u00e7\u00e3o.md"),
        ("A", "changelog.d/fixed/2-y.md"),
        ("D", "velho.md"),
    ]


def _git(repo, *args):
    subprocess.run(
        ["git", "-C", str(repo), *args],
        check=True,
        capture_output=True,
        env={
            "PATH": os.environ.get("PATH", ""),
            "HOME": str(repo),
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_AUTHOR_NAME": "t",
            "GIT_AUTHOR_EMAIL": "t@example.invalid",
            "GIT_COMMITTER_NAME": "t",
            "GIT_COMMITTER_EMAIL": "t@example.invalid",
        },
    )


@pytest.mark.skipif(shutil.which("git") is None, reason="git ausente")
def test_fragmento_com_acento_no_nome_conta_de_ponta_a_ponta(tmp_path, capsys):
    """Roda o MESMO comando `git diff` do workflow num repo real: com o
    `core.quotePath` default o caminho sairia entre aspas com escape octal
    e o fragmento nao contaria."""
    repo = tmp_path / "repo"
    repo.mkdir()
    _git(repo, "init", "-q")
    (repo / "a.txt").write_text("a\n", encoding="utf-8")
    _git(repo, "add", "-A")
    _git(repo, "commit", "-q", "-m", "base")
    frag = repo / "changelog.d" / "added"
    frag.mkdir(parents=True)
    (frag / "1228-a\u00e7\u00e3o.md").write_text("- x\n", encoding="utf-8")
    _git(repo, "add", "-A")
    _git(repo, "commit", "-q", "-m", "frag")

    comando = _comando_diff_do_workflow()
    saida = subprocess.run(
        ["git", "-C", str(repo), *comando],
        check=True,
        capture_output=True,
        env={"PATH": os.environ.get("PATH", ""), "HOME": str(repo)},
    ).stdout
    changed = tmp_path / "changed.txt"
    changed.write_bytes(saida)
    assert (
        presence.main(
            ["--event", "pull_request", "--name-status-file", str(changed)]
        )
        == 0
    )
    capsys.readouterr()


def _comando_diff_do_workflow() -> list[str]:
    """Os argumentos do `git` no passo `Arquivos que o PR muda`, lidos do
    proprio YAML para o teste nao divergir do workflow."""
    texto = WORKFLOW.read_text(encoding="utf-8")
    bloco = next(b for b in _run_blocks(texto) if "git " in b and "diff" in b)
    comando = " ".join(
        l.strip().rstrip("\\").strip() for l in bloco.splitlines()
    )
    m = re.search(r"\bgit (.+?) > changed\.txt", comando)
    assert m, comando
    return m.group(1).split()


def test_secoes_vem_do_assemble():
    assert presence.SECTIONS == ["added", "changed", "deprecated", "removed", "fixed", "security"]


def test_main_exit_codes(tmp_path, capsys):
    changed = tmp_path / "changed.txt"
    changed.write_text("M\tsrc/lib.rs\n", encoding="utf-8")
    base = ["--event", "pull_request", "--name-status-file", str(changed)]
    assert presence.main(base + ["--labels-json", "[]"]) == 1
    assert presence.main(base + ["--labels-json", '["no-changelog"]']) == 0
    # `toJSON` de um evento sem PR devolve `null`.
    assert presence.main(["--event", "merge_group", "--labels-json", "null"]) == 0
    assert presence.main(base + ["--labels-json", "{nao json"]) == 2
    assert presence.main(base + ["--labels-json", '"no-changelog"']) == 2
    capsys.readouterr()


# --- contrato do workflow -------------------------------------------------


def _run_blocks(texto: str) -> list[str]:
    """O conteudo de cada `run:` do YAML, por indentacao (sem pyyaml, que o
    job do CI nao instala)."""
    blocos: list[str] = []
    linhas = texto.splitlines()
    i = 0
    while i < len(linhas):
        m = re.match(r"^(\s*)(?:- )?run:\s*(.*)$", linhas[i])
        if not m:
            i += 1
            continue
        indent = len(m.group(1))
        resto = m.group(2)
        corpo = [] if resto in ("|", ">", "") else [resto]
        i += 1
        while i < len(linhas):
            linha = linhas[i]
            if linha.strip() and len(linha) - len(linha.lstrip()) <= indent:
                break
            corpo.append(linha)
            i += 1
        blocos.append("\n".join(corpo))
    return blocos


@pytest.fixture(scope="module")
def workflow() -> str:
    return WORKFLOW.read_text(encoding="utf-8")


def test_workflow_nao_interpola_expressao_no_run(workflow):
    blocos = _run_blocks(workflow)
    assert blocos, "o workflow tem de ter passos run:"
    for bloco in blocos:
        assert "${{" not in bloco, f"expressao interpolada no run:\n{bloco}"


def test_workflow_e_pull_request_read_only_e_sem_continue_on_error(workflow):
    sem_comentario = "\n".join(
        l for l in workflow.splitlines() if not l.lstrip().startswith("#")
    )
    assert "pull_request_target" not in sem_comentario
    assert "continue-on-error" not in sem_comentario
    assert re.search(r"^permissions:\s*\n\s+contents: read\s*$", sem_comentario, re.M)
    assert "secrets." not in sem_comentario
    for tipo in ("labeled", "unlabeled", "synchronize"):
        assert tipo in sem_comentario
    assert "pull_request.title" not in sem_comentario
    assert "pull_request.body" not in sem_comentario
    assert "check_presence.py" in sem_comentario


def test_workflow_passa_same_repo_do_evento(workflow):
    assert (
        "github.event.pull_request.head.repo.full_name == github.repository"
        in workflow
    )
    assert '--same-repo "${SAME_REPO:-false}"' in workflow
