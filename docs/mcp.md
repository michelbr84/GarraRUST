# MCP (Model Context Protocol)

GarraIA supports the Model Context Protocol for connecting to external tools and services.

## Setup

Both `config.yml` and `mcp.json` live in the **active config directory**:
`$GARRAIA_CONFIG_DIR` if set, else `~/.config/garraia` (the default on
new installs), else legacy `~/.garraia`. Run `garraia config check` to
confirm which directory the gateway actually reads — files edited in the
wrong one are silently ignored.

### Stdio Transport

Configure MCP servers in `config.yml`:

```yaml
mcp:
  filesystem:
    command: npx
    args: ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
  
  github:
    command: npx
    args: ["-y", "@modelcontextprotocol/server-github"]
    env:
      GITHUB_TOKEN: "your-github-token"
```

### HTTP Transport

For remote MCP servers (requires `mcp-http` feature):

```yaml
mcp:
  remote-server:
    transport: http
    url: "http://localhost:3000/mcp"
```

## CLI Commands

### List MCP Servers

```bash
garraia mcp list
```

### Inspect Server

```bash
garraia mcp inspect <server-name>
```

### List Resources

```bash
garraia mcp resources <server-name>
```

### List Prompts

```bash
garraia mcp prompts <server-name>
```

## Available MCP Servers

### Filesystem

Access local filesystem:

```yaml
mcp:
  filesystem:
    command: npx
    args: ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/directory"]
```

### GitHub

```yaml
mcp:
  github:
    command: npx
    args: ["-y", "@modelcontextprotocol/server-github"]
    env:
      GITHUB_TOKEN: "ghp_..."
```

### Database

```yaml
mcp:
  postgres:
    command: npx
    args: ["-y", "@modelcontextprotocol/server-postgres", "postgresql://user:pass@localhost/db"]
```

### AWS KB Retrieval

```yaml
mcp:
  aws:
    command: npx
    args: ["-y", "@modelcontextprotocol/server-aws-kb-retrieval-server"]
```

## Claude Desktop Compatibility

GarraIA is compatible with Claude Desktop MCP configuration.

Create `mcp.json` in the active config directory (default
`~/.config/garraia/mcp.json`):

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
    }
  }
}
```

## Tool Namespacing

MCP tools are namespaced with the server name:

```
server_name.tool_name
```

Example: `filesystem.read_file`

## Health Monitoring

MCP servers are monitored for health:

```bash
garraia health
```

Check MCP status in health output.

## Troubleshooting

### Server won't start

Check logs:
```bash
garraia logs | grep mcp
```

### Tool not found

Verify server is running:
```bash
garraia mcp list
```

### Connection timeout

Increase timeout in config:

```yaml
timeouts:
  mcp:
    default_secs: 60  # Increase from default
```

## See also

- [`docs/cli-mcp-server.md`](cli-mcp-server.md) — the reverse direction:
  GarraIA as MCP **server** exposing `garra_ask` to other hosts.
- [`docs/hermes-integration.md`](hermes-integration.md) — pairing
  GarraIA with another agent in both directions (loop topology, policy).
