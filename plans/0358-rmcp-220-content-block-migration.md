# Plan 0358 — rmcp 1.7 → 2.2 (`ContentBlock` e o fim do `Annotated<RawContent>`)

**Status:** 🚧 Em revisão
**Branch:** `claude/rmcp-2-2-0-migration-iz08vr`
**Origem:** Dependabot PR #853 (`dependabot/cargo/rmcp-2.2.0`), 9 jobs de CI vermelhos
**Data:** 2026-08-29 (America/New_York)

## Goal

Destravar o bump do `rmcp`, adiado desde o PR #844 com `@dependabot ignore`
(tracker interno #163), migrando o código para a API alinhada à spec MCP
2025-11-25.

## Root cause

A 2.2.0 traz *"[breaking] align model types with MCP 2025-11-25 spec"*
(upstream `modelcontextprotocol/rust-sdk#927`). A mudança dissolve duas camadas
de indireção. Na 1.7 o conteúdo era embrulhado:

```rust
pub struct Annotated<T: AnnotateAble> { #[serde(flatten)] pub raw: T, pub annotations: ... }
pub type Content = Annotated<RawContent>;          // model/content.rs:161
pub enum RawContent { Text(RawTextContent), ... }  // model/content.rs:153
```

Na 2.2 tudo virou um enum achatado, com `annotations` embutido em cada struct:

```rust
#[non_exhaustive]
pub enum ContentBlock { Text(TextContent), Image, Audio, Resource, ResourceLink }
pub struct TextContent { pub text: String, pub meta: ..., pub annotations: ... }
pub struct PromptMessage { pub role: Role, pub content: ContentBlock }
```

`rg 'RawContent|AnnotateAble|\bAnnotated\b|PromptMessageRole|PromptMessageContent'`
no source da 2.2.0 retorna **zero hits**: a família `Raw*`, o wrapper
`Annotated<T>`, o alias `Content` e os dois enums específicos de prompt foram
todos removidos.

Importante: **o formato de wire não mudou** — o handshake continua emitindo
`{"type":"text","text":...}`. A quebra é exclusivamente na superfície de tipos
Rust.

## Fix

Seis edições de código em três arquivos, mais o manifesto. Verificado símbolo a
símbolo contra os `.crate` de 1.7.0 e 2.2.0 baixados do crates.io (docs.rs está
bloqueado pelo proxy de egress desta sessão).

| Local | De | Para |
|---|---|---|
| `tool_bridge.rs:6` | `use rmcp::model::{CallToolRequestParams, RawContent}` | `…, ContentBlock}` |
| `tool_bridge.rs:143-144` | `match &content.raw { RawContent::Text(tc) =>` | `match content { ContentBlock::Text(tc) =>` |
| `tool_bridge.rs:145` | `tc.text.to_string()` | `tc.text.clone()` (o campo é `String`; o `match` liga `&TextContent`) |
| `manager.rs:770-771` | `PromptMessageRole::{User,Assistant}` | `Role::{User,Assistant}` |
| `manager.rs:775` | `PromptMessageContent::Text { text } => text` | `ContentBlock::Text(tc) => tc.text` |
| `mcp_server.rs:25,397` | `Content` / `Content::text(…)` | `ContentBlock` / `ContentBlock::text(…)` |

O braço `_` continua obrigatório nos dois `match` de `ContentBlock`: o enum é
`#[non_exhaustive]`. Já `Role` é `#[expect(clippy::exhaustive_enums)]`, então o
match de 2 braços compila sem curinga.

**Não** seguir a sugestão do compilador (`rmcp::model::PromptMessage::User`):
`PromptMessage` virou struct, não enum.

### A quebra que o CI não mostrou

O log do `Build Check` do #853 lista 5 erros, todos em `garraia-agents`, e
então aborta (*"build failed, waiting for other jobs to finish"*) — **sem
nunca compilar o `garraia-cli`**. `mcp_server.rs` importa `rmcp::model::Content`
(:25) e chama `Content::text` (:397), ambos removidos na 2.2. Corrigir apenas os
5 erros reportados produziria uma segunda rodada de CI vermelho.

### O buraco de CI que este plan fecha

`mcp-http` é OFF por default em toda a árvore e **nenhum** job do CI passava
`--features mcp-http` ou `--all-features` — logo `McpManager::connect_http` e o
transporte Streamable HTTP nunca eram compilados. O bump quebrou exatamente esse
caminho, e de um jeito que nenhum grep no nosso código acharia:

```
error[E0599]: no associated function named `from_bytes_stream` found for struct `SseStream<B>`
  --> rmcp-2.2.0/src/transport/common/reqwest/streamable_http_client.rs:87
```

O rmcp 2.2.0 declara `sse-stream = "0.2"` mas usa `SseStream::from_bytes_stream`,
que só existe a partir da **0.2.4** — under-specification upstream. Nosso lock
carregava `0.2.3` da resolução antiga. Resolvido com `cargo update -p sse-stream`
(0.2.3 → 0.2.5, semver-compatível), e o step novo de clippy no `ci.yml` impede a
regressão.

## Verificado como NÃO precisando de mudança

Levantado explicitamente para evitar churn defensivo:

- **`ServerCapabilities`** — os 8 campos (`experimental`, `extensions`, `logging`,
  `completions`, `prompts`, `resources`, `tools`, `tasks`) são idênticos entre
  1.7 e 2.2, então `get_info_advertises_only_tools_capability` segue válido.
- **`ListToolsResult { tools, next_cursor, meta }`** — a macro `paginated_result!`
  marca `#[expect(clippy::exhaustive_structs)]`, **não** `#[non_exhaustive]`; o
  struct literal em `mcp_server.rs:330` compila. É o único struct literal de tipo
  rmcp no repo inteiro.
- **`Resource` / `Prompt` / `PromptArgument`** — deixaram de ser `Annotated<RawX>`
  e viraram structs achatados, mas com nomes e tipos de campo idênticos. Como
  `Annotated<T>` fazia `Deref<Target = T>`, os acessos em `manager.rs` (`r.uri
  .to_string()`, `.description.as_deref()`, `a.required.unwrap_or(false)`)
  compilam nas duas versões.
- **`ResourceContents::TextResourceContents { text, .. }`** — variantes e campos
  idênticos; virou `#[non_exhaustive]`, e o braço `_ => None` já existia.
- **Nomes de features** — `client`, `server`, `macros`, `transport-child-process`,
  `transport-io`, `transport-streamable-http-client-reqwest` inalterados: nenhum
  `Cargo.toml` de crate muda.
- **Split reqwest 0.12/0.13** — a 2.2 continua em `reqwest 0.13.2`, igual à 1.7.
  O gap de DNS-pinning documentado em `manager.rs:44-58` **permanece correto** e
  não foi editado.
- **Versão de protocolo** — `ProtocolVersion::LATEST` é `2025-11-25` nas duas
  versões e o client não valida a versão do servidor, então o fixture
  `fake_mcp_server.py` (`2025-06-18`) segue funcionando sem alteração.

## Cobertura de runtime

`crates/garraia-agents/tests/mcp_lifecycle.rs::tool_survives_reconnect` é o único
teste que exercita o caminho de decode alterado de ponta a ponta — ele afirma
que a saída da tool contém `"pong"`, o que só acontece se o braço
`ContentBlock::Text` extrair o texto. Verde.

Além disso, o handshake completo foi executado de verdade contra o binário
recompilado (`initialize → tools/list → tools/call`, provider `echo` keyless), e
o transcript em `docs/integrations/hermes-mcp.md` foi atualizado com a saída
real — `"serverInfo":{"name":"rmcp","version":"2.2.0"}` — em vez de editado à
mão.

## Out of scope

- Salto para **rmcp 3.1.4** (tracker interno #163 segue aberto). O 3.x mantém
  `ContentBlock`/`Role`/`PromptMessage` e todas as features que usamos, mas
  adiciona módulos novos (`mrtr`, `request_state`, `service/client/`,
  `mcp_headers`) cujas breaking changes não foram auditadas aqui.
- Cobertura de teste para o handler `call_tool` de `mcp_server.rs` (:337-403),
  que hoje tem zero — só os helpers têm.

## File structure

| Arquivo | Mudança |
|---|---|
| `crates/garraia-agents/src/mcp/tool_bridge.rs` | import + loop de decode |
| `crates/garraia-agents/src/mcp/manager.rs` | `Role` + `ContentBlock` em `get_prompt` |
| `crates/garraia-cli/src/mcp_server.rs` | import + `ContentBlock::text` + prosa "rmcp 1.6" desatualizada |
| `Cargo.toml` | `rmcp = { version = "2.2" }` + comentário de justificativa |
| `Cargo.lock` | `rmcp`/`rmcp-macros` 1.7.0→2.2.0, `sse-stream` 0.2.3→0.2.5 |
| `.github/workflows/ci.yml` | step novo `Run clippy (mcp-http)` no job `clippy` |
| `docs/integrations/hermes-mcp.md` | transcript real do handshake contra a 2.2.0 |
| `CHANGELOG.md` | `[Unreleased]` → Changed |
| `TODO.md` | rmcp sai da lista de upgrades adiados |
| `plans/README.md` | linha de índice 0358 |

O diff do `Cargo.lock` fica restrito a 3 pacotes — `rmcp`, `rmcp-macros`,
`sse-stream`.

## Acceptance criteria

- [x] `cargo check -p garraia-agents --features mcp` limpo (os 5 erros do #853 somem)
- [x] `cargo check -p garraia` limpo (a 6ª quebra latente, que o CI nunca alcançou)
- [x] `cargo check -p garraia --features mcp-http` limpo (caminho sem cobertura de CI)
- [x] `cargo test -p garraia-agents --test mcp_lifecycle` — 3/3, incl. `tool_survives_reconnect`
- [x] `cargo test -p garraia --bin garra mcp_server` — 32/32, incl. os dois `audit_…` que varrem o próprio source
- [x] `cargo fmt --check --all` limpo
- [x] `cargo clippy --workspace --exclude garraia-desktop --all-targets -- -D warnings` limpo
- [x] `cargo clippy -p garraia --features mcp-http --all-targets -- -D warnings` limpo
- [x] Handshake MCP real (`initialize → tools/list → tools/call`) verde contra a 2.2.0

## Risk register

| Risco | Prob. | Mitigação |
|---|---|---|
| Comentário de migração citar string proibida e quebrar os testes `audit_…` de `mcp_server.rs`, que fazem `include_str!` do próprio arquivo | Média — é uma armadilha não óbvia | Comentários redigidos sem as strings da lista; 32/32 verdes |
| `mcp-http` quebrar sem ninguém ver | **Materializou-se** (E0599 do `sse-stream`) | `cargo update -p sse-stream` + step novo no `ci.yml` |
| Alguma variante nova de `ContentBlock` cair no placeholder silenciosamente | Baixa | Comportamento idêntico ao anterior — o `_` já existia na 1.7 |
| `Role` ganhar variante e quebrar o match de 2 braços | Baixa | É `#[expect(clippy::exhaustive_enums)]` upstream, intencionalmente exaustivo |

Rollback: nada aqui toca config, migrations, schema ou API pública. `git revert`
simples restaura `rmcp = "1.7"` e o código antigo.

## Cross-references

- Dependabot PR #853 (rmcp 2.2.0) — superseded por este trabalho
- Upstream `modelcontextprotocol/rust-sdk#927` (align model types with MCP 2025-11-25)
- plan 0356 (lopdf 0.42 → 0.44) — precedente direto de bump com compat fix
- plans 0102 / 0103 / 0104 (GAR-583 / 585 / 587) — descrevem a API rmcp 1.6 em prosa
- TODO.md — tracker interno #163 (rmcp), agora reduzido ao salto 3.x
