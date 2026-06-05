# TODO

Status operacional do backlog do GarraIA/GarraRUST. Este arquivo complementa
`ROADMAP.md`: o roadmap guarda a direção de produto; este TODO registra o que
foi concluído, o que ficou parcial ou adiado, decisões tomadas e próximos passos
curtos para a próxima sessão autônoma.

**Atualizado:** 2026-06-05 (America/New_York)

## Concluído nesta sessão

- GAR-794 / plan 0263 — POST /v1/me/invites/{invite_id}/accept:
  - `accept_my_invite` handler in `me.rs`: UUID-based authenticated accept.
  - Atomic tx: UPDATE group_invites (with all terminal guards in WHERE) + INSERT group_members + audit InviteAccepted.
  - 410 (expired) distinguished from 404 via follow-up SELECT when UPDATE returns None.
  - 409 (already member) via SQLSTATE 23505 on group_members INSERT.
  - Route registered in all 3 mod.rs branches; OpenAPI path + AcceptMyInviteResponse schema registered.
  - 6 unit tests covering: serialization, no-PII fields, role variants, nil UUID round-trip, PendingInviteSummary excludes accepted_at, exactly-3-fields shape.
  - Completes the invite lifecycle: list (GAR-777) → accept (GAR-794) / decline (GAR-783); token-based accept (plan 0019) unchanged.

## Concluído em sessões anteriores

- GAR-777 / plan 0255 — GET /v1/me/invites (caller-scoped pending group invites inbox):
  - Merged PR #621 (`762d63c`) after CI went 20/20 green.
  - GAR-777 → Done in Linear.
  - Bookkeeping PR #624 (docs/mark-plan-0255-merged) open, CI running.

- GAR-780 / plan 0257 — GET + DELETE /v1/groups/{id}/invites/{invite_id} (invite revocation):
  - Migration 024: `revoked_at` + `revoked_by` columns on `group_invites`; recreated partial unique index to exclude revoked rows (enables re-invite after revocation).
  - `WorkspaceAuditAction::InviteRevoked` variant + `"invite.revoked"` string + test assertion.
  - `list_invites` WHERE updated: `AND revoked_at IS NULL`.
  - `get_invite` handler: returns `InviteSummary` (404 if not found/accepted/revoked).
  - `revoke_invite` handler: `UPDATE SET revoked_at = now()`, emits `InviteRevoked` audit event, 204 No Content (404 if already accepted/revoked).
  - Routes in all 3 `mod.rs` branches. OpenAPI paths + schemas (`InviteSummary`, `ListInvitesResponse`).
  - 5 unit tests (serialization, cursor, role round-trip, no `revoked_at` in response).
  - Squash-merged PR #625 (`46a8658`) 2026-06-03 — 20/20 CI green.
  - GAR-780 → Done in Linear.
  - Bookkeeping PR (docs/mark-plan-0257-merged) open.

## Concluído em sessões anteriores


- GAR-767 / plan 0246 — GET /v1/me/files (caller-scoped uploaded-files inbox):
  - `ListMyFilesQuery` struct with `group_id` (required), `after`, `limit`, `folder_id` (optional).
  - `MyFileSummary` fields: `id`, `group_id`, `name`, `mime_type`, `size_bytes`, `folder_id` (skip_if_none), `created_at`, `updated_at` (skip_if_none).
  - `MyFilesResponse` with `items` + `next_cursor` (skip_serializing_if None).
  - 4-branch query (cursor × folder_id filter), keyset on `(files.created_at DESC, files.id DESC)`.
  - Route `.route("/v1/me/files", get(me::list_my_files))` registered in all 3 `mod.rs` branches.
  - OpenAPI annotation + component registration in `openapi.rs`.
  - 8 new unit tests (serialization, limit clamp, folder filter, cursor, large size).
  - Branch: `routine/202506010015-me-files-inbox`. GAR-767 In Progress → Done pending CI.

