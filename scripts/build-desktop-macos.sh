#!/usr/bin/env bash
# build-desktop-macos.sh
# Gera o pacote macOS do Garra Desktop (gateway + overlay num unico pacote):
# DMG, via bundler do proprio Tauri.
#
# Uso:
#   ./scripts/build-desktop-macos.sh
#   STAGE_DIR=/tmp/out ./scripts/build-desktop-macos.sh   # copia os bundles p/ la
#
# Pre-requisitos (macos-latest; mesmo toolchain do release.yml):
#   cargo install tauri-cli --version "^2" --locked
#
# Este script e a fonte unica de verdade de "como se constrói o Garra Desktop
# no macOS" — irmao do scripts/build-desktop-linux.sh (Linux) e do
# scripts/build-installer.ps1 (Windows). Os jobs build-macos-desktop
# (release.yml) e build-macos-bundles (desktop.yml) o invocam em vez de
# repetir os passos.
#
# Como os irmaos, falha alto de proposito: um `cargo tauri build` que saia 0
# sem emitir bundle nao pode deixar o script verde.

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
cp "target/release/garra" "$bin_dir/garraia-$host_triple"

echo "==> [3/4] Gerando bundles com cargo tauri build (app + dmg)..."
(
  cd crates/garraia-desktop/src-tauri
  cargo tauri build --bundles app dmg
)

echo "==> [4/4] Localizando bundles..."
# Falha alto de proposito, como os irmaos: sem .dmg o job ficaria verde com
# release sem pacote macOS.
shopt -s nullglob
# Dentro do workspace o `cargo tauri build` escreve em target/ NA RAIZ
# (src-tauri e membro do workspace — e o que o CI faz); um checkout avulso do
# crate deixa o bundle em src-tauri/target. Checa os dois caminhos. O bug que
# o job build-macos-desktop do CI pegou em #1586 era so olhar o segundo.
bundle_root="target/release/bundle"
dmgs=("$bundle_root/dmg/"*.dmg)
if [ ${#dmgs[@]} -eq 0 ]; then
  bundle_root="crates/garraia-desktop/src-tauri/target/release/bundle"
  dmgs=("$bundle_root/dmg/"*.dmg)
fi
apps=("$bundle_root/macos/"*.app)
app_tar=("$bundle_root/macos/"*.app.tar.gz)
if [ ${#dmgs[@]} -eq 0 ]; then
  echo "ERRO: nenhum .dmg encontrado em $bundle_root/dmg/ apos o build" >&2
  exit 1
fi

staging="${STAGE_DIR:-}"
if [ -n "$staging" ]; then
  mkdir -p "$staging"
  cp "${dmgs[@]}" "$staging/"
  # O .app tambem sobe como artefacto (util para inspecao/manual install).
  if [ ${#apps[@]} -gt 0 ]; then
    cp -R "${apps[@]}" "$staging/"
  fi
  # Artefactos de updater (#1568): o .app.tar.gz + .sig sao o que o
  # latest.json referencia por darwin-*. Staged com nome estavel por
  # arquitetura. Condicionais: sem TAURI_SIGNING_PRIVATE_KEY o bundler nao
  # emite, e o DMG continua sendo o pacote de distribuicao.
  if [ ${#app_tar[@]} -gt 0 ] && [ -f "${app_tar[0]}.sig" ]; then
    cp "${app_tar[0]}" "$staging/garraia-desktop-macos-$arch.app.tar.gz"
    cp "${app_tar[0]}.sig" "$staging/garraia-desktop-macos-$arch.app.tar.gz.sig"
    echo "    Staged: garraia-desktop-macos-$arch.app.tar.gz + .sig (updater)"
  else
    echo "    AVISO: .app.tar.gz de updater ausente em $bundle_root/macos/ — latest.json vai omitir darwin" >&2
  fi
  echo "bundles copiados para $staging:"
  ls -l "$staging"
else
  echo "bundles gerados em $bundle_root/:"
  ls -l "${dmgs[@]}"
fi

echo "OK: DMG macOS gerado (arquitetura $arch)"
