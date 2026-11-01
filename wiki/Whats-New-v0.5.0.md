# What's new in v0.5.0

> 🇧🇷 [Versão em português](Novidades-v0.5.0) · 📋 [Full CHANGELOG](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md) · 📦 [Download](https://github.com/michelbr84/GarraRUST/releases/tag/v0.5.0)

Stable release from November 1, 2026 — replaces v0.4.10 as the `latest` release.
Validated as rc1 (10/30) and rc2 (10/30); fixes found while validating the
RCs are included.

One front this time: **the chat turn is back under your control**. The
120-second wall-clock ceiling that cut long answers in half is out of the
way by default — and if you need a ceiling back, it is now inherited by the
whole CLI, not just the gateway.

## Behavior changes

- **Chat runs without a wall-clock ceiling by default (#1751).** `garra
  chat` no longer stops at 120 s; `--timeout-secs` is opt-in and text already
  streamed stays in the history marked as partial. `0` = unlimited is the
  interactive default.
- **MSRV raised to 1.97 (#1714, #1750).** `wasmtime` 50 (without the `cache`
  feature, removing the vulnerable zstd from the lockfile) and the new
  binary require Rust 1.97+. CI, scripts and docs updated.
- **`/model` accepts OpenRouter presets and routing variants (#1752,
  #1764).** `@preset/<slug>` and the routing suffixes `:floor`/`:nitro`/
  `:online`/`:exacto` are validated with a 1-token probe; `--model` on the
  command line takes the same string.
- **The API tells you who answered (#1733).** `provider`/`model`/`fallback`
  in the body plus `X-AI-Provider`/`X-AI-Model` headers; an invalid model is
  a sanitized 4xx instead of a 500 with the raw body (#1726).
- **Honest fallback and health (#1735, #1743).** `fallback_providers` kicks
  in when the primary refuses the credential, and `/api/health` no longer
  reports green with a rejected key.
- **Web TTS plays again (#1754)** and `/clear` wipes the persisted history
  (#1723).
- **First-class Windows (#1697, #1722, #1720, #1737).** The `filesystem`
  MCP, `file_jail`, `garraia rollback` and the voice diagnostics were fixed;
  the unit suite also runs on Windows and macOS in CI.

## Fixes from the validation train (rc2 → stable)

- **Empty 200 joins the retry/fallback circuit (#1776):** an `Ok` with no
  text and no tool call retries once, falls back, and records one breaker
  failure per turn.
- **The CLI inherits config timeouts (#1778):** providers built by the CLI
  now honor `config.timeouts` (before: bare client, chat against Ollama/
  llama.cpp ran unbounded).
- **3xx gets its own diagnostic (#1779):** a redirect no longer lands in the
  4xx/5xx branch with an empty body; the `Location` is quoted in the
  message, sanitized. Redirect policy untouched (rule 14).
- **`code_review` with its own ceiling (#1780):** the tool's internal LLM
  call gets its own timeout (180 s) and the `review` mode rises to 120 s —
  reviews stop dying before the provider answers.
- **Gate reclassifies PowerShell (#1781):** `[Environment]::OSVersion` is no
  longer a false positive of the bare `environ` substring, and spelled-out
  `Invoke-WebRequest`/`Invoke-RestMethod`/`Invoke-Expression` (plus aliases
  `iwr`/`irm`/`iex`) no longer slip through.
- **Honest `garra memory stats` (#1782):** it no longer promises a fix the
  reindex cannot do.
- **`garra update` no longer offers a downgrade (#1772)** and **wasmtime
  50.0.1 closes four advisories from 50.0.0 (#1774)** — both already in rc2.

## Install

Windows: `irm https://garraia.org/install.ps1 | iex` · Linux/macOS:
`curl -fsSL https://garraia.org/install.sh | sh` · Docker: `docker pull
ghcr.io/michelbr84/garraia:0.5.0` — or `garra update` from 0.4.10.
