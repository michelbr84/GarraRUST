# Plan 0104 — GAR-587: apply `garra_ask` schema defaults before calling `ask_oneshot`

- **Issue:** [GAR-587](https://linear.app/chatgpt25/issue/GAR-587)
- **Parent epic:** [GAR-583](https://linear.app/chatgpt25/issue/GAR-583) (`garra mcp-server`, PR #270)
- **Sibling:** [GAR-585](https://linear.app/chatgpt25/issue/GAR-585) (`get_info` advertises tools, PR #271)
- **Branch:** `fix/gar-587-mcp-garra-ask-defaults`
- **Created:** 2026-05-11 (Florida)
- **Status:** Draft → ready for execution

## §1 Goal

When an MCP host calls `tools/call name=garra_ask` with the minimum payload
(`{ "message": "..." }` and **no** `provider` / `model`), the handler must
apply the same defaults the JSON schema advertises — `provider="openrouter"`
and `model="openrouter/free"` — **before** invoking `ask::ask_oneshot`.
Hosts do not synthesize JSON-Schema `default` values; today the handler
forwards `None` and `ask_oneshot` falls through to local `AppConfig`
resolution, which on a misconfigured operator machine surfaces unrelated
provider 401s.

## §2 Background / Root cause

`crates/garraia-cli/src/mcp_server.rs:97-144` advertises a JSON schema with:

```json
"provider": { ..., "default": "openrouter" },
"model":    { ..., "default": "openrouter/free" }
```

JSON-Schema `default` is **advisory** — neither `rmcp` nor the MCP host
(Claude Code, Claude Desktop) injects those values into the call payload
before delivering it to `call_tool`. The handler at
`crates/garraia-cli/src/mcp_server.rs:210-217` builds `AskOptions` by
forwarding `args.provider` / `args.model` straight through:

```rust
provider_override: args.provider,   // None when host omitted it
model_override:    args.model,      // None when host omitted it
```

`ask::ask_oneshot` then resolves provider/model from `AppConfig`, env vars,
and CLI defaults — none of which honor the **MCP schema contract**. The
`timeout_secs` field already follows the right pattern
(`args.timeout_secs.unwrap_or(ARG_TIMEOUT_SECS_DEFAULT)` at L215), so the
fix is to apply the same shape to `provider` / `model`.

### Empirical repro (2026-05-11, Florida)

From Claude Code, calling `mcp__garra__garra_ask` with `{ "message": "Oi,
tudo bem?" }`:

```text
{"error":{"kind":"provider_error","message":"agent error: openai API error: status=401 Unauthorized ..."}}
```

Same call with explicit `provider="openrouter"` + `model="openrouter/free"`
returns `ok:true` with `latency_ms: 1541`. Confirms the gap is **handler-side
default application**, not MCP transport, not OpenRouter config.

## §3 Scope / Non-scope

### In scope

- Add two file-scoped constants in `crates/garraia-cli/src/mcp_server.rs`:
  `PROVIDER_DEFAULT = "openrouter"`, `MODEL_DEFAULT = "openrouter/free"`.
- In `call_tool`, wrap `args.provider` / `args.model` with
  `Some(... .unwrap_or_else(|| <const>.to_string()))`.
- Four new unit tests (see §5).
- Re-run `cargo fmt --check`, `cargo clippy ... -D warnings`,
  `cargo test -p garraia --bin garra`, `cargo build --release --bin garra`.

### Out of scope (locked)

- `garra ask` CLI behavior (`run_ask` already resolves correctly via
  GAR-579 / GAR-582 paths).
- `garraia-config`, `AppConfig`, or any global provider-resolution logic.
- `openrouter/auto` policy — still opt-in only, never automatic.
- Bumping `rmcp` or any workspace dependency.
- `.env`, OpenRouter key, TLS, `RedactingWriter`, Claude Code / Desktop
  config.
- Real LLM calls in CI.
- Touching the two `audit_…` invariants
  (`audit_mcp_server_never_writes_to_stdout`,
  `audit_mcp_server_never_registers_dangerous_tools`).

## §4 Acceptance criteria

1. With minimum args (`{ "message": "hi" }`), the `AskOptions` passed into
   `ask::ask_oneshot` has `provider_override == Some("openrouter")` and
   `model_override == Some("openrouter/free")`.
2. Explicit values from the host (e.g. `provider="anthropic"`,
   `model="claude-opus-4-7"`) survive unchanged.
3. `openrouter/auto` is **only** reachable when the host passes it
   explicitly — never inferred from the absence of `model`.
4. JSON-Schema `default` fields for `provider` / `model` remain unchanged
   in the tool descriptor (pinned by existing tests
   `tool_descriptor_default_model_is_openrouter_free` and new
   `tool_descriptor_default_provider_is_openrouter`).
5. Both `audit_…` invariants remain green.
6. `cargo fmt --check`, `cargo clippy --workspace --exclude garraia-desktop
   --all-targets -- -D warnings`, `cargo test -p garraia --bin garra`,
   `cargo build --release --bin garra` all green locally and in CI.

## §5 Tests (RED first)

Add to the `#[cfg(test)] mod tests` block in `mcp_server.rs`. The tests
target the **handler-side default resolution** (the gap fixed in this PR),
not the schema descriptor (already covered by GAR-583 tests). To keep them
pure and offline, factor the default-application into a tiny helper:

```rust
fn resolve_overrides(args: &GarraAskArgs) -> (String, String) {
    (
        args.provider.clone().unwrap_or_else(|| PROVIDER_DEFAULT.to_string()),
        args.model.clone().unwrap_or_else(|| MODEL_DEFAULT.to_string()),
    )
}
```

Then exercise it directly — no `RequestContext`, no `tokio` runtime, no
network:

| Test | Input | Asserts |
|------|-------|---------|
| `resolve_overrides_applies_defaults_when_args_are_none` | `{ message: "hi" }` | `("openrouter", "openrouter/free")` |
| `resolve_overrides_keeps_explicit_provider_and_model` | `provider="anthropic"`, `model="claude-opus-4-7"` | `("anthropic", "claude-opus-4-7")` |
| `resolve_overrides_passes_openrouter_auto_only_when_explicit` | `provider=None`, `model="openrouter/auto"` | `("openrouter", "openrouter/auto")` — and a second case with both None ⇒ `model != "openrouter/auto"` |
| `tool_descriptor_default_provider_is_openrouter` | (schema-side, mirrors `…_default_model_is_openrouter_free`) | `schema.properties.provider.default == "openrouter"` |

All four are pure, deterministic, < 1 ms each, and use no fixtures.

## §6 Implementation

### §6.1 Constants

Add next to `ARG_TIMEOUT_SECS_DEFAULT`
(`crates/garraia-cli/src/mcp_server.rs` ~L42):

```rust
/// GAR-587 — Default provider applied when the MCP caller omits `provider`.
/// Matches the JSON-Schema `default` advertised by `garra_ask_tool`.
const PROVIDER_DEFAULT: &str = "openrouter";

/// GAR-587 — Default model applied when the MCP caller omits `model`.
/// Matches the JSON-Schema `default` advertised by `garra_ask_tool`.
/// NOTE: stays at `openrouter/free`. `openrouter/auto` must remain
/// opt-in (caller-explicit only).
const MODEL_DEFAULT: &str = "openrouter/free";
```

### §6.2 Helper

Add a tiny pure helper above `impl ServerHandler for GarraToolHandler`:

```rust
/// GAR-587 — Apply MCP schema defaults to `(provider, model)` when the
/// caller omitted them. JSON-Schema `default` is advisory; MCP hosts
/// do **not** synthesize missing values, so the handler must apply them
/// explicitly to honor the contract advertised by `garra_ask_tool`.
fn resolve_overrides(args: &GarraAskArgs) -> (String, String) {
    let provider = args
        .provider
        .clone()
        .unwrap_or_else(|| PROVIDER_DEFAULT.to_string());
    let model = args
        .model
        .clone()
        .unwrap_or_else(|| MODEL_DEFAULT.to_string());
    (provider, model)
}
```

### §6.3 `call_tool` rewire

In `call_tool` (~L210-217), replace:

```rust
let opts = AskOptions {
    message: args.message,
    provider_override: args.provider,
    model_override: args.model,
    url_override: None,
    timeout_secs: args.timeout_secs.unwrap_or(ARG_TIMEOUT_SECS_DEFAULT),
    system_prompt_override: args.system_prompt,
};
```

with:

```rust
let (provider, model) = resolve_overrides(&args);
let opts = AskOptions {
    message: args.message,
    provider_override: Some(provider),
    model_override: Some(model),
    url_override: None,
    timeout_secs: args.timeout_secs.unwrap_or(ARG_TIMEOUT_SECS_DEFAULT),
    system_prompt_override: args.system_prompt,
};
```

### §6.4 Schema descriptor (no change)

The JSON schema in `garra_ask_tool` already advertises both defaults and
is pinned by `tool_descriptor_default_model_is_openrouter_free`. Add a
symmetric `tool_descriptor_default_provider_is_openrouter` test for parity
— **no edit to the schema literal itself**.

## §7 Validation

```bash
cargo fmt --all -- --check
cargo clippy --workspace --exclude garraia-desktop --all-targets -- -D warnings
cargo test -p garraia --bin garra
cargo build --release --bin garra
```

All four must be green.

## §8 Smoke (post-merge, operator-side only — not in CI)

After merge, from Claude Code call `mcp__garra__garra_ask` with
`{ "message": "ping" }` (no `provider`/`model`). Expect `ok: true`,
`provider: "openrouter"`, `model: "openrouter/free"`. No OpenAI 401.

No automated smoke is added in this PR — that would either require a real
network call (forbidden in CI) or a mock provider injection that doesn't
exist for `ask_oneshot` today.

## §9 Risks

- **Low.** The change is three lines in `mcp_server.rs` plus four pure
  unit tests. The only failure mode is making `openrouter/auto` reachable
  by default, which test #3 in §5 explicitly guards against.
- No security surface change. No new code paths reach the LLM that
  weren't already reachable via explicit caller payload.
- No behavior change for `garra ask` CLI (different code path).

## §10 Out-of-scope follow-ups (not in this PR)

- Wire a feature-flagged offline provider into `ask::ask_oneshot` so we
  can write an end-to-end MCP smoke without hitting the network. Would
  belong in a separate slice.
- Surface a structured `MCP defaults applied` audit log entry when
  defaults kicked in (low value; deferred).
