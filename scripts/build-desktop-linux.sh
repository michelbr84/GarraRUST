#!/usr/bin/env bash
# build-desktop-linux.sh
# Gera os pacotes Linux do Garra Desktop (gateway + overlay num único pacote):
# .deb e AppImage, via bundler do próprio Tauri.
#
# Uso:
#   ./scripts/build-desktop-linux.sh
#   STAGE_DIR=/tmp/out ./scripts/build-desktop-linux.sh   # copia os bundles p/ lá
#
# Pré-requisitos (ubuntu-22.04; mesmo baseline glibc 2.35 do release.yml):
#   sudo apt-get install -y libwebkit2gtk-4.1-dev build-essential curl wget file \
#     libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev \
#     patchelf libgtk-3-dev libfuse2
#   cargo install tauri-cli --version "^2" --locked
#
# Este script é a fonte única de verdade de "como se constrói o Garra Desktop
# no Linux" — irmão do scripts/build-installer.ps1 (Windows). Os jobs
# build-linux-bundles (desktop.yml) e build-linux-desktop (release.yml) o
# invocam em vez de repetir os passos.
#
# NO_STRIP/APPIMAGE_EXTRACT_AND_RUN ficam AQUI, não só no workflow: o
# linuxdeploy que o Tauri baixa é um AppImage (precisa de FUSE ou de
# extract-and-run) e tem uma falha conhecida de strip — quem rodar o script
# fora do CI herda os dois contornos.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

host_triple="$(rustc -vV | sed -n 's/^host: //p')"
arch="${host_triple%%-*}"

echo "==> [1/4] Compilando gateway (release)..."
cargo build -p garraia --release

echo "==> [2/4] Copiando sidecar para binaries/..."
bin_dir="crates/garraia-desktop/src-tauri/binaries"
mkdir -p "$bin_dir"
# Tauri externalBin = "binaries/garraia" espera "binaries/garraia-<triple>".
# O basename tem de casar com o `.sidecar("garraia")` de src/gateway.rs —
# se divergir, o app instala mas o gateway nunca sobe (o desktop.yml assevera
# essa igualdade). O binário da CLI em si chama-se `garra`.
cp "target/release/garra" "$bin_dir/garraia-$host_triple"
echo "    Copiado: $bin_dir/garraia-$host_triple"

echo "==> [3/4] Gerando bundles com cargo tauri build (deb + appimage)..."
(
  cd crates/garraia-desktop/src-tauri
  NO_STRIP=1 APPIMAGE_EXTRACT_AND_RUN=1 cargo tauri build --bundles deb appimage
)

echo "==> [4/4] Localizando bundles..."
# Falha alto de propósito, como o build-installer.ps1: um `cargo tauri build`
# que saia 0 sem emitir bundle não pode deixar o script verde.
shopt -s nullglob
debs=("target/release/bundle/deb/"*.deb)
appimages=("target/release/bundle/appimage/"*.AppImage)
if [ ${#debs[@]} -eq 0 ]; then
  echo "ERRO: nenhum .deb encontrado em target/release/bundle/deb/ após o build" >&2
  exit 1
fi
if [ ${#appimages[@]} -eq 0 ]; then
  echo "ERRO: nenhum .AppImage encontrado em target/release/bundle/appimage/ após o build" >&2
  exit 1
fi
echo "    DEB:      ${debs[0]}"
echo "    AppImage: ${appimages[0]}"

if [ -n "${STAGE_DIR:-}" ]; then
  mkdir -p "$STAGE_DIR"
  # Nomes estáveis, sem versão (mesma razão do garraia-desktop-windows-*.msi):
  # a URL /releases/latest/download/garraia-desktop-linux-<arch>.deb é
  # permanente e pode ser fixada na documentação. Aditivo aos assets da CLI
  # (garraia-linux-<arch>.deb) — regra 15 do CLAUDE.md.
  cp "${debs[0]}" "$STAGE_DIR/garraia-desktop-linux-$arch.deb"
  echo "    Staged: garraia-desktop-linux-$arch.deb"
  cp "${appimages[0]}" "$STAGE_DIR/garraia-desktop-linux-$arch.AppImage"
  echo "    Staged: garraia-desktop-linux-$arch.AppImage"
fi

echo ""
echo "Pacotes Linux gerados com sucesso!"
