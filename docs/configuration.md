# Configuration Reference

Complete reference for GarraIA configuration options.

## Configuration File Location

GarraIA looks for `config.yml` (fallback `config.toml`) in its config directory, resolved as:
1. `$GARRAIA_CONFIG_DIR` (when set)
2. `~/.config/garraia` (XDG; the default on new installs)
3. `~/.garraia` (legacy — only when the XDG directory does not exist)

Run `garraia config check` to see which directory and file are active.

## Full Configuration Example

```yaml
# Gateway settings
gateway:
  host: "127.0.0.1"  # Bind address
  port: 3888         # HTTP/WebSocket port

# LLM Providers
llm:
  # Anthropic Claude
  claude:
    provider: anthropic
    model: claude-sonnet-4-5-20250929
    api_key: "sk-ant-..."

  # OpenAI
  openai:
    provider: openai
    model: gpt-4o
    api_key: "sk-..."
    base_url: "https://api.openai.com/v1"  # Optional custom endpoint

  # Azure OpenAI
  azure:
    provider: openai
    model: gpt-4o
    api_key: "your-azure-key"
    base_url: "https://your-resource.openai.azure.com/"
    
  # Ollama (local)
  ollama:
    provider: ollama
    model: qwen3.8:latest
    base_url: "http://localhost:11434"

  # OpenRouter
  openrouter:
    provider: openrouter
    model: openai/gpt-4o
    api_key: "sk-or-..."

  # Other OpenAI-compatible providers
  deepseek:
    provider: openai
    model: deepseek-chat
    base_url: "https://api.deepseek.com/v1"
    api_key: "your-key"

  mistral:
    provider: openai
    model: mistral-large-latest
    base_url: "https://api.mistral.ai/v1"
    api_key: "your-key"

# Channel Configuration
# Tokens should be provided via environment variables (TELEGRAM_BOT_TOKEN,
# DISCORD_BOT_TOKEN, etc.) rather than stored in this file.
# See: docs/src/guides/connect-telegram.md for the full precedence chain.
channels:
  telegram:
    type: telegram
    enabled: true
    # bot_token resolved from: vault → config → TELEGRAM_BOT_TOKEN env var
    
  discord:
    type: discord
    enabled: false
    # bot_token resolved from: vault → config → DISCORD_BOT_TOKEN env var
    # application_id: "123456789"
    
  slack:
    enabled: false
    bot_token: "xoxb-..."
    app_token: "xapp-..."
    
  whatsapp:
    enabled: false
    phone_number_id: "123456789"
    access_token: "YOUR_ACCESS_TOKEN"
    verify_token: "YOUR_VERIFY_TOKEN"
    webhook_verify: true

# Agent Configuration
agent:
  system_prompt: |
    You are GarraIA, a helpful AI assistant.
    You are running locally and respect user privacy.
  max_tokens: 4096
  max_context_tokens: 100000
  temperature: 0.7
  tools:
    - bash
    - file_read
    - file_write
    - web_fetch
    - web_search
  # Backend of `web_search` (#1034). Omit the section to keep the old rule:
  # Brave when a key resolves, otherwise no search tool. `searxng` needs no
  # key — point it at a self-hosted instance with `json` in search.formats.
  web_search:
    backend: searxng                 # brave | searxng
    searxng_url: http://127.0.0.1:8081   # or GARRAIA_SEARXNG_URL

# Multi-agent Configuration
# Named agents are resolved via `agent_router` (explicit agent_id > channel
# setting > "default" agent). Each entry maps to `NamedAgentConfig`:
#   provider, model, system_prompt, max_tokens, max_context_tokens, tools
agents:
  assistant:
    provider: openai
    system_prompt: "You are a helpful general assistant."
    max_tokens: 4096

  coder:
    provider: openai
    system_prompt: "You are an expert programmer."
    tools: [bash, file_read, file_write]

  # Exemplo: agente nomeado "hera" com persona própria (docs/operacao/hera-persona.md).
  # A persona é o system_prompt; a memória usa o tenant padrão até existir
  # tenant por agente (issue #965 — conversa entre agentes via A2A).
  hera:
    provider: ollama-local
    system_prompt: |
      Você é a Hera: a assistente pessoal desta instalação do Garra.
      Português do Brasil. Frases curtas. Responda primeiro, explique depois.
      Você não tem internet, calendário, email nem ferramentas.
      Não invente fato pessoal. "Não sei" é resposta completa.

# Memory Configuration
memory:
  enabled: true
  embedding_provider: ollama-embed   # key of the `embeddings` entry below
  # shared_continuity: false         # optional; fact extraction runs on every turn (no auto_extract knob)

# Embeddings Configuration (named map — pick one entry via memory.embedding_provider)
embeddings:
  ollama-embed:
    provider: ollama
    model: nomic-embed-text
    base_url: "http://localhost:11434"
    dimensions: 768

# Voice Configuration
voice:
  enabled: false
  tts_endpoint: "http://127.0.0.1:7860"
  stt_provider: whisper
  language: "pt"
  
# MCP Servers
mcp:
  filesystem:
    command: npx
    args: ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
    
  github:
    command: npx
    args: ["-y", "@modelcontextprotocol/server-github"]
    env:
      GITHUB_TOKEN: "your-token"

# Timeouts (in seconds)
timeouts:
  llm:
    default_secs: 30
  tts:
    default_secs: 120
  mcp:
    default_secs: 60
  health:
    default_secs: 5

# Execution profile (ADR 0024, #1329) — see docs/execution-profiles.md.
# `standard` (default, section absent) is today's posture: ToolGate floor by
# mode, file-tool jail, `search` floor on personal WhatsApp. `isolated-pod`
# declares that THIS process runs in a disposable pod: full power inside the
# pod for the WhatsApp owner in 1:1 chats, MCP filesystem rooted at pod_root.
# GARRAIA_EXECUTION_PROFILE overrides the file; an invalid value refuses boot.
execution:
  profile: standard        # standard | isolated-pod
  # pod_root: /workspace   # optional, absolute; only meaningful in isolated-pod

# Security
security:
  vault_password: "your-vault-password"  # Or set GARRAIA_VAULT_PASSWORD env
  pairing_code_length: 8
  rate_limit:
    enabled: true
    requests_per_minute: 60

# Logging
logging:
  level: "info"  # debug, info, warn, error
  format: "json"  # text, json
```

