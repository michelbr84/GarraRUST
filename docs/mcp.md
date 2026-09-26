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
    args: ["-y", "@modelcontextprotocol/server-filesystem@2026.8.31", "/tmp"]
  
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
    args: ["-y", "@modelcontextprotocol/server-filesystem@2026.8.31", "/path/to/directory"]
```

Pin the version (`@2026.8.31`). A bare `npx -y @modelcontextprotocol/server-filesystem`
resolves to whatever is newest on the registry every time the npx cache is
cold, so the gateway runs a build nobody tested. A dist-tag (`@latest`,
`@next`) or a range (`@^1`, `@~2026.8`) floats the same way and is not
counted as pinned. New installs are provisioned with the pinned form (#1346);
the `mcp.filesystem_pinned` check of `GET /api/diagnostics` warns about an
existing unpinned entry and prints the exact `args` to paste — `mcp.json`
itself is never rewritten.

Pinning fixes the top-level package only. Its own dependencies
(`@modelcontextprotocol/sdk`, `zod`, `glob`, ...) are declared with semver
ranges and the package ships no lockfile, so a cold `npx -y` still resolves
the newest versions that match those ranges.

#### Auto-provisioned root

On the very first boot, when `<config_dir>/mcp.json` does not exist yet, the
gateway writes one with a `filesystem` server. The directory it is rooted at
depends on the **execution profile** (ADR 0024, #1329;
[`execution-profiles.md`](execution-profiles.md)):

| `execution.profile` | Root passed to `server-filesystem` |
|---|---|
| `standard` (default) | `agent.file_roots` from the config when set; otherwise `<data_dir>/workspace`. `GARRAIA_FILE_ROOTS` and the session `working_dir`, which the native file-tool jail (#1244) also adds, are **not** used here |
| `isolated-pod` | `execution.pod_root` when set; otherwise `<data_dir>/workspace` |

`$HOME` is **never** an implicit root in either profile. The effective root
is logged at provisioning time and reported by the `mcp.filesystem_root`
check of `GET /api/diagnostics`. On first boot only the default
`<data_dir>/workspace` is created; a **declared** root (`agent.file_roots`,
`execution.pod_root`) must already exist — if it does not, provisioning is
skipped with a `warn!` naming the missing root, and nothing wider is created
in its place.

**Per-session confinement (#1482).** The autoprovisioned `filesystem` root is
`<data_dir>/workspace` in `standard`, which is the *parent* of every
per-session workspace (#1449). So the gateway runs every MCP filesystem call
through the same `FileJail` as the native file tools: `path`, `paths`,
`source` and `destination` are resolved and must fall under the calling
session's directory (plus `agent.file_roots`), `list_allowed_directories`
answers with the session's effective roots instead of the server's, and a
path outside gets the same single denial message the native tools use. The
local CLI (`garraia mcp call`) runs without that jail: there is a human at
the keyboard.

An existing `mcp.json` is **never rewritten** — the only gate is "file
absent". Installations provisioned before v0.4.4 therefore keep the old
`$HOME` root; in `standard` the diagnostic flags it as `Warning` (the
`filesystem` entry points outside the declared roots). The check reads both
`mcp.json` and the `mcp:` section of `config.yml`, which wins, the same merge
the gateway uses to spawn servers. To fix it, change the last `args` entry of
`filesystem` to a directory inside the declared roots (one of
`agent.file_roots`, or `<data_dir>/workspace`) and restart the gateway. Set `GARRAIA_DISABLE_MCP_AUTOPROVISION=1` to skip provisioning
entirely.

Which tools of that server the model may call is decided per turn by the
mode's `ToolGate`, by tool name (`filesystem__write_file`), never by the
server's existence — see [`whatsapp.md`](whatsapp.md#ferramentas-e-servidores-mcp)
for the channel floor.

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
      "args": ["-y", "@modelcontextprotocol/server-filesystem@2026.8.31", "/tmp"]
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

### Server stuck on `ERR_MODULE_NOT_FOUND` (corrupted npx cache)

`npx -y <package>` installs the package into `<npm cache>/_npx/<16 hex>/` and
runs it from there. An install that was interrupted (killed boot, full disk,
registry hiccup) leaves that directory half-populated, and from then on every
spawn dies with `Error [ERR_MODULE_NOT_FOUND]: Cannot find module
'.../_npx/<hash>/node_modules/...'` — npx never repairs an entry that exists.

Since v0.4.5 the gateway handles this itself (#1346):

- The child's stderr is captured instead of inherited. It is forwarded at
  `debug` level only (`RUST_LOG=garraia_agents=debug` to see it), so a failing
  server no longer floods the log with Node stack traces.
- When a spawn fails with a missing module inside an npx entry, the gateway
  removes **that one entry** and retries the connect once — only if the
  command is `npx`, the path resolves to exactly
  `<npm cache>/_npx/<16 hex>` of the cache the gateway gave the child
  (`npm_config_cache` in the server's `env`, else `$HOME/.npm`, or
  `%LOCALAPPDATA%\npm-cache` on Windows), it is a real directory (no symlink
  is ever followed), and its `package.json` lists the configured package. It
  never runs `npm`, never touches anything outside `_npx`, and does it at most
  once per server per gateway process. A manual restart re-arms it.
- If the server still cannot start, it keeps the normal backoff
  (`max_restarts`, default 5). When the retries run out the gateway logs one
  `error` naming the cause, and stops — later health ticks stay silent.

`GET /api/mcp/health` lists the failed server with `"connected": false`,
`"status": "retrying"` or `"failed"`, the classified `cause`
(`npx_cache_corrupt`, `disk_full`, `other`) and a short `last_error`.
`last_error` names the cause but never a path. `GET /api/diagnostics` has an
`mcp.servers` row whose `next_step` names the cache directory to remove — only
when the gateway confirmed it is the entry of the configured package inside
the npm cache it gave the child; a path the child printed is never repeated.
To fix it by hand:

```bash
rm -rf ~/.npm/_npx/<hash>      # the directory named by the diagnostic
# or: npm cache verify
curl -X POST localhost:3888/admin/api/mcp/filesystem/restart   # or restart garraia
```

After the restart the `mcp.servers` row of `GET /api/diagnostics` turns `ok`
once the handshake succeeds.

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
