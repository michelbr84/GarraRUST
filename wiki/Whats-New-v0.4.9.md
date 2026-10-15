# What's New in v0.4.9

> 🇧🇷 [Versão em português](Novidades-v0.4.9) · 📋 [Full CHANGELOG](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md) · 📦 [Download](https://github.com/michelbr84/GarraRUST/releases/tag/v0.4.9)

An October 2026 release, large and on two fronts.

The first is **data subject rights**: export, email correction, anonymization
and deletion stop being a promise and become a working path in the gateway, each
closed tightly enough not to turn into an account-takeover door or an account
enumeration oracle.

The second is the **gateway as an MCP server over HTTP** gaining task
delegation: an external orchestrator can now ask the Garra LLM a question and
request a full task, with its own identity, its own budget, and secret
redaction. Around that: a dependency advisory sweep, the desktop updater with a
DMG and `latest.json`, and a band of reliability fixes.

---

## Data subject rights come off the page

Four data-subject-rights endpoints (`GET /v1/me/export`, `PATCH /v1/me`,
`POST /v1/me/anonymize`, `DELETE /v1/me`) now work end to end.

- **Full export (`GET /v1/me/export`).** It used to return account data only;
  it now includes the subject's personal content across all of their groups —
  messages they wrote (full body), mentions received (snippet), uploaded files,
  memories and tasks — in a single transaction that re-points the tenant GUC per
  group, with fail-closed RLS and a subject-id filter on both ends. The file
  declares what was left out (`exclusions`) and what hit a ceiling
  (`truncated_sections`), because a silently incomplete export is a false
  compliance claim.
- **Email correction with no blind swap (`PATCH /v1/me`).** `users.email` is the
  login key, so changing it on an access token alone would be an
  account-takeover primitive. Since the repo has no email-sending
  infrastructure, the request is recorded as `pending` plus audit, and applying
  it is a manual operator step with out-of-band proof of possession. The endpoint
  also does not reveal whether the requested address already exists — answering
  that would become an account enumeration oracle.
- **Deletion with a grace window and a dedicated worker (`DELETE /v1/me`).** The
  same transaction that lays the tombstone and revokes sessions enqueues the
  request with a 30-day grace period. The new `account_purge_worker` destroys the
  subject's content and identifiers (message, comment, memory, file plus versions
  and blob in the object store) and leaves the `users` row as a tombstone
  (`status = 'purged'`) — it is not removed, because five FKs point at it and
  removing it would erase third-party context; another member's message is never
  deleted by one person's request. `audit_events` survives de-identified: it is
  the only proof the deletion happened. **With no object store wired, the request
  is NOT closed** — marking it `completed` with a live blob would lie in the
  audit.

The worker runs with dedicated credentials and is resumable. The `garraia_purge`
role (migration 034) has only `EXECUTE` on the four `SECURITY DEFINER` deletion
functions — no table privileges, not `BYPASSRLS`. The worker only starts if
`GARRAIA_PURGE_DATABASE_URL` is set; without it the log warns and the manual
procedure still applies. Progress is persisted on the request's own row, in the
same transaction as the deletes: a crash midway re-runs neither the database pass
nor closes the request with a live blob.

## The MCP HTTP gateway gains task delegation

`POST /mcp` already read state and sent messages. Now an external orchestrator
can also ask the Garra LLM a question and request a full task. Everything is
**off by default** and carries its own budget, which never shares the sending
one.

