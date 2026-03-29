#!/usr/bin/env bash
# GarraIA SuperPowers — stop hook
# Persiste estado em .garra-estado.md e salva sessão com cleanup

set -euo pipefail

ESTADO_FILE=".garra-estado.md"
SESSIONS_DIR=".claude/sessions"
MAX_SESSIONS=10

mkdir -p "$SESSIONS_DIR"

TIMESTAMP=$(date '+%Y-%m-%d_%H-%M-%S')
SUMMARY="${CLAUDE_STOP_HOOK_SUMMARY:-Sessao encerrada sem resumo.}"
BRANCH=$(git -C . branch --show-current 2>/dev/null || echo "N/A")
LOG=$(git -C . log --oneline -5 2>/dev/null || echo "N/A")

# ── 1. Atualizar .garra-estado.md (mais recente primeiro) ─────────────────
ESTADO_ENTRY="## $TIMESTAMP

**Branch:** $BRANCH

### Resumo
$SUMMARY

### Commits recentes
$LOG

---
"

if [ -f "$ESTADO_FILE" ]; then
  EXISTING=$(cat "$ESTADO_FILE")
  echo -e "# Estado GarraIA\n\n$ESTADO_ENTRY\n$EXISTING" > "$ESTADO_FILE"
else
  echo -e "# Estado GarraIA\n\n$ESTADO_ENTRY" > "$ESTADO_FILE"
fi

# Stage .garra-estado.md (não commita)
git add "$ESTADO_FILE" 2>/dev/null || true

# ── 2. Salvar sessão ──────────────────────────────────────────────────────
SESSION_FILE="$SESSIONS_DIR/session-$TIMESTAMP.md"
cat > "$SESSION_FILE" <<EOF
# Sessao GarraIA — $TIMESTAMP

## Resumo
$SUMMARY

## Estado do repositorio
$LOG

## Branch
$BRANCH
EOF

# ── 3. Cleanup: manter apenas as últimas N sessões ───────────────────────
SESSION_COUNT=$(find "$SESSIONS_DIR" -name "session-*.md" 2>/dev/null | wc -l)
if [ "$SESSION_COUNT" -gt "$MAX_SESSIONS" ]; then
  EXCESS=$((SESSION_COUNT - MAX_SESSIONS))
  find "$SESSIONS_DIR" -name "session-*.md" -print0 2>/dev/null | \
    sort -z | head -z -n "$EXCESS" | xargs -0 rm -f 2>/dev/null || true
fi

echo "Sessao salva em $SESSION_FILE"
exit 0
