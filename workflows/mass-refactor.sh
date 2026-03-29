#!/usr/bin/env bash
# GarraIA SuperPowers — Mass Refactor
# Aplica refactoring via Claude em todos os arquivos que contêm um padrão
#
# Uso: ./workflows/mass-refactor.sh --pattern "old_name" --goal "rename to new_name"

set -euo pipefail

PATTERN=""
GOAL=""
MAX_FILES=50

while [[ $# -gt 0 ]]; do
  case $1 in
    --pattern) PATTERN="$2"; shift 2 ;;
    --goal) GOAL="$2"; shift 2 ;;
    --max-files) MAX_FILES="$2"; shift 2 ;;
    *) echo "Opção desconhecida: $1"; exit 1 ;;
  esac
done

if [ -z "$PATTERN" ] || [ -z "$GOAL" ]; then
  echo "Uso: $0 --pattern <padrão> --goal <objetivo>"
  exit 1
fi

CYAN='\033[0;36m'
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${CYAN}GarraIA Mass Refactor${NC}"
echo -e "Padrão: ${YELLOW}$PATTERN${NC}"
echo -e "Objetivo: ${YELLOW}$GOAL${NC}"
echo ""

# Encontrar arquivos Rust/Dart/TS que contêm o padrão
FILES=$(grep -rl "$PATTERN" --include="*.rs" --include="*.dart" --include="*.ts" --include="*.js" . 2>/dev/null | head -n "$MAX_FILES")

if [ -z "$FILES" ]; then
  echo "Nenhum arquivo encontrado com o padrão '$PATTERN'"
  exit 0
fi

FILE_COUNT=$(echo "$FILES" | wc -l)
echo -e "Encontrados: ${CYAN}$FILE_COUNT${NC} arquivos"

if [ "$FILE_COUNT" -gt 20 ]; then
  echo -e "${YELLOW}AVISO: $FILE_COUNT arquivos serão modificados. Continuar? (CTRL+C para cancelar)${NC}"
  echo "Continuando em 5 segundos..."
  sleep 5
fi

PASSED=0
FAILED=0

for FILE in $FILES; do
  echo -e "${CYAN}→ $FILE${NC}"

  if claude --print "No arquivo $FILE, aplique o seguinte refactoring: $GOAL. Padrão a buscar: '$PATTERN'. Faça edições mínimas e mantenha a funcionalidade." 2>&1; then
    PASSED=$((PASSED + 1))
    echo -e "${GREEN}  ✓ Refactored${NC}"
  else
    FAILED=$((FAILED + 1))
    echo -e "${RED}  ✗ Failed${NC}"
  fi
done

echo ""
echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "Resultados: ${GREEN}$PASSED passed${NC} / ${RED}$FAILED failed${NC} / $FILE_COUNT total"
echo ""
echo "Git diff summary:"
git diff --stat
