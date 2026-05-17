# Plan 0142 — GAR-656: Replace daemonize 0.5 with nix syscalls (RUSTSEC-2025-0069)

> Health routine fix — 2026-05-17 (Florida ET)
> Branch: `health/202605171245-replace-daemonize-nix`
> Linear: [GAR-656](https://linear.app/chatgpt25/issue/GAR-656)

## Goal

Remove the `daemonize 0.5.0` crate (flagged unmaintained by RUSTSEC-2025-0069) from the
dependency graph of `garraia-cli`. Replace it with direct POSIX syscalls via `nix`
(already a transitive dep at 0.31.3) and the existing `libc` (already an explicit
unix dep of the same crate). Net new transitive deps: zero.

## Architecture

The `start_daemon` function in `crates/garraia-cli/src/main.rs` implements Unix
double-fork daemonization. `daemonize::Daemonize` wraps: `fork()` → `setsid()` →
second `fork()` → PID-file write → `dup2()` redirects → return to grandchild.
We replace all of that with equivalent nix + libc calls inline.

## Tech stack

- `nix 0.31.3` (already in lockfile via transitive paths; added as direct dep with
  `features = ["process"]` for `fork`, `setsid`, `getpid`)
- `libc` (already an explicit `[target.'cfg(unix)'.dependencies]` dep in garraia-cli)
  used for `dup2`, `STDIN_FILENO`, `STDOUT_FILENO`, `STDERR_FILENO`

## Design invariants

- Behaviour of the daemon is identical: double-fork, setsid, PID file, stdin→null,
  stdout+stderr→log file, chdir stays `.` (unchanged).
- The `fork()` call happens before any tokio runtime is created (existing invariant;
  preserved verbatim).
- `std::process::exit(0)` is used (not libc's `_exit`) in the intermediate processes
  because Rust's process model is fine here — no runtime, no threads yet.

## Out of scope

- Upgrading opentelemetry (tracked separately in GAR-629 notes).
- Windows or non-unix start_daemon stubs (unchanged).
- Any change to daemon behaviour or CLI surface.

## Rollback

Revert the two file changes (Cargo.toml + main.rs) and re-add RUSTSEC-2025-0069 to
deny.toml. No migration needed.

## §12 Open questions

None — the nix double-fork idiom is well-established POSIX.

## File Structure

```
crates/garraia-cli/Cargo.toml          → [target.'cfg(unix)'.dependencies]: drop daemonize, add nix
crates/garraia-cli/src/main.rs         → rewrite start_daemon (unix variant only)
deny.toml                              → remove RUSTSEC-2025-0069 from ignore list
plans/README.md                        → add row 0142
```

## Tasks (M1)

- [x] T1: Update `crates/garraia-cli/Cargo.toml` — drop `daemonize`, add `nix = { version = "0.31", features = ["process"] }`
- [x] T2: Rewrite `start_daemon` in `src/main.rs` using `nix::unistd::{fork,setsid,getpid}` + `libc::dup2`
- [x] T3: Remove RUSTSEC-2025-0069 from `deny.toml` ignore list
- [ ] T4: `cargo check -p garraia` — Linux clean
- [ ] T5: `cargo clippy --workspace --tests --exclude garraia-desktop --no-deps -- -D warnings` — clean
- [ ] T6: Commit, push, open PR
- [ ] T7: CI green (20/20 checks)
- [ ] T8: Squash-merge, update plans/README.md, mark GAR-656 Done

## Risk register

| Risk | Mitigation |
|------|-----------|
| nix `process` feature missing `getpid`/`fork`/`setsid` | fall back to `libc` equivalents if compile fails |
| dup2 errno not checked | explicit check of libc return value → bail! |
| PID written by grandchild ≠ what daemonize wrote | identical: grandchild calls `getpid()` which is the daemon PID |

## Acceptance criteria

- `daemonize` absent from `Cargo.lock`
- RUSTSEC-2025-0069 absent from `deny.toml`
- `cargo check -p garraia` clean
- All 20 CI checks green
- `deny.toml` cargo-deny warnings drop from 23 → 22 (one entry removed)

## Cross-references

- RUSTSEC-2025-0069 (daemonize unmaintained)
- GAR-430 (quality gates epic)
- `deny.toml:53` (old entry location)
- `crates/garraia-cli/src/main.rs:1289`

## Estimativa

T-shirt: XS (< 1h, 3 files, ~50 LOC delta)
