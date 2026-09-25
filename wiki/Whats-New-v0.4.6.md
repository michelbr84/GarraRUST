# What's New in v0.4.6

> 🇧🇷 [Versão em português](Novidades-v0.4.6) · 📋 [Full CHANGELOG](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md) · 📦 [Download](https://github.com/michelbr84/GarraRUST/releases/tag/v0.4.6)

A **stabilization** release after v0.4.5: no new surface, just what a clean
0.4.5 install exposed. CI works again without depending on a registry for
MinIO. A WhatsApp session without a project gets a safe default workspace,
**isolated per session**, instead of `NoRoots`. A forged `X-Session-Id` can no
longer reach someone else's conversation. Personal WhatsApp gets the missing
administration commands. And the release runbook gets a manual dogfood gate,
because v0.4.4 and v0.4.5 were green in CI and broken on the real path.

---

## CI without a registry for MinIO (#1458)

On 2026-09-24 `quay.io` started requiring a login for every `minio/minio` tag,
after Docker Hub had already removed the repository (#1230) and `dl.min.io`
started answering 410 for the binaries. The required `Clippy Linting` check
(which runs the `storage-s3` suite against a real MinIO) failed on every PR.

What is still public is the **source**. `scripts/ci/build-minio-image.sh`
builds `minio` from the pinned upstream tag (`RELEASE.2025-02-28T09-55-16Z`),
refuses if the tag no longer points at the expected commit, packages the
binary on `debian:bookworm-slim` and tags the image with the exact `name:tag`
the testcontainer asks for — testcontainers only pulls when the image is not
present locally. The binary is cached in Actions per tag, so only the first
run after a bump pays the build. The dev `docker-compose.minio.yml` uses the
same script.

## Default workspace per session (#1378, #1449)

A freshly linked WhatsApp session has no project. With `agent.file_roots`
empty — the default of every install — the file jail had **no roots**, and no
roots means deny everything: `file_read`, `file_write` and `list_dir` were
registered and every call came back `NoRoots`.

When nothing is declared, the effective root is now
**`<data_dir>/workspace/<session>`** — one subdirectory per session (named by
the SHA-256 of the `session_id`, never the raw id), inside the directory ADR
0024 already reserves for Garra itself. Never `/`, never `$HOME`, never the
`pod_root`. The independent security review (#1449) is why it is "per
session": the first version used one shared root, and one WhatsApp contact
could read what another wrote. Declared `agent.file_roots` /
`GARRAIA_FILE_ROOTS` still win on their own, without per-session scoping.
`/api/diagnostics` shows the source in `files.workspace`.

## A forged `X-Session-Id` does not read someone else's chat (#1462)

`POST /v1/chat/completions` accepted `X-Session-Id` verbatim and
`POST /api/sessions/{id}/messages` accepted the id in the path, neither
checking whose session it was. Channel ids are guessable by construction
(`whatsapp-linked-<number>`, `telegram-<chat>`), so a forged id loaded the
victim's summary and up to 100 turns into the request — and wrote the
attacker's turn into her conversation.

By id, only sessions of the **operator's local surfaces** (`api`, `vscode`,
`web`, `parrot`) are reachable now. A channel or mobile session answers
`404 session not found`, without confirming it exists, and is left untouched.
`GET /api/sessions/{id}/history`, which the Web Console uses to export any
session, stays as is — a product decision recorded on the issue.

## Personal WhatsApp: administer without editing YAML

- `garraia whatsapp users [--json]` lists who may talk to Garra, by role and
  last four digits (#1393).
- `garraia whatsapp remove <number>` revokes; removing an owner asks first (#1394).
- `garraia whatsapp owner|unowner <number>` promotes and demotes; `owner` only
  in `isolated-pod` (#1395).
- `garraia whatsapp allow '*'` explains that "allow everyone" **does not
  exist** on this channel instead of looking like a typo (#1389).
- `garraia whatsapp link` ends by showing the **effective access** — channel,
  counts and identities — so nobody leaves the wizard without seeing the gate
  (#1429, partial).
- `garra init` offers personal WhatsApp next to Telegram (#1430).

## More honest diagnostics and console

- `/api/diagnostics` separates **"never configured"** (`not_configured`,
  `disabled`) from **"broken"** (`error`): missing TLS, no `.env`, Telegram
  without a token or WhatsApp without a linked device no longer light up
  yellow on a fresh install (#1437).
- The Web Console shows the **execution profile** as a fixed header badge (#1410).
- `garra_status` explains what `withheld` means: a field withheld by policy is
  not a missing capability (#1382, #1387).
- `repo_search` fails fast without an active repository instead of retrying (#1380).

## Security

- **MCP servers no longer become slash commands** (#1386): automatic
  registration exposed every MCP tool as a `/command`, outside the modes'
  `ToolGate`.
- The #1462 finding above.

## Process and tooling

- **Manual dogfood gate** before the tag, in `docs/releasing.md` §1.5 (#1439):
  the D1–D9 matrix (clean install on both installers, WhatsApp end to end,
  restart without QR, `update`/`rollback`, desktop bundles, per-session
  isolation, clean diagnostics), run against the candidate, with date, OS and
  who ran it in the release PR body. A row without a date means no release.
- The agents' `pre-tool-use` hook stops blocking `rm -rf /tmp/…` and
  `rm -rf ./x`: the block is anchored on the **target** (root, home, cwd,
  parent), not on the prefix (#1453).
- `scripts/setup-toolchain.sh` pins Rust 1.95 locally through `rustup
  override`, without a `rust-toolchain.toml` (which breaks CI cross-compiles)
  (#1452).

## Upgrading from v0.4.5

```bash
garraia update
```

No format changes: `config.yml`, the WhatsApp `session.enc`, `allow`/`owners`
and history all keep working. Two visible behaviour changes:

- Remote sessions without a project now get files under
  `<data_dir>/workspace/<session>` — a new, empty folder per session. Anyone
  who already declared `agent.file_roots` sees no difference.
- Clients that used `X-Session-Id` to **continue a channel session** through
  the compat API (for example, resuming a Telegram chat from VS Code) now get
  `404`. That is exactly the path #1462 closes; the conversation remains
  available through its own channel and the Web Console.

## Known limits

- Reading history by id (`GET /api/sessions/{id}/history`) remains open to
  whoever reaches the port (loopback, or `gateway.api_key` on an exposed
  bind). Closing it breaks the Web Console's *Export* for channel sessions;
  the decision is recorded on #1462.
- On a clean install `/api/diagnostics` still shows `warning` on `tools.bash`
  (off by design in the `standard` profile) and `runtime.channels` (no
  channels) — two non-actionable warnings, tracked in #1471.
- The WhatsApp QR is still **in the terminal**: putting it in the desktop app
  is plan 0363, not this release.
