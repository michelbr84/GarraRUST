# What's New in v0.4.5

> 🇧🇷 [Versão em português](Novidades-v0.4.5) · 📋 [Full CHANGELOG](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md) · 📦 [Download](https://github.com/michelbr84/GarraRUST/releases/tag/v0.4.5)

This release removes unrestricted `bash` from the places where no human
confirms commands, and it finishes what v0.4.4 left half-done. In the
`standard` profile, `garraia mcp-server` and the gateway only give the model a
shell inside a `docker`/`podman` sandbox. Saying "yes" to a confirmation
request approves it again on every channel with a person on the other end.
`garraia whatsapp link` asks who may talk to GarraIA. And the boot now
**refuses** two configurations that used to start wide open: an exposed bind
without a credential, and half-configured TLS.

---

## `bash` only in a sandbox where no human is in the loop (#1272)

Before, in `standard`, `garraia mcp-server` and the gateway runtime registered
a `bash` that ran on the host. The risky-command tier only catches what
*looks* dangerous, so a `cat /etc/shadow` or an `echo x > /anywhere` requested
by the model went through. The decision is now a pure function of the profile
and `agent.sandbox`, and it never comes from detecting a container:

| `agent.sandbox` for `bash` | `standard` (default) | `isolated-pod` (explicit) |
| --- | --- | --- |
| Required and usable: `docker`/`podman` with `mode: all` (without `bash` in `elevated`) or `allowlist` with `bash`, binary present | registered; every command runs in the container, with no host fallback | registered, in the sandbox |
| Not required: `mode: off` (the default), `bash` in `elevated`, `allowlist` without `bash` | **does not exist** | registered on the pod host, with the denylist and risky tier on |
| Required but unusable: `backend: ssh`, no backend, binary missing | **does not exist** | **does not exist**, and the warning gives the real reason |

- **The same rule covers the gateway's `run_tests`.** That tool runs the
  `scripts.test` of `package.json`, `build.rs` and `conftest.py`, and
  `file_write` can write all three.
- **No sandbox, no shell.** The boot logs one `warn!` saying why and how to
  turn it back on, `/api/diagnostics` gains the `tools.bash` check, and the
  `garra_agent` system prompt tells the model there is no shell.
- **The container is now a real boundary.** It gets `--cap-drop ALL`,
  `--pids-limit 512` and the operator's `--user <uid>:<gid>`
  (`--userns=keep-id` on podman). Only the session's canonical working
  directory is mounted: `/`, `$HOME`, an ancestor of it, or a session without
  a `working_dir` is a refusal. On timeout the container is removed.
- **`git_diff` and `code_review` do not run programs planted in the
  repository.** Git runs with no fsmonitor, hooks, textconv, filters or
  submodules, and `file_write` refuses any path with a `.git` component.
- `garraia chat` is unchanged: a human confirms in the terminal there.

The decision is recorded in the 2026-09-21 Amendment of
[ADR 0024](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0024-perfis-de-execucao-isolated-pod.md).

## "Yes" approves again, on every channel with a human (#1343)

When a tool asked for confirmation (a risky `bash` command with
`agent.tool_confirmation_enabled`, or an R3/R4 `device_execute`), the turn
paused, and the "yes" in the next message approved nothing. The channels store
history as text, so the paused request never came back and the tool asked
again forever. The request now stays in memory. The next message, if it is
**entirely** an approval word (`sim`, `yes`, `ok`, `confirma`, `confirmar`,
`proceed`, `approve`), runs the request **once**, within **5 minutes**, and
only if it comes from the same sender, in the same session and on the same
channel:

| Where | Who can approve |
| --- | --- |
| Web Console (`/ws`) and desktop (`/ws/parrot`) | the same connection; after a reconnect it asks again |
| `/v1/chat/completions` (with and without `"stream": true`) | the owner, with the **same** `Authorization` and the **same** `X-Session-Id` on both requests |
| Mobile app (`POST /chat`) | the JWT `sub` |
| Telegram, Discord, Slack, WhatsApp, Matrix, IRC, Signal, LINE, Teams, Google Chat, iMessage, personal WhatsApp | the user's platform id |
| `garraia chat` | the terminal itself |

- In a group, another member's "yes" does not approve, and it **ends** the
  request.
- Any other message in between, "no" included, ends the request (#1340). A
  second "yes" pauses again, and restarting the gateway cancels every pending
  request.
- These paths still have no resume, on purpose:
  - A2A and OpenClaw;
  - `POST /api/sessions/{id}/messages`;
  - the agent's reply in the workspace chat, and scheduled tasks;
  - `garraia ask` and the `garra_agent` of `garraia mcp-server`.
- **The request reaches you without the internal marker** (#1373). Users no
  longer see `[CONFIRM_REQUIRED:…]` in the message or in the `garraia chat`
  tool line. The sentence still names the command and says "Responda **sim**
  para executar".
- **Phone numbers are masked in the log.** The WhatsApp and Signal session ids
  embed the sender's number, and the runtime spans logged it in full. The
  `RedactingWriter` of stderr and `garraia.log` now replaces every run of 10 or
  more digits not glued to a letter with `…` plus the last 4.

Full refusal matrix:
[`docs/security/threat-model.md` §5.16](https://github.com/michelbr84/GarraRUST/blob/main/docs/security/threat-model.md).

## Personal WhatsApp: who may talk, and a bridge that updates itself

- **`link` asks who may talk to GarraIA** (#1345). After the QR, the command
  asks for the number with its country code and only says "ready" once someone
  is authorized. An empty answer leaves the gate closed and prints the warning
  plus the command that fixes it later.
- **`garraia whatsapp allow <number> [--owner] [--yes]`** authorizes without a
  terminal. The number needs `+` and the country code, and `<id>@lid` is also
  accepted. It exits 65 on an invalid number, and 64 for `--owner` outside
  `isolated-pod` or in a pipe without `--yes`. Owner is only offered in
  `isolated-pod`, defaulting to no.
- **Authorizing and revoking take effect without a restart.** The gateway
  re-reads `allow`, `owners` and `enabled` from `config.yml` on every message.
  To revoke, delete the number from the list in the file. `enabled: false`
  refuses everyone on the next message. A Brazilian mobile number with and
  without the ninth digit matches as the same number.
- **Honest status.** `garraia whatsapp status` shows
  `Authorized: N · Owners: M` (counts, never numbers). `/api/diagnostics` and
  the boot log warn when nobody is authorized.
- **The bridge updates itself at boot** (#1373). After a `garraia update`, the
  gateway rewrites the bridge embedded in the binary before launching it.
  Before, it kept launching the previous version's `bridge.mjs` until someone
  linked again. `npm ci` only runs when `node_modules` is missing or nothing
  proves the installed tree matches the embedded lock.
- **The log shows the right ending of the connected number** (#1373). Before,
  the JID's device suffix was counted.
- **A dead-bridge error carries the tail of Node's stderr again** (#1368).

## MCP `filesystem` without a corrupted npx cache (#1346)

- Provisioning on a new install pins
  `@modelcontextprotocol/server-filesystem@2026.8.31`, instead of downloading
  the newest build on every cold cache. An existing `mcp.json` is never
  rewritten: the `mcp.filesystem_pinned` check of `/api/diagnostics` warns
  installs without a version and gives the exact `args` to paste.
- An `ERR_MODULE_NOT_FOUND` inside `<npm cache>/_npx/<hash>/` removes only
  that validated entry and retries once.
- Once `max_restarts` is exhausted, the error is logged once, not every 30 s.
- `/api/mcp/health` now lists servers that failed at boot, with `status`,
  `cause` and `last_error`. The `mcp.servers` check gives the next step for
  each cause.

## Garra knows what it has: `garra_status` (#1347)

With linked WhatsApp connected, Garra answered "I don't have access to
WhatsApp". Three things changed:

- **`garra_status` joins the whitelist modes** (`search`, `architect`,
  `debug`, `orchestrator`, `review`, `edit`). The runtime appends an
  instruction to consult it before denying access to a channel.
- **The report sees what `/api/channels` sees.** The list includes
  `whatsapp_linked` and the push channels, each with a `status`
  (`active`/`offline`), and it comes from the same function. On a fresh
  install the list is empty, instead of showing eight `offline` channels. The
  report gains `execution_profile`, `mcp_servers` (name, state and tool count
  only) and `session.channel`, and `session.id` is masked.
- **Operator data stays with the operator.** The report withholds
  `working_dir`, `project_id`, the provider list, the MCP server names and the
  exact version, and lists what it withheld in `withheld`, in two cases:
  - a restricted-gate turn, such as the WhatsApp `search` floor;
  - a session not proven to be the operator's: mobile app, A2A, messaging
    channels, an unknown session, or a gateway exposed through the keyless
    opt-out.
- The streaming turn now applies the mode's prompt and `max_tokens`, like the
  batch path.

## A boot that refuses what would fail open (#1261, #1247)

- **An exposed bind without a credential does not start.** `garraia start`,
  `start -d` and `restart` exit 78 when any bind address is not loopback and
  there is neither `gateway.api_key` nor `GARRAIA_GATEWAY_API_KEY`. The
  refusal comes before the bind, before the fork in `-d` (so the message
  reaches the terminal) and before `restart` stops the running daemon. TLS
  does not exempt the bind, and a name that does not resolve is refused too.
- **New env var `GARRAIA_GATEWAY_API_KEY`.** It wins over the file, an empty
  value counts as unset, and it is never written to `config.yml`.
  `config check` reports it by presence only.
- **Deliberate opt-out**: `gateway.allow_unauthenticated_network_bind: true`,
  file only, with no env var or flag, and a loud warning on every boot.
- **`gateway.host`/`gateway.port` in the file are deprecated.** Those keys
  never fed the bind. `garraia init` stops writing them and removes them on a
  re-run, and `status`, `stop`, `doctor` and `admin` use the same address as
  `start`. `garraia restart` now reads `HOST`/`PORT`, like `start`.
- **`config check` runs on every boot.** `start`, `restart` and `start -d` run
  the same check once: each `Error` is logged as an error and each `Warning`
  as a warning. In `start -d` the lines go to stderr before the fork.
- **Only a closed list refuses the boot**: today, half-configured TLS (only
  `tls_cert_path` or only `tls_key_path`), which used to serve plain HTTP in
  silence. Every other `Error` is reported and brings nothing down.

## Sandbox, runs and runtime

- **The sandbox covers the repository tools** (#1225). With
  `agent.sandbox.mode: all`, `run_tests`, `git_diff`, `code_review` and
  `repo_search` run in the container, with an argv built without a shell.
  `backend: ssh` is refused for them.
- **Runs ledger** (#1227):
  - `garraia runs list` reads `sessions.db` without talking to the gateway,
    with `--status`, `--limit` (default 50) and `--json`;
  - `GET /api/runs` is read-only, returns previews of up to 120 characters,
    and has stricter access than the rest of `/api/*`;
  - `runs.retention_days` turns on a sweep (at boot and every 24 h) that
    deletes old terminal runs. The default `0` never deletes, and a `running`
    run is never deleted.
- **Two dead crates are removed** (#1226): `garraia-tools` and
  `garraia-runtime`. The workspace is down to 22 crates, and the
  `garraia max-power` snapshot announces `tool_program`.
- **The loop detector warns before it aborts** (#1295). On the first
  repetition (same tool, same arguments, three times in a row), the model
  receives a corrective note instead of the turn dying. Any later detection in
  the same task aborts as before, and the blocked call never runs.
- **`garraia max-power`** (#1228). `--goal` no longer aborts with "Cannot start
  a runtime from within a runtime", and with no usable provider it runs
  offline, as its help promises. The command writes to `garraia.log`, and a
  body-read error from an OpenAI-compatible provider now states the real
  cause.

## Fixes from the clean-install smoke test

The v0.4.4 clean-install smoke test found these:

- **`install.sh` in containers and CI** (#1369). Without a real terminal, the
  installer takes the non-interactive path with no error lines. A non-empty
  `CI` also counts as no terminal, as in `install.ps1`.
- **`garraia.log` is kept** (#1371). `garraia start -d` opens the log in append
  mode, instead of erasing the previous run and writing over the head of the
  file.
- **`garraia status | head` exits quietly** (#1371), with no broken-pipe
  panic, in every command that only reads and prints.
  `GET /admin/api/logs` reads only the last 512 KiB and no longer returns 500
  on a non-UTF-8 byte.
- **REST sessions survive a restart** (#1372). `GET .../history`,
  `POST .../messages` and `DELETE /api/sessions/{id}` stop returning 404 for a
  session that is in `sessions.db`. `DELETE` writes the `api_logout` marker,
  and a closed session does not come back.
- **A provider's key only goes to its own `llm:` entry's endpoint** (#1370).
  `garraia ask -p openai` sent `llm.openai.api_key` to
  `https://api.openai.com` even when the entry had its own `base_url`, and the
  audit found the same class of bug on four more CLI paths. The kind's
  environment variable (`OPENAI_API_KEY`, …) only goes to that kind's default
  host.

## Infrastructure and process

- **Quality Ratchet** (#1254): `freeze-baseline.py` gains
  `--adopt-current-file-metrics --reason '#NNN'`. The flag adopts only the
  file-size metrics and records where they came from. Audit, coverage and
  clippy stay on the strict ratchet.
- **A PR without a fragment in `changelog.d/` goes red**, with an exemption
  through the `no-changelog` label.
- **Coverage and the ratchet edit a single comment per PR**, instead of
  posting a new one on every push.
- **Swagger UI is vendored**, with no download during the build.
- **The release uploads the CLI's aarch64 AppImage again** (#1344), and CI
  checks the upload list.

## Upgrading from 0.4.4

```bash
garraia update
```

What an existing install runs into when it starts v0.4.5:

- **An exposed bind without a credential no longer starts** (exit 78, with a
  message that says how to fix it). `install.sh` installs and the systemd unit
  bind to loopback and are unaffected. If you expose the port, do this
  **before** upgrading:
  - set `gateway.api_key` in the file, or export
    `GARRAIA_GATEWAY_API_KEY="$(openssl rand -hex 32)"`. `garraia init`
    also writes the key when the machine is server-like (root or a RunPod pod)
    or when `HOST` is not loopback;
  - or bind to loopback only again, with `garraia start --host 127.0.0.1`,
    and remove `HOST` from the environment;
  - the **Docker image** (its `CMD` binds `0.0.0.0`), the
    `docker-compose*.yml` files and **RunPod pods** need
    `GARRAIA_GATEWAY_API_KEY` in the environment. Without it the container
    exits with 78 and `restart: unless-stopped` loops, so
    `docker compose ps` shows `Restarting`. `.env.example` ships the line empty
    on purpose, for you to fill in;
  - **Helm**: the `gatewayApiKey` block (generated on install and kept on
    upgrade, or `existingSecret`/`value`). `helm template`/ArgoCD need one of
    the two;
  - **Terraform/ECS**: the required variable `gateway_api_key_secret_arn`.
    Without it, `terraform plan` fails before the image is swapped;
  - behind an authenticating proxy, with the port open on purpose, use
    `gateway.allow_unauthenticated_network_bind: true` in the file;
  - `/ping`, `/health` and `/api/health` stay open for health checks, and
    clients send the key as `Authorization: Bearer <key>`.
- **`garraia restart` now reads `HOST`/`PORT`.** A daemon restarted in a pod
  with `HOST=0.0.0.0` now binds the exposed address, instead of silently
  falling back to loopback, so it also needs the credential.
- **Config errors that block the boot.** With only `gateway.tls_cert_path` or
  only `gateway.tls_key_path` set, `start`, `restart` and `start -d` exit 78
  and name the missing field. To start anyway, set
  `GARRAIA_ALLOW_INVALID_CONFIG=1` (exactly `1`); the finding is still logged
  as an error. This escape hatch does **not** turn off the exposed-bind
  refusal. Every other `config check` `Error` now shows up in the log on every
  boot but does not stop it. A `memory.retention` with `interval_hours` or
  `max_age_days` out of range no longer crashes the worker with a panic: the
  sweep does not start, and nothing is deleted.
- **In `standard` without a sandbox, `garraia mcp-server` and the gateway have
  no `bash`** (and the gateway has no `run_tests`). The boot warns once and
  `/api/diagnostics` shows `tools.bash`. To get the shell back, contain it:
  ```yaml
  agent:
    sandbox:
      mode: all
      backend: docker   # or podman; the binary must be installed
  ```
  Or, **only** if the process really runs in a disposable pod, declare
  `execution.profile: isolated-pod`. `file_write` now refuses paths with
  `.git`.
- **`agent.sandbox.mode: all` now also contains `run_tests`, `git_diff`,
  `code_review` and `repo_search`.** The default image
  (`debian:bookworm-slim`) only has `grep`, so point `agent.sandbox.image` at
  an image with `git`/`cargo`/`rg`, or list the tool in
  `agent.sandbox.elevated`. With `network_disabled: true`, `cargo` cannot
  download crates.
- **The gateway's first boot rewrites the WhatsApp bridge.** The gateway
  replaces the files that differ from the embedded ones (between 0.4.4 and
  0.4.5 only `bridge.mjs` changed). It runs `npm ci` only if `node_modules` is
  missing or nothing proves the tree matches the lock. A 0.4.4 install whose
  `npm ci` finished is adopted without `npm`, through
  `node_modules/.package-lock.json`. If `npm ci` is needed, `npm` must be on
  the gateway process's `PATH`. Without it, or if it fails, the bridge does
  not start. `garraia whatsapp status` then shows
  `Bridge:   dependencies missing` (in English) and `/api/diagnostics` gives
  the step `rode npm ci em <dir> e reinicie o gateway` (run `npm ci` in the
  directory, then restart the gateway). Do that from a shell that has `npm`;
  the gateway adopts the tree at the next boot. The linked session is not
  touched.
- **WhatsApp with an empty `allow`.** The gateway always dropped those
  messages, but it now warns at boot, in `status` and in `/api/diagnostics`.
  Run `garraia whatsapp allow +<country><number>`. Edits to
  `allow`/`owners`/`enabled` now apply on the next message, and a `config.yml`
  that does not parse keeps the previous list. `garraia whatsapp allow`
  rewrites the file, and comments are lost.
- **Mobile app sessions count as restricted in `garra_status`.**
  `/auth/register` is open, and having an account on the gateway does not
  prove you are its operator. In those sessions the report withholds the
  directory, `project_id`, providers, MCP server names and the exact version.
  The same applies to web, API, VS Code and desktop on a gateway exposed
  through the keyless opt-out.
- **Resuming an approval on `/v1/chat/completions` needs a stable
  `X-Session-Id`.** Without it, every request is a new session and the "yes"
  approves nothing. Send the same `X-Session-Id` and the same `Authorization`
  on the request and on the "yes". With no owner claimed in the allowlist, the
  pause stays terminal.
- **An `llm:` entry with its own `base_url` and no `api_key` no longer receives
  the kind's environment variable**, including through
  `agent.default_provider`. An OpenAI-compatible endpoint gets the
  `not-needed` placeholder, and `anthropic`/`openrouter` refuse with an error.
  Put the `api_key` in the entry itself. The `https://openrouter.ai/api/v1`
  that `garraia init` writes keeps working with the key in the environment. An
  ad-hoc `--url` only uses `LLM_API_KEY` or the key of an entry with that same
  `base_url`.
- **Limits documented in this release's fragments:**
  - `garraia.log` now grows across starts with no rotation, as it already did
    with a foreground `garraia start`;
  - `runs.retention_days` stays at `0` (never delete) until you change it, and
    until then the boot logs how many runs exist;
  - an existing `mcp.json` is not rewritten. Follow `mcp.filesystem_pinned` to
    pin the version, which covers only the top-level package (its dependencies
    follow their semver ranges);
  - a REST session comes back from disk without its `working_dir`;
  - a contact who paired with a `/pair` code and was never in `allow` stays
    admitted until the restart;
  - `gateway.host`/`gateway.port` stay in the schema, and `config check` flags
    them as deprecated when they differ from the effective bind;
  - the Web Console shows the bind as read-only;
  - pending confirmation requests are dropped when the gateway restarts;
  - the loop warning costs at most one extra LLM round, and no setting turns
    it off;
  - the CI's `swagger-ui-cache` action stays for one release as a leftover.

## Known limits

- **A mode's `temperature` still does not reach the provider.** The mode's
  prompt and `max_tokens` reach it on every turn. Chat, channel and API turns
  still send no temperature, so the provider uses its own default. Sending it
  would change the request of every turn that has a mode (the built-in modes
  declare 0.3 to 0.7). That waits until the provider can omit the parameter
  for models that reject it. See
  [`docs/src/modes.md`](https://github.com/michelbr84/GarraRUST/blob/main/docs/src/modes.md).
- **REST sessions closed before 0.4.5 can be read again.** The `api_logout`
  marker only exists for a `DELETE` made from this version on. A REST session
  closed on an earlier version has no marker, and after the upgrade
  `GET /api/sessions/{id}/history` serves it again as a live session. The
  route belongs to the operator (loopback, or the `api_key` gate on an exposed
  bind), and the history is already in the operator's own `sessions.db`. To
  close it for good, send a new `DELETE`, which now writes the marker.
- **Sandboxed `bash` is still a shell line.** `run_tests`, `git_diff`,
  `code_review` and `repo_search` start the container through argv, with no
  shell (#1225). `bash` still builds its `docker run`/`podman run` as one line
  that a shell on the host interprets, with the command in single quotes
  (`sh_quote`). Converting `bash` to argv is not in this release. Both paths
  apply the same containment flags (`--cap-drop ALL`, `--pids-limit 512`,
  `--user`/`--userns=keep-id`, `no-new-privileges`), and neither has
  `--read-only` or a memory limit
  ([threat model](https://github.com/michelbr84/GarraRUST/blob/main/docs/security/threat-model.md),
  priority 9). In `standard` without a sandbox, `bash` is not registered at
  all, so this limit only applies with a configured `docker`/`podman` sandbox
  or in `isolated-pod`.
- **The gateway still resolves provider keys as config > env without looking
  at `base_url`.** The #1370 rule applies to the CLI (`ask`, `chat`,
  `max-power`, `garra_ask`/`garra_agent`). In the gateway, an entry without a
  key can still receive the kind's environment variable. See
  [`docs/configuration.md`](https://github.com/michelbr84/GarraRUST/blob/main/docs/configuration.md).
- **A WhatsApp contact identified only by `@lid`, with no number, does not
  match a number in `allow`.** Its messages are refused in silence, and
  `status` counts those refusals. To authorize the contact, use a `/pair` code
  or `garraia whatsapp allow <id>@lid`.
- **Refusing another member's "yes" has no end-to-end test in each of the 11
  `bootstrap/` channels.** One end-to-end test covers the shared path, and a
  static guard pins, per file, which identifier becomes the sender. The guard
  proves the identifier, but not that the platform delivers it authenticated.
  On IRC the nick is the only identity, and it can be taken over without
  NickServ.
