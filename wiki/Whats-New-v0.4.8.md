# What's New in v0.4.8

> 🇧🇷 [Versão em português](Novidades-v0.4.8) · 📋 [Full CHANGELOG](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md) · 📦 [Download](https://github.com/michelbr84/GarraRUST/releases/tag/v0.4.8)

**Security leads this release.** On v0.4.7, `bash` runs on the host **without**
an allowlist boundary. v0.4.8 introduces the `HostComAllowlist` mode (#1272):
`agent.bash_allowlist` stops being a confirmation waiver and becomes a real
limit. Alongside it, the `wasmtime` runtime behind the WASM sandbox moves from
48.0.2 to 49.0.2 in two steps (#1524, #1559) and closes **eleven advisories**.

The release also finishes what v0.4.7 deferred: the global **Agents &
Permissions** page in the Web Console (#1433) and policy **import/export** for
WhatsApp access (#1435). Around that, it re-wires the six channels the README
already advertised (#1567), fixes quote escaping in the `bash` allowlist
(#1579), and repairs LLM provider resolution.

---

## Bash on the host, bounded by an allowlist (#1272)

Up to v0.4.7, `agent.bash_allowlist` only waived the confirmation on a command.
Outside a `docker`/`podman` sandbox, there was no way to run `bash` on the host
with a real limit.

The `HostComAllowlist` mode changes that. When the config declares command
patterns (a trailing `*` prefix, or an exact command), `BashTool` enters
allowlist-only mode: the list becomes a **boundary**, and any command outside it
is denied before the risky tier. The `safety_gate` denylist stays in front of
everything. This is the path to give the agent a shell on a device without
Docker — Android/Termux, for example.

Two adjustments ship with the mode:

- **Quotes now protect the argument's prose (#1579).** The old scan treated
  parentheses, `;`, `|` and `$` **inside quotes** as a compound command. A
  command carrying `"(transcribe the full content)"` in its message was denied
  even with the prefix allowed. The check now follows shell tokenization: single
  quotes literalize everything, double quotes keep only backtick, escape and
  expansion `$` active. A real compound (`;`, `&&`, pipe, `$(...)`,
  redirection, newline) still never matches.
- **`garraia doctor` shows the agent's shell on Termux (#1560).** With no
  docker/podman (always the case on Android) and no `agent.bash_allowlist`, the
  Termux block shows a `Tool bash` warning with the allowlist-only recipe —
  instead of the operator finding out only through the agent itself. The
  `HostComAllowlist` description in `doctor`, `/api/diagnostics` and
  `garra_status` now says "NO sandbox" explicitly.

## wasmtime 49 closes eleven advisories (#1524, #1559)

The WASM runtime moved in two steps:

- **48.0.2 → 49.0.1 (#1524):** closes four advisories — fuel not counted in
  `call_ref` and dynamic record lifting, host memory exhaustion writing an
  outgoing HTTP body, and a panic on an out-of-range datetime in the WASI
  filesystem `set-times`. This step **raises the MSRV floor to 1.96**, because
  the cranelift 0.136.1 tree declares `rustc 1.96.0`.
- **49.0.1 → 49.0.2 (#1559):** closes seven advisories — three in `wasmtime`
  (GC heap corruption and a stack buffer overflow in an async-lifted callback)
  and four in `wasmtime-wasi` (`poll_oneoff` bypassing fuel accounting,
  over-allocated host memory without stdio, `fd_readdir` copying uninitialized
  padding, and a pre-epoch filesystem timestamp crashing the host). This step
  does **not** raise the MSRV: the 49.0.2 tree stays on `rustc 1.96.0`.

Neither step required an API migration in `garraia-plugins`. The scheduled
`Security - cargo audit` had gone red over the seven advisories in the second
step.

## The Web Console finishes what v0.4.7 deferred (#1433, #1435)

v0.4.7 left two pieces of Access Policy v2 open. v0.4.8 delivers both.

**Global Agents & Permissions page (#1433).** It generalizes the *WhatsApp
Access* page into one place: each principal and its effective capabilities per
channel, through the same policy engine (ADR 0025) and the capability registry's
vocabulary, with no duplicated authorization logic. The read route
`GET /admin/api/permissions/overview` returns the execution profile, the class
legend (each marked sensitive or not), and each channel with its policy source —
today only `whatsapp_linked` has a per-principal engine, and the rest are listed
honestly. The page does not edit: it routes to the single path of the *WhatsApp
Access* page. Granting a principal a mutating class it did not have now requires
explicit confirmation, **enforced on the server**: without `confirm_elevation:
true` the endpoint refuses with `409`.

**Export and import the policy, with no secret (#1435).** `garraia whatsapp
access export` and `GET /admin/api/whatsapp/access/export` produce a versioned
document assembled field by field from an allowlist — token, vault and session
material are excluded by construction. `--redact-pii` masks identities to their
last four digits and marks the export as non-importable. `import` validates
format and version, refuses a redacted export, replaces the whole policy, shows
per-principal impact with `--dry-run`, and requires explicit confirmation for
any widening. The write is atomic, with a backup for rollback, and lands in the
audit log.

## The six channels the README advertised count again (#1567)

The README, README.pt-BR and the ROADMAP said Google Chat, Microsoft Teams,
Matrix, LINE, IRC and Signal were "implemented in the crate, gateway wiring
pending", and the badge advertised "5 channels". All six were already on in the
default build: `garraia-gateway` enables the channel features unconditionally
and boot instantiates the six, with no `cfg`. The badge becomes **11**, and the
status table distinguishes a channel with unit coverage in the crate from one
covered by a gateway integration test — which is the real pending item, now
named.

## Voice: a healthy Chatterbox, and the agent speaking on its own

- **A healthy Chatterbox stops being reported as down (#1538).** The probe hit
  `/gradio_api/config`, a route no Gradio version serves: with a running
  `multilingual_app.py`, the gateway said `❌ tts-chatterbox` and kept voice
  mode off. The chain is now `/` → `/config` → `/gradio_api/info`, stops at the
  first 2xx, and is the same in both places that used to probe separately. The
  failure WARN names each route and what it answered.
- **`telegram_send_voice` tool (#1557).** The agent synthesizes the text (up to
  1000 characters) with the configured TTS and delivers it as a Telegram voice
  message. The target is the same decision as `telegram_send`, with its own
  ceiling of 2 voices per minute per session. A TTS failure becomes an explicit
  error, with no silent fallback to text; the tool disappears from the list when
  TTS is missing or the channel is offline.
- **`docs/voice.md` gains "Known pitfalls with the stock app" (#1539).** Two
  traps for first-time Chatterbox users: the health probe on the wrong route
  (fixed above) and the default voice prompts, which are URLs passed raw to
  `librosa.load` and break synthesis.

## LLM: the right provider, and an error that is no longer swallowed

A set of defects made the gateway answer through the wrong provider, or fake
success on an empty response. v0.4.8 closes all of them (#1540, #1541, #1554):

- **Two `llm:` profiles of the same type stop fighting over one slot.** Each
  profile is now registered by its **key** (`with_name`), not its type. Before,
  two `provider: openai` entries collided and the effective provider was drawn
  fresh at each boot. This also covers `anthropic` and `llamacpp` (#1554).
- **The fallback uses ITS model.** The loop passed the primary's `model` to the
  fallback, and `model` is the name *that* backend understands — an `openrouter`
  receiving `glm53-flash` answered `400`. The fallback now uses the model the
  config already named for it.
- **SSE stops swallowing the provider error.** Under `stream: true`, a turn's
  `Err` was discarded and the stream ended in `finish_reason: "stop"` — a blank
  response that looked like success. The stream now emits `{"error": {...}}`.
- **`POST /v1/chat/completions` with no `model` stops sending `"gpt-4"`.** A body
  with no `model` reaches the provider as the empty sentinel it resolves with its
  own configured model, and the response's `model` echoes who actually answered.

## Reliability and operation

- **`restart` no longer creates an orphan daemon under systemd (#1542).** One
  v0.4.6 install reached 4606 restarts in ~8h because `restart` killed the
  unit's daemon and brought up a process outside it, fighting over `Address
  already in use`. The CLI now reads the cgroup of whoever holds the port and, if
  it is a systemd unit, refuses **before** stopping anything (exit 78), naming
  the correct `systemctl restart`. Hatch: `GARRAIA_ALLOW_SYSTEMD_RESTART=1`.
- **MCP remediation points at a command that exists (#1543).** The texts said
  `garraia mcp restart <name>`, which clap does not know; they now point at
  `POST /admin/api/mcp/<name>/restart`, the same as the Restart button in the
  console. A test scans the source so the phantom command cannot return.
- **The repo `.mcp.json` boots the MCP server again (#1536).** The entry pointed
  at `./target/release/garra`, a path that only exists after a build and only
  resolves from the repo root. It now uses `garra` from `PATH`.

## Quality: coverage measured, and a floor that fails the build (#1566)

`.quality/baseline.json` said `coverage_pct: null` on every run — the ratchet job
always looked before the coverage job finished. The search now scans recent runs
for a `completed`+`success` one and falls back to the last green on `main` when
the PR SHA hasn't closed yet. The baseline now carries **72.00%** line coverage,
measured and written from `lcov.info` — never by hand, the anti-fraud invariant
still holds.

A coverage floor that **fails** the build ships with it: `scripts/ci/
coverage_gate.py` fails the job below `70.0%`, reads `lcov.info` through the same
parser that writes the baseline, is fail-closed (missing or empty fails), and
runs as the last step so the failure never hides the information from whoever has
to fix it. The AI Quality Ratchet stays **report-only**: no PR is blocked by it.

---

## How to update

```bash
garra update            # CLI; keeps the previous version and supports `garra rollback`
```

Fresh installs go through the [usual installer](Instalacao-e-Primeiros-Passos).

## Known limits

- Windows installers and desktop bundles are still **unsigned** — SmartScreen
  warns on the first run of the desktop MSI.
- The Tauri desktop auto-updater is still off; the supported update path is the
  CLI's `garra update`.
- The six re-wired channels (#1567) have unit coverage in the crate, but a
  gateway integration test for them is still missing.
