# Plan 0073 — GAR-527: openssl 0.10.78 → 0.10.79 security patch

**Linear issue:** [GAR-527](https://linear.app/chatgpt25/issue/GAR-527) — "openssl 0.10.78 → 0.10.79 security patch (Dependabot PR #166 follow-up)"

**Status:** ✅ Merged — PR #168 (`8e41201`), 2026-05-06 (Florida).

**Goal:** Apply the openssl 0.10.78 → 0.10.79 + openssl-sys 0.9.114 → 0.9.115 lockfile-only bump that was identified as a Dependabot security update (GH Dependabot PR #166). The grouped PR was intentionally closed by the repo owner because it included a breaking rand 0.8/0.9 → 0.10 major-version upgrade; the owner explicitly requested a narrower follow-up PR with just the openssl patch.

**Architecture:** Lockfile-only change (`Cargo.lock`). No Cargo.toml edits; openssl is a transitive dependency only. The `cargo update -p openssl --precise 0.10.79` command also bumps `openssl-sys` 0.9.114 → 0.9.115 automatically (they are versioned together).

**Tech stack:** Rust / Cargo lockfile. No code changes.

**Design invariants:**
1. Cargo.toml is NOT modified — openssl is not a direct dependency in any workspace member.
2. `cargo audit` and `cargo deny` must remain green after the bump.
3. No other packages are updated by this PR (verified via `cargo update --dry-run`).

**Out of scope:**
- rand 0.8/0.9 → 0.10 major upgrade (separate planned migration, GAR-437/GAR-25 sub-tracking).
- openssl-sys version pin in Cargo.toml.
- Any code changes beyond Cargo.lock.

**Rollback:** `git revert` the single lockfile commit is fully sufficient.

---

## §12 Open questions

| # | Question | Decision |
|---|---|---|
| Q1 | Is the openssl advisory already in `audit.toml` allowlist? | No — not present (confirmed by reading `.cargo/audit.toml`). This bump should close the advisory cleanly. |
| Q2 | Does the bump break any workspace crate? | No — dry-run showed only openssl+openssl-sys change; `cargo build` confirms. |

---

## File structure

- `Cargo.lock` — 2 version lines changed (openssl 0.10.78→0.10.79, openssl-sys 0.9.114→0.9.115)
- `docs/security/dependabot-status.md` — add snapshot update row for 2026-05-06
- `plans/0073-gar-527-openssl-0.10.79.md` — this file
- `plans/README.md` — new row for plan 0073

---

## Tasks (M1)

- [x] T1: Create health/ branch `health/202505061650-openssl-0.10.79` from main
- [x] T2: Create this plan file + README row + commit `docs(plans): add plan 0073 for GAR-527 openssl 0.10.79 security patch`
- [x] T3: Run `cargo update -p openssl --precise 0.10.79`; verify only openssl+openssl-sys change via `git diff Cargo.lock`
- [x] T4: Run `cargo build --workspace --exclude garraia-desktop` to confirm no compilation errors
- [x] T5: Run `cargo test --workspace --exclude garraia-desktop --no-run` to confirm test artifacts compile
- [x] T6: Run `cargo audit --no-fetch` to confirm openssl advisory no longer fires (or remains in allowlist if advisory not yet in local DB)
- [x] T7: Commit `fix(deps): GAR-527 — bump openssl 0.10.78→0.10.79 + openssl-sys 0.9.114→0.9.115 (security patch)`
- [x] T8: Update `docs/security/dependabot-status.md` snapshot row; commit
- [x] T9: Push + open PR via GitHub MCP; poll CI until green; squash-merge
- [x] T10: Mark GAR-527 Done in Linear; update plans/README.md row

---

## Risk register

| Risk | Mitigation |
|---|---|
| openssl-sys bump pulls in a new system lib ABI | openssl-sys 0.9.x is backwards-compatible with OpenSSL 1.1/3.x; standard patch bump |
| CI flake on windows/macos | Retry; no code change means only environment noise |
| rand breakage accidentally pulled in | Confirmed via dry-run that only openssl+openssl-sys change; other crates pinned |

---

## Acceptance criteria

1. `Cargo.lock` shows `openssl = "0.10.79"` and `openssl-sys = "0.9.115"`.
2. CI: Format, Clippy, Test×3, Build, MSRV, cargo-deny, Security Audit, Coverage, Analyze(rust), Analyze(js-ts), Playwright, E2E, Secret Scan, Dependency Review — all green.
3. `cargo audit` green (advisory auto-closes or never fires locally).
4. `docs/security/dependabot-status.md` updated.
5. GAR-527 marked Done.

---

## Cross-references

- Dependabot PR #166 (closed without merge — rationale in comment)
- `docs/security/dependabot-status.md` — dependabot owner map
- GAR-437 / GAR-25 — rand upgrade tracking (separate, out of scope here)
- GAR-484 — previous openssl triage (0.10.75→0.10.78 in PR #99)

## Estimativa

T3–T8: ~15 min. T9 (CI wait): ~15–20 min. Total: ~35 min.