## Environment Variables

You can use environment variables for sensitive values:

```yaml
llm:
  openai:
    provider: openai
    model: gpt-4o
    # Resolved in order: config > env var
    api_key: "${OPENAI_API_KEY}"
```

Supported env var resolution:
- `${VAR_NAME}` - Uses environment variable
- Leave empty - Uses `GARRAIA_{PROVIDER}_API_KEY`

Runtime overrides read directly by the loader (not secrets):

| Variable | Overrides | Notes |
|---|---|---|
| `GARRAIA_CONFIG_DIR` | config directory | See "Configuration File Location" above. |
| `GARRAIA_EXECUTION_PROFILE` | `execution.profile` | `standard` \| `isolated-pod`. **Wins over the file**, resolved once at load; `config check` and `/api/diagnostics` report the source (`default` \| `file` \| `env`). Any other value is a load error: the gateway refuses to boot and `config check` reports `Error` (exit 2). Never persisted back to the file by a save. [`execution-profiles.md`](execution-profiles.md). |
| `GARRAIA_FILE_ROOTS` | adds to `agent.file_roots` | PATH-style list; extra roots for the native file-tool jail (#1244). When neither this nor `agent.file_roots` resolves to a root, the jail falls back to the profile's default workspace (`<data_dir>/workspace`, #1378) — never `/` and never `$HOME`. |
| `GARRAIA_ALLOW_INVALID_CONFIG` | the boot gate (#1247) | Exactly `1` lets `garraia start`/`restart` boot despite a blocking config `Error` (exit 78 otherwise); any other value (`true`, `0`, ` 1`, empty) counts as unset. The blocking findings are still logged at error level, naming this variable. |
| `GARRAIA_GATEWAY_API_KEY` | `gateway.api_key` | **Secret.** A non-blank value wins over the file (#1261); blank counts as unset. Applied at load into a field that is never serialized, so a save never writes it to disk. `config check` reports presence only, and warns when it differs from the file key. Required (one or the other) for a non-loopback bind: without a credential `garraia start` refuses to boot (exit 78). |
| `HOST` / `PORT` | the listener bind | Read by `garraia start` **and** `garraia restart` (flag > env > `127.0.0.1:3888`). `gateway.host` / `gateway.port` in the file are deprecated and never read. |

## Provider / model resolution precedence

> GAR-576 — clarifies how `garra chat` picks a provider and a model when
> multiple signals compete (CLI flags, config blocks, environment
> variables, `.env`).

### Provider resolution

When `garra chat` runs, the provider is chosen in this strict order:

1. **`--provider <X>` CLI flag** — absolute precedence. `<X>` is a kind
   (`ollama`, `llamacpp`, `anthropic`, `openai`, `openrouter`) or the
   name of any `llm:` entry (an alias such as `lmstudio` with
   `provider: openai`). See "Endpoint and credential" below for which
   entry supplies the URL and the key.
2. **`config.agent.default_provider`** — read as a *lookup key* into
   `config.llm[...]`. If the matching block has a usable credential
   (api_key in config; the matching `*_API_KEY` env var, but only when
   the block talks to the kind's default host — see "Endpoint and
   credential" below; or, for OpenAI-compatible local backends, a
   `base_url` of their own), this provider wins.
3. **Autodetect** (no `--provider`, no usable `default_provider`) —
   cloud first, since issue #1180: the cloud providers you hold a
   credential for (`api_key` in config or the matching `*_API_KEY` env
   var, under the same default-host rule) are tried in the order
   **Anthropic → OpenAI → OpenRouter**, a provider with no credential
   being skipped; then **Ollama**, if its health check passes; and, as
   the last resort, an **offline Ollama** handle so the REPL still opens
   and tells you what is missing.

Before #1180 this chain probed Ollama *first*, so a stray `ollama serve`
silently beat an exported `OPENROUTER_API_KEY`. If you want the local
daemon to win over a cloud credential, say so explicitly — that is the
escape hatch, not autodetect:

```yaml
agent:
  default_provider: ollama
```

### Model resolution (per provider kind)

Inside the chosen provider, the model is resolved in this order:

1. **`--model <X>` CLI flag** — absolute precedence.
2. **The `model` of the entry the provider was bound to** (`llm.<X>` for
   `--provider <X>`, `llm.<default_provider>` for the default). With
   `default_provider: lmstudio` next to an `llm.openai`, the LM Studio
   endpoint gets `llm.lmstudio.model`, never `llm.openai.model`.
3. **`config.llm[<key>].model`** with `<key>` matching the provider
   kind (key-match).
4. **First `config.llm[*]` whose `provider:` field equals the chosen
   kind** and supplies a non-empty `model` (provider-field match — lets
   operators give blocks arbitrary names like `my-router`).
5. **Hardcoded last-resort default** per kind: `qwen3.8:latest`,
   `claude-sonnet-4-5-20250929`, `gpt-4o`, `z-ai/glm-5.3-flash`. The
   source of truth for the two project defaults (`DEFAULT_CLOUD_MODEL`,
   `DEFAULT_LOCAL_MODEL` and their provider keys) is
   `crates/garraia-config/src/defaults.rs` (issue #1180) — the CLI *and*
   the gateway read that file, so `garra chat`, the wizard, `garra
   mcp-server`, the gateway boot path and `POST /api/providers` cannot
   disagree. `crates/garraia-cli/src/defaults.rs` only re-exports those
   constants and holds the test that locks the Desktop's
   `config.default.yml` to the same values. The per-kind table above is
   `chat.rs::hardcoded_default_model`, which reads the constants for its
   two #1180 rows.

### Endpoint and credential (always the same entry)

A provider's `base_url` and its `api_key` always come from the **same**
`llm:` entry. The CLI never pairs the key of one entry with the address
of another, nor the key of an entry with the provider's default host when
that entry points somewhere else (`crates/garraia-cli/src/provider_binding.rs`).

| How the provider was chosen | Entry that supplies URL + key |
| --------------------------- | ----------------------------- |
| `--provider <X>` / MCP `provider: <X>` | `llm.<X>`; if absent and `<X>` is a kind, `llm.main` when its `provider:` is `<X>`; otherwise none |
| `agent.default_provider: <K>` | `llm.<K>` |
| autodetect (Anthropic, OpenAI, OpenRouter) | same rule as `--provider <kind>`; an `llm.<kind>` that declares **another** `provider:` is used whole only when it carries its own `api_key`, otherwise the candidate falls back to "no entry" |

Inside that entry the entry's own `api_key` wins over the kind's
environment variable (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`,
`OPENROUTER_API_KEY`). The variable **only ever reaches the kind's
default host**: it fills an entry that has no key only when that entry
has no `base_url`, or a `base_url` that *is* the default host
(`https://api.openai.com[/v1]`, `https://api.anthropic.com`,
`https://openrouter.ai/api/v1` — the last one is what `garraia init`
writes, with the key left in the environment). An entry pointing
anywhere else without its own `api_key` gets **no** credential: an
OpenAI-compatible one sends the keyless placeholder `not-needed` (what
LM Studio / vLLM expect), `anthropic` and `openrouter` refuse with an
error. So an `OPENAI_API_KEY` sitting in a `.env` of the current
directory (loaded by `dotenvy`) is never shipped to a proxy or to a
third-party LM Studio just because its entry has no `api_key`. This is
stricter than the gateway, which resolves config > env without looking
at the `base_url`.

When **no** entry applies, the endpoint is the kind's default host and
the key can only come from that environment variable — the key of some
other entry of the same kind is never borrowed.

The MCP `garra_agent` looks the built provider up by the id it
registered under (`llama-cpp` for `llamacpp`, the kind for an alias of
`anthropic`/`ollama`/`llamacpp`), so every alias accepted by `-p` also
works there; the envelope still reports the name that was passed.

`--url <address>` (an ad-hoc OpenAI-compatible endpoint) takes its key
from `LLM_API_KEY`, or from the `api_key` of an `llm:` entry whose
`base_url` is that same address; otherwise it sends none. It does **not**
read `OPENAI_API_KEY` or `GARRAIA_EMBEDDING_API_KEY` (it did up to
v0.4.4): those are credentials of other endpoints. `--url` is honoured
together with `--provider` only for the keyless `llamacpp`.

### OpenRouter cost policy

`z-ai/glm-5.3-flash` is the project's official default model (issue
#1180): a flash-tier model cheap enough to be the unattended default
while being good enough for real work. Everything else is an explicit
choice:

| Model               | When to use                                                                                   |
| ------------------- | --------------------------------------------------------------------------------------------- |
| `z-ai/glm-5.3-flash`| **The default.** Every surface (`garra`, `garra ask`, `garra mcp-server`, the wizard, Desktop) resolves to it when nothing is chosen. |
| `openrouter/auto`   | Real tasks / complex reasoning, at a price. Only via an explicit `--model openrouter/auto`.    |
| `openrouter/free`   | Zero-cost smoke tests. No longer a default anywhere — pass `--model openrouter/free` to get it.|

Recommended baseline `config.yml`:

```yaml
llm:
  openrouter:
    provider: openrouter
    model: z-ai/glm-5.3-flash       # the official default
    base_url: "https://openrouter.ai/api/v1"
  ollama:
    provider: ollama
    model: qwen3.8:latest           # second option / fallback
    base_url: "http://localhost:11434"

agent:
  default_provider: openrouter      # honored by `garra chat` autodetect
  fallback_providers: ["ollama"]    # local is always the SECOND option
```

To run a heavier task, pass the model explicitly:

```bash
garra chat --provider openrouter --model openrouter/auto
```

There is no automatic upgrade to a pricier model — paid traffic above
the default always requires an explicit `--model` flag.

> GAR-579 — esta mesma precedência é consumida por
> [`garra ask`](cli-ask.md), o comando não-interativo. Tanto `garra chat`
> quanto `garra ask` compartilham `chat::select_explicit_provider` /
> `chat::detect_provider`, então o comportamento é byte-equivalente.

## Hot Reload

Configuration changes in `config.yml` are applied automatically:
1. Edit `<config-dir>/config.yml` (`~/.config/garraia/config.yml` by default)
2. Changes detected within seconds
3. No restart required

## CLI Configuration

### Validate config

```bash
garraia config check
```

Options:
- `--json` — machine-readable JSON output
- `--strict` — treat warnings as errors (useful for CI)

### The same check runs at boot (#1247)

Every `garraia start`, `garraia restart` and `garraia start -d` runs the same
check once, on the loaded config, before anything else happens (fork, PID
file, stopping the running daemon, bind):

- each `Error` is logged once at error level and each `Warning` once at warn
  level, followed by a summary line pointing to `garraia config check`. In
  `start -d` the same lines go to the terminal's stderr before the fork,
  because afterwards the log file is the only output;
- the boot is **refused** (exit 78, `EX_CONFIG`) only for an `Error` on a
  short, closed list of fields that would fail open with nothing downstream
  to catch them. In v0.4.5 that list is the half-configured TLS pair
  (`gateway.tls_cert_path` / `gateway.tls_key_path`): with only one of them
  set the gateway used to serve plain HTTP in silence;
- every other `Error` (for example an `llm` entry without a resolvable key)
  is reported but does **not** block the boot, so configs that work today
  keep working after an update;
- the `gateway.host` / `gateway.port` findings are not repeated at boot: the
  bind is judged on the real address by the #1261 refusal
  ([auth-config.md §5.1](auth-config.md#51-the-gateway-bind-address--what-config-check-sees-vs-what-start-binds)).

An invalid `execution.profile` is still a load error and never reaches this
check. Out-of-range `memory.retention.interval_hours` / `max_age_days` do not
block the boot either: the retention worker does not start (and deletes
nothing), with an error in the log.

## Advanced Options

### Custom Channels

```yaml
channels:
  custom:
    type: http
    endpoint: "http://localhost:8080/webhook"
    auth_header: "X-Custom-Auth"
```

### Scheduling

```yaml
scheduler:
  enabled: true
  timezone: "America/Sao_Paulo"
```

### Observability

```yaml
observability:
  tracing:
    enabled: true
    endpoint: "http://localhost:4318/v1/traces"
  metrics:
    enabled: true
    port: 9090
```

### Tool sandbox (`agent.sandbox`)

Runs the agent's process-spawning tools inside a throwaway container instead
of on the host. Default `mode: off` changes nothing.

```yaml
agent:
  sandbox:
    mode: all            # off (default) | all | allowlist
    backend: docker      # docker | podman | ssh (ssh is remote execution, not isolation)
    image: debian:bookworm-slim   # must contain the programs the tools call
    elevated: []         # tools that stay on the host even with mode: all
    sandboxed_tools: []  # used by mode: allowlist
    mount_workdir: true  # mount only the (canonical) working dir, read-write
    network_disabled: true
```

Covered tools: `bash` (shell line) and, since #1225 S2, `run_tests`,
`git_diff`, `code_review` and `repo_search` (argv, no shell). Every container
runs with `--cap-drop ALL`, `--pids-limit 512`, `--security-opt
no-new-privileges` and your own uid (`--user <uid>:<gid>`, or
`--userns=keep-id` on podman). Nothing falls back to the host: a missing
backend, a stopped daemon, a relative or missing working dir, or `ssh` for a
working-dir tool is a refused call.

**Migration for `mode: all`.** Those four tools now run in the container, so
the image needs their programs (`git`, `rg` or `grep`, `cargo`/`npm`/`python`).
The default `debian:bookworm-slim` has only `grep`: `run_tests` and `git_diff`
answer "the program does not exist in the sandbox image". Either point
`image` at one with your toolchain, or list the tool in `elevated` to keep it
on the host. With `network_disabled: true`, `cargo` cannot download crates.

In `execution.profile: standard`, `garraia mcp-server` and the gateway only
register `bash` when this section puts it in a working docker/podman
container (#1272); see [`execution-profiles.md`](execution-profiles.md).

A remote container is `backend: docker` with a `docker context` pointing at
`ssh://host` — note that the mount then refers to paths on the REMOTE host.
There is no `ssh` + container backend (#1225 S5, won't-do: see
[`security/threat-model.md`](security/threat-model.md) §5.13).

## Runs ledger (`runs`)

The gateway records every scheduled run (`mode: heartbeat`) in the
`agent_runs` table of `sessions.db`. `garraia runs list` reads it from the
terminal, and `GET /api/runs` reads it over HTTP (#1227).

### Retention

```yaml
runs:
  retention_days: 0   # default: never delete
```

- `0` (the default) keeps every row. An upgrade never deletes history. While
  retention is off the gateway logs once at boot how many runs the ledger
  holds and how to turn retention on.
- `1..=3650` deletes **terminal** runs (`done`, `error`, `cancelled`,
  `interrupted`) whose end time (or start time, when no end was recorded) is
  older than that many days. The sweep runs at boot and then every 24 hours.
  Its log carries only the count, never run content.
- A `running` row is **never** deleted, whatever its age. A run left
  `running` by a crash becomes `interrupted` at the next boot, and only then
  ages like any other terminal run.
- `garraia config check` rejects values above 3650.

The key is top-level on purpose: `agents:` is a map of named agents, so
`agents.runs_retention_days` would be read as an agent called
`runs_retention_days`.

### `GET /api/runs`

Read-only. Query: `status` (`running`, `done`, `error`, `cancelled`,
`interrupted`; anything else is `400`) and `limit` (default 20, clamped to
`[1, 200]`). The response is `{"runs": [...]}` with `id`, `session_id`,
`mode`, `status`, `started_at`, `finished_at` (UTC ISO 8601 with `Z`) and
`goal_preview` / `result_preview` / `error_preview`: at most 120 characters,
with known secret formats redacted and control characters replaced. The full
500-character snippets stored in the ledger are not exposed over HTTP.

Access is stricter than the rest of `/api/*`:

- With `gateway.api_key` set, a valid `Authorization: Bearer` is required.
  This is how a phone or another machine on the LAN reads the ledger.
- Without `gateway.api_key`, only the local machine can read it: the peer
  must be loopback **and** the `Host` header must be a loopback address
  (`127.0.0.1`, `[::1]`, any `127.x.y.z`) or `localhost`. A LAN peer gets
  `503 runs: auth not configured`; a loopback peer behind another `Host` name
  (DNS rebinding) gets `403`.
- Behind a reverse proxy, set `gateway.api_key`. Without it, a request that
  carries `X-Forwarded-For`, `X-Real-IP`, `Forwarded` or `X-Forwarded-Host`
  gets `503 runs: auth not configured` even from a loopback peer: the peer is
  the proxy, not the owner's own client, and a proxy that rewrites `Host` to
  its upstream would otherwise pass the loopback checks.

### Why there is no `runs resume`

A scheduled run marked `interrupted` is already retried automatically: at
boot the scheduler puts a task whose lease expired back to `pending`
(`recover_expired_leases`), and the next tick executes it again, which
records a **new** run in the ledger. A manual resume of the old row would
execute the task twice (duplicate system message and duplicate channel
delivery). The ledger also cannot replay a run faithfully: `goal` is
truncated to 500 characters and no column links a run back to its task.
Resuming is left out until a real consumer of the sub-agent coordinator
exists; when it does, it must require explicit confirmation and create a new
run that references the original.
