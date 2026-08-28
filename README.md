<!-- markdownlint-disable MD033 MD041 MD060 -->

<p align="right"><strong>🇺🇸 English</strong> · <a href="README.pt-BR.md">🇧🇷 Português</a></p>

<p align="center">
  <img src="assets/logo.png" alt="GarraIA" width="280" />
</p>

<h1 align="center">GarraIA</h1>

<p align="center">
  <strong>The secure, lightweight open-source framework for AI agents — written in Rust, born in Brazil.</strong>
</p>

<p align="center">
  <a href="https://github.com/michelbr84/GarraRUST/actions"><img src="https://github.com/michelbr84/GarraRUST/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI"></a>
  <a href="https://github.com/michelbr84/GarraRUST/actions/workflows/codeql.yml"><img src="https://github.com/michelbr84/GarraRUST/actions/workflows/codeql.yml/badge.svg?branch=main" alt="CodeQL"></a>
  <a href="https://github.com/michelbr84/GarraRUST/actions/workflows/cargo-audit.yml"><img src="https://github.com/michelbr84/GarraRUST/actions/workflows/cargo-audit.yml/badge.svg?branch=main" alt="Security Audit"></a>
  <a href="https://github.com/michelbr84/GarraRUST/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT"></a>
  <a href="https://github.com/michelbr84/GarraRUST/stargazers"><img src="https://img.shields.io/github/stars/michelbr84/GarraRUST" alt="Stars"></a>
  <a href="https://github.com/michelbr84/GarraRUST/issues?q=label%3Agood-first-issue+is%3Aopen"><img src="https://img.shields.io/github/issues/michelbr84/GarraRUST/good-first-issue?color=7057ff&label=good%20first%20issues" alt="Good First Issues"></a>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/rust-1.94%2B-orange?logo=rust" alt="Rust">
  <img src="https://img.shields.io/badge/crates-22-green" alt="Crates">
  <img src="https://img.shields.io/badge/channels-5%20wired-purple" alt="Channels">
  <img src="https://img.shields.io/badge/LLM%20providers-15-red" alt="Providers">
</p>

<p align="center">
  <a href="#quick-start">Quick Start</a> &middot;
  <a href="#why-garraia">Why GarraIA?</a> &middot;
  <a href="#features">Features</a> &middot;
  <a href="#memory-and-self-learning">Memory</a> &middot;
  <a href="#security">Security</a> &middot;
  <a href="#architecture">Architecture</a> &middot;
  <a href="#migrating-from-openclaw">Migrate from OpenClaw</a> &middot;
  <a href="#contributing">Contributing</a>
</p>

---

**A personal AI assistant that runs on your machine.** One self-contained
native binary runs your agents on Telegram, Discord, Slack, WhatsApp and
iMessage — with an AES-256-GCM encrypted credential vault, hot config
reload, a full memory system with local embeddings, and a warm Brazilian
Portuguese persona as the default voice (fully configurable, including a
neutral English mode).

Every performance and comparison number in this README is **measured, not
asserted**: the reproducible harness, the scenario definitions, and the
raw evidence live in
[`benches/agent-framework-comparison/`](benches/agent-framework-comparison/).
If a number has no committed measurement behind it, it does not appear here.

