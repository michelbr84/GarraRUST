#!/usr/bin/env bash
# GarraIA SuperPowers — Parallel Writer/Reviewer
# Cria worktrees isoladas para implementar e revisar em paralelo
#
# Uso: ./workflows/parallel-review.sh --feature nome --task "descrição"

set -euo pipefail

FEATURE=""
TASK=""

while [[ $# -gt 0 ]]; do
  case $1 in
    --feature) FEATURE="$2"; shift 2 ;;
    --task) TASK="$2"; shift 2 ;;
    *) echo "Opção desconhecida: $1"; exit 1 ;;
  esac
done

if [ -z "$FEATURE" ] || [ -z "$TASK" ]; then
  echo "Uso: $0 --feature <nome> --task <descrição>"
  exit 1
fi

CYAN='\033[0;36m'
GREEN='\033[0;32m'
NC='\033[0m'

BRANCH="feat/$FEATURE"
WRITER_WORKTREE="../garrarust-writer-$FEATURE"
REVIEWER_WORKTREE="../garrarust-reviewer-$FEATURE"
REPORT_FILE="parallel-review-$FEATURE.md"

# Cleanup on exit
cleanup() {
  echo "Limpando worktrees..."
  git worktree remove "$WRITER_WORKTREE" --force 2>/dev/null || true
  git worktree remove "$REVIEWER_WORKTREE" --force 2>/dev/null || true
}
trap cleanup EXIT

echo -e "${CYAN}GarraIA Parallel Review — feature: $FEATURE${NC}"
echo ""

# 1. Criar branch e worktree do writer
echo -e "${CYAN}[1/4] Criando worktree do writer...${NC}"
git branch "$BRANCH" 2>/dev/null || true
git worktree add "$WRITER_WORKTREE" "$BRANCH"

# 2. Writer implementa a feature
echo -e "${CYAN}[2/4] Writer implementando feature...${NC}"
cd "$WRITER_WORKTREE"
claude --print "Implemente a seguinte feature no GarraRUST: $TASK. Use TDD quando possível. Faça cargo check após cada mudança." 2>&1 | tee "../writer-output.txt"
cd -

# 3. Capturar diff do writer
echo -e "${CYAN}[3/4] Capturando diff e criando worktree do reviewer...${NC}"
cd "$WRITER_WORKTREE"
DIFF=$(git diff HEAD 2>/dev/null || echo "(sem diff)")
cd -

git worktree add "$REVIEWER_WORKTREE" HEAD

# 4. Reviewer analisa o diff
echo -e "${CYAN}[4/4] Reviewer analisando o código...${NC}"
cd "$REVIEWER_WORKTREE"
claude --print "Revise o seguinte diff como code-reviewer sênior do GarraRUST:

$DIFF

Avalie: correção, segurança, testes, style. Use o formato de revisão padrão com veredicto APROVADO/MUDANÇAS NECESSÁRIAS." 2>&1 | tee "../reviewer-output.txt"
cd -

# 5. Combinar relatórios
{
  echo "# Parallel Review — $FEATURE"
  echo ""
  echo "## Task"
  echo "$TASK"
  echo ""
  echo "## Writer Output"
  cat "../writer-output.txt" 2>/dev/null || echo "(sem output)"
  echo ""
  echo "## Reviewer Output"
  cat "../reviewer-output.txt" 2>/dev/null || echo "(sem output)"
} > "$REPORT_FILE"

rm -f "../writer-output.txt" "../reviewer-output.txt"

echo ""
echo -e "${GREEN}Relatório salvo em: $REPORT_FILE${NC}"
