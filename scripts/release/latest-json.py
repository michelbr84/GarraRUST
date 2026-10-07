#!/usr/bin/env python3
"""Gera o `latest.json` do Tauri updater a partir dos artefactos de release.

O `tauri-plugin-updater` do app ja consulta
`releases/latest/download/latest.json`; o problema da #1568 e que nenhum
workflow publicava esse arquivo — e, sem `createUpdaterArtifacts: true`, o
bundler nem emitia os `.tar.gz`/`.zip` de updater com a `.sig` ao lado.

Formato consumido pelo plugin (Tauri v2):

    {
      "version": "0.4.8",
      "notes": "...",
      "pub_date": "2026-10-07T12:00:00Z",
      "platforms": {
        "darwin-x86_64":   {"signature": "<base64>", "url": "https://..."},
        "darwin-aarch64":  {"signature": "...", "url": "..."},
        "linux-x86_64":    {"signature": "...", "url": "..."},
        "windows-x86_64":  {"signature": "...", "url": "..."}
      }
    }

Regras deste gerador (todas deliberadas):

- **Fail-closed**: se nao existir NENHUM artefacto de updater com `.sig`, o
  script recusa (exit 1). Publicar `latest.json` vazio faria o updater do app
  falhar no usuario final em vez de aqui, no CI.
- **Plataformas ausentes sao omitidas**, nao viram entrada vazia: um build
  best-effort que nao saiu contribui menos assinatura, nao uma entrada quebra.
- A URL e o `release download` canonico do GitHub para a tag — o mesmo host do
  endpoint configurado no `tauri.conf.json`.
- A assinatura vem do arquivo `.sig` vizinho, texto puro (base64), sem
  transformacao: e exatamente o que o plugin espera.

Uso:
    python3 scripts/release/latest-json.py \
        --version 0.4.8 \
        --repo michelbr84/GarraRUST \
        --artifacts-dir artifacts \
        --out latest.json
"""

from __future__ import annotations

import argparse
import base64
import json
import sys
from pathlib import Path

# Pares (padrao de fim de nome, chave de plataforma do updater). O nome real
# do artefacto depende do bundler e de como o job de build o estagia — por
# isso a correspondencia e por padrao de fim, nao por nome exato.
#
#   *.AppImage.tar.gz  -> linux-x86_64 / linux-aarch64
#   *.app.tar.gz       -> darwin-x86_64 / darwin-aarch64   (macOS)
#   *.nsis.zip         -> windows-x86_64 / windows-aarch64
#   *.msi.zip          -> windows-x86_64 / windows-aarch64
PADROES = [
    (".AppImage.tar.gz", "linux"),
    (".app.tar.gz", "darwin"),
    (".nsis.zip", "windows"),
    (".msi.zip", "windows"),
    # Tauri v2 com `createUpdaterArtifacts: true` assina o proprio bundle no
    # Linux: o artefacto de updater e o `.AppImage` com `.sig` vizinho (o
    # `.AppImage.tar.gz` e o formato legado). O prefixo `garraia-desktop-`
    # e obrigatorio aqui: o job package-linux publica TAMBEM um
    # `garraia-linux-x86_64.AppImage` (o pacote da CLI) que nao tem `.sig` e
    # sem o guard o gerador entraria em fail-closed por causa dele.
    (".AppImage", "linux"),
]


def _plataforma_do_nome(nome: str) -> str | None:
    """Deriva a chave do updater a partir do nome do artefacto."""
    for padrao, sistema in PADROES:
        if not nome.endswith(padrao):
            continue
        # So o app desktop e atualizavel pelo tauri-plugin-updater; o
        # AppImage da CLI (garraia-linux-*) nao tem `.sig` e nao deve
        # disputar a chave linux-x86_64 com o artefacto do desktop.
        if padrao == ".AppImage" and not nome.startswith("garraia-desktop-"):
            continue
        arquitetura = "aarch64" if "aarch64" in nome or "arm64" in nome else "x86_64"
        return f"{sistema}-{arquitetura}"
    return None


def _acha_artefactos(dir_artefactos: Path) -> dict[str, Path]:
    """Indice os artefactos de updater pela chave de plataforma derivada."""
    encontrados: dict[str, Path] = {}
    if not dir_artefactos.is_dir():
        return encontrados
    for caminho in sorted(dir_artefactos.rglob("*")):
        if not caminho.is_file() or caminho.name.endswith(".sig"):
            continue
        chave = _plataforma_do_nome(caminho.name)
        if chave is None:
            continue
        # Primeira ocorrência vence (ordenada por caminho); o gerador de
        # release estagia um artefacto por plataforma.
        encontrados.setdefault(chave, caminho)
    return encontrados


def _assinatura(artefacto: Path) -> str:
    """Le a `.sig` vizinha; recusa se ausente ou vazia (fail-closed)."""
    sig = artefacto.with_name(artefacto.name + ".sig")
    if not sig.is_file():
        raise FileNotFoundError(f"assinatura ausente para {artefacto.name}: {sig.name}")
    texto = sig.read_text(encoding="utf-8").strip()
    if not texto:
        raise ValueError(f"assinatura vazia em {sig}")
    # Valida que e base64 — o plugin decodifica; quebrar aqui e melhor que
    # quebrar no updater do usuario.
    try:
        base64.b64decode(texto, validate=True)
    except Exception as exc:  # noqa: BLE001 - mensagem importa, tipo nao
        raise ValueError(f"assinatura em {sig} nao e base64 valido: {exc}") from exc
    return texto


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True, help="Versao sem 'v' (ex.: 0.4.8)")
    parser.add_argument("--repo", required=True, help="owner/name do repositorio")
    parser.add_argument(
        "--artifacts-dir",
        required=True,
        type=Path,
        help="Diretorio com os artefactos baixados do workflow",
    )
    parser.add_argument("--out", required=True, type=Path, help="Caminho do latest.json")
    parser.add_argument(
        "--notes",
        default="",
        help="Notas curtas exibidas no dialogo de atualizacao",
    )
    args = parser.parse_args()

    artefactos = _acha_artefactos(args.artifacts_dir)
    if not artefactos:
        print(
            "ERRO: nenhum artefacto de updater encontrado em "
            f"{args.artifacts_dir} — createUpdaterArtifacts esta ligado? "
            "Recusando publicar latest.json vazio (fail-closed).",
            file=sys.stderr,
        )
        return 1

    plataformas: dict[str, dict[str, str]] = {}
    for chave, artefato in artefactos.items():
        if chave in plataformas:
            continue  # nao sobrescrever a primeira fonte valida
        try:
            assinatura = _assinatura(artefato)
        except (FileNotFoundError, ValueError) as exc:
            print(f"ERRO: {exc}", file=sys.stderr)
            return 1
        # URL canonica de download da tag no proprio repo.
        url = f"https://github.com/{args.repo}/releases/download/v{args.version}/{artefato.name}"
        plataformas[chave] = {"signature": assinatura, "url": url}

    if not plataformas:
        print("ERRO: nenhum artefacto com assinatura valida.", file=sys.stderr)
        return 1

    from datetime import datetime, timezone

    doc = {
        "version": args.version,
        "notes": args.notes,
        "pub_date": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": plataformas,
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(doc, indent=2) + "\n", encoding="utf-8")
    print(f"latest.json gerado com {len(plataformas)} plataforma(s): {sorted(plataformas)}")
    print(f"  -> {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
