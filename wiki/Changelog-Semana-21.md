# GarraIA Changelog — Semana 21 (18/05 a 24/05)

## Destaques

A semana 21 é a semana de **fechamento do epic GAR-391** (Identity + RBAC + RLS + cross-group authz HTTP). Com a superfície REST /v1 da Fase 3.4 fechada na semana 20 (Files/Tasks/Groups/Memory/Chats), o GAR-391d (≥100 cenários cross-group authz via HTTP) finalmente destrava — ele estava bloqueado pela ausência desses endpoints. Em paralelo, os endpoints `/v1/me` e `/v1/groups/{id}/storage-usage` entram em curso como "últimos detalhes" antes do beta da Fase 3.

Em uma linha: **as 4 camadas de prova de tenant isolation (auth, RBAC, RLS SQL, cross-group HTTP) fecham nesta semana → Fase 3 pronta para beta.**

## Cross-group authz via HTTP (GAR-391d)

- **[GAR-391d](https://linear.app/chatgpt25/issue/GAR-391d)** — suite end-to-end de cross-group authz via HTTP, ≥100 cenários. Cada cenário sobe Postgres real via testcontainers (~7s), autentica via `/v1/auth/login` real, monta JWT real, faz request HTTP real contra recurso de outro grupo. Asserção: status `403 Forbidden` em todos os pares `(usuário do grupo A, recurso do grupo B)`. Cobertura matricial sobre `/v1/{files,tasks,groups,memory,chats}`. Fecha o epic GAR-391.

## Endpoints finais da Fase 3 (REST /v1)

- **`/v1/me`** — endpoint canônico do usuário autenticado: id, email, roles agregadas por grupo, `last_login_at`, `hash_upgraded_at` (transparência sobre o estado do lazy upgrade Argon2id).
- **`/v1/groups/{id}/storage-usage`** — cota de storage por grupo, agregado de `files` + `file_versions`. Útil para enforcement futuro de quota (Fase 3.5) e para o app mobile mostrar gauge de uso.

## Mobile (Flutter)

- Integração da Files API completa no upload flow do app Garra Cloud Alpha. Validação de resumable uploads em rede mobile flaky (Brasil, 4G), com retry inteligente sobre TUS 1.0.

## Health & Security

- Health-routine semanal — verificação de todas as surfaces (gateway, mobile, mcp, voice). Dependabot rotation conforme calendário SRE.

## Próximos passos (Fase 3 → beta)

- Fechado GAR-391d → epic GAR-391 fecha → Fase 3 entra em beta candidate.
- Fase 3.5 (quota enforcement, billing rails opcional, plan limits).
- Documentação pública de migração: como subir um GarraIA self-hosted para um time brasileiro, com checklist LGPD.

---

Repositório: https://github.com/michelbr84/GarraRUST
Roadmap: https://github.com/michelbr84/GarraRUST/blob/main/ROADMAP.md
Linear: https://linear.app/chatgpt25/team/GAR/all
