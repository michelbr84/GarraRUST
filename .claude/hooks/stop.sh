#!/usr/bin/env bash
# GarraRUST — stop hook
# Persiste resumo da sessão em .claude/sessions/

SESSIONS_DIR=".claude/sessions"
mkdir -p "$SESSIONS_DIR"

TIMESTAMP=$(date '+%Y-%m-%d_%H-%M-%S')
SESSION_FILE="$SESSIONS_DIR/session-$TIMESTAMP.md"

SUMMARY="${CLAUDE_STOP_HOOK_SUMMARY:-Sessão encerrada sem resumo.}"

cat > "$SESSION_FILE" <<EOF
# Sessão GarraRUST — $TIMESTAMP

## Resumo
$SUMMARY

## Estado do repositório
$(git -C . log --oneline -5 2>/dev/null || echo "N/A")

## Branch
$(git -C . branch --show-current 2>/dev/null || echo "N/A")
EOF

echo "→ Sessão salva em $SESSION_FILE"
exit 0
