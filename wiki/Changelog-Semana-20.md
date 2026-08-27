# GarraIA Changelog — Semana 20 (11/05 a 17/05)

## Destaques

A semana 20 foi a semana em que a **superfície REST /v1 da Fase 3 (Group Workspace) fechou**. A Files API completou os 9 slices entre `GET /v1/groups/{group_id}/files` (slice 1) e `POST /v1/groups/{group_id}/files` (slice 9, upload direto). Em paralelo, Tasks ganhou `task_attachments` com migration 017 e RLS FORCE via JOIN policy, e Groups ganhou members + invites read endpoints. A CLI ganhou fix do default OpenRouter model.

Em uma linha: **`/v1/{files,tasks,groups,memory,chats}` está virtualmente completo — falta apenas a suite cross-group authz (GAR-391d) para fechar o epic GAR-391 e liberar Fase 3 beta.**

## Files API — 9 slices completos

- **[GAR-555](https://linear.app/chatgpt25/issue/GAR-555)** — slice 1: `GET /v1/groups/{group_id}/files` + `GET /v1/groups/{group_id}/folders` + `DELETE /v1/files/{file_id}` (soft-delete). Cursor keyset pagination, RLS FORCE via `set_config('app.current_group_id', $1, true)`.
- **[GAR-562](https://linear.app/chatgpt25/issue/GAR-562)** — slice 5: `POST /v1/groups/{group_id}/folders` + `DELETE /v1/groups/{group_id}/folders/{folder_id}` (soft-delete). Plan 0092.
- **[GAR-564](https://linear.app/chatgpt25/issue/GAR-564)** — slice 6: `GET /v1/files/{file_id}/download` — streaming binário com `Content-Type` + `Content-Disposition: attachment`. Plan 0093.
- **[GAR-567](https://linear.app/chatgpt25/issue/GAR-567)** — slice 7: `POST /v1/groups/{group_id}/files/{file_id}/versions` — uploads de nova versão. Plan 0094.
- **[GAR-569](https://linear.app/chatgpt25/issue/GAR-569)** — slice 8: `GET /v1/groups/{group_id}/files/{file_id}/versions` — list paginado (newest first). Plan 0095.
- **[GAR-577](https://linear.app/chatgpt25/issue/GAR-577)** — slice 9: `POST /v1/groups/{group_id}/files` — direct file upload (criação). Plan 0099, PR #264, commit `725cf54`.

## Task attachments — superfície + schema

- **[GAR-572](https://linear.app/chatgpt25/issue/GAR-572)** — migration 017 (`task_attachments` table com PK `task_id+file_id`, `group_id` denormalizada, `attached_by ON DELETE SET NULL`, `attached_by_label` cache para LGPD erasure survival). Dois novos audit variants: `TaskFileAttached` / `TaskFileDetached`. FORCE RLS via JOIN policy sobre `tasks`. Três endpoints: `POST/GET/DELETE /v1/groups/{group_id}/tasks/{task_id}/attachments`. Plan 0096.

## Groups API — slice 2 fechou

- **[GAR-574](https://linear.app/chatgpt25/issue/GAR-574)** — `GET /v1/groups/{id}/members` + `GET /v1/groups/{id}/invites` com cursor pagination + filtros (`role`, `status`). Qualquer membro ativo pode ler. Plan 0097.

## CLI

- **[GAR-576](https://linear.app/chatgpt25/issue/GAR-576)** — `garra chat --provider openrouter` agora respeita o `config.llm["openrouter"].model` em vez de hardcodar `openrouter/auto`. Fix do autodetect chain `Ollama → Anthropic → OpenAI → OpenRouter` que era sequestrado por `OPENAI_API_KEY` stale no `.env` da cwd.

## Health & Security

- **[GAR-575](https://linear.app/chatgpt25/issue/GAR-575)** + **[GAR-578](https://linear.app/chatgpt25/issue/GAR-578)** — health-routine 2026-05-11 (2 runs, all surfaces green). Dependabot baseline: 8 open (2 HIGH, 2 MEDIUM, 4 LOW), todos tracked.

## Próximos passos

- **GAR-391d** — suite cross-group authz via HTTP (≥100 cenários) — fecha o epic GAR-391 e libera a Fase 3 para beta.
- Endpoints `/v1/me` e `/v1/groups/{id}/storage-usage` (cota por grupo).
- Mobile (Flutter): integrar a Files API completa no upload flow do app, validar resumable uploads com rede flaky.

---

Repositório: https://github.com/michelbr84/GarraRUST
Roadmap: https://github.com/michelbr84/GarraRUST/blob/main/ROADMAP.md
Linear: https://linear.app/chatgpt25/team/GAR/all
