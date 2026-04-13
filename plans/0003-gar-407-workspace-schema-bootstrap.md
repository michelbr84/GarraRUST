# Plan 0003: GAR-407 — `garraia-workspace` crate + migration 001 (users & groups)

> **Status:** 📋 Awaiting approval
> **Issue:** [GAR-407](https://linear.app/chatgpt25/issue/GAR-407)
> **Project:** Fase 3 — Group Workspace
> **Labels:** `epic:ws-schema`
> **Priority:** Urgent
> **Estimated session size:** 4-6 horas de trabalho focado
> **Author:** Claude Opus 4.6 + @michelbr84
> **Date:** 2026-04-13
> **Depends on:** ✅ GAR-373 (ADR 0003 Postgres, merged `32dba08`)
> **Unblocks:** GAR-386, GAR-387, GAR-388, GAR-389, GAR-390, GAR-408 (migrations 002-007)

---

## 1. Goal (one sentence)

Criar o crate `garraia-workspace` com `sqlx::migrate!`, migration `001_initial_users_groups.sql` (tabelas `users`, `user_identities`, `sessions`, `api_keys`, `groups`, `group_members`, `group_invites`), e um smoke test que prova que a migration aplica do zero contra um `testcontainers::pgvector:pg16` efêmero em menos de 30 segundos — de modo que as migrations subsequentes (002-007) possam ser adicionadas por issues separadas sem retrabalho.

---

## 2. Rationale — por que esse agora

1. **Execução literal do ADR 0003.** O ADR define Postgres 16 + pgvector + sqlx 0.8; este plan materializa a primeira fatia real de código.
2. **Caminho crítico destravado.** Com o crate existindo + migration 001 + testcontainer workflow funcionando, as issues de migrations 002-007 viram trabalho mecânico: "mais uma tabela, mesmo padrão."
3. **Baixo risco.** Crate novo, zero código existente afetado. `cargo check --workspace` só ganha um novo member; o resto do workspace não sente.
4. **Tamanho cabível em uma sessão.** ~4-6h. Menor que GAR-373 porque não tem research — a decisão já existe.
5. **Validação end-to-end do pipeline sqlx.** O crate exercita: `sqlx::migrate!()` macro, testcontainers-modules/postgres, seed de fixtures em testes, integração com `tokio`. Todos os downstream (migrations 002-007, garraia-auth, API /v1/groups) herdam esse pipeline validado.

---

## 3. Scope & Non-Scope

### In scope

- Novo crate `crates/garraia-workspace/` como **workspace member** (ao contrário do `benches/database-poc/` que é isolado).
- **Migration 001** em `crates/garraia-workspace/migrations/001_initial_users_groups.sql` com as 7 tabelas: `users`, `user_identities`, `sessions`, `api_keys`, `groups`, `group_members`, `group_invites`.
- **Extensões Postgres obrigatórias** declaradas na migration: `pgcrypto` (para `gen_random_uuid()`), `citext` (email case-insensitive).
- **Estrutura pública mínima** em `src/lib.rs`: `Workspace` (wrapper de `PgPool`), `WorkspaceConfig` (database_url, max_connections, migrate_on_start), `WorkspaceError` enum, função `Workspace::connect(config)` que faz pool + opcional `sqlx::migrate!().run(&pool)`.
- **Integração com `garraia-telemetry`** via `#[tracing::instrument]` nas funções públicas de conexão/migração (sem PII, obviamente — skip_all no database_url).
- **Integration test** `tests/migration_smoke.rs` que:
  - Sobe `pgvector/pgvector:pg16` via `testcontainers-modules`.
  - Chama `Workspace::connect` com `migrate_on_start = true`.
  - Verifica que as 7 tabelas existem via `information_schema.tables`.
  - Verifica que os índices críticos existem.
  - Faz 1 insert trivial em `users` via `sqlx::query(...)` raw (não `query!` — ver §12 Q1) para provar que a tabela é escreviível.
- **Adicionar `garraia-workspace` ao workspace `[members]`** em `Cargo.toml` root.
- **Documentação mínima** em `crates/garraia-workspace/README.md` explicando como rodar os testes localmente (Docker Desktop + `cargo test -p garraia-workspace`).
- **Update do ROADMAP.md** marcando GAR-407 como `[x]` em Fase 3.2 quando merged.
- **Linear GAR-407** movida para Done após merge.

### Out of scope

- ❌ **Sem `sqlx::query!` macros.** Toda query usa `sqlx::query()` / `sqlx::query_as()` runtime-dispatched. Motivo: `query!` exige ou uma DB rodando durante `cargo check` ou `.sqlx/` committed via `cargo sqlx prepare`, ambos adicionam complexidade que não pertence ao bootstrap. Follow-up issue (a filar) habilita `query!` + `.sqlx/` offline cache.
- ❌ **Sem CRUD completo.** Nenhuma fn `create_user`, `list_groups`, etc. Apenas uma inserção trivial no smoke test. CRUD real vira issue separada (parte do GAR-393 API /v1/groups).
- ❌ **Sem RLS.** Estas tabelas são "tenant roots" e não têm `group_id` para filtrar. RLS entra na migration 007 (GAR-408) sobre `messages`/`files`/`memory_items`/etc.
- ❌ **Sem integração no gateway.** `garraia-gateway` não ganha dep em `garraia-workspace` neste PR. Wiring vem em GAR-393.
- ❌ **Sem migration do SQLite atual.** `garraia-cli migrate workspace` é GAR-413.
- ❌ **Sem GitHub Actions CI.** Rodar testcontainers em CI pede docker-in-docker setup; issue separada após bootstrap local verde.
- ❌ **Sem Argon2id password hashing.** `user_identities.password_hash` é apenas uma coluna `text`; o algoritmo é decidido quando `garraia-auth` implementar login (GAR-391, depende de ADR 0005).
- ❌ **Sem tabelas de files/chats/memory/tasks/audit_events.** Cada uma tem sua própria migration e issue separada.

---

## 4. Acceptance criteria (verificáveis)

- [ ] `cargo check --workspace` verde após adicionar `garraia-workspace` aos members.
- [ ] `cargo check --workspace --no-default-features` verde (telemetry-off path).
- [ ] `cargo clippy --workspace -- -D warnings` verde.
- [ ] `cargo test -p garraia-workspace` verde localmente com Docker Desktop rodando.
- [ ] O smoke test roda em **menos de 30 segundos** de wall clock (container start + migration + verifications + teardown).
- [ ] `sqlx::migrate!()` aplica a migration 001 do zero sem erros.
- [ ] Todas as 7 tabelas (`users`, `user_identities`, `sessions`, `api_keys`, `groups`, `group_members`, `group_invites`) existem após a migration, verificado por query em `information_schema.tables`.
- [ ] Índices críticos existem: `user_identities (provider, provider_sub) UNIQUE`, `sessions_user_id_idx`, `api_keys_key_hash UNIQUE`, `groups_created_by_idx`, `group_members_user_id_idx`, `group_invites_email_idx (partial)`.
- [ ] Um insert de usuário fake na tabela `users` via `sqlx::query()` retorna o UUID gerado.
- [ ] Um segundo insert com o MESMO email falha com unique constraint violation (prova que a constraint funciona).
- [ ] Migration é **forward-only** (sem `DROP TABLE`, sem `ALTER COLUMN DROP`).
- [ ] Nenhum `unwrap()` em código de produção (testes podem).
- [ ] Nenhum secret (DATABASE_URL completo) vaza em logs — verificado por inspeção dos spans via `#[tracing::instrument(skip(...))]`.
- [ ] `docs/adr/0003-database-for-workspace.md` referenciado no cabeçalho da migration SQL como fonte da decisão.
- [ ] `ROADMAP.md §3.2` atualizado marcando GAR-407 como `[x]`.
- [ ] PR review verde por `@code-reviewer` E `@security-auditor` (security porque mexe em auth tables).

---

## 5. File-level changes

### 5.1 Novos arquivos

```
crates/garraia-workspace/
├── Cargo.toml
├── README.md                        # 15-20 linhas: como rodar os testes locais
├── migrations/
│   └── 001_initial_users_groups.sql # ★ o schema
├── src/
│   ├── lib.rs                       # Workspace, WorkspaceConfig, connect()
│   ├── config.rs                    # WorkspaceConfig (serde + validator, from_env)
│   ├── error.rs                     # WorkspaceError enum (thiserror)
│   └── store.rs                     # PgPool wrapper + migrate_on_start logic
└── tests/
    └── migration_smoke.rs           # integration test com testcontainers
```

### 5.2 Edits em arquivos existentes

- `Cargo.toml` (workspace root): adicionar `"crates/garraia-workspace"` ao array `[workspace].members`, mantendo ordem alfabética.
- `ROADMAP.md` §3.2: marcar GAR-407 como `[x]` com link e menção ao commit.
- Nenhum outro arquivo do workspace tocado.

### 5.3 Dependencies (novas no workspace root `[workspace.dependencies]` ou por-crate?)

Por-crate, para manter o escopo cirúrgico e não forçar outros crates a resolverem sqlx. Em `crates/garraia-workspace/Cargo.toml`:

```toml
[package]
name = "garraia-workspace"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "Multi-tenant Postgres workspace (groups, members, auth, RBAC, audit) — Fase 3"

[dependencies]
# SQL driver (runtime-only, no query! macros yet — see plan §12 Q1)
sqlx = { version = "0.8", features = ["runtime-tokio", "postgres", "uuid", "chrono", "migrate", "json"] }
# Identifiers
uuid = { version = "1", features = ["v7", "serde"] }
# Timestamps
chrono = { version = "0.4", features = ["serde"] }
# Serialization
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
# Errors
thiserror = { workspace = true }
# Async runtime
tokio = { workspace = true }
# Tracing (PII-safe spans via skip)
tracing = { workspace = true }
# Config validation
validator = { version = "0.18", features = ["derive"] }

[dev-dependencies]
testcontainers = "0.23"
testcontainers-modules = { version = "0.11", features = ["postgres"] }
tokio = { workspace = true, features = ["rt-multi-thread", "macros", "test-util"] }
anyhow = "1"
```

Versões alinhadas com `benches/database-poc/` (já validadas verdes no GAR-373), o que reduz risco de surpresa.

### 5.4 Nada tocado em production code

- `crates/garraia-gateway` — não mexe.
- `crates/garraia-db` — não mexe. Continua servindo single-user per ADR 0003.
- `crates/garraia-auth` — ainda não existe (GAR-391).
- `apps/garraia-mobile` — não mexe.

---

## 6. Schema details (o SQL completo)

Abaixo o conteúdo literal da migration 001. É parte do plano porque pequenas decisões de schema (tipos, constraints, índices) importam muito e devem ser aprovadas antes de codar.

```sql
-- 001_initial_users_groups.sql
-- GAR-407 — garraia-workspace bootstrap
-- Decision: docs/adr/0003-database-for-workspace.md (Postgres 16 + pgvector)
-- Forward-only. No DROP TABLE, no destructive ALTER.

-- ─── Extensions ────────────────────────────────────────────────────────────
-- pgcrypto provides gen_random_uuid() (v4). Rust code generates uuid_v7 for
-- time-ordered inserts; the DB default is a v4 fallback for SQL-level inserts
-- (migrations, debugging, psql).
CREATE EXTENSION IF NOT EXISTS pgcrypto;

-- citext gives case-insensitive email comparison without lower() tricks.
CREATE EXTENSION IF NOT EXISTS citext;

-- ─── users ─────────────────────────────────────────────────────────────────
CREATE TABLE users (
    id            uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    email         citext      NOT NULL UNIQUE,
    display_name  text        NOT NULL,
    status        text        NOT NULL DEFAULT 'active'
                  CHECK (status IN ('active', 'suspended', 'deleted')),
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now()
);

COMMENT ON TABLE  users IS 'Tenant-root user identities. RLS NOT enabled — visible to app layer.';
COMMENT ON COLUMN users.email IS 'Case-insensitive via citext. Unique across the instance.';
COMMENT ON COLUMN users.status IS 'active → normal; suspended → blocked login; deleted → tombstone pending hard delete';

-- ─── user_identities ───────────────────────────────────────────────────────
-- Maps a user to one or more identity providers (internal JWT, OIDC, SAML).
-- Shape matches ADR 0003 and leaves room for ADR 0005 (identity provider)
-- without forcing a schema change when OIDC adapters land.
CREATE TABLE user_identities (
    id             uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id        uuid        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider       text        NOT NULL,
    provider_sub   text        NOT NULL,
    password_hash  text,
    created_at     timestamptz NOT NULL DEFAULT now(),
    UNIQUE (provider, provider_sub)
);
CREATE INDEX user_identities_user_id_idx ON user_identities(user_id);

COMMENT ON COLUMN user_identities.provider IS 'internal | oidc | saml. Default internal for self-host.';
COMMENT ON COLUMN user_identities.provider_sub IS 'Stable subject identifier from the provider. For internal provider, equals users.id::text.';
COMMENT ON COLUMN user_identities.password_hash IS 'Only used when provider=internal. NULL for external IdPs. Algorithm decided in garraia-auth (GAR-391).';

-- ─── sessions ──────────────────────────────────────────────────────────────
CREATE TABLE sessions (
    id                  uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id             uuid        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    refresh_token_hash  text        NOT NULL,
    device_id           text,
    expires_at          timestamptz NOT NULL,
    revoked_at          timestamptz,
    created_at          timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX sessions_user_id_idx ON sessions(user_id);
CREATE INDEX sessions_active_expires_idx
    ON sessions(expires_at)
    WHERE revoked_at IS NULL;

COMMENT ON COLUMN sessions.refresh_token_hash IS 'Argon2id hash of the refresh token. Never the token itself.';

-- ─── api_keys ──────────────────────────────────────────────────────────────
CREATE TABLE api_keys (
    id            uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id       uuid        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    label         text        NOT NULL,
    key_hash      text        NOT NULL UNIQUE,
    scopes        jsonb       NOT NULL DEFAULT '[]'::jsonb,
    created_at    timestamptz NOT NULL DEFAULT now(),
    revoked_at    timestamptz,
    last_used_at  timestamptz
);
CREATE INDEX api_keys_active_user_idx
    ON api_keys(user_id)
    WHERE revoked_at IS NULL;

COMMENT ON COLUMN api_keys.key_hash IS 'SHA-256 or Argon2id hash of the API key. Never the key itself.';
COMMENT ON COLUMN api_keys.scopes IS 'JSON array of permission strings, e.g. ["workspace.read", "files.write"].';

-- ─── groups ────────────────────────────────────────────────────────────────
-- The tenant root. Every downstream table (messages, files, memory) will
-- reference group_id and enable RLS filtered by current_setting('app.current_group_id').
-- Migration 007 (GAR-408) applies FORCE ROW LEVEL SECURITY to those downstream tables.
-- groups itself is NOT under RLS — membership-based visibility lives in the app layer
-- (JOIN groups ↔ group_members WHERE group_members.user_id = current_user_id).
CREATE TABLE groups (
    id          uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    name        text        NOT NULL,
    type        text        NOT NULL CHECK (type IN ('family', 'team', 'personal')),
    created_by  uuid        NOT NULL REFERENCES users(id),
    settings    jsonb       NOT NULL DEFAULT '{}'::jsonb,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX groups_created_by_idx ON groups(created_by);

COMMENT ON TABLE groups IS 'Tenant root. All scoped data (messages/files/memory/tasks) references group_id.';
COMMENT ON COLUMN groups.type IS 'family → household; team → professional; personal → solo-user fallback for SQLite migration';

-- ─── group_members ─────────────────────────────────────────────────────────
CREATE TABLE group_members (
    group_id    uuid        NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    user_id     uuid        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role        text        NOT NULL CHECK (role IN ('owner', 'admin', 'member', 'guest', 'child')),
    status      text        NOT NULL DEFAULT 'active'
                CHECK (status IN ('active', 'invited', 'removed', 'banned')),
    joined_at   timestamptz NOT NULL DEFAULT now(),
    invited_by  uuid        REFERENCES users(id),
    PRIMARY KEY (group_id, user_id)
);
CREATE INDEX group_members_user_id_idx ON group_members(user_id);
CREATE INDEX group_members_active_by_group_idx
    ON group_members(group_id)
    WHERE status = 'active';

COMMENT ON COLUMN group_members.role IS 'Maps to capability matrix in ROADMAP §3.3 / GAR-391';

-- ─── group_invites ─────────────────────────────────────────────────────────
CREATE TABLE group_invites (
    id             uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    group_id       uuid        NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    invited_email  citext      NOT NULL,
    proposed_role  text        NOT NULL
                   CHECK (proposed_role IN ('admin', 'member', 'guest', 'child')),
    token_hash     text        NOT NULL UNIQUE,
    expires_at     timestamptz NOT NULL,
    created_by     uuid        NOT NULL REFERENCES users(id),
    created_at     timestamptz NOT NULL DEFAULT now(),
    accepted_at    timestamptz,
    accepted_by    uuid        REFERENCES users(id)
);
CREATE INDEX group_invites_group_id_idx ON group_invites(group_id);
CREATE INDEX group_invites_pending_email_idx
    ON group_invites(invited_email)
    WHERE accepted_at IS NULL;

COMMENT ON COLUMN group_invites.token_hash IS 'Argon2id hash of the invite token sent via email. Never the token itself.';
COMMENT ON COLUMN group_invites.proposed_role IS 'owner cannot be invited — only created by the bootstrap user.';
```

### Design notes (decisões registradas aqui para não ficarem só no commit)

1. **Sem `groups.deleted_at`.** Hard delete via CASCADE por enquanto. Se o produto pedir "recuperar grupo deletado", adicionamos `deleted_at` em migration futura + política de retenção.
2. **`user_identities.password_hash` é `text` genérico.** O algoritmo (Argon2id) é decidido em GAR-391 quando `garraia-auth` implementar login. O hash é armazenado como string no formato PHC (`$argon2id$v=19$m=65536,t=3,p=4$...`).
3. **`users.updated_at` não tem trigger.** Caller (Rust) é responsável por setar `updated_at = now()` no UPDATE. Triggers viram follow-up se virarem problema real. Mantém a migration simples e auditável.
4. **UUIDs: v7 do lado Rust, v4 como default SQL.** `uuid::Uuid::now_v7()` é time-ordered (melhor para índices B-tree); `gen_random_uuid()` é o fallback quando a inserção vem direto via psql/migration.
5. **`owner` não pode ser `proposed_role`.** Owner é criado no bootstrap do grupo, não via invite. Constraint enforçada no CHECK.
6. **`groups.type = 'personal'` existe.** Para a migração SQLite→PG (GAR-413): o usuário single-user vira owner de um grupo `personal` automático. Sem esse enum value, a migração precisaria de um hack.
7. **Índices parciais (`WHERE revoked_at IS NULL`, `WHERE accepted_at IS NULL`).** Economizam espaço e aceleram queries de "itens ativos" que são o caminho comum.

---

## 7. Test plan

### 7.1 `tests/migration_smoke.rs` — integration test único

```rust
// Pseudo-código, o agente wave 1 escreve o real
#[tokio::test]
async fn migration_001_applies_and_schema_is_sane() -> anyhow::Result<()> {
    // 1. Start pgvector/pgvector:pg16 via testcontainers
    let container = /* ... */;
    let database_url = format!("postgres://postgres:postgres@{host}:{port}/postgres");

    // 2. Connect via garraia_workspace::Workspace::connect
    let workspace = Workspace::connect(WorkspaceConfig {
        database_url: database_url.clone(),
        max_connections: 5,
        migrate_on_start: true,
    })
    .await?;

    // 3. Verify 7 tables exist via information_schema
    let tables: Vec<(String,)> = sqlx::query_as(
        "SELECT table_name FROM information_schema.tables
         WHERE table_schema = 'public' ORDER BY table_name"
    ).fetch_all(workspace.pool()).await?;
    let names: Vec<&str> = tables.iter().map(|(n,)| n.as_str()).collect();
    for expected in &[
        "api_keys", "group_invites", "group_members", "groups",
        "sessions", "user_identities", "users",
    ] {
        assert!(names.contains(expected), "missing table: {expected}");
    }

    // 4. Verify critical indexes exist
    let indexes: Vec<(String,)> = sqlx::query_as(
        "SELECT indexname FROM pg_indexes WHERE schemaname = 'public'"
    ).fetch_all(workspace.pool()).await?;
    for expected in &[
        "user_identities_provider_provider_sub_key",
        "sessions_user_id_idx",
        "api_keys_key_hash_key",
        "groups_created_by_idx",
        "group_members_user_id_idx",
        "group_invites_pending_email_idx",
    ] {
        assert!(indexes.iter().any(|(n,)| n == expected), "missing index: {expected}");
    }

    // 5. Insert a fake user and verify UUID comes back
    let user_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO users (email, display_name) VALUES ($1, $2) RETURNING id"
    )
    .bind("test@example.com")
    .bind("Test User")
    .fetch_one(workspace.pool()).await?;
    assert!(!user_id.is_nil());

    // 6. Second insert with same email must fail on unique constraint
    let err = sqlx::query(
        "INSERT INTO users (email, display_name) VALUES ($1, $2)"
    )
    .bind("TEST@example.com")  // citext — same row
    .bind("Other User")
    .execute(workspace.pool()).await;
    assert!(err.is_err(), "citext unique constraint should block case-insensitive dup");

    Ok(())
}
```

### 7.2 Unit tests (inline em `src/config.rs`)

- `config::from_env_parses_database_url`
- `config::validates_database_url_is_postgres_scheme`
- `config::default_max_connections_is_10`

### 7.3 What we're NOT testing in this PR

- Login flow (GAR-391)
- Group CRUD (GAR-393)
- RLS enforcement (GAR-408)
- Migration performance at scale (GAR-407 is about correctness, not perf)
- CI integration (docker-in-docker on GitHub Actions is a separate issue)

---

## 8. Rollback plan

Rollback em 3 níveis, todos triviais:

1. **Antes de merge:** fechar o PR. Nada atingiu `main`.
2. **Depois de merge, antes de GAR-386:** `git revert` do commit. O crate `garraia-workspace` some, o workspace root volta a ter 17 members, nada mais muda porque nenhum outro crate depende dele.
3. **Depois de GAR-386+ dependerem do crate:** não rollback. Forward-fix. Mas esse cenário só aparece nas próximas issues — neste PR, rollback é pleno e gratuito.

Nenhuma migration de DB em produção (nem existe Postgres em produção ainda), nenhum secret novo, nenhum breaking change em API pública. O crate é aditivo puro.

---

## 9. Risks & mitigations

| Risco | Prob. | Impacto | Mitigação |
|---|---|---|---|
| `testcontainers-modules 0.11` API mudou desde o GAR-373 | Baixa | Médio | Código de referência em `benches/database-poc/src/postgres_scenarios.rs` — copiar o padrão exato de startup |
| `sqlx 0.8` + `citext` → type mapping problemático | Média | Médio | `citext` é "just text" para sqlx; binding como `String` funciona. PoC rápida antes de finalizar se houver dúvida |
| `pgcrypto` ou `citext` ausentes na imagem `pgvector/pgvector:pg16` | Baixa | Alto | Ambas são `contrib` extensions standard; o image usa Postgres oficial como base e tem `contrib` incluído. Testamos no benchmark já |
| Smoke test timeout em máquinas lentas | Média | Baixo | `#[tokio::test(flavor = "multi_thread")]` + comentário no teste dizendo "first run pulls image, ~60s cold, ~5s warm" |
| `uuid v7` feature requer crate >1.8 | Baixa | Baixo | Pinning `uuid = "1"` já cobre v7 (estável desde 1.8). `cargo tree` confirma se necessário |
| `validator 0.18` vs versão de outros crates | Baixa | Baixo | Já usada em `garraia-telemetry` (commit 84c4753) — compat validada |
| Cargo.lock churn gigante por sqlx + tokio | Média | Baixo | Esperado. Revisor ignora deps transitivas; só verifica se o topo faz sentido |
| `#[tracing::instrument]` no `connect()` vaza `database_url` em span | Média | **Alto** (secret leak) | `skip(config)` **obrigatório**; record somente os campos não-sensíveis via `Span::current().record(...)` se necessário |
| Constraint CHECK em enum string vs tipo `ENUM` Postgres | Baixa | Baixo | Escolha intencional: CHECK é mais fácil de evoluir que CREATE TYPE ... AS ENUM (que exige `ALTER TYPE` pesado). Documentado acima |

---

## 10. Sequence of work (ordem proposta quando aprovado)

### Wave 1 — scaffold + migration SQL (~1.5h, single agent, foreground)

1. Criar `crates/garraia-workspace/` com `Cargo.toml`, `src/lib.rs`, `src/config.rs`, `src/error.rs`, `src/store.rs`, `README.md` (stubs).
2. Adicionar `"crates/garraia-workspace"` a `Cargo.toml` root.
3. `cargo check -p garraia-workspace` verde (sem logic ainda, só deps resolvendo).
4. Escrever `migrations/001_initial_users_groups.sql` literalmente conforme §6.
5. Implementar `WorkspaceConfig` (from_env, validator), `WorkspaceError` (thiserror), `Workspace::connect` com `migrate_on_start` opcional.
6. Adicionar `#[tracing::instrument(skip(config))]` em `connect`.
7. `cargo check --workspace` verde.

### Wave 2 — integration test (~1.5h, same or second agent)

8. Escrever `tests/migration_smoke.rs` conforme §7.1.
9. Rodar `cargo test -p garraia-workspace` localmente. Iterar até verde.
10. Verificar wall time < 30s (via `time cargo test -p garraia-workspace`).
11. `cargo clippy --workspace -- -D warnings` verde.

### Wave 3 — parallel review (~20min wall, 2 agents background)

12. Spawn `@code-reviewer` com escopo de `crates/garraia-workspace/**` + migration SQL.
13. Spawn `@security-auditor` com foco em: tracing instrument PII, password_hash/key_hash/token_hash column semantics, CASCADE delete implications for GDPR right to erasure, extension requirements (pgcrypto/citext supply chain).

### Wave 4 — fixes + docs + ROADMAP + commit (~30min, me)

14. Aplicar findings dos reviews inline.
15. Atualizar `ROADMAP.md §3.2` marcando GAR-407 como `[x]` com link para commit.
16. `git add` cirúrgico (apenas os novos arquivos + workspace Cargo.toml + ROADMAP).
17. Commit com mensagem seguindo o padrão dos commits anteriores (GAR-384, GAR-373).
18. `git push origin main`.
19. Linear GAR-407 → Done.

**Total estimado: 3.5-5 horas.** Budget folgado porque é trabalho direto sem research.

---

## 11. Definition of Done

- [ ] Todos os 15 itens do §4 (acceptance criteria) marcados.
- [ ] PR merged em `main`.
- [ ] Review verde de `@code-reviewer` + `@security-auditor`.
- [ ] Linear GAR-407 → **Done** com link para o commit.
- [ ] Follow-up issue filada (se os reviews levantarem): spec de `.sqlx/` offline cache para quando passarmos a usar `sqlx::query!` macros.
- [ ] Próxima sessão pode abrir migration 002 (GAR-386 RBAC) sem precisar re-ler o pipeline todo — o crate existe, o test infra funciona, o padrão está estabelecido.

---

## 12. Open questions (preciso da sua resposta antes de começar)

1. **`sqlx::query!` macros ou `sqlx::query()` runtime?** Recomendo **runtime-only** nesta PR (veja §3 Out of scope). Follow-up issue ativa `query!` com `.sqlx/` offline cache quando for hora de escrever CRUD real. Isso mantém este bootstrap ≤ 6h. Confirma?

2. **`citext` vs `text` + `lower()`?** Recomendo **`citext`** — é extension standard Postgres, simples de usar, e evita a pegadinha de esquecer `lower()` num SELECT ad-hoc. Single-line `CREATE EXTENSION IF NOT EXISTS citext` é o único custo. Alternativa: `text` + `UNIQUE (lower(email))`. Ambas funcionam; `citext` é mais à prova de bala.

3. **`groups.type` enum inclui `'personal'`?** Recomendo **sim** — serve como fallback semântico para a migração SQLite→PG (GAR-413) onde o user single-user vira owner de um grupo `personal`. Sem isso, GAR-413 precisa de hack. Custo: uma opção a mais na UI de "criar grupo" (que nem existe ainda, então custo zero real).

4. **UUID v7 (time-ordered) do lado Rust?** Recomendo **sim** (`uuid::Uuid::now_v7()` nas funções Rust de insert). Benefícios: índices B-tree ficam sequenciais, menos page splits, melhor cache locality. Default SQL `gen_random_uuid()` é v4 como fallback para inserts SQL-diretos. Confirma?

5. **Migração SQLite→PG preserva IDs ou gera novos?** Esta pergunta é para **GAR-413**, não este plan, mas decido aqui se o schema de migration 001 precisa de coluna `legacy_sqlite_id`. Recomendo **sim, adicionar coluna `legacy_sqlite_id text NULL`** em `users` para mapear `mobile_users.id` → `users.id` durante a migração. Custo: 1 coluna opcional que fica vazia na maioria dos casos. Benefício: auditoria pós-migração e debugging. Confirma ou reverto para GAR-413 definir?

6. **Devo instrumentar `connect()` com `tracing::instrument`?** Recomendo **sim mas com `skip(config)` obrigatório** — `database_url` é secret. Campos que gravamos no span: `max_connections`, `migrate_on_start`. Nenhum PII. Confirma?

7. **Adicionar `garraia-workspace` como feature flag opcional do gateway ou deixar sem wiring?** Recomendo **sem wiring nesta PR** (seção §3 Out of scope). Gateway só começa a usar o crate em GAR-393 (API /v1/groups). Isso garante que merge deste PR não mexe no comportamento do gateway em produção. Confirma?

---

## 13. Next recommended issue (depois de GAR-407 merged)

Com o crate + migration 001 + test infra estáveis, as próximas issues são mecânicas:

- **GAR-386 — migration 002 RBAC** (`roles`, `permissions`, `role_permissions`, `audit_events`): 2-3h. Segue o mesmo padrão, só muda o SQL.
- **GAR-387 — migration 003 files** (`folders`, `files`, `file_versions`, `file_shares`): 2-3h.
- **GAR-388 — migration 004 chats** (`chats`, `chat_members`, `messages` + tsvector + GIN): 3-4h (FTS tem nuances).
- **GAR-389 — migration 005 memory** (`memory_items`, `memory_embeddings` + HNSW): 2-3h (tem pgvector specifics já validados no benchmark).
- **GAR-390 — migration 006 tasks** (`task_lists`, `tasks`, `task_assignees`, `task_labels`, `task_comments`, `task_activity`): 3-4h.
- **GAR-408 — migration 007 RLS** (ENABLE + FORCE + CREATE POLICY em todas as tenant-scoped tables): 2-3h (política já definida no ADR 0003).

**Ordem ótima:** GAR-386 (RBAC primeiro porque audit_events é pré-requisito pra todas as outras migrations auditarem mudanças) → GAR-408 (RLS antes de ter dados para proteger, para forçar o pattern desde o início) → GAR-387/388/389/390 em paralelo ou serial conforme tempo.

Alternativamente, depois de GAR-407, posso dar um passo lateral e fazer **GAR-379 (`garraia-config` reativo)** — independente da Fase 3, destrava iteração rápida de config nas outras issues. Mas isso é otimização de workflow, não caminho crítico.

Recomendação firme: **GAR-386 é o próximo natural** depois deste.

---

**Aguardando sua aprovação.** Se aprovar como está, respondo as 7 open questions do §12 com os defaults recomendados (a menos que você ajuste) e começo pelo passo 1 do §10. Se quiser cortar escopo (ex.: "deixa `citext` de fora, usa `text`", "skip `legacy_sqlite_id` column", "NÃO instrumenta com tracing"), me diga antes que eu toque em código.