**Local-first.** All state — conversations, memory, config, credentials —
is stored on your machine, and there is no telemetry or analytics
phone-home. Your prompts go only to the LLM provider you configure; run
[Ollama](https://ollama.com) for a fully offline, zero-egress setup.

## 🐾 Meet Garra

Garra is not just an API endpoint — it is a personal assistant with a
personality. From the first `garra start` it introduces itself by name and
speaks warm, direct, first-person Brazilian Portuguese (no sycophancy).
When something breaks, it explains what happened and what to do next
instead of dumping raw error codes.

> _"Oi! 👋 Eu sou o Garra, seu assistente pessoal. Pode falar comigo como
> você falaria com um amigo."_

The persona is **the default, not a cage**: set `agent.system_prompt` for
a custom personality or `agent.persona = "neutral"` for a fully neutral
tone in any language. See [ADR 0012](docs/adr/0012-garra-persona.md).

## Quick Start

```bash
# Requires Rust 1.94+ (matches the MSRV declared in Cargo.toml)
cargo build --release -p garraia

# Interactive setup — pick your LLM provider; optionally store API keys
# in the encrypted vault (the wizard's default is config.yml, mode 0600)
./target/release/garra init

# Start
./target/release/garra start

# One-shot non-interactive ask — great for scripts and CI
./target/release/garra ask --provider openrouter --model openrouter/free \
  --json --timeout-secs 30 "Reply with exactly: OK"

# MCP server over stdio — exposes `garra_ask` to Claude Desktop / Claude Code
./target/release/garra mcp-server
```

<details>
<summary>Install via script (Linux, macOS) — uses published release binaries</summary>

```bash
curl -fsSL https://garraia.org/install.sh | sh
```

Mirrors (same script, auto-synced): GitHub release CDN
`https://github.com/michelbr84/GarraRUST/releases/latest/download/install.sh`
(most robust against IP rate limits) and
`https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.sh`.

The installer downloads the binary for your platform, verifies it against
the release's `SHA256SUMS`, then chains into init and start. Note: the
installer names the binary `garraia`, while a cargo build produces
`garra` — the commands are otherwise identical. Env toggles:
`GARRAIA_SKIP_INIT=1`, `GARRAIA_SKIP_START=1`,
`GARRAIA_BOOTSTRAP_LOCAL=0`. In TTY-less contexts (Docker build, CI) it
prints next steps and exits 0 instead of blocking.

Releases ship 5 prebuilt CLI binaries: Linux x86_64/aarch64,
macOS x86_64/aarch64, Windows x86_64.

</details>

<details>
<summary>Update an existing install — <code>garra update</code></summary>

```bash
garra update          # interactive
garra update --yes    # non-interactive (CI)
garra rollback        # restore the previous binary if anything goes wrong
```

Self-update downloads the platform binary, verifies it against the
release's per-asset SHA-256 checksum (aborts on mismatch or missing
checksum), and swaps the executable atomically.

</details>

<details>
<summary>Build the desktop app (Tauri)</summary>

```bash
cargo build --release -p garraia
cp target/release/garra crates/garraia-desktop/src-tauri/binaries/garra-$(rustc -vV | grep host | cut -d' ' -f2)
cargo build --release -p garraia-desktop
```

</details>

## Why GarraIA?

### Measured, reproducible numbers

Measured with the versioned harness in
[`benches/agent-framework-comparison/`](benches/agent-framework-comparison/)
(scenarios [001](benches/agent-framework-comparison/scenarios/001-binary-size.md),
[002](benches/agent-framework-comparison/scenarios/002-peak-rss.md),
[003](benches/agent-framework-comparison/scenarios/003-cold-start.md);
raw logs committed under
[`results/`](benches/agent-framework-comparison/results/)).
Performance was measured on `openclaw@2026.7.1-2` (npm `latest` at run
time) and a fresh ZeroClaw `master` clone (`d355e3b`); hardware and
versions recorded in `environment.txt`. The security-audit scenarios
below use separate commit-pinned inspection checkouts.

| Metric | **GarraIA** | **OpenClaw** (Node.js) | **ZeroClaw** (Rust) |
|---|---|---|---|
| Installed footprint | 47 MiB single binary (LTO, stripped) | 370 MiB `node_modules` + Node.js ≥ 22.22.3 runtime | 40 MiB binary (default lean-bundle build) |
| Peak RSS, `--help` | **8.6 MiB** (8,756 KiB) | 49.2 MiB (50,388 KiB) | 15.3 MiB (15,704 KiB) |
| Cold start, `--help` (mean of 20) | **4.1 ms** | 46.2 ms | 8.5 ms |

Yes, ZeroClaw's default binary is 7 MiB smaller than ours — the table
reports what the harness measured, including where we lose.

