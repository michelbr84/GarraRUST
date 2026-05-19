# Plan 0154 — GAR-497: Bash Safety Gate — centralize denylist in `garraia-common`

**Issue:** [GAR-497](https://linear.app/chatgpt25/issue/GAR-497) (parent: GAR-492 GarraMaxPower epic)
**Branch:** `routine/202605191215-gar-497-bash-safety-gate` (off `main`)
**Date:** 2026-05-19 (Florida)
**Scope:** `crates/garraia-common` + `crates/garraia-agents/src/tools/bash_tool.rs` — no gateway, no DB, no migrations.
**Labels:** `security`, `epic:superpowers`
**Priority:** Urgent

## Goal

Centralize bash command safety checking into `garraia_common::safety_gate` so that **all** tools
(not just `BashTool`) can enforce the denylist consistently. Refactor `bash_tool.rs` to delegate to
the new central function. Expand coverage to ≥ 30 table-driven test cases including evasion attempts.

The existing `BashTool::DENY_LIST` + `CONFIRM_LIST` in `crates/garraia-agents/src/tools/bash_tool.rs`
is the source of truth to extract from — verified empirically in this plan.

## Architecture

```
crates/garraia-common/src/
  safety_gate.rs    ← NEW: SafetyDenied enum + safety_gate() + DENY_LIST + CONFIRM_LIST
  lib.rs            ← add `pub mod safety_gate;`

crates/garraia-agents/src/tools/bash_tool.rs
  → DENY_LIST / CONFIRM_LIST constants removed (kept inline comments)
  → is_dangerous() delegates to safety_gate::safety_gate()
  → is_risky()     delegates to safety_gate::is_risky()
```

No new crate deps — `garraia-common` has no project deps, so no cycle risk.
`garraia-agents` already depends on `garraia-common`.

## Public API (garraia_common::safety_gate)

```rust
/// Error returned when a command is rejected by the safety gate.
///
/// The error message never includes the raw command string to avoid leaking
/// secrets that might appear in positional arguments (passwords, tokens, API keys).
/// Only the matched `pattern` (a static string from the denylist) is reported.
#[derive(Debug, Error, PartialEq, Clone)]
pub enum SafetyDenied {
    /// Hard-blocked: command matches a pattern in the destructive denylist.
    #[error("command blocked by safety gate: matched pattern '{pattern}'")]
    DangerousCommand { pattern: &'static str },

    /// Risky: command requires explicit user confirmation before execution.
    #[error("command requires user confirmation: matched pattern '{pattern}'")]
    RequiresConfirmation { pattern: &'static str },
}

/// Check a raw bash/shell command against the safety denylist.
///
/// Returns `Ok(())` if the command is safe to execute, or `Err(SafetyDenied)`
/// if it matches a dangerous or risky pattern.
///
/// Matching is case-insensitive substring match on the full command string.
/// The first match short-circuits: `DangerousCommand` takes priority over
/// `RequiresConfirmation`.
pub fn safety_gate(cmd: &str) -> Result<(), SafetyDenied>

/// Check only the risky tier (confirmation-required patterns).
///
/// Returns `Ok(())` if the command is NOT risky, or `Err(SafetyDenied::RequiresConfirmation)`.
/// Used when the caller already passed `safety_gate()` (hard block) and wants to
/// check if confirmation is needed.
pub fn is_risky(cmd: &str) -> Result<(), SafetyDenied>
```

## Denylist (expanded from bash_tool.rs)

### DENY_LIST (hard block — DangerousCommand)

| Pattern | Rationale |
|---|---|
| `rm -rf /` | Delete entire filesystem |
| `rm -r /` | Variant |
| `rm -f /` | Variant |
| `rm -rf ~` | Delete home directory |
| `rm -rf $home` | Delete home (env var) |
| `rm -rf ${home}` | Delete home (braced) |
| `:(){ :|:& };:` | Fork bomb |
| `format c:` | Windows disk format |
| `format d:` | Windows disk format |
| `format e:` | Windows disk format |
| `format f:` | Windows disk format |
| `diskpart` | Windows disk management |
| `fdisk` | Linux disk management |
| `mkfs` | Filesystem creation |
| `dd if=` | Low-level disk write |
| `> /dev/sd` | Direct write to device |
| `chmod 777 /` | World-writable root |
| `chown -r` | Recursive ownership change |
| `curl \| sh` | Pipe-exec from URL |
| `curl \| bash` | Pipe-exec from URL |
| `wget \| sh` | Pipe-exec from URL |
| `wget \| bash` | Pipe-exec from URL |
| `nc -` | Netcat (reverse shell) |
| `netcat` | Netcat |
| `nmap` | Port scanner |
| `ssh root@` | Root SSH session |
| `sudo su` | Escalate to root |
| `kill -9 -1` | Kill all processes |
| `pkill -9` | Force kill processes |
| `reboot` | System reboot |
| `shutdown` | System shutdown |
| `init 0` | System halt |
| `init 6` | System reboot |
| `halt` | System halt |
| `poweroff` | Power off system |
| `git push --force origin main` | Force push to protected branch |
| `git push --force-with-lease origin main` | Variant |
| `git push -f origin main` | Variant |
| `python -m http` | Python HTTP server |

### CONFIRM_LIST (risky — RequiresConfirmation)

| Pattern | Rationale |
|---|---|
| `rm -r` | Recursive delete (not root) |
| `del /s` | Windows recursive delete |
| `del /f` | Windows force delete |
| `rd /s` | Windows remove dir recursively |
| `git reset --hard` | Discard all local changes |
| `git push --force` | Force push (any branch) |
| `git push -f` | Variant |
| `git clean -f` | Delete untracked files |
| `drop table` | SQL destructive |
| `drop database` | SQL destructive |
| `drop schema` | SQL destructive |
| `truncate table` | SQL destructive |
| `truncate ` | SQL truncate (with space) |
| `delete from` | SQL delete (no WHERE check) |
| `kill ` | Process kill |
| `taskkill` | Windows process kill |
| `stop-process` | PowerShell process kill |
| `remove-item -recurse` | PowerShell recursive delete |
| `remove-item -r` | PowerShell recursive delete |

## Design invariants

1. **No raw command in error messages.** Commands may contain API keys, passwords, or tokens
   in positional arguments. The error only reports the matched `pattern` (a static string).
2. **Case-insensitive substring match.** Both pattern and command are lowercased before comparison.
3. **DangerousCommand takes priority over RequiresConfirmation.** `safety_gate()` checks
   DENY_LIST first; if the command is already in DENY_LIST, it never falls through to CONFIRM_LIST.
4. **No regex dependency.** Plain `str::contains()` is sufficient for the MVP denylist.
   Regex support is deferred to a future slice.
5. **False-positive rate = 0 for specified safe commands.** The plan includes explicit
   test cases for `cargo test`, `git status`, `ls -la`, `git push origin feature-branch`,
   `cargo build`, `date`, `echo hello`, `curl https://example.com/file.json` (no pipe).
6. **bash_tool.rs refactored — no behavior change.** The existing tests in bash_tool.rs must
   still pass after the refactor. Only the implementation delegate changes, not the interface.

## Out of scope

- WASM sandbox for tools (Fase 2.2, separate plan)
- AppArmor/seccomp profiles (Fase 5)
- Regex-based pattern matching (future slice)
- `shell_inject` detection (XSS-style injection via `$(...)` / backticks) — deferred
- Integration with garraia-gateway or garraia-workspace

## Rollback

All changes are additive (new file in garraia-common) + refactor (bash_tool.rs delegates).
Rollback = revert the 3-4 commits of this plan. No migration, no schema change.

## Task list (M1)

- [x] **T1** — Create `plans/0154-gar-497-bash-safety-gate.md` (this file)
- [ ] **T2** — Implement `crates/garraia-common/src/safety_gate.rs`:
  - `SafetyDenied` enum (`DangerousCommand` + `RequiresConfirmation`)
  - `DENY_LIST` and `CONFIRM_LIST` constants (extracted + expanded from bash_tool.rs)
  - `pub fn safety_gate(cmd: &str) -> Result<(), SafetyDenied>`
  - `pub fn is_risky(cmd: &str) -> Result<(), SafetyDenied>`
  - `#[cfg(test)] mod tests { ... }` with ≥ 30 table-driven cases
  - `pub use safety_gate::{SafetyDenied, safety_gate, is_risky};` in lib.rs
- [ ] **T3** — Refactor `crates/garraia-agents/src/tools/bash_tool.rs`:
  - Replace inline `DENY_LIST` + `CONFIRM_LIST` with delegation to `garraia_common::safety_gate`
  - `is_dangerous()` → calls `safety_gate::safety_gate(cmd).is_err()`
  - `is_risky()` → calls `safety_gate::is_risky(cmd).is_err()`
  - Keep all existing tests passing (zero behavior change)
  - Add 2 tests: one for `DangerousCommand` error variant, one for `RequiresConfirmation` variant
- [ ] **T4** — Update `plans/README.md` with plan 0154 row
- [ ] **T5** — `cargo check -p garraia-common && cargo check -p garraia-agents`
- [ ] **T6** — `cargo test -p garraia-common && cargo test -p garraia-agents`
- [ ] **T7** — `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings`

## Risk register

| Risk | Mitigation |
|---|---|
| False positive on `sh -c` pattern | Already in DENY_LIST; test `echo sh -c hello` to confirm match is on `sh -c` specifically |
| `chown -R` blocks legitimate `chown -R www-data /var/www` | Accept — per GAR-497 this is in DENY_LIST; agents should use more specific patterns |
| Order of DENY_LIST vs CONFIRM_LIST | `safety_gate()` always checks DENY first; test with commands in both |

## Acceptance criteria

- [ ] `cargo test -p garraia-common` → ≥ 30 tests pass in `safety_gate` module
- [ ] `cargo test -p garraia-agents` → all existing bash_tool tests still pass
- [ ] `git grep 'DENY_LIST\|CONFIRM_LIST' crates/garraia-agents/` returns zero hits after refactor
- [ ] `cargo clippy --workspace -- -D warnings` → zero warnings
- [ ] Error messages from `SafetyDenied` do NOT contain the raw command string (verified by test)
- [ ] @security-auditor agent review completed before merge

## Cross-references

- GAR-492 (epic parent)
- GAR-494 (garra max-power skeleton, sibling — adds subcommand)
- CLAUDE.md §"Regras absolutas" rules 2 (no unwrap) + 6 (no secrets in logs)
- ADR 0009 (GarraMaxPower — ADR still Proposed, not yet Accepted; safety gate is an impl detail)
- `crates/garraia-learning/src/safety.rs` — related but different scope (skill body safety, not bash commands)

## Estimativa

- T2 (impl): ~150 LOC. T3 (refactor): ~20 LOC change. T4: 1 row. T5-T7: run commands.
- Wall time: ~30-45 min.
