# What's New in v0.4.10

> 🇧🇷 [Versão em português](Novidades-v0.4.10) · 📋 [Full CHANGELOG](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md) · 📦 [Download](https://github.com/michelbr84/GarraRUST/releases/tag/v0.4.10)

A stable release of October 25, 2026 — replaces v0.4.9 as the `latest`
release. Validated as rc1 (10/23) and rc2 (10/24); the fixes found during the
rc1 validation are all in here.

This version has three fronts. The first is the **Garra Desktop** coming out
of its shell: the app gains a **First-Run Assistant** — a five-screen wizard
in pt-BR that installs, pairs WhatsApp by scanning a QR code and gets the user
talking without ever opening a terminal — and the installers now ship a
private Node runtime (downloaded with GPG + SHA-256 verification), with
installer smoke tests on all three platforms running in CI.

The second is the **product learning to explain itself**: the binary now
ships a versioned feature catalog (the `garra_capabilities` tool), the gateway
gains a native read-only diagnostic (`garra_diagnostics`), and the project
publishes a **Docusaurus documentation site** on GitHub Pages, generated from
the markdown that already lives in the repository.

The third points toward the **Garra Cloud**: the foundation of the billing
schema lands (plans, subscriptions and invoices per tenant, ADR 0026 — no
pricing is defined in this MIT repository) and the D1/D3a/D9 dogfood gate
becomes a CI job (`dogfood-linux.yml`).

Rounding it off: a wide band of fixes — installers no longer fall onto a
prerelease, voice with the official Chatterbox, a desktop that leaves no
orphan processes, Docker builds with the `third_party/` forks, honest
diagnostics on a fresh install — and the security batch: revocation of the
privileges migration 007 leaked, `wasmtime` 49.0.4 (a critical RUSTSEC), and
`jsonwebtoken` switching its crypto backend to `aws_lc_rs`, which drops twenty
crates from the RustCrypto chain in the lockfile.

---

## The desktop installs, pairs and chats without a terminal

- **First-Run Assistant (#1636, slice 3).** On first launch, the Garra
  Desktop opens a five-screen wizard in pt-BR — Welcome → Mode → QR →
  Personality → Done. The QR is rasterized on a canvas from the NDJSON matrix
  emitted by the `garraia whatsapp link --machine` sidecar (slice 1) — the
  raw payload (the credential) never reaches the webview. The "own number" vs
  "dedicated number" choice persists in `onboarding.json` and stays
  configurable later under Settings → WhatsApp.
- **Mode and personality mapping into the config (#1636).** "My own number"
  applies `whatsapp access open --yes`, only with the user's explicit
  confirmation; "Dedicated number" applies `whatsapp access restricted` **and**
  `whatsapp allow <personal number>` (the personal number must be
  authorized, otherwise the gate would hold zero allowees and silently drop
  the owner). No security default is loosened silently; the persona becomes
  `agent.system_prompt`, and the wizard only writes `concluido` — and
  restarts the gateway — after every step exits 0.
- **Hardened webview (#1636).** A restrictive CSP in `tauri.conf.json` and all
  desktop JS moved to external files, no inline scripts; the wizard gets its
  own minimal capability, outside the broad `default`.
- **An honest "Done" screen (#1700).** With no LLM provider in the config,
  the assistant warns before the first unanswered "hi", with the path to
  `garraia init` (and the Windows path) — the key never crosses the webview.
  Choosing the provider inside the assistant is deferred to 0.5.0.
- **Private Node runtime bundled in the installers (#1636, slice C).** The
  Windows MSI/NSIS, Linux `.deb`/AppImage and macOS DMG/`.app` now carry Node
  v24.22.1 LTS and the pre-built WhatsApp bridge — no Node/npm to install and
  no dependencies to download on first run. `docs/desktop.md` documents the
  per-format layout, uninstall and what remains (`~/.garraia/whatsapp/` is
  preserved).
- **Clean-environment installer smoke in CI (#1636).** Per-platform jobs
  extract the tree into a path with a space, hide every global Node from the
  `PATH` and require `node_origin == private` + `self_test == ok` from the
  installed sidecar; sibling jobs cover app launch (`/api/health` within
  15s), upgrade (v0.4.9 → new, session survives) and uninstall (user data
  preserved).

## The bridge's private Node, verified end to end

- **Verified build (#1636, slice B).** `build-node-runtime.sh` downloads the
  OFFICIAL Node artifact, proves provenance via GPG against the release
  team's keys vendored in the repo (fingerprint allowlist, isolated
  GNUPGHOME) and integrity via SHA-256, builds the bridge with the tarball's
  own npm, and generates `THIRD_PARTY_NOTICES.txt` + `manifest.json`. The
  `node-runtime.yml` workflow also proves the rejection of tampered artifacts
  or SHASUMS.
- **Node by explicit path (#1636).** The order is `GARRAIA_NODE_BIN` → the
  private Node next to the running executable → `PATH`, always by absolute
  path, without touching the process `PATH` or replacing existing
  installations; CLI and gateway share the resolver and record the origin
  (`env`/`private`/`path`).
- **`garra whatsapp self-test [--json]` (#1636).** Imports
  Baileys/pino/sharp with no socket and no pairing, and reports the Node
  origin, path and versions — the smoke the installers run without a global
  Node.
- **NDJSON pairing: `garra whatsapp link --machine` (#1636).** One line per
  event (`status`, `qr`, `error`, `log`), the QR as a `0`/`1` matrix from the
  same engine as the terminal, the raw payload never printed, errors and exit
  codes identical to the interactive `link`. Built for the desktop app to
  consume as a sidecar (see `docs/whatsapp.md`, "Machine mode").
- **Robustness details.** `npm` is only required when there are dependencies
  to install; the pre-built bridge tree is seeded when it matches the
  `package-lock.json` embedded in the binary; and the onboarding state
  machine lives in `garraia-desktop-core` — pure logic, CI-tested, no Tauri.

## The product explains itself

- **Versioned feature catalog embedded in the binary (#1670).** The Garra
  answers what it can do from the
  `crates/garraia-gateway/assets/catalogo-funcionalidades.json` manifest,
  embedded via `include_str!` — it works even on a clean install, without a
  repo clone. Each feature carries a short description, prerequisites,
  limitations and state (`available`/`configurable`/`optional`), with a
  feature/config marker for what depends on compilation or a switch. The new
  `garra_capabilities` tool (filters by `name`/`category`; without filters it
  returns counts only) and a `catalog` section in `garra_status`. Under a
  restricted turn it returns only what the permission allows.
- **Native, safe diagnostics (`garra_diagnostics`, #1669).** A structured,
  read-only report of the installation itself — version, gateway, channels,
  MCP servers, tools, validated effective config, credential presence and
  ORIGIN (vault / config / env, never values), a redacted sample of the log —
  with a closed state vocabulary
  (`ok`/`error`/`not_configured`/`blocked`/`degraded`/`unsupported`), a file
  jail for log reads, and access gated by operator session + turn,
  fail-closed.
- **Docusaurus documentation site on GitHub Pages (#1639).** The content is
  the markdown that already exists in the repository (`README`, `docs/**`,
  `wiki/**`, `CHANGELOG`, ...), synced by a pure-Node script — a single
  source of truth, nothing rewritten by hand. Portuguese as the default
  language, partial English, offline search, light/dark theme, and a publish
  on every push to `main` (https://michelbr84.github.io/GarraRUST/). It does
  not touch `ci.yml` nor the garraia.org site.

## Toward the Garra Cloud

- **Billing schema foundation: plan/subscription/invoice (#1652, ADR 0026).**
  `plans` is a global catalog seeded only with the `free` plan at no price
  (pricing is not set in this MIT repository); `subscriptions` and `invoices`
  are per-tenant under FORCE RLS (fail-closed), money as integer cents +
  ISO 4217, the invoice as an immutable ledger, provider payment IDs optional
  and UNIQUE, never card PII. Every group without a live subscription
  resolves to Free, so a new tenant is born Free without touching anyone.
  Schema only: the Stripe/Pix webhook, entitlements, metering and the signup
  page are the next issues.
- **The D1/D3a/D9 dogfood gate runs in CI.** `dogfood-linux.yml`
  (`workflow_dispatch`) installs the version-pinned, SHA-256-verified Ollama,
  starts the server, pulls the model and runs
  `scripts/dogfood/linux-clean-install.sh --source local` against the
  candidate; the job fails if the summary says FALHOU. The lines that need
  two real phones remain manual by design.

## WhatsApp cadence and reliability

- **Cross-turn cadence guard per chat (#1650 + #1658).** Past 20 admitted
  turns in 60s on the same chat (configurable ceiling,
  `channels.whatsapp_linked.max_turnos_per_minute`), the turn is cut with the
  `possivel loop entre agentes` log; the cut turn does not count against the
  window, and the chat recovers on its own once the other side goes quiet.
  Growing backoff per burst (60s → 120s → 240s, 8min ceiling, resets after
  2min of silence) and `turnos_ultima_hora` in `garra_diagnostics`.

## Fixes worth the release

- **Installers no longer fall onto a prerelease through the unpinned path.**
  The `install.sh`/`install.ps1` fallback now skips `prerelease: true` (in
  addition to `draft: true`) and picks the most recent stable, classifying
  each release by `tag_name`/`draft`/`prerelease`. To install an RC on
  purpose, pin `GARRAIA_VERSION=vX.Y.Z-rcN`.
- **Voice with the official Chatterbox works again.** The client discovers
  the signature of `generate_tts_audio` (the official app binds 7 inputs, not
  the 4 the client used to send — Gradio 6.x rejected the short call) and
  builds the call from the server's defaults, caching the signature; an SSE
  `event: error` becomes a real error with the server's message, and
  `/api/tts` and the Telegram voice channel log the reason before falling
  back to text.
- **Docker builds again with the `third_party/` forks (#1694).** The
  post-v0.4.9 `[patch.crates-io]` (`tauri-plugin-updater`,
  `utoipa-swagger-ui`) broke `cargo chef` — the rc1 Deploy published no
  image. The Dockerfile now copies `third_party/`, a test pins the rule on
  every PR, and `docker-build.yml` builds the image and answers `/ping`
  whenever Dockerfile/Cargo/third_party change.
- **Honest `/api/diagnostics` on a fresh install (#1695, #1696, #1698).**
  Missing configuration becomes `not_configured` (neutral) instead of
  `warning`; with no `npx` on the `PATH` the boot no longer provisions an MCP
  `filesystem` that could never start (and says how to enable it); and on
  Windows both sides of the path comparison go through the same canonical
  form, so the workspace is no longer reported as "outside the data_dir".
  Configured-and-broken stays yellow.
- **No orphans when the parent dies abruptly (#1636).** On Windows, the
  bridge and sidecar join a `KILL_ON_JOB_CLOSE` Job Object; on Unix, their
  own process group + `kill(-pgid)` and `garra start --exit-with-parent`
  watching the app's PID (PID-reuse proof via `getppid()`); on all platforms,
  EOF on stdin shuts down. Orphan tests on ubuntu/windows/macos in CI,
  including `TerminateProcess`.
- **A band of point fixes.** The boot probes Whisper before declaring STT
  ready (`STT ready` only when the server answers); the Web Console dashboard
  shows the real bind instead of a fixed "0.0.0.0"; the provider selector
  shows the real instances (`llm.<key>`), not just fixed types; `garraia
  doctor` warns when an older binary masks the new one on the `PATH`; the
  WhatsApp reconnect log reports `reason`/`reason_code`; `find_executable`
  requires the executable bit on Unix; the non-streaming autonomous path no
  longer publishes `[no textual response provided by the model]`; and the
  autoprovisioned `@modelcontextprotocol/server-filesystem` is pinned to
  `@2.0.0` (upstream deprecated the date-based series).

## Security

- **Revoking the privileges migration 007 leaked (migration 036,
  forward-only).** Migration 007's ALTER DEFAULT PRIVILEGES granted full CRUD
  to every table created after it; nine tables carried excess
  UPDATE/DELETE on the `garraia_app` role. The revocation is selective —
  SELECT/INSERT and each table's legitimate mutation privilege remain — and
  the LGPD cross-tenant deletion keeps running in dedicated `SECURITY
  DEFINER` functions, never through the app pool. The heavyweight case is
  `account_deletion_requests`: without the revoke, a handler could mark a
  request as completed without the purge.
- **`jsonwebtoken` switches its crypto backend to `aws_lc_rs` (#1701).**
  RUSTSEC-2026-0387 has no fix in `rust_crypto` (a monolithic feature) and
  the code only uses HS256/RS256 — the entire RustCrypto EC/RSA chain (20
  crates, including the vulnerable `crypto-bigint`, only fixed in 0.7.6)
  leaves the lockfile with no exception in `deny.toml`/`audit.toml`. The
  documented trade-off: `aws-lc-sys` compiles native C (cmake + perl).
- **The wasmtime family at 49.0.4** closes RUSTSEC-2026-0379 (critical) and
  RUSTSEC-2026-0378 in the plugin sandbox; `rustls-webpki` 0.103.16 and
  `webpki-roots`/`webpki-root-certs` 1.0.10 close
  RUSTSEC-2026-0381/0382/0383; `ctutils` 0.4.5 closes RUSTSEC-2026-0385
  (non-constant-time `u128` masking and comparisons) — it enters via
  `digest` 0.11, used by `argon2`/`blake2`/`hmac`.
- **Fail-closed jail for filesystem MCP tools (#1482)** — a tool from the
  filesystem server that the confinement does not map (a new or renamed tool
  from a future package version) is refused instead of passed through
  unchecked.
- **Per-orchestrator jail and workdir for the MCP HTTP `garra_agent`
  (#1626).** Each orchestrator now gets its own writable subtree under
  `<data_dir>/workspace`, derived from its identity by digest — the single
  effective root of the file tools and the sandbox workdir. The jail used to
  be global: what one orchestrator wrote, the others could read.