- **`garra_ask` over HTTP (#1612).** It only appears with
  `gateway.mcp_http.allow_ask: true`, accepts only the project's default model
  while `ask_allowed_models` is empty, and has `ask_budget_per_minute` (default
  10). The `garra ask` core was extracted into the `garraia-ask` crate, shared
  by the CLI and the gateway.
- **Orchestrators with their own identity (#1613).** `mcp_http.orchestrators[]`
  declares a `name` and a `key_env` — the key value never goes into the config.
  Each orchestrator has its own policy (a subset of tools and destinations
  intersected with the global allowlist), per-call attributable audit, and secret
  redaction in both payload and error. An empty list keeps the prior behavior.
- **`garra_agent` over HTTP (#1615).** The orchestrator requests a full task —
  read and write files within the allowed roots, search the web, inspect git,
  and, **only with a sandbox**, run a command — and gets the `garra.agent.v1`
  envelope with a summary of each tool used. It only appears with
  `gateway.mcp_http.allow_agent: true` and with `agent` in the orchestrator's
  tool list. It has `agent_budget_per_minute` (default 2) and a time cap
  `agent_max_seconds` (default 300, max 600). Input and output pass through
  secret redaction, and no prompt, response or tool output goes to the log.
- **Operator-extensible redaction (#1635).** `gateway.mcp_http.redact_patterns`
  is opt-in: up to 32 regexes of up to 512 characters, added to the built-in
  secret patterns. Useful for masking an environment-specific token format in the
  payload and the error that cross the bridge.

The `garra_agent` schema now announces the operator ceiling: `timeout_secs`
ships with `maximum` and `default` derived from `agent_max_seconds` instead of
fixed values — before, the schema said a value was valid and the refusal came
later; two contract tests now exercise `bash` over this bridge end to end
(#1626).

## Security: a dependency advisory sweep (#1630)

The October sweep closes the advisories open in the graph:

- **`rustls` 0.23.45 → 0.23.46** (RUSTSEC-2026-0346 through 0349).
- **`quinn-proto` 0.11.16 → 0.11.18** (RUSTSEC-2026-0338 through 0342).
- **`wasmtime`/`wasmtime-wasi` 49.0.2 → 49.0.3** (RUSTSEC-2026-0352 through
  0362), which includes a filesystem sandbox escape when renaming a symlink with
  a trailing slash. The MSRV floor stays **1.96**.
- **`chacha20` 0.10.0 → 0.10.2.**
- **`serde_yaml` replaced by the maintained fork `serde_norway` 0.9.42**, same
  API, to drop the archived `unsafe-libyaml` from the graph (RUSTSEC-2024-0449).

On the same supply-chain axis: each release asset now gets a **provenance
attestation** verifiable with `gh attestation verify` (#1606); the WhatsApp
bridge's `music-metadata` moves to 11.16.1, closing two Dependabot alerts; and
the eight `rust/path-injection` alerts in `uploads.rs` and `session_workspace.rs`
enter the CodeQL ledger as **false positives**, audited per entry — the path
originates from a typed `Uuid` or a hex SHA-256 digest, neither of which
expresses traversal.

## Reliability and operation

- **Uploads and recurrence workers scan again (#1611).** Both acquired a Postgres
  advisory lock on one pool connection and tried to release it on another — and
  an advisory lock is per session, so the lock survived every tick and, from the
  first one on, replicas were **permanently** blocked from scanning. Both now
  pin one connection from acquire to unlock, as `account_purge_worker` already
  did. The regression test found a second independent defect: the recurrence scan
  died before scanning over a `bigint` the Postgres function signature did not
  match.
- **Per-provider LLM timeout (#1593).** `llm.<name>.timeout_secs` overrides the
  global window for that provider — useful for a slow host without affecting the
  rest. And `0` now truly means "no timeout"; before, it became an already-expired
  sleep that failed every LLM call in milliseconds.
- **Telegram: a network failure stops being reported as "invalid token"
  (#1594).** `connect()` treated any `GetMe` error as an invalid token — under
  Termux, an SSL/DNS failure told the operator to swap a token that was fine. The
  error is now classified: the API answering fails immediately; network/IO
  retries with backoff and points at the network diagnostic.
- **The `bash` allowlist evaluates chaining segment by segment (#1592).** The
  allowlist denied ANY command with `|`, `&&`, `||` or `;` — with `git *` and
  `grep *` allowed, a `git status | grep changed` died. Separators are now split
  into segments (a quote-aware scan) and the list checks EACH one: it matches
  only when all match. Command substitution, redirection and a bare `&` still
  deny the whole command.
- **`garraia doctor mcp` validates that the MCP server actually came up (#1595).**
  The doctor checked installation but never the MCP plan: a failed startup went
  unnoticed. The new subcommand spawns each stdio server through the same code as
  the gateway boot, counts the handshake's tools, and shuts the children down;
  exit 0 when all came up, **exit 2 when one did not**, useful in CI.
- **`garra update` and `garra rollback` no longer exit 0 on failure (#1603).**
  The message now goes to stderr with the full error chain and the process exits
  with 70, matching `config check`, `doctor` and `desktop`.
- **`.env` is read from the current directory only (#1604).** `dotenvy` walked up
  the parent directories until it found a `.env`, and a file at the repo root
  injected config and credentials into every command run from a subfolder. It is
  now a plain `File::open`: the parent's `.env` no longer applies.
- **An invalid `GARRAIA_CONFIG_DIR` falls back with a warning (#1620).** A
  placeholder value from `.env.example` or an empty one now falls back to the
  default config_dir with a warning, instead of bringing down any command —
  including `garra config check` — with an opaque `EACCES`.

## Desktop: a macOS DMG and an updater with `latest.json` (#1568)

The desktop was not shippable as a packaged distribution: there was no macOS DMG,
the updater plugin was on with an empty `pubkey`, and no workflow published
`latest.json` — the update button never worked. v0.4.9 emits the `.app.tar.gz`
and the DMG, builds the DMG in a `build-macos-desktop` job, and now generates and
publishes a signed `latest.json` alongside the binaries, at the endpoint the
app's updater already queried. A PR mirror verifies the bundle on macOS before a
tag exists. The item that still blocks the distribution channel — EV/OV code
signing — stays external and tracked.

## Quality and CI

- **The gateway request-path denies `unwrap()`/`expect()` (#1569, first batch).**
  Ten modules that take external input gain `#![deny(clippy::unwrap_used,
  clippy::expect_used)]`, with both allowed in tests. The production occurrences
  became a typed error or a compile-time-checked constant.
- **A Postgres fixture unblocks 10 integration tests (#1570).** A new job brings
  up a `postgres:16.8-alpine` with the env vars the gateway bootstrap requires,
  and the ten `#[ignore]` auth, integration and voice tests — which exited
  silently without Postgres — now run for real in CI.
- **Block dependency bumps and flaky tests.** `tokio` 1.53.3, `serde_json`
  1.0.154, `uuid` 1.28, `opentelemetry` 0.33.1, Tauri 2.12.2 and plugins,
  Playwright 1.64, and the Dockerfile on `rust:1.99-slim`; plus two flaky CI
  tests fixed.

## New in the CLI: `garra uninstall`

`garra uninstall` removes the CLI from the machine — binary, `garra` alias,
Termux wrappers and `.old`/`.new` backups. `--purge` includes config, data and
the legacy `~/.garraia`; `--all-binaries` scans `PATH`. It refuses with exit 78
when the daemon belongs to a systemd unit, and with exit 64 in a pipe without
`--yes`.

## Documents: cleanup and contribution

- **Legal and pricing drafts leave the public repo (#1616).** An exposure audit
  found documents marked "DRAFT — DO NOT PUBLISH" and the internal financial
  model on `main`. The files stop being versioned (now covered by `.gitignore`);
  final reviewed versions publish on garraia.org. Git history and existing forks
  are unaffected.
- **Contact domain corrected to `garraia.org` (#1574).** Some paths pointed at
  `garraia.cloud`, which does not resolve. The project's real domain is
  `garraia.org`, and the security email is `security@garraia.org`.
- **End-user contribution guide (#1577).** How to run the pre-flight before
  opening an issue, which template to use, where to ask for help, and why a
  vulnerability never goes in a public issue. Linked from `SUPPORT.md`.
- **`channels.md` stops saying `whatsapp_linked` does not exist in `config.yml`
  (#1605).** The linked-device channel has a documented entry and `config check`
  validation; the old text pointed at an absence that was no longer there.

---

## How to update

```bash
garra update            # CLI; keeps the previous version and supports `garra rollback`
```

Fresh installs go through the [usual installer](Instalacao-e-Primeiros-Passos).

## Known limits

- Email change (`PATCH /v1/me`) and closing a deletion with blobs depend on
  operator steps: email sending and a wired object store are not set up by
  default.
- Windows installers and desktop bundles are still **unsigned** (Gatekeeper on
  macOS, SmartScreen on Windows); the distribution channel's EV/OV code signing
  stays external.
- `garra_ask`/`garra_agent` delegation over MCP HTTP is **off by default**;
  nothing changes until the operator turns it on and declares an orchestrator.
