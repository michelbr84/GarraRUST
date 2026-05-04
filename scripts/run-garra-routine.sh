#!/usr/bin/env bash
# scripts/run-garra-routine.sh
#
# Wrapper for the /garra-routine slash command (.claude/commands/garra-routine.md)
# intended to be invoked from system cron at xH:15 every 2 hours (Florida local).
#
# Crontab entry (run `crontab -e` and append, adjust to your repo path):
#
#   15 */2 * * * /home/user/GarraRUST/scripts/run-garra-routine.sh >> /var/log/garra-routine.log 2>&1
#
# The wrapper:
#   - cd's into the repo (must be the working tree, not a worktree)
#   - exports the cached swagger-ui zip path (avoids github.com TLS fetches)
#   - invokes Claude Code headlessly with the slash command
#   - exits with the same status code Claude returned
#
# Requirements:
#   - The `claude` CLI must be on $PATH (https://docs.claude.com/claude-code).
#   - The repo must already be cloned at $REPO_DIR with a checked-out branch.
#   - The user running cron must have `gh auth status` valid (the tools the
#     routine uses operate via MCP under the Claude Code session).
#   - Network access to github.com, linear.app and the Anthropic API.
#
# Hard guardrails inherited from .claude/commands/garra-routine.md:
#   - Never push to main directly; always through PR + green CI.
#   - Never `unwrap()` outside tests; never log PII; never SQL string concat.

set -euo pipefail

REPO_DIR="${GARRA_REPO_DIR:-$(cd "$(dirname "$0")/.." && pwd)}"
SWAGGER_CACHE="${GARRA_SWAGGER_CACHE:-/tmp/swagger-ui-cache/v5.17.14.zip}"

# Ensure swagger-ui zip is cached locally so utoipa-swagger-ui's build
# script does not try to fetch github.com (whose cert chain reqwest's
# bundled webpki rejects in some sandboxes).
if [[ ! -f "$SWAGGER_CACHE" ]]; then
    echo "[$(date -u +%FT%TZ)] swagger-ui cache missing; fetching" >&2
    mkdir -p "$(dirname "$SWAGGER_CACHE")"
    curl --fail --silent --show-error --location \
        --output "$SWAGGER_CACHE" \
        https://github.com/swagger-api/swagger-ui/archive/refs/tags/v5.17.14.zip
fi

export SWAGGER_UI_DOWNLOAD_URL="file://$SWAGGER_CACHE"

cd "$REPO_DIR"

# Belt-and-braces: refuse to run if HEAD is not on a tracking branch we
# can safely fetch from. The /garra-routine command starts by `git fetch
# origin main && git checkout main && git pull --ff-only`, so any
# uncommitted state would block it. Better to fail loud here.
if ! git diff --quiet || ! git diff --cached --quiet; then
    echo "[$(date -u +%FT%TZ)] ERROR: working tree not clean; aborting" >&2
    git status --short >&2
    exit 65
fi

# Invoke Claude Code with the slash command. `--print` (-p) runs
# non-interactively and prints the final assistant message.
# `--dangerously-skip-permissions` is the canonical flag for unattended
# runs — keep this wrapper inside cron + a dedicated user account so the
# blast radius is the working tree only.
exec claude --print --dangerously-skip-permissions '/garra-routine'
