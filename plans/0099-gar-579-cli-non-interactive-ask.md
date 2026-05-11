# Plan 0099 — GAR-579: CLI `garra ask` non-interactive command

**Status:** Em execução
**Autor:** Claude Opus 4.7 (sessão interativa 2026-05-11, America/New_York)
**Data:** 2026-05-11 (America/New_York)
**Issue:** [GAR-579](https://linear.app/chatgpt25/issue/GAR-579/cli-add-non-interactive-garra-ask-command)
**Branch:** `routine/202605110709-gar-579-cli-non-interactive-ask`
**Epic:** `epic:cli`
**Parent:** —
**Builds on:** [GAR-576](https://linear.app/chatgpt25/issue/GAR-576) / [PR #263](https://github.com/michelbr84/GarraRUST/pull/263) — provider/model resolution

---

## §1 Goal

Add a non-interactive `garra ask` subcommand suitable for Claude Code, CI, hooks, scripts, and future MCP integration. The interactive `garra chat` REPL stays untouched; `ask` is a separate channel with a parseable contract.

Scope cuts approved by user 2026-05-11 (4 of 4):

1. **No `--stream`** — JSON one-shot covers all consumers. Streaming → PR v2.
2. **No `--enable-tools`** — `ask` is LLM-only. Tools (bash/file_read/file_write/git_diff) need allowlist + audit + security-auditor in a dedicated PR.
3. **No `--system-prompt-file`** — only `--system-prompt <STR>` inline.
4. **Minimum visibility surface** — expose at most 3 helpers from `chat.rs` as `pub(crate)`; if that grows, prefer a small `provider_resolution.rs` module (not in this PR).

---

## §2 Surface

| # | Comando | stdout | Exit |
|---|---------|--------|------|
| A | `garra ask "ping"` | resposta texto (sem banner, sem ANSI) | 0 |
| B | `garra ask --json "ping"` | linha JSON `garra.ask.v1` | 0 |
| C | `garra ask --provider openrouter "ping"` | usa modelo do config (GAR-576) | 0 |
| D | `garra ask --provider openrouter --model openrouter/auto "ping"` | usa auto | 0 |
| E | `echo "ping" \| garra ask` | lê stdin (cap 64 KiB) | 0 |
| F | `garra ask ""` (arg vazio) | erro claro, sem stack trace | 2 |
| G | `garra ask --timeout-secs 1 "msg"` (lento) | erro timeout | 124 |
| H | provider auth falha | erro JSON com `kind="provider_error"` | 69 |

Flags aceitas: `--json`, `--provider/-p`, `--model/-m`, `--url/-u`, `--timeout-secs` (default 60, range [1, 600]), `--system-prompt <STR>`.

Defaults invariantes:
- Tools **NUNCA** registrados em `ask` (zero `AgentRuntime::register_tool` no path do ask).
- Banner **NUNCA** impresso.
- ANSI escapes **NUNCA** em stdout.
- System prompt mínimo (sem scan de diretório, sem persona).
- `--model` tem precedência absoluta sobre config.

---

## §3 Design invariants

1. **Reuso de GAR-576**. Provider/model resolution já implementado; expor o mínimo via `pub(crate)` e chamar de `ask.rs`.
2. **Sem nova dep**. Nenhum item em `[dependencies]` ou `[dev-dependencies]`.
3. **Pure tests**. `ask::tests` não toca rede, env-var-mutation, ou filesystem (exceto via `Cursor` em memória).
4. **Sem fingerprint de secret em erro**. Response body de erro do provider passa por sanitização básica (`sk-`, `sk-or-v1-` → redacted) antes de virar JSON.
5. **Stdin cap**. 64 KiB hard limit para evitar OOM por pipe malicioso/acidental.
6. **Timeout cap**. `--timeout-secs` clamp [1, 600] via clap range.
7. **Single-PR scope**. Nada de MCP, tools, stream, file-based prompt, RedactingWriter extension.

---

## §4 Code changes

### `crates/garraia-cli/src/chat.rs` — visibility flip + extração mínima

Promover de privado para `pub(crate)` (alvo: 3 itens, não mais):

- `detect_provider` — já documentado, usado pelo autodetect path do ask.
- `select_explicit_provider` — nova função extraída do match `match p.as_str()` em `run_chat` (4 arms ollama/anthropic/openai/openrouter). Retorna `Result<(String, String, Arc<dyn LlmProvider>)>`.
- `build_url_override_provider` — nova função extraída do bloco `if let Some(url) = url_override` em `run_chat` / `detect_provider`. Retorna `(String, String, Arc<dyn LlmProvider>)`.

`run_chat` é refatorado para chamar essas funções em vez de duplicar lógica. Comportamento preservado (testes GAR-576 + smoke verificam).

### `crates/garraia-cli/src/ask.rs` — novo módulo

```rust
pub async fn run_ask(
    config: AppConfig,
    message: Option<String>,
    provider_override: Option<String>,
    model_override: Option<String>,
    url_override: Option<String>,
    json: bool,
    timeout_secs: u64,
    system_prompt: Option<String>,
) -> Result<i32>;  // returns exit code; caller std::process::exit's

#[derive(Debug)]
pub(crate) enum AskError {
    UsageError(String),       // → 2
    NoProvider(String),       // → 69
    ProviderError(String),    // → 69
    Timeout(u64),             // → 124
    IoError(String),          // → 74
}

impl AskError {
    fn exit_code(&self) -> i32 { ... }
    fn kind_str(&self) -> &'static str { ... }
}
```

Fluxo:
1. Resolver `message`: arg posicional > stdin (cap 64 KiB) > erro vazio.
2. Resolver provider:
   - Se `url_override`: `chat::build_url_override_provider`.
   - Senão se `provider_override`: `chat::select_explicit_provider`.
   - Senão: `chat::detect_provider`.
3. Construir `AgentRuntime` **sem tools** (LLM-only).
4. Set system_prompt (do flag ou default mínimo).
5. Wrap chamada LLM em `tokio::time::timeout(Duration::from_secs(timeout_secs), ...)`.
6. Em sucesso: emit resposta (text ou JSON) em stdout; retornar 0.
7. Em erro: emit JSON envelope (se `--json`) ou mensagem humana em stderr; retornar exit code.

### `crates/garraia-cli/src/main.rs` — wiring

- `mod ask;`
- Novo variant em `Commands`:
  ```rust
  /// Non-interactive AI query — single message in, single answer out.
  Ask {
      /// Message to send. If absent, reads from stdin (64 KiB cap).
      message: Option<String>,
      #[arg(long, short = 'p')] provider: Option<String>,
      #[arg(long, short = 'm')] model: Option<String>,
      #[arg(long, short = 'u')] url: Option<String>,
      #[arg(long)] json: bool,
      #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u64).range(1..=600))]
      timeout_secs: u64,
      #[arg(long)] system_prompt: Option<String>,
  },
  ```
- Match arm em `async_main`: dispatcher to `ask::run_ask`; respect exit code via `std::process::exit`.

### Tests (em `ask.rs::tests`)

Todos puros, sem rede, sem env-mutation:

1. `ask_error_exit_codes_match_doc` — table-driven, asserts each variant.
2. `ask_error_kind_str_stable` — JSON kind labels are stable strings.
3. `json_envelope_success_shape` — `garra.ask.v1`, ok=true, answer/provider/model/latency_ms present.
4. `json_envelope_error_shape` — ok=false, error.kind, error.message.
5. `resolve_message_arg_wins_over_stdin` — passing message + stdin: arg wins.
6. `resolve_message_uses_stdin_when_arg_absent` — uses `Cursor` to mock stdin.
7. `resolve_message_empty_returns_usage_error` — empty arg + empty stdin → UsageError.
8. `resolve_message_stdin_capped_at_64kib` — oversized input → UsageError (or truncated, design choice; pick UsageError for safety).
9. `sanitize_provider_error_redacts_key_fingerprints` — `"sk-or-v1-abc"` and `"sk-proj-…J-QA"` replaced with `[REDACTED]` before JSON emit.

### Docs

- `docs/cli-ask.md` (novo, ~90 LOC) — surface, JSON schema, exit codes, examples.
- `docs/configuration.md` — 4 linhas de nota cruzada na seção "Provider/model resolution precedence" (GAR-576).
- `docs/src/SUMMARY.md` — 1 linha de link.
- `README.md` — 3 linhas no exemplo de uso CLI.

---

## §5 Verification

Comandos obrigatórios (mesmo gate do CI, ci.yml:54):

```bash
cargo fmt --all -- --check
cargo clippy --workspace --exclude garraia-desktop --all-targets -- -D warnings
cargo test -p garraia --bin garra
cargo build --release --bin garra
```

Smoke (manual, `openrouter/free` apenas):

```bash
GARRAIA_CONFIG_DIR=$HOME/.garraia ./target/release/garra.exe ask \
  --provider openrouter --model openrouter/free --json --timeout-secs 30 \
  "Responda apenas: GAR-ASK-OK"
```

Acceptance (deve passar antes do merge):

- Stdout = uma linha JSON válida `{"schema":"garra.ask.v1","ok":true,"answer":"GAR-ASK-OK",...}`.
- Exit 0.
- Nenhuma sequência ANSI no output (`grep $'\033'` returns nothing).
- `cargo test` 38 (GAR-576) + 9 (novos) = 47 passing.
- Zero nova dep em `Cargo.toml`.

`openrouter/auto` **NÃO** executado neste smoke. Só se `free` falhar e a razão for documentada antes.

---

## §6 Risks

- **Médio**: clap `value_parser!(u64).range(1..=600)` precisa de clap >= 4.something — workspace já usa, verificar antes.
- **Médio**: stdin reading via `tokio::io::stdin().read_to_end(&mut buf)` pode travar se for TTY interativo (sem EOF). Mitigação: detectar TTY via `atty::is(Stream::Stdin)` e exigir argumento posicional nesse caso. **Decisão**: tentar sem `atty` primeiro (sem nova dep). Se mostrar problema, adicionar dep ou usar `IsTerminal` da std (1.70+, workspace MSRV 1.92 → disponível).
- **Baixo**: timeout cancela o future do provider mas sockets HTTP podem demorar para fechar. `tokio::time::timeout` é o mecanismo correto; dropar o `Arc<dyn LlmProvider>` ao timeout libera recursos.
- **Baixo**: sanitização regex pode ter false-negatives (formatos novos de chave). Defesa em profundidade, não única linha. `RedactingWriter` real fica para PR separado.

Segurança:
- Tools NUNCA registrados — auditável via `grep -n register_tool crates/garraia-cli/src/ask.rs` → 0 hits.
- `--enable-tools` flag não existe — auditável via `grep enable.tools` → 0 hits.
- Resposta de erro do provider sanitizada antes de virar JSON — coberto pelo teste 9.

---

## §7 Out of scope (follow-ups)

Mantidos como follow-ups separados (alinhado com lista do usuário "Eu não faria agora"):

1. `--stream` flag — PR v2 depois deste.
2. `--enable-tools` + tool allowlist + auditoria — PR dedicado com `security-auditor`.
3. `--system-prompt-file` — PR pequeno depois.
4. MCP wrapper sobre `garra ask` — GAR-N+1, bloqueado por este PR.
5. `RedactingWriter` extension para payload de erro de provider — issue separada.
6. Automatic `openrouter/free → openrouter/auto` fallback — decisão de custo de tokens.
7. `dotenvy::dotenv_override(false)` ou opt-in para `.env` loading.
8. `GARRAIA_CONFIG_DIR=/custom/config/path` fantasma config — issue separada.
9. `.env` operator-side edits — decisão do operador.
10. Integration test com OpenRouter real em CI — gasta tokens, mantido manual.
11. `garra ask --session <id>` — continuação via `SessionStore`, v2.

---

## §8 Commit shape

- Commit title: `feat(cli): add non-interactive garra ask command`
- Conventional commits, NOT breaking (adição pura).
- Single commit preferred (squash on merge), but split if CodeQL drama exige refactor pós-CI (mesmo padrão do GAR-576).
