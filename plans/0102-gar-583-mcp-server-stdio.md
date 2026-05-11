# Plan 0102 — GAR-583: MCP server exposing `garra ask` as stdio tool

**Status:** Em execução
**Autor:** Claude Opus 4.7 (sessão interativa 2026-05-11, America/New_York)
**Data:** 2026-05-11 (America/New_York)
**Issue:** [GAR-583](https://linear.app/chatgpt25/issue/GAR-583/mcp-expose-garra-ask-as-a-stdio-tool)
**Branch:** `routine/202605111207-gar-583-mcp-server-stdio`
**Epic:** `epic:cli`
**Builds on:** GAR-576 (PR #263), GAR-579 (PR #267), GAR-582 (PR #269)

---

## §1 Goal

Expose `garra ask` as an MCP (Model Context Protocol) tool via stdio so Claude Desktop / Claude Code / any MCP host can invoke it. The Garra stops being CLI-only.

Validated risk #10 from spec phase: **`rmcp 1.6.0` server-side API is mature and already in workspace deps**:
- `ServerHandler` trait + `serve_server(handler, transport)` function.
- `rmcp::transport::io::stdio()` returns `(Stdin, Stdout)` tuple.
- `default = ["base64", "macros", "server"]` — server feature ON by default.
- `garraia-agents` already depends on `rmcp = { workspace = true, features = ["client", ...] }`.
- We add `garraia-cli`-scoped dep with `features = ["server", "transport-io", "macros"]`.

---

## §2 Scope (locked-in MVP — user-approved 2026-05-11 with 3 adjustments + 13 constraints)

**In:**
- Subcommand `garra mcp-server` (stdio transport only).
- Tool: `garra_ask` (single).
- Defaults: `provider="openrouter"`, `model="openrouter/free"`, `timeout_secs=60`, cap=600.
- `openrouter/auto` accepted ONLY when caller passes it explicitly.
- Response = full `garra.ask.v1` envelope as MCP text content.
- In-process call to refactored `ask::ask_oneshot` (zero subprocess).
- TWO audit tests: `audit_no_stdout_writes` + `audit_no_dangerous_tools`.

**Out (separate PRs):**
- Integration test JSON-RPC via stdin pipe (deferred).
- Streamable HTTP transport.
- Tools beyond `garra_ask`.
- `RedactingWriter` extension.
- `.env`/`dotenvy`/`GARRAIA_CONFIG_DIR` operator fixes.
- TLS local fixes.
- MCP server telemetry / metrics.
- Per-user auth.
- Automatic `free → auto` fallback.

---

## §3 Architecture

### Subcommand
**Option A**: `garra mcp-server` — single binary, consistent with `garra ask` / `garra chat` / `garra mcp` (existing client-side). Zero binary fragmentation.

### Crate
**`rmcp 1.6.0`** already in workspace. Feature flip in `crates/garraia-cli/Cargo.toml`:
```
rmcp = { workspace = true, features = ["server", "transport-io", "macros"] }
```
Zero new crate in workspace.

### Wiring

```
garra mcp-server (stdio)
  │
  ├─ rmcp::ServerHandler trait impl on GarraToolHandler
  │   ├─ list_tools() → [garra_ask_tool_descriptor()]
  │   └─ call_tool(name="garra_ask", args) → handle_garra_ask(args)
  │
  ├─ handle_garra_ask(args: GarraAskArgs) -> CallToolResult
  │   - serde validates args (schema + bounds)
  │   - build AskOptions
  │   - call ask::ask_oneshot(config, opts).await
  │   - pack AskResult → CallToolResult { content: text(envelope), is_error }
  │
  └─ ask::ask_oneshot (REFACTORED in this PR)
      - pure async fn, NO I/O
      - returns AskResult struct
      - reuses chat::select_explicit_provider / chat::detect_provider
      - AgentRuntime with ZERO tool registration
```

### Stdio discipline (critical invariant)

| Stream | Owner | Content |
|---|---|---|
| stdin | `rmcp` | JSON-RPC requests from Claude |
| stdout | `rmcp` (exclusive) | JSON-RPC responses to Claude. NO other code writes here. |
| stderr | `tracing` (via `RedactingWriter` from `main.rs:558`) | logs, redacted |

**Audit test #1**: `audit_no_stdout_writes` scans `mcp_server.rs` production code for `println!`, `print!`, `io::stdout()`, `std::io::stdout` — must return 0 matches.

---

## §4 Design invariants

1. **Stdio discipline**: zero `println!`/`print!`/`stdout()` in `mcp_server.rs` production code (audit test).
2. **Zero dangerous tools**: `mcp_server.rs` production code MUST NOT contain `register_tool`, `BashTool`, `FileReadTool`, `FileWriteTool`, `GitDiffTool`, `std::process::Command` (audit test #2).
3. **Zero subprocess**: handler calls `ask::ask_oneshot` in-process. No PATH lookup. No shell.
4. **Zero new crate**: only feature flips on `rmcp` already in workspace.
5. **`ask::ask_oneshot` is pure**: returns `AskResult`, no I/O. `run_ask` (existing CLI) wraps it.
6. **Default to cheap**: `model="openrouter/free"` default; `openrouter/auto` opt-in only.
7. **Envelope as text content**: MCP response carries full `garra.ask.v1` JSON, not just `answer`.
8. **Author canonical**: commit metadata enforced via `git config --local` + `git commit --amend --reset-author` if needed.

---

## §5 Code changes

### `crates/garraia-cli/Cargo.toml`
```diff
+rmcp = { workspace = true, features = ["server", "transport-io", "macros"] }
```

### `crates/garraia-cli/src/ask.rs` — refactor: extract pure core

```rust
pub(crate) struct AskOptions {
    pub message: String,
    pub provider_override: Option<String>,
    pub model_override: Option<String>,
    pub url_override: Option<String>,
    pub timeout_secs: u64,
    pub system_prompt_override: Option<String>,
}

pub(crate) struct AskResult {
    pub schema: &'static str,         // "garra.ask.v1"
    pub ok: bool,
    pub answer: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub latency_ms: Option<u128>,
    pub error_kind: Option<&'static str>,
    pub error_message: Option<String>,
}

impl AskResult {
    pub(crate) fn to_envelope(&self) -> serde_json::Value { ... }
    pub(crate) fn exit_code(&self) -> i32 { ... }  // for CLI caller
}

pub(crate) async fn ask_oneshot(
    config: &AppConfig,
    opts: AskOptions,
) -> AskResult {
    // moved from run_ask: provider resolution + AgentRuntime + LLM call + timeout
    // ZERO println!/eprintln! in this function
}
```

`run_ask` becomes a thin wrapper: parse stdin if needed, build `AskOptions`, call `ask_oneshot`, emit JSON/text via `println!`/`eprintln!`, return exit code.

### `crates/garraia-cli/src/mcp_server.rs` — new module

```rust
use std::sync::Arc;
use anyhow::Result;
use garraia_config::AppConfig;
use rmcp::{
    ServerHandler, RoleServer, serve_server,
    transport::io::stdio,
    model::{
        CallToolRequestParams, CallToolResult, ListToolsResult, PaginatedRequestParams,
        Tool, RawContent, Annotated,
    },
    service::RequestContext,
    ErrorData as McpError,
};
use serde::Deserialize;

use crate::ask::{self, AskOptions};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GarraAskArgs {
    message: String,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(default)]
    system_prompt: Option<String>,
}

struct GarraToolHandler {
    config: Arc<AppConfig>,
}

impl ServerHandler for GarraToolHandler {
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult {
            tools: vec![garra_ask_tool()],
            next_cursor: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        if request.name != "garra_ask" {
            return Err(McpError::method_not_found::<...>());
        }
        let args: GarraAskArgs = serde_json::from_value(
            serde_json::Value::Object(request.arguments.unwrap_or_default())
        ).map_err(|e| McpError::invalid_params(format!("bad arguments: {e}"), None))?;

        // bounds validation
        if args.message.is_empty() || args.message.len() > 65_536 { ... }
        if let Some(ts) = args.timeout_secs && !(1..=600).contains(&ts) { ... }
        if let Some(ref sp) = args.system_prompt && sp.len() > 8192 { ... }

        let opts = AskOptions {
            message: args.message,
            provider_override: args.provider,
            model_override: args.model,
            url_override: None,
            timeout_secs: args.timeout_secs.unwrap_or(60),
            system_prompt_override: args.system_prompt,
        };

        let result = ask::ask_oneshot(&self.config, opts).await;
        let envelope = result.to_envelope();
        let text = serde_json::to_string(&envelope).unwrap_or_default();
        let is_error = !result.ok;

        Ok(CallToolResult {
            content: vec![Annotated::new(RawContent::text(text), None)],
            is_error: Some(is_error),
            ..Default::default()
        })
    }
}

fn garra_ask_tool() -> Tool { /* descriptor with schema */ }

pub async fn run_mcp_server(config: AppConfig) -> Result<()> {
    let handler = GarraToolHandler { config: Arc::new(config) };
    let (stdin, stdout) = stdio();
    let service = serve_server(handler, (stdin, stdout)).await?;
    service.waiting().await?;
    Ok(())
}
```

### `crates/garraia-cli/src/main.rs` — wire subcommand

```rust
mod mcp_server;

enum Commands {
    ...
    /// Start MCP server (stdio transport) exposing `garra_ask` tool.
    /// Designed for Claude Desktop / Claude Code integration.
    McpServer,
    ...
}

match cli.command {
    ...
    Commands::McpServer => {
        init_tracing(&effective_level);
        mcp_server::run_mcp_server(config).await?;
    }
    ...
}
```

---

## §6 Tests

### Unit tests (pure, in `mcp_server.rs::tests`)

1. `tool_descriptor_name_is_garra_ask` — descritor tem `name: "garra_ask"`.
2. `tool_descriptor_has_message_required` — `inputSchema.required` inclui `"message"`.
3. `tool_descriptor_default_model_is_openrouter_free` — schema property `model` has default `"openrouter/free"`.
4. `tool_descriptor_timeout_range_1_to_600` — schema constrains `timeout_secs`.
5. `args_deserialize_minimum` — `{"message":"hi"}` parses with defaults.
6. `args_reject_missing_message` — required field validation.
7. `args_reject_message_over_64kib` — bounds.
8. `args_reject_timeout_out_of_range` — bounds.
9. `args_reject_system_prompt_over_8kib` — bounds.
10. `args_reject_additional_properties` — `additionalProperties: false`.
11. `args_explicit_openrouter_auto_accepted` — opt-in works.
12. **`audit_no_stdout_writes`** — scans `mcp_server.rs` production code for `println!`, `print!`, `io::stdout`. Production = code before `#[cfg(test)]`.
13. **`audit_no_dangerous_tools`** — scans for `register_tool`, `BashTool`, `FileReadTool`, `FileWriteTool`, `GitDiffTool`, `std::process::Command`. Production-scope same as #12.

### Unit tests (in `ask.rs::tests`, new)

14. `ask_oneshot_usage_error_for_empty_message` — pure path.
15. `ask_result_to_envelope_success_shape` — envelope structure.
16. `ask_result_to_envelope_error_shape` — error envelope.
17. `ask_result_exit_code_mapping` — table-driven (5 variants).

Existing 15 `ask::tests` from GAR-579 + 12 `chat::tests` from GAR-576 must continue passing.

### Manual smokes (no token cost)

- `./target/release/garra.exe mcp-server --help` exit 0.
- `./target/release/garra.exe mcp-server` (don't kill immediately; pipe `{"jsonrpc":"2.0","id":1,"method":"tools/list"}` via stdin) → response JSON includes `garra_ask` — OPTIONAL, only if rmcp doesn't require complex handshake.

### Smoke real (opcional, network-dependent)

`openrouter/free` only. Configure in Claude Desktop:
```json
{"mcpServers":{"garra":{"command":"garra","args":["mcp-server"]}}}
```
Restart Claude Desktop. Ask "Oi, tudo bem?". `openrouter/auto` **NOT** executed.

---

## §7 Verification

```bash
cargo fmt --all -- --check
cargo clippy --workspace --exclude garraia-desktop --all-targets -- -D warnings
cargo test -p garraia --bin garra
cargo build --release --bin garra
./target/release/garra.exe mcp-server --help   # smoke
```

---

## §8 Risks (carry-forward from spec §10, with mitigations now concrete)

| # | Risk | Mitigation in implementation |
|---|---|---|
| 1 | Stdout corruption | Pure `ask_oneshot` + audit test `audit_no_stdout_writes` |
| 2 | Prompt injection | Out of scope — LLM-level concern, not server-level |
| 3 | Secret in logs | `sanitize_provider_error` from GAR-579 covers error path |
| 4 | Timeout | `tokio::time::timeout` + schema cap 600 |
| 5 | Path hijacking | Zero subprocess — `audit_no_dangerous_tools` catches `std::process::Command` |
| 6 | Shell injection | Zero shell |
| 7 | Prompt size | Schema `maxLength: 65536`; rejected before LLM call |
| 8 | Output size | `max_tokens: 4096` in `ask_oneshot` |
| 9 | Loops | OpenRouter (provider) doesn't call back into MCP |
| 10 | rmcp server API | **VALIDATED 2026-05-11**: `ServerHandler` trait + `serve_server` + `transport::io::stdio` confirmed in cached source |
| 11 | Token cost | Default `openrouter/free`; explicit opt-in for `openrouter/auto` |
| 12 | CodeQL FP | Pattern: separate `get_api_key` from result-returning paths (mirrors GAR-576 fix) |

---

## §9 Out of scope (follow-ups, mirroring user's lock-in §2)

1. Integration test JSON-RPC via stdin pipe.
2. Streamable HTTP transport.
3. Additional tools (`garra_chat`, `garra_files`).
4. `--enable-tools` in `garra ask` + MCP propagation.
5. `--stream` partial responses.
6. Auto `free → auto` fallback.
7. `RedactingWriter` provider-error extension.
8. `.env` / `dotenvy_override` / `GARRAIA_CONFIG_DIR` operator fixes.
9. AgentRuntime sandbox.
10. MCP telemetry.
11. Per-user auth.

---

## §10 Commit shape

- Title: `feat(cli): add MCP server exposing garra ask as stdio tool`
- Conventional commits, NOT breaking.
- Single commit preferred; split only if CodeQL drama (GAR-576 pattern).
- Author: `michelbr84 <166889728+michelbr84@users.noreply.github.com>` (canonical, verified via `git config --local`).
