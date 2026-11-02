# What's new in v0.5.1

> 🇧🇷 [Versão em português](Novidades-v0.5.1) · 📋 [Full CHANGELOG](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md) · 📦 [Download](https://github.com/michelbr84/GarraRUST/releases/tag/v0.5.1)

Stable release from November 2, 2026 — replaces v0.5.0 as the `latest` release.

One front this time: **GarraIA uses your ChatGPT plan**, with no API key,
through OpenAI's official flow for open-source apps. Alongside: three
security fixes and the definitive signing key for the Android APK.

## Your ChatGPT plan, no API key (#1796)

- **Continuar com ChatGPT (Continue with ChatGPT).** On the **Providers**
  page (port 3888), the ChatGPT card opens OpenAI's sign-in in your browser; from the command line,
  `garra chatgpt login --padrao`. You authorize plan usage and the card
  switches to **Conectado** (Connected).
- **Official path only.** OAuth with PKCE S256, dynamic app registration, the
  `chatgpt.tokens.use.direct` scope and an ID token verified with RS256
  against OpenAI's published keys. Inference goes to the public
  `POST /v1/responses` endpoint with `store: false`. GarraIA does not use
  private ChatGPT endpoints, cannot read your conversations and does not
  reuse other tools' credentials.
- **Protected tokens.** Stored under `credentials/openai-codex` with `0600`
  permissions, never logged; refresh is serialized across gateway, CLI and
  MCP server (the refresh token rotates) and **Sair** (Sign out) revokes the session.
- **GPT-6.1 Sol with High reasoning** by default. The card offers the Minimal,
  Low, Medium, High, Extra High, Max and Ultra levels and the Thinking, Fast
  and Ultrafast speeds; anything the model or account does not offer is
  disabled, with the reason (Ultra only exists in Codex). **Testar conexão** (Test connection)
  runs a real inference and shows the tier OpenAI actually served.
- **Plan limit falls back.** When OpenAI reports the usage limit is reached,
  GarraIA does not retry through the plan and goes straight to the first
  provider in `agent.fallback_providers` (OpenRouter, for example). Making
  ChatGPT the default turns the previous default provider into the first
  fallback.

Guide (Portuguese): [Use your ChatGPT plan](https://github.com/michelbr84/GarraRUST/blob/main/docs/src/guides/connect-chatgpt.md).

## Security

- **Voice: one user no longer hears another user's answer (#1754).** Hibiki
  and LM Studio passed audio through a temp file with the same name for every
  call; bytes now come straight from the HTTP response. Whisper and Hibiki get
  a 120 s timeout.
- **Command gate recognizes Windows executables (#1791).** `curl.exe`,
  `C:\...\git.exe`, `CURL.EXE` and quoted paths get the same check as `curl`
  and `git`; the same applies to `sudo`, pipe-to-shell and `bash -c` with an
  absolute path. `kubectl exec` now asks for confirmation, like `docker exec`.
- **Dependencies (#1795).** `serialize-javascript` 7.1.4 on the docs site
  (GHSA-m5mm-p8h7-m29v) and `clipboard-win` 5.4.3 in the Windows CLI (5.4.1
  was yanked).

## Garra Mobile (Android): definitive signing key

The v0.5.1 APK is the first signed with the project's **definitive key**;
v0.5.0 shipped with the CI runner's debug key. Android will not install an
update signed with a different key, so **uninstall v0.5.0 once** and install
v0.5.1. From then on, updates install over the existing app.

## How to install

Windows: `irm https://garraia.org/install.ps1 | iex` · Linux/macOS:
`curl -fsSL https://garraia.org/install.sh | sh` · Docker: `docker pull
ghcr.io/michelbr84/garraia:0.5.1` — and `garra update` from 0.5.0.
