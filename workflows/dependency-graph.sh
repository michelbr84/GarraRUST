#!/usr/bin/env bash
# GarraIA SuperPowers — Dependency Graph
# Analisa imports entre crates Rust e gera grafo Graphviz
#
# Uso: ./workflows/dependency-graph.sh [--output docs/deps.svg]

set -euo pipefail

OUTPUT="${1:-docs/deps.svg}"
DOT_FILE="${OUTPUT%.svg}.dot"

CYAN='\033[0;36m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${CYAN}GarraIA Dependency Graph${NC}"

# Extrair dependências entre crates do workspace
echo -e "${CYAN}Analisando Cargo.toml de cada crate...${NC}"

{
  echo "digraph GarraRUST {"
  echo '  rankdir=LR;'
  echo '  node [shape=box, style=filled, fillcolor="#f0f0f0", fontname="Helvetica"];'
  echo '  edge [color="#666666"];'
  echo ""

  # Nó especial para crates core
  echo '  "garraia-gateway" [fillcolor="#ff6b6b"];'
  echo '  "garraia-agents" [fillcolor="#4ecdc4"];'
  echo '  "garraia-db" [fillcolor="#45b7d1"];'
  echo '  "garraia-security" [fillcolor="#96ceb4"];'
  echo '  "garraia-channels" [fillcolor="#feca57"];'
  echo ""

  for TOML in crates/*/Cargo.toml; do
    CRATE_DIR=$(dirname "$TOML")
    CRATE_NAME=$(basename "$CRATE_DIR")

    # Extrair dependências que são crates do workspace
    grep -E '(garraia-|path = )' "$TOML" 2>/dev/null | while read -r line; do
      DEP=$(echo "$line" | grep -oP 'garraia-\w+' | head -1)
      if [ -n "$DEP" ] && [ "$DEP" != "$CRATE_NAME" ]; then
        echo "  \"$CRATE_NAME\" -> \"$DEP\";"
      fi
    done
  done

  echo "}"
} > "$DOT_FILE"

echo -e "${GREEN}DOT file gerado: $DOT_FILE${NC}"

# Tentar gerar SVG se dot (Graphviz) estiver disponível
if command -v dot &>/dev/null; then
  mkdir -p "$(dirname "$OUTPUT")"
  dot -Tsvg "$DOT_FILE" -o "$OUTPUT"
  echo -e "${GREEN}SVG gerado: $OUTPUT${NC}"
else
  echo -e "${YELLOW}Graphviz (dot) não encontrado. Instale para gerar SVG:${NC}"
  echo "  choco install graphviz   (Windows)"
  echo "  apt install graphviz     (Linux)"
  echo "  brew install graphviz    (macOS)"
  echo ""
  echo "O arquivo DOT pode ser visualizado em https://dreampuf.github.io/GraphvizOnline/"
fi
