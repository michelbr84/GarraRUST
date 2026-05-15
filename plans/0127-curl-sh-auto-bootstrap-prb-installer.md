# Plan 0127 — `curl | sh` auto-bootstrap **PR-B**: `install.sh` wiring

> **Status:** ⏳ **Draft — in flight 2026-05-14 (Florida).** PR-A merged 2026-05-14 via PR #348 (`6a2279e`), unblocking this slice. §M1 below is now the authoritative implementation checklist.

**Linear issue:** TBD — to be created when PR-A merges.

**Goal:** Update `install.sh` so the one-line `curl -fsSL https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.sh | sh` flow installs the `garraia` binary, then **safely** runs `garraia init </dev/tty` (TTY reopened because stdin is the curl pipe), then runs `garraia start` in the foreground — without breaking non-interactive CI installs.

## Decisions (locked 2026-05-14, same as plan 0126)

1. PR shape — two PRs; this is **PR-B**.
2. Service supervision — `garraia start` in **foreground** after init. Print `Press Ctrl+C to stop. To run later in background: garraia start -d`.
3. Local AI stack scope — wizard already gated in PR-A; installer just runs `garraia init`.
4. Existing config — never silently overwrite; PR-A's wizard handles that.

## Architecture

`install.sh` keeps its existing 5 functions intact (`detect_platform`,
`resolve_version`, `download_and_verify`, `install_binary`, `error`) and
gains one new function:

```
bootstrap_phase
    │
    ├─ if both GARRAIA_SKIP_INIT=1 and GARRAIA_SKIP_START=1
    │     → print_next_steps_legacy; return
    │
    ├─ if /dev/tty unreadable (true non-interactive: docker build / pure CI)
    │     → print_non_interactive_hint; return
    │
    ├─ if GARRAIA_SKIP_INIT ≠ 1
    │     → "${INSTALL_PATH}" init </dev/tty
    │       (failure is non-fatal — fall through to next-steps)
    │
    └─ if GARRAIA_SKIP_START ≠ 1
          → echo "Press Ctrl+C to stop. To run later in background: garraia start -d"
          → exec "${INSTALL_PATH}" start </dev/tty
          (exec replaces shell so Ctrl-C goes to garraia; stdout/stderr
           already inherit the user's terminal — only stdin needs the
           tty redirect)
```

Library mode for tests: at the very bottom, before `main`, the script
honors `GARRAIA_INSTALL_SH_LIBRARY=1` and `return`s instead of calling
`main`. The shell test sources `install.sh` with that flag set, then
invokes `bootstrap_phase` directly with `INSTALL_PATH` pointing at a
stub that echoes its args.

### File layout

```
install.sh                                           [REWRITE — adds bootstrap_phase + library guard]
tests/install_sh/
  bootstrap_phase.sh                                 [NEW — bash test runner]
  fixtures/
    garraia-stub.sh                                  [NEW — echoes args to a log so the test asserts on it]
.github/workflows/ci.yml                             [+1 step — shellcheck install.sh]
README.md                                            [UPDATE — install section reflects auto-bootstrap]
docs/installation.md                                 [UPDATE — env skips documented]
```

`bats-core` is **out of scope** — adding a new test framework just for
`install.sh` is heavier than the value. A plain bash test runner suffices
and aligns with the project's existing `tests/e2e_*.sh` style.

## §M1 — Implementation checklist (subagent-executable tasks)

1. **Bookkeep PR-A as merged** — `plans/README.md` row 0126 flips to
   `✅ Merged 2026-05-14 via PR #348 (6a2279e)`; this plan's status flips
   from `🕐 Blocked` → `⏳ Draft`. (Done as part of PR-B's first commit.)

2. **Refactor `install.sh`** — add `bootstrap_phase`, `print_non_interactive_hint`
   (renames the current "Next steps" echo block into a function), and a
   library-mode guard. Preserve `set -eu`, the existing 5 functions
   verbatim, and the SHA256 verification path. **Gate:** `shellcheck install.sh`
   clean.

3. **Add `tests/install_sh/`** — `bootstrap_phase.sh` test runner + a
   `garraia-stub.sh` fixture. Cases:
   - (a) no `/dev/tty` → prints non-interactive hint, exits 0.
   - (b) `GARRAIA_SKIP_INIT=1 GARRAIA_SKIP_START=1` → prints next-steps,
         neither command invoked.
   - (c) `GARRAIA_SKIP_INIT=1` (only) → start invoked, init skipped.
   - (d) `GARRAIA_SKIP_START=1` (only) → init invoked, start skipped.
   - (e) default (with simulated `/dev/tty`) → init then start invoked
         in that order.
   The stub writes `init <args>` / `start <args>` lines to a log file
   the test asserts against.

4. **Wire `shellcheck`** — add a 5-line job step in `.github/workflows/ci.yml`
   that runs `shellcheck install.sh` (and `bash -n tests/install_sh/*.sh`
   for the new shell tests).

5. **Docs**:
   - `README.md` install section — describe the one-line auto-bootstrap
     and the three skip env vars.
   - `docs/installation.md` — same skip-env documentation in the existing
     "Onboarding wizard" section that PR-A added.

6. **Local validation gates** (matches PR-A's pattern):
   ```
   shellcheck install.sh
   bash -n install.sh
   bash tests/install_sh/bootstrap_phase.sh
   cargo fmt --all -- --check                     # smoke (no Rust changes expected)
   cargo clippy --workspace --exclude garraia-desktop --all-targets -- -D warnings
   cargo test -p garraia --test wizard_smoke      # still green
   ```

7. **Open PR** with title `feat(install.sh): plan 0127 — auto-bootstrap
   wizard + foreground start (PR-B)` and body containing the §Decisions
   block + §Architecture diagram + the validation log. **Wait for CI
   green and explicit user approval before merging.**

## Acceptance criteria

- [ ] `curl -fsSL …/install.sh | sh` on a fresh RunPod GPU pod with a
      TTY: installs binary → wizard runs → `garraia start` foreground.
- [ ] Same command in a `docker build` (no TTY) prints next-steps and
      exits 0; never hangs.
- [ ] `GARRAIA_SKIP_INIT=1` skips wizard; `GARRAIA_SKIP_START=1` skips
      start; both keep current behavior backward compatible.
- [ ] `shellcheck install.sh` clean.
- [ ] `tests/install_sh/bootstrap_phase.sh` covers all 5 cases above
      and runs green in CI.
- [ ] User-explicit approval before merge.

## Cross-references

- Plan 0126 (PR-A) — prerequisite; defines `garraia init` env-detection contract this installer relies on.
- `install.sh` current state — see file at repo root; PR-B keeps `main()`, `detect_platform`, `resolve_version`, `download_and_verify`, `install_binary` intact, adds a new `bootstrap_phase` function.
