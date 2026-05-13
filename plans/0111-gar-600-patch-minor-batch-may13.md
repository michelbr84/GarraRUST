# Plan 0111 — GAR-600: Patch-and-minor batch upgrade May 13, 2026

**Status:** In Progress  
**Branch:** `health/202505130900-patch-minor-batch`  
**PR:** (pending)  
**Linear:** [GAR-600](https://linear.app/chatgpt25/issue/GAR-600)  
**Triggered by:** Health routine 2026-05-13, priority (h) — medium-severity Dependabot batch

---

## Goal

Port Dependabot PR #296 (which became `dirty` against main) to a fresh `health/` branch and merge. Keeps 17 workspace crates at their latest patch/minor versions, including security-relevant libraries (jsonwebtoken, hyper, axum, tower-http).

## Architecture

- Workspace root `Cargo.lock` update (transitive lockfile pins)
- Single direct `Cargo.toml` change: `tracing-appender = "0.2.5"` in `crates/garraia-cli/Cargo.toml`
- No schema changes, no API surface changes, no feature flag changes

## Tech stack

- Rust / Cargo workspace
- cargo update --precise (lockfile-level bumps)

## Design invariants

- All packages stay within their declared SemVer range in workspace `Cargo.toml`
- No major-version crossings in this batch
- `cargo check --workspace --exclude garraia-desktop` must pass before push
- CI all-green before merge (18 checks)

## Out of scope

- Major version bumps (thiserror 2.x, toml 0.9, dialoguer 0.12, notify 8.x) — tracked separately in Dependabot PRs #288, #292, #290, #284
- Dependabot PR #281 (aws-actions GHA bump)

## Rollback

`git revert` the squash-merge commit on main. Cargo.lock reverts automatically.

## Packages updated

| Package | From | To | Notes |
| --- | --- | --- | --- |
| tokio | 1.49.0 | 1.50.0 | Runtime |
| axum | 0.8.8 | 0.8.9 | HTTP framework |
| tower-http | 0.6.8 | 0.6.10 | Drops iri-string, adds url |
| hyper | 1.8.1 | 1.9.0 | Drops pin-utils |
| sqlite-vec | 0.1.6 | 0.1.9 | Vector search |
| clap | 4.5.58 | 4.5.60 | CLI |
| tracing-subscriber | 0.3.22 | 0.3.23 | Logging |
| anyhow | 1.0.101 | 1.0.102 | Error handling |
| uuid | 1.21.0 | 1.23.1 | UUID generation |
| jsonwebtoken | 10.3.0 | 10.4.0 | JWT — gains zeroize on key material |
| poise | 0.6.1 | 0.6.2 | Discord bot framework |
| chrono | 0.4.43 | 0.4.44 | Date/time |
| tauri-plugin-dialog | 2.7.0 | 2.7.1 | Desktop dialog |
| tauri-plugin-fs | 2.5.0 | 2.5.1 | Desktop filesystem |
| tracing-appender | 0.2.4 | 0.2.5 | Log file appender |
| utoipa | 5.4.0 | 5.5.0 | OpenAPI docs |
| ipnet | 2.11.0 | 2.12.0 | IP network |
| filetime | 0.2.27 | 0.2.28 | File timestamps |

## Tasks

- [x] T1: Create `health/202505130900-patch-minor-batch` off main
- [x] T2: Update `tracing-appender = "0.2.5"` in `crates/garraia-cli/Cargo.toml`
- [x] T3: Run `cargo update -p <pkg> --precise <ver>` for all 17 packages
- [x] T4: `cargo check --workspace --exclude garraia-desktop` → clean
- [x] T5: Create plan file + Linear GAR-600
- [ ] T6: Commit + push + open PR
- [ ] T7: Wait for 18 CI checks green
- [ ] T8: Squash-merge, mark GAR-600 Done, update plans/README.md

## Risk register

| Risk | Likelihood | Mitigation |
| --- | --- | --- |
| Cargo.lock conflict with concurrent merge to main | Low | Branch off latest main; cargo check verifies |
| jsonwebtoken API change breaking auth | Very low | Patch release; cargo check passes; no API change in 10.3→10.4 |
| poise 0.6.2 macro regression | Low | cargo check passes; garraia-channels affected crate builds clean |

## Acceptance criteria

- All 18 CI checks green
- `Cargo.lock` shows target versions for all 17 packages
- Dependabot PR #296 can be closed (superseded)
- GAR-600 marked Done

## Cross-references

- Dependabot PR #296 (superseded)
- GAR-573 (last health routine, 2026-05-10)
- GAR-486 (green security baseline umbrella)

## Estimativa

1 session — purely mechanical dep bumps, already verified locally.