- GAR-765 / plan 0245 — GET /v1/me/chats (caller-scoped chat membership inbox):
  - `ListMyChatsQuery` struct with `group_id` (required), `after`, `limit`, `type` (optional).
  - `ChatMembershipSummary` fields: `chat_id`, `group_id`, `name`, `type`, `role`, `joined_at`, `muted`, `last_read_at`.
  - `MyChatsMembershipResponse` with `items` + `next_cursor` (skip_serializing_if None).
  - 4-branch query (cursor × type filter), keyset on `(cm.joined_at DESC, cm.chat_id DESC)`.
  - Route `.route("/v1/me/chats", get(me::list_my_chats))` registered in `mod.rs`.
  - OpenAPI annotation + component registration in `openapi.rs`.
  - 8 new unit tests (serialization, type filter validation, cursor, muted/last_read_at).
  - All CI checks expected green (no migration, additive handler only).
  - Branch: `routine/202605311818-me-chats-inbox`. GAR-765 In Progress → Done.

## Concluído em sessões anteriores

- GAR-733 / plan 0215 — Search slice 14 (`types=groups` group name FTS):
  - `SearchResultType::Group` variant; `include_groups: bool` in `ValidatedSearch`.
  - `parse_and_validate`: recognizes `"groups"`, rejects non-user scope with 400.
  - `GroupSearchRow` struct + `fetch_groups()` async (runtime `to_tsvector('simple', g.name)`).
  - Handler block: `if validated.include_groups { ... }` mapping to `SearchResult`.
  - 6 unit tests (scope guards + multi-type combos). No migration needed — FORCE RLS migration 018.
  - PR #561 squash-merged 2026-05-29 (`1bb2f10`). GAR-733 Done in Linear.

- GAR-705 / plan 0187 — Health run 30: all surfaces clean, priority (i). PR #508 squash-merged (`ef040ad`).

- GAR-467 / plan 0188 — Q6.5 Mutation Testing — audit_event observability coverage:
  - Added `count_audit_action(...) == 1` assertion to all 7 terminals of `verify_credential_with_ctx`.
  - Added `row.ip.is_some()` assertion to all non-argon2id terminals (T3–8).
  - New test `null_stored_hash_emits_unknown_hash_audit`: seeds user with NULL password_hash,
    asserts `Err(UnknownHashFormat)` + 1 audit row committed + ip populated.
  - Total: 11 integration tests (was 10). Tests-only PR, no production code changes.
  - PR #509 squash-merged (`a1b0fdd`).

## Concluído em sessões anteriores

- GAR-702 / plan 0184 — Health run 28: all surfaces clean, priority (i). PR #504 squash-merged.

- GAR-703 / plan 0185 — Search slice 5 (`types=files` file name FTS):
  - `SearchResultType::File` variant added.
  - `include_files: bool` in `ValidatedSearch`.
  - `parse_and_validate`: recognizes `"files"`, rejects non-group scope.
  - `FileSearchRow` struct + `fetch_files()` async function (runtime tsvector 'simple').
  - Handler: `if validated.include_files { ... }` block mapping to `SearchResult`.
  - 6 new unit tests; `unknown_type_rejected` updated to use `"tasks"` (not `"files"`).
  - ROADMAP.md + plans/README.md + TODO.md updated.
  - Branch: `routine/202605251215-search-slice5-files`, PR #505, merged `bb8c040`.

- GAR-697 / plan 0179 — Search slice 4 (`has_attachment` filter):
  - Migration 020 (`message_attachments` M:N join table, FORCE RLS via JOIN
    through messages, índice `message_attachments_message_idx` para o EXISTS
    subquery path).
  - `search.rs`: `SearchQuery.has_attachment: Option<bool>`, validação (rejeita
    quando `types` não inclui `messages`), predicado SQL EXISTS-equality trick.
  - Tests: 5 unit tests novos (slice 4 block), S18/S19/S20 integration scenarios.
  - ROADMAP.md + plans/README.md + TODO.md atualizados.
  - Branch: `routine/202605250015-search-has-attachment`, PR em revisão.

