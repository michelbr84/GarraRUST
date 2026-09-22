"""Testes do checador de assets da release (`scripts/release/check_files.py`)."""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

MODULE_PATH = Path(__file__).resolve().parents[1] / "check_files.py"
_spec = importlib.util.spec_from_file_location("check_files", MODULE_PATH)
assert _spec and _spec.loader
check_files = importlib.util.module_from_spec(_spec)
sys.modules["check_files"] = check_files
_spec.loader.exec_module(check_files)

REPO = Path(__file__).resolve().parents[3]

WORKFLOW = """
jobs:
  release:
    steps:
      - name: Stage release assets
        run: |
          cp artifacts/garraia-linux-x86_64/garraia-linux-x86_64 release/
          for pkg in garraia-linux-aarch64.deb garraia-linux-aarch64.AppImage; do
            if [ -f "artifacts/garraia-linux-packages/${pkg}" ]; then
              cp "artifacts/garraia-linux-packages/${pkg}" release/
            fi
          done
          cp install.sh release/
      - name: Create Release
        uses: softprops/action-gh-release@v3
        with:
          files: |
            release/garraia-linux-x86_64
            release/garraia-linux-aarch64.deb
            %s
            release/install.sh
            release/*.sha256
        env:
          GITHUB_TOKEN: x
"""


def test_o_release_yml_do_repo_sobe_tudo_que_copia():
    texto = (REPO / ".github" / "workflows" / "release.yml").read_text(encoding="utf-8")
    assert check_files.padroes_do_upload(texto), "a lista files: do softprops sumiu"
    assert check_files.fora_do_upload(texto) == []


def test_o_caso_da_v044_e_pego():
    """AppImage aarch64 copiado para release/ e ausente do upload."""
    texto = WORKFLOW % ""
    assert check_files.fora_do_upload(texto) == ["garraia-linux-aarch64.AppImage"]


def test_listado_passa():
    texto = WORKFLOW % "release/garraia-linux-aarch64.AppImage"
    assert check_files.fora_do_upload(texto) == []


def test_glob_do_upload_cobre_o_nome():
    texto = WORKFLOW % "release/*.AppImage"
    assert check_files.fora_do_upload(texto) == []


def test_nomes_com_variavel_nao_entram():
    texto = "cp \"$ARQ\" release/\nfor pkg in $LISTA; do\n"
    assert check_files.nomes_copiados(texto) == set()


def test_le_os_nomes_do_for_e_do_cp():
    texto = WORKFLOW % ""
    assert check_files.nomes_copiados(texto) == {
        "garraia-linux-x86_64",
        "garraia-linux-aarch64.deb",
        "garraia-linux-aarch64.AppImage",
        "install.sh",
    }


def test_sem_passo_de_upload_e_erro(tmp_path):
    arq = tmp_path / "release.yml"
    arq.write_text("jobs: {}\n", encoding="utf-8")
    assert check_files.main(["x", str(arq)]) == 1
