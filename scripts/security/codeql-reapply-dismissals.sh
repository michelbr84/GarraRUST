#!/usr/bin/env bash
# scripts/security/codeql-reapply-dismissals.sh
#
# Reaplica dismissals CodeQL via REST API a partir do ledger machine-readable
# `docs/security/codeql-suppressions.json`. Owner: GAR-491.
#
# Modes:
#   --dry-run     (default) print intended PATCH calls but do not execute
#   --apply       execute PATCH (mutually exclusive with --dry-run)
#   --check-md    validate that `.md` ledger and `.json` ledger list the same
#                 alert numbers; exit 0 if in sync, 4 if drift
#   --alert <N>   restrict to a single alert number (used by empirical proof)
#   -h | --help   print usage
#
# Exit codes:
#   0  success
#   1  usage error
#   2  ledger inconsistency: alert mutated since ledger entry was written
#      (rule_id/path/line mismatch). Manual re-audit required — DO NOT auto-fix.
#   3  alert no longer relevant (state=fixed or 404). Ledger entry stale.
#   4  --check-md drift: `.md` and `.json` ledgers out of sync.
#   5  precondition failure (gh CLI missing, jq missing, ledger missing, etc.)
#
# Fail-closed contract: this script NEVER silently ignores a mismatch.
# A divergence between the live alert and the ledger MUST be surfaced to a
# human; it could mean the codebase changed, the rule was renamed, or the
# alert was renumbered. Auto-applying PATCH on a stale ledger would mask
# real regressions.
#
# Per amendment A8: there is intentionally NO schedule wiring here. This
# script is invoked manually from the GAR-491 PR and the empirical proof.
# A future sub-issue (GAR-491.2) decides if/when to wire it into a workflow.
#
# Empirical-proof anchor: alert #43 (credentials.rs:49) is the proof target.
# All other entries in the ledger are dismissed only AFTER #43 persists
# across a CodeQL re-run.

set -euo pipefail

# ---- defaults --------------------------------------------------------------
REPO="${GARRAIA_CODEQL_REPO:-michelbr84/GarraRUST}"
LEDGER_JSON="${LEDGER_JSON:-docs/security/codeql-suppressions.json}"
LEDGER_MD="${LEDGER_MD:-docs/security/codeql-suppressions.md}"
MODE="dry-run"
SINGLE_ALERT=""
CHECK_MD=0

usage() {
  cat <<'EOF'
Usage:
  codeql-reapply-dismissals.sh [--dry-run|--apply] [--alert <N>]
  codeql-reapply-dismissals.sh --check-md
  codeql-reapply-dismissals.sh -h | --help

Default mode is --dry-run; pass --apply to actually PATCH.
EOF
}

# ---- arg parsing -----------------------------------------------------------
while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run)   MODE="dry-run"; shift ;;
    --apply)     MODE="apply"; shift ;;
    --check-md)  CHECK_MD=1; shift ;;
    --alert)
      if [[ $# -lt 2 ]]; then
        echo "error: --alert requires an argument" >&2
        usage >&2
        exit 1
      fi
      SINGLE_ALERT="$2"
      shift 2
      ;;
    -h|--help) usage; exit 0 ;;
    *)
      echo "error: unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

# ---- preconditions ---------------------------------------------------------
need() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "error: required tool not found in PATH: $1" >&2
    exit 5
  }
}
need gh
need jq

if [[ ! -f "$LEDGER_JSON" ]]; then
  echo "error: ledger JSON not found at $LEDGER_JSON" >&2
  exit 5
fi
if [[ ! -f "$LEDGER_MD" ]]; then
  echo "error: ledger MD not found at $LEDGER_MD" >&2
  exit 5
fi

# Validate JSON syntax up front.
if ! jq empty "$LEDGER_JSON" >/dev/null 2>&1; then
  echo "error: ledger JSON is malformed: $LEDGER_JSON" >&2
  exit 5
fi

# ---- --check-md mode --------------------------------------------------------
# Cross-validate that every alert_number listed in `.json` appears in `.md`
# and vice versa. The MD has rows like `[#40](.../alerts/40)` so we extract
# the numbers via regex. This is intentionally simple — the MD is human
# authored, drifts happen, this catches them at PR time.
if [[ "$CHECK_MD" -eq 1 ]]; then
  # Normalize CR (Windows jq emits CRLF) so the equality test compares only
  # the numeric content.
  json_nums=$(jq -r '.entries[].alert_number' "$LEDGER_JSON" | tr -d '\r' | sort -n)
  md_nums=$(grep -oE '/code-scanning/[0-9]+' "$LEDGER_MD" \
              | grep -oE '[0-9]+$' \
              | tr -d '\r' \
              | sort -nu)
  if [[ "$json_nums" != "$md_nums" ]]; then
    echo "error: ledger MD/JSON drift detected" >&2
    echo "JSON entries:" >&2
    echo "$json_nums" | sed 's/^/  #/' >&2
    echo "MD entries (from /code-scanning/N links):" >&2
    echo "$md_nums" | sed 's/^/  #/' >&2
    exit 4
  fi
  count=$(echo "$json_nums" | wc -l | tr -d ' ')
  echo "ok: ledger MD/JSON in sync ($count entries)"
  exit 0
fi

# ---- main reapply loop ------------------------------------------------------
ENTRIES=$(jq -c '.entries[]' "$LEDGER_JSON")

skipped=0
applied=0
dryrun=0
errors=0

