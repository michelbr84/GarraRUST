#!/usr/bin/env bash
# GarraIA SuperPowers — Verify Script
# Valida que todos os componentes estão funcionando corretamente

set -euo pipefail

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

echo -e "${CYAN}GarraIA SuperPowers — Verificação${NC}"
echo ""

ERRORS=0
WARNINGS=0

pass() { echo -e "  ${GREEN}✓${NC} $1"; }
fail() { echo -e "  ${RED}✗${NC} $1"; ERRORS=$((ERRORS + 1)); }
warn() { echo -e "  ${YELLOW}○${NC} $1"; WARNINGS=$((WARNINGS + 1)); }

# ── 1. Ferramentas ────────────────────────────────────────────────────────
echo -e "${CYAN}[1/5] Ferramentas${NC}"
for tool in git cargo rustc gh python3; do
  command -v "$tool" &>/dev/null && pass "$tool" || fail "$tool não encontrado"
done
for tool in flutter jq dot shellcheck; do
  command -v "$tool" &>/dev/null && pass "$tool (opcional)" || warn "$tool (opcional)"
done

# ── 2. Configuração ──────────────────────────────────────────────────────
echo ""
echo -e "${CYAN}[2/5] Configuração${NC}"
[ -f ".env" ] && pass ".env existe" || fail ".env não encontrado"
[ -f ".claude/settings.json" ] && pass "settings.json existe" || fail "settings.json não encontrado"

if [ -f ".claude/settings.json" ]; then
  # Verificar que hooks estão configurados
  if python3 -c "
import json
s = json.load(open('.claude/settings.json'))
hooks = s.get('hooks', {})
assert 'SessionStart' in hooks, 'SessionStart missing'
assert 'PreToolUse' in hooks, 'PreToolUse missing'
assert 'PostToolUse' in hooks, 'PostToolUse missing'
assert 'Stop' in hooks, 'Stop missing'
" 2>/dev/null; then
    pass "4 hooks configurados em settings.json"
  else
    fail "hooks incompletos em settings.json"
  fi
fi

# ── 3. Hooks executáveis ─────────────────────────────────────────────────
echo ""
echo -e "${CYAN}[3/5] Hooks${NC}"
for hook in session-start pre-tool-use post-tool-use stop; do
  FILE=".claude/hooks/$hook.sh"
  if [ -f "$FILE" ]; then
    pass "$hook.sh existe"
  else
    fail "$hook.sh não encontrado"
  fi
done

# ── 4. Agents e Skills ──────────────────────────────────────────────────
echo ""
echo -e "${CYAN}[4/5] Agents${NC}"
for agent in code-reviewer security-auditor doc-writer team-coordinator; do
  FILE=".claude/agents/$agent.md"
  [ -f "$FILE" ] && pass "$agent" || fail "$agent não encontrado"
done

echo ""
echo -e "${CYAN}[5/5] Skills${NC}"
for skill in review-pr tdd-loop fix-issue pre-commit refactor-module assemble-team generate-docs code-review git-assist shell-explain summarize translate web-lookup; do
  FILE="skills/$skill.md"
  [ -f "$FILE" ] && pass "$skill" || warn "$skill não encontrado"
done

# ── 5. Autenticação ──────────────────────────────────────────────────────
echo ""
echo -e "${CYAN}Autenticação${NC}"
gh auth status &>/dev/null && pass "GitHub CLI autenticado" || warn "gh não autenticado"

# ── Resultado ─────────────────────────────────────────────────────────────
echo ""
echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
if [ "$ERRORS" -eq 0 ]; then
  echo -e "${GREEN}Verificação aprovada!${NC} ($WARNINGS avisos)"
else
  echo -e "${RED}Verificação falhou: $ERRORS erros, $WARNINGS avisos${NC}"
  exit 1
fi
