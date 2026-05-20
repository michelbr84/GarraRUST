# Plan 0157 — GAR-500: Auto Dream / Handoff via `.garra-estado.md`

**Issue:** [GAR-500](https://linear.app/chatgpt25/issue/GAR-500)
**Epic:** [GAR-492](https://linear.app/chatgpt25/issue/GAR-492) — GarraMaxPower
**Branch:** `routine/202005200619-gar-500-auto-dream-handoff`
**Date (Florida):** 2026-05-20
**Status:** ✅ Done — merged PR #445 (`f1fb596`) 2026-05-20

---

## Goal

Persist the GarraMaxPower pipeline state between sessions: last action performed,
next action suggested, current plan reference, active Linear issue, git branch.
The file is read at `garra max-power` startup and printed as a "handoff summary"
so a new session can continue where the previous one left off.

---

## Architecture

A new module `garraia_common::handoff` provides:
- `HandoffState` — the serialisable struct (TOML). Only allow-listed fields
  (no message bodies, no user PII by construction).
- `RedactedString` newtype — can only be created via `RedactedString::new(raw)`
  which applies `redact()` before storing. Provides compile-time + runtime
  enforcement that description strings are always scrubbed before storage.
- `redact(s: &str) -> String` — strips emails, JWT-shaped tokens, home paths,
  truncates to 500 chars.
- `load(path: &Path) -> Result<HandoffState, HandoffError>` — reads `.garra-estado.md`
  (actually a TOML file despite the `.md` extension, for human readability).
- `save(state: &HandoffState, path: &Path) -> Result<(), HandoffError>` — atomic write.

The CLI `max_power::run()` calls `handoff::load()` at startup and prints the
last-action / next-action summary when the file exists.

---

## Tech stack

- Rust / serde + toml — serialisation
- `garraia-common` crate — module location
- `garraia-cli` — wiring at `garra max-power` startup

---

## Design invariants

1. **No PII by construction** — `HandoffState` has no free-form `String` field
   outside of `RedactedString`. The struct layout is the allow-list.
2. **Compile-time + runtime** — you cannot store raw user text in `description`
   without going through `RedactedString::new()`, which always redacts.
3. **Fail-closed** — missing or corrupt `.garra-estado.md` is gracefully ignored
   (startup continues, no state displayed).
4. **Atomic write** — `save()` writes to a `.tmp` sibling and renames.
5. **`.gitignore`** — `.garra-estado.md` is ignored by default; opt-in to track it.

---

## Validações pré-plano

- [x] `garraia-common` has `serde` + `chrono` deps → no new transitive deps needed
- [x] `toml = "1.0"` is already a workspace dep → just add to `garraia-common` deps
- [x] No DB/network required → pure file I/O
- [x] `max_power.rs` entrypoint exists → easy wiring

---

## Out of scope

- Remote sync of handoff (Fase 7)
- Multi-user handoff (Group Workspace)
- JSON output format (TOML is canonical; JSON export is a future flag)
- Full state-machine execution (that's GAR-495..GAR-499)

---

## Rollback

Delete `crates/garraia-common/src/handoff.rs`, revert `lib.rs`, revert
`max_power.rs`, revert Cargo.toml. No schema migration needed.

---

## File Structure

```
crates/garraia-common/
  src/
    handoff.rs        ← NEW: HandoffState + RedactedString + redact() + load/save
    lib.rs            ← amend: pub mod handoff + re-exports
  Cargo.toml          ← amend: add toml workspace dep
crates/garraia-cli/
  src/
    max_power.rs      ← amend: load handoff at startup, print summary
.gitignore            ← amend: add .garra-estado.md
docs/maxpower/
  handoff-schema.md   ← NEW: TOML schema documentation
plans/README.md       ← amend: add row 0157
ROADMAP.md            ← amend: §7 note
```

---

## M1 Tasks

- [x] T1: `crates/garraia-common/src/handoff.rs` — types + redact + load/save + tests
- [x] T2: `crates/garraia-common/src/lib.rs` — export handoff module
- [x] T3: `crates/garraia-common/Cargo.toml` — add `toml = { workspace = true }`
- [x] T4: `crates/garraia-cli/src/max_power.rs` — wire load + summary print at startup
- [x] T5: `docs/maxpower/handoff-schema.md` — schema documentation
- [x] T6: `.gitignore` — add `.garra-estado.md`
- [x] T7: `plans/README.md` — add row 0157
- [x] T8: `ROADMAP.md` — §7 status note (done above in bookkeeping commit)

---

## Risk register

| Risk | Mitigation |
|------|-----------|
| TOML parse error on corrupt file | `load()` returns `Err` which caller ignores (fail-closed) |
| `.md` extension confused for Markdown | Comment at top of file clarifies TOML format |
| Redaction regex too aggressive | Conservatively truncate only, never replace valid branch names / plan IDs |
| Windows path separator in redaction | Use `std::path` to detect paths, not raw regex on `\` |

---

## Acceptance criteria

- `cargo test -p garraia-common -- handoff` → all green (round-trip + redaction tests)
- `HandoffState { description: "raw string".to_string() }` does NOT compile (field is `RedactedString`)
- `garra max-power` with a valid `.garra-estado.md` prints "Última ação: ... | Próxima: ..."
- `garra max-power` with missing `.garra-estado.md` starts cleanly with no error
- `.garra-estado.md` is in `.gitignore`

---

## Cross-references

- GAR-494 (max-power skeleton) — skeleton that this wires into
- GAR-492 (epic) — GarraMaxPower umbrella
- ADR 0009 — referenced in issue but actually covers web console design; handoff uses TOML
- `plans/0154-gar-497-bash-safety-gate.md` — same epic, safety gate (Done)
- `plans/0155-gar-501-garra-verify.md` — same epic, garra verify (Done)

---

## Estimativa

0.5 / 1 / 1.5 days (small focused module, no external deps).
