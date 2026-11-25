# What's new in v0.5.2

> 🇧🇷 [Versão em português](Novidades-v0.5.2) · 📋 [Full CHANGELOG](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md) · 📦 [Download](https://github.com/michelbr84/GarraRUST/releases/tag/v0.5.2)

Stable release from November 25, 2026 — replaces v0.5.1 as the `latest` release.

v0.5.2 is the stable cut of a series that went through **seven release
candidates** (rc1 to rc7, November 3 to 22). It brings continuous voice
conversation in the browser, a Desktop app that sets itself up without a
terminal, a linked WhatsApp that never drops a message silently, modes and an
approval level in the CLI, and diagnostics that say where each claim came
from.

## Continuous Voice Chat in the Web Console (#1754)

- **One call, not one button per turn.** The `#/voice` page starts the call
  once, listens without a press per utterance and can be interrupted: speak
  over GarraIA and it stops.
- **Swap mid-call.** STT, conversation model, TTS, voice and language change
  without losing the session.
- **Local or OpenRouter.** It uses the local `--with-voice` servers and the
  OpenRouter instances you already configured, with pricing as the provider
  reports it. The browser only picks IDs: URL, key and routing policy stay on
  the server, and STT/TTS have no fallback.
- **A bounded voice base.** Size-capped responses, timeouts and clients that
  do not follow redirects; Chatterbox audio is only fetched from the
  configured origin, and a timeout no longer triggers a second paid synthesis.

Guide: [docs/voice.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/voice.md) · decision: [ADR 0027](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0027-natural-voice-conversation.md).

## Desktop: who answers, without a terminal (#1700)

- **The "Quem responde" (Who answers) step.** The wizard offers local Ollama
  (detected, with its installed models) or OpenRouter (key + model) and
  verifies it with a real call. The final screen only says it is ready to
  talk once the model has answered; otherwise it says why not.
- **Another installation on the machine** (#1816, #1824, #1826). With another
  Garra on port 3888 or holding the config directory, the app reports which
  version is serving, does not start a second gateway, asks for explicit
  confirmation before writing into the other installation's config and keeps
  a copy of `config.yml` under `backups/`. Pairing WhatsApp on top of it is
  refused.
- **`garraia start` refuses before boot** when the port is taken or the
  config directory is already in use (exit 69).
- **Double-clicking the CLI AppImage** (#1817) opens a terminal instead of
  "doing nothing", and the release now runs the AppImages before publishing
  (#1818).

## Linked WhatsApp

- **The "You" chat converses** (#1813, #1822). In "my own number" mode, a
  message to yourself becomes a turn — without Garra replying to itself and
  without ever answering third parties on the owner's behalf. The opt-in is
  explicit: `garra whatsapp access self-chat on|off` (`access.self_chat`).
  Without the key in your config, nothing changes on update.
- **A message sent during a restart gets answered** (#1814): anything that
  arrived "offline" up to 10 minutes ago becomes a turn; older messages are
  dropped with a reason.
- **No drop is silent** (#1815): decryption failures, expired messages and
  non-text content show up in the log, the Web Console and
  `/api/diagnostics`.
- **No double replies** (#1822): a redelivery with the same id is not a
  second turn, each chat runs one turn at a time, in order, and the
  reconnection backlog becomes one batch and one reply per chat.

## CLI: modes, approval and tool memory

- **`garra chat --mode` and `/mode`** (#1858) apply the same modes as the
  gateway. `plan` is an alias for `architect`: read-only, every mutating tool
  is denied.
- **Approval level** (`agent.approval`): `always` (the default, as before),
  `session` (the same approved request does not ask again until the session
  ends) and `never-confirm`, with `--yolo` as a shortcut that never persists.
  The hard deny (`rm -rf /`, fork bomb, exfiltration) and fail-closed
  behaviour without a confirmation channel are unchanged at every level, and
  `never-confirm` leaves an audit trail. Details in
  [docs/hardening-gateway.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/hardening-gateway.md).
- **Tool history across turns** (#1708): the next turn receives calls and
  results paired by id — including a call interrupted by Ctrl+C.
- **`cli_inspect`, `/status` and `/tools` read the same source** (#1709,
  #1712) and report the turn's effective permissions and the effects the
  executor recorded. Guide: [docs/cli-runtime-inspection.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/cli-runtime-inspection.md).
- **`garra skills mine|candidates|list`** (#1901): the skills pipeline from
  the CLI, with no gateway.
- **Timeout windows** (#1790, #1793): `garra ask` cut by `timeouts.llm` exits
  with 124; probes use `timeouts.health` and `garra mcp` uses `timeouts.mcp`.

## Diagnostics and review with evidence

- **`garra_diagnostics`** (#1914) reports the executable's SHA-256, the
  build's commit and tag, which process is answering and who supervises it,
  and when the observation was made. `garra_status` no longer confuses an RC
  with a stable release.
- **Remote review** (#1909): `repo_search` says where it looked when it finds
  nothing; `git_diff` and `code_review` name the exact range, without leaking
  the host's absolute path.
- **An outcome per call** (#1915): every tool call ends with a classified
  outcome, a denial names the layer that blocked it and a cancelled call does
  not count as a model failure.

## Security

- **Safety gate** (#1791, #1861): reading the environment through
  .NET/PowerShell, downloading via `WebClient`/BITS and
  `[Diagnostics.Process]::Start` ask for confirmation; a leading assignment
  (`DEBUG=1 node server.js`), code piped through stdin and interpreters with
  a version in their name no longer slip through. And a command that merely
  **quotes** a dangerous literal (`echo 'never run rm -rf /'`) is no longer
  blocked.
- **Approvals bound to the workspace** (#1912): switching workspaces requires
  a new approval; a "yes" is consumed once and does not survive a restart.
- **No runtime HTTP client follows redirects** (#1792), and credentials in
  query strings are redacted.
- **HSTS** when the transport is https (#1895); a **declared body cap on
  webhooks** (#1987); **clean shutdown** — a stdio MCP server takes its whole
  process tree down and gateway shutdown is bounded (#1913).
- **Dependencies:** `rmcp` 3.9.0, wasmtime 50.0.3, SQLite 3.54,
  `jsonwebtoken` 12, `rustix`, `quinn`, `url` 2.5.9 and `cookie` 0.18.3,
  closing RUSTSEC advisories.

## Install or update

```bash
# Linux/macOS
curl -fsSL https://garraia.org/install.sh | sh
```

```powershell
# Windows
irm https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.ps1 | iex
```

Already on v0.5.1? `garra update`. The installers now finish an upgrade over
a running gateway (#1862, #1865) and print the restart command.

Docker image: `ghcr.io/michelbr84/garraia:0.5.2`.
