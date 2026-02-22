# Architecture

GarraIA is a Rust-based AI agent framework designed for performance and security.

## Structure

```
crates/
  garraia-cli/        # CLI, init wizard, daemon management
  garraia-gateway/    # WebSocket gateway, HTTP API, sessions
  garraia-config/     # YAML/TOML loading, hot-reload, MCP config
  garraia-channels/   # Discord, Telegram, Slack, WhatsApp, iMessage
  garraia-agents/     # LLM providers, tools, MCP client, agent runtime
  garraia-db/         # SQLite memory, vector search (sqlite-vec)
  garraia-plugins/    # WASM plugin sandbox (wasmtime)
  garraia-media/      # Media processing
  garraia-security/   # Credential vault, allowlists, pairing, validation
  garraia-skills/     # SKILL.md parser, scanner, installer
  garraia-common/     # Shared types, errors, utilities
```

## Tools

The agent runtime includes 6 built-in tools that the LLM can invoke during a conversation. The tool loop runs for up to 10 iterations per message.

| Tool | Description |
|------|-------------|
| `bash` | Execute shell commands (30s timeout, 32 KB max output) |
| `file_read` | Read file contents (1 MB max, path traversal prevention) |
| `file_write` | Write file contents (1 MB max, path traversal prevention) |
| `web_fetch` | Fetch web pages (30s timeout, 1 MB max response) |
| `web_search` | Search via Brave Search API (requires `BRAVE_API_KEY`) |
| `schedule_heartbeat` | Schedule future agent wake-ups (30-day max, 5 pending limit) |

See [Tools](./tools.md) for the full reference.

## MCP (Model Context Protocol)

GarraIA can connect to external MCP servers to extend the agent's capabilities. MCP tools are discovered at startup and appear as native agent tools with namespaced names (`server.tool_name`).

Configuration lives in `config.yml` under the `mcp:` section or in `~/.garraia/mcp.json` (Claude Desktop compatible format). Both sources are merged at startup.

The `garraia-agents` crate contains the MCP client (using the `rmcp` crate) with a tool bridge that converts MCP tool definitions into the internal tool format.

See [MCP](./mcp.md) for the full reference.

## Architectural Decision Records

See [Decision Records](./adr/README.md).
