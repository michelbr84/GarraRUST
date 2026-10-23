#!/usr/bin/env bash
#
# prove-standalone.sh — prova que o bundle roda SOZINHO (fatia B, issue #1636)
#
# Esconde qualquer Node global do runner (filtra o PATH) e, sem NODE_PATH, prova
# que o Node PRIVADO do bundle:
#   - e exatamente a versao fixada;
#   - carrega TODAS as deps nativas da ponte (baileys + pino + sharp);
#   - roda o handshake da ponte (bridge.mjs --protocol-check), sem tocar a rede.
#
#   prove-standalone.sh --out <dir do bundle> --expect-version <X.Y.Z>
#
# Roda em Windows (Git Bash), Linux e macOS com `shell: bash`.

set -euo pipefail

OUT=""
EXPECT=""
while [ $# -gt 0 ]; do
  case "$1" in
    --out) OUT="${2:-}"; shift 2 ;;
    --expect-version) EXPECT="${2:-}"; shift 2 ;;
    *) echo "argumento desconhecido: $1" >&2; exit 2 ;;
  esac
done
[ -n "$OUT" ] && [ -n "$EXPECT" ] || { echo "uso: --out <dir> --expect-version <X.Y.Z>" >&2; exit 2; }

# Resolve OUT para caminho ABSOLUTO: mais adiante entramos em bridge-whatsapp com
# `cd`, e um OUT relativo deixaria de apontar para o node do bundle.
[ -d "$OUT" ] || { echo "ERRO: diretorio do bundle nao existe: $OUT" >&2; exit 1; }
OUT="$(cd "$OUT" && pwd)"

if [ -x "${OUT}/node/node.exe" ]; then
  NODE="${OUT}/node/node.exe"
elif [ -x "${OUT}/node/bin/node" ]; then
  NODE="${OUT}/node/bin/node"
else
  echo "ERRO: binario node do bundle nao encontrado em ${OUT}/node" >&2
  exit 1
fi

# Esconde todo Node global: remove do PATH qualquer diretorio com node/node.exe.
NEWPATH=""
IFS=':' read -ra PARTS <<< "${PATH}"
for d in "${PARTS[@]}"; do
  [ -z "$d" ] && continue
  if [ -x "${d}/node" ] || [ -x "${d}/node.exe" ]; then
    echo "   PATH: removendo diretorio com node global -> ${d}"
    continue
  fi
  NEWPATH="${NEWPATH:+${NEWPATH}:}${d}"
done
export PATH="$NEWPATH"
unset NODE_PATH || true

echo "=== nenhum node global deve sobrar no PATH ==="
if command -v node >/dev/null 2>&1; then
  echo "ERRO: ainda ha node global acessivel: $(command -v node)" >&2
  exit 1
fi
echo "   ok: 'node' nao resolve no PATH sanitizado"

echo "=== versao do node privado ==="
GOT="$("$NODE" --version)"
echo "   ${GOT}"
if [ "$GOT" != "v${EXPECT}" ]; then
  echo "ERRO: versao do node privado (${GOT}) != esperada (v${EXPECT})" >&2
  exit 1
fi

echo "=== carregando TODAS as deps da ponte com o node privado ==="
cd "${OUT}/bridge-whatsapp"
# TODO(#1636): quando a fatia A mergear, trocar este -e por `"$NODE" bridge.mjs --self-test`.
"$NODE" -e "Promise.all([import('@whiskeysockets/baileys'),import('pino'),import('sharp')]).then(()=>console.log('   DEPS OK: baileys+pino+sharp carregados pelo node privado')).catch((e)=>{console.error('FALHOU ao carregar deps:',e);process.exit(1)})"

echo "=== handshake da ponte (bridge.mjs --protocol-check, sem rede) ==="
"$NODE" bridge.mjs --protocol-check > protocol-check.out
head -1 protocol-check.out
rm -f protocol-check.out

echo "STANDALONE OK: node privado v${EXPECT} roda a ponte sem nenhum Node global."