while IFS= read -r entry; do
  n=$(echo "$entry"     | jq -r '.alert_number')
  rule=$(echo "$entry"  | jq -r '.rule_id')
  path=$(echo "$entry"  | jq -r '.path')
  line=$(echo "$entry"  | jq -r '.line')
  reason=$(echo "$entry"| jq -r '.dismissed_reason')
  just=$(echo "$entry"  | jq -r '.justification')
  gar=$(echo "$entry"   | jq -r '.gar_ref')
  anchor=$(echo "$entry"| jq -r '.ledger_md_anchor')

  if [[ -n "$SINGLE_ALERT" && "$SINGLE_ALERT" != "$n" ]]; then
    continue
  fi

  echo
  echo "── alert #$n ($rule @ $path:$line) ──────────────────────────────"

  # Fetch current alert state. 404 => alert no longer exists (likely fixed
  # or rule removed). Treat as exit 3 — manual ledger cleanup.
  if ! current=$(gh api "repos/$REPO/code-scanning/alerts/$n" 2>/dev/null); then
    echo "  error: alert #$n not retrievable from $REPO (404 or auth issue)"
    errors=$((errors + 1))
    exit 3
  fi

  cur_rule=$(echo "$current" | jq -r '.rule.id // ""')
  cur_path=$(echo "$current" | jq -r '.most_recent_instance.location.path // ""')
  cur_line=$(echo "$current" | jq -r '.most_recent_instance.location.start_line // 0')
  cur_state=$(echo "$current" | jq -r '.state // ""')
  cur_reason=$(echo "$current" | jq -r '.dismissed_reason // ""')

  # Fail-closed validation: rule_id, path, and line MUST match the ledger.
  # Anything else means the alert mutated since the ledger entry was authored
  # and a human needs to re-audit.
  mismatch=0
  if [[ "$cur_rule" != "$rule" ]]; then
    echo "  FAIL: rule_id mismatch — ledger=$rule, current=$cur_rule"
    mismatch=1
  fi
  if [[ "$cur_path" != "$path" ]]; then
    echo "  FAIL: path mismatch — ledger=$path, current=$cur_path"
    mismatch=1
  fi
  if [[ "$cur_line" != "$line" ]]; then
    echo "  FAIL: line mismatch — ledger=$line, current=$cur_line"
    mismatch=1
  fi
  if [[ $mismatch -eq 1 ]]; then
    echo "  → exit 2: ledger entry stale, manual re-audit required"
    exit 2
  fi

  # Already-dismissed and matching reason: skip silently.
  if [[ "$cur_state" == "dismissed" && "$cur_reason" == "$reason" ]]; then
    echo "  skip: already dismissed (reason=$cur_reason)"
    skipped=$((skipped + 1))
    continue
  fi

  # Already-fixed: alert no longer applicable.
  if [[ "$cur_state" == "fixed" ]]; then
    echo "  → exit 3: alert state=fixed, ledger entry stale"
    exit 3
  fi

  # State is open (or dismissed with WRONG reason — we'll re-PATCH to correct).
  #
  # GitHub API quirks (HTTP 422 surfaces):
  #   - `dismissed_reason` values use SPACES not underscores:
  #     {"false positive", "won't fix", "used in tests"}.
  #   - `dismissed_comment` max 280 characters total.
  #
  # We keep snake_case in the JSON ledger (it survives shell/jq cleanly)
  # and translate here. We compose a short comment that always fits in the
  # 280-char limit and points the auditor at the ledger anchor for the
  # full justification.
  case "$reason" in
    false_positive) api_reason="false positive" ;;
    used_in_tests)  api_reason="used in tests" ;;
    "won't_fix"|wont_fix) api_reason="won't fix" ;;
    *)
      echo "  FAIL: unknown dismissed_reason in ledger: $reason"
      errors=$((errors + 1))
      exit 5
      ;;
  esac

  # 280-char comment budget. Shape: "GAR-491 #N: <first ~180 of just>... See <ledger>#<anchor>."
  ledger_ref=" See $LEDGER_MD#$anchor."
  prefix="$gar #$n: "
  budget=$((280 - ${#prefix} - ${#ledger_ref}))
  if [[ ${#just} -gt $budget ]]; then
    short_just="${just:0:$((budget - 3))}..."
  else
    short_just="$just"
  fi
  comment="${prefix}${short_just}${ledger_ref}"

  if [[ ${#comment} -gt 280 ]]; then
    # Defensive — should never trigger given the math above.
    comment="${comment:0:277}..."
  fi

  if [[ "$MODE" == "dry-run" ]]; then
    echo "  DRY-RUN: would PATCH alert #$n state=dismissed reason='$api_reason'"
    echo "    comment (${#comment}/280 chars): $comment"
    dryrun=$((dryrun + 1))
    continue
  fi

  # --apply: actually PATCH.
  echo "  apply: PATCH alert #$n state=dismissed reason='$api_reason' (${#comment}/280 chars)"
  if gh api -X PATCH "repos/$REPO/code-scanning/alerts/$n" \
       -f state=dismissed \
       -f dismissed_reason="$api_reason" \
       -f dismissed_comment="$comment" \
       --silent; then
    applied=$((applied + 1))
    echo "  ok"
  else
    echo "  ERROR: PATCH failed"
    errors=$((errors + 1))
  fi
done <<< "$ENTRIES"

echo
echo "── summary ─────────────────────────────────────────────────────"
echo "  applied:   $applied"
echo "  dry-run:   $dryrun"
echo "  skipped:   $skipped (already-dismissed)"
echo "  errors:    $errors"

if [[ $errors -gt 0 ]]; then
  exit 5
fi
exit 0
