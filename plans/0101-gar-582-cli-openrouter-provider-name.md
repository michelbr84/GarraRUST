# Plan 0101 — GAR-582: name OpenRouter provider explicitly in chat.rs

**Status:** Em execução
**Autor:** Claude Opus 4.7 (sessão interativa 2026-05-11, America/New_York)
**Data:** 2026-05-11 (America/New_York)
**Issue:** [GAR-582](https://linear.app/chatgpt25/issue/GAR-582)
**Branch:** `routine/202605110956-gar-581-cli-openrouter-provider-name`
**Epic:** `epic:cli`
**Parent:** —
**Builds on:** GAR-576 (PR #263), GAR-579 (PR #267)

> Branch name retains the `gar-581` prefix because the issue was opened
> as GAR-581 in this session's prompt; Linear then assigned the next
> available number (`GAR-582`) since 580/581 were already taken. The
> plan + commit reference the actual ID `GAR-582`.

---

## §1 Goal

Eliminate the runtime WARN observed during the GAR-579 smoke test:

```
WARN garraia_agents::runtime: Provider 'openrouter' not found, falling back to default
```

Trivial 3-LOC fix in `crates/garraia-cli/src/chat.rs` — chain
`.with_name("openrouter")` onto every `OpenAiProvider::new(...)` whose
`base_url` points at OpenRouter. Same pattern already used for LM
Studio at `chat.rs:174` (`.with_name("lmstudio")`).

---

## §2 Cause

`OpenAiProvider::new(_, _, _)` defaults its internal `name()` to
`"openai"`. When `AgentRuntime` later resolves a provider by name —
because we registered it and want to address it by the same string —
the lookup misses for `"openrouter"` and the runtime emits the WARN +
silently uses its default provider. The HTTP request still routes
correctly (via `base_url`), so the call works; only the log output is
noisy and the internal lookup is wrong.

Three call sites:

1. `chat::detect_provider` step 4 (autodetect openrouter, ~line 234).
2. `chat::select_explicit_provider` `"openrouter"` arm (~line 397, GAR-579).
3. `chat::try_build_default_provider` `"openrouter"` arm (~line 329, GAR-576).

---

## §3 Change

```rust
let op = OpenAiProvider::new(
    &key,
    Some(model.clone()),
    Some("https://openrouter.ai/api/v1".to_string()),
)
.with_name("openrouter");
```

Three identical chains. No other changes.

---

## §4 Design invariants

1. **No behavior change to HTTP path** — `base_url` already correct, request still goes to `openrouter.ai/api/v1/chat/completions`.
2. **No new dependency**.
3. **No change to LM Studio / OpenAI / Anthropic / Ollama paths** — they already either have correct naming (`with_name("lmstudio")`) or use a default that matches their kind.
4. **No new tests required** — smoke validation is the regression check. Existing 53 unit tests must pass unchanged.
5. **Single-commit scope** — `fix(cli): …`, NOT breaking.

---

## §5 Verification

```bash
cargo fmt --all -- --check
cargo clippy --workspace --exclude garraia-desktop --all-targets -- -D warnings
cargo test -p garraia --bin garra
cargo build --release --bin garra
```

Smoke (cheap, `openrouter/free` only — `openrouter/auto` NOT executed):

```bash
GARRAIA_CONFIG_DIR="$HOME/.garraia" ./target/release/garra.exe ask \
  --provider openrouter --model openrouter/free \
  --json --timeout-secs 30 "Responda apenas: GAR-ASK-OK" \
  >stdout.txt 2>stderr.txt
```

Acceptance:
- `grep -F "Provider 'openrouter' not found" stderr.txt` → **0 matches** (WARN gone).
- `stdout.txt` is a single valid line JSON envelope `garra.ask.v1`.
- No api-key fingerprint in stdout/stderr.
- If network/TLS still blocks success path, error path JSON envelope remains valid.

---

## §6 Out of scope (follow-ups)

- MCP wrapper over `garra ask` (next slice, GAR-580).
- `RedactingWriter` extension to cover provider error payloads.
- Other provider naming inconsistencies (LM Studio explicit `--provider openai --url …` path; custom OpenAI-compat backends in default_provider).
- `.env` / `dotenvy` / `GARRAIA_CONFIG_DIR` operator-side fixes.
- Automatic `openrouter/free → openrouter/auto` fallback.
