#!/usr/bin/env bash
# GarraIA SuperPowers — Batch Fix Issues
# Processa múltiplos issues do GitHub sequencialmente via claude --print
#
# Uso: ./workflows/batch-fix.sh owner/repo 10 11 12
# Ou:  ./workflows/batch-fix.sh michelbr84/GarraRUST 10 11 12

set -euo pipefail

REPO="${1:?Uso: $0 <owner/repo> <issue1> [issue2] [issue3] ...}"
shift
ISSUES=("$@")

if [ ${#ISSUES[@]} -eq 0 ]; then
  echo "Erro: forneça ao menos um número de issue"
  echo "Uso: $0 <owner/repo> <issue1> [issue2] [issue3] ..."
  exit 1
fi

RESULTS_FILE="batch-results.json"
echo "[]" > "$RESULTS_FILE"

GREEN='\033[0;32m'
RED='\033[0;31m'
CYAN='\033[0;36m'
NC='\033[0m'

TOTAL=${#ISSUES[@]}
PASSED=0
FAILED=0

echo -e "${CYAN}GarraIA Batch Fix — processando $TOTAL issues em $REPO${NC}"
echo ""

for i in "${!ISSUES[@]}"; do
  ISSUE="${ISSUES[$i]}"
  IDX=$((i + 1))
  START=$(date +%s)

  echo -e "${CYAN}[$IDX/$TOTAL] Issue #$ISSUE${NC}"

  if claude --print "/fix-issue --issue $ISSUE --repo $REPO" 2>&1; then
    STATUS="success"
    PASSED=$((PASSED + 1))
    echo -e "${GREEN}  ✓ Issue #$ISSUE corrigido${NC}"
  else
    STATUS="failure"
    FAILED=$((FAILED + 1))
    echo -e "${RED}  ✗ Issue #$ISSUE falhou${NC}"
  fi

  END=$(date +%s)
  DURATION=$((END - START))

  # Append result to JSON
  python3 -c "
import json
with open('$RESULTS_FILE', 'r') as f:
    results = json.load(f)
results.append({
    'issue': $ISSUE,
    'repo': '$REPO',
    'status': '$STATUS',
    'duration_seconds': $DURATION
})
with open('$RESULTS_FILE', 'w') as f:
    json.dump(results, f, indent=2)
" 2>/dev/null || echo "  (aviso: não foi possível salvar resultado em JSON)"

  echo ""
done

echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "Resultados: ${GREEN}$PASSED passed${NC} / ${RED}$FAILED failed${NC} / $TOTAL total"
echo -e "Detalhes salvos em: $RESULTS_FILE"

[ "$FAILED" -gt 0 ] && exit 1
exit 0
