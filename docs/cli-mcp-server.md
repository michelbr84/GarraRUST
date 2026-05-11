# `garra mcp-server` — MCP server exposing `garra ask`

> GAR-583 — stdio Model Context Protocol server. Exposes a single tool
> (`garra_ask`) that runs the same code path as
> [`garra ask`](cli-ask.md) in-process — no subprocess, no shell, LLM-only.
>
> Designed for Claude Desktop, Claude Code, and any MCP host that
> speaks stdio JSON-RPC.

## Quickstart

### Claude Desktop

Edit `~/.config/claude/claude_desktop_config.json` (macOS/Linux) or
`%APPDATA%\Claude\claude_desktop_config.json` (Windows) and add:

```json
{
  "mcpServers": {
    "garra": {
      "command": "garra",
      "args": ["mcp-server"]
    }
  }
}
```

Restart Claude Desktop. The `garra_ask` tool should appear in the
tools list of the assistant.

### Claude Code

Same config shape, in the project's MCP config:

```json
{
  "mcpServers": {
    "garra": {
      "command": "garra",
      "args": ["mcp-server"]
    }
  }
}
```

If `garra` is not on `PATH`, use the absolute path:

```json
{
  "mcpServers": {
    "garra": {
      "command": "G:/Projetos/GarraRUST/target/release/garra.exe",
      "args": ["mcp-server"]
    }
  }
}
```

### Test the server manually

The server speaks JSON-RPC 2.0 over stdio. It's not meant to be poked
by humans, but you can sanity-check the binary:

```bash
./target/release/garra.exe mcp-server --help
```

