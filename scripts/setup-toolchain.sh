#!/usr/bin/env bash
# setup-toolchain.sh — instala e ativa localmente a toolchain minima do
# workspace (MSRV, hoje 1.95 — ver o job "MSRV check (1.95)" em ci.yml).
#
# Por que nao um rust-toolchain.toml na raiz: ja foi tentado (issue #1452,
# PR #1454) e quebrou CI real. O arquivo faz o rustup ganhar precedencia
# sobre a toolchain que dtolnay/rust-toolchain acabou de instalar COM
# targets/components extras (Android, Windows ARM64), entao os jobs de
# cross-compile perdem esses targets e falham com "can't find crate for
# `core`". Este script evita isso de proposito: usa `rustup override set`,
# que grava a preferencia em `~/.rustup/settings.toml` (por diretorio, no
# HOME de quem roda), nunca em um arquivo do repo — CI nao ve nem herda.
set -euo pipefail

MSRV="1.95"

if ! command -v rustup >/dev/null 2>&1; then
  echo "rustup nao encontrado. Instale em https://rustup.rs antes de continuar." >&2
  exit 1
fi

if ! rustup toolchain list | grep -q "^${MSRV}"; then
  echo "Instalando toolchain ${MSRV} (rustfmt + clippy)..."
  rustup toolchain install "${MSRV}" --profile minimal --component rustfmt --component clippy
fi

rustup override set "${MSRV}"

echo "Toolchain ${MSRV} ativa para $(pwd) (override local, nao commitado ao repo)."
rustc --version
