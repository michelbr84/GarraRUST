#!/usr/bin/env bash
# GarraIA SuperPowers — Setup Script
# Verifica pré-requisitos, configura hooks e prepara o ambiente

set -euo pipefail

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${CYAN}  GarraIA SuperPowers — Setup${NC}"
echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

ERRORS=0

check_tool() {
  local tool="$1"
  local desc="$2"
  if command -v "$tool" &>/dev/null; then
    echo -e "  ${GREEN}✓${NC} $tool — $desc"
  else
    echo -e "  ${RED}✗${NC} $tool — $desc (NÃO ENCONTRADO)"
    ERRORS=$((ERRORS + 1))
  fi
}

check_optional() {
  local tool="$1"
  local desc="$2"
  if command -v "$tool" &>/dev/null; then
    echo -e "  ${GREEN}✓${NC} $tool — $desc"
  else
    echo -e "  ${YELLOW}○${NC} $tool — $desc (opcional)"
  fi
}

# ── Ferramentas obrigatórias ───────────────────────────────────────────────
echo ""
echo -e "${CYAN}Verificando ferramentas obrigatórias:${NC}"
check_tool "git" "controle de versão"
check_tool "cargo" "build system Rust"
check_tool "rustc" "compilador Rust"
check_tool "gh" "GitHub CLI"
check_tool "python3" "scripts de automação"

# ── Ferramentas opcionais ─────────────────────────────────────────────────
echo ""
echo -e "${CYAN}Verificando ferramentas opcionais:${NC}"
check_optional "flutter" "build Flutter mobile"
check_optional "jq" "processamento JSON"
check_optional "dot" "Graphviz (dependency graph)"
check_optional "shellcheck" "linting de shell scripts"
check_optional "cargo-clippy" "linting Rust"

# ── Configurar .env ───────────────────────────────────────────────────────
echo ""
echo -e "${CYAN}Configurando ambiente:${NC}"
if [ ! -f ".env" ]; then
  if [ -f ".env.example" ]; then
    cp ".env.example" ".env"
    echo -e "  ${GREEN}✓${NC} .env criado a partir de .env.example"
    echo -e "  ${YELLOW}→ Edite .env e preencha os valores${NC}"
  else
    echo -e "  ${YELLOW}○${NC} .env.example não encontrado"
  fi
else
  echo -e "  ${GREEN}✓${NC} .env já existe"
  if grep -q 'CHANGE_ME\|<your_\|TODO\|PLACEHOLDER' ".env" 2>/dev/null; then
    echo -e "  ${YELLOW}→ .env contém valores placeholder — configure antes de usar${NC}"
  fi
fi

# ── Tornar hooks/scripts executáveis ──────────────────────────────────────
echo ""
echo -e "${CYAN}Configurando permissões:${NC}"
for f in .claude/hooks/*.sh scripts/*.sh workflows/*.sh; do
  if [ -f "$f" ]; then
    chmod +x "$f" 2>/dev/null || true
    echo -e "  ${GREEN}✓${NC} $f"
  fi
done

# ── Verificar gh auth ─────────────────────────────────────────────────────
echo ""
echo -e "${CYAN}Verificando autenticação:${NC}"
if gh auth status &>/dev/null; then
  echo -e "  ${GREEN}✓${NC} GitHub CLI autenticado"
else
  echo -e "  ${YELLOW}○${NC} GitHub CLI não autenticado — rode: gh auth login"
fi

# ── Verificar estrutura ──────────────────────────────────────────────────
echo ""
echo -e "${CYAN}Verificando estrutura do projeto:${NC}"
REQUIRED_FILES=(
  ".claude/settings.json"
  ".claude/hooks/session-start.sh"
  ".claude/hooks/pre-tool-use.sh"
  ".claude/hooks/post-tool-use.sh"
  ".claude/hooks/stop.sh"
  ".claude/agents/code-reviewer.md"
  ".claude/agents/security-auditor.md"
  ".claude/agents/doc-writer.md"
  ".claude/agents/team-coordinator.md"
  "CLAUDE.md"
  "skills/review-pr.md"
  "skills/tdd-loop.md"
  "skills/fix-issue.md"
  "skills/pre-commit.md"
  "skills/refactor-module.md"
  "skills/assemble-team.md"
  "skills/generate-docs.md"
  "scripts/garra-dream.sh"
)

for f in "${REQUIRED_FILES[@]}"; do
  if [ -f "$f" ]; then
    echo -e "  ${GREEN}✓${NC} $f"
  else
    echo -e "  ${RED}✗${NC} $f (FALTANDO)"
    ERRORS=$((ERRORS + 1))
  fi
done

# ── Resultado ─────────────────────────────────────────────────────────────
echo ""
echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
if [ "$ERRORS" -eq 0 ]; then
  echo -e "${GREEN}Setup completo! GarraIA SuperPowers pronto para uso.${NC}"
  echo ""
  echo "Próximos passos:"
  echo "  1. Edite .env com suas credenciais"
  echo "  2. Abra o projeto com Claude Code"
  echo "  3. Use /assemble-team para tarefas complexas"
  echo "  4. Use /review-pr para revisar PRs"
else
  echo -e "${RED}Setup incompleto: $ERRORS erros encontrados.${NC}"
  echo "Corrija os erros acima e rode novamente."
  exit 1
fi
