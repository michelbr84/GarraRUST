# `garra mcp-server` — MCP server exposing `garra ask`

> GAR-583 — stdio Model Context Protocol server. Exposes `garra_ask`,
> an LLM-only tool that runs the same code path as
> [`garra ask`](cli-ask.md) in-process — no subprocess, no shell.
> Optionally (operator opt-in via `GARRAIA_MCP_ENABLE_TOOLS=1`) it also
> exposes `garra_agent`, a **full-agent** sibling with shell, file,
> git and web tools — see
> ["Full agent tool (`garra_agent`)"](#full-agent-tool-garra_agent--operator-opt-in)
> below.
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

To also expose the full-agent tool (opt-in — read the
[security surface](#security-surface-read-before-enabling) first), set
the env in the server entry:

```json
{
  "mcpServers": {
    "garra": {
      "command": "garra",
      "args": ["mcp-server"],
      "env": { "GARRAIA_MCP_ENABLE_TOOLS": "1" }
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

## Operator limits (env vars, opt-in)

By default the server accepts any model and any `timeout_secs` up to the
schema max — the historic behavior. When exposing `garra_ask` to another
agent (Hermes, Claude Desktop, a CI bot), the operator can restrict it
at startup via env vars, read once when `garra mcp-server` boots:

| Env var | Effect |
|---------|--------|
| `GARRAIA_MCP_MODEL_ALLOWLIST` | Comma-separated model names. When set, a call whose (explicit or defaulted) `model` is not in the list is rejected with `invalid_params`. The operator's way to keep `openrouter/auto` unreachable: `GARRAIA_MCP_MODEL_ALLOWLIST=openrouter/free`. Applies to BOTH tools. |
| `GARRAIA_MCP_MAX_TIMEOUT_SECS` | Cap on `timeout_secs` (clamped to each tool's schema max: 600 for `garra_ask`, 1800 for `garra_agent`). Calls above the cap are rejected; a call omitting `timeout_secs` gets `min(schema_default, cap)` — 60 for `garra_ask`, 300 for `garra_agent`. |
| `GARRAIA_MCP_ENABLE_TOOLS` | Opt-in for the full-agent tool. Truthy values: `1`, `true`, `yes` (case-insensitive). When unset, `garra_agent` is not advertised in `tools/list` and calls come back as `unknown tool` — the surface is byte-identical to the pre-agent server. |
| `GARRAIA_MCP_ALLOWED_DIRS` | Optional comma-separated directory allowlist for `garra_agent`'s file tools (`file_read`/`file_write` relative paths). Falls back to the server's CWD. **UX belt only, not a boundary** — unrestricted `bash` can read anywhere the process can. |

The `provider` enum advertised in the JSON schema
(`ollama|anthropic|openai|openrouter`) is enforced at runtime — plus any
provider alias configured under `llm:` in `config.yml`. The active
policy is logged to stderr at startup.

## Full agent tool (`garra_agent`) — operator opt-in

`garra_agent` runs the GarraIA assistant as a **complete agent** — the
same runtime the Telegram/gateway path uses — instead of the LLM-only
`garra_ask`. One-shot per call: fresh session (`mcp-<uuid>`), empty
history, one turn. The tool is only advertised and dispatched when the
server process starts with `GARRAIA_MCP_ENABLE_TOOLS` set.

### Registered tools

Mirrors the gateway bootstrap exactly (`garraia-gateway/src/bootstrap/`):

| Tool | Notes |
|------|-------|
| `bash` | Full-auto (Telegram parity): only the `safety_gate` DENY_LIST blocks; no per-command confirmation. Confirmation would deadlock a stateless MCP call anyway — it needs conversation history to be approved. |
| `file_read` / `file_write` | Relative paths resolve against `working_dir` (or the server CWD). `GARRAIA_MCP_ALLOWED_DIRS` narrows them. |
| `web_fetch` | No blocked-domain list by default. |
| `git_diff` | Read-only git inspection. |
| `web_search` | Only when a Brave key exists (`config.yml llm.brave.api_key` or `BRAVE_API_KEY` env). |

### Tool schema (`garra_agent`)

| Field           | Type    | Required | Default            | Notes |
|-----------------|---------|----------|--------------------|-------|
| `message`       | string  | yes      | —                  | Max 64 KiB. The task for the agent. |
| `provider`      | string  | no       | `openrouter`       | Same enum as `garra_ask`. |
| `model`         | string  | no       | `openrouter/free`  | Pass `openrouter/auto` for complex tasks. |
| `timeout_secs`  | integer | no       | `300`              | Range `[5, 1800]`. **Wall-clock cap for the ENTIRE agent loop** — every LLM round-trip plus every tool execution. |
| `system_prompt` | string  | no       | generated          | Max 8 KiB. The default prompt names every registered tool and instructs the model to investigate instead of describing. |
| `working_dir`   | string  | no       | —                  | Directory for file-tool relative paths. **`bash` ignores it** — it runs in the server's CWD; the default system prompt tells the model to prefix bash commands with `cd <dir> && `. Must exist and be a directory. |

### Response shape (`garra.agent.v1`)

Same shape as `garra.ask.v1` plus `session_id` and a `tool_calls`
summary (each entry: `name`, `duration_ms`, `success`, `summary` —
redacted and truncated at the source by the runtime):

```json
{
  "schema": "garra.agent.v1",
  "ok": true,
  "answer": "...",
  "provider": "openrouter",
  "model": "z-ai/glm-5.3-flash",
  "latency_ms": 15230,
  "session_id": "mcp-0b6c…",
  "tool_calls": [
    {"name": "bash", "duration_ms": 120, "success": true, "summary": "3 files"}
  ]
}
```

On failure or timeout, `ok` is `false` and `error` carries
`{kind, message}` (same stable labels as `garra.ask.v1`) — and
`tool_calls` still travels, so the host sees what the agent executed
before dying.

### Security surface (read before enabling)

- **The bash tool is full-auto** — the operator opt-in chooses Telegram
  parity. The only hard gate is the `safety_gate` DENY_LIST
  (substring blocklist, trivially bypassable by quoting/encoding).
- **The child shell inherits the MCP server process environment with no
  scrubbing.** Depending on how the server was started, that can include
  `ANTHROPIC_API_KEY`, `OPENROUTER_API_KEY`, `SSH_AUTH_SOCK`,
  `XAUTHORITY`, and any secrets `dotenvy` auto-loaded from a `.env` in
  the CWD. A prompt-injected model can exfiltrate them (e.g. `curl` with
  `$OPENROUTER_API_KEY`). Do not enable this flag for MCP servers
  exposed to third-party agents; prefer per-call `working_dir` scoping
  plus a clean env wrapper if you must.
- **`allowed_dirs` is UX, not a boundary** — unrestricted bash reaches
  the whole filesystem.
- **No per-caller authentication or rate limiting** (same as
  `garra_ask`); concurrent calls from the host run concurrently.
- **Config write access**: the file tools can edit `~/.garraia/config.yml`
  and anything else writable by the process user.

Implementation note: the agent handler lives in
`crates/garraia-cli/src/mcp_agent.rs`, a module the audit tests in
`mcp_server.rs` deliberately do NOT scan (they scan only their own
file). `mcp_server.rs` remains a pure dispatcher — it names no runtime
tool constructor and spawns no process.

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

Since the `garra_agent` opt-in, the agent machinery (tool
constructors, shell execution) lives in
`crates/garraia-cli/src/mcp_agent.rs`, which those audits
deliberately do NOT scan — they `include_str!` only `mcp_server.rs`
itself. The dispatcher file stays free of runtime tool names and
process spawning; the escape hatch is documented in the module
docblock of `mcp_agent.rs`. `ask.rs` has its own third audit
(`ask_module_never_registers_a_tool`, GAR-579) that also scans
`mcp_server.rs` for spinner references — untouched.

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

### Termux: the host cannot start the server at all

`Connection closed` with no output, or `Permission denied` — and
`garra mcp-server` works fine when you run it yourself in the Termux shell.

MCP hosts spawn their servers with a filtered environment (`env -i PATH=…
HOME=…`, good security hygiene), which drops `LD_PRELOAD`. On Android the ELF
exec resolves through the termux-exec shim, so without it the exec fails
**before** `garraia` runs — there is no point inside the process at which it
could repair this, which is why the fix lives in the parent.

Reproduce:

```bash
env -i PATH=$PATH HOME=$HOME garraia mcp-server                        # fails
LD_PRELOAD=$PREFIX/lib/libtermux-exec.so garraia mcp-server            # works
```

From `v0.3.7`, `install.sh` writes `$PREFIX/bin/garra-mcp-server` on the
Android branch: a wrapper that exports the shim (when installed) and execs the
CLI. Point the host at it:

```json
{ "mcpServers": { "garra": { "command": "/data/data/com.termux/files/usr/bin/garra-mcp-server" } } }
```

Or configure the host's environment directly, which is equivalent:

```json
{ "mcpServers": { "garra": {
    "command": "garraia", "args": ["mcp-server"],
    "env": { "LD_PRELOAD": "/data/data/com.termux/files/usr/lib/libtermux-exec.so" }
} } }
```

`pkg install termux-exec` is a prerequisite either way. After a build from
source the wrapper does not exist — `docs/installation.md` has the snippet to
recreate it. `garraia doctor` checks for both the shim and the wrapper.

### Termux: still `Permission denied` with the wrapper

The wrapper above assumes two things that a hard-filtering host breaks: that the
host can exec a *script*, and that the shim is what the inner exec needs. Issue
#920 hit both, on Android 13 with `env -i PATH=… HOME=…`:

```bash
# the host cannot exec the wrapper script at all
env -i PATH=$PATH HOME=$HOME $PREFIX/bin/garra-mcp-server
# failed to run command 'garra-mcp-server': Permission denied

# or the wrapper runs, and the inner exec dies
# garra-mcp-server: 8: exec: /data/data/…/garraia: Permission denied
```

Hand the ELF to the Android dynamic loader instead. It maps the binary directly,
so neither termux-exec nor `LD_PRELOAD` is involved:

```bash
/system/bin/linker64 $PREFIX/bin/garraia mcp-server                    # works, no env at all
```

From `v0.3.9`, `install.sh` also writes `$PREFIX/bin/garra-mcp-server-linker`,
which wraps exactly that:

```json
{ "mcpServers": { "garra": { "command": "/data/data/com.termux/files/usr/bin/garra-mcp-server-linker" } } }
```

That wrapper is still a script, so it cannot help with the first failure above.
When the host cannot exec a script at all, put the loader in `command` and let
the binary be an argument — no wrapper in between. This is the configuration
verified end to end in issue #920, with the MCP handshake and `garra_ask`
working under a fully filtered environment:

```json
{ "mcpServers": { "garra": {
    "command": "/system/bin/linker64",
    "args": ["/data/data/com.termux/files/usr/bin/garraia", "mcp-server"]
} } }
```

`/apex/com.android.runtime/bin/linker64` is the fallback path on Android 10+.
`garraia doctor` prints this recipe with your real install path filled in.

## Security notes

- **In-process only**. The tool handler calls `ask::ask_oneshot`
  directly. There is no subprocess spawn, no shell invocation, no
  PATH lookup. Path hijacking and shell injection are not in the
  threat model.
- **LLM-only runtime by default**. No `bash`/`file_read`/`file_write`/`git_diff`
  tools are registered on the agent runtime unless the operator sets
  `GARRAIA_MCP_ENABLE_TOOLS` — and even then the registration happens
  in `mcp_agent.rs`, never in this dispatcher. The audit test prevents
  regression in `mcp_server.rs`.
- **Prompt size bounded**. Messages ≤ 64 KiB, system prompts ≤ 8 KiB.
- **Output bounded**. `max_tokens: 4096` in the runtime caps the
  response.
- **No loops**. The provider (OpenRouter, Anthropic, etc.) is a plain
  HTTPS endpoint; it doesn't call MCP servers back.
- **Provider errors sanitized** before reaching the response or
  stderr (regex-based redaction of `sk-…`/`sk-or-v1-…` fingerprints).
- **Operator limits** — see "Operator limits" above for the
  `GARRAIA_MCP_MODEL_ALLOWLIST` / `GARRAIA_MCP_MAX_TIMEOUT_SECS` /
  `GARRAIA_MCP_ENABLE_TOOLS` knobs.
  There is still no per-caller authentication or rate limiting: the
  trust boundary is process-level (whoever can spawn the binary can
  call the tools within the configured policy).
- **`garra_agent` extends the threat model when enabled** — env
  inheritance by the bash child, full filesystem via unrestricted
  bash, config write access. Read
  ["Security surface (read before enabling)"](#security-surface-read-before-enabling)
  before turning the flag on.

## Out of scope (separate follow-ups)

- Streamable HTTP transport (this PR is stdio-only).
- Additional conversational tools beyond `garra_ask`/`garra_agent`
  (e.g. `garra_chat`, `garra_files`).
- `--enable-tools` opt-in for `garra ask` (the MCP-side opt-in
  `GARRAIA_MCP_ENABLE_TOOLS` shipped 2026-09; the CLI `ask`
  flag remains open).
- Streaming partial responses via MCP.
- `RedactingWriter` extension for provider error payloads.
- Automatic `openrouter/free → openrouter/auto` fallback.
- Per-user authentication / permissions.
- MCP server telemetry / Prometheus counters.
- Bash env allowlist for `garra_agent` (`GARRAIA_MCP_BASH_ENV_ALLOWLIST`)
  to stop child-process env inheritance.

## See also

- [`docs/mcp.md`](mcp.md) — the reverse direction: GarraIA as MCP
  **client** consuming external servers.
- [`docs/hermes-integration.md`](hermes-integration.md) — pairing
  GarraIA with another agent in both directions (loop topology, policy).
- [`docs/cli-ask.md`](cli-ask.md) — `garra ask` reference.
- [`docs/configuration.md`](configuration.md) — provider/model resolution.
- `plans/0102-gar-583-mcp-server-stdio.md` — this PR's plan.
- [Model Context Protocol specification](https://modelcontextprotocol.io/).
