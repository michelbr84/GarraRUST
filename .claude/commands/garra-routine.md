---
description: Roadmap-driven autonomous slice. Reads ROADMAP+Linear, picks the next step, plans it, implements via PR + CI, merges to main when green, updates tracking. Designed to be invoked manually or via cron at xH:15 every 2 hours (Florida local time).
---

# `/garra-routine` — autonomous next-slice routine

Each invocation runs the full workflow below from scratch — there is no persistent memory between iterations. Treat each run as independent and idempotent.

## Workflow

### 1. Read state

- Read `ROADMAP.md` (especially §1.5 latest snapshot, §3.4 chats checklist, §7 "Próximos passos imediatos").
- `git fetch origin main && git checkout main && git pull --ff-only`.
- `git log --oneline -20` and `git status`.
- Query Linear via MCP `mcp__cd3f2209-...__list_issues` (team GAR, project "Fase 3 — Group Workspace", state In Progress + recent Done).
- Verify open PRs via `mcp__github__list_pull_requests` (state=open). If any open PR is waiting on CI or merge, **complete that first** before opening a new one.

### 2. Decide next step

Apply ROADMAP §7 priority order:

1. Close any open security/CI follow-ups.
2. Fase 3.4 — next API slice on `/v1/*`.
3. Q6 mutation triage (GAR-505 family).
4. ADR 0004 / Fase 3.5 storage validation in production paths.
5. Fase 5.1 — CredentialVault final (GAR-291).

Prefer slices that are: (i) shovel-ready (schema + auth foundation already shipped); (ii) <500 LOC; (iii) have clear acceptance criteria.

Concrete candidate queue (as of 2026-05-04):

- **Plan 0055** = `POST/GET /v1/chats/{id}/messages` + DM creation (`type='dm'`) — chats slice 2.
- **Plan 0056** = `POST /v1/messages/{id}/threads` — chats slice 3.
- **GAR-505** = Q6.10 mutation triage (6 missed mutants in `jwt.rs` / `storage_redacted.rs` / `app_pool.rs` + 3 timeouts).
- **GAR-503** = remove `CARGO_BIN_EXE_garraia` fallback in `crates/garraia-cli/tests/migrate_workspace_integration.rs:34-42`.
- **GAR-504** = first benchmark evidence run on DigitalOcean droplet (1 vCPU / 1 GB) for the `README.md` table.

If everything tracked is Done or blocked, fall back to the smallest CI/docs cleanup available (e.g., GAR-503).

### 3. Create the plan

- Write `plans/00NN-...md` with the same shape as plan 0054 (Goal, Architecture, Tech stack, Design invariants, Validações pré-plano, Out of scope, Rollback, §12 Open questions, File Structure, M1 tasks with checkboxes, Risk register, Acceptance criteria, Cross-references, Estimativa).
- File a Linear child issue under the right epic via MCP `mcp__cd3f2209-...__save_issue` (team GAR, labels per the epic: `epic:ws-chat`/`epic:ws-api`/`epic:test-cov`/etc.). Capture the GAR-NNN id and amend the plan + `plans/README.md` row.
- **Search Linear first** to avoid duplicates: `list_issues` with a representative query; only create if no candidate matches.
- Commit `docs(plans): add plan 00NN for GAR-NNN ...` on a new branch named `routine/<UTC-yyyymmddhhmm>-<slug>` off the current `main` HEAD.

### 4. Implement task-by-task

Follow plan 0054's TDD pattern: tests first → red → impl → green → clippy strict → commit per task.

- Use `SWAGGER_UI_DOWNLOAD_URL=file:///tmp/swagger-ui-cache/v5.17.14.zip cargo ...` for any gateway compile (cached zip is at that path; if missing, `curl -sLo /tmp/swagger-ui-cache/v5.17.14.zip https://github.com/swagger-api/swagger-ui/archive/refs/tags/v5.17.14.zip` works because curl uses system certs).
- Use `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` for workspace-wide lint (Tauri desktop fails locally without GTK).
- Each task = independent commit. Push after each task.

**Hard rules** (from `CLAUDE.md`):

