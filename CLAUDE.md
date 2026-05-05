# GarraIA — Gateway de IA Multi-Canal

> Rust-based AI gateway: multi-channel, multi-provider LLM orchestration with mobile client.
> **Harness:** ClaudeMaxPower (branded localmente como "GarraIA SuperPowers") + official Superpowers plugin — hooks, agent teams, quality gates e subagents são parte do workflow obrigatório (ver `skills/`, `.claude/agents/`, `.claude/hooks/`).

## Identidade do Projeto

- **Nome:** GarraIA (GarraRUST)
- **Stack:** Rust (Axum 0.8) + Flutter + Tauri v2
- **Repo:** michelbr84/GarraRUST
- **Equipe Linear:** GAR

## Protocolo de início de sessão

1. Leia `.garra-estado.md` para contexto da sessão anterior
2. Verifique `git status` e `git log --oneline -5`
3. Consulte a memória em `.claude/` se o contexto for relevante

## Estrutura de crates

Atualizado após GAR-391a (2026-04-13). **19 crates ativos** no workspace + 1 PoC efêmero em `benches/`.

```text
crates/
  garraia-cli/        — binário "garraia" (clap), wizard, chat interativo, migrate,
                        `config check` (GAR-379 slice 1) com validation + precedence
                        report + exit codes sysexits (0/2/65). Plan 0039 (GAR-413
                        Stage 1): novo subcomando `garraia migrate workspace
                        --from-sqlite … --to-postgres …` implementando users +
                        user_identities + PHC reassembly PBKDF2-SHA256 → PHC
                        format aceito por `garraia_auth::hashing::verify_pbkdf2`
                        + audit atômico in-tx. Plan 0040 (GAR-413 Stage 3) adiciona
                        groups + group_members: auto-cria (ou reusa) bucket
                        `--target-group-name` / `--target-group-type` (defaults
                        `'Legacy Personal Workspace'` / `'personal'`), primeiro user
                        migrado (`created_at ASC`) vira `owner`, demais `member`,
                        audit `groups.imported_from_sqlite` + N×
                        `group_members.imported_from_sqlite` atômico no mesmo tx
                        dos stages 1+2. Plan 0045 (GAR-413 Stage 5, sessão autônoma
                        Lote A-2 2026-04-22) adiciona chats + chat_members: amendment
                        normativo ao plan 0034 §7.5 (tabela legacy real é `sessions`,
                        não `conversations` — evidência em
                        `garraia-db/src/session_store.rs:105`), mapping
                        `sessions → chats` (type `'channel'`) + `chat_members`
                        (role `'owner'` para o `sessions.user_id` migrado), audit
                        `chats.imported_from_sqlite` + `chat_members.imported_from_sqlite`
                        atômico na mesma tx dos stages 1+2+3, `ChatMapping
                        { legacy_session_id → new_chat_id }` exposto em memória para
                        o stage 6 (messages) consumir em slice futuro. Stages 6+
                        (messages, memory, sessions, api_keys) em slices futuros.
  garraia-gateway/    — servidor HTTP/WS (Axum 0.8), admin API, MCP registry, router
  garraia-agents/     — LLM providers (OpenAI/OpenRouter/Anthropic/Ollama), AgentRuntime, tools
  garraia-auth/       — ✅ verify path real + extractor + endpoints (GAR-391a/b/c).
                        Tipos: IdentityProvider trait + InternalProvider + LoginPool/SignupPool
                        newtypes (private inner PgPool, validated via SELECT current_user, !Clone
                        enforced via static_assertions) + Role/Action enums + fn can() central
                        com 5×22=110-case table-driven test + Principal extractor (Axum
                        FromRequestParts) + RequirePermission struct method (NOT FromRequestParts
                        devido a const-generic limitation do Axum). Crypto: Argon2id (RFC 9106
                        m=64MiB,t=3,p=4) + PBKDF2 dual-verify + lazy upgrade transacional sob
                        FOR NO KEY UPDATE OF ui + constant-time anti-enumeration via DUMMY_HASH
                        em build.rs. JWT: HS256 access token (15min) + algorithm-confusion guards
                        + refresh token opaco com HMAC-SHA256 separado. PII: Credential.password
                        em SecretString + RedactedStorageError wrapper. Endpoints (default-on,
                        feature `auth-v1` REMOVIDA em 391c): POST /v1/auth/{login,refresh,logout,
                        signup} retornando 401 byte-identical em todos os modos de falha + 409
                        em duplicate signup. Audit em todos os terminals do login flow. Gateway
                        wiring via AuthConfig em garraia-config (4 env vars, fail-soft). Métricas
                        Prometheus baseline com bounded outcome enum. GAR-392 (pure RLS
                        matrix, 81 cenários, plan 0013 path C) ✅ entregue 2026-04-14 —
                        matriz table-driven contra pgvector/pg16 real exercitando
                        garraia_app (10 FORCE RLS tables × 4 TenantCtx + WITH CHECK
                        writes) + garraia_login e garraia_signup (grant layer allow/
                        denied). Oracle SQLSTATE distingue InsufficientPrivilege
                        (42501 grant) / PermissionDenied (42501 WITH CHECK) /
                        RlsFilteredZero (USING) / RowsVisible (any positive). GAR-391d
                        (app-layer cross-group via HTTP) DEFERIDO para plan 0014 /
                        Fase 3.4 — endpoints REST /v1/{chats,messages,memory,tasks,
                        groups,me} ainda não existem em garraia-gateway (verificado
                        empiricamente 2026-04-14). Epic GAR-391 continua aberto.
                        Decisão: docs/adr/0005-identity-provider.md (com Amendment 2026-04-13).
  garraia-channels/   — Telegram, Discord, Slack, WhatsApp, iMessage
  garraia-db/         — SQLite (rusqlite), SessionStore, CRUD (dev/CLI single-user).
                        Plan 0036 (GAR-382): `update_mobile_user_hash` para lazy upgrade
                        de PBKDF2 → Argon2id sem mexer no schema.
  garraia-security/   — CredentialVault (AES-256-GCM), PBKDF2, RedactingWriter
  garraia-config/     — schema unificado de config (serde + validator + notify).
                        Plan 0035 (GAR-379 slice 1): novo módulo `check` com `run_check`
                        + `ConfigCheck`/`Finding`/`Severity`/`SourceReport` alimentando o
                        subcomando CLI `garraia config check [--json] [--strict]`.
                        Plan 0044 (GAR-395 slice 2) adiciona `StorageConfig` +
                        `StorageBackend` enum (`local` | `s3`) + `LocalFsConfig` +
                        `S3Config` com validações (staging_dir writable,
                        `max_patch_bytes` na faixa, S3 endpoint bem-formado, MIME
                        allow-list override via `allow_unsafe_mime_in_local_fs`),
                        4 unit tests na matriz em `check.rs`. Plan 0046 (GAR-379
                        slice 3, sessão autônoma Lote A-3 2026-04-22) adiciona
                        `AuthSection { jwt_algorithm, access_token_ttl_secs,
                        refresh_token_ttl_secs, metrics_token_ttl_hint_secs }` em
                        `AppConfig` — APENAS knobs não-secret (secrets seguem
                        env-only via `AuthConfig::from_env`, §5.1). `AuthConfig`
                        ganha fallback `GarraIA_VAULT_PASSPHRASE` (zero breaking
                        para deploys legacy). `config check` ganha 4 validações
                        (algoritmo aceito, TTL ranges, access ≤ refresh,
                        env-override Info). Redaction invariant: output (humano
                        + JSON) só reporta presença de secrets (`api_key_set:
                        true`), nunca valores.
  garraia-telemetry/  — ✅ OpenTelemetry + Prometheus baseline (GAR-384) — feature-gated
  garraia-workspace/  — ✅ Postgres 16 + pgvector multi-tenant — Fase 3 schema COMPLETO
                        (GAR-407 + GAR-386 + GAR-388 + GAR-389 + GAR-408 + GAR-390 + 391a/b/c
                        + GAR-387 + GAR-395). 29 tabelas em 14 migrations, 22 sob FORCE RLS, 7 tenant-root
                        sob app-layer:
                        • 001 users/groups/identities/sessions/api_keys/invites (tenant roots)
                        • 002 RBAC roles/permissions/63 role_permissions + audit_events + single-owner idx
                        • 003 folders/files/file_versions (GAR-387) — compound FK + object_key UNIQUE
                              + HMAC integrity + FORCE RLS com WITH CHECK explícito
                        • 004 chats/chat_members/messages (FTS) /message_threads com compound FK
                        • 005 memory_items/memory_embeddings (pgvector HNSW cosine)
                        • 006 tasks Tier 1 Notion-like (8 tabelas com RLS embedded + subtasks)
                        • 007 RLS FORCE wrap-up em 10 tabelas com NULLIF fail-closed
                        • 008 garraia_login NOLOGIN BYPASSRLS dedicated role (GAR-391a)
                        • 009 user_identities.hash_upgraded_at (GAR-391b prereq, plan 0011.5)
                        • 010 garraia_signup NOLOGIN BYPASSRLS + GRANT SELECT ON sessions/group_members
                              TO garraia_login (GAR-391c, Gaps A+B+C closed)
                        • 011 group_invites pending UNIQUE, 012 single-owner idx active-only,
                          013 audit_events WITH CHECK explícito (padrão seguido por 003).
                        • 014 tus_uploads (GAR-395 plan 0041) — ledger de upload tus 1.0 com
                              FORCE RLS + `tus_uploads_group_isolation` + CHECK `upload_length ≤ 5 GiB`
                              + `object_key` UNIQUE + índice parcial `expires_in_progress_idx`.
                        Handle PII-safe via skip(config) + custom Debug redaction.
                        Decisão: docs/adr/0003-database-for-workspace.md + 0004-object-storage.md.
  garraia-plugins/    — sandbox WASM inicial (wasmtime) — features adicionais na Fase 2.2
  garraia-voice/      — STT (Whisper) + TTS (Chatterbox/ElevenLabs/Kokoro)
  garraia-media/      — processamento de PDF, imagens, mídia
  garraia-skills/     — registry de skills para o agente
  garraia-tools/      — tools compartilhadas (file ops, search, web)
  garraia-runtime/    — runtime helpers
  garraia-common/     — tipos + erros compartilhados
  garraia-glob/       — glob matching utilitário
  garraia-desktop/    — Tauri v2 app (Windows MSI, overlay)
  garraia-gateway/    — Plan 0046 (GAR-379 slice 3, 2026-04-22) remove hardcoded
                        fallback inseguro `garraia-insecure-default-jwt-secret-change-me`
                        de `mobile_auth.rs` e introduz sentinel `AuthConfigMissing`
                        + getter `AppState::jwt_signing_secret() -> Result<SecretString,
                        AuthConfigMissing>`. `issue_jwt` / `issue_jwt_pub` propagam
                        `?` até handler, que converte em **503 fail-closed** (alinha
                        `/auth/*` com `/v1/auth/*` quando nenhum secret configurado).
                        Grep invariant: `std::env::var("GARRAIA_JWT_SECRET")` e
                        `std::env::var("GarraIA_VAULT_PASSPHRASE")` agora aparecem
                        SÓ em `crates/garraia-config/src/auth.rs` (oauth.rs e totp.rs
                        refactorados). `metrics_token` lido via `garraia-telemetry::config`
                        dedicado. Ver `docs/auth-config.md` para matriz de precedência.
                        Fase 3.5 (GAR-395 slice 1 plan 0041 + slice 2 plan 0044 +
                        slice 3 plan 0047) adiciona `rest_v1::uploads` com `POST
                        /v1/uploads` (tus 1.0 Creation) + `HEAD /v1/uploads/{id}`
                        (Resume probe) + `PATCH /v1/uploads/{id}` (Core byte append)
                        + `DELETE /v1/uploads/{id}` (Termination) + `OPTIONS
                        /v1/uploads` (tus discovery) atrás de `Tus-Resumable:
                        1.0.0` precondition. Stored em `tus_uploads` (migration
                        014, FORCE RLS). Slice 2 wire `ObjectStore` em `AppState`
                        via novo `StorageConfig` (`garraia-config::model::StorageConfig`,
                        backend `local` ou `s3` feature-gated), staging FS local
                        append-only, commit two-phase ordering (blob-first via
                        `ObjectStore::put` + `files`/`file_versions` atomic + audit
                        `upload.completed` + `tus_uploads.status='completed'` →
                        `COMMIT` Postgres em seguida — plan 0044 §5.3.1). Cap
                        operacional `storage.max_patch_bytes` default 100 MiB.
                        Plan 0047 (GAR-395 slice 3, 2026-04-23 merged em `96f5c03`
                        via PR #62) fecha o epic GAR-395 adicionando: `DELETE
                        /v1/uploads/{id}` (Termination idempotente 204/404),
                        expiration worker dedicado em `uploads_worker.rs` (332 LOC)
                        + `uploads_worker_util.rs` purgando uploads `status='in_progress'`
                        expirados via `expires_in_progress_idx` com budget + jitter
                        configuráveis, e `ObjectStore::put_stream` em `LocalFs` para
                        patches grandes sem buffer integral em RAM. 752 LOC de
                        integration tests novos em `rest_v1_uploads_delete_worker.rs`.
                        GAR-395 movido para Done em 2026-04-23 17:38Z.
                        Plan 0050 Lote 2 (GAR-438, 2026-04-24 merged em `1828625`
                        via PR #64) corrige o pipeline CI `.github/workflows/ci.yml`:
                        `e2e` + `playwright` antes chamavam `./target/release/garraia-gateway`
                        (binário inexistente — `garraia-gateway` é biblioteca) e o
                        mascaramento via `continue-on-error: true` escondia o
                        `No such file or directory`. Fix cirúrgico: `cargo build
                        --bin garraia --release` + `./target/release/garraia start
                        --host 0.0.0.0 --port 3888` + `services: postgres:16.8-alpine`
                        + envs mínimas (`GARRAIA_JWT_SECRET`, `GARRAIA_REFRESH_HMAC_SECRET`,
                        `GARRAIA_LOGIN_DATABASE_URL`, `GARRAIA_SIGNUP_DATABASE_URL`
                        com `::add-mask::`). 4 de 7 `continue-on-error` removidos
                        permanentemente; 3 remanescentes rastreáveis por issue
                        (L286→GAR-444 mock LLM, L402→GAR-443 UI drift, L443→Lote 4
                        RUSTSEC).
                        Plan 0052 (GAR-443 Lote 4, 2026-04-24) migra
                        `tests/playwright/mcp-manager.spec.ts` para `getByTestId(...)`
                        ancorados em `data-testid` adicionados a `admin.html`
                        (`showMcpForm` + `renderMcpPage`). **Convenção**: specs
                        Playwright do admin DEVEM preferir `data-testid` estáveis
                        em vez de `placeholder*=` ou `getByRole(button,{name})` —
                        copy/placeholder são propriedade da UX e podem mudar; os
                        testids são contrato de teste. Remove `continue-on-error: true`
                        do step `Run Playwright tests` (`ci.yml:425`); CoE count
                        cai 2→1 (só RUSTSEC remanescente).
  garraia-storage/    — Fase 3.5 (GAR-394 slice 1 plan 0037 + slice 2 plan 0038) —
                        trait ObjectStore + LocalFs baseline + path_sanitize. Slice 2
                        adiciona `S3Compatible` (aws-sdk-s3) atrás da feature
                        `storage-s3` com SSE-S3 obrigatório, MIME allow-list
                        compartilhada com LocalFs (ADR 0004 §Security 3), HMAC-SHA256
                        integrity sobre `{key}:{version_id}:{sha256_hex}` via
                        `PutOptions::hmac_secret` (ADR 0004 §Security 4), presigned
                        URLs reais com TTL range [30s, 900s]. MinIO coberto via
                        endpoint override. Integration tests: MinIO testcontainer
                        gated pela feature. Wiring no `garraia-gateway` +
                        `garraia-config::StorageConfig` fica para slice 3.
apps/
  garraia-mobile/     — Flutter Android client (Riverpod, go_router, Dio)
```

### Crates planejados (ROADMAP AAA Fases 2-3)

```text
garraia-embeddings/  — Fase 2.1 (GAR-372) — embeddings locais mxbai + vector store lancedb
```

### PoCs efêmeros

```text
benches/
  database-poc/    — GAR-373 bench harness (Postgres vs SQLite). Crate ISOLADO, NÃO é
                     workspace member. Deletar depois que garraia-workspace (GAR-407)
                     estiver estabilizado. Tem [workspace] próprio no Cargo.toml.
```

## Convenções de código

### Rust

- `AppState` é `Arc<AppState>` — import via `crate::state::AppState`
- DB via `SessionStore` (rusqlite, sync, `tokio::sync::Mutex`)
- Axum 0.8: `FromRequestParts` usa AFIT nativo — **sem** `#[async_trait]`.
  Exceção documentada: traits que são usados como `dyn Trait` (ex.:
  `garraia_storage::ObjectStore`) usam `#[async_trait]` por causa de
  limitação de AFIT + `dyn` em Rust stable. Ver plan 0037 §5.1.
- Usar `?` operator para tratamento de erros (não `unwrap()` em produção)
- SQL queries via `params!` macro (nunca concatenar strings)
- `cargo check -p <crate>` antes de qualquer commit
- `cargo clippy --workspace` para linting

### Flutter

- State management: Riverpod + code generation
- Navigation: go_router com auth redirect
- HTTP: Dio com `_AuthInterceptor` (JWT bearer)
- Nunca usar `withOpacity()` — usar `withValues(alpha:)`

### Shell / Scripts

- `set -euo pipefail` em todos os scripts
- Usar `#!/usr/bin/env bash` (não `/bin/bash`)
- Paths devem funcionar cross-platform (usar `which` ou env vars)

### Convenção de datas

- **Project narrative dates** (ROADMAP, plans, ADRs, READMEs, commit prose, doc paragraphs como "entregue em YYYY-MM-DD") usam **America/New_York (Florida)** local time. Nunca usar UTC para data narrativa do projeto sem dizer explicitamente.
- **API timestamps, audit_events, log timestamps, JWT `iat`/`exp`, `expires_at` em response bodies** são sempre **UTC ISO 8601 com sufixo `Z`** — declaração explícita de UTC.
- Quando estiver em dúvida em prosa de doc/plan/commit, use o local time da Flórida. Se a referência for tecnicamente UTC (ex.: timestamp de log capturado), anote `(UTC)` inline.

### Commits

- Formato: Conventional Commits (`feat:`, `fix:`, `chore:`, `refactor:`, `test:`, `docs:`)
- Imperativo: "adiciona feature" (não "adicionada feature")
- Limite 72 chars no assunto

## Regras absolutas

1. **NUNCA** commitar `.env`, credenciais ou tokens
2. **NUNCA** `rm -rf /`, `rm -rf ~` ou fork bombs
3. **NUNCA** force push para `main`
4. **NUNCA** usar `unwrap()` em código de produção (apenas em testes)
5. **NUNCA** concatenar strings em SQL queries — `params!` (rusqlite) ou `sqlx::query!` (Postgres)
6. **NUNCA** expor secrets/PII em logs (`GARRAIA_JWT_SECRET`, `GARRAIA_REFRESH_HMAC_SECRET`, `GARRAIA_METRICS_TOKEN`, `ANTHROPIC_API_KEY`, etc.)
7. **NUNCA** ignorar erros de compilação do `cargo check`
8. **SEMPRE** escrever ADR em `docs/adr/NNNN-*.md` antes de decisão arquitetural irreversível (Postgres vs SQLite, vector store, storage backend, etc.) — ver `ROADMAP.md` §3.1
9. **SEMPRE** migrations Postgres forward-only (colunas novas → backfill → NOT NULL depois)
10. **SEMPRE** testes de autorização cross-group antes de merge em qualquer rota nova de `garraia-workspace`/`garraia-auth`
11. **SEMPRE** usar a `garraia_login` BYPASSRLS dedicated role exclusivamente em paths de credential verification (login + lazy upgrade PBKDF2→Argon2id + extractor membership lookup + refresh token verify/revoke). Acesso ao role só via `garraia-auth::LoginPool` newtype — nunca raw `PgPool`. Documentado em `docs/adr/0005-identity-provider.md` (com Amendment 2026-04-13 cobrindo Gaps A/C).
12. **NUNCA** ler `user_identities.password_hash` no app pool role (`garraia_app`) — RLS filtra para 0 rows. Tratar 0 rows como "user not found" é anti-pattern (significa "RLS bloqueou"). Sempre usar `garraia_login` via login endpoint. Ver ADR 0005 §"Anti-patterns".
13. **SEMPRE** usar a `garraia_signup` BYPASSRLS dedicated role exclusivamente para o signup flow (`POST /v1/auth/signup`). Acesso só via `garraia-auth::SignupPool` newtype — nunca raw `PgPool`, nunca substituível pelo `LoginPool`. O role tem `INSERT` em `users`/`user_identities` mas NENHUM acesso a `sessions`, `messages`, `chats`, `memory_*`, `tasks*`, `groups`, `group_members` ou qualquer dado de tenant. Migration 010, ADR 0005 §"Amendment 2026-04-13" Gap B.

## Framework de Desenvolvimento: Superpowers

O projeto utiliza [Superpowers](https://github.com/obra/superpowers) como framework primário de workflow de desenvolvimento.

- **Config:** `.claude/superpowers-config.md` — contexto do projeto para o Superpowers
- **Bridge:** `skills/superpowers-bridge.md` — mapeamento entre skills locais e Superpowers
- **Regra:** Para features novas, bugs complexos e refactoring → usar workflow Superpowers (brainstorming → spec → plan → TDD → review → merge)
- **Skills locais** são usadas para operações específicas: pre-commit, generate-docs, translate, shell-explain

## Skills disponíveis

| Skill | Uso |
| ------- | ----- |
| `/superpowers-bridge` | Mapeamento skills locais ↔ Superpowers |
| `/review-pr` | Revisa PR com code-reviewer + security-auditor |
| `/tdd-loop` | Red-Green-Refactor automático |
| `/fix-issue` | Corrige issue GitHub via TDD |
| `/pre-commit` | Validação pré-commit (segredos, debug, lint) |
| `/refactor-module` | Refactoring seguro com testes |
| `/assemble-team` | Monta equipe de agentes coordenados |
| `/generate-docs` | Gera documentação automática |
| `/code-review` | Revisão de código inline |
| `/git-assist` | Ajuda com git workflow |

## Agents disponíveis

| Agent | Papel |
| ------- | ------- |
| `code-reviewer` | Revisor sênior Rust/Flutter |
| `security-auditor` | Auditor OWASP, JWT, crypto |
| `doc-writer` | Escritor técnico PT-BR/EN |
| `team-coordinator` | Orquestrador de equipes de agentes |

## Ferramentas preferenciais

- Buscar arquivos: `Glob` (não `find`)
- Buscar conteúdo: `Grep` (não `grep`)
- Ler arquivos: `Read` (não `cat`)
- Editar arquivos: `Edit` (não `sed`)
- Testar Rust: `cargo test -p <crate>`
- Testar Flutter: `flutter test`
- Lint Rust: `cargo clippy --workspace`

## AI Quality Ratchet (`.quality/` + `scripts/quality/`)

Sistema de Quality Gates inspirado no padrão Catraca: métricas só sobem ou ficam, **nunca regridem**. Vide `plans/0064-quality-ratchet-pr1.md` para o scaffold inicial e `.quality/README.md` para a filosofia completa.

**Status atual: PR-1 — report-only.** Nenhum PR é bloqueado pelo ratchet ainda. Workflow `.github/workflows/quality-ratchet.yml` posta `quality-report.md` como comentário no PR e segue. Promoção a bloqueante (`compare.py --mode enforce`) entra em PR-4 com aprovação explícita.

### Comandos rápidos

```bash
# Coleta rápida (default — sob 10s):
bash scripts/quality/collect-metrics.sh > current-metrics.json

# Comparar contra baseline (report-only — sempre exit 0):
python3 scripts/quality/compare.py --mode report-only \
    .quality/baseline.json current-metrics.json

# Propor novo baseline (gera .proposed.json — NÃO commita):
python3 scripts/quality/freeze-baseline.py current-metrics.json

# Rodar testes dos parsers:
python3 -m pytest scripts/quality/tests/
```

### Regras absolutas (ratchet)

- **NUNCA** editar `.quality/baseline.json` manualmente para "passar" o ratchet — é fraude. Use `freeze-baseline.py` que gera `.quality/baseline.proposed.json` para review humano.
- **NUNCA** adicionar `continue-on-error: true` em workflows. Modo report-only é controlado pela flag `compare.py --mode report-only`.
- **NUNCA** desativar gates pré-existentes do `ci.yml` (fmt/clippy/test/audit/deny/etc.).
- Se o `/quality-babysit` propuser correção que toca segurança, auth, storage, RLS, secrets ou CI crítico → chamar `security-auditor` + `code-reviewer` agents antes de continuar (ver `.claude/commands/quality-babysit.md` §Guardrails).

## Referências

- @imports `.claude/agents/` para agentes especializados
- @imports `skills/` para workflows reutilizáveis
- @imports `.garra-estado.md` para estado da sessão anterior
- @imports `ROADMAP.md` — plano AAA em 7 fases, fonte de verdade do planejamento
- @imports `deep-research-report.md` — base arquitetural da Fase 3 (Group Workspace multi-tenant)
- @imports `docs/adr/` — decisões arquiteturais. **Accepted:** 0003 (Postgres para Group Workspace). **Proposed/blocked:** 0001, 0002, 0004-0008. Ver `docs/adr/README.md` para o índice.
- Linear: [time GarraIA-RUST (GAR)](https://linear.app/chatgpt25/team/GAR/projects) — execução semana a semana
