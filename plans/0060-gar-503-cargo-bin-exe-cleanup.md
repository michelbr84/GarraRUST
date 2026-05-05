# Plan 0060 — GAR-503: Remove `CARGO_BIN_EXE_garraia` dead-code fallback

## §1 Goal

Remove the `CARGO_BIN_EXE_garraia` backward-compat fallback in
`crates/garraia-cli/tests/migrate_workspace_integration.rs:40` plus its
explanatory docblock paragraph. The bin was renamed from `garraia` → `garra`
long ago; the fallback is confirmed dead code in CI and in every active local
checkout. Keeping it clutters the signal in `garra_bin()` and contradicts the
codebase convention against dead-code accumulation.

**Combined housekeeping** delivered in the same PR:

- Mark plans 0058 (GAR-509) and 0059 (GAR-511) as Merged in `plans/README.md`.
- Mark the three Grupos endpoints `POST /v1/groups/{id}/invites`,
  `POST /v1/groups/{id}/members/{user_id}:setRole`, and
  `DELETE /v1/groups/{id}/members/{user_id}` as `[x]` in `ROADMAP.md §3.4`
  (they shipped in plans 0018–0020 in April 2026 but the ROADMAP checkbox was
  never flipped).

## §2 Architecture

No architectural change — pure dead-code removal + doc accuracy fix.

## §3 Tech stack / crates touched

| Crate / file | Change |
|---|---|
| `crates/garraia-cli/tests/migrate_workspace_integration.rs` | Remove `.or_else(\|\| std::env::var_os("CARGO_BIN_EXE_garraia"))` (line 40) + remove docblock paragraph (lines 34-37) |
| `plans/README.md` | Update rows 0058 and 0059 from 🟡 to ✅ Merged |
| `ROADMAP.md` | Flip 3 × `[ ]` → `[x]` in §3.4 Grupos |
| `plans/0060-gar-503-cargo-bin-exe-cleanup.md` | This file |

## §4 Design invariants

- The `garra_bin()` function must still compile and still return the correct path
  to the `garra` binary in CI (`CARGO_BIN_EXE_garra` is still present).
- No test logic changes — only the env-var lookup fallback chain shrinks.
- `cargo test -p garraia-cli --test migrate_workspace_integration` must remain
  green (or skip gracefully when Docker absent, as before).

## §5 Validações pré-plano

- [x] `CARGO_BIN_EXE_garraia` grep returns 0 hits outside this single file
      (`grep -r CARGO_BIN_EXE_garraia crates/ --include='*.rs' | grep -v migrate_workspace_integration`).
- [x] `CARGO_BIN_EXE_garra` is the active variable per `cargo test` output and
      Cargo docs (env exported for each `[[bin]]` by name).
- [x] GAR-503 exists in Linear (Backlog, Low priority, no duplicate).

## §6 Out of scope

- Removing or renaming the `garra` binary itself.
- Changing any other integration test.
- Migrating the SQLite→Postgres stages.
- Any Clippy or fmt changes beyond what the dead-code removal triggers.

## §7 Rollback plan

`git revert <commit>` — the change is a pure line deletion; reverting it
re-introduces the dead fallback harmlessly.

## §8 File structure (changes)

```
crates/garraia-cli/tests/migrate_workspace_integration.rs   ← -6 lines
plans/0060-gar-503-cargo-bin-exe-cleanup.md                 ← this file (new)
plans/README.md                                             ← update rows 0058, 0059, add 0060
ROADMAP.md                                                  ← flip 3 checkboxes in §3.4 Grupos
```

## §9 M1 tasks

- [x] **T1** — Write plan 0060 (`plans/0060-gar-503-cargo-bin-exe-cleanup.md`)
- [x] **T2** — Remove dead `.or_else(|| std::env::var_os("CARGO_BIN_EXE_garraia"))` + paragraph
- [x] **T3** — Update `plans/README.md` (rows 0058, 0059 → ✅ Merged; add row 0060)
- [x] **T4** — Update `ROADMAP.md §3.4 Grupos` (3 checkboxes)
- [x] **T5** — Commit, push, open PR, wait for CI green, merge
- [x] **T6** — Mark GAR-503 Done in Linear

## §10 Risk register

| Risk | Probability | Impact | Mitigation |
|---|---|---|---|
| Compile error if CARGO_BIN_EXE_garra absent in some env | Low | Medium | The `garra` bin is the only binary in `garraia-cli`; Cargo always exports this var for `[[test]]` bins |
| Merge conflict with PR #131 | Low | Low | PR #131 only touches `.github/workflows/` — no overlap |

## §11 Acceptance criteria

- `grep -r CARGO_BIN_EXE_garraia crates/` → 0 results.
- `cargo test -p garraia-cli` passes (or skips Docker-gated tests).
- `plans/README.md` shows 0058 and 0059 as ✅ Merged with correct SHAs.
- `ROADMAP.md §3.4` shows invites, setRole, delete as `[x]`.

## §12 Open questions

_None — scope is fixed._

## §13 Cross-references

- GAR-503: <https://linear.app/chatgpt25/issue/GAR-503>
- Plans 0018 (invites), 0019 (accept), 0020 (setRole + delete)
- Plans 0058 (GAR-509 threads), 0059 (GAR-511 uploads_worker)

## §14 Estimativa

< 30 min (pure deletion + doc update).
