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
D4=$(printf "rm -rf %s" "~")
D5=$(printf "sudo rm -rf %s" "/")
D6=$(printf "rm -r -f %s" "/")
D7=$(printf "rm -rf %s" "/*")
D8=$(printf "cd /tmp && rm -rf %s" ".")
D9=$(printf "rm -rf %s" '$HOME')
D10=$(printf "rm -rf %s" "..")
D11=$(printf "rm -rf -- %s" "/")

run_case "blocks rm -rf root"        "$(printf "$DANGER1" "$D1")" 2
run_case "blocks rm -rf cwd-glob"    "$(printf "$DANGER1" "$D2")" 2
run_case "blocks DROP TABLE"         "$(printf "$DANGER1" "$D3")" 2
run_case "blocks rm -rf home"        "$(printf "$DANGER1" "$D4")" 2
run_case "blocks sudo rm -rf root"   "$(printf "$DANGER1" "$D5")" 2
run_case "blocks rm -r -f root"      "$(printf "$DANGER1" "$D6")" 2
run_case "blocks rm -rf root-glob"   "$(printf "$DANGER1" "$D7")" 2
run_case "blocks rm -rf cwd after &&" "$(printf "$DANGER1" "$D8")" 2
run_case "blocks rm -rf \$HOME"      "$(printf "$DANGER1" "$D9")" 2
run_case "blocks rm -rf parent"      "$(printf "$DANGER1" "$D10")" 2
run_case "blocks rm -rf -- root"     "$(printf "$DANGER1" "$D11")" 2

# 2. legitimate removals that the old substring match blocked (#1453): a
#    path UNDER /, ., ~ or .. is not the catastrophic target itself.
S1=$(printf "rm -rf %s" "/tmp/claude-0/scratchpad/pintest")
S2=$(printf "rm -rf %s" "./pintest")
S3=$(printf "rm -rf %s" "~/.cache/garraia-test")
S4=$(printf "rm -rf %s" "../build")
S5=$(printf "rm -rf %s" ".git/worktrees/tmp")
S6=$(printf "rm -rf %s %s" "target/tmp" "target/tmp2")

run_case "allows cargo test"           "$(printf "$DANGER1" "$SAFE_CMD")" 0
run_case "allows rm -rf /tmp/<path>"   "$(printf "$DANGER1" "$S1")" 0
run_case "allows rm -rf ./<path>"      "$(printf "$DANGER1" "$S2")" 0
run_case "allows rm -rf ~/<path>"      "$(printf "$DANGER1" "$S3")" 0
run_case "allows rm -rf ../<path>"     "$(printf "$DANGER1" "$S4")" 0
run_case "allows rm -rf .git/<path>"   "$(printf "$DANGER1" "$S5")" 0
run_case "allows rm -rf two rel paths" "$(printf "$DANGER1" "$S6")" 0
run_case "no-op on empty cmd"     '{"tool_name":"Bash","tool_input":{}}' 0
run_case "no-op on non-Bash tool" '{"tool_name":"Read","tool_input":{}}' 0

echo ""
echo "Total: $((PASS + FAIL))   Pass: $PASS   Fail: $FAIL"
exit "$FAIL"