To verify the tool descriptor is wired correctly, you can pipe a
`tools/list` request and inspect stdout (logs go to stderr):

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}' | ./target/release/garra.exe mcp-server 2>/dev/null
```

## Tool schema (`garra_ask`)

| Field           | Type    | Required | Default            | Notes                                                |
|-----------------|---------|----------|--------------------|------------------------------------------------------|
| `message`       | string  | yes      | —                  | Max 64 KiB. Trimmed; empty rejected.                 |
| `provider`      | string  | no       | `openrouter`       | Enum: `ollama`/`anthropic`/`openai`/`openrouter`.    |
| `model`         | string  | no       | `openrouter/free`  | Pass `openrouter/auto` explicitly for complex tasks. |
| `timeout_secs`  | integer | no       | `60`               | Range `[1, 600]`. Excedes → `error.kind: "timeout"`. |
| `system_prompt` | string  | no       | minimal default    | Max 8 KiB.                                           |

The server accepts only those properties (`additionalProperties:
false`); unknown fields are rejected with `invalid_params`.

## Response shape

The tool returns a single text content block containing the
[`garra.ask.v1`](cli-ask.md#json-envelope-schema-garraaskv1) JSON
envelope. Success:

```json
{
  "content": [
    {
      "type": "text",
      "text": "{\"schema\":\"garra.ask.v1\",\"ok\":true,\"answer\":\"...\",\"provider\":\"openrouter\",\"model\":\"openrouter/free\",\"latency_ms\":1234}"
    }
  ],
  "isError": false
}
```

Error:

```json
{
  "content": [
    {
      "type": "text",
      "text": "{\"schema\":\"garra.ask.v1\",\"ok\":false,\"error\":{\"kind\":\"provider_error\",\"message\":\"...\"}}"
    }
  ],
  "isError": true
}
```

This shape gives Claude direct visibility into `provider`, `model`,
`latency_ms`, and `error.kind` without parsing free-form text.

## Cost policy (`openrouter/free` vs `openrouter/auto`)

- **`openrouter/free`** is the **default**. Use for general
  conversation, smoke tests, CI, automation, and anything where cost
  matters.
- **`openrouter/auto`** is **opt-in only** — the caller MUST pass
  `model: "openrouter/auto"` explicitly. There is no automatic
  `free → auto` upgrade.

This mirrors the [`garra ask`](cli-ask.md) policy locked-in by user
2026-05-11.

## Stdio invariants

| Stream | Owner             | Content                                            |
|--------|-------------------|----------------------------------------------------|
| stdin  | `rmcp`            | JSON-RPC requests from the MCP host                |
| stdout | `rmcp` (exclusive)| JSON-RPC responses. **Nothing else writes here.** |
| stderr | `tracing`         | Redacted logs (api-key fingerprints sanitized).    |

The codebase enforces these invariants via two compile-time audit
tests in `crates/garraia-cli/src/mcp_server.rs::tests`:

- `audit_mcp_server_never_writes_to_stdout` — scans production code
  for `println!`, `print!`, `io::stdout`, `stdout().write`.
- `audit_mcp_server_never_registers_dangerous_tools` — scans for
  `register_tool`, `BashTool`, `FileReadTool`, `FileWriteTool`,
  `GitDiffTool`, `std::process::Command`, `tokio::process::Command`.

A future PR that tries to slip a tool registration or a subprocess
spawn into `mcp_server.rs` will fail the audit at `cargo test` time.

## Troubleshooting

### Claude Desktop / Code can't find the `garra_ask` tool

Verify the server starts:

```bash
garra mcp-server --help
```

If `--help` works but the tool doesn't show up in Claude:

1. Check `garra` is on `PATH` (or use absolute path in MCP config).
2. Restart Claude Desktop / Claude Code fully (not just the tab).
3. Look at host-side MCP logs — most hosts log MCP server stderr to a
   file under their data directory.

### Tool calls timeout immediately

The default `timeout_secs` is 60. If the LLM takes longer (rare for
`openrouter/free`), pass a larger value:

```json
{"name": "garra_ask", "arguments": {"message": "...", "timeout_secs": 120}}
```

Range is `[1, 600]`; values outside the range are rejected with
`invalid_params`.

### `error.kind: "provider_error"` — network/auth issue

The Garra successfully invoked the provider but the provider returned
an error or the network couldn't reach it. Common causes:

- Invalid API key in `~/.garraia/config.yml` or `.env`. Use `garra
  config check` to validate.
- Windows TLS revocation check failure
  (`CRYPT_E_NO_REVOCATION_CHECK`) — known transient issue on some
  networks. Try a different network or VPN.

The `error.message` is sanitized — api-key fingerprints are
redacted via [`sanitize_provider_error`](../crates/garraia-cli/src/ask.rs).

### `error.kind: "usage"` — bad arguments

Schema validation rejected the call. Check:

- `message` is non-empty and ≤ 64 KiB.
- `timeout_secs` is in `[1, 600]`.
- `system_prompt` ≤ 8 KiB.
- No unknown properties in the arguments object.

## Security notes

- **In-process only**. The tool handler calls `ask::ask_oneshot`
  directly. There is no subprocess spawn, no shell invocation, no
  PATH lookup. Path hijacking and shell injection are not in the
  threat model.
- **LLM-only runtime**. No `bash`/`file_read`/`file_write`/`git_diff`
  tools are registered on the agent runtime — the audit test prevents
  regression.
- **Prompt size bounded**. Messages ≤ 64 KiB, system prompts ≤ 8 KiB.
- **Output bounded**. `max_tokens: 4096` in the runtime caps the
  response.
- **No loops**. The provider (OpenRouter, Anthropic, etc.) is a plain
  HTTPS endpoint; it doesn't call MCP servers back.
- **Provider errors sanitized** before reaching the response or
  stderr (regex-based redaction of `sk-…`/`sk-or-v1-…` fingerprints).

## Out of scope (separate follow-ups)

- Streamable HTTP transport (this PR is stdio-only).
- Additional tools beyond `garra_ask` (e.g. `garra_chat`,
  `garra_files`).
- `--enable-tools` opt-in for `garra ask` (and MCP propagation).
- Streaming partial responses via MCP.
- `RedactingWriter` extension for provider error payloads.
- Automatic `openrouter/free → openrouter/auto` fallback.
- Per-user authentication / permissions.
- MCP server telemetry / Prometheus counters.

## See also

- [`docs/cli-ask.md`](cli-ask.md) — `garra ask` reference.
- [`docs/configuration.md`](configuration.md) — provider/model resolution.
- `plans/0102-gar-583-mcp-server-stdio.md` — this PR's plan.
- [Model Context Protocol specification](https://modelcontextprotocol.io/).
