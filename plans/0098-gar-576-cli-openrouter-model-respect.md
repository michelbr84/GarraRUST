# Plan 0098 — GAR-576: CLI respects configured OpenRouter model + `default_provider`

> Renumbered from 0097 to 0098 on 2026-05-11 after
> `plans/0097-gar-574-groups-members-invites-api.md` landed via PR #261
> while this branch was in flight.

**Status:** Em execução
**Autor:** Claude Opus 4.7 (sessão interativa 2026-05-10, America/New_York)
**Data:** 2026-05-10 (America/New_York)
**Issue:** [GAR-576](https://linear.app/chatgpt25/issue/GAR-576/cli-respect-configured-openrouter-model-and-default-provider)
**Branch:** `routine/202605102242-gar-576-cli-openrouter-model-respect`
**Epic:** `epic:cli`
**Parent:** —

---

## §1 Goal

Fix two interlocking CLI bugs in `garra chat`:

1. `garra chat --provider openrouter` ignores `config.llm["openrouter"].model` and hardcodes `openrouter/auto` (chat.rs:306).
2. `garra chat` (autodetect) walks a hardcoded chain (Ollama health → Anthropic env → OpenAI env → OpenRouter env) and never consults `config.agent.default_provider` (which already exists in `crates/garraia-config/src/model.rs:446`). With `OPENAI_API_KEY` loaded from cwd `.env` via `dotenvy::dotenv()` (main.rs:522), OpenAI wins even when the operator explicitly configured OpenRouter as the primary provider.

Operational policy locked by user (2026-05-10):

- **`openrouter/free`** is the default for smoke tests and cheap validations.
- **`openrouter/auto`** is reserved for real / complex tasks — only when the operator passes `--model openrouter/auto` explicitly.

---

## §2 Resolution rules

### Provider resolution precedence

1. `--provider <X>` CLI flag (absolute).
2. `config.agent.default_provider` (NEW: read as a lookup key into `config.llm[*]`).
3. Hardcoded autodetect chain (compat).

### Model resolution precedence (per provider kind)

1. `--model <X>` CLI flag (absolute).
2. `config.llm[provider_kind].model` (key-match).
3. First `config.llm[*]` entry whose `provider` field equals `provider_kind` and has `model = Some(_)`.
4. Hardcoded default per kind (`openrouter/auto`, `gpt-4o`, `claude-sonnet-4-5-20250929`, `llama3.1`).

---

## §3 Design invariants

1. **Pure helpers.** `resolve_provider_model` and `decide_default_provider` are synchronous and do **not** read `std::env` directly — env presence is passed in via parameters, so unit tests need zero env mutation and zero new dev-dependencies.
2. **Backwards compatible.** Without `config.agent.default_provider`, autodetect chain is byte-identical to today's behavior.
3. **No new runtime dependency.** Zero changes to `Cargo.toml` (workspace or `garraia-cli`).
4. **No secret exposure.** No new log lines. No `Display`/`Debug` impl change that could leak `api_key`.
5. **Single-PR scope.** No `garra ask`, no MCP wrapper, no `RedactingWriter` extension, no `.env` edits.
6. **Smoke uses `openrouter/free` only.** `openrouter/auto` is only invoked manually if `free` fails (and the failure reason is explained before retry).

---

## §4 Code changes

### `crates/garraia-cli/src/chat.rs`

**(A) New pure helper `resolve_provider_model`** (after `get_api_key`, ~25 LOC):

```rust
fn resolve_provider_model(
    config: &AppConfig,
    provider_kind: &str,
    model_override: Option<&str>,
) -> Option<String> {
    if let Some(m) = model_override {
        return Some(m.to_string());
    }
    if let Some(cfg) = config.llm.get(provider_kind)
        && let Some(m) = cfg.model.as_deref()
        && !m.is_empty()
    {
        return Some(m.to_string());
    }
    for cfg in config.llm.values() {
        if cfg.provider == provider_kind
            && let Some(m) = cfg.model.as_deref()
            && !m.is_empty()
        {
            return Some(m.to_string());
        }
    }
    None
}
```

**(B) New pure helper `decide_default_provider`** (~40 LOC) — returns a `DefaultProviderDecision` enum that says either "use this config_key" or "fall through, reason=...". Tests assert against the enum; production then async-constructs the `LlmProvider` impl.

**(C) Refactor `match p.as_str()` branches in `run_chat`** (4 branches, ~16 LOC delta): each branch replaces `model_override.unwrap_or_else(|| "<hardcoded>".to_string())` with `resolve_provider_model(&config, "<kind>", model_override.as_deref()).unwrap_or_else(|| "<hardcoded>".to_string())`.

**(D) New branch at top of `detect_provider`** (~30 LOC) — calls `decide_default_provider`; on `UseDefault`, constructs the corresponding `LlmProvider` (Ollama health-checked, others built directly). Falls through to legacy chain on `FallThroughToChain` or construction failure.

### Tests (in `chat.rs::tests`)

| # | Name | Asserts |
|---|------|---------|
| 1 | `resolve_provider_model_override_wins` | `--model` wins over everything |
| 2 | `resolve_provider_model_key_match` | `llm["openrouter"].model = "openrouter/free"` is returned |
| 3 | `resolve_provider_model_provider_field_match` | `llm["my-router"].provider = "openrouter"` resolves when key name differs from kind |
| 4 | `resolve_provider_model_no_match` | empty config returns `None` |
| 5 | `resolve_provider_model_empty_string_skipped` | empty `model: ""` is not considered |
| 6 | `decide_default_provider_no_default_falls_through` | no `agent.default_provider` → FallThrough |
| 7 | `decide_default_provider_missing_llm_key_falls_through` | `default_provider = "missing"` → FallThrough |
| 8 | `decide_default_provider_openrouter_wins_over_openai_env` | `default_provider = "openrouter"` + `env_has_openai=true` → UseDefault(openrouter) — the regression test for the OPENAI_API_KEY hijack |
| 9 | `decide_default_provider_falls_through_when_no_credential` | `default_provider = "openrouter"` + no env + no `cfg.api_key` → FallThrough |
| 10 | `decide_default_provider_ollama_no_credential_needed` | `provider: "ollama"` doesn't need an api key (health-check happens later) |
| 11 | `decide_default_provider_openai_compat_with_base_url_accepts_no_key` | LM Studio scenario: `provider: "openai"` + `base_url: Some(...)` → UseDefault even without key |

### `docs/configuration.md`

Add a new section "Provider/model resolution precedence" with both precedence ladders from §2 above, plus the policy note about `openrouter/free` vs `openrouter/auto`.

### `docs/src/providers.md`

Add a "Política de teste — `openrouter/free` vs `openrouter/auto`" subsection at the bottom of the OpenRouter section (around line 158).

---

## §5 Verification

Mandatory commands:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

Smoke (manual, cheap — `openrouter/free` only):

```bash
printf 'Responda apenas: GAR-FREE-OK\n/exit\n' \
  | timeout 90 ./target/release/garra.exe chat --provider openrouter --model openrouter/free 2>&1 \
  | sed -E 's/sk-or-v1-[A-Za-z0-9_-]+/sk-or-v1-[REDACTED]/g; s/sk-[A-Za-z0-9_-]+/sk-[REDACTED]/g'
```

Acceptance:

- Banner shows `Model: openrouter/free` (NOT `openrouter/auto`) when only `--provider` is passed and config declares `model: openrouter/free`.
- `--model openrouter/auto` still routes to `openrouter/auto`.
- All cargo verification commands exit 0.
- No new dependency in `Cargo.toml` (`git diff Cargo.toml` and `git diff crates/garraia-cli/Cargo.toml` are empty).

---

## §6 Risks

- **Behavior change for users with `model: <something>` in the openrouter config block.** Today they silently get `openrouter/auto`; after this PR they get what they configured. This is the intentional fix. Documented in the PR description.
- **Env-test isolation.** Avoided by passing env presence as bool parameters to `decide_default_provider`. No `std::env::set_var` in tests.
- **No clippy regression.** New `let-chains` (`if let … && let …`) are already used elsewhere in `chat.rs` (e.g. `get_api_key` lines 133-154) so the workspace edition supports them.

---

## §7 Out of scope (follow-ups)

- `garra ask` non-interactive command — separate issue.
- MCP wrapper over CLI — blocked by `garra ask`.
- `RedactingWriter` extension to cover provider error payloads (OpenAI 401 leaks key fingerprint to stderr) — separate issue, requires `security-auditor` review.
- `dotenvy::dotenv_override(false)` behavior change.
- `GARRAIA_CONFIG_DIR=/custom/config/path` fantasma config detection in `config check`.
- Automatic `openrouter/free → openrouter/auto` fallback (token cost decision).
- Renaming the synthetic `main` config key (defaults artifact).
