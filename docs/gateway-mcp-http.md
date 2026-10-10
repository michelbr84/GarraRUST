# `POST /mcp` — GarraIA as an MCP server over HTTP

> Issue #1513. The gateway exposes a **Streamable HTTP** MCP endpoint at
> `http://127.0.0.1:3888/mcp`, so an external orchestrator that only accepts a
> URL (Paperclip's "Custom MCP server", Claude Code's HTTP transport, n8n, …)
> can use this Garra as a source of tools.
>
> This is the third MCP surface in the project, and they point in different
> directions. Getting them confused is the single most likely mistake here:
>
> | Surface | Direction | Transport | Doc |
> |---|---|---|---|
> | `mcp:` section in `config.yml` | Garra **consumes** other servers | stdio / HTTP | [mcp.md](./mcp.md) |
> | `garra mcp-server` | Garra **is** a server, one LLM tool | stdio | [cli-mcp-server.md](./cli-mcp-server.md) |
> | **`POST /mcp`** (this page) | Garra **is** a server, six gateway tools | Streamable HTTP | — |
>
> `garra_ask` is in both server surfaces since #1612. It is **off by default**
> here and has its own switch, allowlist and budget (§4.3), because it spends
> the owner's LLM key on behalf of an external caller. The stdio server keeps
> working as before, and both share the same one-shot core (`garraia-ask`).

## 1. Turning it on

Two keys turn the bridge on and unlock its writes, and three more govern
`garra_ask`. All are in `config.yml`, and all are off or minimal by default:

```yaml
gateway:
  # Required: the bridge refuses to mount without a credential (see §3).
  api_key: "uma-credencial-longa-e-aleatoria"
  mcp_http:
    enabled: true          # mounts POST /mcp
    allow_send: false      # unlocks garra_send_message (see §4.2)
    max_history_messages: 50
    allow_ask: false       # unlocks garra_ask, which spends LLM tokens (see §4.3)
    ask_allowed_models: [] # empty = only the project default model
    ask_budget_per_minute: 10
```

`GARRAIA_GATEWAY_API_KEY` works instead of `gateway.api_key`, with the usual
env-beats-file precedence.

Restart the gateway. Boot logs one line:

```
ponte MCP Streamable HTTP montada (host: so loopback) rota=/mcp envio_liberado=false destinos_liberados=0
```

Check the configuration before starting:

```bash
garra config check
```

It reports `gateway.mcp_http.enabled` when the bridge is enabled without a
credential (it would answer 404), and `gateway.mcp_http.allow_send` when sending
is enabled without an approved destination (every send would be refused).

## 2. Connecting a client

Point the host at `http://127.0.0.1:3888/mcp` with an `Authorization: Bearer`
header carrying `gateway.api_key`.

The transport is **stateless with JSON responses**: each `POST` is a complete
JSON-RPC exchange, there is no `Mcp-Session-Id` to carry, and the response is
`application/json` rather than an SSE frame. That makes `curl` an honest way to
check the endpoint:

```bash
KEY=uma-credencial-longa-e-aleatoria
curl -s http://127.0.0.1:3888/mcp \
  -H "Authorization: Bearer $KEY" \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}' | jq '.result.tools[].name'
```

The `Accept` header with **both** media types is part of the Streamable HTTP
spec; without it the transport answers `406 Not Acceptable`.

## 3. Security model

Four layers, and they are deliberately in different places.

### 3.1 The route does not exist unless asked for

With `gateway.mcp_http.enabled: false` (the default), `/mcp` is never registered.
A request gets the router's plain `404` — not a `403`, which would confirm that
something is there.

### 3.2 The route refuses to mount without `gateway.api_key`

This is stricter than the rest of `/api/*` on purpose. The gateway's api-key gate
is a **pass-through** when no key is configured (see
[`gateway_auth.rs`](../crates/garraia-gateway/src/gateway_auth.rs)), and that is
the right default for a local Web Console: whoever reaches the port is the owner.

It is not the right default here. This bridge hands an external process the list
of the owner's conversations and the contents of those conversations. So the
credential became a **precondition**: with `enabled: true` and no key, the bridge
does not mount, `/mcp` answers 404, and boot logs a warning naming the fix.

With the key configured, `/mcp` is behind the same gate as `/api/*` — `401` with
the standard body on a missing or wrong bearer.

### 3.3 Loopback only

`rmcp`'s `Host` validation is left at its default, which accepts only loopback
authorities. This is the SDK's anti-DNS-rebinding guard, and it matches the
"bind stays on 127.0.0.1" line in the issue. The consequence is intentional and
worth stating plainly: **a gateway bound to `0.0.0.0` still does not serve MCP to
the LAN.** Reaching it from another machine is an SSH tunnel's job.

### 3.4 What it inherits for free

`/mcp` sits inside the router's existing layers, so it also gets:

- the generic anti-CSRF guard ([`origin_guard`](../crates/garraia-gateway/src/origin_guard.rs)) — a
  `POST` fired from inside a browser carries `Origin` and dies there; an MCP
  client carries none and passes;
- the per-IP rate limit from `gateway.rate_limit`;
- the 4 MiB request body cap from `rmcp`.

## 4. The tools

| Tool | Returns | Writes? |
|---|---|---|
| `garra_status` | version, uptime, channels, conversations in memory | no |
| `garra_list_chats` | live conversations, optionally filtered by channel | no |
| `garra_read_history` | last N messages of one conversation, secrets redacted | no |
| `garra_pair_status` | channel pairing and allowlist state | no |
| `garra_send_message` | sends a message on a real channel | **yes** |
| `garra_ask` | asks the Garra's LLM one question (#1612) | spends LLM tokens |

