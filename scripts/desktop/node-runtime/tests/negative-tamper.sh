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

CHECK="${NR_DIR}/lib/check-gpg-status.sh"
ALLOW="${NR_DIR}/keys/FINGERPRINTS.txt"
# Um fingerprint que ESTA na allowlist (Antoine du Hamel, assinou v24.22.1).
ALLOWED_FPR="5BE8A3F6C8A5C01D106C0AD820B1A390B168D356"

# --- parte 1: teste unitario da decisao de procedencia (sem rede, sem chave) --
# Prova, com status sinteticos do gpg, que a funcao de parse recusa chave
# expirada/revogada/assinatura ruim e so aceita GOODSIG+VALIDSIG allowlistado.
echo "==> unit: decisao de procedencia (lib/check-gpg-status.sh)"
unit_case() {
  local label="$1" statusfile="$2" want="$3" want_fpr="${4:-}"
  local out rc
  set +e
  out="$(bash "$CHECK" "$statusfile" "$ALLOW" 2>/dev/null)"
  rc=$?
  set -e
  if [ "$want" = pass ]; then
    if [ "$rc" -ne 0 ]; then echo "FALHA unit ${label}: esperava pass, rc=${rc}"; exit 1; fi
    if [ "$out" != "$want_fpr" ]; then echo "FALHA unit ${label}: fpr '${out}' != '${want_fpr}'"; exit 1; fi
  else
    if [ "$rc" -eq 0 ]; then echo "FALHA unit ${label}: esperava recusa, mas passou (out=${out})"; exit 1; fi
  fi
  echo "   OK unit ${label} (${want})"
}

VS="VALIDSIG ${ALLOWED_FPR} 2026-10-21 1792578925 0 4 0 22 8 00 ${ALLOWED_FPR}"
NOT_ALLOWED="1111111111111111111111111111111111111111"

printf '[GNUPG:] GOODSIG 20B1A390B168D356 Antoine du Hamel\n[GNUPG:] %s\n' "$VS" > "${WORK}/st-good.txt"
unit_case good-allowlistado "${WORK}/st-good.txt" pass "$ALLOWED_FPR"

printf '[GNUPG:] EXPKEYSIG 20B1A390B168D356 Antoine du Hamel\n[GNUPG:] %s\n' "$VS" > "${WORK}/st-exp.txt"
unit_case chave-expirada "${WORK}/st-exp.txt" fail

printf '[GNUPG:] REVKEYSIG 20B1A390B168D356 Antoine du Hamel\n[GNUPG:] %s\n' "$VS" > "${WORK}/st-rev.txt"
unit_case chave-revogada "${WORK}/st-rev.txt" fail

printf '[GNUPG:] BADSIG 20B1A390B168D356 Antoine du Hamel\n' > "${WORK}/st-bad.txt"
unit_case assinatura-ruim "${WORK}/st-bad.txt" fail

printf '[GNUPG:] GOODSIG 1111 Alguem\n[GNUPG:] VALIDSIG %s 2026-10-21 0 0 4 0 22 8 00 %s\n' "$NOT_ALLOWED" "$NOT_ALLOWED" > "${WORK}/st-notallow.txt"
unit_case assinante-fora-da-allowlist "${WORK}/st-notallow.txt" fail

printf '[GNUPG:] %s\n' "$VS" > "${WORK}/st-nogood.txt"
unit_case sem-GOODSIG "${WORK}/st-nogood.txt" fail

echo "   unit OK: a decisao de procedencia recusa expirada/revogada/ruim/fora-da-allowlist."

# --- parte 2: fim-a-fim com mirror file:// (precisa de rede p/ SHASUMS+sig) ---
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
