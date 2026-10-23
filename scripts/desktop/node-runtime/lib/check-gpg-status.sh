#!/usr/bin/env bash
#
# check-gpg-status.sh — decisao de PROCEDENCIA a partir do status do GnuPG.
# Fatia B da issue #1636. Extraido para ser testavel sem chave privada real:
# recebe o status (--status-fd) de um arquivo e a allowlist de outro, imprime o
# fingerprint primario APROVADO em stdout (exit 0) ou morre (stderr, exit != 0).
#
#   check-gpg-status.sh <arquivo-de-status> <arquivo-de-allowlist>
#
# Por que nao basta "VALIDSIG + exit 0": o GnuPG emite VALIDSIG e SAI 0 tambem
# para chave EXPIRADA (EXPKEYSIG) e, em varias versoes, REVOGADA (REVKEYSIG).
# Entao aqui exigimos GOODSIG, recusamos qualquer token de assinatura ruim/
# expirada/revogada, e so entao conferimos o fingerprint contra a allowlist.

set -uo pipefail

STATUS_FILE="${1:-}"
ALLOWLIST_FILE="${2:-}"
[ -f "$STATUS_FILE" ]    || { echo "ERRO: arquivo de status ausente: $STATUS_FILE" >&2; exit 2; }
[ -f "$ALLOWLIST_FILE" ] || { echo "ERRO: allowlist ausente: $ALLOWLIST_FILE" >&2; exit 2; }

# 1) Recusa qualquer status de assinatura ruim / expirada / revogada / ausente.
#    (EXPKEYSIG e REVKEYSIG convivem com exit 0 — por isso sao checados aqui.)
for tok in BADSIG ERRSIG EXPSIG EXPKEYSIG REVKEYSIG NO_PUBKEY NODATA; do
  if grep -q "^\[GNUPG:\] ${tok} " "$STATUS_FILE"; then
    echo "ERRO: procedencia recusada: status do gpg contem ${tok}" >&2
    exit 1
  fi
done

# 2) Exige uma assinatura BOA de forma explicita.
grep -q "^\[GNUPG:\] GOODSIG " "$STATUS_FILE" \
  || { echo "ERRO: procedencia recusada: sem GOODSIG" >&2; exit 1; }

# 3) Exige VALIDSIG e extrai o fingerprint da chave PRIMARIA (ultima coluna).
vline="$(grep '^\[GNUPG:\] VALIDSIG ' "$STATUS_FILE" | head -1 || true)"
[ -n "$vline" ] || { echo "ERRO: procedencia recusada: sem VALIDSIG" >&2; exit 1; }
fpr="$(echo "$vline" | awk '{print $NF}' | tr '[:lower:]' '[:upper:]')"
case "$fpr" in
  *[!0-9A-F]*|"") echo "ERRO: fingerprint invalido no VALIDSIG: '$fpr'" >&2; exit 1 ;;
esac
[ "${#fpr}" -eq 40 ] || { echo "ERRO: fingerprint com tamanho inesperado: $fpr" >&2; exit 1; }

# 4) O assinante tem de estar na allowlist (primeiro token de 40 hex por linha).
allow="$(grep -vE '^[[:space:]]*#' "$ALLOWLIST_FILE" \
  | tr '[:lower:]' '[:upper:]' \
  | grep -oE '[0-9A-F]{40}' || true)"
echo "$allow" | grep -qx "$fpr" \
  || { echo "ERRO: assinante $fpr NAO esta na allowlist" >&2; exit 1; }

echo "$fpr"
exit 0
