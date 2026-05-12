# Plan 0108 — GAR-593: Drop stale RUSTSEC-2026-0002 (lru) after lru 0.16.4

**Status:** In Progress  
**Linear:** [GAR-593](https://linear.app/chatgpt25/issue/GAR-593)  
**Branch:** `health/202605121530-gar593-lru-advisory-cleanup`  
**Created:** 2026-05-12 (Florida time)

---

## Goal

Remove the now-stale `RUSTSEC-2026-0002` ignore entry from `.cargo/audit.toml`
and `deny.toml` atomically, following the SYNC NOTE invariant. The advisory
(lru IterMut stacked-borrows violation) is patched in lru ≥ 0.16.3; PR #297
(`fix(security): bump aws-sdk-s3 1.119->1.132 to pull lru 0.16.4`, merged
2026-05-12 as `8f73144`) landed the patch in `Cargo.lock`.

---

## Architecture

Config-only change. No code, no schema, no runtime impact.

---

## Tech Stack

- `.cargo/audit.toml` — cargo-audit ignore list
- `deny.toml` — cargo-deny advisories ignore list

---

## Design Invariants

1. SYNC NOTE invariant: any ID that appears in both files MUST be dropped
   from BOTH atomically in the same PR/commit.
2. Only drop RUSTSEC-2026-0002. The remaining GAR-513 carve-outs (glib
   RUSTSEC-2024-0429, rand RUSTSEC-2026-0097) still have valid upstream
   blockers — do NOT touch them.
3. Update the SYNC NOTE header in both files to reflect the current state.

---

## Out of Scope

- glib (RUSTSEC-2024-0429) — Tauri-side upstream blocker
- rand (RUSTSEC-2026-0097) — phf_codegen build-time dep, no patch
- Any Dependabot version-bump PRs

---

## Rollback

Revert the two config edits. No migration or database change involved.

---

## Open Questions

_None — the fix is deterministic: lru 0.16.4 is in Cargo.lock and patched._

---

## File Structure

```
.cargo/audit.toml          — remove lru block + closure note + update SYNC NOTE
deny.toml                  — remove lru mirror + closure note + update SYNC NOTE
plans/0108-gar-593-lru-advisory-cleanup.md  ← this file (renumbered from 0106; 0106/0107 already taken by GAR-589/GAR-592)
plans/README.md            — add row 0108
```

---

## Tasks

### M1 — Config cleanup

- [x] T1: Update SYNC NOTE in `.cargo/audit.toml` (stale wasmtime/webpki refs)
- [x] T2: Remove lru block + add closure note in `.cargo/audit.toml`
- [x] T3: Update SYNC NOTE in `deny.toml`
- [x] T4: Remove lru mirror + add closure note in `deny.toml`
- [x] T5: Write plan file + update `plans/README.md`
- [ ] T6: Commit + push + open PR
- [ ] T7: Verify CI 18/18 green
- [ ] T8: Merge + mark GAR-593 Done

---

## Risk Register

| Risk | Likelihood | Mitigation |
|---|---|---|
| cargo-deny warns on RUSTSEC-2026-0002 as still-needed | Low | lru 0.16.4 confirmed in Cargo.lock |
| Merge conflict with PR #295 | None | PR #295 already merged; branch rebased |

---

## Acceptance Criteria

- `cargo deny check advisories` passes with no `advisory-not-detected` for RUSTSEC-2026-0002
- `cargo audit --deny unsound` passes (lru 0.16.4 patched)
- CI 18/18 green

---

## Cross-References

- GAR-513: parent tracking issue for glib/lru/rand carve-outs
- GAR-593: this issue
- PR #297 (`8f73144`): lru 0.16.4 landed in main
- PR #295 (`1d0d332`): webpki cleanup (sister health PR, merged same day)
- Plan 0053 PR-6 / GAR-452: original audit.toml cleanup convention

---

## Estimativa

~30 min total (config-only, no code changes).
