#!/usr/bin/env bash
# GarraDream — Consolidação automática de memória do GarraIA
# Baseado no Auto Dream do ClaudeMaxPower, adaptado para GarraRUST
#
# Gatilho: 24h desde última consolidação + 5 sessões
# Fases: inventário → staleness → duplicatas → rebuild index → update state
# Segurança: read-only no código do projeto, write-only em arquivos de memória

set -euo pipefail

MEMORY_DIR="$HOME/.claude/projects/g--Projetos-GarraRUST/memory"
STATE_FILE="$MEMORY_DIR/.dream-state.json"
LOCK_FILE="$MEMORY_DIR/.dream.lock"
MEMORY_INDEX="$MEMORY_DIR/MEMORY.md"

# Configuração via env vars (com defaults)
DREAM_INTERVAL_HOURS="${GARRA_DREAM_INTERVAL_HOURS:-24}"
DREAM_MIN_SESSIONS="${GARRA_DREAM_MIN_SESSIONS:-5}"
STALE_DAYS="${GARRA_DREAM_STALE_DAYS:-30}"

# ── Lock: evitar execução concorrente ──────────────────────────────────────
if [ -f "$LOCK_FILE" ]; then
  LOCK_AGE=$(( $(date +%s) - $(date -r "$LOCK_FILE" +%s 2>/dev/null || echo 0) ))
  if [ "$LOCK_AGE" -lt 3600 ]; then
    echo "GarraDream: lock ativo (age: ${LOCK_AGE}s), saindo."
    exit 0
  fi
  # Lock stale (>1h), remover
  rm -f "$LOCK_FILE"
fi

trap 'rm -f "$LOCK_FILE"' EXIT
echo "$$" > "$LOCK_FILE"

# ── Verificar condições de gatilho ─────────────────────────────────────────
if [ -f "$STATE_FILE" ]; then
  LAST_RUN=$(python3 -c "import json; print(json.load(open('$STATE_FILE')).get('last_run', 0))" 2>/dev/null || echo 0)
  SESSION_COUNT=$(python3 -c "import json; print(json.load(open('$STATE_FILE')).get('session_count', 0))" 2>/dev/null || echo 0)
else
  LAST_RUN=0
  SESSION_COUNT=0
fi

# Incrementar contador de sessões
SESSION_COUNT=$((SESSION_COUNT + 1))

NOW=$(date +%s)
HOURS_SINCE=$((($NOW - $LAST_RUN) / 3600))

if [ "$HOURS_SINCE" -lt "$DREAM_INTERVAL_HOURS" ] || [ "$SESSION_COUNT" -lt "$DREAM_MIN_SESSIONS" ]; then
  # Salvar session count atualizado e sair
  mkdir -p "$MEMORY_DIR"
  cat > "$STATE_FILE" <<EOF
{
  "last_run": $LAST_RUN,
  "session_count": $SESSION_COUNT,
  "last_check": $NOW
}
EOF
  echo "GarraDream: condições não atingidas (${HOURS_SINCE}h/${DREAM_INTERVAL_HOURS}h, ${SESSION_COUNT}/${DREAM_MIN_SESSIONS} sessões)"
  exit 0
fi

echo "GarraDream: iniciando consolidação de memória..."

# ── Fase 1: Inventário ────────────────────────────────────────────────────
MEMORY_FILES=()
while IFS= read -r f; do
  MEMORY_FILES+=("$f")
done < <(find "$MEMORY_DIR" -name "*.md" -not -name "MEMORY.md" -not -name "cycles.md" 2>/dev/null)

TOTAL_FILES=${#MEMORY_FILES[@]}
echo "GarraDream: $TOTAL_FILES arquivos de memória encontrados"

if [ "$TOTAL_FILES" -eq 0 ]; then
  echo "GarraDream: nenhum arquivo de memória para consolidar"
  cat > "$STATE_FILE" <<EOF
{
  "last_run": $NOW,
  "session_count": 0,
  "last_check": $NOW,
  "total_files": 0
}
EOF
  exit 0
fi

# ── Fase 2: Detectar memórias stale (>30 dias) ────────────────────────────
STALE_FILES=()
for f in "${MEMORY_FILES[@]}"; do
  if [ -f "$f" ]; then
    FILE_AGE=$(( ($NOW - $(date -r "$f" +%s 2>/dev/null || echo $NOW)) / 86400 ))
    if [ "$FILE_AGE" -gt "$STALE_DAYS" ]; then
      STALE_FILES+=("$(basename "$f") (${FILE_AGE} dias)")
    fi
  fi
done

if [ ${#STALE_FILES[@]} -gt 0 ]; then
  echo "GarraDream: ${#STALE_FILES[@]} memórias possivelmente stale:"
  for s in "${STALE_FILES[@]}"; do
    echo "  - $s"
  done
fi

# ── Fase 3: Detectar duplicatas (nomes similares) ─────────────────────────
NAMES=()
for f in "${MEMORY_FILES[@]}"; do
  NAMES+=("$(basename "$f" .md)")
done

DUPES=()
for i in "${!NAMES[@]}"; do
  for j in "${!NAMES[@]}"; do
    if [ "$i" -lt "$j" ] && [ "${NAMES[$i]}" = "${NAMES[$j]}" ]; then
      DUPES+=("${NAMES[$i]}")
    fi
  done
done

if [ ${#DUPES[@]} -gt 0 ]; then
  echo "GarraDream: ${#DUPES[@]} possíveis duplicatas:"
  for d in "${DUPES[@]}"; do
    echo "  - $d"
  done
fi

# ── Fase 4: Rebuild MEMORY.md index ───────────────────────────────────────
echo "GarraDream: reconstruindo índice MEMORY.md..."

# Categorizar por tipo (lendo frontmatter)
declare -A TYPE_FILES
for f in "${MEMORY_FILES[@]}"; do
  TYPE=$(grep -m1 '^type:' "$f" 2>/dev/null | sed 's/type: *//' | tr -d '[:space:]' || echo "unknown")
  NAME=$(grep -m1 '^name:' "$f" 2>/dev/null | sed 's/name: *//' || echo "$(basename "$f" .md)")
  DESC=$(grep -m1 '^description:' "$f" 2>/dev/null | sed 's/description: *//' || echo "")
  BASENAME=$(basename "$f")

  LINE="- [$NAME]($BASENAME) — $DESC"
  TYPE_FILES["$TYPE"]+="$LINE
"
done

# Escrever MEMORY.md organizado por tipo
{
  echo "# GarraRUST Project Memory"
  echo ""

  for TYPE in user feedback project reference unknown; do
    if [ -n "${TYPE_FILES[$TYPE]:-}" ]; then
      case "$TYPE" in
        user) echo "## User" ;;
        feedback) echo "## Feedback" ;;
        project) echo "## Project" ;;
        reference) echo "## Reference" ;;
        unknown) echo "## Other" ;;
      esac
      echo "${TYPE_FILES[$TYPE]}"
    fi
  done
} > "$MEMORY_INDEX"

echo "GarraDream: MEMORY.md reconstruído com sucesso"

# ── Fase 5: Atualizar state ──────────────────────────────────────────────
cat > "$STATE_FILE" <<EOF
{
  "last_run": $NOW,
  "session_count": 0,
  "last_check": $NOW,
  "total_files": $TOTAL_FILES,
  "stale_files": ${#STALE_FILES[@]},
  "duplicates": ${#DUPES[@]}
}
EOF

echo "GarraDream: consolidação concluída. $TOTAL_FILES memórias, ${#STALE_FILES[@]} stale, ${#DUPES[@]} duplicatas."
exit 0
