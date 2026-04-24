#!/usr/bin/env bash
# GarraIA SuperPowers — stop hook
# Persiste estado em .garra-estado.md e salva sessão com cleanup

set -euo pipefail

# Resolve project root so all relative paths below work regardless of the CWD
# Claude Code used to invoke this hook (sessions started from a worktree or
# subdir used to fail with "No such file or directory" — see GAR-445).
cd "${CLAUDE_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"

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
  # Strip any accumulated "# Estado GarraIA" header(s) from $EXISTING — keep
  # only the content starting at the first "## <timestamp>" entry, and drop
  # header lines that may have accumulated between entries. Old hook versions
  # prepended a fresh header every session; this idempotent awk keeps exactly
  # one header at the top of the file forever.
  EXISTING=$(awk '/^## / { capture = 1 } capture && !/^# Estado GarraIA$/' "$ESTADO_FILE")
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

# ── 4. Token-stack observability (plan foamy-origami Lote 0.2) ───────────
# Grava observação no .acts/store.db via MCP server local (fail-soft — se
# Node/acts não estiver disponível, silenciosamente ignora).
if command -v node &>/dev/null; then
  node ".claude/hooks/acts-log.js" "$BRANCH" "$TIMESTAMP" "$SUMMARY" 2>/dev/null || true
fi

exit 0
