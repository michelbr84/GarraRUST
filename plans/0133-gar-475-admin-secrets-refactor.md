# Plan 0133 — Q9.f: extract admin/secrets.rs (GAR-475)

**Status:** Done
**Issue:** [GAR-475](https://linear.app/chatgpt25/issue/GAR-475)
**Branch:** `routine/202605160020-q9f-secrets-rs`
**Epic parent:** GAR-439 / GAR-430

---

## Goal

Extract all secrets-management code (CRUD, rotation, migration, AES-256-GCM helpers,
and config-secret redaction) from `admin/handlers.rs` into a new focused module
`admin/secrets.rs`. Zero behaviour change; re-exports in `handlers.rs` keep every
`routes.rs` call-site unchanged.

`handlers.rs` shrinks from **1738 → ~1300 LOC** (−~440 lines).

---

## Architecture

Follows the pattern established in slices 9.b–9.g:

1. New file `crates/garraia-gateway/src/admin/secrets.rs` — owns all secrets logic.
2. `handlers.rs` gets a `pub use super::secrets::{...}` re-export block.
3. `handlers.rs` gets a private `use super::secrets::redact_config_secrets;` import
   for use in `get_config` and `export_config` (which stay in handlers.rs).
4. `admin/mod.rs` gains `pub mod secrets;`.

---

## Functions extracted

| Function / Type | Visibility in secrets.rs |
|---|---|
| `SetSecretRequest` | `pub` |
| `default_tenant` | `fn` (private) |
| `set_secret` | `pub async fn` |
| `list_secrets` | `pub async fn` |
| `delete_secret` | `pub async fn` |
| `test_secret` | `pub async fn` |
| `list_secret_versions` | `pub async fn` |
| `encrypt_value` | `fn` (private) |
| `decrypt_value` | `fn` (private) |
| `redact_config_secrets` | `pub(super) fn` |
| `RotateSecretRequest` | `pub` |
| `rotate_secret` | `pub async fn` |
| `migrate_secrets` | `pub async fn` |

---

## Security invariants (must be preserved)

1. `encrypt_value` / `decrypt_value` stay private — callers must go through the
   public handler surface.
2. `test_secret` returns only `value_length` (integer), never the decrypted bytes.
3. `redact_config_secrets` must keep masking `api_key`, LLM keys, channel tokens/keys/secrets/passwords.
4. No new `tracing::error!` / `eprintln!` that could expose `encryption_key` bytes.
5. `encryption_key` is consumed by reference only; never cloned or logged.

---

## Out of scope

- Any functional change to secrets logic.
- Adding new tests (zero behaviour = same coverage).
- Touching routes.rs.

---

## M1 tasks

- [x] Create `plans/0133-gar-475-admin-secrets-refactor.md`
- [x] Mark GAR-475 In Progress in Linear
- [x] Create `admin/secrets.rs` with all extracted code
- [x] Update `admin/handlers.rs` (remove extracted code, add re-exports + private import)
- [x] Update `admin/mod.rs` (add `pub mod secrets;`)
- [x] `cargo check -p garraia-gateway` green
- [x] `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` clean
- [x] Commit + push
- [x] Open PR + wait for CI green
- [x] Merge + mark GAR-475 Done
- [x] Update plans/README.md (0132 ✅ + 0133 ✅)

---

## Acceptance criteria

- New file `admin/secrets.rs` with all secrets handlers + helpers.
- `handlers.rs` LOC ≤ 1320.
- `cargo check -p garraia-gateway` and clippy clean.
- CI 100% green.
- No additional exposure of `encryption_key` in logs / Debug impls / error messages.
- PR ≤ 550 LOC diff.

---

## Risk register

| Risk | Likelihood | Mitigation |
|---|---|---|
| Missing re-export breaks routes.rs | Low | `cargo check` catches at compile time |
| `redact_config_secrets` not imported in handlers.rs | Low | Compiler error immediate |
| Crypto helpers leak key bytes via new log | Low | Review every new `Err(e)` path in secrets.rs |

---

## Cross-references

- GAR-439 (epic: admin/handlers.rs refactor series)
- GAR-430 (umbrella: Quality Gates Phase 3.6)
- Prior: plan 0132 / GAR-474 (Q9.g users.rs)
- plans/0128-0131 — Q9.b–9.e pattern reference
