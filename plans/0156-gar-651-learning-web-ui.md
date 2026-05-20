# Plan 0156 — GAR-651: Learning Agent Web UI

**Issue:** [GAR-651](https://linear.app/chatgpt25/issue/GAR-651)  
**Epic:** [GAR-641](https://linear.app/chatgpt25/issue/GAR-641) — Garra Learning Agent (sub-issue 10/10)  
**Branch:** `routine/202005201545-gar-651-learning-web-ui`  
**Date (Florida):** 2026-05-20  

---

## Goal

Deliver the Web UI for Skills and Learning Logs — component 10/10 of the Learning Agent epic. Operators can list, inspect, approve, reject, lock, and roll back skills through the Garra Glass Web Console without touching the command line.

---

## Architecture

REST handlers in `crates/garraia-gateway/src/learning_handler.rs` call into `garraia-learning::registry` (list, approve, reject, lock, delete) and `garraia-learning::versioning` (history, rollback) using the real `ProcessShellRunner`. A single HTML page (`learning.html`, embedded via `include_str!`) is served at `GET /learning` and fetches data from the `/api/learning/*` API.

URL namespace: `/api/learning/skills/*` and `/api/learning/logs/*` to avoid conflict with the existing `/api/skills` Phase-3.3 Skills Editor.

---

## Tech stack

- Rust / Axum 0.8 — handlers, routing
- `garraia-learning` — registry + versioning modules (already in workspace)
- Garra Glass design system — `--garra-*` CSS tokens, Inter + JetBrains Mono, glassmorphism panels (ADR 0009)
- Vanilla JS — fetch API for data, no external framework

---

## Design invariants

1. **Auth-free** — `/api/learning/*` follows the same model as `/api/health`, `/api/diagnostics` (Web Console endpoints are auth-free, secret-free).
2. **Skill name validation** — reuse `crate::path_validation::validate_skill_name` on every `{name}` path parameter to prevent path traversal.
3. **No PII in logs** — name_len logged, never the raw name on rejection.
4. **Read-only defaults** — rollback and approve/reject/lock are POST/DELETE, never GET (prevents CSRF via link).
5. **Fail gracefully** — if `garraia-learning` registry dir doesn't exist, endpoints return `{"skills": []}` / `{"items": []}` with HTTP 200.

---

## Out of scope

- Playwright E2E (deferred to next slice after design validation)
- Score timeline chart (SVG stub returned; full chart is post-MVP)
- Marketplace / skill publishing (Fase 7 pós-GA)
- Mobile / Desktop rendering (Web Console is desktop-first)

---

## Rollback

If CI fails: `git revert HEAD` on branch. No DB migrations, no config changes — pure code addition.

---

## File structure

```
crates/garraia-gateway/
  Cargo.toml                          ← add garraia-learning dep
  src/
    lib.rs                            ← add pub mod learning_handler
    learning_handler.rs               ← new: REST handlers
    learning.html                     ← new: Garra Glass page
  src/router.rs                       ← add /learning routes
plans/
  0156-gar-651-learning-web-ui.md     ← this file
  README.md                           ← add row
```

---

## Tasks

### T1 — Plan file + README
- [x] Write `plans/0156-gar-651-learning-web-ui.md`
- [x] Add row to `plans/README.md`

### T2 — Cargo dependency
- [ ] Add `garraia-learning = { workspace = true }` to `crates/garraia-gateway/Cargo.toml`

### T3 — REST handlers (`learning_handler.rs`)
- [ ] `list_learning_skills()` — reads both global + project registry dirs, merges, returns JSON array
- [ ] `get_learning_skill(name)` — full detail including frontmatter + history stub
- [ ] `approve_skill(name)` — calls `registry::promote_with_intent(SafetyIntent::HumanApproved)`
- [ ] `reject_skill(name)` — moves skill to `_rejected/`, records reason
- [ ] `lock_skill(name)` — sets `locked: true` in frontmatter
- [ ] `rollback_skill(name)` — calls `versioning::rollback` with `ProcessShellRunner`
- [ ] `delete_learning_skill(name)` — removes file (soft: moves to `_rejected/`)
- [ ] `get_learning_log_sessions()` — returns stub list from `~/.garra/sessions/`
- [ ] `get_learning_log_candidates()` — returns candidate skills (files in `_candidates/`)
- [ ] `get_learning_log_scores(since)` — returns score history from skill frontmatter
- [ ] `learning_ui()` — serves `learning.html`
- [ ] Unit tests for: list (empty dir returns []), name validation rejection, approve happy-path

### T4 — Module wiring
- [ ] `lib.rs`: add `pub mod learning_handler;`
- [ ] `router.rs`: add GET `/learning`, all `/api/learning/*` routes

### T5 — HTML page (`learning.html`)
- [ ] Garra Glass header + nav consistent with webchat.html
- [ ] Skills tab: table with name, score, scope, promoted_at, action buttons
- [ ] Skill detail modal: frontmatter, history list, action buttons (approve/reject/lock/rollback)
- [ ] Learning Logs tab: sessions list + candidates list
- [ ] Confirm modal for destructive actions (reject, rollback, delete)
- [ ] Fetch from `/api/learning/*` endpoints

---

## Acceptance criteria

- [ ] `GET /api/learning/skills` returns `{"skills": [...]}` (200, even if dir empty)
- [ ] `GET /api/learning/skills/:name` returns skill detail or 404
- [ ] `POST /api/learning/skills/:name/approve` calls safety gate, returns 200 or 400/403
- [ ] Name with `../` in path returns 400
- [ ] `GET /learning` returns HTML 200 with Garra Glass page
- [ ] `cargo test -p garraia-gateway -- learning_handler` passes
- [ ] `cargo clippy -p garraia-gateway` passes (no new warnings)

---

## Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| `garraia-learning` API surface changes between plan and impl | Low | Modules are stable post-GAR-642..650 |
| Path traversal via skill name | Medium | `validate_skill_name` from `path_validation` already covers `..` |
| Registry dir absent (fresh install) | High | Handlers return empty arrays instead of 500 |

---

## Cross-references

- ADR 0009 (`docs/adr/0009-garra-glass.md`) — design system
- ADR 0010 (`docs/adr/0010-garra-learning-agent.md`) — Learning Agent architecture
- Plan 0148 (GAR-645) — Skill Registry implementation
- Plan 0151 (GAR-650) — Skill Versioning/Rollback implementation

---

## Estimativa

0.5 / 1 / 2 days (all prereqs done, established patterns)
