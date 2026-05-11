# `garra ask` — Non-interactive AI query

> GAR-579 — single message in, single answer out. LLM-only (no tools).
> Designed for Claude Code, CI, hooks, scripts, and future MCP wrappers.

`garra ask` is the **non-interactive** counterpart to `garra chat`. It
takes one message, returns one answer, and exits with a sysexits-style
code. No banner, no ANSI escapes, no `voce >` / `garra >` prompts —
stdout is the answer (or a `garra.ask.v1` JSON envelope when `--json`).

## Surface

| Comando | stdout | Exit |
|---------|--------|------|
| `garra ask "ping"` | resposta como texto puro | `0` |
| `garra ask --json "ping"` | linha JSON `garra.ask.v1` | `0` |
| `garra ask --provider openrouter "ping"` | usa modelo do `config.yml` (precedência GAR-576) | `0` |
| `garra ask --provider openrouter --model openrouter/auto "ping"` | usa `openrouter/auto` explícito | `0` |
| `echo "ping" \| garra ask` | lê stdin (cap 64 KiB) quando arg posicional ausente | `0` |
| `garra ask ""` (arg vazio) | erro claro em stderr (ou JSON em stdout se `--json`) | `2` |
| `garra ask --timeout-secs 1 "msg lenta"` | erro timeout | `124` |
| provider auth/network falha | erro `provider_error` com mensagem sanitizada | `69` |

## Flags

| Flag | Default | Descrição |
|------|---------|-----------|
| `--provider`, `-p <kind>` | autodetect | `ollama` \| `anthropic` \| `openai` \| `openrouter` |
| `--model`, `-m <name>` | resolve via config (GAR-576) | sobrescreve modelo (precedência absoluta) |
| `--url`, `-u <url>` | — | endpoint custom (LM Studio, vLLM, etc.) |
| `--json` | `false` | emit JSON envelope em stdout |
| `--timeout-secs <N>` | `60` | range `[1, 600]`; excede → exit `124` |
| `--system-prompt <STR>` | minimum default | sobrescreve system prompt inline |

## JSON envelope (`schema: garra.ask.v1`)

**Sucesso**:

```json
{
  "schema": "garra.ask.v1",
  "ok": true,
  "answer": "GAR-ASK-OK",
  "provider": "openrouter",
  "model": "openrouter/free",
  "latency_ms": 1234
}
```

**Erro**:

```json
{
  "schema": "garra.ask.v1",
  "ok": false,
  "error": {
    "kind": "provider_error",
    "message": "401 Unauthorized — key sk-or-v1-[REDACTED] invalid"
  }
}
```

`error.kind` é estável: `usage`, `no_provider`, `provider_error`,
`timeout`, `io`. Strings podem ser parseadas por scripts.

## Exit codes

| Code | Significado |
|------|-------------|
| `0` | sucesso |
| `2` | erro de uso (mensagem vazia, stdin acima do cap, etc.) |
| `65` | arquivo de config inválido (`EX_DATAERR`) — reservado, propagado pela inicialização |
| `69` | provider indisponível, auth, network (`EX_UNAVAILABLE`) |
| `74` | falha de I/O ao ler stdin (`EX_IOERR`) |
| `124` | LLM excedeu `--timeout-secs` (convenção Unix `timeout(1)`) |

## Resolução de provider e model

Idêntica ao `garra chat` — ver
[`docs/configuration.md` §"Provider / model resolution precedence"](configuration.md#provider--model-resolution-precedence) (GAR-576):

1. `--model <X>` (precedência absoluta).
2. `config.llm[<key>].model` (key match).
3. Primeiro `config.llm[*]` cujo `provider:` casa.
4. Fallback hardcoded por kind.

Para provider:

1. `--provider <X>` (precedência absoluta).
2. `config.agent.default_provider`.
3. Cadeia autodetect (Ollama health → Anthropic env → OpenAI env → OpenRouter env).

## Exemplos

### Smoke barato (recomendado para CI / testes)

```bash
garra ask --provider openrouter --model openrouter/free \
  --json --timeout-secs 30 "Responda apenas: GAR-ASK-OK"
```

Saída (uma linha):

```json
{"schema":"garra.ask.v1","ok":true,"answer":"GAR-ASK-OK","provider":"openrouter","model":"openrouter/free","latency_ms":1432}
```

### Tarefa real (uso humano)

```bash
garra ask --provider openrouter --model openrouter/auto \
  "Explique em uma frase o que é o GarraIA."
```

Saída (texto puro, sem banner):

```
O GarraIA é um gateway de IA multi-canal escrito em Rust que orquestra LLMs
via Telegram, Discord, Slack, WhatsApp e API REST.
```

### Stdin pipeline

```bash
cat README.md | garra ask --json --provider openrouter \
  --system-prompt "Resuma em 3 bullets." -m openrouter/free
```

### Em scripts / Claude Code / MCP

```bash
RESULT=$(garra ask --json --provider openrouter -m openrouter/free \
  --timeout-secs 30 "$prompt")
ANSWER=$(echo "$RESULT" | jq -r '.answer // .error.message')
```

## Tools e segurança

`garra ask` é **LLM-only**. Tool registration (`bash`, `file_read`,
`file_write`, `git_diff`) está **ausente** do runtime por design.
Auditável via teste `ask_module_never_registers_a_tool` que escaneia o
código de produção em build time.

Suporte a tools com allowlist + auditoria entra em PR separado
(eg. `GAR-579+`); requer security-auditor.

## Segurança operacional

- Response body de erro do provider passa por
  `sanitize_provider_error` antes de virar JSON ou stderr —
  fingerprints `sk-…` / `sk-or-v1-…` são redactionados a
  `[REDACTED]`, mensagens longas truncadas em 512 caracteres.
- Stdin é lido até 64 KiB; input maior retorna `usage` error (exit 2),
  não trunca silenciosamente.
- Sem `panic!` em paths de I/O / provider / network — todos os erros
  voltam pelo enum `AskError` e viram exit code previsível.
- Defesa em profundidade: `RedactingWriter` provider-aware fica em PR
  dedicado de `garraia-security`.

## Não está aqui

PRs separados manterão escopo limpo:

- `--stream` para emit incremental.
- `--enable-tools` + tool allowlist + auditoria.
- `--system-prompt-file <PATH>`.
- MCP server wrapper expondo `ask` como tool MCP.
- `--session <id>` para continuação de conversa.
- Automatic `openrouter/free → openrouter/auto` fallback.

## Ver também

- [`docs/configuration.md`](configuration.md) — provider/model resolution (GAR-576).
- [`docs/src/providers.md`](src/providers.md) — OpenRouter cost policy.
- `plans/0098-gar-576-cli-openrouter-model-respect.md` — base já estável.
- `plans/0099-gar-579-cli-non-interactive-ask.md` — este PR.
