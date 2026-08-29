# Pairing GarraIA with another agent (Hermes) over MCP

How to wire GarraIA and an external agent — Hermes is the running
example, but everything here applies to any MCP-speaking agent — in
**both** directions, with an explicit security policy.

> **Project-rule note:** ROADMAP §1.4 and the root CLAUDE.md forbid
> *copying Hermes Agent code* into this repository (the acceptance check
> greps `Cargo.lock` for Hermes imports). Interoperating with a Hermes
> instance over the MCP wire protocol does **not** violate that rule —
> protocol interop is not code import.

## The two directions are two different mechanisms

| Direction | Mechanism | Where tools appear |
|-----------|-----------|--------------------|
| Hermes → GarraIA | Hermes runs `garra mcp-server` (stdio) and calls the `garra_ask` tool | Inside Hermes |
| GarraIA → Hermes | GarraIA's gateway spawns a Hermes MCP server declared in `mcp.json` / `config.yml` | In the **gateway** agent runtime (web console chat, Telegram/Discord/… channels). `garra ask` and `garra chat` do **not** consume client-side MCP tools. |

## Direction 1 — Hermes → GarraIA

Register GarraIA in Hermes' MCP config (already covered in detail by
[cli-mcp-server.md](cli-mcp-server.md)):

```json
{
  "mcpServers": {
    "garraia": {
      "command": "garraia",
      "args": ["mcp-server"]
    }
  }
}
```

Recommended operator limits (env vars for the spawned process, read once
at startup — see [cli-mcp-server.md](cli-mcp-server.md#operator-limits-env-vars-opt-in)):

```json
{
  "mcpServers": {
    "garraia": {
      "command": "garraia",
      "args": ["mcp-server"],
      "env": {
        "GARRAIA_MCP_MODEL_ALLOWLIST": "openrouter/free",
        "GARRAIA_MCP_MAX_TIMEOUT_SECS": "120"
      }
    }
  }
}
```

With the allowlist set, a caller passing `model: "openrouter/auto"` (or
any other model) gets a clean `invalid_params` rejection instead of
spending money.

## Direction 2 — GarraIA → Hermes

Declare Hermes in the **active config directory** (default
`~/.config/garraia/`; confirm with `garraia config check`), either in
`mcp.json` or under `mcp:` in `config.yml`.

### If Hermes speaks MCP over stdio (preferred)

No persistent Hermes service is needed — the gateway spawns the process
on demand, monitors its health, and auto-restarts it with exponential
backoff:

```json
{
  "mcpServers": {
    "filesystem": { "command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem", "/root"] },
    "hermes": {
      "command": "hermes",
      "args": ["mcp", "serve"],
      "timeout": 60,
      "allowed_tools": ["hermes_ask"]
    }
  }
}
```

`allowed_tools` is the client-side allowlist (bare tool names): only the
listed tools are registered into the agent runtime, and calls to
anything else are refused. Empty/omitted = all discovered tools.

### If Hermes only serves MCP over HTTP

Stock `garra` binaries are **stdio-only** MCP clients (`transport: http`
is skipped with a warning). Two options:

1. **stdio→HTTP bridge** (works with the stock binary; the pattern is
   also shown in `mcp.json.example`):

   ```json
   {
     "mcpServers": {
       "hermes": {
         "command": "npx",
         "args": ["-y", "supergateway", "--streamableHttp", "http://127.0.0.1:8811/mcp"],
         "timeout": 60,
         "allowed_tools": ["hermes_ask"]
       }
     }
   }
   ```

2. **Source build with native HTTP**:
   `cargo build --release -p garraia --features mcp-http`, then use
   `"transport": "http", "url": "http://127.0.0.1:8811/mcp"` (a
   `command` value is still required by the config schema today).

### Verify

```bash
garraia restart          # or stop + start
garraia mcp list         # hermes should be listed [enabled]
# in the gateway log, expect:
#   MCP server 'hermes': registered N tool(s)
#   MCP tools registered into AgentRuntime … "hermes__…"
```

Then, in the web console (`http://127.0.0.1:3888/`) or any connected
channel, ask Garra to use a `hermes__*` tool. Connection failures show
up as `Error` status in `garraia mcp list` / the admin UI.

## Loop topology — read before closing the circle

With both directions active, a cycle exists:
`Hermes → garra_ask → LLM` and `gateway conversation → hermes tool → Hermes`.

Today this is safe **because `garra_ask` is tool-less by construction**:
it calls the LLM provider in-process and cannot invoke MCP tools, so a
Hermes call into GarraIA always terminates there (audit tests in
`crates/garraia-cli/src/mcp_server.rs` enforce this at `cargo test`
time). There is **no depth counter or cycle detection** in the gateway
agent runtime — if a future GarraIA MCP tool were backed by the
tool-enabled gateway runtime, or if the Hermes tool you expose to
GarraIA can itself call `garra_ask`, an unbounded ping-pong becomes
possible, bounded only by per-call timeouts.

Practical rules:

- Keep `garra_ask` the only tool GarraIA exposes to Hermes.
- In GarraIA, allowlist only Hermes tools that do **not** call back into
  GarraIA (or that answer from Hermes' own state).
- Set explicit timeouts on both sides (`timeout` in the client config;
  `GARRAIA_MCP_MAX_TIMEOUT_SECS` on the server side).

## Security policy checklist

- [ ] `GARRAIA_MCP_MODEL_ALLOWLIST` set where Hermes spawns
      `garra mcp-server` (blocks `openrouter/auto` and other expensive
      models).
- [ ] `GARRAIA_MCP_MAX_TIMEOUT_SECS` set (e.g. `120`).
- [ ] `allowed_tools` set on the `hermes` entry in GarraIA's config —
      enforced both at tool registration and on every dispatch.
- [ ] Timeout (`timeout`) set on the `hermes` entry.
- [ ] No secrets in `env` blocks that the other agent shouldn't see;
      remember MCP child processes inherit what you pass there.
- [ ] Gateway not exposed on the network without auth (`/v1/auth/*`
      secrets configured) if Hermes runs on another machine.

## See also

- [docs/mcp.md](mcp.md) — GarraIA as MCP **client** (config reference).
- [docs/cli-mcp-server.md](cli-mcp-server.md) — GarraIA as MCP
  **server** (`garra_ask` contract, operator limits, stdio invariants).
- `mcp.json.example` — bridge recipes (`mcp-remote`, `supergateway`).
