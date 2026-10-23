#!/usr/bin/env bash
#
# build-node-runtime.sh — fatia B da issue #1636
#
# Monta, de forma reprodutivel e verificavel, o runtime Node.js PRIVADO que o
# instalador do GarraIA Desktop vai empacotar, junto com o node_modules
# pre-construido da ponte do WhatsApp (bridge/whatsapp). Para uma plataforma por
# vez, roda nativamente no runner da plataforma alvo (sem cross-install):
#
#   build-node-runtime.sh --platform win-x64|linux-x64|darwin-arm64 --out <dir>
#
# O que ele garante, nesta ordem:
#   a. baixa os artefatos OFICIAIS de https://nodejs.org/dist/v<versao>/
#   b. PROCEDENCIA: gpg --verify do SHASUMS256.txt.sig contra um keyring isolado
#      montado so com as chaves vendorizadas (keys/*.asc), e exige que a chave
#      PRIMARIA que assinou esteja na allowlist keys/FINGERPRINTS.txt;
#   c. INTEGRIDADE: sha256 do artefato contra a linha exata do SHASUMS256.txt;
#   d. extrai so o runtime (binario node + LICENSE) no layout combinado com a
#      fatia A: Windows node/node.exe, Unix node/bin/node;
#   e. monta a ponte: copia bridge/whatsapp (sem node_modules) e roda
#      `npm ci --omit=dev` com o node E o npm do MESMO tarball oficial baixado;
#   f. gera THIRD_PARTY_NOTICES.txt (Node + todos os pacotes de node_modules);
#   g. grava manifest.json com versao, sha256, fingerprint do assinante e o
#      estado do lock da ponte.
#
# A versao exata do Node vem de node-version.txt (fonte unica de verdade).
#
# Dependencias de runtime (presentes em Git Bash no Windows e nos runners
# Linux/macOS): bash, curl, gpg, tar (e xz para .tar.xz no Linux), unzip (win),
# sha256sum OU shasum. Node/npm vem do proprio tarball baixado — nenhum node
# global e usado para montar a ponte.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEFAULT_KEYS_DIR="${SCRIPT_DIR}/keys"
DEFAULT_LIB_DIR="${SCRIPT_DIR}/lib"
DEFAULT_VERSION_FILE="${SCRIPT_DIR}/node-version.txt"
# scripts/desktop/node-runtime -> raiz do repo
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"

DIST_BASE_URL="https://nodejs.org/dist"
PLATFORM=""
OUT_DIR=""
NODE_VERSION=""
KEYS_DIR="${DEFAULT_KEYS_DIR}"
BRIDGE_DIR="${REPO_ROOT}/bridge/whatsapp"

die() { echo "ERRO: $*" >&2; exit 1; }
info() { echo "==> $*"; }

usage() {
  cat >&2 <<'USAGE'
uso: build-node-runtime.sh --platform <p> --out <dir> [opcoes]

  --platform <p>        win-x64 | linux-x64 | darwin-arm64  (obrigatorio)
  --out <dir>           diretorio de saida do bundle          (obrigatorio)
  --node-version <v>    sobrepoe node-version.txt (ex.: 24.22.1)
  --dist-base-url <u>   base do mirror (default https://nodejs.org/dist;
                        aceita file:// para testes)
  --keys-dir <dir>      diretorio das chaves .asc + FINGERPRINTS.txt
  --bridge-dir <dir>    diretorio de origem da ponte (default bridge/whatsapp)
  -h, --help            esta ajuda
USAGE
}

while [ $# -gt 0 ]; do
  case "$1" in
    --platform) PLATFORM="${2:-}"; shift 2 ;;
    --out) OUT_DIR="${2:-}"; shift 2 ;;
    --node-version) NODE_VERSION="${2:-}"; shift 2 ;;
    --dist-base-url) DIST_BASE_URL="${2:-}"; shift 2 ;;
    --keys-dir) KEYS_DIR="${2:-}"; shift 2 ;;
    --bridge-dir) BRIDGE_DIR="${2:-}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) usage; die "argumento desconhecido: $1" ;;
  esac
done

