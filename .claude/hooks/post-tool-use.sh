#!/usr/bin/env bash
# GarraRUST — post-tool-use hook
# Roda cargo check ou flutter analyze após edições em arquivos fonte.
#
# Claude Code passa a chamada de tool via JSON em STDIN, no formato:
#   { "tool_name": "Edit"|"Write", "tool_input": { "file_path": "...", ... }, ... }
# (anteriormente: CLAUDE_TOOL_INPUT_FILE_PATH env var — abandonado pelo upstream).
# Mantemos fallback ao env var legacy para compat retroativa.

set -euo pipefail

# Resolve project root so Cargo / flutter lookups work regardless of CWD
# (GAR-445).
cd "${CLAUDE_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"

# Force non-interactive pagers — avoid subprocess hangs on minimal envs.
export GIT_PAGER="${GIT_PAGER:-cat}"
export PAGER="${PAGER:-cat}"
export LESS="${LESS:-}"

# ── Resolve file_path from stdin JSON (canonical) or legacy env var ───────
FILE="${CLAUDE_TOOL_INPUT_FILE_PATH:-}"

if [ -t 0 ]; then
  STDIN_PAYLOAD=""
else
  STDIN_PAYLOAD="$(cat 2>/dev/null || true)"
fi

if [ -n "$STDIN_PAYLOAD" ] && command -v jq >/dev/null 2>&1; then
  PARSED=$(echo "$STDIN_PAYLOAD" | jq -r '.tool_input.file_path // empty' 2>/dev/null || true)
  [ -n "$PARSED" ] && FILE="$PARSED"
fi

[ -z "$FILE" ] && exit 0

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

# Arquivo Rust (.rs) → cargo check no crate correspondente
if echo "$FILE" | grep -q '\.rs$'; then
  DIR="$FILE"
  while [ "$DIR" != "/" ] && [ "$DIR" != "." ]; do
    DIR=$(dirname "$DIR")
    if [ -f "$DIR/Cargo.toml" ] && grep -q '^\[package\]' "$DIR/Cargo.toml" 2>/dev/null; then
      CRATE=$(grep '^name' "$DIR/Cargo.toml" | head -1 | sed 's/.*= *"\(.*\)"/\1/')
      echo "→ cargo check -p $CRATE"
      if cargo check -p "$CRATE" --quiet 2>&1; then
        echo -e "${GREEN}✓ cargo check passou${NC}"
      else
        echo -e "${RED}✗ cargo check falhou — corrija antes de continuar${NC}"
      fi
      break
    fi
  done
fi

# Arquivo Flutter/Dart (.dart) → flutter analyze rápido
if echo "$FILE" | grep -q '\.dart$'; then
  FLUTTER_DIR=""
  DIR=$(dirname "$FILE")
  while [ "$DIR" != "/" ] && [ "$DIR" != "." ]; do
    if [ -f "$DIR/pubspec.yaml" ]; then
      FLUTTER_DIR="$DIR"
      break
    fi
    DIR=$(dirname "$DIR")
  done

  if [ -n "$FLUTTER_DIR" ]; then
    FLUTTER_CMD=$(command -v flutter 2>/dev/null || echo "")
    if [ -z "$FLUTTER_CMD" ] && [ -f "G:/Projetos/flutter/bin/flutter.bat" ]; then
      FLUTTER_CMD="G:/Projetos/flutter/bin/flutter.bat"
    fi

    if [ -z "$FLUTTER_CMD" ]; then
      echo "→ flutter não encontrado no PATH, pulando analyze"
      exit 0
    fi

    echo "→ flutter analyze $FLUTTER_DIR"
    if "$FLUTTER_CMD" analyze "$FLUTTER_DIR" --no-pub 2>&1 | grep -q "No issues found"; then
      echo -e "${GREEN}✓ flutter analyze passou${NC}"
    else
      echo -e "${RED}✗ flutter analyze encontrou problemas${NC}"
    fi
  fi
fi

exit 0
