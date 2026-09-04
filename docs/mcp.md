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

### Tool inventory stays in sync with the runtime

A reconnect restores the server's tools into the **agent runtime**, not just
into the connection pool. Before `v0.3.9` it did not (issue #924): the runtime's
tool list was written once during boot and then frozen, so a server that
connected late — or reconnected, or was added through the admin API — showed up
as `connected` with N tools while the agent could not call any of them. They
still worked through `garraia mcp call` and the admin UI, which build the call
on demand, so the failure looked like a cosmetic counter bug.

`GET /api/mcp/tools` reports the breakdown that makes this checkable:

```json
{
  "runtime_tool_count": 20,
  "native_tool_count": 6,
  "mcp_tool_count": 14,
  "runtime_tools_detailed": [
    { "name": "bash", "source": "native" },
    { "name": "filesystem__read_file", "source": "mcp", "server": "filesystem" }
  ],
  "mcp_servers": [{ "server": "filesystem", "tool_count": 14, "connected": true }]
}
```

`GET /api/mcp/health` adds `runtime_in_sync_with_manager`. If it is ever
`false`, the runtime and the connection pool disagree — report it, because the
sync runs on every health tick and should converge within 30 seconds.

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

If `garraia mcp call` reaches the tool but the **agent** says it has no such
tool, compare the two counts — that asymmetry is exactly what issue #924 was:

```bash
curl -s localhost:3888/api/mcp/tools | jq '{runtime: .mcp_tool_count, servers: .mcp_servers}'
curl -s localhost:3888/api/mcp/health | jq '.runtime_in_sync_with_manager'
```

### Server won't start on Termux (Android)

`env: 'node': No such file or directory`, or a child that dies immediately
with `Permission denied`.

Servers installed manually through npm/pip carry `/usr/bin/env` shebangs, and
Termux has no `/usr/bin`. The fix is the termux-exec shim, which rewrites those
paths at exec time:

```bash
pkg install termux-exec
```

From `v0.3.7` the gateway also injects
`LD_PRELOAD=$PREFIX/lib/libtermux-exec.so` into MCP children on Android when
nothing else set it — but the shim still has to be installed for that to mean
anything. An explicit `env.LD_PRELOAD` on the server's config is never
overridden.

For a single binary you installed by hand, `termux-fix-shebang <file>` rewrites
its shebang in place instead.

`garraia doctor` reports whether the shim is present, along with the rest of
the Termux environment. The reverse direction — an external MCP host failing to
start `garra mcp-server` — is a different problem with a different fix; see
[`docs/cli-mcp-server.md`](cli-mcp-server.md).

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
