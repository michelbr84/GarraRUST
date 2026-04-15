# plans/

Histórico de planos de execução do GarraIA. Cada plano está atrelado a uma issue GAR-* no Linear e é aprovado antes da execução.

## Convenção de nome

`NNNN-gar-XXX-slug-descritivo.md`

- `NNNN` — sequencial monotônico (`0001`, `0002`, ...) — ordem cronológica de criação.
- `gar-XXX` — issue Linear principal que o plano entrega.
- `slug-descritivo` — identificador humano curto em kebab-case.

## Regras

- **Aprovação obrigatória:** nenhum plano vira código sem "Plano aprovado" explícito do owner.
- **Imutável após merge:** um plano é o registro histórico de como a decisão foi tomada. Se o escopo mudar, crie um novo plano (`NNNN+1`) que o supersede.
- **Escopo claro:** `§1 Goal`, `§3 Scope/Non-scope`, `§4 Acceptance criteria` são obrigatórios.
- **Rollback plan:** todo plano precisa de `§8 Rollback plan` — se é reversível, como; se não é, por quê.
- **Open questions:** dúvidas que bloqueiam execução ficam no `§12 Open questions` e precisam ser respondidas antes do start.

## Index

| # | Plano | Issue | Status |
|---|---|---|---|
| 0001 | [OpenTelemetry + Prometheus baseline](0001-gar-384-opentelemetry-baseline.md) | [GAR-384](https://linear.app/chatgpt25/issue/GAR-384) | ✅ Merged 2026-04-13 (`84c4753`) |
| 0002 | [ADR 0003 — Database para Group Workspace](0002-gar-373-adr-postgres-decision.md) | [GAR-373](https://linear.app/chatgpt25/issue/GAR-373) | ✅ Merged 2026-04-13 (`32dba08`) |
| 0003 | [`garraia-workspace` crate + migration 001 (users & groups)](0003-gar-407-workspace-schema-bootstrap.md) | [GAR-407](https://linear.app/chatgpt25/issue/GAR-407) | ✅ Merged 2026-04-13 (`4c0f07e`) |
| 0004 | [Migration 002 — RBAC + audit_events](0004-gar-386-migration-002-rbac.md) | [GAR-386](https://linear.app/chatgpt25/issue/GAR-386) | ✅ Merged 2026-04-13 (`54cefca`, closes GAR-414) |
| 0005 | [Migration 004 — chats + messages + FTS](0005-gar-388-migration-004-chats-fts.md) | [GAR-388](https://linear.app/chatgpt25/issue/GAR-388) | ✅ Merged 2026-04-13 (`1514227`) |
| 0006 | [Migration 005 — memory_items + pgvector HNSW](0006-gar-389-migration-005-memory-pgvector.md) | [GAR-389](https://linear.app/chatgpt25/issue/GAR-389) | ✅ Merged 2026-04-13 (`d790b9a`) |
| 0007 | [Migration 007 — Row-Level Security (FORCE RLS)](0007-gar-408-migration-007-rls.md) | [GAR-408](https://linear.app/chatgpt25/issue/GAR-408) | ✅ Merged 2026-04-13 (`18d0326`) |
| 0008 | [Migration 006 — Tasks (Tier 1) com RLS embutido](0008-gar-390-migration-006-tasks-with-rls.md) | [GAR-390](https://linear.app/chatgpt25/issue/GAR-390) | ✅ Merged 2026-04-13 (`883399e`) |
| 0009 | [ADR 0005 — Identity Provider (BYPASSRLS + Argon2id + HS256)](0009-gar-375-adr-0005-identity-provider.md) | [GAR-375](https://linear.app/chatgpt25/issue/GAR-375) | ✅ Merged 2026-04-13 (`a89c783`) |
| 0010 | [`garraia-auth` crate skeleton + migration 008 (login role)](0010-gar-391a-garraia-auth-crate-skeleton.md) | [GAR-391a](https://linear.app/chatgpt25/issue/GAR-391) | ✅ Merged 2026-04-13 (`c5d6350`) |
| 0011 | [`verify_credential` real impl + audit + JWT issuance](0011-gar-391b-verify-credential-impl.md) | [GAR-391b](https://linear.app/chatgpt25/issue/GAR-391) | ✅ Merged 2026-04-13 (`354d72a`) |
| 0011.5 | [Migration 009 — `user_identities.hash_upgraded_at` (corretiva 391b)](0011.5-gar-391b-migration-009-hash-upgraded-at.md) | [GAR-391b](https://linear.app/chatgpt25/issue/GAR-391) | ✅ Merged 2026-04-13 (`356e5e4`) |
| 0012 | [Axum extractor + `RequirePermission` + refresh/logout/signup endpoints](0012-gar-391c-extractor-and-wiring.md) | [GAR-391c](https://linear.app/chatgpt25/issue/GAR-391) | ✅ Merged 2026-04-13 (`88f323e`) |
| 0013 | [RLS matrix (GAR-392) — path C, 391d deferido](0013-gar-391d-392-authz-suite.md) | [GAR-392](https://linear.app/chatgpt25/issue/GAR-392) | ✅ Merged 2026-04-14 (`4069ace` + `1267987`). GAR-391d re-escopado para plan 0014; epic **GAR-391 permanece aberto** |
| 0014 | _(planejado)_ App-layer cross-group authz matrix via HTTP | [GAR-391d](https://linear.app/chatgpt25/issue/GAR-391) | ⏳ Deferido — aguarda endpoints REST `/v1/{chats,messages,memory,tasks,groups,me}` materializarem na Fase 3.4 |
| 0015 | [Fase 3.4 — REST `/v1` skeleton (slice 1: `GET /v1/me`)](0015-fase-3-4-rest-v1-skeleton.md) | GAR-WS-API (pré-condição GAR-391d) | ✅ Merged 2026-04-14 (`4afb4fe`, PR #8). Entregue **apenas o slice 1**: endpoint `GET /v1/me`, OpenAPI/Swagger em `/docs`, RFC 9457 Problem Details, fail-soft 503 e teste de integração fail-soft. Cobertura autenticada (`200`/`401`/`403` com JWT + Postgres real) foi deferida ao plan 0016. |
| 0016 | [Fase 3.4 — Slice 2: AppPool + harness + authed `/v1/me` + `/v1/groups` skeleton](0016-fase-3-4-slice-2-apppool-harness-groups.md) | GAR-WS-API (destrava definitivamente GAR-391d após M3) | 🚧 **M1 merged** 2026-04-14 (`3d2fc66`, PR #10) — `AppPool` newtype + `GARRAIA_APP_DATABASE_URL` + `AppState.app_pool` + split `RestV1State` → `AuthState`/`FullState` com 5 `FromRef` impls + router 3-way match + `/v1/groups` stubs apontando para `unconfigured_handler`. **M2+ pendentes:** M2 harness de integração compartilhado, M3 authed `/v1/me` + OpenAPI bearer, M4 handlers reais de `/v1/groups`, M5 follow-ups do review do PR #8. |

## Arquivos não-versionados

Drafts ad-hoc dentro de `plans/` que **não** sigam o padrão `NNNN-*.md` ficam gitignored por design — ver `.gitignore`. Útil para rascunhos pessoais antes de formalizar um plano numerado.
