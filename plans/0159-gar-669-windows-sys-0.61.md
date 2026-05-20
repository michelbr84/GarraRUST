# Plan 0159 — GAR-669 Slice 2: windows-sys 0.52 → 0.61 (garraia-cli)

**Status:** In Progress
**Branch:** `routine/202005201220-gar-669-windows-sys-0.61`
**Linear:** [GAR-669](https://linear.app/chatgpt25/issue/GAR-669)
**Parent plan:** GAR-669 (Cargo deps breaking API changes)

---

## Goal

Bump `windows-sys` from `0.52` to `0.61` in `crates/garraia-cli/Cargo.toml`, close
Dependabot PR #422, and verify the `#[cfg(windows)]` Windows-only path in `main.rs`
still compiles on CI (Test windows-latest).

---

## Architecture

Single-crate change in `garraia-cli`. The only Windows-specific code is
`is_process_running(pid: u32)` in `crates/garraia-cli/src/main.rs` (lines 482–505)
which calls:
- `OpenProcess` / `GetExitCodeProcess` / `CloseHandle` from `Win32::System::Threading` +
  `Win32::Foundation`
- `HANDLE` comparison: already uses `handle == 0` (not `ptr::null_mut()`)

In both windows-sys 0.52 and 0.61, `HANDLE` is `isize`, so the comparison is
compatible. No code changes required — only the version constraint and lockfile.

---

## Tech stack

- `windows-sys 0.61.2` (was 0.52.0)
- Feature flags unchanged: `Win32_Foundation`, `Win32_System_Threading`,
  `Win32_System_Diagnostics_ToolHelp`

---

## Design invariants

- No production logic changes — version bump only.
- No `unwrap()` introduced.
- Lockfile updated automatically by cargo.

---

## Validações pré-plano

- [x] `cargo check -p garraia` clean (verified locally 2026-05-20)
- [x] `cargo clippy --workspace --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` clean
- [x] Cargo.lock now contains `windows-sys 0.61.2`

---

## Out of scope

- `windows-sys` bumps in other crates (transitive deps, not direct)
- GAR-669 Slice 3 (password-hash 0.6) — separate PR

---

## Rollback

Revert the version change in `Cargo.toml` and run `cargo update -p windows-sys --precise 0.52.0`.

---

## M1 Tasks

- [x] T1: Bump `windows-sys` version constraint `0.52` → `0.61` in `crates/garraia-cli/Cargo.toml`
- [x] T2: Verify `cargo check -p garraia` clean (no windows-specific changes needed)
- [x] T3: Verify workspace clippy clean
- [x] T4: Commit + push
- [ ] T5: Open PR, wait for CI green (especially `Test (windows-latest)`)
- [ ] T6: Merge + mark GAR-669 Slice 2 done
- [ ] T7: Update plans/README.md + ROADMAP.md

---

## Acceptance criteria

- [ ] All 20 CI checks green including `Test (windows-latest)`
- [ ] Dependabot PR #422 can be closed (superseded)
- [ ] `cargo audit` clean (windows-sys 0.61 has no known advisories)

---

## Cross-references

- [GAR-669](https://linear.app/chatgpt25/issue/GAR-669) — parent issue
- Dependabot PR #422 — superseded
- Plan 0158 — GAR-669 Slice 1 (rand_chacha)

---

## Estimativa

- **Baixa:** ~30 min (version bump only, no code changes)
- **Provável:** 1h (CI verification + merge)