## Parcialmente concluído

- GAR-603 Runpod Load Balancer Serverless:
  - Concluído por evidência estática/docs: `garra start` em modo HTTP,
    container bindando `0.0.0.0`, rotas `/ping` e `/health`, `PORT`/`HOST`,
    Dockerfile sem REPL, receita local Docker, settings Runpod e distinção
    queue-based vs Load Balancer.
  - Pendente: smoke Docker local nesta sessão e smoke público
    `https://<ENDPOINT_ID>.api.runpod.ai/ping`.
  - Pendente técnico: suporte a `PORT_HEALTH` separado quando a health port
    precisar diferir de `PORT`; hoje a documentação exige `PORT_HEALTH=PORT`.

## Adiado com justificativa

- GAR-372 / Fase 2.1 RAG embeddings: adiado porque a próxima entrega real
  exige toolchain Rust e testes; o ambiente local desta sessão não tinha
  `cargo`, `rustc` ou `rustfmt`.
- GAR-374 / Object storage S3-compatible validation: adiado por depender de
  MinIO/S3/R2/GCS ou CI com serviço externo configurado.
- GAR-410 / CredentialVault final: adiado por ser item crítico de segurança,
  amplo e inadequado para alteração sem toolchain local e validação profunda.
- GAR-504 / benchmark evidence run: adiado por depender de infra externa
  (droplet/host dedicado).
- Execução async/provider-backed das native skills GarraMaxPower: adiada para
  slice próprio após decidir o fechamento do épico GAR-492.

## Novas pendências encontradas

- O repositório não tinha `TODO.md`; manter este arquivo atualizado em toda
  sessão autônoma daqui para frente.
- O ambiente local tinha `git`, `node` e `rg`, mas não tinha `cargo`,
  `rustc`, `rustfmt`, `gitleaks` ou `markdownlint`. Mudanças de runtime devem
  esperar toolchain local ou depender explicitamente de CI no PR.
- `ROADMAP.md` ainda contém vários itens antigos marcados como `[ ]` que podem
  estar parcialmente entregues por PRs anteriores. Próxima limpeza deve
  reconciliar apenas itens com evidência clara para evitar falsear status.
- `GAR-492` está em In Review: decidir se fecha como MVP completo ou se mantém
  aberto somente até abrir follow-ups separados.

## Decisões tomadas

- Não alterar runtime Rust nesta sessão: sem toolchain local, o caminho seguro
  foi documentação, rastreabilidade e reconciliação de backlog.
- Marcar GAR-603 como parcialmente concluído, não totalmente fechado: a
  implementação/documentação está presente, mas falta prova operacional recente
  em Docker e Runpod público.
- Criar `TODO.md` como backlog operacional curto, evitando sobrecarregar
  `ROADMAP.md` com detalhes de sessão.

## Próximos passos recomendados

1. Rodar smoke Docker GAR-603:
   `docker build -t garraia:local .`,
   `docker run --rm -p 3888:3888 garraia:local`,
   `curl -fsS http://localhost:3888/ping`,
   `curl -fsS http://localhost:3888/health`.
2. Rodar smoke público Runpod quando houver endpoint disponível:
   `curl -fsS https://<ENDPOINT_ID>.api.runpod.ai/ping`.
3. Abrir follow-up para `PORT_HEALTH` separado somente se Runpod exigir health
   listener distinto de `PORT`.
4. Decidir destino de GAR-492: fechar épico como MVP completo ou abrir issues
   separadas para dogfood em bug real e execução async/provider-backed.
5. Preparar ambiente local com Rust toolchain para permitir mudanças de código
   mais ambiciosas nas próximas sessões.
