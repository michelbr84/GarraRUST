# GarraIA Changelog — Semana 19 (4 mai – 10 mai 2026)

## TL;DR

A camada REST `/v1` do Group Workspace saiu do papel. **7 endpoints novos** sobre o pool RLS-enforced (`garraia_app`), todos no mesmo padrão arquitetural: `Principal` extractor + `RequirePermission` + `set_config` parameterized + `FORCE RLS` no banco. A fundação foi a migração de `format!("SET LOCAL …")` para `SELECT set_config($1, $2, true)` (GAR-508 + GAR-511), que fechou 20 ocorrências de SQL-injection-estrutural e desbloqueou os slices.

Onda paralela de hardening: 5 PRs de XSS no front (escapeHtml + DOMPurify) — todos os assets HTML/JS do gateway agora escapam dado vindo do servidor.

## Group Workspace REST API — Fase 3.4

### Chats

- **GAR-506** — `POST/GET /v1/groups/{group_id}/chats` (slice 1, criação e listagem de canais por grupo).
- **GAR-507** — `POST/GET /v1/chats/{chat_id}/messages` (slice 2, envio + listagem com cursor `?after=<uuid>&limit=<n>`).
- **GAR-509** — `POST /v1/messages/{message_id}/threads` (slice 3, promove mensagem em thread root via `message_threads`).

### Memory

- **GAR-514** — `GET/POST/DELETE /v1/memory` (slice 1) com `scope_type ∈ {user, group, chat}` + `scope_id`. Plano 0062. Reusa pgvector HNSW da migration 003.

### Tasks (Notion-like Tier 1)

- **GAR-516** — `GET/POST/PATCH/DELETE` para `task-lists` e `tasks` (slice 1). Plano 0066, PR #145.
- **GAR-518** — `GET /v1/groups/{group_id}/tasks/{task_id}` + task-list `PATCH/DELETE` idempotente (slice 2). Plano 0068, PR #150.
- **GAR-520** — `task_comments` API (slice 3): `POST/GET/DELETE` em `/v1/groups/{group_id}/tasks/{task_id}/comments` com soft-delete e cursor. Schema migration 006 com FORCE RLS via JOIN policy `task_comments_through_tasks`.

### Fundação parameterized SQL

- **GAR-508** — replaced `format!("SET LOCAL app.current_X_id = '{val}'")` por `SELECT set_config('app.current_X_id', $1, true)` em 19 ocorrências (`rest_v1/{groups,invites,chats,messages,uploads}.rs`).
- **GAR-511** — cobriu a 20ª ocorrência em `uploads_worker.rs` fora do escopo de GAR-508.

## Security hardening — XSS wave (epic GAR-486)

- **GAR-510** — `admin.html` ganhou `escapeHtml` + 3 sinks corrigidos (channel rows + log viewer).
- **GAR-512** — `webchat.html` 4 sinks `innerHTML` agora wrapped em `escapeHtml`.
- **GAR-515** — `webchat.html` adopta DOMPurify para sanitizar `marked.parse()` em 2 sinks streaming + `escapeHtml(langLabel)`. SRI hash adicionado ao DOMPurify CDN.
- **GAR-517** — Criado módulo ES `assets/utils.js` com `escapeHtml`/`escapeAttr` shared, aplicado em `api.js` (5 sinks), `mcpView.js` e `memoryView.js`. PR #144.
- **GAR-519** — `modeSidebar.js` 7 sinks (`mode.name`/`description`/`id`) wrapped. Custom modes têm campos user-controlled, então XSS via mode persistia para todos os clientes do gateway.

## Outros movimentos

- **GAR-505** — Mutation Testing run 2026-05-04: triagem dos 6 mutantes missed + 3 timeouts. Workflow `mutants.yml` continua sem `continue-on-error` (fix forward).
- **GAR-513** — followup de GAR-437 acompanhando carve-outs RUSTSEC (glib/lru/rand) até 2026-07-31.
- **GAR-521** — Health & Security Routine 2026-05-05: todas as superfícies green, sem ação requerida.
- **GAR-503** — removido fallback dead-code `CARGO_BIN_EXE_garraia` dos integration tests.

## O que isso destrava

Com a camada `/v1` em pé sobre `set_config` parameterized + `garraia_app` pool, o caminho para a Fase 3.5 (deeper Notion-like — tasks subscriptions, activity feed, doc blocks) está livre. O próximo épico (GAR-396) já tem os primitivos REST que precisava.

## Métricas da semana

- 7 endpoints REST `/v1` novos (3 chat + 1 memory + 3 tasks).
- 20 ocorrências de SQL-injection-estrutural fechadas via parameterized SQL.
- 19 sinks XSS fechados em 5 assets do gateway.
- 24 issues fechadas no Linear (state=completed) team GAR-RUST.

## Links

- README: https://github.com/michelbr84/GarraRUST/blob/main/README.md
- Linear team GAR: https://linear.app/chatgpt25/team/GAR/all
- ROADMAP: https://github.com/michelbr84/GarraRUST/blob/main/ROADMAP.md
