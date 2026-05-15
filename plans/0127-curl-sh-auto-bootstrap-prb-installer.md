# Plan 0127 — `curl | sh` auto-bootstrap **PR-B**: `install.sh` wiring

> **Status:** 🕐 **Blocked on plan [0126](0126-curl-sh-auto-bootstrap-pra-wizard.md) (PR-A) merging.** Details below are a sketch — the precise §M1 will be finalized once PR-A is green so the installer can rely on `garraia init`'s new env-detection and config-preservation behavior.

**Linear issue:** TBD — to be created when PR-A merges.

**Goal:** Update `install.sh` so the one-line `curl -fsSL https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.sh | sh` flow installs the `garraia` binary, then **safely** runs `garraia init </dev/tty` (TTY reopened because stdin is the curl pipe), then runs `garraia start` in the foreground — without breaking non-interactive CI installs.

## Decisions (locked 2026-05-14, same as plan 0126)

1. PR shape — two PRs; this is **PR-B**.
2. Service supervision — `garraia start` in **foreground** after init. Print `Press Ctrl+C to stop. To run later in background: garraia start -d`.
3. Local AI stack scope — wizard already gated in PR-A; installer just runs `garraia init`.
4. Existing config — never silently overwrite; PR-A's wizard handles that.

## Scope sketch (to be refined when PR-A merges)

- Preserve existing `install.sh` invariants: platform detect, release resolution, SHA256 verify, install dir safety, sudo handling.
- After `install_binary` succeeds, branch on TTY availability:
  - `/dev/tty` readable → run `garraia init </dev/tty >/dev/tty` (unless `GARRAIA_SKIP_INIT=1`), then `garraia start </dev/tty >/dev/tty` (unless `GARRAIA_SKIP_START=1`).
  - No TTY (true CI / docker build) → print the current "Next steps" message verbatim and exit 0.
- New env toggles:
  - `GARRAIA_SKIP_INIT=1` — skip the wizard.
  - `GARRAIA_SKIP_START=1` — skip the foreground `garraia start`.
  - `GARRAIA_BOOTSTRAP_LOCAL=0` — propagates into the wizard (already implemented in PR-A).
- Add `shellcheck` step to local validation; the GitHub Actions workflow already has shellcheck for `scripts/`, extend the matrix or add an inline `shellcheck install.sh` invocation.
- Tests:
  - `tests/installer/test_install_sh.bats` (NEW) with bats-core under `tests/`. Cases: TTY+skip envs, no-TTY, missing checksum, GARRAIA_VERSION pin.
  - `tests/installer/fixtures/` with a mocked `garraia` script stub (echoes args) so the bats tests exercise the wiring without downloading a real release.
- Docs: README install section gets the same "after install, the wizard runs automatically" paragraph; `docs/installation.md` aligned.

## Acceptance criteria (sketch)

- [ ] `curl -fsSL …/install.sh | sh` on a fresh RunPod GPU pod with a TTY: installs binary → wizard runs → `garraia start` foreground.
- [ ] Same command in a `docker build` (no TTY) prints next-steps and exits 0; never hangs.
- [ ] `GARRAIA_SKIP_INIT=1` skips wizard; `GARRAIA_SKIP_START=1` skips start; both keep current behavior backward compatible.
- [ ] `shellcheck install.sh` clean.
- [ ] bats tests green in CI.
- [ ] User-explicit approval before merge.

## Cross-references

- Plan 0126 (PR-A) — prerequisite; defines `garraia init` env-detection contract this installer relies on.
- `install.sh` current state — see file at repo root; PR-B keeps `main()`, `detect_platform`, `resolve_version`, `download_and_verify`, `install_binary` intact, adds a new `bootstrap_phase` function.
