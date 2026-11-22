# Plan 0367 — modularizar `rest_v1/me.rs` (Quality Ratchet `max_file_lines`)

**Issue:** [#1966](https://github.com/michelbr84/GarraRUST/issues/1966)
**Branch:** `refactor/1966-modulariza-me-rs`
**Status:** 🚧 Em revisão (2026-11-21)

## Contexto

O Quality Ratchet (report-only) no `86c43a7b` apontou o
`crates/garraia-gateway/src/rest_v1/me.rs` como o maior arquivo do repositório:
7510 linhas (6367 no baseline de 2026-10-07), com 27 handlers `/v1/me/*` e ~2040
linhas de testes unitários. O relatório pede uma regressão por vez; este plano
cuida só desse arquivo.

## O que mudou

`rest_v1/me.rs` virou `rest_v1/me/`. Cada arquivo tem um recurso e os seus
testes, e todos ficam abaixo de 700 linhas:

| Arquivo | Conteúdo |
|---|---|
| `mod.rs` | doc do módulo (tabela dos direitos do titular) + `pub use` de tudo o que era `pub` |
| `profile.rs` | `GET`/`PATCH /v1/me` |
| `inbox/{mentions,tasks,chats,files,memory,reactions,threads,doc_page_mentions,doc_pages}.rs` | as caixas de entrada paginadas por grupo |
| `invites.rs`, `invite_accept.rs` | convites: listar + recusar; aceitar |
| `sessions.rs`, `api_keys.rs` (+ `api_keys/tests.rs`), `password.rs`, `audit.rs` | recursos da conta |
| `delete.rs`, `anonymize.rs` | apagamento e anonimização |
| `export/{mod,types,group_content}.rs` | `GET /v1/me/export`: handler, tipos/tetos e a varredura por grupo |

São dois commits:

1. **Só movimento de código.** As únicas edições foram caminhos relativos
   (`super::problem` → `crate::rest_v1::problem`), imports por arquivo, o
   `#![deny(clippy::unwrap_used, clippy::expect_used)]` repetido em cada arquivo
   (como no `tasks/labels.rs`) e `pub(super)` nos helpers do export.
2. **Extração da varredura por grupo do `export_me`** (sozinho ele tinha 682
   linhas) para `export/group_content.rs`. Corpo do loop, SQL, binds e
   orçamentos ficaram iguais. O helper também passou a devolver o GUC
   `app.current_group_id` ao nil uuid antes de retornar `Ok`, já que quem aponta
   o RLS para um tenant é quem deve restaurá-lo.

## Invariantes

- `rest_v1/mod.rs` (router) e `openapi.rs` não mudam.
- **Spec OpenAPI byte-idêntico**: no utoipa 6 (`utoipa-gen` 6.0.1, `impl_paths`),
  a tag padrão de uma operação é o caminho passado em `paths(...)` sem o último
  segmento, hoje `super::me`. Por isso o `me/mod.rs` também re-exporta os
  `__path_*`. Se o `openapi.rs` apontasse para os submódulos, a tag mudaria.
- Os 89 itens `pub` continuam em `rest_v1::me::*` e os 148 testes unitários
  seguem com os mesmos nomes.

## Métricas esperadas

`files_over_700` 156 → 155 · `files_over_1500` 52 → 51 · `files_over_2500` 26 → 25
· `max_file_lines` 7510 → 7198. O `max_file_lines` **continua em regressão**: o
`garraia-config/src/check.rs` (7198; baseline 6933) passa a ser o maior e precisa
de um plano próprio. Mover só os testes dele pioraria as contagens.

## Test plan

- [x] `cargo fmt`, `cargo clippy -p garraia-gateway --lib --tests -- -D warnings`
- [x] `cargo test -p garraia-gateway --lib rest_v1::me::`: 148 testes, mesmos nomes
- [x] dump do spec OpenAPI antes/depois: idêntico
- [x] suítes `test-helpers` contra Postgres real (`rest_v1_me*`, `authz_http_matrix`, `harness_smoke`)
- [x] revisão independente: code-reviewer (MERGE_READY) + security-auditor (aprovado, R1, sem achados)
- [ ] CI verde