> These are CLI-floor measurements, not idle-server memory — the harness
> says exactly what it measures and what it does not. Numbers above are
> from `results/2026-08-28-vm/` (x86_64 Linux container); the dedicated
> 1 vCPU / 1 GB droplet run is the reference target and lands in a
> follow-up results directory.

### Audited security posture — all three frameworks, pinned commits

Verified by mechanical inspection of pinned checkouts (OpenClaw
`343252a`, ZeroClaw `d5617f1`) — every row has a
reproducible check in scenarios
[004](benches/agent-framework-comparison/scenarios/004-credentials-at-rest.md)
and [005](benches/agent-framework-comparison/scenarios/005-attack-surface.md),
with per-claim evidence in `results/`:

| | **GarraIA** | **OpenClaw** | **ZeroClaw** |
|---|---|---|---|
| Credentials at rest | AES-256-GCM vault (PBKDF2, 600k iters) available; **opt-in** — the init wizard's default is `config.yml` at mode 0600. MCP secrets auto-move to the vault when a passphrase is set | Not encrypted at rest (their docs' own words) — POSIX 0600/0700 perms; SecretRefs + 1Password/Vault are opt-in | ChaCha20-Poly1305 **by default** — the only default-encrypted posture of the three. Caveats: master key sits on the same filesystem; 1Password refs opt-in |
| Default bind | 127.0.0.1 | loopback | 127.0.0.1 |
| Gateway auth default | Messaging channels are deny-by-default (pairing codes). Local API is open on loopback; token/session auth is opt-in | Token required out of the box; **fails closed** without one | Pairing required by default; public bind is warn-only |
| Dependency tree | 1,061 crates (Cargo.lock) | 66 direct prod npm deps; `npm install -g` resolved 300 packages on the measured run | 1,265 crates (Cargo.lock) |
| Plugin isolation | WASM sandbox (wasmtime): memory caps + execution deadlines, opt-in feature | In-process, plugins are trusted code (their threat model says so) | WASM component model; Ed25519 signing exists but defaults to disabled |

### Feature comparison

| | **GarraIA** | **OpenClaw** | **ZeroClaw** |
|---|---|---|---|
| Chat channels | 5 wired end-to-end (Telegram, Discord, Slack, WhatsApp, iMessage·macOS) + web chat + OpenAI-compatible API; 6 more implemented in-crate, not yet wired | 27 bundled channel plugins | ~40 adapters (default build bundles 6) |
| LLM providers | 15 built-in (Anthropic, OpenAI, Ollama native + 12 OpenAI-compatible presets); 100+ models via OpenRouter; any endpoint via `base_url` | plugin providers | multiple, feature-gated |
| MCP | client: stdio (default build) + Streamable HTTP (`mcp-http` feature) | client: stdio/SSE/Streamable HTTP; also serves MCP | client: stdio/http/sse, per-agent fail-closed scoping |
| Memory | SQLite + local vector search (sqlite-vec) + LLM fact extraction, auto-injected into context | Markdown files + SQLite FTS5/vector | sqlite/postgres/qdrant backends |
| Config hot reload | file watch — most settings apply live (channels/providers wire at boot) | file watch (hybrid mode) | explicit reload endpoint only |
| Scheduling | one-shot scheduled tasks (persisted heartbeats, up to 30 days); cron-style recurrence is on the roadmap | full cron + automations | cron + SOP engine |
| Multi-tenant group workspace | in active development — Postgres 16 + pgvector, 37 tables across 32 migrations with FORCE Row-Level Security on tenant data ([Phase 3](ROADMAP.md)) | explicit non-goal (single trusted operator) | no |
| Native PT-BR assistant persona | yes — first-class, default | no | no |
| Prebuilt binaries + self-update | 5 targets, SHA-256-verified atomic self-update | npm package (needs Node runtime) | 10 targets, SLSA provenance |

