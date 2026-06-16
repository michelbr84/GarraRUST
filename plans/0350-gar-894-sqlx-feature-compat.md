# Plan 0350 — GAR-894: split sqlx runtime-tokio-native-tls → runtime-tokio + tls-native-tls

> **Status:** In Progress
> **Linear:** [GAR-894](https://linear.app/chatgpt25/issue/GAR-894)
> **Branch:** `routine/202606160045-sqlx-feature-compat`
> **Run:** 146 — 2026-06-16 ~00:28 ET / 2026-06-16T04:28 UTC

## Goal

Split the combined `runtime-tokio-native-tls` feature in the workspace root `Cargo.toml` into
`runtime-tokio` + `tls-native-tls`. The combined feature was removed in sqlx 0.9; keeping it
in the workspace root blocks Dependabot from auto-creating security fix PRs for sqlx.

Identified in plan 0349 (health run 145) as next-cycle tech debt. No active CVE today — proactive fix.

## Root cause

`Cargo.toml` (workspace root) line 114:
```toml
sqlx = { version = "0.8", default-features = false, features = ["runtime-tokio-native-tls", "postgres", "chrono"] }
```

The `runtime-tokio-native-tls` combined feature was removed in sqlx 0.9. The `garraia-workspace/Cargo.toml`
already correctly uses `runtime-tokio` (no `tls-native-tls` needed there because it overrides at crate level).

## Fix

Single-line change to workspace root `Cargo.toml`:

```toml
# Before:
sqlx = { version = "0.8", default-features = false, features = ["runtime-tokio-native-tls", "postgres", "chrono"] }

# After:
sqlx = { version = "0.8", default-features = false, features = ["runtime-tokio", "tls-native-tls", "postgres", "chrono"] }
```

Both `runtime-tokio` and `tls-native-tls` exist in sqlx 0.8 (the combined feature is an alias). CI
continues to pass. Dependabot can now auto-create sqlx upgrade PRs if a RUSTSEC advisory appears.

## Files changed

```
Cargo.toml            ← line 114: runtime-tokio-native-tls → runtime-tokio + tls-native-tls
plans/
  0350-gar-894-sqlx-feature-compat.md  ← this file
  README.md                             ← 0350 row added; 0346/0347/0349 stale status fixed
```

## Tasks

- [x] T1 — Create Linear issue GAR-894
- [x] T2 — Create plan file `plans/0350-...md`
- [x] T3 — Edit `Cargo.toml` line 114
- [x] T4 — Update `plans/README.md` (0350 row + fix 0346/0347/0349 stale entries)
- [ ] T5 — Push + open PR + wait for CI green
- [ ] T6 — Squash-merge; mark GAR-894 Done

## Acceptance criteria

- `cargo check --workspace --exclude garraia-desktop` passes
- CI all checks green
- `grep runtime-tokio-native-tls Cargo.toml` returns empty (compat feature gone from workspace root)
- Dependabot next sqlx attempt: no longer blocked by removed feature

## Risk

Negligible — `runtime-tokio` + `tls-native-tls` have been available since sqlx 0.7 and are exactly
what the split of `runtime-tokio-native-tls` produces. sqlx 0.8 accepts both the legacy combined
form and the split form. No schema changes, no migration, no API surface changes.
