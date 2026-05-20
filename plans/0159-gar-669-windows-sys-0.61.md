# Plan 0159 — GAR-669 Slice 2: windows-sys 0.52 → 0.61 (garraia-cli)

**Status:** Done — Merged 2026-05-20 via PR #451 (`1e7ce50`)
**Branch:** `routine/202005201220-gar-669-windows-sys-0.61-fix`
**Linear:** [GAR-669](https://linear.app/chatgpt25/issue/GAR-669)
**Parent plan:** GAR-669 (Cargo deps breaking API changes)

---

## Goal

Bump `windows-sys` from `0.52` to `0.61` in `crates/garraia-cli/Cargo.toml`, fix the
breaking API change in `main.rs`, close Dependabot PR #422, and verify the
`#[cfg(windows)]` Windows-only path compiles on CI (`Test (windows-latest)`).

---

## Architecture

Single-crate change in `garraia-cli`. The only Windows-specific code is
`is_process_running(pid: u32)` in `crates/garraia-cli/src/main.rs` which calls
`OpenProcess` / `GetExitCodeProcess` / `CloseHandle`.

**Breaking change in windows-sys 0.61:**

In windows-sys 0.52, `HANDLE = isize`. In windows-sys 0.61:
```
pub type HANDLE = *mut core::ffi::c_void;
```

This means `if handle == 0` is a type error (cannot compare pointer to integer).
The fix is `if handle.is_null()`.

All other APIs (`GetExitCodeProcess`, `CloseHandle`, `STILL_ACTIVE`, `FALSE`,
`PROCESS_QUERY_LIMITED_INFORMATION`) remain compatible:
- `STILL_ACTIVE: NTSTATUS = 0x103_u32 as _` (259i32); `STILL_ACTIVE as u32` = 259u32 ✅
- `FALSE: windows_sys::core::BOOL = 0i32` ✅
- `GetExitCodeProcess` and `CloseHandle` accept `HANDLE = *mut c_void` ✅

---

## Tech stack

- `windows-sys 0.61.2` (was 0.52.0) — uses `windows-link 0.2.1` instead of `windows-targets`
- Feature flags unchanged: `Win32_Foundation`, `Win32_System_Threading`,
  `Win32_System_Diagnostics_ToolHelp`

---

## Design invariants

- Zero production logic changes beyond the type fix.
- No `unwrap()` introduced.
- Lockfile updated automatically by cargo.

---

## Validações pré-plano

- [x] windows-sys 0.61 source inspected: `HANDLE = *mut core::ffi::c_void`
- [x] `cargo check -p garraia` clean (Linux)
- [x] Cargo.lock updated to windows-sys 0.61.2

---

## Out of scope

- windows-sys bumps in other crates (transitive deps, not direct)
- GAR-669 Slice 3 (password-hash 0.6) — separate PR

---

## Rollback

Revert the version change in `Cargo.toml` and the `is_null()` fix in `main.rs`.
Run `cargo update -p windows-sys --precise 0.52.0`.

---

## M1 Tasks

- [x] T1: Bump `windows-sys` version constraint `0.52` → `0.61` in `crates/garraia-cli/Cargo.toml`
- [x] T2: Fix `handle == 0` → `handle.is_null()` in `is_process_running`
- [x] T3: Verify `cargo check -p garraia` clean (Linux)
- [x] T4: Commit + push
- [x] T5: Open PR, wait for CI green (especially `Test (windows-latest)`)
- [x] T6: Merge + mark GAR-669 Slice 2 done
- [x] T7: Update plans/README.md + ROADMAP.md

---

## Acceptance criteria

- [x] All CI checks green including `Test (windows-latest)` (compilation-only via `--no-run`)
- [x] Dependabot PR #422 superseded by PR #451
- [x] `cargo audit` clean (Security Audit check passed)

---

## Cross-references

- [GAR-669](https://linear.app/chatgpt25/issue/GAR-669) — parent issue
- Dependabot PR #422 — superseded
- Plan 0158 — GAR-669 Slice 1 (rand_chacha)

---

## Estimativa

- **Baixa:** 1h (version bump + type fix + CI verification)
- **Provável:** 2h (including CI wait)
