# Plan 0262 — GAR-789: Drop stale RUSTSEC-2026-0097 (rand 0.7.3) from audit.toml

## Goal

Remove the now-stale `RUSTSEC-2026-0097` suppression entry from
`.cargo/audit.toml`. The advisory targeted `rand 0.7.3` (thread_rng unsound
custom-logger reentrance), which was a build-time-only transitive dep via the
`phf_codegen → phf_generator → selectors → tauri-utils` chain.
`phf_generator 0.13.x` switched from `rand` to `fastrand`, severing this chain.
Cargo.lock now carries only `rand 0.8.6 / 0.9.4 / 0.10.1` — rand 0.7.3 is gone.

Keeping a dead suppress entry violates audit.toml policy: *"Every entry in
`ignore` is a REAL advisory against a dep in our Cargo.lock."*

## Architecture

No code change. Config-only cleanup:

- `.cargo/audit.toml` — remove the 7-line `RUSTSEC-2026-0097` block from
  `ignore = [...]`; update the `audit.toml-ONLY residuals` clause in the SYNC
  NOTE to remove RUSTSEC-2026-0097.
- `deny.toml` — update the NOTE at the RUSTSEC-2026-0097 line to reflect it is
  also removed from audit.toml; update the SYNC NOTE closed-history list.

## Tech stack

cargo-audit, cargo-deny, CI (Security Audit + cargo-deny steps).

## Design invariants

- `cargo audit --deny unsound` must remain green after the change (the entry
  being removed no longer matches any crate in Cargo.lock — confirmed by cargo
  audit CI pass on 2026-06-04 SHA `1f501ea` where rand 0.7.3 absent).
- `cargo deny check advisories` CI step must remain green.
- No Cargo.toml / Cargo.lock changes.
- No schema migrations, no code changes.

## Out of scope

- `RUSTSEC-2024-0429` (glib 0.18.5 — still in Cargo.lock, still needed in
  audit.toml). Untouched.
- `RUSTSEC-2023-0071` (rsa 0.9.10 — still in Cargo.lock via jsonwebtoken).
  Untouched.

## Rollback

Revert commits on the health/ branch. No data migration required.

## Open questions

None.

## File Structure

```
.cargo/audit.toml             — remove RUSTSEC-2026-0097 block + update SYNC NOTE
deny.toml                     — update NOTE + SYNC NOTE closed-history
plans/0262-gar-789-drop-stale-rustsec-2026-0097.md  (this file)
plans/README.md               — add row 0262
```

## M1 Tasks

- [x] T1: Remove `RUSTSEC-2026-0097` block from `.cargo/audit.toml` + update SYNC NOTE
- [x] T2: Update `deny.toml` NOTE (mark also removed from audit.toml) + SYNC NOTE
- [x] T3: Add `plans/0262` + `plans/README.md` row
- [x] T4: Push branch, open PR, wait for CI green, squash-merge

## Risk register

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| Removing entry breaks cargo audit | Low | CI red | Empirically confirmed: rand 0.7.3 absent from Cargo.lock |

## Acceptance criteria

- `cargo audit` CI (Security Audit job) green on PR
- `cargo deny check advisories` CI green on PR
- `RUSTSEC-2026-0097` no longer appears anywhere in `.cargo/audit.toml`
- PR squash-merged to main

## Cross-references

- GAR-789 (this issue — In Progress)
- GAR-776 (identified the fix on 2026-06-02, never shipped)
- GAR-513 (glib+rand carve-outs tracker — glib portion remains, due 2026-07-31)

## Estimativa

0.5 SP — config-only, no code.
