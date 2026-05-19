# Plan 0155 — GAR-501: `garra verify` — local idempotent validation pipeline

## Goal

Implement the `garra verify` subcommand in `garraia-cli`: a local, idempotent
validation pipeline that runs `cargo fmt --check`, `cargo clippy`, `cargo test`,
`flutter analyze`, and `gitleaks detect` in sequence, reports per-step status,
and exits with sysexits-compatible codes (0 ok / 2 step-failed).

## Architecture

New module `crates/garraia-cli/src/verify.rs` + new `Commands::Verify` variant in
`main.rs`. Pattern mirrors `config_cmd.rs` (early-intercept before config load,
sysexits exit codes, `--json` + `--strict` flags).

The command is synchronous (no async): each step is a `std::process::Command`
call. Output is streamed (inherited) in human mode; captured in `--json` mode.

## Tech stack

- `std::process::Command` for subprocess invocation — no new deps.
- `serde_json` (already in `garraia-cli`) for `--json` output.
- `serde` + `Serialize` for the report structs.

## Design invariants

1. No `unwrap()` in production paths.
2. Exit code 0 only when all non-skipped steps pass.
3. Exit code 2 on any step failure.
4. `flutter` and `gitleaks` steps auto-skip if the tool is absent — never error.
5. `--skip fmt` (etc.) allows selective opt-out; unknown step names are ignored.
6. Output schema (`--json`) is stable: `{ ok, exit_code, steps: [{name, outcome, ...}] }`.
7. `--exclude garraia-desktop` on all `cargo` steps (no GTK/GDK in dev container).

## Out of scope

- Coverage thresholds (Fase 5.2).
- Mutation testing (Fase 5.2).
- Auto-fix (`cargo fmt` without `--check`).
- Parallelism between steps (deferred; steps share stdout and depend on compile cache).

## Rollback

Revert `src/verify.rs` + the `Commands::Verify` + `mod verify` in `main.rs`. Zero
schema changes; zero new deps.

## File structure

```
crates/garraia-cli/src/verify.rs    ← new module
crates/garraia-cli/src/main.rs      ← +Commands::Verify, +mod verify, intercept
docs/maxpower/verify-schema.json    ← JSON output schema doc
plans/0155-gar-501-garra-verify.md  ← this file
plans/README.md                     ← +row for plan 0155
```

## M1 Tasks

- [x] T1 — Write `verify.rs`: `StepResult`, `StepOutcome`, `run()`, `run_steps()`,
  `print_human()`, `print_json_report()`
- [x] T2 — Wire `Commands::Verify` in `main.rs` (early-intercept like `config check`)
- [x] T3 — Unit tests: `compute_exit_code`, `skip_flag`, JSON serialization shape
- [x] T4 — `docs/maxpower/verify-schema.json` schema doc
- [x] T5 — Update `plans/README.md`
- [x] T6 — `cargo clippy` clean + `cargo test -p garraia`

## Risk register

| Risk | Mitigation |
|------|-----------|
| `cargo test` in CI hits gateway compile via SWAGGER_UI | Steps inherit env; user sets `SWAGGER_UI_DOWNLOAD_URL` if needed |
| Windows `which`-check failure | Use `std::process::Command::output()` probe, not `which` crate |
| JSON output schema drift | `verify-schema.json` is contract; breaking changes require doc update |

## Acceptance criteria

- `garra verify --help` prints all flags without panic.
- `cargo test -p garraia` passes with new unit tests for verify module.
- `cargo clippy --workspace ...` stays clean.
- `--json` flag emits a valid JSON object matching `verify-schema.json`.

## Cross-references

- GAR-501 (Linear): https://linear.app/chatgpt25/issue/GAR-501
- GAR-492 (GarraMaxPower epic)
- Plan 0153 (GAR-494 `garra max-power` skeleton)
- Plan 0154 (GAR-497 bash safety gate)
- Plan 0035 (GAR-379 `config check` — UX model)

## Estimativa

0.5 / 1 / 1.5 days.
