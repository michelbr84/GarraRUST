# Plan 0051 — GAR-444 Lote 3: Echo LLM provider (feature-gated)

> **Linear:** [GAR-444](https://linear.app/chatgpt25/issue/GAR-444) (Q18, parent [GAR-430](https://linear.app/chatgpt25/issue/GAR-430) Quality Gates Phase 3.6)
> **Session plan:** `C:/Users/miche/.claude/plans/ative-o-modo-de-inherited-pelican.md` §F
> **Branch:** `feat/quality-gates-lote-3-echo-provider` (worktree `.claude/worktrees/echo-provider`)
> **Base:** `main @ 7c9f81a`
> **Date:** 2026-04-24 (Florida local)
> **Approved:** 2026-04-24 after Repo Analyst report confirmed contract

## 1. Goal

Unblock tests 3–6 of `tests/e2e_telegram_api.sh` in CI without depending on any real LLM provider and without adding new crate dependencies. **Remove the `continue-on-error: true` from `ci.yml:L286`** so the E2E job becomes a genuine blocking gate.

## 2. Why Option A (EchoProvider nativo, feature-gated)

- Doesn't introduce a CI-only code branch inside `/v1/chat/completions` (rejected Option B).
- Doesn't add a container sidecar or new CI moving parts (rejected Option C).
- Reuses the existing `LlmProvider` trait + `AgentRuntime::register_provider` path — exactly the same wiring the anthropic/openai/ollama providers use.
- Also useful to developers locally without API keys.

## 3. Scope

- New module `crates/garraia-agents/src/echo.rs` defining `EchoProvider` + inline `#[cfg(test)]` unit tests.
- Feature `dev-echo-provider = []` in `crates/garraia-agents/Cargo.toml` (default off).
- `#[cfg(feature = "dev-echo-provider")] pub mod echo;` + re-export `EchoProvider` in `crates/garraia-agents/src/lib.rs`.
- Feature forwarding:
  - `crates/garraia-gateway/Cargo.toml`: `dev-echo-provider = ["garraia-agents/dev-echo-provider"]`.
  - `crates/garraia-cli/Cargo.toml`: `dev-echo-provider = ["garraia-gateway/dev-echo-provider"]`.
- `crates/garraia-gateway/src/bootstrap.rs`: add `#[cfg(feature = "dev-echo-provider")] "echo" => {...}` case in the `match llm_config.provider.as_str()` block (line 76+).
- `.github/workflows/ci.yml` e2e job:
  - Build step adds `--features dev-echo-provider`.
  - New "Write echo config" step generates `/tmp/garraia-e2e/config.toml` with `[llm.echo]` + `[agent] default_provider = "echo"`.
  - Start step exports `GARRAIA_CONFIG_DIR=/tmp/garraia-e2e`.
  - **Remove `continue-on-error: true`** from `Run E2E tests`.
  - Remove the `GAR-NEW-Q18` comment block above that step.

## 4. Non-scope (hard blocks)

- No real LLM provider (Anthropic/OpenAI/Ollama) in CI.
- No MSRV bump (GAR-441), no RUSTSEC, no coverage, no mutation, no hotspot refactor.
- No changes to `admin.html` / `admin/handlers.rs` / `bootstrap.rs` except the single feature-gated match arm.
- No trait changes to `LlmProvider` — EchoProvider uses the existing signature verbatim.
- No new workspace crate.
- No changes to Playwright / `GAR-443` — its CoE (L402) stays.
- No changes to `cargo audit` / `GAR-444`-path — its CoE (L443) stays.
- No `.garra-estado.md` rotation, no CLAUDE.md narrative expansion.

## 5. Contract (from Repo Analyst, 2026-04-24)

### 5.1 `LlmProvider` trait (`crates/garraia-agents/src/providers.rs:9-42`)

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn provider_id(&self) -> &str;
    async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse>;
    async fn stream_complete(...) -> Result<...>;         // default returns Err
    fn configured_model(&self) -> Option<&str>;            // default None
    async fn available_models(&self) -> Result<Vec<String>>; // default Vec::new()
    async fn health_check(&self) -> Result<bool>;
}
```

- Uses `#[async_trait]` (trait-object capable — `Arc<dyn LlmProvider>`).
- EchoProvider must implement: `provider_id`, `complete`, `health_check`. Plus optionally override `configured_model` + `available_models` for ergonomics.

### 5.2 Types (re-exported from `providers`)

