#!/usr/bin/env bash
# scripts/quality/rust-file-stats.sh
#
# Emit a JSON snapshot of Rust source-file size statistics across the repo.
#
# Output JSON shape (stdout):
#   {
#     "total_rs_files": 334,
#     "max_file_lines": 3240,
#     "max_file_path": "crates/garraia-gateway/src/admin/handlers.rs",
#     "files_over_700": 33,
#     "files_over_1500": 7,
#     "files_over_2500": 2,
#     "top_15": [
#       {"path": "crates/.../foo.rs", "lines": 3240},
#       ...
#     ]
#   }
#
# Uses `git ls-files '*.rs'` so .gitignored files (target/, vendor/) are
# excluded automatically. No cargo invocations — runs in <1s on cold cache.
#
# Required tools: bash 4+, git, wc, awk, sort, head, jq.

set -euo pipefail

# Allow override for testing or non-repo invocation.
REPO_ROOT="${GARRAIA_REPO_ROOT:-$(git rev-parse --show-toplevel)}"
cd "$REPO_ROOT"

# Capture line counts per file. `wc -l` outputs `   N path` (right-aligned),
# we normalize to `N\tpath`. Strip the trailing total summary line.
counts_file=$(mktemp)
trap 'rm -f "$counts_file"' EXIT

git ls-files -- '*.rs' \
    | xargs -d '\n' wc -l 2>/dev/null \
    | awk 'NF==2 && $2 != "total" {printf "%s\t%s\n", $1, $2}' \
    | sort -nr -k1,1 \
    > "$counts_file"

total=$(wc -l < "$counts_file" | tr -d ' ')
max_line=$(head -n1 "$counts_file" || echo "0	")
max_lines=$(printf '%s\n' "$max_line" | awk -F'\t' '{print $1}')
max_path=$(printf '%s\n' "$max_line" | awk -F'\t' '{print $2}')

# Threshold counts.
over_700=$(awk -F'\t' '$1 > 700  {n++} END {print n+0}' "$counts_file")
over_1500=$(awk -F'\t' '$1 > 1500 {n++} END {print n+0}' "$counts_file")
over_2500=$(awk -F'\t' '$1 > 2500 {n++} END {print n+0}' "$counts_file")

# Top 15 as JSON array.
top_15_json=$(head -n15 "$counts_file" \
    | jq -R -s 'split("\n") | map(select(length > 0)) | map(split("\t")) | map({path: .[1], lines: (.[0] | tonumber)})')

jq -n \
    --argjson total "$total" \
    --argjson max_lines "${max_lines:-0}" \
    --arg max_path "$max_path" \
    --argjson over_700 "$over_700" \
    --argjson over_1500 "$over_1500" \
    --argjson over_2500 "$over_2500" \
    --argjson top_15 "$top_15_json" \
    '{
      total_rs_files: $total,
      max_file_lines: $max_lines,
      max_file_path: $max_path,
      files_over_700: $over_700,
      files_over_1500: $over_1500,
      files_over_2500: $over_2500,
      top_15: $top_15
    }'
