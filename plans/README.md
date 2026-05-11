# Plans — índice de planos de implementação

Cada plano documenta um slice de funcionalidade ou tarefa de engenharia.
O número é sequencial e permanente — nunca reutilizado.

## Planos

| # | Título | Issue Linear | Status |
|---|--------|-------------|--------|
| 0001 | [Plano inicial — estrutura do projeto](0001-estrutura-inicial.md) | — | ✅ Implementado |
| 0002 | [CLI: subcomando `garraia start`](0002-cli-start.md) | — | ✅ Implementado |
| 0003 | [Canal Telegram](0003-canal-telegram.md) | — | ✅ Implementado |
| 0004 | [Canal Discord](0004-canal-discord.md) | — | ✅ Implementado |
| 0005 | [Canal Slack](0005-canal-slack.md) | — | ✅ Implementado |
| 0006 | [Canal WhatsApp (Baileys)](0006-canal-whatsapp.md) | — | ✅ Implementado |
| 0007 | [Canal iMessage (BlueBubbles)](0007-canal-imessage.md) | — | ✅ Implementado |
| 0008 | [STT Whisper local](0008-stt-whisper.md) | — | ✅ Implementado |
| 0009 | [TTS Chatterbox/ElevenLabs/Kokoro](0009-tts-chatterbox.md) | — | ✅ Implementado |
| 0010 | [PoC benchmark Postgres vs SQLite](0010-poc-benchmark.md) | [GAR-373](https://linear.app/chatgpt25/issue/GAR-373) | ✅ Entregue (PoC efêmero) |
| 0011 | [Workspace Postgres multi-tenant (schema + migrations)](0011-workspace-postgres.md) | [GAR-407](https://linear.app/chatgpt25/issue/GAR-407) | ✅ Merged |
| 0011.5 | [user_identities.hash_upgraded_at (GAR-391b prereq)](0011.5-hash-upgraded-at.md) | [GAR-391b](https://linear.app/chatgpt25/issue/GAR-391) | ✅ Merged |
| 0012 | [Auth endpoints + extractor + wiring (GAR-391a/b/c)](0012-gar-391c-extractor-and-wiring.md) | [GAR-391](https://linear.app/chatgpt25/issue/GAR-391) | ✅ Merged |
| 0013 | [RLS matrix pura (GAR-392, plan C)](0013-gar-392-rls-matrix.md) | [GAR-392](https://linear.app/chatgpt25/issue/GAR-392) | ✅ Merged |
| 0014 | [App-layer cross-group authz via HTTP (GAR-391d)](0014-gar-391d-app-layer-authz.md) | [GAR-391](https://linear.app/chatgpt25/issue/GAR-391) | ✅ Merged |
| 0015 | [REST /v1 foundation: OpenAPI + Swagger UI + /me (GAR-386)](0015-gar-386-rest-v1-foundation.md) | [GAR-386](https://linear.app/chatgpt25/issue/GAR-386) | ✅ Merged |
| 0016 | [REST /v1/groups CRUD (GAR-388)](0016-gar-388-rest-v1-groups.md) | [GAR-388](https://linear.app/chatgpt25/issue/GAR-388) | ✅ Merged |
| 0017 | [REST /v1/groups/{id}/members CRUD (GAR-389)](0017-gar-389-rest-v1-group-members.md) | [GAR-389](https://linear.app/chatgpt25/issue/GAR-389) | ✅ Merged |
| 0018 | [REST /v1/groups/{id}/invites (POST + validate) (GAR-390)](0018-gar-390-rest-v1-group-invites.md) | [GAR-390](https://linear.app/chatgpt25/issue/GAR-390) | ✅ Merged |
| 0019 | [REST /v1/invites/{token}/accept (GAR-393)](0019-gar-393-rest-v1-invite-accept.md) | [GAR-393](https://linear.app/chatgpt25/issue/GAR-393) | ✅ Merged |
| 0020 | [REST /v1/me PATCH (profile update) (GAR-408)](0020-gar-408-rest-v1-me-patch.md) | [GAR-408](https://linear.app/chatgpt25/issue/GAR-408) | ✅ Merged |
| 0021 | [Audit events API (GAR-409)](0021-gar-409-audit-events.md) | [GAR-409](https://linear.app/chatgpt25/issue/GAR-409) | ✅ Merged |
| 0022 | [Rate limiting + IP trust (GAR-426)](0022-gar-426-rate-limiting.md) | [GAR-426](https://linear.app/chatgpt25/issue/GAR-426) | ✅ Merged |
| 0023 | [Tauri v2 desktop overlay (GAR-411)](0023-gar-411-tauri-desktop.md) | [GAR-411](https://linear.app/chatgpt25/issue/GAR-411) | ✅ Merged |
| 0024 | [Prometheus /metrics endpoint (GAR-412)](0024-gar-412-prometheus-metrics.md) | [GAR-412](https://linear.app/chatgpt25/issue/GAR-412) | ✅ Merged |
| 0025 | [REST /v1/chats (POST + GET) (GAR-413)](0025-gar-413-rest-v1-chats.md) | [GAR-413](https://linear.app/chatgpt25/issue/GAR-413) | ✅ Merged |
| 0026 | [REST /v1/chats/{id}/messages (POST + GET) (GAR-414)](0026-gar-414-rest-v1-messages.md) | [GAR-414](https://linear.app/chatgpt25/issue/GAR-414) | ✅ Merged |
| 0027 | [REST /v1/memory (POST + GET + DELETE) (GAR-415)](0027-gar-415-rest-v1-memory.md) | [GAR-415](https://linear.app/chatgpt25/issue/GAR-415) | ✅ Merged |
| 0028 | [REST /v1/tasks (CRUD task-lists + tasks) (GAR-416)](0028-gar-416-rest-v1-tasks.md) | [GAR-416](https://linear.app/chatgpt25/issue/GAR-416) | ✅ Merged |
| 0029 | [REST /v1/search (FTS unified) (GAR-417)](0029-gar-417-rest-v1-search.md) | [GAR-417](https://linear.app/chatgpt25/issue/GAR-417) | ✅ Merged |
| 0030 | [CI pipeline fixes (GAR-438)](0030-gar-438-ci-pipeline-fixes.md) | [GAR-438](https://linear.app/chatgpt25/issue/GAR-438) | ✅ Merged |
| 0031 | [Playwright test fixes (GAR-443)](0031-gar-443-playwright-test-fixes.md) | [GAR-443](https://linear.app/chatgpt25/issue/GAR-443) | ✅ Merged |
| 0032 | [Dev echo LLM provider (GAR-444)](0032-gar-444-dev-echo-provider.md) | [GAR-444](https://linear.app/chatgpt25/issue/GAR-444) | ✅ Merged |
| 0033 | [MSRV + cargo-deny (GAR-445)](0033-gar-445-msrv-cargo-deny.md) | [GAR-445](https://linear.app/chatgpt25/issue/GAR-445) | ✅ Merged |
| 0034 | [CLI migrate workspace (GAR-413 Stage 1)](0034-gar-413-migrate-workspace-stage1.md) | [GAR-413](https://linear.app/chatgpt25/issue/GAR-413) | ✅ Merged |
| 0035 | [Config check subcommand (GAR-379 slice 1)](0035-gar-379-config-check-slice1.md) | [GAR-379](https://linear.app/chatgpt25/issue/GAR-379) | ✅ Merged |
| 0036 | [SQLite lazy hash upgrade (GAR-382)](0036-gar-382-sqlite-lazy-hash-upgrade.md) | [GAR-382](https://linear.app/chatgpt25/issue/GAR-382) | ✅ Merged |
| 0037 | [ObjectStore trait + LocalFs (GAR-394 slice 1)](0037-gar-394-object-store-localfs.md) | [GAR-394](https://linear.app/chatgpt25/issue/GAR-394) | ✅ Merged |
| 0038 | [S3Compatible ObjectStore (GAR-394 slice 2)](0038-gar-394-s3-compatible.md) | [GAR-394](https://linear.app/chatgpt25/issue/GAR-394) | ✅ Merged |
| 0039 | [CLI migrate workspace stage 1 (users + identities) (GAR-413)](0039-gar-413-migrate-workspace-users.md) | [GAR-413](https://linear.app/chatgpt25/issue/GAR-413) | ✅ Merged |
| 0040 | [CLI migrate workspace stage 3 (groups + members) (GAR-413)](0040-gar-413-migrate-workspace-groups.md) | [GAR-413](https://linear.app/chatgpt25/issue/GAR-413) | ✅ Merged |
| 0041 | [tus 1.0 Creation endpoint POST /v1/uploads (GAR-395 slice 1)](0041-gar-395-tus-creation.md) | [GAR-395](https://linear.app/chatgpt25/issue/GAR-395) | ✅ Merged |
| 0042 | [Quality Ratchet scaffold PR-1 (GAR-449)](0042-gar-449-quality-ratchet-pr1.md) | [GAR-449](https://linear.app/chatgpt25/issue/GAR-449) | ✅ Merged |
| 0043 | [CLI migrate workspace stage 5 (chats + chat_members) (GAR-413)](0043-gar-413-migrate-workspace-chats.md) | [GAR-413](https://linear.app/chatgpt25/issue/GAR-413) | ✅ Merged |
| 0044 | [StorageConfig + tus PATCH commit two-phase (GAR-395 slice 2)](0044-gar-395-tus-patch-commit.md) | [GAR-395](https://linear.app/chatgpt25/issue/GAR-395) | ✅ Merged |
| 0045 | [Auth config AuthSection + fail-closed JWT (GAR-379 slice 3)](0045-gar-379-auth-config-slice3.md) | [GAR-379](https://linear.app/chatgpt25/issue/GAR-379) | ✅ Merged |
| 0046 | [StorageConfig validation (GAR-395 slice 2 — config check ext)](0046-gar-395-storage-config-validation.md) | [GAR-395](https://linear.app/chatgpt25/issue/GAR-395) | ✅ Merged |
| 0047 | [tus DELETE + expiration worker + put_stream (GAR-395 slice 3)](0047-gar-395-tus-delete-worker.md) | [GAR-395](https://linear.app/chatgpt25/issue/GAR-395) | ✅ Merged |
| 0048 | [CLI migrate workspace stage 5b (messages) (GAR-413)](0048-gar-413-migrate-workspace-messages.md) | [GAR-413](https://linear.app/chatgpt25/issue/GAR-413) | ✅ Merged |
| 0049 | [REST /v1/audit (GET /v1/groups/{group_id}/audit) (GAR-500)](0049-gar-500-rest-v1-audit.md) | [GAR-500](https://linear.app/chatgpt25/issue/GAR-500) | ✅ Merged |
| 0050 | [CI pipeline fix Lote 2 (GAR-438)](0050-gar-438-ci-pipeline-fix-lote2.md) | [GAR-438](https://linear.app/chatgpt25/issue/GAR-438) | ✅ Merged |
| 0051 | [Dev echo provider (GAR-444 Lote 3)](0051-gar-444-dev-echo-provider-lote3.md) | [GAR-444](https://linear.app/chatgpt25/issue/GAR-444) | ✅ Merged |
| 0052 | [Playwright test fixes Lote 4 (GAR-443)](0052-gar-443-playwright-lote4.md) | [GAR-443](https://linear.app/chatgpt25/issue/GAR-443) | ✅ Merged |
| 0053 | [Chat management + member CRUD (GAR-501)](0053-gar-501-chat-management.md) | [GAR-501](https://linear.app/chatgpt25/issue/GAR-501) | ✅ Merged |
| 0054 | [REST /v1 chats slice 1: POST + GET (GAR-506)](0054-gar-506-chats-slice1.md) | [GAR-506](https://linear.app/chatgpt25/issue/GAR-506) | ✅ Merged |
| 0055 | [REST /v1 messages slice 2: POST + GET (GAR-507)](0055-gar-507-messages-slice2.md) | [GAR-507](https://linear.app/chatgpt25/issue/GAR-507) | ✅ Merged |
| 0056 | [Q6 mutation triage (GAR-505)](0056-gar-505-q6-mutation-triage.md) | [GAR-505](https://linear.app/chatgpt25/issue/GAR-505) | ✅ Merged |
| 0057 | [REST /v1/memory GET + PATCH (GAR-528)](0057-gar-528-memory-get-patch.md) | [GAR-528](https://linear.app/chatgpt25/issue/GAR-528) | ✅ Merged |
| 0058 | [Tasks slice 4: task assignees (GAR-533)](0058-gar-533-task-assignees.md) | [GAR-533](https://linear.app/chatgpt25/issue/GAR-533) | ✅ Merged |
| 0059 | [Tasks slice 5: task labels (GAR-537)](0059-gar-537-task-labels.md) | [GAR-537](https://linear.app/chatgpt25/issue/GAR-537) | ✅ Merged |
| 0060 | [Tasks slice 6: task subscriptions (GAR-539)](0060-gar-539-task-subscriptions.md) | [GAR-539](https://linear.app/chatgpt25/issue/GAR-539) | ✅ Merged |
| 0061 | [Tasks slice 7: task activity log (GAR-541)](0061-gar-541-task-activity.md) | [GAR-541](https://linear.app/chatgpt25/issue/GAR-541) | ✅ Merged |
| 0062 | [Tasks slice 8: task move (GAR-544)](0062-gar-544-task-move.md) | [GAR-544](https://linear.app/chatgpt25/issue/GAR-544) | ✅ Merged |
| 0063 | [Tasks slice 3: subtasks (GAR-546)](0063-gar-546-subtasks.md) | [GAR-546](https://linear.app/chatgpt25/issue/GAR-546) | ✅ Merged |
| 0064 | [Quality Ratchet PR-1 scaffold (GAR-449)](0064-quality-ratchet-pr1.md) | [GAR-449](https://linear.app/chatgpt25/issue/GAR-449) | ✅ Merged |
| 0065 | [Tasks slice 1: task-lists + tasks CRUD (GAR-516)](0065-gar-516-tasks-slice1.md) | [GAR-516](https://linear.app/chatgpt25/issue/GAR-516) | ✅ Merged |
| 0066 | [FTS unified search (GAR-549)](0066-gar-549-fts-search.md) | [GAR-549](https://linear.app/chatgpt25/issue/GAR-549) | ✅ Merged |
| 0067 | [Memory API slice 1: GET/POST/DELETE (GAR-514)](0067-gar-514-memory-slice1.md) | [GAR-514](https://linear.app/chatgpt25/issue/GAR-514) | ✅ Merged |
| 0068 | [Audit API slice 1: GET /v1/groups/{group_id}/audit (GAR-522)](0068-gar-522-audit-slice1.md) | [GAR-522](https://linear.app/chatgpt25/issue/GAR-522) | ✅ Merged |
| 0069 | [Chat mgmt: individual chat ops + member CRUD (GAR-530)](0069-gar-530-chat-mgmt.md) | [GAR-530](https://linear.app/chatgpt25/issue/GAR-530) | ✅ Merged |
| 0070 | [REST /v1/groups/{group_id}/audit (GAR-522 slice 1)](0070-gar-522-audit-slice1-groups.md) | [GAR-522](https://linear.app/chatgpt25/issue/GAR-522) | ✅ Merged |
| 0071 | [Files REST API slice 1: POST /v1/groups/{group_id}/files (GAR-553)](0071-gar-553-files-api-slice1.md) | [GAR-553](https://linear.app/chatgpt25/issue/GAR-553) | ✅ Merged |
| 0072 | [Files REST API slice 2: GET /v1/groups/{group_id}/files (GAR-555)](0072-gar-555-files-api-slice2-list.md) | [GAR-555](https://linear.app/chatgpt25/issue/GAR-555) | ✅ Merged |
| 0073 | [Files REST API slice 3: GET /v1/files/{file_id} (GAR-556)](0073-gar-556-files-api-slice3-get.md) | [GAR-556](https://linear.app/chatgpt25/issue/GAR-556) | ✅ Merged |
| 0074 | [REST /v1/memory GET + PATCH /v1/memory/{id} (GAR-528 slice 3)](0074-gar-528-memory-get-patch-slice3.md) | [GAR-528](https://linear.app/chatgpt25/issue/GAR-528) | ✅ Merged |
| 0075 | [Files REST API slice 4: DELETE /v1/files/{file_id} (GAR-559)](0075-gar-559-files-api-slice4-delete.md) | [GAR-559](https://linear.app/chatgpt25/issue/GAR-559) | ✅ Merged |
| 0076 | [Chat mgmt: individual chat ops + member CRUD (GAR-530 slice 2)](0076-gar-530-chat-mgmt-slice2.md) | [GAR-530](https://linear.app/chatgpt25/issue/GAR-530) | ✅ Merged |
| 0077 | [Tasks slice 4: task assignees API (GAR-533)](0077-gar-533-task-assignees-api.md) | [GAR-533](https://linear.app/chatgpt25/issue/GAR-533) | ✅ Merged |
| 0078 | [Tasks slice 5: task labels API (GAR-537)](0078-gar-537-task-labels-api.md) | [GAR-537](https://linear.app/chatgpt25/issue/GAR-537) | ✅ Merged |
| 0079 | [Tasks slice 6: task subscriptions API (GAR-539)](0079-gar-539-task-subscriptions-api.md) | [GAR-539](https://linear.app/chatgpt25/issue/GAR-539) | ✅ Merged |
| 0080 | [Tasks slice 7: task activity log API (GAR-541)](0080-gar-541-task-activity-api.md) | [GAR-541](https://linear.app/chatgpt25/issue/GAR-541) | ✅ Merged |
| 0081 | [Tasks slice 8: subtasks API (GAR-546)](0081-gar-546-subtasks-api.md) | [GAR-546](https://linear.app/chatgpt25/issue/GAR-546) | ✅ Merged |
| 0082 | [Tasks slice 8b: task move (GAR-544)](0082-gar-544-task-move.md) | [GAR-544](https://linear.app/chatgpt25/issue/GAR-544) | ✅ Merged |
| 0083 | [Tasks slice 9a: subtasks API (parent_task_id + GET subtasks) (GAR-546)](0083-gar-546-subtasks-api-slice9a.md) | [GAR-546](https://linear.app/chatgpt25/issue/GAR-546) | ✅ Merged |
| 0084 | [FTS unified search GET /v1/search (GAR-549)](0084-gar-549-fts-search.md) | [GAR-549](https://linear.app/chatgpt25/issue/GAR-549) | ✅ Merged |
| 0085 | [Files REST API slice 1b: POST /v1/groups/{group_id}/files (folder support) (GAR-553)](0085-gar-553-files-api-slice1b.md) | [GAR-553](https://linear.app/chatgpt25/issue/GAR-553) | ✅ Merged |
| 0086 | [Files REST API slice 2b: GET /v1/groups/{group_id}/files list (GAR-555)](0086-gar-555-files-api-slice2b.md) | [GAR-555](https://linear.app/chatgpt25/issue/GAR-555) | ✅ Merged |
| 0087 | [Files REST API slice 3b: GET /v1/files/{file_id} (GAR-556)](0087-gar-556-files-api-slice3b.md) | [GAR-556](https://linear.app/chatgpt25/issue/GAR-556) | ✅ Merged |
| 0088 | [Files REST API slice 4b: DELETE /v1/files/{file_id} (GAR-559)](0088-gar-559-files-api-slice4b.md) | [GAR-559](https://linear.app/chatgpt25/issue/GAR-559) | ✅ Merged |
| 0089 | [Files REST API slice 5: PATCH /v1/groups/{group_id}/files/{file_id} (GAR-557)](0089-gar-557-files-api-slice5-patch.md) | [GAR-557](https://linear.app/chatgpt25/issue/GAR-557) | ✅ Merged |
| 0090 | [Files REST API slice 5b: folder POST + DELETE (GAR-562)](0090-gar-562-files-api-slice5b-folder.md) | [GAR-562](https://linear.app/chatgpt25/issue/GAR-562) | ✅ Merged |
| 0091 | [Files REST API slice 5c: PATCH /v1/groups/{group_id}/files/{file_id} (GAR-557 slice 2)](0091-gar-557-files-api-slice5c-patch.md) | [GAR-557](https://linear.app/chatgpt25/issue/GAR-557) | ✅ Merged |
| 0092 | [GAR-562 — Files REST API slice 5: folder POST + DELETE](0092-gar-562-files-api-slice5-folder-post-delete.md) | [GAR-562](https://linear.app/chatgpt25/issue/GAR-562) | ✅ Merged 2026-05-09 via PR #247 (`28b3b0f`) |
| 0093 | [GAR-564 — Files REST API slice 6: GET /v1/files/{file_id}/download](0093-gar-564-files-api-slice6-download.md) | [GAR-564](https://linear.app/chatgpt25/issue/GAR-564) | ✅ Merged 2026-05-10 via PR #250 (`b2de161`) |
| 0094 | [GAR-567 — Files REST API slice 7: POST /v1/groups/{group_id}/files/{file_id}/versions (new version)](0094-gar-565-files-api-slice7-new-version.md) | [GAR-567](https://linear.app/chatgpt25/issue/GAR-567) | ✅ Merged 2026-05-10 via PR #254 (`5b0c5fd`) |
| 0095 | [GAR-569 — Files REST API slice 8: GET /v1/groups/{group_id}/files/{file_id}/versions (list versions)](0095-gar-569-files-api-slice8-list-versions.md) | [GAR-569](https://linear.app/chatgpt25/issue/GAR-569) | ✅ Merged 2026-05-10 via PR #253 (`0cc9a85`) |
| 0096 | [GAR-572 — Tasks REST API slice 9: task_attachments (migration 017 + POST/GET/DELETE)](0096-gar-572-task-attachments-api.md) | [GAR-572](https://linear.app/chatgpt25/issue/GAR-572) | ✅ Merged 2026-05-10 via PR #257 (`2c1460c`) |
| 0097 | [GAR-574 — Groups REST API slice 2: GET /v1/groups/{id}/members + GET /v1/groups/{id}/invites](0097-gar-574-groups-members-invites-api.md) | [GAR-574](https://linear.app/chatgpt25/issue/GAR-574) | 🔵 Em execução 2026-05-11 |

## Arquivos não-versionados

Drafts ad-hoc dentro de `plans/` que **não** sigam o padrão `NNNN-*.md` ficam gitignored por design — ver `.gitignore`. Útil para rascunhos pessoais antes de formalizar um plano numerado.
