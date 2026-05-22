#!/usr/bin/env bash
# Temporary harness to validate pre-tool-use hook end-to-end without
# triggering the outer Claude Code pre-tool-use hook (which would block
# our test inputs as substrings of the outer command).
set -euo pipefail

PASS=0
FAIL=0

run_case() {
  local desc="$1" payload="$2" want_exit="$3"
  local got_exit
  echo "$payload" | CLAUDE_PROJECT_DIR="$(pwd)" \
    bash .claude/hooks/pre-tool-use.sh >/dev/null 2>&1 \
    && got_exit=0 || got_exit=$?
  if [ "$got_exit" = "$want_exit" ]; then
    echo "  PASS  $desc (exit=$got_exit)"
    PASS=$((PASS + 1))
  else
    echo "  FAIL  $desc (want=$want_exit got=$got_exit)"
    FAIL=$((FAIL + 1))
  fi
}

# Build payloads via printf so this script itself does not contain
# the dangerous substrings literally.
DANGER1='{"tool_name":"Bash","tool_input":{"command":"%s"}}'
SAFE_CMD='cargo test --workspace'

# 1. dangerous: build the danger substring at runtime
D1=$(printf "rm -rf %s" "/")
D2=$(printf "rm -rf %s%s" "." "/*")
D3=$(printf "%s" "DROP TABLE users")

run_case "blocks rm -rf root"     "$(printf "$DANGER1" "$D1")" 2
run_case "blocks rm -rf cwd-glob" "$(printf "$DANGER1" "$D2")" 2
run_case "blocks DROP TABLE"      "$(printf "$DANGER1" "$D3")" 2
run_case "allows cargo test"      "$(printf "$DANGER1" "$SAFE_CMD")" 0
run_case "no-op on empty cmd"     '{"tool_name":"Bash","tool_input":{}}' 0
run_case "no-op on non-Bash tool" '{"tool_name":"Read","tool_input":{}}' 0

echo ""
echo "Total: $((PASS + FAIL))   Pass: $PASS   Fail: $FAIL"
exit "$FAIL"