- NO `unwrap()` outside tests.
- NO SQL string concat (use `params!` for rusqlite, `sqlx::query!`/`query_as` for Postgres).
- NO PII in audit metadata (carry `name_len` not `name`, etc.).
- SET LOCAL **both** `app.current_user_id` AND `app.current_group_id` for any FORCE-RLS table.
- Cross-group authz tests on every new tenant-scoped route (regra 10).

### 5. Push + open PR + wait for CI green

- Push all commits, open PR via `mcp__github__create_pull_request` (base=main, head=routine branch). Body shape mirrors PR #122 with a "Test plan" checkbox section.
- Poll CI via `mcp__github__pull_request_read get_check_runs` until ALL actual workflow checks complete. Use a backgrounded `sleep N && echo done` to space polls 4–8 minutes apart — never tight loops.
- **If "Format Check" fails:** `cargo fmt`, commit `style: cargo fmt`, push.
- **If "Clippy" fails:** read failures via the job's html_url, fix per cluster, commit, push.
- **If "Test (ubuntu/macos/windows)" fails:** read job log, fix, push. Repeat until green.
- **Legacy "CodeQL" check** (3-second run, URL pattern `/runs/...` not `/actions/runs/`) is a known artifact from GAR-486 default→advanced setup migration — treat as PASS regardless of conclusion. The actual CodeQL is `Analyze (rust)` + `Analyze (javascript-typescript)`.
- **If main moves ahead** (PR shows `mergeable_state: behind`): `git fetch origin main && git merge origin/main --no-edit && git push`. Re-poll.

**Acceptance:** ≥16 actual workflow checks all `success` (Format, Clippy, Test×3, Build, MSRV, cargo-deny, Security Audit, Coverage, Analyze rust, Analyze js-ts, Playwright, E2E, Secret Scan, Dependency Review).

### 6. Merge + bookkeeping

- Squash-merge via `mcp__github__merge_pull_request` with `merge_method=squash`. Commit title = PR title with `(#PR)` suffix. Body = condensed summary referencing GAR-NNN.
- Mark Linear issue Done via `mcp__cd3f2209-...__save_issue` with `state=Done`.
- If the merged commit didn't already update `ROADMAP.md` and `plans/README.md` (T8 of the plan), open a small doc-only PR flipping the relevant `[ ]` → `[x]` and adding the merged commit sha + PR number to the plan row. CI on docs-only is fast.

### 7. Stop

End the iteration with a one-paragraph summary: branch name, PR number, GAR-NNN, commit sha on main, what's next.

**DO NOT** invoke `/garra-routine` recursively. The harness handles re-arming.

## Hard guardrails

- Never push directly to `main`. Always go through PR + green CI.
- Never `unwrap()` in production; never SQL string concat; never log/audit PII.
- Never bypass the RLS protocol (`SET LOCAL app.current_user_id` + `app.current_group_id` for FORCE-RLS tables).
- Never include automated AI signature in commits beyond the existing `https://claude.ai/code/session_...` line.
- Never amend or force-push merged commits.
- Never run `rm -rf` or destructive git ops outside the working tree.
- Never spam Linear with duplicate issues — search existing issues by `query` first; if a candidate matches, update it instead of creating a new one.
- If the routine cannot pick a productive next step (everything Done or blocked), file a single status note in the team's tracker (or create one once if absent) and exit cleanly without opening a PR.

## Local sandbox notes

- The dev container does not have GTK/GDK system libs (Tauri desktop fails to compile) — workaround `--exclude garraia-desktop`.
- `reqwest`'s bundled certs reject github.com (utoipa-swagger-ui build script fails) — workaround `SWAGGER_UI_DOWNLOAD_URL=file:///tmp/swagger-ui-cache/v5.17.14.zip` (cache the zip via curl once).

## When to invoke this command

- **Manually:** type `/garra-routine` in any Claude Code session.
- **Cron:** wire to `scripts/run-garra-routine.sh` from system cron at `15 */2 * * *` (Florida local). The wrapper calls `claude --print '/garra-routine'` headlessly.
- **GitHub Actions reminder:** the workflow `.github/workflows/garra-routine-trigger.yml` opens a tracking issue every 2h at xH:15 UTC; the issue body links back here.
