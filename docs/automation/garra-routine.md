# `garra-routine` — autonomous next-slice routine

> Read `ROADMAP.md` + Linear, pick the next step, plan it, implement it via PR + CI,
> merge to main when green, update tracking. Designed to fire every 2 hours at
> xH:15 (Florida local) — but can also be triggered manually or on-demand.

## Why this exists

The owner asked for a recurring routine that:

1. Reads `ROADMAP.md` and Linear (https://linear.app/chatgpt25/team/GAR/projects/all).
2. Picks the best next step.
3. Creates a plan.
4. Implements via GitHub Actions (PR + CI green) — never directly on `main`.
5. Verifies CI is green and merges to `main`.
6. Updates `ROADMAP.md` and Linear.
7. Sleeps until the next xH:15 (every 2 hours).

The orchestration logic lives entirely in the slash command
[`.claude/commands/garra-routine.md`](../../.claude/commands/garra-routine.md).
This document explains **how to wire that command to a recurring schedule**.

## Three wiring options (pick one or stack them)

### Option 1 — Manual invocation (zero setup, recommended to start)

In any Claude Code session at the repo root:

```text
/garra-routine
```

The command will execute the full workflow from scratch and end with a
one-paragraph summary plus (typically) a merged PR.

**When to use:** any time. There is no per-iteration state, so you can run
it 5 minutes apart or 5 days apart.

### Option 2 — System cron + the headless `claude` CLI

The repo ships
[`scripts/run-garra-routine.sh`](../../scripts/run-garra-routine.sh) which:

- ensures the swagger-ui zip cache is present,
- exports `SWAGGER_UI_DOWNLOAD_URL` to point at the cache,
- refuses to run with a dirty working tree,
- invokes `claude --print --dangerously-skip-permissions /garra-routine`.

Crontab entry for **xH:15 every 2 hours in Florida local time** (replace
`/home/user/GarraRUST` with your actual repo path):

```cron
# m  h  dom mon dow   command
  15 */2 *   *   *    /home/user/GarraRUST/scripts/run-garra-routine.sh >> /var/log/garra-routine.log 2>&1
```

**Important:** cron uses the **system local timezone** by default. If your
server is on UTC and you want Florida local time, either:

1. Set `TZ=America/New_York` at the top of your crontab, or
2. Translate to UTC manually (Florida is UTC−5 in EST, UTC−4 in EDT — the
   xH:15 alignment shifts one hour twice a year).

**Requirements:**

- The `claude` CLI must be on `$PATH` (https://docs.claude.com/claude-code).
- The user running cron must have:
  - `gh auth status` valid (the routine uses MCP-backed GitHub tools).
  - Linear MCP credentials configured for the same Claude install.
  - Network access to github.com, linear.app, anthropic.com.
- The repo must be already cloned at the path the wrapper points at.
- The working tree must be clean — the wrapper exits with code 65 (sysexits
  `EX_DATAERR`) otherwise. This is intentional: a routine triggered while
  uncommitted changes are sitting in the tree would either eat them or
  refuse to make progress.

**When to use:** server-side automation on a host you control. This is the
canonical "every 2h at xH:15" path.

### Option 3 — GitHub Actions cron + tracking issue (passive reminder)

The workflow
[`.github/workflows/garra-routine-trigger.yml`](../../.github/workflows/garra-routine-trigger.yml)
runs every 2 hours at HH:15 **UTC** and opens a tracking issue. It does NOT
actually run the routine — it only files a reminder that someone (or some
other automation) can act on.

The issue body links back to `.claude/commands/garra-routine.md` so the
person who picks it up has the full procedure inline.

**Idempotency:** the workflow first searches for an existing open issue with
title `garra-routine: YYYY-MM-DDTHH UTC` and skips the create step if one
already exists. Two cron events that fire close to each other in the same
UTC hour will only produce one issue.

**Trade-off:** GitHub Actions cron does not honor named timezones — the
`15 */2 * * *` expression is UTC. If you specifically want xH:15 **in
Florida local time**, prefer Option 2.

**When to use:** lightweight reminder for teams that do not want to grant
their CI runner credentials to invoke Claude headlessly. Stack on top of
Option 1 for visibility.

## How the slash command decides what to work on

Priority order (from `ROADMAP.md` §7 "Próximos passos imediatos"):

1. Close any open security/CI follow-ups.
2. Fase 3.4 — next API slice on `/v1/*`.
3. Q6 mutation triage.
4. ADR 0004 / Fase 3.5 storage validation in production paths.
5. Fase 5.1 — CredentialVault final (GAR-291).

Concrete shovel-ready candidates as of 2026-05-04 (after PR #122 merged):

- **Plan 0055** — `POST/GET /v1/chats/{id}/messages` + DM creation
  (`type='dm'`). Chats slice 2.
- **Plan 0056** — `POST /v1/messages/{id}/threads`. Chats slice 3.
- **GAR-505** — Q6.10 mutation triage.
- **GAR-503** — remove `CARGO_BIN_EXE_garraia` fallback in
  `crates/garraia-cli/tests/migrate_workspace_integration.rs:34-42`.

## Hard guardrails (enforced by the slash command)

- Never push directly to `main`. Always through PR + green CI.
- Never `unwrap()` outside tests; never SQL string concat; never log/audit
  PII.
- Never bypass RLS protocol (`SET LOCAL app.current_user_id` +
  `app.current_group_id` for FORCE-RLS tables).
- Never amend or force-push merged commits.
- Never run `rm -rf` or destructive git ops outside the working tree.
- Never spam Linear with duplicate issues — search first; update if a
  candidate matches.

If the routine cannot pick a productive next step (everything Done or
blocked), it files a single status note on the team's tracker and exits
without opening a PR. Silence is acceptable; spam is not.

## Local sandbox notes

The dev container that authored this routine has two known gaps:

- **No GTK/GDK** — Tauri desktop fails to compile. Workaround:
  `cargo clippy --workspace --tests --exclude garraia-desktop ...`.
- **`reqwest`'s bundled certs reject github.com** — utoipa-swagger-ui's
  build script fails. Workaround:
  `SWAGGER_UI_DOWNLOAD_URL=file:///tmp/swagger-ui-cache/v5.17.14.zip`
  (cache the zip via curl once; curl uses system certs and works).

The wrapper script handles both automatically.

## See also

- [`.claude/commands/garra-routine.md`](../../.claude/commands/garra-routine.md)
  — the full slash command body.
- [`scripts/run-garra-routine.sh`](../../scripts/run-garra-routine.sh) — the
  cron wrapper.
- [`.github/workflows/garra-routine-trigger.yml`](../../.github/workflows/garra-routine-trigger.yml)
  — the GitHub Actions reminder cron.
- [`ROADMAP.md`](../../ROADMAP.md) §7 — priority order the routine reads.
- [`CLAUDE.md`](../../CLAUDE.md) — convention rules the routine inherits.
- [`plans/0054-gar-ws-chat-slice1-chats-crud.md`](../../plans/0054-gar-ws-chat-slice1-chats-crud.md)
  — the canonical plan shape the routine produces.
