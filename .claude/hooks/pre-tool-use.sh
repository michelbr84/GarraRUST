#!/usr/bin/env bash
# GarraIA SuperPowers — pre-tool-use hook
# Bloqueia comandos perigosos, detecta segredos e registra audit log

# Resolve project root so AUDIT_LOG resolves regardless of CWD (GAR-445).
cd "${CLAUDE_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"

CMD="${CLAUDE_TOOL_INPUT_COMMAND:-}"
AUDIT_LOG=".claude/audit.log"

log() {
  echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*" >> "$AUDIT_LOG"
}

[ -n "$CMD" ] && log "CMD: $CMD"

# ── Padroes bloqueados (exit 2 = bloquear) ────────────────────────────────
BLOCKED=(
  "rm -rf /"
  "rm -rf ~"
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
    exit 2
  fi
done

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
    exit 2
  fi
done

exit 0
