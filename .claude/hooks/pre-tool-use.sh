#!/usr/bin/env bash
# GarraIA SuperPowers — pre-tool-use hook
# Bloqueia comandos perigosos, detecta segredos e registra audit log.
#
# Claude Code passa a chamada de tool via JSON em STDIN, no formato:
#   { "tool_name": "Bash", "tool_input": { "command": "...", "description": "..." }, ... }
# (anteriormente: CLAUDE_TOOL_INPUT_COMMAND env var — abandonado pelo upstream).
# Mantemos um fallback ao env var legacy para compat retroativa.

set -euo pipefail

# Resolve project root so AUDIT_LOG resolves regardless of CWD (GAR-445).
cd "${CLAUDE_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"

AUDIT_LOG=".claude/audit.log"
mkdir -p "$(dirname "$AUDIT_LOG")"

# ── Resolve command from stdin JSON (canonical) or legacy env var ─────────
CMD="${CLAUDE_TOOL_INPUT_COMMAND:-}"
TOOL_NAME=""

# Read stdin if available (non-blocking via /dev/stdin probe)
if [ -t 0 ]; then
  STDIN_PAYLOAD=""
else
  STDIN_PAYLOAD="$(cat 2>/dev/null || true)"
fi

if [ -n "$STDIN_PAYLOAD" ] && command -v jq >/dev/null 2>&1; then
  PARSED_CMD=$(echo "$STDIN_PAYLOAD" | jq -r '.tool_input.command // empty' 2>/dev/null || true)
  TOOL_NAME=$(echo "$STDIN_PAYLOAD" | jq -r '.tool_name // empty' 2>/dev/null || true)
  [ -n "$PARSED_CMD" ] && CMD="$PARSED_CMD"
fi

# Nada para auditar / inspecionar → fast-path
if [ -z "$CMD" ]; then
  exit 0
fi

log() {
  echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*" >> "$AUDIT_LOG"
}

log "CMD${TOOL_NAME:+($TOOL_NAME)}: $CMD"

# ── Padroes bloqueados (exit 2 = bloquear) ────────────────────────────────
#
# Duas listas, de proposito (#1453). A primeira e casada como SUBSTRING
# literal (`grep -F`): sao frases que nao aparecem em comando legitimo. A
# segunda e uma regex ANCORADA no alvo: `rm -rf /` como substring bloqueava
# `rm -rf /tmp/qualquer-coisa` e `rm -rf ./` bloqueava `rm -rf ./pintest` —
# toda remocao por caminho absoluto ou relativo, inclusive as que uma rodada
# autonoma faz no proprio scratchpad. O perigo nunca foi o prefixo: e o alvo
# ser a raiz, o home, o diretorio atual ou o pai — sozinhos ou com glob.
BLOCKED=(
  "rm --no-preserve-root"
  ":(){ :|:& };:"
  "DROP TABLE"
  "DROP DATABASE"
  "TRUNCATE TABLE"
  "git push --force origin main"
  "git push -f origin main"
  "git push --force origin master"
  "git push -f origin master"
  "git reset --hard HEAD"
  "dd if="
  "mkfs."
  "> /dev/sd"
)

for pattern in "${BLOCKED[@]}"; do
  if echo "$CMD" | grep -qF "$pattern"; then
    echo "BLOQUEADO: comando perigoso detectado — '$pattern'" >&2
    log "BLOQUEADO: $CMD"
    # JSON response per current Claude Code hook protocol
    echo '{"hookSpecificOutput":{"permissionDecision":"deny"},"systemMessage":"Dangerous command pattern blocked by GarraIA pre-tool-use hook"}' >&2
    exit 2
  fi
done

# `rm` com alvo catastrofico. Le-se: um `rm` no inicio ou depois de `;`,
# `&`, `|` ou espaco (cobre `sudo rm`, `xargs rm`, `&& rm`); zero ou mais
# flags (`-rf`, `-r -f`, `--recursive`, `--`); e o ALVO, opcionalmente entre
# aspas, seguido de fim de linha, espaco ou separador de comando. Os alvos
# sao a raiz (`/`, `/*`), o home (`~`, `~/`, `~/*`, `$HOME`, `${HOME}`), o
# diretorio atual (`.`, `./`, `./*`, `.*`) e o pai (`..`, `../`, `../*`).
# `rm -rf /tmp/x`, `rm -rf ./pintest` e `rm -rf ~/.cache/x` NAO casam: depois
# do alvo vem outro caractere, nao um limite. Flags nao decidem nada — `rm /`
# sem `-r` ja falha sozinho, e tratar igual e mais simples do que enumerar.
RM_ALVOS='(/|/\*|~|~/|~/\*|\$HOME|\$\{HOME\}|\.|\./|\./\*|\.\*|\.\.|\.\./|\.\./\*)'
RM_FLAGS='([[:space:]]+(--|--?[[:alnum:]][[:alnum:]=-]*))*'
RM_CATASTROFICO="(^|[;&|[:space:]])rm${RM_FLAGS}[[:space:]]+[\"']?${RM_ALVOS}[\"']?([[:space:]]|\$|[;&|)])"

if echo "$CMD" | grep -qE "$RM_CATASTROFICO"; then
  echo "BLOQUEADO: comando perigoso detectado — 'rm' com alvo raiz, home, diretorio atual ou pai" >&2
  log "BLOQUEADO: $CMD"
  echo '{"hookSpecificOutput":{"permissionDecision":"deny"},"systemMessage":"Dangerous command pattern blocked by GarraIA pre-tool-use hook"}' >&2
  exit 2
fi

# ── Avisos (nao bloqueiam, apenas registram) ──────────────────────────────
WARNINGS=(
  "curl | bash"
  "curl | sh"
  "wget | bash"
  "wget | sh"
  "pip install"
  "npm install -g"
  "cargo install"
)

for pattern in "${WARNINGS[@]}"; do
  if echo "$CMD" | grep -qF "$pattern"; then
    echo "AVISO: padrao arriscado detectado — '$CMD'" >&2
    log "AVISO: $CMD"
  fi
done

# ── Detectar exposicao de segredos ────────────────────────────────────────
SECRET_PATTERNS=(
  "GARRAIA_JWT_SECRET"
  "GARRAIA_REFRESH_HMAC_SECRET"
  "GARRAIA_METRICS_TOKEN"
  "GarraIA_VAULT_PASSPHRASE"
  "GARRAIA_ADMIN_PASSWORD"
  "OPENAI_API_KEY"
  "ANTHROPIC_API_KEY"
  "GITHUB_TOKEN"
  "SENTRY_TOKEN"
  "API_KEY"
  "SECRET_KEY"
  "PRIVATE_KEY"
  "PASSWORD"
)

for secret in "${SECRET_PATTERNS[@]}"; do
  if echo "$CMD" | grep -qi "echo.*$secret\|print.*$secret\|cat.*\.env"; then
    echo "BLOQUEADO: possivel exposicao de segredo — '$secret'" >&2
    log "BLOQUEADO SEGREDO: $CMD"
    echo '{"hookSpecificOutput":{"permissionDecision":"deny"},"systemMessage":"Possible secret exposure blocked by GarraIA pre-tool-use hook"}' >&2
    exit 2
  fi
done

exit 0
