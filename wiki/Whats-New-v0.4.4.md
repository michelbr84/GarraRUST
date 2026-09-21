# What's New in v0.4.4

> 🇧🇷 [Versão em português](Novidades-v0.4.4) · 📋 [Full CHANGELOG](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md) · 📦 [Download](https://github.com/michelbr84/GarraRUST/releases/tag/v0.4.4)

The release that makes personal WhatsApp work on a **fresh** install and gives
operators who run Garra in a pod an explicit way to hand the agent full power.
In v0.4.3, "install + `garra whatsapp link`" never answered: the channel refused
to start because every new install gets an MCP server. That is fixed. And for
anyone who wants a fully autonomous agent inside a disposable container, there
is now the `isolated-pod` execution profile, with one rule: **full power inside
the isolated pod; no implicit access outside the pod.**

---

## Personal WhatsApp on a fresh install

- **The channel starts with MCP registered** (#1327). The
  `FerramentaMcpRegistrada` refusal compensated for a `ToolGate` gap closed in
  #1288 and had become obsolete; because the `filesystem` server is provisioned
  on first boot, it switched the channel off on every default install. The
  `search` floor still denies MCP tools by name on every turn.
- **Both `garra` and `garraia` exist** (#1328). `install.sh` creates `garra` as
  a relative symlink to `garraia`, `install.ps1` writes a `garra.cmd` shim, and
  the `.deb`/`.rpm` packages ship `/usr/bin/garra`. A `garra` the installer did
  not create is kept, with a warning.
- **Every instruction names the executable that is running** (#1329). The
  post-link message says `garraia start` when you ran `garraia` (and
  `garra start` when you ran `garra`); the same holds for
  `garraia whatsapp status`, the `/api/diagnostics` next step, the gateway log
  when the channel does not start, and `whatsapp link` failures.
- **The boot no longer warns `unknown channel type: whatsapp_linked`**, and every
  reason the channel does not start is a `WARN` that states the action.

## Execution profiles: `standard` and `isolated-pod` (ADR 0024)

| | `standard` (default) | `isolated-pod` |
| --- | --- | --- |
| How to enable | nothing to do | `execution.profile: isolated-pod` or `GARRAIA_EXECUTION_PROFILE=isolated-pod` (env wins) |
| WhatsApp owner, 1:1 chat | `search` floor | `code` floor: `bash`, `file_write`, subagents and every MCP tool |
| Paired-only contact, admitted non-owner, any group | `search` floor | `search` floor — full power is never inherited |
| Auto-provisioned MCP `filesystem` root | `agent.file_roots`, else `<data_dir>/workspace` | `execution.pod_root`, else `<data_dir>/workspace` |
| Risky-command gate, file-tool jail, sandbox | on | on |

```yaml
execution:
  profile: isolated-pod
  pod_root: /workspace          # optional; absolute path inside the pod
channels:
  whatsapp_linked:
    type: whatsapp_linked
    enabled: true
    owners: ["5511999998888"]   # only declared owners, only in 1:1 chats
```

- **Explicit, never inferred.** No code reads `/.dockerenv` or cgroups to pick
  the profile (two tests scan the source). An invalid value refuses the boot and
  is an `Error` in `garraia config check` (exit 2), in the file and in the env.
- **Auditable.** At boot, a single `WARN` says what the profile unlocks, what it
  does **not** isolate (host filesystem, Docker/Podman socket, host namespaces,
  undeclared mounts, host secrets) and how to revert. The profile and its source
  (`default`/`file`/`env`) show in `config check`, the `execution.profile`
  check of `/api/diagnostics`, the `security.execution_profile` row of
  `/api/settings/effective` and `garraia whatsapp status`. Each WhatsApp turn
  logs the applied profile (`completo`/`padrao`) and mode, identifying the
  sender only by the last 4 digits.
- **The owner is a declared identity.** Only entries in `owners` get the full
  profile, and only in 1:1 chats. Pairing by code admits a contact but never
  makes an owner. Use digits (`5511999998888`) or the `@lid` JID; `config check`
  warns when an `…@s.whatsapp.net` entry can never match.

## MCP `filesystem` without `$HOME`

The server provisioned on first boot no longer points at your home directory in
**either** profile. Only the default `<data_dir>/workspace` is created; a
declared root that does not exist skips provisioning (with a warning) instead of
falling back to anything wider. An existing `mcp.json` is **never rewritten** —
if your install predates v0.4.4, the `mcp.filesystem_root` check of
`/api/diagnostics` warns in `standard` and says how to fix it.

## `tool_program`: several steps in one turn, gated per step

The model can send a program of up to 16 steps (`tool_program`) and the runtime
runs it without going back to the LLM between steps (#1226 S-B). Every step goes
through the same dispatch as the normal loop: same `ToolGate`, same budget, same
confirmation pause. The `auto`, `code` and `ask` modes expose `tool_program`;
the whitelist modes (`search`, `architect`, `debug`, `orchestrator`, `review`,
`edit`) do not. The post-merge review closed eleven findings before the release:
an unresolved `$name` fails the step, a pause gives the model what earlier steps
already did, and a one-step program no longer slips past the loop detector.

## Other fixes

- **Command approval only covers the request that paused the turn** (#1339).
  A page read by `web_fetch`, a file, or an MCP result that carried a copy of a
  real confirmation marker could compete with the actual request, and your
  "ok" covered the wrong command. Now only the request itself carries a valid
  marker in the history.
- **`config check` and the bind** (#1261): the check judges the bind by what
  `garraia start` uses (`HOST`/`PORT` env, else `127.0.0.1:3888`) and flags the
  file's `gateway.host`/`gateway.port` as keys `start` does not read. The
  behavior decisions (refusing an exposed bind without credentials, the fate of
  those keys) remain with the owner in #1261.
- **Update notice** (#1320): only announces a release **newer** than the binary.
- **Docker image** builds again with the WhatsApp bridge, and the release's
  aarch64 AppImage is packaged again.
- **CI**: the Security Gate no longer times out after the tests pass (#1332).

## Upgrading

```bash
garraia update
```

- Nothing changes without configuration: with no `execution` section, the
  profile is `standard`.
- To enable the profile in a pod, see [`docs/execution-profiles.md`](https://github.com/michelbr84/GarraRUST/blob/main/docs/execution-profiles.md).
- Installs whose `filesystem` still points at `$HOME`: change the last argument
  of `filesystem` in `<config_dir>/mcp.json` to a directory inside the declared
  roots and restart.
- `garraia config check --strict` may start exiting 2 on wizard-generated
  configs with `host: 0.0.0.0` (a key `start` does not read); see #1261.

### Known limitations

- `isolated-pod` trusts the pod boundary **you** built: if the container has
  the Docker socket, `--privileged` or `--pid=host`, so does the agent.
- The profile only changes the floor of personal WhatsApp; other channels are
  unchanged.
- The native file-tool jail still applies in `isolated-pod`: also declare the
  pod root in `agent.file_roots` if you want `file_write` outside the workspace.