- `LlmRequest { model, messages: Vec<ChatMessage>, system, max_tokens, temperature, tools }`
- `ChatMessage { role: ChatRole, content: MessagePart }`
- `ChatRole { System, User, Assistant, Tool }`
- `MessagePart { Text(String), Parts(Vec<ContentBlock>) }` (serde-untagged)
- `LlmResponse { content: Vec<ContentBlock>, model, usage, stop_reason }`
- `ContentBlock::Text { text }` is what we return.

### 5.3 Bootstrap wiring (`crates/garraia-gateway/src/bootstrap.rs:74-76`)

```rust
for (name, llm_config) in &config.llm {
    match llm_config.provider.as_str() {
        "anthropic" => { ... }
        "openai"    => { ... }
        ...
    }
}
```

First provider registered auto-becomes default (`register_provider()` in `runtime.rs:240-242`). When the test sends `"model": "auto"`, the gateway falls back to the default provider — so if EchoProvider is the only one registered in CI, it wins automatically.

### 5.4 E2E contract (`tests/e2e_telegram_api.sh`)

- Test 3 POSTs `{model: "auto", session_id, stream: false, messages: [{role: user, content: "..."}]}` to `/v1/chat/completions`.
- Asserts `choices[0].message.content` is non-empty string.
- Test 5 re-POSTs with 2nd user turn + `assistant` history — same assertions.
- Tests 4 (history) and 6 (delete) are non-fatal — they return 0 even on endpoint error. Echo only needs to make 3 and 5 pass.

### 5.5 Config loader

- `LlmProviderConfig { provider: String, model, api_key, base_url, extra }` — `provider` is a plain `String`, not enum. Adding `"echo"` requires no schema change.
- `GARRAIA_CONFIG_DIR` env var chooses the config dir; file is `config.toml`.

## 6. EchoProvider behavior

```rust
pub struct EchoProvider {
    model: String,  // default "echo-stub" if None
}

impl LlmProvider for EchoProvider {
    fn provider_id(&self) -> &str { "echo" }

    async fn complete(&self, req: &LlmRequest) -> Result<LlmResponse> {
        let user_text = last_user_text(req);  // last User message, flattened
        let text = format!("[echo] {}", user_text);
        Ok(LlmResponse {
            content: vec![ContentBlock::Text { text }],
            model: self.model.clone(),
            usage: Some(Usage { input_tokens: 0, output_tokens: 0 }),
            stop_reason: Some("stop".to_string()),
        })
    }

    fn configured_model(&self) -> Option<&str> { Some(&self.model) }
    async fn available_models(&self) -> Result<Vec<String>> { Ok(vec![self.model.clone()]) }
    async fn health_check(&self) -> Result<bool> { Ok(true) }
}
```

Deterministic, stateless, zero external dependencies. Never logs the prompt (see §Security).

## 7. Acceptance criteria

### 7.1 Unit (local, `cargo test`)

- [ ] `cargo test -p garraia-agents --features dev-echo-provider echo` ≥3 tests green:
  - Text prompt → `[echo] <text>`.
  - Parts prompt (`ContentBlock::Text`) → flatten + `[echo]` prefix.
  - Last-user-message is picked when history contains assistant turns.
- [ ] `cargo build -p garraia-agents` (no features) still compiles (feature is OFF by default).
- [ ] `cargo clippy -p garraia-agents --features dev-echo-provider -- -D warnings` clean.
- [ ] `cargo clippy --workspace --all-targets --locked -- -D warnings` (existing CI gate) clean.

### 7.2 Local E2E

- [ ] `cargo build --bin garraia --release --features dev-echo-provider` succeeds.
- [ ] With `/tmp/garraia-e2e/config.toml` seeded + `GARRAIA_CONFIG_DIR=/tmp/garraia-e2e` + same env vars as CI (GARRAIA_JWT_SECRET, GARRAIA_REFRESH_HMAC_SECRET, GARRAIA_LOGIN_DATABASE_URL, GARRAIA_SIGNUP_DATABASE_URL), starting the gateway and running `bash tests/e2e_telegram_api.sh` produces **6/6 [PASS]** with exit 0.

### 7.3 CI

- [ ] All 12 CI checks green on the PR.
- [ ] The `E2E Tests` step reports real 6/6 (not masked by `continue-on-error`).
- [ ] `grep -cE "^\s*continue-on-error:\s*true\s*$" .github/workflows/ci.yml` returns **2** (down from 3).
- [ ] `grep -c "GAR-NEW-Q18" .github/workflows/ci.yml` returns **0**.
- [ ] `grep -c "GAR-NEW-Q17" .github/workflows/ci.yml` still returns **>0** (Playwright drift left to GAR-443 by design).

### 7.4 Runtime invariants

