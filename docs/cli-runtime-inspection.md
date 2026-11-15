# CLI Runtime Inspection (`cli_inspect`, issue #1712)

The interactive `garra chat` registers a native **read-only** `cli_inspect` tool. It is the
agent-facing counterpart of the user-facing `/status`, `/tools` and `/context` commands, and it
is **not** the gateway's privileged `garra_diagnostics` tool: the CLI never contacts the gateway
to answer.

## Evidence classes

Every field declares where it comes from, so "configured", "observed" and "not verifiable from
here" cannot be read as the same thing:

- **observed** — the running process recorded it (binary version, current turn's tool gate, a
  completed turn's statistics, the executor's tool trail).
- **declared** — read from the configuration this process loaded (`config.yml` plus the merged
  `mcp.json`). Declaration is never health.
- **unverified / unknown** — no authorized local source. The tool says so instead of inferring
  from the repository or a release page.

## Tool contract

Input: `{"scope":"identity"|"tools"|"all", "offset":0..1000}`. `scope` defaults to `identity`;
`offset` only pages the tool list.

Every response carries `origin=cli_runtime`, `read_only=true`, `channel=cli`, plus the blocks
below.

- **`identity`** (scope `identity`/`all`)
  - `version.value` from `env!("CARGO_PKG_VERSION")` with `source=running_binary`.
  - `provider_selected` from the CLI selection; `model_requested` from the current selection, so
    `/model` changes are reflected.
  - `last_turn` from the last completed turn: `provider`, `model`,
    `model_confirmed_by_provider`, `fallback_used`, `tool_calls`, and `source`
    (`provider_response` only when the provider confirmed the model, otherwise
    `runtime_requested_model`). A requested model is never reported as confirmed. Without a
    completed turn the state is `unknown` with a reason.
- **`tools`** (scope `tools`/`all`)
  - Built from the same collector as `/status`, `/tools` and `/context`
    (`AgentRuntime::visao_das_ferramentas`), so the two surfaces cannot drift.
  - Only tools allowed by the current turn gate are listed; blocked tool names are omitted.
    Each entry reports `origin`, `registration` (`registered`/`synthetic` — the synthetic
    `tool_program` appears only when the gate permits it), `runtime_availability`
    (`available` / `unavailable` with `reason_code`) and `turn_permission`.
  - `registered_total` is the registry count and `count` is what was allowed: registered is not
    the same number as permitted, and the report labels both.
  - Pages of at most 16 entries, with `offset`, `page_size` and `has_more`.
  - Without a turn dispatch snapshot, permission is `unknown` and the list is empty (fail-closed).
- **`tool_effects`** (always present)
  - Executor records for this CLI session: `tool_call_id`, `tool`, `class`
    (`observado`/`falhou`/`nao_verificado`), `turn`, `order` and a short `claim`. No inputs,
    outputs or paths.
  - `unverified` when no tool turn was observed — an empty list must not read as "nothing ran".
  - An interrupted call is not evidence that it did not run.
- **`gateway`** and **`mcp_servers`** (always present)
  - `gateway`: `not_configured` when no listener is declared, otherwise `declared` with the
    listen address. `live_verification=unavailable_from_cli` always.
  - `mcp_servers`: `not_configured` when the merged configuration has no server, otherwise
    `declared` with `mcp_json_included`, `count`, `enabled_count`, `listed`, `has_more` and up
    to 16 entries (`name`, `transport`, `enabled`). A missing `enabled` field means enabled, the
    same default as the loader.
  - Neither starts, probes or restarts anything. Declared is not running, and an integration
    disabled in configuration is neither reported as operational nor as broken.

## Safety envelope

- No shell, no filesystem reads, no HTTP calls, no credential reads, no service restarts, no raw
  log output. Nothing outside the process is mutated.
- Credentials are redacted as whole values when an authorization header or a credential-like
  label marks them; tool and MCP names are sanitized before leaving the process.
- Result size is bounded (tool pagination, MCP list cap, short claims) because the output goes
  into the model's turn.
- The tool stays usable with the gateway down: local facts remain, external state stays
  qualified as declared/unknown.

## Verification

Contract tests live next to the implementation (`crates/garraia-cli/src/cli_inspect_tool.rs`),
the wiring guards next to the chat loop (`crates/garraia-cli/src/chat.rs`,
`visao_de_ferramentas_tests`), and the provider-facing end-to-end tests drive the real
`AnthropicProvider` and `OpenAiProvider` over a local `wiremock` server
(`crates/garraia-cli/src/chat/e2e_provedores_reais_tests.rs`).
