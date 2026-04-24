#!/usr/bin/env bash
# GarraIA SuperPowers — session-start hook
# Carrega contexto do projeto, estado anterior e lança GarraDream em background

set -euo pipefail

# Resolve project root so all relative paths below work regardless of CWD
# (GAR-445).
cd "${CLAUDE_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${CYAN}  🦀 GarraIA SuperPowers — Sessão iniciada${NC}"
echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

# Data e hora
echo -e "${GREEN}Data:${NC} $(date '+%Y-%m-%d %H:%M:%S')"

# Git: branch atual e últimos 5 commits
BRANCH=$(git -C . branch --show-current 2>/dev/null || echo "N/A")
echo -e "${GREEN}Branch:${NC} $BRANCH"
echo -e "${GREEN}Últimos commits:${NC}"
git -C . log --oneline -5 2>/dev/null || echo "  (sem histórico git)"

# Estado anterior da sessão (.garra-estado.md)
ESTADO_FILE=".garra-estado.md"
if [ -f "$ESTADO_FILE" ]; then
  echo ""
  echo -e "${YELLOW}Estado da sessão anterior:${NC}"
  head -30 "$ESTADO_FILE" 2>/dev/null
  echo ""
else
  echo -e "${YELLOW}Nenhum estado anterior encontrado (.garra-estado.md)${NC}"
fi

# Verificar .env
if [ ! -f ".env" ]; then
  echo -e "${YELLOW}AVISO: arquivo .env não encontrado. Copie de .env.example${NC}"
elif grep -q 'CHANGE_ME\|<your_\|TODO\|PLACEHOLDER' ".env" 2>/dev/null; then
  echo -e "${YELLOW}AVISO: .env contém valores placeholder — configure antes de usar${NC}"
fi

# Listar skills disponíveis
if [ -d "skills" ]; then
  SKILL_COUNT=$(find skills -name "*.md" 2>/dev/null | wc -l)
  echo -e "${GREEN}Skills disponíveis:${NC} $SKILL_COUNT"
  for skill in skills/*.md; do
    [ -f "$skill" ] && echo "  /$(basename "$skill" .md)"
  done
fi

# Listar agents disponíveis
if [ -d ".claude/agents" ]; then
  echo -e "${GREEN}Agents disponíveis:${NC}"
  for agent in .claude/agents/*.md; do
    [ -f "$agent" ] && echo "  @$(basename "$agent" .md)"
  done
fi

# Lançar GarraDream em background (se existir)
DREAM_SCRIPT="scripts/garra-dream.sh"
if [ -f "$DREAM_SCRIPT" ] && [ -x "$DREAM_SCRIPT" ]; then
  bash "$DREAM_SCRIPT" &
  disown 2>/dev/null || true
fi

echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
exit 0