[ -n "$PLATFORM" ] || { usage; die "--platform e obrigatorio"; }
[ -n "$OUT_DIR" ] || { usage; die "--out e obrigatorio"; }

# O bundle e montado do zero com `rm -rf "$OUT_DIR"`; recusa alvos perigosos
# (raiz, raiz de drive, HOME, diretorios de sistema) para que um --out errado
# nunca apague algo serio. Guarda best-effort, defesa em profundidade.
#
# A deteccao de absoluto usa o valor CRU (antes de tirar a barra final): se
# tirassemos a barra primeiro, "C:/" viraria "C:" e escaparia como relativo.
case "$OUT_DIR" in
  /*|[A-Za-z]:*) OUT_ABS="$OUT_DIR" ;;
  *) OUT_ABS="${PWD%/}/$OUT_DIR" ;;
esac
OUT_ABS="${OUT_ABS//\\//}"   # normaliza barras invertidas do Windows
OUT_NORM="${OUT_ABS%/}"      # "C:/" -> "C:"  ;  "/" -> ""
guard_home=""; [ -n "${HOME:-}" ] && guard_home="${HOME%/}"
guard_up=""
if [ -n "${USERPROFILE:-}" ]; then up="${USERPROFILE//\\//}"; guard_up="${up%/}"; fi
case "$OUT_NORM" in
  ""|.|..|/|//) die "--out aponta para um caminho perigoso: '$OUT_DIR'" ;;
  [A-Za-z]:) die "--out e a raiz de um drive: '$OUT_DIR'" ;;
  /bin|/sbin|/lib|/lib64|/usr|/usr/*|/etc|/etc/*|/opt|/var|/boot|/dev|/proc|/sys|/root) \
      die "--out e um diretorio de sistema: '$OUT_DIR'" ;;
  /home|/Users|/mnt|/c|/d|/e|[A-Za-z]:/Windows|[A-Za-z]:/Users|[A-Za-z]:/Program*) \
      die "--out e uma raiz de usuarios/sistema: '$OUT_DIR'" ;;
esac
if [ -n "$guard_home" ] && [ "$OUT_NORM" = "$guard_home" ]; then
  die "--out e o seu HOME: '$OUT_DIR'"
fi
if [ -n "$guard_up" ] && [ "$OUT_NORM" = "$guard_up" ]; then
  die "--out e o seu perfil de usuario: '$OUT_DIR'"
fi
# so agora (apos as guardas) normaliza o OUT_DIR usado adiante
OUT_DIR="${OUT_DIR%/}"

if [ -z "$NODE_VERSION" ]; then
  [ -f "$DEFAULT_VERSION_FILE" ] || die "node-version.txt nao encontrado em $DEFAULT_VERSION_FILE"
  NODE_VERSION="$(tr -d ' \t\r\n' < "$DEFAULT_VERSION_FILE")"
fi
[ -n "$NODE_VERSION" ] || die "versao do Node vazia"

# Mapeia a plataforma -> tag do artefato oficial + extensao + layout de saida.
case "$PLATFORM" in
  win-x64)
    NODE_TAG="win-x64"; ARCHIVE_EXT="zip"
    OUT_NODE_REL="node/node.exe"; TOOL_NODE_REL="node.exe"
    TOOL_NPM_REL="node_modules/npm/bin/npm-cli.js" ;;
  linux-x64)
    NODE_TAG="linux-x64"; ARCHIVE_EXT="tar.xz"
    OUT_NODE_REL="node/bin/node"; TOOL_NODE_REL="bin/node"
    TOOL_NPM_REL="lib/node_modules/npm/bin/npm-cli.js" ;;
  darwin-arm64)
    NODE_TAG="darwin-arm64"; ARCHIVE_EXT="tar.gz"
    OUT_NODE_REL="node/bin/node"; TOOL_NODE_REL="bin/node"
    TOOL_NPM_REL="lib/node_modules/npm/bin/npm-cli.js" ;;
  *) die "plataforma invalida: $PLATFORM (use win-x64 | linux-x64 | darwin-arm64)" ;;
esac

ARCHIVE_DIRNAME="node-v${NODE_VERSION}-${NODE_TAG}"
ARCHIVE_NAME="${ARCHIVE_DIRNAME}.${ARCHIVE_EXT}"
DIST_DIR_URL="${DIST_BASE_URL}/v${NODE_VERSION}"

[ -d "$KEYS_DIR" ] || die "keys-dir nao existe: $KEYS_DIR"
[ -f "${KEYS_DIR}/FINGERPRINTS.txt" ] || die "allowlist ausente: ${KEYS_DIR}/FINGERPRINTS.txt"
[ -d "$BRIDGE_DIR" ] || die "bridge-dir nao existe: $BRIDGE_DIR"
[ -f "${BRIDGE_DIR}/package.json" ] || die "ponte sem package.json em $BRIDGE_DIR"
[ -f "${BRIDGE_DIR}/package-lock.json" ] || die "ponte sem package-lock.json em $BRIDGE_DIR"

command -v curl >/dev/null 2>&1 || die "curl nao encontrado"
command -v gpg  >/dev/null 2>&1 || die "gpg nao encontrado"

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | cut -d' ' -f1
  else
    die "nem sha256sum nem shasum disponiveis"
  fi
}

# Diretorios de trabalho isolados (os downloads sao dados nao confiaveis: ficam
# no proprio diretorio, longe de scripts).
DL_DIR="$(mktemp -d)"
TOOLCHAIN_DIR="$(mktemp -d)"
GNUPGHOME_DIR="$(mktemp -d)"
NPM_CACHE_DIR="$(mktemp -d)"
cleanup() { rm -rf "$DL_DIR" "$TOOLCHAIN_DIR" "$GNUPGHOME_DIR" "$NPM_CACHE_DIR" 2>/dev/null || true; }
trap cleanup EXIT
chmod 700 "$GNUPGHOME_DIR" 2>/dev/null || true

info "Node v${NODE_VERSION}  plataforma=${PLATFORM}  artefato=${ARCHIVE_NAME}"
info "origem: ${DIST_DIR_URL}"

# --- (a) download dos artefatos oficiais ------------------------------------
# Em producao a origem e sempre https (sem downgrade). Um mirror local file://
# (usado pelo teste negativo) relaxa so o esquema, nunca em producao.
case "$DIST_BASE_URL" in
  file://*) CURL_PROTO=(--proto '=file') ;;
  https://*) CURL_PROTO=(--proto '=https' --tlsv1.2) ;;
  *) die "--dist-base-url precisa ser https:// (ou file:// para teste): $DIST_BASE_URL" ;;
esac
fetch() {
  # $1 = nome do arquivo remoto; baixa para $DL_DIR/$1
  curl -fsSL --retry 3 --retry-delay 2 "${CURL_PROTO[@]}" \
    "${DIST_DIR_URL}/$1" -o "${DL_DIR}/$1"
}
info "(a) baixando artefato, SHASUMS256.txt e SHASUMS256.txt.sig"
fetch "$ARCHIVE_NAME"      || die "falha ao baixar $ARCHIVE_NAME"
fetch "SHASUMS256.txt"     || die "falha ao baixar SHASUMS256.txt"
fetch "SHASUMS256.txt.sig" || die "falha ao baixar SHASUMS256.txt.sig"

# --- (b) procedencia: assinatura por uma chave do release team --------------
info "(b) verificando procedencia (GPG) com keyring isolado"
ALLOWLIST_FILE="${KEYS_DIR}/FINGERPRINTS.txt"
# Normaliza a allowlist: primeiro token de 40 hex de cada linha nao-comentario.
ALLOWLIST="$(grep -vE '^[[:space:]]*#' "$ALLOWLIST_FILE" \
  | tr '[:lower:]' '[:upper:]' \
  | grep -oE '[0-9A-F]{40}' || true)"
[ -n "$ALLOWLIST" ] || die "FINGERPRINTS.txt nao tem nenhum fingerprint valido"

# Importa SO as chaves allowlistadas (keys/<FPR>.asc) — casa com o que o
# cabecalho de FINGERPRINTS.txt promete: um .asc nao allowlistado no diretorio
# nunca entra no keyring.
imported=0
while IFS= read -r FPR; do
  [ -n "$FPR" ] || continue
  asc="${KEYS_DIR}/${FPR}.asc"
  if [ ! -f "$asc" ]; then
    echo "    aviso: fingerprint ${FPR} na allowlist sem keys/${FPR}.asc (ignorado)" >&2
    continue
  fi
  if gpg --homedir "$GNUPGHOME_DIR" --batch --quiet --import "$asc" >/dev/null 2>&1; then
    imported=$((imported+1))
  fi
done <<< "$ALLOWLIST"
[ "$imported" -gt 0 ] || die "nenhuma chave allowlistada importada de $KEYS_DIR"
info "    chaves allowlistadas importadas no keyring isolado: $imported"

STATUS_FILE="${DL_DIR}/gpg-status.txt"
set +e
gpg --homedir "$GNUPGHOME_DIR" --batch --status-fd=1 \
  --verify "${DL_DIR}/SHASUMS256.txt.sig" "${DL_DIR}/SHASUMS256.txt" \
  >"$STATUS_FILE" 2>/dev/null
gpg_rc=$?
set -e
# BADSIG/ERRSIG fazem o gpg sair != 0 (pega SHASUMS adulterado).
[ "$gpg_rc" -eq 0 ] || { sed 's/^/    gpg: /' "$STATUS_FILE" >&2 2>/dev/null || true; die "gpg --verify falhou (rc=$gpg_rc)"; }

# Decisao autoritativa e testavel: exige GOODSIG, recusa
# EXPKEYSIG/REVKEYSIG/EXPSIG/BADSIG/ERRSIG/NO_PUBKEY (chave expirada/revogada
# emite VALIDSIG e sai 0!), e so entao confere o assinante contra a allowlist.
if ! SIGNER_FPR="$(bash "${DEFAULT_LIB_DIR}/check-gpg-status.sh" "$STATUS_FILE" "$ALLOWLIST_FILE" 2>"${DL_DIR}/gpg-check.err")"; then
  cat "${DL_DIR}/gpg-check.err" >&2 2>/dev/null || true
  die "procedencia recusada pela verificacao de status do gpg"
fi
info "    assinatura BOA pela chave primaria $SIGNER_FPR (na allowlist)"

# --- (c) integridade: sha256 do artefato ------------------------------------
info "(c) verificando integridade (SHA-256)"
# Match EXATO do nome do arquivo (campo 2), nao regex — evita ponto como curinga
# e garante que node-v..-linux-x64 nunca case com node-v..-linux-x64.tar.xz.
EXPECTED_SHA="$(awk -v f="$ARCHIVE_NAME" '$2==f {print $1}' "${DL_DIR}/SHASUMS256.txt" | head -1 || true)"
[ -n "$EXPECTED_SHA" ] || die "SHASUMS256.txt nao tem linha para ${ARCHIVE_NAME}"
ACTUAL_SHA="$(sha256_of "${DL_DIR}/${ARCHIVE_NAME}")"
[ "$EXPECTED_SHA" = "$ACTUAL_SHA" ] \
  || die "sha256 nao bate para ${ARCHIVE_NAME}: esperado=$EXPECTED_SHA obtido=$ACTUAL_SHA"
info "    sha256 ok: $ACTUAL_SHA"

# --- (d) extracao do runtime ------------------------------------------------
info "(d) extraindo o runtime"
case "$ARCHIVE_EXT" in
  zip)
    command -v unzip >/dev/null 2>&1 || die "unzip nao encontrado (necessario para .zip)"
    unzip -q "${DL_DIR}/${ARCHIVE_NAME}" -d "$TOOLCHAIN_DIR" ;;
  tar.xz)
    command -v xz >/dev/null 2>&1 || die "xz nao encontrado (necessario para .tar.xz)"
    tar -xJf "${DL_DIR}/${ARCHIVE_NAME}" -C "$TOOLCHAIN_DIR" ;;
  tar.gz)
    tar -xzf "${DL_DIR}/${ARCHIVE_NAME}" -C "$TOOLCHAIN_DIR" ;;
  *) die "extensao inesperada: $ARCHIVE_EXT" ;;
esac

EXTRACTED="${TOOLCHAIN_DIR}/${ARCHIVE_DIRNAME}"
[ -d "$EXTRACTED" ] || die "diretorio extraido ausente: $EXTRACTED"
TOOL_NODE="${EXTRACTED}/${TOOL_NODE_REL}"
TOOL_NPM="${EXTRACTED}/${TOOL_NPM_REL}"
TOOL_LICENSE="${EXTRACTED}/LICENSE"
[ -f "$TOOL_NODE" ] || die "binario node nao encontrado em $TOOL_NODE"
[ -f "$TOOL_NPM" ]  || die "npm-cli.js nao encontrado em $TOOL_NPM"
[ -f "$TOOL_LICENSE" ] || die "LICENSE do Node nao encontrado em $TOOL_LICENSE"

# Monta so o necessario no bundle (binario node + LICENSE). npm/npx/corepack NAO
# vao para o bundle — a ponte nao precisa de npm em runtime (so `node bridge.mjs`).
rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR/$(dirname "$OUT_NODE_REL")"
cp "$TOOL_NODE" "${OUT_DIR}/${OUT_NODE_REL}"
chmod +x "${OUT_DIR}/${OUT_NODE_REL}" 2>/dev/null || true
cp "$TOOL_LICENSE" "${OUT_DIR}/node/LICENSE"
info "    runtime em ${OUT_DIR}/${OUT_NODE_REL}"

# --- (e) monta a ponte e roda npm ci com o Node/npm do tarball --------------
info "(e) montando a ponte e rodando npm ci --omit=dev"
OUT_BRIDGE="${OUT_DIR}/bridge-whatsapp"
mkdir -p "$OUT_BRIDGE"
# copia tudo exceto node_modules (a origem e gitignored e nao tem, mas garantimos)
cp -R "${BRIDGE_DIR}/." "$OUT_BRIDGE/"
rm -rf "${OUT_BRIDGE}/node_modules"

# --ignore-scripts: as deps da ponte nao precisam de install-scripts em runtime
# (sharp 0.34 usa binarios pre-compilados @img/sharp-* como deps opcionais, nao
# compila libvips no install), entao nenhum script arbitrario de dependencia
# roda no build do bundle — endurecimento de supply chain.
( cd "$OUT_BRIDGE" && "$TOOL_NODE" "$TOOL_NPM" ci --omit=dev --ignore-scripts \
    --no-audit --no-fund --cache "$NPM_CACHE_DIR" --loglevel=warn ) \
  || die "npm ci falhou"

[ -d "${OUT_BRIDGE}/node_modules" ] || die "node_modules nao foi criado"
[ -f "${OUT_BRIDGE}/node_modules/.package-lock.json" ] \
  || die "node_modules/.package-lock.json (lock oculto) ausente — npm ci nao concluiu"
info "    node_modules + .package-lock.json ok"

# --- (f) THIRD_PARTY_NOTICES.txt --------------------------------------------
info "(f) gerando THIRD_PARTY_NOTICES.txt"
"$TOOL_NODE" "${DEFAULT_LIB_DIR}/gen-third-party-notices.mjs" \
  --node-modules "${OUT_BRIDGE}/node_modules" \
  --node-license "${OUT_DIR}/node/LICENSE" \
  --node-version "$NODE_VERSION" \
  --out "${OUT_DIR}/THIRD_PARTY_NOTICES.txt" \
  || die "geracao do THIRD_PARTY_NOTICES.txt falhou"

# --- (g) manifest.json ------------------------------------------------------
info "(g) gravando manifest.json"
"$TOOL_NODE" "${DEFAULT_LIB_DIR}/gen-manifest.mjs" \
  --out "${OUT_DIR}/manifest.json" \
  --platform "$PLATFORM" \
  --node-version "$NODE_VERSION" \
  --node-archive "$ARCHIVE_NAME" \
  --node-archive-sha256 "$ACTUAL_SHA" \
  --signer-fingerprint "$SIGNER_FPR" \
  --bridge-lock "${OUT_BRIDGE}/package-lock.json" \
  || die "geracao do manifest.json falhou"

info "BUNDLE PRONTO em: $OUT_DIR"
echo "---- conteudo de topo ----"
ls -la "$OUT_DIR"
echo "---- manifest.json ----"
cat "${OUT_DIR}/manifest.json"
