#!/usr/bin/env bash
# GarraRUST — pre-tool-use hook
# Bloqueia comandos perigosos e registra audit log

CMD="${CLAUDE_TOOL_INPUT_COMMAND:-}"
AUDIT_LOG=".claude/audit.log"

log() {
  echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*" >> "$AUDIT_LOG"
}

[ -n "$CMD" ] && log "CMD: $CMD"

# Padrões bloqueados
BLOCKED=(
  "rm -rf /"
  "rm -rf ~"
  ":(){ :|:& };:"
  "DROP TABLE"
  "DROP DATABASE"
  "git push --force origin main"
  "git push -f origin main"
  "git reset --hard HEAD"
)

for pattern in "${BLOCKED[@]}"; do
  if echo "$CMD" | grep -qF "$pattern"; then
    echo "BLOQUEADO: comando perigoso detectado — '$pattern'" >&2
    log "BLOQUEADO: $CMD"
    exit 2
  fi
done

# Avisos (não bloqueiam, apenas registram)
WARNINGS=(
  "curl | bash"
  "curl | sh"
  "wget | bash"
)

for pattern in "${WARNINGS[@]}"; do
  if echo "$CMD" | grep -qF "$pattern"; then
    echo "AVISO: padrão arriscado detectado — '$CMD'" >&2
    log "AVISO: $CMD"
  fi
done

# Detectar exposição de segredos
SECRET_PATTERNS=(
  "GARRAIA_JWT_SECRET"
  "GarraIA_VAULT_PASSPHRASE"
  "GARRAIA_ADMIN_PASSWORD"
  "OPENAI_API_KEY"
  "ANTHROPIC_API_KEY"
)

for secret in "${SECRET_PATTERNS[@]}"; do
  if echo "$CMD" | grep -q "$secret"; then
    echo "AVISO: possível exposição de segredo — '$secret'" >&2
    log "AVISO SEGREDO: $CMD"
  fi
done

exit 0
