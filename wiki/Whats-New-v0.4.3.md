# What's New in v0.4.3

> 🇧🇷 [Versão em português](Novidades-v0.4.3) · 📋 [Full CHANGELOG](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md) · 📦 [Download](https://github.com/michelbr84/GarraRUST/releases/tag/v0.4.3)

The release where Garra reached your **personal** WhatsApp. Until now the
WhatsApp channel required a Business account, a number registered with Meta and
a public URL — a bar most home users never clear. Now `garra whatsapp` draws a
QR code in your terminal, you scan it with your phone, and the number already in
your pocket talks to the agent, with the session encrypted on disk and the
gateway supervising the bridge.

Alongside it came the largest security pass of any release so far: five MCP
fail-opens closed, a directory jail on the file tools, the indirect-injection
guard extended to MCP, files and devices, and the per-tool sandbox
(`agent.sandbox`) that existed in full and no install could turn on. In the
runtime: a single default LLM (`z-ai/glm-5.3-flash` via OpenRouter), the mode
auto-router on every entry point, a local fallback when the network drops, and
a `garra chat` that survives crashes, timeouts and Ctrl+C.

---

## Personal WhatsApp by linked device

Before: only the Meta Cloud API. Now there are two paths, and they don't mix.

```text
garra whatsapp
  1) Connect my personal WhatsApp (scan a QR code)      →  garra whatsapp link
  2) WhatsApp Business through Meta's official Cloud API →  garra whatsapp cloud
```

| Command | What it does | Exit code |
|---|---|---|
| `garra whatsapp` | two-option menu | 0, or 1 if cancelled |
| `garra whatsapp link` | link by QR | 0 · 1 cancelled · 69 no Node / QR not scanned · 70 internal error |
| `garra whatsapp cloud` | Cloud API wizard | 0 · 1 cancelled · 70 internal error |
| `garra whatsapp status` | says whether a link exists and whether the session opens | 0 linked · 69 not linked or unreadable |
| `garra whatsapp logout` | deletes the session and disables the channel | 0 · 1 cancelled |
| `garra whatsapp restore` | puts `session.enc.prev` back in place | 0 · 69 nothing archived or session in use · 70 internal error |

Without a terminal (`ssh server 'garra whatsapp'`), the menu prints both
options with their commands and exits 0, like `garra init`; if you already
chose `link`, you get the reason, the way out (`ssh -t …`) and exit 69 instead
of a false success (#1238).

### How it works inside

```text
garra whatsapp link / gateway (serve)
     |  NDJSON v1 over stdio · ping/pong · env_clear + allowlist · PDEATHSIG · Drop kills the child
bridge/whatsapp/bridge.mjs   Node + Baileys 7.0.0-rc14, STATELESS — never writes to disk
     |
WhatsApp Web (linked device)
```

- **The bridge is deliberately dumb.** Node handles only the WhatsApp
  protocol; Rust draws the QR, encrypts and stores the session, applies the
  allowlist and routes to the agent. Auth state lives in the bridge's memory
  and comes back as a full snapshot (`session_update`) for Rust to persist
  (#1238, ADR 0023).
- **Session encrypted at rest.** AES-256-GCM (the same stack as the
  `CredentialVault`) in `<data_dir>/whatsapp/default/session.enc`, `0600`
  files in a `0700` directory, atomic writes. The key derives from
  `GARRAIA_VAULT_PASSPHRASE` when present; otherwise it lives in a local
  `session.key`, and `status` says so plainly.
- **Nothing destructive before the last confirmation.** A consent screen
  precedes the first QR (unofficial client, the account may be banned, use a
  secondary number). A re-link that never gets to write a new session — QR
  expired, Ctrl+C, bridge dying, WhatsApp refusing — restores the previous
  one; `enabled = true` reaches the config only **after** the session exists
  on disk, so an aborted pairing never leaves the gateway paying a timeout on
  every boot.
- **Every clock has a ceiling.** Handshake, QR, "connecting",
  authenticated-but-never-connects, the courtesy `shutdown` (500 ms), `npm ci`
  (600 s, no orphan): each pairing phase says how long is left and gives up
  with the right message. A test enumerates every phase and fails to compile
  if someone adds one without saying what its ceiling is.

### The `whatsapp_linked` channel in the gateway

The message scanned in by QR reaches the agent through its own **pull**
channel, and its threat model is different: the message comes from anyone who
knows the operator's personal number. Three decisions follow, none of them
configurable downwards (#1238):

1. **Its own allowlist, fail-closed.** You get in with a `/pair` code or via
   the config's `allow` list; an empty gate means **nobody**. The global
   allowlist won't do here because every sibling channel fills it on its own
   by auto-claiming the first sender.
2. **Read-only tools by default.** A session with no chosen mode resolves to
   the `search` profile until the operator raises it with `/mode`.
3. **Indirect-injection guard** on the incoming text.

Groups only get answers with explicit opt-in, and your own messages
(`from_me`) never start a turn. The gateway supervises the bridge in
`mode: "serve"` with proof of life in the protocol: after connecting, a `ping`
every 15 s with a reply required within 45 s, otherwise it falls to the
reconnect backoff (#1283). Ctrl+C on the gateway **cancels** the supervisor —
the Node bridge no longer outlives "shut down gracefully". `garra whatsapp
status`, `GET /api/channels` and the `whatsapp.linked` check in
`GET /api/diagnostics` classify through the same function, each with an
actionable next step ("run `garra whatsapp link`", "run `npm ci` in <dir>").

### What lands in your terminal

The tail of Node's stderr is the only raw external-tool output in the flow,
and it is redacted before it shows: long sequences that look like key material
(standard and url-safe base64, including ones glued to field names such as
`creds.noiseKey=` or `npm ERR! _auth=`) become `<redigido: N caracteres>`,
measured over 2000 keys with ~1.3 characters of residual leak (#1238, #1276).
The account name refuses `con`, `prn`, `aux`, `nul`, `com1`–`com9`,
`lpt1`–`lpt9`, which cannot become directories on Windows.

<!-- wave2 -->
- The stderr redaction also covers **percent-encoded** base64 and base64 with
  the slash escaped as `\/`, the shapes a library `throw` or serialized JSON
  produce (#1276).

**What needs Node:** only the QR path. `node` and `npm` are looked up on the
`PATH`, and the requirement is **Node.js 20 or newer** (the bridge's
`engines`: `>=20`). The Cloud API needs no Node. The bridge lives in
`bridge/whatsapp/` — a directory, not a crate — and the Rust tests run
against a fake Python bridge, with no Node and no phone; pairing with a real
device is manual validation, described step by step in the docs.

📖 [`docs/whatsapp.md`](https://github.com/michelbr84/GarraRUST/blob/main/docs/whatsapp.md) · [ADR 0023](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0023-whatsapp-dispositivo-vinculado.md)

---

## Security

### Per-tool sandbox: `agent.sandbox`

The containment from #1222 existed in full — `SandboxPolicy`, backends,
fail-closed when the backend is missing, eleven tests — and **no install could
turn it on**: all three production constructors of `BashTool` pinned `off`,
and the config key the error messages cited did not exist in the schema. Now
the section exists, read by the same function at the three production points
(gateway, `garra chat`, `garra mcp-agent`), and `garra config check` rejects a
sandbox that looks on but isn't (#1225).

| Key | Values | Note |
|---|---|---|
| `mode` | `off` (default) · `all` · `allowlist` | missing section = `off`, byte for byte the previous behaviour |
| `backend` | `docker` · `podman` · `ssh` | `mode != off` without a backend is an **error** |
| `image` | container image | default `debian:bookworm-slim` |
| `ssh_host` | remote host | required for `ssh` |
| `sandboxed_tools` / `elevated` | tool lists | `elevated` escapes and runs **on the host** |
| `mount_workdir` / `network_disabled` | bool, default `true` | **ignored** by `ssh` |

What each backend guarantees — and does not — is the table in threat model
§5.13. Three limits hold for all of them: **only the `bash` tool is wrapped
today** (`run_tests`, `git_diff`, `code_review` and `repo_search` still run on
the host even with `mode: all`); the `ssh` backend **is not a sandbox**, it is
remote execution that isolates the local host and nothing else; and in
practice this is Unix — on Windows `wrap_command` fails closed and
`config check` reports an error.

Two holes in the first version closed with it: shell metacharacters escaped to
the host because the command was quoted with `{:?}` instead of POSIX quoting
(#1231), and a config value starting with `-` (`ssh_host: "-oProxyCommand=…"`)
became an **option** of `ssh`/`docker` rather than an argument — now refused
in three layers, never logging the value (#1225). The sandboxed command goes
through `redact_secrets` and truncation before reaching the log, at `debug!`.

<!-- wave2 -->
- `backend: ssh` with `network_disabled` or `mount_workdir` on is now
  **refused fail-closed**: since ssh ignores both, the config has to say
  `false` explicitly so the operator acknowledges there is no containment
  (#1225 S3).
<!-- wave2 -->
- `mode: all` declares at startup and in `config check` that it covers only
  `bash`, listing `run_tests`, `git_diff`, `code_review` and `repo_search` as
  host-only (#1225 S2).
<!-- wave2 -->
- An integration test with real Docker proves `--network none` on Linux CI
  (#1225 S4).

📖 [`docs/security/threat-model.md` §5.13](https://github.com/michelbr84/GarraRUST/blob/main/docs/security/threat-model.md) · [`config.hardened.example.yml`](https://github.com/michelbr84/GarraRUST/blob/main/config.hardened.example.yml)

### MCP: five fail-opens closed

- **Restarting a server through the admin API no longer wipes its tool
  allowlist** (#1242). The restart reconnected with an empty list — which
  means "allow everything" — after `disconnect` had discarded the only copy.
  The allowlist is now resolved **before** teardown, against the same merge
  (`mcp.json` + `config.yml`) the boot reads, and a restart never widens what
  is in force.
- **`DELETE /admin/api/mcp/{id}` actually revokes** (#1262): it drops the live
  connection, clears `pending` (from which the health monitor used to
  resurrect the deleted server with the freshly revoked secrets) and releases
  the tools from the runtime.
- **The agent mode whitelist now applies to MCP tools** (#1264). `ToolGate`
  exempted any name containing `__`; a read-only mode let a write tool from
  an MCP server through. Permission is now declared: `my-server/*` allows the
  server, a full name allows just that tool, `denied` wins over everything.
- **HTTP entries in `mcp.json` are no longer dropped by the loader** (#1274)
  — and with them the `allowed_tools` the operator wrote is no longer lost.
- **An admin write no longer erases `allowed_tools`, `inherit_env` and
  `enabled` from the other servers in `mcp.json`** (#1273): the two types
  sharing that name now agree on a single schema.

And at the child-process boundary:

- **Stdio MCP servers stop inheriting the gateway's entire environment**
  (#1075). Every `npx -y some-server` received `GARRAIA_JWT_SECRET`, the
  provider keys and the whole `.env` without a single tool call. The
  environment is now built from a minimal allowlist (`PATH`, `HOME`, locale,
  `TMPDIR`, CA bundles) plus that server's `env` map; `inherit_env: true` is
  the per-server escape hatch, with a `warn!` and unavailable through the
  admin API.
- **`vault:` references in an MCP server's `env` are resolved at boot**,
  fail-closed (#1237): an unresolvable reference keeps the server from
  starting, naming server and key, never the value. Before, the literal
  string `vault:…` reached the child as if it were the secret.
- **MCP tool output enters the context through the indirect-injection guard,
  with a 256 KiB cap** and visible truncation (#1243).
- **`POST /api/mcp/marketplace/install` requires an authenticated admin** with
  `Permission::ManagePlugins` and refuses `env` that decides *which* code the
  child runs: `PATH`, `LD_PRELOAD`, `NODE_OPTIONS`, `npm_config_*` (#1245).

### The `api_key` gate and exposed binds

- **`gateway.api_key` now covers the conversation plane and A2A** (#1240):
  `POST /v1/chat/completions`, `/v1/messages`, `/v1/messages/count_tokens` and
  `/a2a/*` sat outside the gate, which tested only the `/api/` prefix. The
  parrot socket (`/ws/parrot`) came along, and Garra Desktop now actually
  sends the credential. **With no `gateway.api_key` configured, nothing
  changes.**
- **`garra init` on a VM as root no longer leaves the gateway on the internet
  with no credential** (#1241): a non-loopback bind generates 32 CSPRNG bytes
  into `gateway.api_key`, `config.yml` is born `0600`, and the key is **not**
  printed to the terminal. The boot warns once when the bound address reaches
  the network and there is no credential — including in `garra start -d`,
  before the fork.
- **A random web page can no longer fire POST/PATCH/DELETE at the local
  gateway** (#1182): a generic anti-CSRF guard over the whole mutating
  surface, `Origin` checked on the `/ws` and `/ws/parrot` handshake, and
  **CORS closed by default** — an empty `gateway.allowed_origins` now means no
  cross-origin origin at all (see "Upgrading").
- **`/pair` works again on every channel** (#1189) — each channel built its own
  always-empty `PairingManager` — and the pairing code gains attempt limits
  (5 per user / 15 min, 20 global attempts burn the code), constant-time
  comparison and a heads-up to the owner when something was burned (#1191).

### The agent's tools

- **Directory jail on the file tools** (#1244). `file_read`, `file_write` and
  `list_dir` accepted `~` and absolute paths with no confinement: a prompt
  over Telegram could read `~/.ssh/id_rsa`. `FileJail` now confines to the
  union of `agent.file_roots` (+ `GARRAIA_FILE_ROOTS`) and the session's
  `working_dir`; an empty set **denies everything**. It canonicalizes before
  comparing (`..`, symlinks, `~`, absolute) and refuses dangling symlinks; the
  refusal is a single sentence, with no path, so the tool cannot become a
  file-existence oracle. Declared residuals: TOCTOU and hardlinks.
- **Indirect-injection guard on `file_read` and the hardware tools** (#1243):
  file contents and whatever comes off the bus (device id, Home Assistant
  `state`, MQTT/serial payload) reach the model framed as untrusted data when
  suspicious; clean text passes byte for byte.
- **The model's query no longer lands in flag position**: `repo_search`
  builds `rg`/`grep`/`findstr` with the query after `--` — `--pre=/bin/sh`
  made ripgrep itself execute a `.txt` (#1266); `git_diff` puts `file_path`
  after `--` and refuses revisions starting with `-` — `--ext-diff` reopened
  external execution via a planted `.git/config` (#1269). The children of
  `run_tests` and `bash` stop inheriting the gateway's stdin (#1270); the full
  inventory of the 14 `Command` call sites is in the threat model.
- **`POST /api/providers` connects with an HTTP client pinned to the
  SSRF-validated IPs and redirects off** (#1248), closing redirect laundering
  to `169.254.169.254` and DNS rebinding; the providers' default constructors
  also stop following redirects.

Also: `rustls` 0.23.45 closes RUSTSEC-2026-0285 (#1206); `deny.toml` drops a
dead `ignore` after poise 0.7 (#1202); `ConfigLoader::save` became atomic
(`0600` tmp + `rename` + fsync) and an empty `config.yml` no longer silently
turns into defaults (#1238).

---

## Agents and runtime

### One default LLM

`z-ai/glm-5.3-flash` via OpenRouter is now the official default on every
surface (#1180, ADR 0022). The question "which LLM does Garra use when the user
picks nothing?" had five answers depending on the door: `openrouter/auto` in
chat and the wizard, `openrouter/free` in `mcp-server`, `openai/gpt-4o` in the
gateway, `lmstudio` on the desktop. Now there is one constant, locked by a
test. Local (Ollama) stays intact as the **second** option in
`agent.fallback_providers`; `garra chat` autodetect tries cloud providers with
credentials before Ollama; and the wizard highlights "Cloud-first" without
pre-selecting the ~18 GB local download. **If you pinned
`GARRAIA_MCP_MODEL_ALLOWLIST=openrouter/free`, you need to adjust** (see
"Upgrading").

### The turn that doesn't die

- **A dropped network triggers the local fallback** (#1249). `connection
  refused`, DNS and timeouts matched no retry pattern and killed the turn
  before the fallback loop. `Error::Transport` is now typed: it spends **one**
  attempt and falls straight to the next provider, without burning ~3.5 s of
  backoff.
- **A stream that breaks midway redoes the turn** through the batch path, when
  nothing has reached the sink yet (#1176).
- **OpenRouter's 404 `No allowed providers are available`** becomes a routing
  configuration error with the model, the effective restriction and a
  recovery path, and is never retried (#1299).
- **A missing tool parameter becomes a soft observation**, with the schema and
  a request to resend — the model corrects itself within the turn instead of
  the turn failing with `agent error:` (#1296).
- **The mode auto-router applies on every entry point** (#1223): `garra
  chat`, `POST /api/chat`, the webchat, the app and the channels. It only acts
  when `agent.auto_router_llm_enabled` is on (default **off**), the session
  has no mode and there is text; an inferred mode grants **no** tool.

<!-- wave2 -->
- **The loop detector's error says what repeated**: the tool, the count inside
  the window and the repeated input, truncated (#1295).

### `garra chat`

- **`--resume` with no value (or `latest`) resumes the most recent session**,
  and the question is written **before** the turn runs, with a marker
  `[turno interrompido: timeout|cancelado|erro]` when it ends without an
  answer — crashes, timeouts and Ctrl+C no longer erase the turn. New
  `/resume [id]` in the REPL (#1300).
- **`/model` became transactional** (#1298): it validates against the
  provider's real catalog (OpenRouter: the full `GET /models`) before
  switching; a missing model or an unresponsive provider keeps the previous
  state.
- **The REPL no longer shows raw tracing** — retry, fallback and circuit
  breaker stay in `garraia.log`; `--verbose`, `--debug` and `RUST_LOG` win
  (#1301).
- **The project context stops spending prompt on `target/`** and now says
  which project it is (from `README.md`) and on which branch (#1219).
- **`memory.auto_extract` and `memory.max_facts`** finally exist: turn off
  just the per-turn fact extraction call without turning off semantic memory
  (#1221).

<!-- wave2 -->
- **Line editor with rustyline**: arrow keys, persistent history (`0600` in
  `~/.garraia/history`), Ctrl+D exits; pipes and CI stay on `read_line`
  (#1297).

### Runtime: run ledger and single dispatch

- **Sub-agent runs leave a trail** in the `agent_runs` table (#1224), and the
  ledger has its first production writer (#1227): gateway and CLI startup
  convert `running` runs left by a crash into `interrupted` (ids in the log,
  never the `goal`), and every scheduler execution writes a row with a
  terminal outcome. Ledger failure is fail-soft.
- **The four copies of the turn loop dispatch tools through a single
  function** (`dispatch_tool_call`): budget, loop detection, mode gate,
  timeout and confirmation pause live in one place (#1226).
- **`git_diff` and `code_review` answer about the right repository**: git
  runs in the session's `working_dir`, and without one the answer says which
  repository it spoke about (#1258).
- **`ToolRegistry::execute_program`** — a JSON program of N steps in a single
  turn, all-or-nothing variable substitution, `max_steps` 16 (#1224). Still no
  production caller.

<!-- wave2 -->
- The scheduler **claims tasks with a lease** (`pending → running`), and the
  re-poll after a crash is explicit and logged (#1227 S2).
<!-- wave2 -->
- `DbRunLedger` accepts the gateway's tokio `Mutex`; `SubAgentConfig.session_id`
  (#1227 S3).

---

## Hardware, desktop, mobile and storage

- **The hardware skills catalog now applies at runtime** (#1250). The rule
  "effective risk = max(adapter, skill)" existed and no deployment exercised
  it. The boot now loads the catalog before starting any adapter, every device
  is wrapped in a decorator that only **raises** risk, and `device_list` shows
  the presets' pt/en `aliases` — "living room light" maps to the id the gate
  sees.
- **ADR 0021 (Desktop Control Center) accepted**, with the first two
  milestones delivered (#1181): the `garraia-desktop-core` crate — the
  **Tauri-free** core that enters the CI gates (`state`, `detect`,
  `supervise`, `locate`) — and `garra desktop [--status] [--no-launch]`,
  which locates and launches the installed app (exit 0/69/70) without the CLI
  gaining a Tauri dependency. Resolution **never** returns the CLI's own
  executable, which in the `.deb` is the app's sibling.
- **Garra Mobile in pt-BR and English** with a language selector in Settings
  (255 keys, `flutter gen_l10n`, a test that fails on new hard-coded strings)
  (#1178), and the **launcher icon** drops the Flutter default for the
  WolfMark (#1177).
- **S3 multipart upload above 16 MiB** (#1214): 8 MiB parts, peak memory down
  from 5 GiB to 8 MiB per upload, any failure aborts the multipart. **SHA-256
  checksum verified per part by the server** (#1229), with
  `request_checksum_calculation` pinned so a deployment cannot downgrade it.
  And the MinIO suite starts a real container again (`quay.io`, #1230).

---

## Operations and CI

- **Every crate declares the MSRV** (`rust-version = "1.95"`, directly or
  inherited from the workspace), and the **`changelog.d/` fragment format
  became a CI gate** (#1228).
- **The CodeQL ledger anchors by content, not by line number** (#1263): the
  checker derives the line from the `sink_snippet`, and the gate only turns
  red when the statement changed, vanished or became ambiguous. The `reapply`
  accepts multi-line statements (alert 173).
- **`#[cfg(windows)]` code under the `mcp` feature compiles in CI** on a
  native Windows step (#1253), and the docs say file-permission hardening
  (`0600`/`0700`) is Unix-only.
- **`garra config check` is an opt-in command, not a boot gate** — the prose
  claiming otherwise was corrected (#1247), and the idle clause of
  `validate_session_token` went through a bind, not `format!`.
- Docs: the real bind precedence (`--host`/`HOST` > env > default; the file's
  `gateway.host`/`gateway.port` do **not** feed `garra start`) (#1261); real
  TTS providers in `docs/voice.md` (#1246); the `garraia-media` PDF and
  magic-bytes tests run again (#1208, #1209).

<!-- wave2 -->
- **The Quality Ratchet reports the delta against the PR's merge-base** (a
  distinguishable signal) and warns when the baseline is older than 90 days;
  the baseline was **not** re-frozen (#1254).

### Deprecated

<!-- wave2 -->
- `ToolRegistry::execute_program` in `garraia-tools` is marked `deprecated`;
  the #1224 fragment was corrected (#1226 S-E).

---

## Upgrading

```bash
garra update          # existing installs
```

Or reinstall: `curl -fsSL https://garraia.org/install.sh | sh`
(Windows: `irm https://garraia.org/install.ps1 | iex`).

**No data migration is needed.** Three things change meaning, and one section
is new:

- **`agent.sandbox` is new and defaults to `off`** — a missing section
  reproduces the previous behaviour byte for byte.
<!-- wave2 -->
- If you already turned on `backend: ssh`, you must declare
  `network_disabled: false` and `mount_workdir: false` explicitly, or the
  config is refused (#1225 S3).
- **An empty `gateway.allowed_origins` now means "no cross-origin origin"**
  (#1182). If you reach the Web Console by **DNS name** — reverse proxy,
  `nas.local`, MagicDNS, ingress — list the origin
  (`https://garraia.yourdomain.com`). IP, `localhost` and the default profile
  do not change.
- **`GARRAIA_MCP_MODEL_ALLOWLIST=openrouter/free` breaks** (#1180): the new
  default is evaluated before the allowlist. Include `z-ai/glm-5.3-flash` or
  remove the variable.
- **`garra chat` autodetect tries cloud before Ollama** (#1180). If you relied
  on Ollama beating an exported cloud key, pin
  `agent.default_provider: ollama`.

### Known limitations

- The sandbox wraps **only the `bash` tool**; `run_tests`, `git_diff`,
  `code_review` and `repo_search` still run on the host. `backend: ssh` is not
  a sandbox. On Windows the sandbox fails closed.
- The WhatsApp bridge requires **Node.js 20+ and `npm`** on the host; the
  Cloud API does not.
- The bridge has **no memory ceiling**: `RLIMIT_AS`, which MCP applies, kills
  Node's V8 right at boot (it reserves several GiB of virtual memory), so
  containment is the clean environment, PDEATHSIG and `Drop`.
- Personal WhatsApp is **one account only** (`default`).
- Pairing with a real device is **manual** validation — CI covers the
  protocol, the state machine, the encrypted store and the child lifecycle
  against a fake bridge.
- ARM64 and the per-platform installers remain **best-effort**, as in v0.4.2:
  a release may ship without one of them.