**Where the others win, honestly:** OpenClaw has the broadest channel and
plugin ecosystem (27 channels, 150+ extensions) plus a mature cron system,
and its gateway requires auth out of the box — GarraIA's local API auth is
still opt-in. ZeroClaw encrypts secrets by default, ships 10 prebuilt
targets with SLSA provenance, and has OS-level exec sandboxing. We compare
against their strengths on purpose: if this table ever reads like
marketing, [open an issue](https://github.com/michelbr84/GarraRUST/issues)
with the scenario ID and we will fix the table, not the finding.

## Features

### LLM providers

3 native providers — **Anthropic Claude** (SSE streaming, tool use),
**OpenAI** (GPT-4o, Azure, any compatible endpoint via `base_url`),
**Ollama** (local models + local embeddings) — plus 12 OpenAI-compatible
presets: OpenRouter (100+ models), DeepSeek, Mistral, Gemini, Qwen, Yi,
Cohere, MiniMax, Moonshot, Falcon, Jais, Sansa. Automatic provider
fallback on 429/5xx with exponential backoff and a circuit breaker.

### Channels

Wired end-to-end today: **Telegram** (streaming, MarkdownV2, bot
commands, pairing), **Discord** (slash commands, sessions), **Slack**
(Socket Mode), **WhatsApp** (Meta Cloud API webhooks), **iMessage**
(macOS, chat.db polling + AppleScript). Also: web chat console and an
**OpenAI-compatible API** (`/v1/chat/completions`) for VS Code
(Continue et al.) sharing the same session history. Six more channel
implementations (Google Chat, Teams, Matrix, LINE, IRC, Signal) exist in
`garraia-channels` and await gateway wiring — tracked on the
[roadmap](ROADMAP.md).

### Agent runtime

Tool-execution loop (bash, file read/write, web fetch/search, repo
search, git diff, scheduling) with per-task tool-call budget; sliding
context window plus automatic background summarization for long sessions;
**execution modes** (ask / code / debug / architect / review / orchestrator
/ custom) with per-mode tool policies, selected via `/mode`, header, or
deterministic auto-routing.

### Voice

STT via Whisper (local whisper.cpp or OpenAI API); TTS via Chatterbox
(GPU, multilingual), Hibiki, ElevenLabs, Kokoro, or OpenAI TTS;
`garra start --with-voice`; automatic audio replies on Telegram; format
conversion via ffmpeg.

### MCP (Model Context Protocol)

Connect any MCP server: stdio for local processes (default build), plus
Streamable HTTP for remote ones behind the `mcp-http` feature (config
accepts `http`/`sse`/`streamable-http` values — all served by the
Streamable HTTP client; legacy SSE-only servers are not supported, and
the prebuilt binaries currently ship stdio-only). Tools appear
namespaced (`server.tool`); MCP prompts become slash commands
automatically; marketplace with one-click install via the web console
(`/api/mcp/marketplace`); admin API adds/removes servers without
restart; CLI: `garra mcp list|inspect|resources|prompts`. Config in
`config.yml` or `~/.garraia/mcp.json` (Claude Desktop-compatible).

### Skills & plugins

Markdown skills (`SKILL.md` + YAML frontmatter) auto-discovered from
`~/.garraia/skills/`, with a visual editor and full CRUD API. Optional
WASM plugin sandbox (wasmtime, `--features plugins`) with per-plugin
memory caps and execution deadlines.

### Web console "Garra Glass"

A no-build-step SPA served at `GET /` — dashboard, chat, providers,
channels, sessions, settings, diagnostics, logs. Security invariant: no
secret is ever returned by any `/api/*` endpoint (write-only settings
report `configured: true|false`). Design system in
[ADR 0009](docs/adr/0009-web-console-design-system.md).

### Infrastructure

Hot config reload (watched `config.yml`), daemonization with PID
management, `garra restart`, runtime provider switching, structured logs
(`request_id`, `session_id`, JSON via `GARRAIA_LOG_FORMAT=json`),
configurable timeouts per subsystem, per-IP rate limiting, health checks
(`GET /api/health` + boot table + background probes), AI-transparency
headers (`X-AI-Model`, `X-AI-Provider`) on every response.

## Memory and self-learning

```text
~/.garraia/
├── memoria/fatos.json      # LLM-extracted facts
├── data/memory.db          # SQLite + vector search (sqlite-vec)
├── data/sessions.db        # persistent conversation sessions
└── credentials/vault.json  # AES-256-GCM encrypted credentials
```

After conversations, a dedicated LLM extractor identifies durable facts
and stores them with context and date; local embeddings (Ollama —
nomic-embed-text, mxbai-embed-large, …) power semantic search; relevant
facts are injected into the agent's context automatically. Memory is
managed through the web console and the gateway API (a `garra memory`
CLI is on the roadmap).

```yaml
memory:
  enabled: true
  auto_extract: true
embeddings:
  provider: ollama
  model: nomic-embed-text
  base_url: "http://localhost:11434"
```

## Security

Built for the requirements of always-on agents that touch private data.
Wording below matches what the code does — audited claim by claim (see
the comparison section above for the evidence trail).

- **Encrypted credential vault (opt-in)** — AES-256-GCM at
  `~/.garraia/credentials/vault.json`, key derived via
  PBKDF2-HMAC-SHA256 (600k iterations) from `GARRAIA_VAULT_PASSPHRASE`.
  Stated plainly: the `garra init` wizard's recommended default stores
  provider keys in `config.yml` (mode 0600, plaintext); choose the vault
  option and export the passphrase on every start to get encryption at
  rest. Making the vault the default is a roadmap item.
- **MCP secrets vault-protected** — sensitive env vars of MCP servers are
  auto-moved to the vault on save; `mcp.json` keeps only
  `vault:mcp.<server>.<key>` references. No passphrase → plaintext with a
  loud warning, never a broken boot.
- **Channels are deny-by-default** — per-channel allowlists; unknown
  users must present a pairing code; unauthorized messages are dropped.
- **Local API binds 127.0.0.1 by default** — enable `gateway.api_key`
  and/or `session_tokens_required: true` to require auth on it
  (opt-in today; hardening this default is tracked on the roadmap).
  The `HOST` env var can override the bind — mind your container config.
- **256-bit session tokens** — HttpOnly, SameSite=Strict cookie (or
  Bearer/`X-Session-Key`), rotated on resume, TTL + idle timeout.
- **Two auth stacks, stated plainly** — Auth v1 (`/v1/auth/*`): 15-minute
  HS256 access tokens + opaque HMAC-signed refresh tokens, Argon2id
  hashing with PBKDF2 lazy upgrade. Legacy mobile endpoints (`/auth/*`):
  30-day JWTs, PBKDF2 — being migrated to v1.
- **Risky-command confirmation** — opt-in `tool_confirmation_enabled`
  pauses before destructive bash (`rm -r`, `git reset --hard`, …).
- **MCP process resource limits** — optional per-server virtual-memory
  cap (setrlimit, Unix), startup timeout, auto-restart with exponential
  backoff. These are resource limits, not a sandbox: MCP processes keep
  filesystem/network access.
- **WASM plugin sandbox** — optional (`--features plugins`): per-plugin
  memory caps and execution deadlines via wasmtime.
- **Heuristic input filtering** — control-character sanitization plus a
  keyword screen for common prompt-injection phrases on chat channels and
  the WebSocket. It is a heuristic, not a guarantee — treat prompt
  injection as unsolved, like every framework should.
- **TLS (source builds)** — compile with `--features tls` and point
  `tls_cert_path`/`tls_key_path` at your certs (e.g. issued via
  certbot/Let's Encrypt). No built-in ACME client. Honest caveats: the
  prebuilt release binaries do **not** include the TLS feature today, and
  with certs configured but the feature absent the gateway logs a warning
  and serves plain HTTP — both are open hardening items on the roadmap.
  For production, a TLS-terminating reverse proxy in front of the
  loopback bind is the recommended setup.

## Migrating from OpenClaw?

```bash
garra migrate openclaw            # --dry-run to preview, --source <dir> for custom paths
```

Imports your skills and channel configurations. Credential files are
detected and listed but **not copied** — re-enter API keys via
`garra init` so they land in the encrypted vault.

## Configuration

GarraIA reads `~/.garraia/config.yml`:

```yaml
gateway:
  host: "127.0.0.1"
  port: 3888

llm:
  claude:
    provider: anthropic
    model: claude-sonnet-4-5
    # api_key resolution: vault > config > ANTHROPIC_API_KEY env var
  ollama-local:
    provider: ollama
    model: llama3.1
    base_url: "http://localhost:11434"

channels:
  telegram:
    type: telegram
    enabled: true
    bot_token: "your-bot-token"   # or TELEGRAM_BOT_TOKEN

agent:
  max_tokens: 4096
  fallback_providers: [openrouter, ollama-local]
  max_history_messages: 20
  summarize_threshold: 40

memory:
  enabled: true
  auto_extract: true
```

Run `garra config check` to validate the effective configuration with
precedence reporting. Full reference — including Discord, Slack,
WhatsApp, iMessage, voice, embeddings, MCP, timeouts, rate limiting and
`.garraignore` — in [docs/](docs/) and the
[Portuguese README](README.pt-BR.md#configuração).

## Architecture

A Rust workspace of **22 crates**, each with a single responsibility:

```text
crates/
├── garraia-cli/        # CLI, init wizard, daemon management, self-update
├── garraia-gateway/    # WebSocket gateway, HTTP API, web console, REST v1
├── garraia-agents/     # LLM providers, tools, MCP client, agent runtime
├── garraia-channels/   # Telegram, Discord, Slack, WhatsApp, iMessage (+6 pending wiring)
├── garraia-auth/       # Auth v1: Argon2id, JWT HS256, RBAC, RLS-backed identity
├── garraia-workspace/  # Postgres 16 + pgvector multi-tenant (FORCE RLS, 29 tables)
├── garraia-security/   # Credential vault, allowlists, pairing, validation
├── garraia-db/         # SQLite memory, vector search, sessions
├── garraia-config/     # YAML/TOML config, hot reload, `config check`
├── garraia-voice/      # Whisper STT → LLM → Chatterbox/Hibiki TTS
├── garraia-plugins/    # WASM plugin sandbox (wasmtime)
├── garraia-embeddings/ # EmbeddingProvider / VectorStore traits
├── garraia-learning/   # Self-improving skills (mining, safety gate, versioning)
└── ...                 # telemetry, media, skills, storage, tools, runtime, common, glob, desktop
apps/
└── garraia-mobile/     # Flutter client (Riverpod, go_router) — Garra Cloud Alpha
```

Deep dives: [runtime flow & voice pipeline](README.pt-BR.md#arquitetura),
[ADRs](docs/adr/), [wiki](https://github.com/michelbr84/GarraRUST/wiki).

## Roadmap

Development follows a 7-phase plan in [ROADMAP.md](ROADMAP.md) — currently
deep in **Phase 3: Group Workspace**, a multi-tenant family/team space
(files, chats, AI memory, Notion-like tasks) on Postgres 16 + pgvector
with FORCE Row-Level Security. Recent milestones: auth v1 (Argon2id +
15-min JWTs + refresh tokens), the 37-table RLS schema, tus resumable
uploads, object storage (local + S3), OpenTelemetry baseline, the Garra
Learning agent, and this benchmark harness.

## Contributing

GarraIA is MIT-licensed open source. Join the
[Discord](https://discord.gg/aEXGq5cS), check
[CONTRIBUTING.md](CONTRIBUTING.md), and filter by
[`good-first-issue`](https://github.com/michelbr84/GarraRUST/issues?q=label%3Agood-first-issue+is%3Aopen).
Support channels are listed in [SUPPORT.md](SUPPORT.md); security reports
go through [SECURITY.md](SECURITY.md).

## License

MIT
