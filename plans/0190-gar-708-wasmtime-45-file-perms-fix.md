# Plan 0190 — GAR-708: wasmtime 44.0.x → 45.0.0 (path_open TRUNCATE FilePerms bypass fix)

## Goal

Upgrade `wasmtime` and `wasmtime-wasi` from `"44"` to `"45"` to include the security fix for
`path_open(TRUNCATE) bypass of FilePerms::WRITE` introduced in wasmtime-wasi 45.0.0.

## Background

Health routine run 32 (2026-05-26 ~00:57 ET) identified a security-relevant commit in the
wasmtime 45.0.0 release branch:

> `1eb2c19 release-45.0.0: Fix wasmtime-wasi path_open(TRUNCATE) bypass of FilePerms::WR`

This is a **new variant** of the WASI filesystem permission bypass class. The previous
RUSTSEC-2026-0149 (fixed in 44.0.2, tracked as GAR-684/GAR-685) addressed
`DirPerms::MUTATE + FilePerms::READ` combinations. The 45.0.0 fix closes a related
bypass where a WASM guest lacking `FilePerms::WRITE` can still use `path_open(O_TRUNC)`
to truncate (i.e., destroy contents of) preopened files.

**Our exposure** (`crates/garraia-plugins/src/runtime.rs:123-127`):
```rust
let file_perms = if writable {
    FilePerms::READ | FilePerms::WRITE
} else {
    FilePerms::READ  // ← read-only mounts: TRUNCATE bypass present in 44.x
};
builder.preopened_dir(&host_path, &guest_path, dir_perms, file_perms)?;
```

A malicious WASM plugin with `filesystem: true` and read-only preopened paths could call
`path_open(O_TRUNC)` to truncate arbitrary files in its preopened directory, bypassing the
`FilePerms::READ`-only restriction.

## Architecture

Single-crate change: `garraia-plugins` (wasmtime + wasmtime-wasi version bump).
No API-breaking changes — the fix is behavioral (syscall validation), not surface-level.

## Tech stack

- Rust / Cargo workspace
- `wasmtime = "44"` → `"45"`
- `wasmtime-wasi = {version = "44", features = ["p1"]}` → `{version = "45", features = ["p1"]}`

## Design invariants

- Plugin sandbox API (`DirPerms`, `FilePerms`, `WasiCtxBuilder`, `WasiP1Ctx`, `p1::add_to_linker_async`, `MemoryInputPipe`, `MemoryOutputPipe`) unchanged in 45.x.
- No production code changes outside `Cargo.toml` / `Cargo.lock`.
- All existing plugin sandbox tests must pass green.

## Out of scope

- Upgrading wasmtime-wasi to WASIp2 — remains WASIp1 (`features = ["p1"]`).
- Dependabot PRs #514 (wasmtime) and #521 (wasmtime-wasi) — will auto-close on merge of this PR since it resolves the same bump.

## Rollback

`cargo update -p wasmtime --precise 44.0.2` + `cargo update -p wasmtime-wasi --precise 44.0.2` and revert Cargo.toml.

## Open questions

None — API compatibility confirmed by reading runtime.rs import surface against wasmtime 45.0.0 release notes.

## File structure

```
Cargo.toml                                  — version bump "44" → "45"
Cargo.lock                                  — auto-updated by cargo update
```

## Tasks

- [x] T1: Create health/ branch off main
- [x] T2: Write plan file 0190 + update plans/README.md
- [ ] T3: Bump wasmtime/"44"→"45", wasmtime-wasi/"44"→"45" in Cargo.toml
- [ ] T4: `cargo update -p wasmtime && cargo update -p wasmtime-wasi` (lock refresh)
- [ ] T5: `cargo build -p garraia-plugins` — confirm no API breakage
- [ ] T6: `cargo test -p garraia-plugins` — all tests green
- [ ] T7: `cargo clippy -p garraia-plugins --tests -- -D warnings`
- [ ] T8: Commit, push, open PR, wait for CI green, squash-merge

## Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| API breakage in wasmtime 45 | Low | Import surface fully verified in runtime.rs; all types stable |
| MSRV bump | Confirmed | wasmtime 45.0.0 requires Rust 1.93.0; workspace rust-version bumped 1.92→1.93 |
| Build time on CI (windows) | Medium | ~24-25 min historical; normal CI tolerance |

## Acceptance criteria

- `cargo build -p garraia-plugins` succeeds with wasmtime 45
- `cargo test -p garraia-plugins` green
- CI 20/20 checks pass
- Cargo.lock contains `wasmtime = "45.x.x"` and `wasmtime-wasi = "45.x.x"`

## Cross-references

- GAR-708 (this issue)
- GAR-684 (Done — RUSTSEC-2026-0149 fixed in 44.0.2)
- GAR-685 (Done — same fix)
- Dependabot PR #514 (wasmtime 45.0.0)
- Dependabot PR #521 (wasmtime-wasi 45.0.0)
- health/202605260057-wasmtime-45-file-perms-fix

## Estimativa

1 task, ~30 min CI.
