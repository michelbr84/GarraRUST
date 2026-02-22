# MCP (Model Context Protocol)

MCP lets you connect external tool servers to GarraIA. Any MCP-compatible server - filesystem access, GitHub, databases, web search - becomes available as native agent tools.

## How It Works

1. You configure MCP servers in `config.yml` or `~/.garraia/mcp.json`
2. At startup, GarraIA connects to each enabled server and discovers its tools
3. MCP tools appear alongside built-in tools with namespaced names: `server.tool_name`
4. The agent can call them like any other tool during conversations

## Transports

- **stdio** (default) - GarraIA spawns the server process and communicates via stdin/stdout
- **HTTP** - Connect to a remote MCP server over HTTP (tracked in [#80](https://github.com/michelbr84/GarraRUST/issues/80))

## Configuration

MCP servers can be configured in two places. Both are merged at startup.

### config.yml

Add an `mcp:` section to `~/.garraia/config.yml`:

```yaml
mcp:
  filesystem:
    command: npx
    args: ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
    enabled: true

  github:
    command: npx
    args: ["-y", "@modelcontextprotocol/server-github"]
    env:
      GITHUB_PERSONAL_ACCESS_TOKEN: "ghp_..."
```

### mcp.json (Claude Desktop compatible)

You can also use `~/.garraia/mcp.json`, which follows the same format as Claude Desktop's MCP configuration:

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
    },
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": {
        "GITHUB_PERSONAL_ACCESS_TOKEN": "ghp_..."
      }
    }
  }
}
```

If the same server name appears in both files, the `config.yml` entry takes precedence.

## McpServerConfig Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `command` | string | (required) | Executable to spawn |
| `args` | string[] | `[]` | Command-line arguments |
| `env` | map | `{}` | Environment variables passed to the process |
| `transport` | string | `"stdio"` | Transport type (`stdio` or `http`) |
| `url` | string | (none) | URL for HTTP transport |
| `enabled` | bool | `true` | Whether to connect at startup |
| `timeout` | integer | `30` | Connection timeout in seconds |

## CLI Commands

### List configured servers

```bash
garraia mcp list
```

Shows all configured MCP servers with their enabled status, command, args, and timeout.

### Inspect tools

```bash
garraia mcp inspect <name>
```

Connects to the named server, discovers all available tools, and prints each one as `server.tool_name` with its description. Disconnects after inspection.

### List resources

```bash
garraia mcp resources <name>
```

Connects to the server and lists all available resources with URI, MIME type, name, and description.

### List prompts

```bash
garraia mcp prompts <name>
```

Connects to the server and lists all available prompts with their names, descriptions, and arguments (including required flags).

## Tool Namespacing

MCP tools are namespaced with the server name to avoid collisions. For example, if you have a server named `filesystem` that exposes a `read_file` tool, it appears as `filesystem.read_file` in the agent's tool list.

This means multiple MCP servers can expose tools with the same name without conflict.

## Examples

### Filesystem server

Give the agent access to read and write files in a specific directory:

```yaml
mcp:
  filesystem:
    command: npx
    args: ["-y", "@modelcontextprotocol/server-filesystem", "/home/user/documents"]
```

### GitHub server

Let the agent interact with GitHub repositories:

```yaml
mcp:
  github:
    command: npx
    args: ["-y", "@modelcontextprotocol/server-github"]
    env:
      GITHUB_PERSONAL_ACCESS_TOKEN: "ghp_..."
```

### SQLite server

Query a local database:

```yaml
mcp:
  sqlite:
    command: npx
    args: ["-y", "@modelcontextprotocol/server-sqlite", "/path/to/database.db"]
```

### HTTP server (future)

Connect to a remote MCP server:

```yaml
mcp:
  remote-tools:
    transport: http
    url: "https://mcp.example.com"
    timeout: 60
```

## Implementation Details

- MCP support is feature-gated behind the `mcp` feature in `garraia-agents` (enabled by default)
- Uses the `rmcp` crate (official Rust MCP SDK)
- `McpManager` in `crates/garraia-agents/src/mcp/manager.rs` handles connections
- `McpTool` in `crates/garraia-agents/src/mcp/tool_bridge.rs` bridges MCP tool definitions to the internal tool interface

## Limitations and Future Work

Tracked in [#80](https://github.com/michelbr84/GarraRUST/issues/80):

- HTTP transport support
- MCP resources integration
- MCP prompts integration
- Auto-reconnect on server crash
