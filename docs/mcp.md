# MCP (Model Context Protocol)

GarraIA supports the Model Context Protocol for connecting to external tools and services.

## Setup

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

Create `~/.garraia/mcp.json`:

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
