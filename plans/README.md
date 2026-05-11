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
| 0006 | [Migration 005 — memory_items + pgvector HNSW](0006-gar-389-migration-005-memory-pgvector.md) | [GAR-389](https://linear.app/chatgpt25/issue/GAR-389) | ✅ Merged 2026-04-13 (`1514227`) |
| 0007 | [Migration 006 — tasks Tier 1 Notion-like](0007-gar-390-migration-006-tasks.md) | [GAR-390](https://linear.app/chatgpt25/issue/GAR-390) | ✅ Merged 2026-04-13 (`1514227`) |
| 0008 | [Migration 007 — RLS FORCE wrap-up](0008-gar-408-migration-007-rls-wrapup.md) | [GAR-408](https://linear.app/chatgpt25/issue/GAR-408) | ✅ Merged 2026-04-13 (`1514227`) |
| 0009 | [Migration 008 — garraia_login role](0009-gar-391a-migration-008-login-role.md) | [GAR-391](https://linear.app/chatgpt25/issue/GAR-391) | ✅ Merged 2026-04-13 (`1514227`) |
| 0010 | [Migration 009 — hash_upgraded_at](0010-gar-391b-migration-009-hash-upgraded-at.md) | [GAR-391](https://linear.app/chatgpt25/issue/GAR-391) | ✅ Merged 2026-04-13 (`1514227`) |
| 0011 | [Migration 010 — garraia_signup role](0011-gar-391c-migration-010-signup-role.md) | [GAR-391](https://linear.app/chatgpt25/issue/GAR-391) | ✅ Merged 2026-04-13 (`1514227`) |
| 0012 | [Migration 011-013 — group_invites + indexes + audit WITH CHECK](0012-gar-391c-migration-011-013.md) | [GAR-391](https://linear.app/chatgpt25/issue/GAR-391) | ✅ Merged 2026-04-13 (`1514227`) |
| 0013 | [RLS matrix (GAR-392, plan C)](0013-gar-392-rls-matrix.md) | [GAR-392](https://linear.app/chatgpt25/issue/GAR-392) | ✅ Merged 2026-04-14 (`plan-C`) |
| 0014 | [App-layer cross-group authz (GAR-391d — deferred)](0014-gar-391d-app-layer-authz.md) | [GAR-391](https://linear.app/chatgpt25/issue/GAR-391) | ⏸ Deferred (Fase 3.4) |
| 0015 | [REST /v1 foundation slice 0 (GAR-386)](0015-gar-386-rest-v1-foundation-slice0.md) | [GAR-386](https://linear.app/chatgpt25/issue/GAR-386) | ✅ Merged 2026-04-14 |
| 0016 | [REST /v1/groups CRUD slice 1 (GAR-386)](0016-gar-386-rest-v1-groups-slice1.md) | [GAR-386](https://linear.app/chatgpt25/issue/GAR-386) | ✅ Merged 2026-04-14 |
| 0017 | [REST /v1/groups/{id}/members + invites slice 1 (GAR-388)](0017-gar-388-rest-v1-members-invites-slice1.md) | [GAR-388](https://linear.app/chatgpt25/issue/GAR-388) | ✅ Merged 2026-04-14 |
| 0018 | [REST /v1/chats + messages slice 1 (GAR-388)](0018-gar-388-rest-v1-chats-messages-slice1.md) | [GAR-388](https://linear.app/chatgpt25/issue/GAR-388) | ✅ Merged 2026-04-14 |
| 0019 | [REST /v1/memory slice 1 (GAR-389)](0019-gar-389-rest-v1-memory-slice1.md) | [GAR-389](https://linear.app/chatgpt25/issue/GAR-389) | ✅ Merged 2026-04-14 |
| 0020 | [REST /v1/tasks slice 1 (GAR-390)](0020-gar-390-rest-v1-tasks-slice1.md) | [GAR-390](https://linear.app/chatgpt25/issue/GAR-390) | ✅ Merged 2026-04-14 |
| 0021 | [Tauri v2 desktop overlay (GAR-411)](0021-gar-411-tauri-desktop.md) | [GAR-411](https://linear.app/chatgpt25/issue/GAR-411) | ✅ Merged 2026-04-14 |
| 0022 | [garraia-telemetry baseline (GAR-384)](0022-gar-384-telemetry-baseline.md) | [GAR-384](https://linear.app/chatgpt25/issue/GAR-384) | ✅ Merged 2026-04-13 (`84c4753`) |
| 0023 | [PoC benchmark Postgres vs SQLite (GAR-373)](0023-gar-373-poc-benchmark.md) | [GAR-373](https://linear.app/chatgpt25/issue/GAR-373) | ✅ Merged 2026-04-13 (PoC efêmero) |
| 0024 | [Migration 014 — tus_uploads (GAR-395 slice 1)](0024-gar-395-migration-014-tus-uploads.md) | [GAR-395](https://linear.app/chatgpt25/issue/GAR-395) | ✅ Merged 2026-04-14 |
| 0025 | [garraia-auth IdentityProvider + LoginPool + endpoints (GAR-391a)](0025-gar-391a-auth-identity-provider.md) | [GAR-391](https://linear.app/chatgpt25/issue/GAR-391) | ✅ Merged 2026-04-13 |
| 0026 | [garraia-auth extractor + RequirePermission (GAR-391b)](0026-gar-391b-auth-extractor.md) | [GAR-391](https://linear.app/chatgpt25/issue/GAR-391) | ✅ Merged 2026-04-13 |
| 0027 | [garraia-auth wiring + SignupPool (GAR-391c)](0027-gar-391c-auth-wiring-signup.md) | [GAR-391](https://linear.app/chatgpt25/issue/GAR-391) | ✅ Merged 2026-04-13 |
| 0028 | [REST /v1/auth/* endpoints (GAR-391a/b/c)](0028-gar-391-rest-v1-auth.md) | [GAR-391](https://linear.app/chatgpt25/issue/GAR-391) | ✅ Merged 2026-04-13 |
| 0029 | [ObjectStore trait + LocalFs (GAR-394 slice 1)](0029-gar-394-object-store-localfs.md) | [GAR-394](https://linear.app/chatgpt25/issue/GAR-394) | ✅ Merged 2026-04-22 |
| 0030 | [S3Compatible ObjectStore (GAR-394 slice 2)](0030-gar-394-s3-compatible.md) | [GAR-394](https://linear.app/chatgpt25/issue/GAR-394) | ✅ Merged 2026-04-22 |
| 0031 | [tus 1.0 Creation endpoint POST /v1/uploads (GAR-395 slice 1)](0031-gar-395-tus-creation.md) | [GAR-395](https://linear.app/chatgpt25/issue/GAR-395) | ✅ Merged 2026-04-22 |
| 0032 | [tus PATCH commit two-phase + StorageConfig (GAR-395 slice 2)](0032-gar-395-tus-patch-commit.md) | [GAR-395](https://linear.app/chatgpt25/issue/GAR-395) | ✅ Merged 2026-04-22 |
| 0033 | [tus DELETE + expiration worker + put_stream (GAR-395 slice 3)](0033-gar-395-tus-delete-worker.md) | [GAR-395](https://linear.app/chatgpt25/issue/GAR-395) | ✅ Merged 2026-04-23 via PR #62 (`96f5c03`) |
| 0034 | [CLI migrate workspace stage 1 (users + identities) (GAR-413)](0034-gar-413-migrate-workspace-stage1.md) | [GAR-413](https://linear.app/chatgpt25/issue/GAR-413) | ✅ Merged 2026-04-13 |
| 0035 | [Config check subcommand (GAR-379 slice 1)](0035-gar-379-config-check-slice1.md) | [GAR-379](https://linear.app/chatgpt25/issue/GAR-379) | ✅ Merged 2026-04-13 |
| 0036 | [SQLite lazy hash upgrade PBKDF2→Argon2id (GAR-382)](0036-gar-382-sqlite-lazy-hash-upgrade.md) | [GAR-382](https://linear.app/chatgpt25/issue/GAR-382) | ✅ Merged 2026-04-13 |
| 0037 | [ObjectStore trait + LocalFs baseline (GAR-394 slice 1)](0037-gar-394-object-store-localfs.md) | [GAR-394](https://linear.app/chatgpt25/issue/GAR-394) | ✅ Merged 2026-04-22 |
| 0038 | [S3Compatible ObjectStore + MinIO (GAR-394 slice 2)](0038-gar-394-s3-compatible.md) | [GAR-394](https://linear.app/chatgpt25/issue/GAR-394) | ✅ Merged 2026-04-22 |
| 0039 | [CLI migrate workspace stage 1 users+identities PHC (GAR-413)](0039-gar-413-migrate-workspace-users.md) | [GAR-413](https://linear.app/chatgpt25/issue/GAR-413) | ✅ Merged 2026-04-22 |
| 0040 | [CLI migrate workspace stage 3 groups+members (GAR-413)](0040-gar-413-migrate-workspace-groups.md) | [GAR-413](https://linear.app/chatgpt25/issue/GAR-413) | ✅ Merged 2026-04-22 |
| 0041 | [tus 1.0 Creation POST /v1/uploads ledger (GAR-395 slice 1)](0041-gar-395-tus-creation-ledger.md) | [GAR-395](https://linear.app/chatgpt25/issue/GAR-395) | ✅ Merged 2026-04-22 |
| 0042 | [Quality Ratchet scaffold PR-1 (GAR-449)](0042-gar-449-quality-ratchet-pr1.md) | [GAR-449](https://linear.app/chatgpt25/issue/GAR-449) | ✅ Merged 2026-04-22 |
| 0043 | [CLI migrate workspace stage 5 chats+chat_members (GAR-413)](0043-gar-413-migrate-workspace-chats.md) | [GAR-413](https://linear.app/chatgpt25/issue/GAR-413) | ✅ Merged 2026-04-22 |
| 0044 | [StorageConfig + tus PATCH commit two-phase (GAR-395 slice 2)](0044-gar-395-storage-config-tus-patch.md) | [GAR-395](https://linear.app/chatgpt25/issue/GAR-395) | ✅ Merged 2026-04-22 |
| 0045 | [CLI migrate workspace stage 5 chats+chat_members (GAR-413 Stage 5)](0045-gar-413-migrate-workspace-chats-stage5.md) | [GAR-413](https://linear.app/chatgpt25/issue/GAR-413) | ✅ Merged 2026-04-22 |
| 0046 | [Auth config AuthSection + fail-closed JWT (GAR-379 slice 3)](0046-gar-379-auth-config-authsection.md) | [GAR-379](https://linear.app/chatgpt25/issue/GAR-379) | ✅ Merged 2026-04-22 via PR #? |
| 0047 | [tus DELETE + expiration worker + put_stream (GAR-395 slice 3)](0047-gar-395-tus-delete-expiration-worker.md) | [GAR-395](https://linear.app/chatgpt25/issue/GAR-395) | ✅ Merged 2026-04-23 via PR #62 (`96f5c03`) |
| 0048 | [CLI migrate workspace stage 5b messages (GAR-413)](0048-gar-413-migrate-workspace-messages.md) | [GAR-413](https://linear.app/chatgpt25/issue/GAR-413) | ✅ Merged 2026-04-23 |
| 0049 | [REST /v1/groups/{group_id}/audit GET (GAR-500)](0049-gar-500-rest-v1-audit.md) | [GAR-500](https://linear.app/chatgpt25/issue/GAR-500) | ✅ Merged 2026-04-23 |
| 0050 | [CI pipeline fix Lote 2 (GAR-438)](0050-gar-438-ci-pipeline-fix-lote2.md) | [GAR-438](https://linear.app/chatgpt25/issue/GAR-438) | ✅ Merged 2026-04-24 via PR #64 (`1828625`) |
| 0051 | [Dev echo LLM provider mock (GAR-444 Lote 3)](0051-gar-444-dev-echo-provider-lote3.md) | [GAR-444](https://linear.app/chatgpt25/issue/GAR-444) | ✅ Merged 2026-04-24 |
| 0052 | [Playwright test fixes data-testid (GAR-443 Lote 4)](0052-gar-443-playwright-lote4-testid.md) | [GAR-443](https://linear.app/chatgpt25/issue/GAR-443) | ✅ Merged 2026-04-24 |
| 0053 | [REST /v1 chats slice 2 (GAR-501)](0053-gar-501-rest-v1-chats-slice2.md) | [GAR-501](https://linear.app/chatgpt25/issue/GAR-501) | ✅ Merged 2026-04-25 |
| 0054 | [REST /v1 chats slice 3 (GAR-506)](0054-gar-506-rest-v1-chats-slice3.md) | [GAR-506](https://linear.app/chatgpt25/issue/GAR-506) | ✅ Merged 2026-04-25 |
| 0055 | [REST /v1 messages slice 2 (GAR-507)](0055-gar-507-rest-v1-messages-slice2.md) | [GAR-507](https://linear.app/chatgpt25/issue/GAR-507) | ✅ Merged 2026-04-25 |
| 0056 | [Q6 mutation triage (GAR-505)](0056-gar-505-q6-mutation-triage.md) | [GAR-505](https://linear.app/chatgpt25/issue/GAR-505) | ✅ Merged 2026-04-25 |
| 0057 | [REST /v1/memory GET+PATCH (GAR-528 slice 3)](0057-gar-528-rest-v1-memory-get-patch.md) | [GAR-528](https://linear.app/chatgpt25/issue/GAR-528) | ✅ Merged 2026-04-29 |
| 0058 | [REST /v1 tasks slice 4: assignees (GAR-533)](0058-gar-533-rest-v1-tasks-assignees.md) | [GAR-533](https://linear.app/chatgpt25/issue/GAR-533) | ✅ Merged 2026-04-30 |
| 0059 | [REST /v1 tasks slice 5: labels (GAR-537)](0059-gar-537-rest-v1-tasks-labels.md) | [GAR-537](https://linear.app/chatgpt25/issue/GAR-537) | ✅ Merged 2026-04-30 |
| 0060 | [REST /v1 tasks slice 6: subscriptions (GAR-539)](0060-gar-539-rest-v1-tasks-subscriptions.md) | [GAR-539](https://linear.app/chatgpt25/issue/GAR-539) | ✅ Merged 2026-04-30 |
| 0061 | [REST /v1 tasks slice 7: activity log (GAR-541)](0061-gar-541-rest-v1-tasks-activity.md) | [GAR-541](https://linear.app/chatgpt25/issue/GAR-541) | ✅ Merged 2026-05-01 |
| 0062 | [REST /v1 tasks slice 8: move task (GAR-544)](0062-gar-544-rest-v1-tasks-move.md) | [GAR-544](https://linear.app/chatgpt25/issue/GAR-544) | ✅ Merged 2026-05-08 via PR #214 (`6232ec1`) |
| 0063 | [REST /v1 tasks slice 9: subtasks (GAR-546)](0063-gar-546-rest-v1-tasks-subtasks.md) | [GAR-546](https://linear.app/chatgpt25/issue/GAR-546) | ✅ Merged 2026-05-08 via PR #217 (`ad394b7`) |
| 0064 | [Quality Ratchet scaffold PR-1 (plan 0064)](0064-quality-ratchet-pr1.md) | [GAR-449](https://linear.app/chatgpt25/issue/GAR-449) | ✅ Merged 2026-04-22 |
| 0065 | [REST /v1 search slice 1: GET /v1/search unified FTS (GAR-549)](0065-gar-549-rest-v1-search-slice1.md) | [GAR-549](https://linear.app/chatgpt25/issue/GAR-549) | ✅ Merged 2026-05-08 via PR #223 (`79199ab`) |
| 0066 | [REST /v1 search slice 2: chat+user scope (GAR-551)](0066-gar-551-rest-v1-search-slice2.md) | [GAR-551](https://linear.app/chatgpt25/issue/GAR-551) | ✅ Merged 2026-05-08 via PR #228 (`fdef9bc`) |
| 0067 | [REST /v1 search slice 3: date+author filters (GAR-552)](0067-gar-552-rest-v1-search-slice3.md) | [GAR-552](https://linear.app/chatgpt25/issue/GAR-552) | ✅ Merged 2026-05-09 via PR #231 (`49c4a6b`) |
| 0068 | [drop aws-sdk-s3 legacy rustls 0.21 chain (GAR-553)](0068-gar-553-drop-aws-sdk-s3-rustls-chain.md) | [GAR-553](https://linear.app/chatgpt25/issue/GAR-553) | ✅ Merged 2026-05-09 via PR #232 (`f69b794`) |
| 0069 | [Files REST API slice 1: GET files + GET folders + DELETE file (GAR-555)](0069-gar-555-files-api-slice1.md) | [GAR-555](https://linear.app/chatgpt25/issue/GAR-555) | ✅ Merged 2026-05-09 via PR #235 (`75d7a44`) |
| 0070 | [Files REST API slice 2: PATCH /v1/groups/{group_id}/files/{file_id} rename (GAR-557)](0070-gar-557-files-api-slice2-rename.md) | [GAR-557](https://linear.app/chatgpt25/issue/GAR-557) | ✅ Merged 2026-05-09 via PR #238 (`9255515`) |
| 0071 | [Files REST API slice 3: GET single file + folder (GAR-559)](0071-gar-559-files-api-slice3-get-single.md) | [GAR-559](https://linear.app/chatgpt25/issue/GAR-559) | ✅ Merged 2026-05-09 via PR #242 (`4adcb02`) |
| 0072 | [Files REST API slice 4: PATCH folder rename (GAR-561)](0072-gar-561-files-api-slice4-folder-rename.md) | [GAR-561](https://linear.app/chatgpt25/issue/GAR-561) | ✅ Merged 2026-05-09 via PR #246 (`3679ccc`) |
| 0073 | [Files REST API slice 5: folder POST + DELETE (GAR-562)](0073-gar-562-files-api-slice5-folder-post-delete.md) | [GAR-562](https://linear.app/chatgpt25/issue/GAR-562) | ✅ Merged 2026-05-09 via PR #247 (`28b3b0f`) |
| 0074 | [Files REST API slice 6: GET /v1/files/{file_id}/download (GAR-564)](0074-gar-564-files-api-slice6-download.md) | [GAR-564](https://linear.app/chatgpt25/issue/GAR-564) | ✅ Merged 2026-05-10 via PR #250 (`b2de161`) |
| 0075 | [Files REST API slice 7: POST /v1/groups/{group_id}/files/{file_id}/versions (GAR-567)](0075-gar-567-files-api-slice7-new-version.md) | [GAR-567](https://linear.app/chatgpt25/issue/GAR-567) | ✅ Merged 2026-05-10 via PR #254 (`5b0c5fd`) |
| 0076 | [Files REST API slice 8: GET /v1/groups/{group_id}/files/{file_id}/versions (GAR-569)](0076-gar-569-files-api-slice8-list-versions.md) | [GAR-569](https://linear.app/chatgpt25/issue/GAR-569) | ✅ Merged 2026-05-10 via PR #253 (`0cc9a85`) |
| 0077 | [GAR-572 — Tasks REST API slice 9: task_attachments (migration 017 + POST/GET/DELETE)](0077-gar-572-task-attachments-api.md) | [GAR-572](https://linear.app/chatgpt25/issue/GAR-572) | ✅ Merged 2026-05-10 via PR #257 (`2c1460c`) |
| 0078 | [REST /v1 tasks slice 4b: assignees API (GAR-533)](0078-gar-533-rest-v1-tasks-assignees-api.md) | [GAR-533](https://linear.app/chatgpt25/issue/GAR-533) | ✅ Merged 2026-04-30 |
| 0079 | [REST /v1 tasks slice 5b: labels API (GAR-537)](0079-gar-537-rest-v1-tasks-labels-api.md) | [GAR-537](https://linear.app/chatgpt25/issue/GAR-537) | ✅ Merged 2026-04-30 |
| 0080 | [REST /v1 tasks slice 6b: subscriptions API (GAR-539)](0080-gar-539-rest-v1-tasks-subscriptions-api.md) | [GAR-539](https://linear.app/chatgpt25/issue/GAR-539) | ✅ Merged 2026-04-30 |
| 0081 | [REST /v1 tasks slice 7b: activity log API (GAR-541)](0081-gar-541-rest-v1-tasks-activity-api.md) | [GAR-541](https://linear.app/chatgpt25/issue/GAR-541) | ✅ Merged 2026-05-01 |
| 0082 | [GAR-544 — REST `/v1` tasks slice 8 (move task between lists)](0082-gar-544-task-move-api.md) | [GAR-544](https://linear.app/chatgpt25/issue/GAR-544) | ✅ Merged 2026-05-08 via PR #214 (`6232ec1`) |
| 0083 | [GAR-546 — REST `/v1` tasks slice 9 (subtasks API)](0083-gar-546-task-subtasks-api.md) | [GAR-546](https://linear.app/chatgpt25/issue/GAR-546) | ✅ Merged 2026-05-08 via PR #217 (`ad394b7`) |
| 0084 | [GAR-549 — REST `/v1` search slice 1: GET /v1/search unified FTS](0084-gar-549-search-api-slice1.md) | [GAR-549](https://linear.app/chatgpt25/issue/GAR-549) | ✅ Merged 2026-05-08 via PR #223 (`79199ab`) |
| 0085 | [GAR-551 — REST `/v1` search slice 2: chat + user scope](0085-gar-551-search-api-slice2-chat-user-scope.md) | [GAR-551](https://linear.app/chatgpt25/issue/GAR-551) | ✅ Merged 2026-05-08 via PR #228 (`fdef9bc`) |
| 0086 | [GAR-552 — REST `/v1` search slice 3: from_date/to_date/author_id filters](0086-gar-552-search-api-slice3-date-author-filters.md) | [GAR-552](https://linear.app/chatgpt25/issue/GAR-552) | ✅ Merged 2026-05-09 via PR #231 (`49c4a6b`) |
| 0087 | [GAR-553 — drop aws-sdk-s3 legacy rustls 0.21 chain (defense-in-depth)](0087-gar-553-aws-sdk-s3-drop-legacy-rustls-chain.md) | [GAR-553](https://linear.app/chatgpt25/issue/GAR-553) | ✅ Merged 2026-05-09 via PR #232 (`f69b794`) |
| 0088 | [GAR-555 — Files REST API slice 1: GET files + GET folders + DELETE file](0088-gar-555-files-api-slice1.md) | [GAR-555](https://linear.app/chatgpt25/issue/GAR-555) | ✅ Merged 2026-05-09 via PR #235 (`75d7a44`) |
| 0089 | [GAR-557 — Files REST API slice 2: PATCH /v1/groups/{group_id}/files/{file_id} (rename)](0089-gar-557-files-api-slice2-rename.md) | [GAR-557](https://linear.app/chatgpt25/issue/GAR-557) | ✅ Merged 2026-05-09 via PR #238 (`9255515`) |
| 0090 | [GAR-559 — Files REST API slice 3: GET single file + folder](0090-gar-559-files-api-slice3-get-single.md) | [GAR-559](https://linear.app/chatgpt25/issue/GAR-559) | ✅ Merged 2026-05-09 via PR #242 (`4adcb02`) |
| 0091 | [GAR-561 — Files REST API slice 4: PATCH folder rename](0091-gar-561-files-api-slice4-folder-rename.md) | [GAR-561](https://linear.app/chatgpt25/issue/GAR-561) | ✅ Merged 2026-05-09 via PR #246 (`3679ccc`) |
| 0092 | [GAR-562 — Files REST API slice 5: folder POST + DELETE](0092-gar-562-files-api-slice5-folder-post-delete.md) | [GAR-562](https://linear.app/chatgpt25/issue/GAR-562) | ✅ Merged 2026-05-09 via PR #247 (`28b3b0f`) |
| 0093 | [GAR-564 — Files REST API slice 6: GET /v1/files/{file_id}/download](0093-gar-564-files-api-slice6-download.md) | [GAR-564](https://linear.app/chatgpt25/issue/GAR-564) | ✅ Merged 2026-05-10 via PR #250 (`b2de161`) |
| 0094 | [GAR-567 — Files REST API slice 7: POST /v1/groups/{group_id}/files/{file_id}/versions (new version)](0094-gar-565-files-api-slice7-new-version.md) | [GAR-567](https://linear.app/chatgpt25/issue/GAR-567) | ✅ Merged 2026-05-10 via PR #254 (`5b0c5fd`) |
| 0095 | [GAR-569 — Files REST API slice 8: GET /v1/groups/{group_id}/files/{file_id}/versions (list versions)](0095-gar-569-files-api-slice8-list-versions.md) | [GAR-569](https://linear.app/chatgpt25/issue/GAR-569) | ✅ Merged 2026-05-10 via PR #253 (`0cc9a85`) |
| 0096 | [GAR-572 — Tasks REST API slice 9: task_attachments (migration 017 + POST/GET/DELETE)](0096-gar-572-task-attachments-api.md) | [GAR-572](https://linear.app/chatgpt25/issue/GAR-572) | ✅ Merged 2026-05-10 via PR #257 (`2c1460c`) |
| 0097 | [GAR-574 — Groups REST API slice 2: GET /v1/groups/{id}/members + GET /v1/groups/{id}/invites](0097-gar-574-groups-members-invites-api.md) | [GAR-574](https://linear.app/chatgpt25/issue/GAR-574) | 🔵 Em execução 2026-05-11 |

## Arquivos não-versionados

Drafts ad-hoc dentro de `plans/` que **não** sigam o padrão `NNNN-*.md` ficam gitignored por design — ver `.gitignore`. Útil para rascunhos pessoais antes de formalizar um plano numerado.