Every response except `garra_ask` is a `garra.mcp.v1` envelope delivered as MCP
text content — versioned from the first release so the body can grow without
breaking whoever read the first one. `garra_ask` returns the `garra.ask.v1`
envelope of the stdio server, unchanged.

### 4.1 Reads

`garra_read_history` runs every message through
`garraia_security::redact_secrets`, the same redaction the log writer uses: a
token someone pasted into a conversation does not travel to an external
orchestrator just because it became history. The envelope says `redacted: true`
so the caller does not have to assume.

`garra_list_chats` reads what is in memory — the same cut as
`GET /api/sessions`. Use the `chat` field of a row as the argument to
`garra_read_history`.

### 4.2 `garra_send_message` — the one write

This is the only tool that reaches the outside world, and it needs **two
independent operator actions**:

1. `gateway.mcp_http.allow_send: true`, and
2. the destination chat listed in `channels.<name>.proactive_chat_ids`.

The second one is the approval. There is no human in the loop *inside* an MCP
call — no next turn in which the owner can say "yes" — so the bridge cannot ask
for confirmation the way the chat surfaces do. Rather than inventing a weaker
approval, it reuses the out-of-band one the gateway already has: the operator
writes the approved destinations into the config, by hand, ahead of time. This
mirrors the hardware rule in `CLAUDE.md` — with no confirmation channel, the
answer is fail-closed.

Consequences:

- `allow_send: true` with an **empty** allowlist sends nothing. The switch does
  not approve anything; the list does.
- A tool that cannot send is **not advertised** in `tools/list`. A model on the
  other side should not plan around a capability that does not exist and then
  read the refusal as a transient failure. Calling it by name anyway is still
  refused — there is no "hidden but callable".
- Only `telegram` is reachable, because `proactive_chat_ids` is the only
  destination allowlist that exists today. A channel with no allowlist has no way
  to say "the owner approved this destination".
- All sends across the bridge share one anti-amplification budget
  (`SendBudget`, 5 per minute). One budget rather than one per caller: a caller
  that could pick its own budget key would have no budget.

Refusals carry a stable `error.kind` so the caller can distinguish causes without
parsing prose: `send_disabled`, `no_allowlist`, `target_not_allowed`,
`channel_unsupported`, `rate_limited`, `channel_offline`, `delivery_failed`.
None of them echoes the requested `chat_id` — a refused destination is still a
real person's conversation, and the response leaves the machine. `delivery_failed`
also does not forward the channel's own error text: that string comes from an
external service in a format the bridge does not control, so the reason goes to
the gateway log and the caller gets the stable code.

### 4.3 `garra_ask` — inference on demand (#1612)

`garra_ask` calls the LLM of this Garra and returns the answer. It is the one
tool whose cost is not bounded by the data it touches: every call is a paid
request on the owner's provider key. It takes **three independent operator
actions**, and none of them is implied by another:

1. `gateway.mcp_http.allow_ask: true`. Off, the tool is not advertised and a
   direct call is refused with `ask_disabled`.
2. `gateway.mcp_http.ask_allowed_models`. **Empty (the default) means only the
   project default model** (`garraia_config::defaults::DEFAULT_CLOUD_MODEL`) is
   accepted, so a caller that omits `model` gets the cheap default and nothing
   else. A non-empty list **replaces** the default: the project model then passes
   only if it is listed. A caller asking for a model outside the list gets
   `model_not_allowed` before any request leaves the machine.
3. `gateway.mcp_http.ask_budget_per_minute` (default 10). One budget for the whole
   bridge, in a session of its own (`mcp-http-ask`). It never shares a counter
   with `garra_send_message`, so spending inference cannot use up the messages
   and the reverse is also true. Like the send budget it is read at boot, and a
   value below 1 is raised to 1.

The arguments follow the stdio contract: `message` is required (up to 64 KiB),
`provider` is one of `ollama`, `anthropic`, `openai`, `openrouter` (checked
against the list, not only the schema), `timeout_secs` is 1–600 (default 60), and
`system_prompt` is up to 8 KiB. Unknown fields are a protocol error. Config
`llm:` aliases are **not** reachable through the bridge in this version: the
provider must be one of the four kinds.

Refusals are `garra.ask.v1` envelopes with a stable `error.kind`: `ask_disabled`,
`model_not_allowed`, `over_budget`. Provider errors arrive as `provider_error`
and timeouts as `timeout`, with the secrets scrubbed by the same sanitizer the
stdio server uses. Neither the prompt nor the answer is written to the gateway
log. A success logs the provider, the model (already accepted by the allowlist)
and the latency; a failure logs only its `kind`.

## 5. Where the code lives

- `crates/garraia-gateway/src/mcp_http/politica.rs` — the decisions, pure
- `crates/garraia-gateway/src/mcp_http/ferramentas.rs` — the six descriptors
- `crates/garraia-ask/` — the one-shot LLM core shared with `garra ask` and the stdio server
- `crates/garraia-gateway/src/mcp_http/handler.rs` — the `rmcp::ServerHandler`
- `crates/garraia-gateway/src/mcp_http/mod.rs` — mounting and the transport config
- `crates/garraia-gateway/tests/mcp_http_contract.rs` — the contract over the
  real `build_router`

Cargo feature: `mcp-http-server`, on by default. Not to be confused with
`mcp-http`, which is the *client* side. The runtime switch is
`gateway.mcp_http.enabled`, not the feature: an endpoint that only exists behind
a non-default feature is an endpoint no installation has.