- Default release build (no feature) does not reference `EchoProvider` in any way — proven by `rg "EchoProvider"` on a `cargo build --bin garraia --release` without the feature finding **zero** strings.
- `ANTHROPIC_API_KEY=sk-ant-test-key-for-ci` continues to register no provider in CI (fallback path unchanged); only the new `[llm.echo]` block adds the echo provider.
- No prompt text is logged at `info!`/`warn!` levels by the new code.

## 8. Rollback plan

Revert the PR. The feature is off-by-default, so a simple `git revert` of the squash commit:
- removes the `echo.rs` module
- removes the two Cargo.toml feature entries (they had no cargo-lock impact because the feature is additive and has no deps)
- restores the `ci.yml:L286` `continue-on-error: true` as it was before

No migrations, no data, no user-facing API change. Full rollback is mechanical.

## 9. Branch / worktree / PR strategy

- Worktree already created at `.claude/worktrees/echo-provider` on `feat/quality-gates-lote-3-echo-provider`.
- Commits in logical chunks:
  1. `feat(agents): add EchoProvider behind dev-echo-provider feature gate`
  2. `feat(gateway,cli): forward dev-echo-provider + wire bootstrap match arm`
  3. `feat(ci): wire echo config for e2e job + remove CoE L286 (GAR-444)`
  4. `docs(plans): plan 0051 + README index`
- PR title: `feat(agents,ci): plan 0051 Lote 3 — echo LLM provider + remove CoE L286 (GAR-444)`
- PR body references `Closes GAR-444`. Parent `GAR-430` stays In Progress.
- Human gate: after CI green, **stop for explicit APPROVE** before merge.

## 10. Agents to use (per §E of `inherited-pelican`)

- **Orchestrator** (this agent): sequence, cut scope creep, write plan.
- **Repo Analyst** (Explore subagent, 2026-04-24): ✅ already delivered — see §5.
- **Test & Coverage Engineer** (this agent, TDD strict): RED first.
- **Dependency & Architecture Auditor** (this agent): verify zero new crates, feature gates tight.
- **CI/CD & GitHub Actions Auditor** (this agent): ci.yml diff + `grep` counts.
- **Linear Workflow Coordinator** (this agent): GAR-444 Backlog→In Progress (done) → Done at merge.
- **Security / Reliability Reviewer** (`@security-auditor` subagent): gate before PR.
- **Code Reviewer / QA Gate** (`@code-reviewer` subagent): gate before PR.
- **Documentation / Handoff Writer** (`@doc-writer` subagent): one-line CLAUDE.md note on `garraia-agents` after merge.

## 11. Security considerations

- `EchoProvider` never calls out to any network service.
- No secret or env var is read by the provider itself.
- Prompt content flows through the provider only to produce the deterministic echo response; nothing is logged at `info!`/`warn!`/`error!` by the new code. Tests will assert the no-log invariant via `tracing_test::traced_test` if convenient; otherwise a grep guard.
- `dev-echo-provider` is `default = []`, so a production binary built by a release pipeline without `--features` does **not** include the module. The re-export in `lib.rs` is also `#[cfg(feature = ...)]`-gated.
- CI today uses `ANTHROPIC_API_KEY=sk-ant-test-key-for-ci` — a fake. That key value does not change with this plan; it simply becomes unused once `[llm.echo]` is the registered provider.

## 12. Open questions

None at this point. Repo Analyst confirmed:
- Trait shape is stable.
- Bootstrap match is string-based (no enum to extend).
- No existing mock provider exists to re-purpose.
- Config loader accepts any provider name via `LlmProviderConfig.provider: String`.

## 13. Verification commands (post-merge quick sanity)

```bash
# 1. CoE count dropped to 2
grep -cE "^\s*continue-on-error:\s*true\s*$" .github/workflows/ci.yml   # 2

# 2. Placeholder removed
grep -c "GAR-NEW-Q18" .github/workflows/ci.yml                          # 0

# 3. Unit tests
cargo test -p garraia-agents --features dev-echo-provider echo

# 4. Feature-off build still compiles
cargo check -p garraia-agents

# 5. Release binary without feature does not link EchoProvider
cargo build --bin garraia --release
strings target/release/garraia | grep -c "EchoProvider"                  # 0
```

## 14. Links

- Linear issue: https://linear.app/chatgpt25/issue/GAR-444
- Parent epic: https://linear.app/chatgpt25/issue/GAR-430
- PR (to be opened): `feat/quality-gates-lote-3-echo-provider` → `main`
- Prior Lote 2 PR: https://github.com/michelbr84/GarraRUST/pull/64 (merged `1828625`)
- Prior micro-PR (hooks): https://github.com/michelbr84/GarraRUST/pull/65 (merged `7c9f81a`)
