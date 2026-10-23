#!/usr/bin/env bash
#
# negative-tamper.sh — teste negativo da fatia B (issue #1636)
#
# Prova que build-node-runtime.sh RECUSA artefatos comprometidos, exercitando os
# dois portoes de seguranca com um mirror local file://:
#
#   caso "integridade":  SHASUMS256.txt + .sig GENUINOS (baixados de nodejs.org),
#                        mas o artefato esta adulterado  -> sha256 nao bate.
#   caso "procedencia":  SHASUMS256.txt ADULTERADO       -> assinatura invalida.
#
# Nunca extrai nem roda binario nenhum: as duas falhas acontecem antes disso, por
# isso o teste roda em qualquer SO (inclusive so no Linux do CI). Precisa de rede
# para baixar o SHASUMS256.txt + .sig genuinos (poucos KB).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
NR_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
BUILD="${NR_DIR}/build-node-runtime.sh"
[ -f "$BUILD" ] || { echo "ERRO: build-node-runtime.sh nao encontrado"; exit 2; }

VER="$(tr -d ' \t\r\n' < "${NR_DIR}/node-version.txt")"
PLATFORM="linux-x64"
ARCHIVE="node-v${VER}-${PLATFORM}.tar.xz"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "==> baixando SHASUMS256.txt + .sig genuinos de v${VER}"
curl -fsSL --retry 3 --proto '=https' --tlsv1.2 \
  "https://nodejs.org/dist/v${VER}/SHASUMS256.txt" -o "${WORK}/SHASUMS256.txt"
curl -fsSL --retry 3 --proto '=https' --tlsv1.2 \
  "https://nodejs.org/dist/v${VER}/SHASUMS256.txt.sig" -o "${WORK}/SHASUMS256.txt.sig"

# Converte um diretorio local para uma URL file:// que o curl aceita nos tres SOs.
to_file_url() {
  local p="$1"
  if command -v cygpath >/dev/null 2>&1; then
    p="$(cygpath -m "$p")"   # /c/Users/... -> C:/Users/...
  fi
  case "$p" in
    /*) echo "file://${p}" ;;     # /tmp/x       -> file:///tmp/x
    *)  echo "file:///${p}" ;;    # C:/Users/x   -> file:///C:/Users/x
  esac
}

run_case() {
  local label="$1" dist="$2" expect="$3"
  local url out rc
  url="$(to_file_url "$dist")"
  set +e
  out="$("$BUILD" --platform "$PLATFORM" --out "${WORK}/out-${label}" --dist-base-url "$url" 2>&1)"
  rc=$?
  set -e
  echo "----- caso ${label}: rc=${rc} -----"
  echo "$out" | tail -4
  if [ "$rc" -eq 0 ]; then
    echo "FALHA DO TESTE: o build deveria ter RECUSADO o caso ${label}, mas saiu 0"
    exit 1
  fi
  if ! echo "$out" | grep -qi "$expect"; then
    echo "FALHA DO TESTE: esperava mensagem contendo [${expect}] no caso ${label}"
    exit 1
  fi
  echo "OK caso ${label}: build recusou como esperado (mensagem contem [${expect}])"
}

# --- caso integridade: artefato adulterado, SHASUMS/sig genuinos ------------
C1="${WORK}/fake-integrity/v${VER}"
mkdir -p "$C1"
cp "${WORK}/SHASUMS256.txt" "${WORK}/SHASUMS256.txt.sig" "$C1/"
printf 'ARTEFATO ADULTERADO — nao e o Node oficial\n' > "${C1}/${ARCHIVE}"
run_case "integridade" "${WORK}/fake-integrity" "sha256 nao bate"

# --- caso procedencia: SHASUMS adulterado (assinatura nao casa) -------------
C2="${WORK}/fake-provenance/v${VER}"
mkdir -p "$C2"
# Qualquer byte a mais quebra a assinatura destacada sobre o conteudo exato.
cp "${WORK}/SHASUMS256.txt" "${C2}/SHASUMS256.txt"
printf 'linha-injetada-pelo-atacante\n' >> "${C2}/SHASUMS256.txt"
cp "${WORK}/SHASUMS256.txt.sig" "${C2}/"
printf 'qualquer coisa\n' > "${C2}/${ARCHIVE}"
run_case "procedencia" "${WORK}/fake-provenance" "gpg --verify falhou"

echo "TESTE NEGATIVO OK: os dois portoes (procedencia e integridade) recusam adulteracao."
