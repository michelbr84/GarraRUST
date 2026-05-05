#!/usr/bin/env bash
# scripts/quality/collect-metrics.sh
#
# Aggregate quality metrics into a single current-metrics.json on stdout.
#
# Default mode (fast, <2 min):
#   - Rust file-size stats (always — git ls-files + wc, sub-second)
#   - cargo audit JSON if `cargo-audit` is installed (~3 s on warm cache)
#   - Coverage % if lcov.info is found in $LCOV_PATH or ./lcov.info (best-effort)
#   - Clippy warnings: NOT collected by default (clippy is the slow one)
#
# Full mode (`--full`):
#   - Everything above PLUS
#   - cargo clippy --message-format=json (slow — re-runs clippy)
#
# Best-effort contract: if a tool is missing or an artifact is absent, the
# corresponding metric is marked `status: "not_collected_this_run"` rather
# than failing. The CI gates already enforce these tools (fmt/clippy/audit
# are blocking in ci.yml); the ratchet only OBSERVES.
#
# Per plan 0060 §"collect-metrics.sh default = fast mode" (Michel ajuste #4):
# default does NOT duplicate the heavy gates the CI already runs.
#
# Required tools: bash 4+, git, jq, python3 (or `py` on Windows).
# Optional tools: cargo-audit, lcov.info artifact, cargo-clippy.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="${GARRAIA_REPO_ROOT:-$(git rev-parse --show-toplevel)}"
cd "$REPO_ROOT"

# Pick python3 vs py launcher (Windows). Prefer python3; fall back to py -3.
PYTHON_BIN="${PYTHON_BIN:-}"
if [[ -z "$PYTHON_BIN" ]]; then
    if command -v python3 >/dev/null 2>&1; then
        PYTHON_BIN=python3
    elif command -v py >/dev/null 2>&1; then
        PYTHON_BIN="py -3"
    else
        echo "ERROR: neither python3 nor py launcher found on PATH" >&2
        exit 2
    fi
fi

mode="fast"
for arg in "$@"; do
    case "$arg" in
        --full) mode="full" ;;
        --fast) mode="fast" ;;
        -h|--help)
            sed -n '2,28p' "$0"
            exit 0
            ;;
        *)
            echo "ERROR: unknown argument $arg (use --fast | --full | -h)" >&2
            exit 2
            ;;
    esac
done

LCOV_PATH="${LCOV_PATH:-${REPO_ROOT}/lcov.info}"
AUDIT_TMP=$(mktemp)
CLIPPY_TMP=$(mktemp)
COV_TMP=$(mktemp)
trap 'rm -f "$AUDIT_TMP" "$CLIPPY_TMP" "$COV_TMP"' EXIT

# 1. Rust file stats (always)
file_stats=$(bash "$SCRIPT_DIR/rust-file-stats.sh")

# 2. Coverage (best-effort)
$PYTHON_BIN "$SCRIPT_DIR/parse-llvm-cov.py" "$LCOV_PATH" > "$COV_TMP"
coverage=$(cat "$COV_TMP")

# 3. cargo audit (best-effort — fast enough for default; ~3 s on warm cache)
if command -v cargo-audit >/dev/null 2>&1 || cargo audit --version >/dev/null 2>&1; then
    if cargo audit --json > "$AUDIT_TMP" 2>/dev/null; then
        : # success
    else
        # cargo audit exits non-zero when vulnerabilities found, but JSON is
        # still emitted on stdout. Don't fail collection.
        true
    fi
    audit=$($PYTHON_BIN "$SCRIPT_DIR/parse-cargo-audit.py" "$AUDIT_TMP")
else
    audit=$($PYTHON_BIN -c '
import json, sys
KNOWN = ("critical","high","medium","low","informational")
out = {sev: 0 for sev in KNOWN}
out["unknown"] = 0
out["total"] = 0
out["status"] = "not_collected_this_run"
out["reason"] = "cargo-audit_not_installed"
json.dump(out, sys.stdout)
')
fi

# 4. Clippy (only in --full mode — heavy)
if [[ "$mode" == "full" ]]; then
    if cargo clippy \
        --workspace \
        --exclude garraia-desktop \
        --message-format=json \
        --no-deps \
        > "$CLIPPY_TMP" 2>/dev/null; then
        : # success path
    else
        true # clippy may exit non-zero on warnings; JSON still emitted
    fi
    clippy=$($PYTHON_BIN "$SCRIPT_DIR/parse-clippy.py" "$CLIPPY_TMP")
else
    clippy=$($PYTHON_BIN -c '
import json, sys
out = {
  "clippy_warnings": 0,
  "non_clippy_warnings": 0,
  "errors": 0,
  "status": "not_collected_this_run",
  "reason": "fast_mode_skips_clippy_use_full_for_offline_run",
}
json.dump(out, sys.stdout)
')
fi

timestamp=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
git_sha=$(git rev-parse HEAD)
mode_label="$mode"

jq -n \
    --argjson file_stats "$file_stats" \
    --argjson coverage   "$coverage" \
    --argjson audit      "$audit" \
    --argjson clippy     "$clippy" \
    --arg     ts         "$timestamp" \
    --arg     sha        "$git_sha" \
    --arg     mode       "$mode_label" \
    '{
      schema_version: "1.0",
      collected_at: $ts,
      git_sha: $sha,
      collect_mode: $mode,
      total_rs_files: $file_stats.total_rs_files,
      max_file_lines: $file_stats.max_file_lines,
      max_file_path:  $file_stats.max_file_path,
      files_over_700:  $file_stats.files_over_700,
      files_over_1500: $file_stats.files_over_1500,
      files_over_2500: $file_stats.files_over_2500,
      top_15: $file_stats.top_15,
      coverage: $coverage,
      audit: $audit,
      clippy: $clippy
    }'
