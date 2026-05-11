# Plan 0103 — GAR-585: advertise `tools` capability in garra MCP server

- **Issue:** [GAR-585](https://linear.app/chatgpt25/issue/GAR-585)
- **Parent epic:** [GAR-583](https://linear.app/chatgpt25/issue/GAR-583) (`garra mcp-server`, PR #270)
- **Branch:** `fix/gar-585-mcp-tools-capability`
- **Created:** 2026-05-11 (Florida)
- **Status:** Draft → ready for execution

## §1 Goal

Make the `garra mcp-server` stdio binary advertise the `tools` capability in its
MCP `initialize` response so that hosts (Claude Code, Claude Desktop, any MCP
client) know they should call `tools/list` and surface `garra_ask` to the user
or model.

## §2 Background / Root cause

`crates/garraia-cli/src/mcp_server.rs` implements `ServerHandler` from
`rmcp = "1.6"` with `list_tools` and `call_tool` only. The trait has a default
`fn get_info(&self) -> ServerInfo` that returns `ServerInfo::default()`, which
uses `ServerCapabilities::default()` — i.e. all-`None` fields and an **empty
capabilities object** when serialized:

```text
"capabilities": {}
"serverInfo":   {"name":"rmcp","version":"1.6.0"}
```

MCP hosts treat an empty `capabilities` object as "this server offers nothing"
and skip the `tools/list` round-trip — even though our `tools/list` handler
already works in a manual smoke. That is exactly the symptom observed in
Claude Code's `/mcp` panel: server connects, command is correct, but the
panel shows `Capabilities: none` and `garra_ask` never lights up.

The canonical rmcp 1.6 fix is to override `get_info` exactly as
`tests/common/calculator.rs:55-58` and `tests/test_custom_headers.rs` do:

```rust
fn get_info(&self) -> ServerInfo {
    ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
}
```

`enable_tools()` flips the `tools` field to `Some(ToolsCapability::default())`,
which serializes as `"tools": {}` inside `capabilities` — non-empty, which is
all hosts need to discover and expose the tool.

## §3 Scope / Non-scope

### In scope

- Override `ServerHandler::get_info` in `GarraToolHandler`
  (`crates/garraia-cli/src/mcp_server.rs`).
- Add **one** unit test asserting `get_info().capabilities.tools.is_some()`.
- Re-run `cargo fmt --check`, `cargo clippy ... -D warnings`,
  `cargo test -p garraia --bin garra`, `cargo build --release --bin garra`.
- Smoke (before/after) running `initialize` + `tools/list` against the
  release binary via stdio; capture evidence.

### Out of scope (locked)

- `garra_ask` arg shape, validation, behavior.
- Provider / model selection or defaults (still `openrouter/free`,
  `openrouter/auto` only opt-in).
- OpenRouter network calls — **no `tools/call` during smoke**.
- `.claude.json` or Claude Desktop server config.
- TLS / certificate handling.
- `RedactingWriter` and stderr routing.
- Adding any "dangerous" tool (no `BashTool`, no `FileWriteTool`,
  no `std::process::Command`, no subprocess spawning).
- Bumping `rmcp` version or touching workspace `Cargo.toml`.

## §4 Acceptance criteria

1. `initialize` response `result.capabilities.tools` is present (non-null) when
   probing `target/release/garra.exe mcp-server` via stdio.
2. `tools/list` still returns **exactly one tool** named `garra_ask`, with the
   pre-existing schema unchanged.
3. New unit test `get_info_advertises_tools_capability` passes; the existing
   audit tests (`audit_mcp_server_never_writes_to_stdout`,
   `audit_mcp_server_never_registers_dangerous_tools`) keep passing.
4. `cargo fmt --all -- --check` ✓
5. `cargo clippy --workspace --exclude garraia-desktop --all-targets -- -D warnings` ✓
6. `cargo test -p garraia --bin garra` ✓
7. `cargo build --release --bin garra` ✓
8. After a Claude Code session restart, `/mcp` panel no longer shows
   `Capabilities: none` for `garra` (manual verification, post-merge).

## §5 Test strategy

### Unit test (red → green)

```rust
#[test]
fn get_info_advertises_tools_capability() {
    let cfg = std::sync::Arc::new(garraia_config::AppConfig::default());
    let handler = GarraToolHandler::new(cfg);
    let info = ServerHandler::get_info(&handler);
    assert!(
        info.capabilities.tools.is_some(),
        "tools capability must be advertised — see GAR-585"
    );
}
```

Initially fails (default `get_info` returns empty capabilities). Passes once
the override is in place.

### Smoke evidence (manual, no LLM call)

Capture two transcripts of:

```text
initialize → notifications/initialized → tools/list
```

piped through `./target/release/garra.exe mcp-server`. Compare:

- Before fix: `"capabilities":{}` (baseline already captured 2026-05-11
  pre-execution).
- After fix: `"capabilities":{"tools":{}}` (or `"tools":{"listChanged":...}`
  if the builder default ever evolves).

Both transcripts pasted in the PR description and in this plan's §11.

## §6 Open questions

None. The API surface (`ServerCapabilities::builder().enable_tools()`),
the default behavior (`ServerInfo::default()` = empty), and the canonical
override site (`fn get_info`) are all confirmed by reading rmcp 1.6 source.

## §7 Risks

- **Risk:** the override path is incorrect for stateful handlers.
  **Mitigation:** mirror rmcp's own `tests/common/calculator.rs` 1:1; the
  signature is `fn get_info(&self) -> ServerInfo` and rmcp's `initialize`
  default implementation simply calls `self.get_info()`
  (`rmcp-1.6.0/src/handler/server.rs:182-191`).
- **Risk:** clippy warns about the new method.
  **Mitigation:** keep the body to a single expression; the lint suite under
  `-D warnings` will catch any regression locally.

## §8 Rollback plan

Reverse the single commit on `fix/gar-585-mcp-tools-capability`. The change
touches only `crates/garraia-cli/src/mcp_server.rs` (one method addition + one
unit test); there is no migration, no schema change, no config change. A
`git revert` is a sufficient rollback.

## §9 Migration / Deployment

None. Pure code change inside `garraia-cli`. Users running an older
`garra mcp-server` keep their behavior; users on the new binary get the
correct capability advertisement immediately on next launch.

## §10 References

- rmcp 1.6 calculator example: `tests/common/calculator.rs:55-58`
- rmcp 1.6 `ServerInfo::default()`: `src/model.rs:892-902`
- rmcp 1.6 default `get_info`: `src/handler/server.rs:332-334`
- rmcp 1.6 capabilities builder: `src/model/capabilities.rs:281-441`
- GAR-583 / PR #270: original MCP server slice.

## §11 Smoke evidence

### Before (baseline, 2026-05-11 ~14:12 ET)

```text
initialize → {"jsonrpc":"2.0","id":1,"result":{
  "protocolVersion":"2025-06-18",
  "capabilities":{},
  "serverInfo":{"name":"rmcp","version":"1.6.0"}
}}

tools/list → {"jsonrpc":"2.0","id":2,"result":{
  "tools":[{"name":"garra_ask", … (full schema preserved)}]
}}
```

### After (2026-05-11 ~14:25 ET, fresh release build to `target-gar585/`)

```text
initialize → {"jsonrpc":"2.0","id":1,"result":{
  "protocolVersion":"2025-06-18",
  "capabilities":{"tools":{}},
  "serverInfo":{"name":"rmcp","version":"1.6.0"}
}}

tools/list → unchanged: returns exactly one tool, `garra_ask`,
             with the same schema as before (verified byte-for-byte
             against the baseline capture).
```

Diff that matters: `"capabilities":{}` → `"capabilities":{"tools":{}}`.
Hosts now know to fetch the tool list and will surface `garra_ask`.

## §12 Final status

- **Linear:** [GAR-585](https://linear.app/chatgpt25/issue/GAR-585) (In Progress)
- **Branch:** `fix/gar-585-mcp-tools-capability`
- **File changed:** `crates/garraia-cli/src/mcp_server.rs`
  (one `get_info` override + two unit tests; one new import).
- **rmcp method used:** `ServerHandler::get_info` override returning
  `ServerInfo::new(ServerCapabilities::builder().enable_tools().build())`.
- **Verification:**
  - `cargo fmt --all -- --check` ✓
  - `cargo clippy --workspace --exclude garraia-desktop --all-targets -- -D warnings` ✓
  - `cargo test -p garraia --bin garra` → 77 passed, 0 failed
  - `cargo build --release --bin garra` → built into `target-gar585/release/`
    (isolated target dir; the in-place `target/release/garra.exe` was
    held by the host's running MCP child).
- **Confirmation:** no `tools/call` issued during this fix; no OpenRouter
  network call; `openrouter/auto` never invoked. Smoke evidence covers
  `initialize` and `tools/list` exclusively.
