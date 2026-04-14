# Plan 0007: GAR-408 — Migration 007 Row-Level Security (FORCE RLS)

> **Status:** 📋 Awaiting approval
> **Issue:** [GAR-408](https://linear.app/chatgpt25/issue/GAR-408)
> **Project:** Fase 3 — Group Workspace
> **Labels:** `epic:ws-authz`, `security`
> **Priority:** Urgent
> **Estimated session size:** 3-4 horas de trabalho focado
> **Author:** Claude Opus 4.6 + @michelbr84
> **Date:** 2026-04-13
> **Depends on:** ✅ GAR-407 (users/groups) + ✅ GAR-386 (audit_events) + ✅ GAR-388 (chats/messages) + ✅ GAR-389 (memory)
> **Unblocks:** GAR-391 (`garraia-auth` pode escrever extractor Axum sabendo que o DB é a última linha de defesa) + GAR-393 (API /v1/groups com defense-in-depth) + todo trabalho de Fase 3 downstream

---

## 1. Goal (one sentence)

Adicionar `migration 007_row_level_security.sql` aplicando `ENABLE ROW LEVEL SECURITY + FORCE ROW LEVEL SECURITY + CREATE POLICY` em 10 tabelas tenant-scoped do `garraia-workspace` (`messages`, `chats`, `chat_members`, `message_threads`, `memory_items`, `memory_embeddings`, `audit_events`, `sessions`, `api_keys`, `user_identities`) usando `current_setting('app.current_group_id', true)` e `current_setting('app.current_user_id', true)` como contexto de request, fail-closed quando unset, e estendendo o smoke test para provar empiricamente que: (a) leitura cross-group retorna 0 rows, (b) unset settings retornam 0 rows, (c) FORCE RLS bloqueia até o owner da tabela, (d) policies com JOIN compõem corretamente — fechando o loop de defense-in-depth LGPD art. 46 / GDPR art. 32 antes de qualquer API code ser escrito.

---

## 2. Rationale — por que esse agora

1. **Momento único de RLS coverage completa.** Todas as tenant-scoped tables que precisam de isolação já existem no schema. Aplicar RLS em uma migration única evita que issues subsequentes (GAR-390 tasks, GAR-387 files, garraia-auth, API code) tenham que voltar para migrations retroativas ou — pior — dependam de enforcement app-layer frágil.
2. **Bloqueia API drift.** Se GAR-393 (`/v1/groups`) ou GAR-391 (`garraia-auth`) shipam antes de RLS, todo código presume isolação app-layer. Ligar RLS depois quebra queries que funcionavam e esconde bugs que passaram despercebidos. Fazer RLS agora força todo código subsequente a respeitar o contrato DB-level.
3. **Segurança da migration 001 fecha.** O `@security-auditor` já flagou no review de GAR-407 que sem `FORCE ROW LEVEL SECURITY` o table owner bypassa policies silenciosamente. Migration 007 é exatamente onde isso fica implementado e testado empiricamente.
4. **Padrão já validado.** O cenário B6 de `benches/database-poc/src/postgres_scenarios.rs` demonstrou policies + `SET LOCAL ROLE app_user` + `SET LOCAL app.current_group_id` em produção — 0 rows cross-group. Este plan institucionaliza esse padrão como primeira classe.
5. **Tamanho cabível — 3-4h.** Maior que as migrations anteriores porque cobre 10 tabelas com 3 classes distintas de policies (direct / JOIN / dual), mas sem tabelas novas, dep changes ou decisões arquiteturais — só SQL + smoke test extensivo.
6. **Fecha dívida de compliance antes de auditoria externa.** LGPD art. 46 demanda defense-in-depth demonstrável. Ter RLS ativo + teste empírico é a diferença entre "confiamos no app code" e "a auditoria passa".

---

## 3. Scope & Non-Scope

### In scope

- **Migration 007** em `crates/garraia-workspace/migrations/007_row_level_security.sql`:
  - `ENABLE ROW LEVEL SECURITY` + `FORCE ROW LEVEL SECURITY` + `CREATE POLICY` em **10 tabelas**:
    1. `messages` — direct policy via `group_id` denormalizado
    2. `chats` — direct policy via `group_id`
    3. `chat_members` — **JOIN policy** via `chats.group_id`
    4. `message_threads` — **JOIN policy** via `chats.group_id`
    5. `memory_items` — **dual policy** (group_id branch + user_id branch para `scope_type='user'`)
    6. `memory_embeddings` — **JOIN policy** via `memory_items` (recursivo: RLS compõe)
    7. `audit_events` — **dual policy** (group_id branch + actor_user_id branch para user-level events)
    8. `sessions` — **user policy** via `user_id = app.current_user_id`
    9. `api_keys` — **user policy** via `user_id = app.current_user_id`
    10. `user_identities` — **user policy** via `user_id = app.current_user_id` (sensível: holds password_hash)
  - **Criar role** `garraia_app` com `NOLOGIN` (fictício para testes) via migration — valida que `SET LOCAL ROLE garraia_app` é demotable desde o superuser connection do testcontainer.
  - **Fail-closed por design**: `current_setting('app.current_group_id', true)` retorna NULL quando unset → `group_id = NULL` → policy returns "not visible" → 0 rows. Não precisa de guard explícito, mas documentado em COMMENT.
  - **COMMENT ON POLICY** em cada policy explicando: classe (direct/join/dual), contexto esperado, fail-closed behavior.
  - **SQL block comment** no topo da migration listando quais tabelas estão EXPLICITAMENTE FORA de RLS (`roles`, `permissions`, `role_permissions`, `users`, `user_identities` parcial, `groups`, `group_members`, `group_invites`) e por quê.
  - Forward-only, sem DROP, sem destructive ALTER.

- **Extension do smoke test** `tests/migration_smoke.rs`:
  - Nova função auxiliar privada `rls_scope(pool, group_id, user_id)` que abre uma transação, faz `SET LOCAL ROLE garraia_app`, `SET LOCAL app.current_group_id = $1`, `SET LOCAL app.current_user_id = $2`, e retorna a transação para o caller executar queries dentro dela. Mesma técnica do benchmark B6.
  - **Cenário 1 — Positive read:** com scope = (grupo A, usuário A), `SELECT * FROM messages WHERE chat_id = $chat_id_A` retorna as 3 mensagens que o block de migration 004 inseriu.
  - **Cenário 2 — Cross-group read blocked:** criar grupo B + chat B + mensagem B (sem scope RLS, usando connection bypass). Com scope = (grupo A, usuário A), `SELECT * FROM messages WHERE id = $message_b_id` retorna 0 rows.
  - **Cenário 3 — Unset settings fail closed:** transação com `SET LOCAL ROLE garraia_app` mas SEM `SET LOCAL app.current_group_id` — `SELECT count(*) FROM messages` retorna 0.
  - **Cenário 4 — FORCE RLS vs owner:** sem `SET LOCAL ROLE` mas com a tabela tendo `FORCE RLS` — mesmo o owner superuser deve ser bloqueado quando `app.current_group_id` não está set. (Confirma que FORCE funciona na prática.)
  - **Cenário 5 — JOIN policy em chat_members:** scope = grupo A, `SELECT count(*) FROM chat_members` retorna só os membros dos chats de grupo A (não de grupo B).
  - **Cenário 6 — memory_items user-scope isolation:** scope = (grupo A, user A). Inserir via bypass um `memory_items` com `scope_type='user'`, `group_id=NULL`, `created_by=user_B`. Query dentro do scope deve retornar 0 rows (user policy branch bloqueia memórias pessoais de outro usuário).
  - **Cenário 7 — memory_embeddings via JOIN:** scope = grupo A, `SELECT count(*) FROM memory_embeddings` só retorna embeddings cujos memory_items são visíveis. Cross-group embeddings bloqueados pela composição RLS.
  - **Cenário 8 — audit_events dual:** tanto eventos de grupo (group_id = A) quanto eventos do próprio usuário (actor_user_id = A, group_id NULL) devem aparecer; eventos de outros usuários (group_id NULL, actor_user_id = B) devem NÃO aparecer.
  - Todos os 8 cenários usam `as_database_error` + count assertions explícitos, sem `.is_err()` frouxo.

- **README do crate:** §Scope atualizada para mencionar migration 007; §Running the tests documenta wall time estimado.
- **ROADMAP.md §3.2:** adicionar linha marcando RLS como `[x]` com link para o commit.
- **Linear GAR-408** → Done após merge.

### Out of scope

- ❌ **Sem RLS em `users`, `groups`, `group_members`, `group_invites`.** Estas são tabelas "tenant-root" — um usuário precisa ver grupos dos quais é membro, membros dos grupos dele, etc. RLS direta requer recursão ou JOIN caro, e a semântica "usuário vê só seus grupos" é mais eficiente em app-layer via `garraia-auth`. Documentado explicitamente no COMMENT block da migration.
- ❌ **Sem RLS em `roles`, `permissions`, `role_permissions`.** Lookup tables estáticas, leitura pública. RLS seria overhead sem benefício.
- ❌ **Sem RLS em `audit_events` por role do admin.** v1: todos os usuários veem seus próprios eventos (group + user). Admin vendo audit de terceiros requer BYPASSRLS role ou view privilegiada — fica em follow-up após GAR-391.
- ❌ **Sem Rust API para setar `app.current_*_id`.** O helper `rls_scope` no smoke test é internal test-only. API pública (extractor Axum) vem em GAR-391.
- ❌ **Sem perf tuning das JOIN policies.** Subqueries em USING cláusula funcionam correctamente mas podem ter overhead em tabelas grandes. Benchmark é follow-up se virar problema.
- ❌ **Sem policies WRITE-side separadas.** v1: uma policy USING única cobre SELECT + UPDATE + DELETE (por default). Write-specific policies (ex: "member can write own messages but not others'") ficam em `garraia-auth` layer via pre-insert check. Documentado.
- ❌ **Sem mudança em migrations 001-005.** RLS é aditivo — nenhuma tabela existente é alterada estruturalmente.
- ❌ **Sem integração com `SET LOCAL app.current_group_id` no connection pool do `garraia-workspace::Workspace`.** Isso é GAR-391 job (extractor Axum emite as `SET LOCAL` por request). O smoke test usa manualmente para validar o schema.
- ❌ **Sem permissions granulares (GRANT SELECT/INSERT/...)** no role `garraia_app`. v1: role criado só para `SET LOCAL ROLE` demote em testes. Operação real em produção usa role separada documentada em GAR-413 (migration tool) e no README do crate.

---

## 4. Acceptance criteria (verificáveis)

- [ ] `cargo check --workspace` verde.
- [ ] `cargo check --workspace --no-default-features` verde.
- [ ] `cargo clippy -p garraia-workspace --all-targets -- -D warnings` verde.
- [ ] `cargo test -p garraia-workspace` — 5 unit + 1 smoke verdes.
- [ ] Smoke test wall time ≤ 25 segundos (era 7.20s em GAR-389; +8 cenários de RLS + criação de grupo/user/chat/message/memory de teste cross-group adicionam ~3-8s).
- [ ] Migration 007 aplica do zero (após 001+002+004+005) em ≤ 1s.
- [ ] `pg_tables.rowsecurity = true` E `pg_class.relforcerowsecurity = true` em TODAS as 10 tabelas listadas no §3.
- [ ] Cada uma das 10 tabelas tem pelo menos 1 policy em `pg_policies`.
- [ ] Role `garraia_app` existe em `pg_roles`.
- [ ] Smoke test **Cenário 1** (positive read) retorna as 3 mensagens esperadas.
- [ ] Smoke test **Cenário 2** (cross-group blocked) retorna 0 rows.
- [ ] Smoke test **Cenário 3** (unset settings) retorna 0 rows.
- [ ] Smoke test **Cenário 4** (FORCE RLS vs owner) prova que até o superuser é bloqueado quando `SET LOCAL ROLE garraia_app` é aplicado.
- [ ] Smoke test **Cenário 5** (chat_members JOIN policy) retorna só membros do escopo.
- [ ] Smoke test **Cenário 6** (memory_items user-scope isolation) prova que memória pessoal de outro usuário NÃO vaza.
- [ ] Smoke test **Cenário 7** (memory_embeddings via JOIN) retorna 0 embeddings cross-group.
- [ ] Smoke test **Cenário 8** (audit_events dual policy) retorna corretamente group+self mas bloqueia outros usuários.
- [ ] Migration é forward-only.
- [ ] COMMENT ON POLICY em cada policy documenta classe, context esperado e fail-closed behavior.
- [ ] README do crate menciona migration 007 + RLS scope.
- [ ] Review verde de `@code-reviewer` + `@security-auditor`.
- [ ] GAR-408 movido para Done após merge.
- [ ] ROADMAP.md §3.2 atualizado com `[x]` para RLS row.

---

## 5. File-level changes

### 5.1 Novo arquivo

```
crates/garraia-workspace/migrations/
  007_row_level_security.sql    # ★ a migration crítica
```

### 5.2 Edits em arquivos existentes

- `crates/garraia-workspace/tests/migration_smoke.rs` — append ~180 linhas com helper `rls_scope` + 8 cenários de teste, após o bloco de migration 005, antes do `Ok(())` final. Mantém tudo anterior intacto.
- `crates/garraia-workspace/README.md` — §Scope ganha linha sobre migration 007 + RLS FORCE; §Required Postgres role privileges expandido notando que prod role NÃO deve ser superuser (RLS bypass) e como setar `SET LOCAL app.current_group_id` por request.
- `ROADMAP.md` §3.2 — adicionar linha `[x] RLS em todas as tenant-scoped tables — migration 007 + FORCE` com link.

### 5.3 Zero edits em Rust source

- `src/lib.rs`, `src/config.rs`, `src/error.rs`, `src/store.rs` — intocados. RLS é puramente SQL-level.
- Lesson learned ainda válida: `cargo clean -p garraia-workspace && cargo test -p garraia-workspace` antes do primeiro run.

---

## 6. Schema details (o SQL completo)

> **⚠️ Wave 1 correction (2026-04-13):** the pre-wave-1 drafts of §6.2–§6.11
> below used `current_setting('app.<name>', true)::uuid` WITHOUT a `NULLIF(..., '')`
> wrapper. That is **incorrect** because custom PostgreSQL GUCs return an empty
> string `''` (not NULL) when the parameter has been declared anywhere in the
> cluster but not set in the current transaction. Casting `''::uuid` raises
> SQLSTATE 22P02 `invalid_text_representation`, which aborts the transaction
> instead of silently failing closed with 0 rows. Wave 1 discovered this
> empirically while running the smoke test and applied `NULLIF(..., '')::uuid`
> wrapping to every `current_setting` call. The §6 SQL blocks below have been
> **updated in-place** to reflect the final correct form that shipped in the
> migration. This amendment preserves the historical `plan § versus final SQL`
> audit trail: the canonical reference for the shipped migration is
> `crates/garraia-workspace/migrations/007_row_level_security.sql`.

### 6.1 Cabeçalho e setup

```sql
-- 007_row_level_security.sql
-- GAR-408 — Migration 007: Row-Level Security em 10 tabelas tenant-scoped.
-- Plan:     plans/0007-gar-408-migration-007-rls.md
-- Depends:  migrations 001 (users/groups), 002 (audit_events), 004 (chats/messages),
--           005 (memory_items/memory_embeddings).
-- Forward-only. No DROP, no destructive ALTER.
--
-- ─── Scope decisions ──────────────────────────────────────────────────────
--
-- IN scope (10 tables with ENABLE + FORCE + CREATE POLICY):
--   messages, chats, chat_members, message_threads,
--   memory_items, memory_embeddings, audit_events,
--   sessions, api_keys, user_identities
--
-- OUT of scope (no RLS — app-layer enforcement only):
--   users               → a user needs to see members of their groups;
--                         recursive RLS is expensive. garraia-auth controls
--                         via SELECT projection with partial columns.
--   groups              → a user sees groups they are a member of; requires
--                         correlated subquery to group_members with its own
--                         RLS. App-layer join is cleaner in v1.
--   group_members       → same reasoning.
--   group_invites       → token-based access, handled by the invite endpoint.
--   roles, permissions, role_permissions → static lookup, public read.
--
-- ─── Contract ──────────────────────────────────────────────────────────────
-- Every request that touches RLS-protected tables must, at the start of the
-- transaction, execute:
--     SET LOCAL app.current_group_id = '<uuid>';
--     SET LOCAL app.current_user_id  = '<uuid>';
-- A connection pool (garraia-workspace::Workspace) MUST NOT reuse a cached
-- setting across transactions. garraia-auth extractor (GAR-391) is the
-- canonical caller.
--
-- Fail-closed: current_setting(..., true) returns NULL when unset. NULL
-- comparisons return NULL in USING clauses, which RLS treats as not-visible
-- → 0 rows. This is a feature, not a bug: a missing SET LOCAL causes queries
-- to silently return empty sets instead of leaking cross-tenant data. The
-- Axum extractor MUST return 500 if the Principal has no group_id instead
-- of trusting the empty result.
--
-- FORCE ROW LEVEL SECURITY is REQUIRED on every table — without it, the
-- table owner (typically the role running migrations, and commonly the
-- same role the app pool uses) bypasses the policy entirely. Benchmark
-- B6 in benches/database-poc/ proves this.

-- Role used by smoke tests to demote from superuser. Not used in production.
-- Production apps create a separate least-privilege role (documented in
-- crate README). IF NOT EXISTS because migrations run once per DB lifetime
-- and the role persists across application restarts in real deployments.
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'garraia_app') THEN
        CREATE ROLE garraia_app NOLOGIN;
    END IF;
END
$$;

-- Grant minimal perms to garraia_app so SELECT through RLS works in tests.
-- Production deployments should use a distinct role with finer-grained grants.
GRANT USAGE ON SCHEMA public TO garraia_app;
GRANT SELECT ON ALL TABLES IN SCHEMA public TO garraia_app;
GRANT INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO garraia_app;
```

### 6.2 messages — direct policy

```sql
ALTER TABLE messages ENABLE ROW LEVEL SECURITY;
ALTER TABLE messages FORCE ROW LEVEL SECURITY;

CREATE POLICY messages_group_isolation ON messages
    USING (group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid);

COMMENT ON POLICY messages_group_isolation ON messages IS
    'Class: direct. Context: app.current_group_id. Fail-closed when unset (NULL=NULL yields NULL → not visible). Uses denormalized group_id column to avoid JOIN overhead.';
```

### 6.3 chats — direct policy

```sql
ALTER TABLE chats ENABLE ROW LEVEL SECURITY;
ALTER TABLE chats FORCE ROW LEVEL SECURITY;

CREATE POLICY chats_group_isolation ON chats
    USING (group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid);

COMMENT ON POLICY chats_group_isolation ON chats IS
    'Class: direct. Context: app.current_group_id. Fail-closed when unset. Direct filter on group_id column.';
```

### 6.4 chat_members — JOIN policy via chats

```sql
ALTER TABLE chat_members ENABLE ROW LEVEL SECURITY;
ALTER TABLE chat_members FORCE ROW LEVEL SECURITY;

CREATE POLICY chat_members_through_chats ON chat_members
    USING (
        chat_id IN (
            SELECT id FROM chats
            WHERE group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid
        )
    );

COMMENT ON POLICY chat_members_through_chats ON chat_members IS
    'Class: JOIN. Resolution: chat_id → chats.group_id → app.current_group_id. The subquery against chats is itself RLS-protected, so the composition is safe (chats RLS filters to current group first, then chat_members FK subquery uses that filtered set). Slight overhead vs direct policy; acceptable because chat_members is small (bounded by members per group).';
```

### 6.5 message_threads — JOIN policy via chats

```sql
ALTER TABLE message_threads ENABLE ROW LEVEL SECURITY;
ALTER TABLE message_threads FORCE ROW LEVEL SECURITY;

CREATE POLICY message_threads_through_chats ON message_threads
    USING (
        chat_id IN (
            SELECT id FROM chats
            WHERE group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid
        )
    );

COMMENT ON POLICY message_threads_through_chats ON message_threads IS
    'Class: JOIN. Resolution: chat_id → chats.group_id → app.current_group_id. Same pattern as chat_members_through_chats.';
```

### 6.6 memory_items — dual policy

```sql
ALTER TABLE memory_items ENABLE ROW LEVEL SECURITY;
ALTER TABLE memory_items FORCE ROW LEVEL SECURITY;

CREATE POLICY memory_items_group_or_self ON memory_items
    USING (
        (group_id IS NOT NULL
         AND group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid)
        OR
        (group_id IS NULL
         AND created_by = NULLIF(current_setting('app.current_user_id', true), '')::uuid)
    );

COMMENT ON POLICY memory_items_group_or_self ON memory_items IS
    'Class: dual. Branch 1 (group): group_id IS NOT NULL AND group_id = app.current_group_id — covers scope_type=group and scope_type=chat. Branch 2 (user): group_id IS NULL AND created_by = app.current_user_id — covers scope_type=user personal memories. Both settings must be set for full coverage; missing either branch silently filters that branch away (fail-closed).';
```

### 6.7 memory_embeddings — JOIN policy via memory_items

```sql
ALTER TABLE memory_embeddings ENABLE ROW LEVEL SECURITY;
ALTER TABLE memory_embeddings FORCE ROW LEVEL SECURITY;

CREATE POLICY memory_embeddings_through_items ON memory_embeddings
    USING (
        memory_item_id IN (SELECT id FROM memory_items)
    );

COMMENT ON POLICY memory_embeddings_through_items ON memory_embeddings IS
    'Class: JOIN (implicit recursive). The subquery against memory_items is itself RLS-protected, so it already returns only rows visible to the current scope (group + self branches). The FK subquery then filters embeddings to those visible items. This means ANN queries that go directly against memory_embeddings respect RLS automatically — but per the COMMENT on memory_embeddings, direct ANN queries are still discouraged until GAR-391 ships proper retrieval helpers.';
```

### 6.8 audit_events — dual policy

```sql
ALTER TABLE audit_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE audit_events FORCE ROW LEVEL SECURITY;

CREATE POLICY audit_events_group_or_self ON audit_events
    USING (
        (group_id IS NOT NULL
         AND group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid)
        OR
        (group_id IS NULL
         AND actor_user_id = NULLIF(current_setting('app.current_user_id', true), '')::uuid)
    );

COMMENT ON POLICY audit_events_group_or_self ON audit_events IS
    'Class: dual. Branch 1 (group audit): events bound to a group visible when inside that group scope. Branch 2 (user audit): user-level events (login/logout/self-export) visible only to the actor themselves. NOTE: admin viewing another user audit trail is NOT covered by this policy — requires a BYPASSRLS role or a security-definer function, deferred to GAR-391 admin endpoints.';
```

### 6.9 sessions — user policy

```sql
ALTER TABLE sessions ENABLE ROW LEVEL SECURITY;
ALTER TABLE sessions FORCE ROW LEVEL SECURITY;

CREATE POLICY sessions_owner_only ON sessions
    USING (user_id = NULLIF(current_setting('app.current_user_id', true), '')::uuid);

COMMENT ON POLICY sessions_owner_only ON sessions IS
    'Class: user. A session row is visible only to its owning user. Fail-closed when app.current_user_id is unset.';
```

### 6.10 api_keys — user policy

```sql
ALTER TABLE api_keys ENABLE ROW LEVEL SECURITY;
ALTER TABLE api_keys FORCE ROW LEVEL SECURITY;

CREATE POLICY api_keys_owner_only ON api_keys
    USING (user_id = NULLIF(current_setting('app.current_user_id', true), '')::uuid);

COMMENT ON POLICY api_keys_owner_only ON api_keys IS
    'Class: user. Same as sessions_owner_only. Protects key_hash from cross-user reads.';
```

### 6.11 user_identities — user policy

```sql
ALTER TABLE user_identities ENABLE ROW LEVEL SECURITY;
ALTER TABLE user_identities FORCE ROW LEVEL SECURITY;

CREATE POLICY user_identities_owner_only ON user_identities
    USING (user_id = NULLIF(current_setting('app.current_user_id', true), '')::uuid);

COMMENT ON POLICY user_identities_owner_only ON user_identities IS
    'Class: user. Critical because this table holds password_hash. A user can only read their own identity records. Login flow in garraia-auth (GAR-391) must temporarily bypass RLS (via BYPASSRLS role or security-definer fn) to verify credentials, because at login time app.current_user_id is not yet known. Documented contract.';
```

### Design notes

1. **10 tables, 3 policy classes.** Direct (2: messages, chats), JOIN (3: chat_members, message_threads, memory_embeddings), dual (2: memory_items, audit_events), user (3: sessions, api_keys, user_identities). Total: 10 policies + 10 `ENABLE` + 10 `FORCE` = ~30 SQL statements.

2. **Why `current_setting(..., true)::uuid` not plain `::uuid`?** The `true` flag means "missing_ok" — returns NULL instead of raising an error. Essential for fail-closed semantics: an unset setting produces NULL which makes the USING clause return NULL which RLS treats as not-visible.

3. **Why not add denormalized `group_id` to `chat_members`/`message_threads`/`memory_embeddings` for direct policies?** Two reasons: (a) these tables are bounded in size (members per chat, threads per chat, embeddings per memory), so JOIN overhead is small; (b) denormalization means every write path must set group_id correctly, which is another invariant to maintain. The JOIN subquery is simpler and safer even if marginally slower.

4. **Why no write-side policies (`WITH CHECK`)?** v1: a single `USING` clause covers SELECT. For INSERT/UPDATE, we rely on app-layer validation (the extractor knows the Principal and sets `group_id` on the row before insert). Adding `WITH CHECK` would double-enforce but also double-complicate debugging of write failures. Follow-up after GAR-391 ships.

5. **Why `garraia_app NOLOGIN`?** NOLOGIN prevents anyone from connecting directly as this role. It exists purely as a target for `SET LOCAL ROLE` demotion from the superuser connection. Real production deployments use a distinct LOGIN role documented in the crate README.

6. **`GRANT SELECT ON ALL TABLES`** is a blunt instrument but acceptable because (a) RLS policies already enforce row-level filtering, and (b) static lookup tables (roles, permissions, role_permissions) are intended to be readable by all users anyway. Finer-grained grants are a follow-up for hardened prod deployments.

---

## 7. Test plan

### 7.1 Helper function `rls_scope`

Added as nested helper inside the test function, following the pattern of `unit_vector`:

```rust
// Helper: opens a transaction, demotes to garraia_app role, and sets both
// app.current_group_id and app.current_user_id for the duration of the
// transaction. Returns the transaction so the caller can run queries and
// then rollback/commit.
async fn rls_scope<'c>(
    pool: &'c sqlx::PgPool,
    group_id: Option<uuid::Uuid>,
    user_id: Option<uuid::Uuid>,
) -> anyhow::Result<sqlx::Transaction<'c, sqlx::Postgres>> {
    let mut tx = pool.begin().await?;
    sqlx::query("SET LOCAL ROLE garraia_app").execute(&mut *tx).await?;
    if let Some(gid) = group_id {
        // Dynamic SET LOCAL via format is intentional: the value is a Uuid
        // that has already been typed-checked by sqlx. No user input involved.
        let stmt = format!("SET LOCAL app.current_group_id = '{gid}'");
        sqlx::query(&stmt).execute(&mut *tx).await?;
    }
    if let Some(uid) = user_id {
        let stmt = format!("SET LOCAL app.current_user_id = '{uid}'");
        sqlx::query(&stmt).execute(&mut *tx).await?;
    }
    Ok(tx)
}
```

### 7.2 8 scenarios

**Cenário 1 — Positive read** (sanity):
```rust
let mut tx = rls_scope(workspace.pool(), Some(group_id), Some(user_id)).await?;
let count: i64 = sqlx::query_scalar("SELECT count(*) FROM messages WHERE chat_id = $1")
    .bind(chat_id)
    .fetch_one(&mut *tx).await?;
assert_eq!(count, 3, "positive read should see all 3 messages from migration 004");
tx.rollback().await?;
```

**Cenário 2 — Cross-group read blocked:**
- First, insert a second group `other_group_id` + `other_chat_id` + `other_message_id` using the superuser connection (bypass RLS via `pool()` directly without `rls_scope`).
- Then:
```rust
let mut tx = rls_scope(workspace.pool(), Some(group_id), Some(user_id)).await?;
let leaked: i64 = sqlx::query_scalar("SELECT count(*) FROM messages WHERE id = $1")
    .bind(other_message_id)
    .fetch_one(&mut *tx).await?;
assert_eq!(leaked, 0, "cross-group message must not be visible");
tx.rollback().await?;
```

**Cenário 3 — Unset settings fail-closed:**
```rust
let mut tx = rls_scope(workspace.pool(), None, None).await?;
let count: i64 = sqlx::query_scalar("SELECT count(*) FROM messages")
    .fetch_one(&mut *tx).await?;
assert_eq!(count, 0, "unset settings must yield 0 rows");
tx.rollback().await?;
```

**Cenário 4 — FORCE RLS vs superuser:**
The test container runs as the superuser `postgres`. Without `SET LOCAL ROLE`, `postgres` is also the table owner, and without FORCE would bypass RLS. Proof:
```rust
let mut tx = workspace.pool().begin().await?;
// No SET LOCAL ROLE — we're still 'postgres' (table owner).
// With FORCE RLS enabled, the owner is ALSO subject to policies.
let count: i64 = sqlx::query_scalar("SELECT count(*) FROM messages")
    .fetch_one(&mut *tx).await?;
assert_eq!(count, 0, "FORCE RLS must block even the table owner when app.current_group_id is unset");
tx.rollback().await?;
```

**Cenário 5 — chat_members JOIN policy:**
Insert a chat_members row into `other_group`'s chat (via superuser bypass). Then from scope(group_id, user_id):
```rust
let mut tx = rls_scope(workspace.pool(), Some(group_id), Some(user_id)).await?;
let own_members: i64 = sqlx::query_scalar(
    "SELECT count(*) FROM chat_members WHERE chat_id = $1"
).bind(chat_id).fetch_one(&mut *tx).await?;
assert!(own_members >= 1, "should see own chat member rows");
let other_members: i64 = sqlx::query_scalar(
    "SELECT count(*) FROM chat_members WHERE chat_id = $1"
).bind(other_chat_id).fetch_one(&mut *tx).await?;
assert_eq!(other_members, 0, "JOIN policy must block cross-group chat_members");
tx.rollback().await?;
```

**Cenário 6 — memory_items user-scope isolation:**
- Create a second user `user_b` (superuser bypass).
- Insert a memory_items with `scope_type='user'`, `group_id=NULL`, `created_by=user_b`.
- From scope(group_id, user_id_A):
```rust
let mut tx = rls_scope(workspace.pool(), Some(group_id), Some(user_id)).await?;
let visible_personal_other: i64 = sqlx::query_scalar(
    "SELECT count(*) FROM memory_items \
     WHERE scope_type = 'user' AND created_by = $1"
).bind(user_b_id).fetch_one(&mut *tx).await?;
assert_eq!(visible_personal_other, 0, "personal memory of another user must not leak");
tx.rollback().await?;
```

**Cenário 7 — memory_embeddings via JOIN:**
Insert an embedding for the other-user personal memory via superuser bypass. Then:
```rust
let mut tx = rls_scope(workspace.pool(), Some(group_id), Some(user_id)).await?;
let query_vec = unit_vector(999);
let hits: Vec<(uuid::Uuid,)> = sqlx::query_as(
    "SELECT memory_item_id FROM memory_embeddings \
     ORDER BY embedding <=> $1 LIMIT 10"
).bind(query_vec).fetch_all(&mut *tx).await?;
// Ensure the cross-user embedding is not in the results.
assert!(
    !hits.iter().any(|(id,)| *id == other_user_memory_id),
    "memory_embeddings RLS must filter via memory_items"
);
tx.rollback().await?;
```

**Cenário 8 — audit_events dual policy:**
- Insert (superuser bypass) 3 audit rows: one with (group_id=A, actor=user_A), one with (group_id=NULL, actor=user_A), one with (group_id=NULL, actor=user_B).
- From scope(group_id, user_id_A):
```rust
let mut tx = rls_scope(workspace.pool(), Some(group_id), Some(user_id)).await?;
let visible: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_events").fetch_one(&mut *tx).await?;
// Should see row 1 (group branch) + row 2 (user branch). NOT row 3.
assert_eq!(visible, 2, "audit_events dual policy: group+self visible, other user NOT");
tx.rollback().await?;
```

### 7.3 What we are NOT testing

- Performance of JOIN policies at scale (benchmarking is follow-up).
- Write paths through RLS (INSERT/UPDATE/DELETE restricted by USING but WITH CHECK absent).
- BYPASSRLS role for admin queries (GAR-391 follow-up).
- Composition behavior of nested RLS policies beyond 2 levels (memory_embeddings → memory_items is the deepest).
- Concurrent connections with conflicting `SET LOCAL` settings (transactions isolate natively).

---

## 8. Rollback plan

Three levels, same as previous plans:

1. **Before merge:** close the PR.
2. **After merge, before downstream consumer:** `git revert` the commit. The migration file is removed; the next `Workspace::connect` with `migrate_on_start=true` doesn't apply it. Existing test data (none in prod) with RLS enabled gets stuck until the migration is re-applied or manually `ALTER TABLE ... DISABLE ROW LEVEL SECURITY`ed — but since there's no prod DB yet, this is free.
3. **After downstream consumer (garraia-auth GAR-391 or API GAR-393 shipping):** forward-fix only. `ALTER TABLE ... DISABLE ROW LEVEL SECURITY` on specific tables if a bug is discovered, in a new migration.

Note: **FORCE RLS** does not leave behind any non-reversible state. Everything is `ALTER TABLE` statements that can be undone. Zero secrets involved. The `garraia_app` role persists across migrations but is harmless (NOLOGIN + limited grants).

---

## 9. Risks & mitigations

| Risco | Prob. | Impacto | Mitigação |
|---|---|---|---|
| `SET LOCAL ROLE garraia_app` falha no testcontainer | Baixa | Alto | Mesma técnica já validada no benchmark B6 em `benches/database-poc/src/postgres_scenarios.rs` — copiar o padrão exato |
| JOIN policy em chat_members tem perf ruim em dataset grande | Média | Médio | Smoke test só valida correctness, não perf. Follow-up de benchmark se virar problema. chat_members é bounded per chat |
| Policy recursiva (memory_embeddings → memory_items) falha em composition | Baixa | Médio | Postgres documenta que RLS compõe corretamente. Smoke test Cenário 7 valida empiricamente |
| `current_setting(..., true)` comporta diferente entre PG 16.x minors | Baixa | Baixo | É parte da API estável desde PG 9.6 |
| Smoke test timeout >25s devido aos múltiplos transactions | Baixa | Baixo | Cenários usam `rollback()` após cada query; transactions são fast-close. Budget folgado |
| Bug em dual policy faz scope_type=user leak para scope_type=group queries | Média | **Alto (LGPD)** | Cenário 6 cobre exatamente isso — personal memory de user B NÃO visível quando scope é group A + user A |
| Developer futuro esquece `SET LOCAL` em uma query específica e vê empty results | Média | Médio | Cenário 3 documenta o fail-closed behavior; README do crate + COMMENT ON POLICY explicitam o contrato; GAR-391 extractor retorna 500 antes de emitir query se Principal.group_id é None |
| GRANT SELECT para garraia_app permite leitura de roles/permissions tables | Baixa | Baixo | Intencional — são lookup tables públicas |
| Login flow do garraia-auth (GAR-391) não funciona com user_identities RLS | **Alta** | **Alto** | Documentado explicitamente no COMMENT ON POLICY — login precisa BYPASSRLS ou security-definer function. É concern de GAR-391, não bloqueia esta migration |

---

## 10. Sequence of work (ordem proposta quando aprovado)

### Wave 1 — migration SQL + smoke test extension (~2-2.5h, single agent)

1. Criar `crates/garraia-workspace/migrations/007_row_level_security.sql` literalmente conforme §6 (header + setup + 10 tabelas).
2. Estender `tests/migration_smoke.rs` com o helper `rls_scope` + 8 cenários do §7.
3. Atualizar `crates/garraia-workspace/README.md` §Scope + §Required Postgres role privileges (documentar pattern de production role com BYPASSRLS para login flow).
4. Rodar `cargo clean -p garraia-workspace && cargo test -p garraia-workspace`. Iterar.
5. `cargo clippy -p garraia-workspace --all-targets -- -D warnings`. Verde.
6. Verificar wall time ≤ 25s.

### Wave 2 — parallel review (~25min wall, 2 agents background)

7. `@security-auditor` — foco específico em RLS correctness: fail-closed paths, FORCE RLS coverage, JOIN policy composition, dual policy branch exhaustion, BYPASSRLS gap para login, out-of-scope tables rationale.
8. `@code-reviewer` — SQL correctness, policy syntax, helper `rls_scope` scoping, test cross-group fixture setup (cuidado com dependências entre cenários), forward-only compliance.

### Wave 3 — fixes + ROADMAP + commit (~30min, me)

9. Aplicar findings.
10. ROADMAP.md §3.2: adicionar linha `[x]` para RLS.
11. Commit + push.
12. Linear GAR-408 → Done.

**Total estimado: 3-4 horas.** Maior que GAR-389 por causa dos 8 cenários de teste e das 10 tabelas com 3 classes de policy distintas.

---

## 11. Definition of Done

- [ ] Todos os itens do §4 acceptance criteria marcados.
- [ ] PR merged em `main`.
- [ ] Review verde de ambos agentes.
- [ ] Linear GAR-408 → Done.
- [ ] ROADMAP §3.2 atualizado.
- [ ] Próxima sessão pode começar GAR-391 (`garraia-auth`) com total confiança no contrato DB-level: o extractor Axum só precisa SET LOCAL app.current_group_id e app.current_user_id por request, e o DB é a última linha de defesa.
- [ ] Próximo ADR (0005 Identity Provider, GAR-375) pode considerar login flow sabendo que user_identities tem RLS — precisa design explícito de BYPASSRLS role ou security-definer function.

---

## 12. Open questions (preciso da sua resposta antes de começar)

1. **Role de teste: `garraia_app NOLOGIN` ou usar `SET LOCAL app.current_group_id` sem demotar role?** Recomendo **`garraia_app NOLOGIN` + `SET LOCAL ROLE`** — é a única forma de testar empiricamente que `FORCE ROW LEVEL SECURITY` está fazendo seu trabalho (sem demote, o superuser bypassaria mesmo com FORCE... na verdade não, FORCE ROW LEVEL SECURITY afeta table owner também. Mas a distinção fica mais clara com o role switch). Confirma?

2. **`user_identities` sob RLS ou não?** Recomendo **sim, sob RLS** — contém `password_hash`, é o vetor mais sensível do schema. Login flow do GAR-391 precisa BYPASSRLS via role separada (documentado no COMMENT). A alternativa ("fica fora de RLS, login usa queries diretas") deixa `password_hash` lido por qualquer superuser leak. Confirma?

3. **Dual policy em `audit_events`: incluir actor_user_id = current_user_id ou apenas filter por group_id?** Recomendo **dual** — user-level events (login/logout/self-export) têm `group_id=NULL` e devem ser visíveis para o próprio usuário. Sem branch 2, esses eventos ficam invisíveis. Confirma?

4. **Tabelas NÃO-RLS (`users`, `groups`, `group_members`, `group_invites`): app-layer enforcement ou tentar RLS recursiva?** Recomendo **app-layer** — v1 simples e correto. Recursão (usuário vê grupos dos quais é membro → requer subquery em group_members → que também precisaria RLS → e assim por diante) introduz complexidade que não paga o retorno em v1. Confirma?

5. **JOIN policies via subquery `IN (SELECT ...)` ou desnormalizar `group_id` em chat_members/message_threads/memory_embeddings?** Recomendo **subquery** — tabelas são bounded em tamanho, desnormalização introduz invariante que pode drift. Se perf virar problema, adicionamos coluna denorm em migration futura (aditivo, seguro). Confirma?

6. **WITH CHECK clauses em INSERT/UPDATE ou só USING (leitura)?** Recomendo **só USING em v1** — USING cobre SELECT + dynamic-update rows; para INSERT, app-layer garante `group_id` correto antes de inserir (extractor sabe o Principal). Adicionar WITH CHECK dobra enforcement mas também dobra complexidade de debug de write failures. Follow-up após GAR-391. Confirma?

7. **Cenário 4 (FORCE RLS vs owner) é suficiente como teste de FORCE?** Recomendo **sim** — qualquer comportamento diferente indicaria que FORCE não está ativo, e `pg_class.relforcerowsecurity` check adicional confirma via metadata. Dois testes complementares: empírico (count=0) e metadata (`relforcerowsecurity=true`). Confirma que cobre?

---

## 13. Impact on GAR-391 and future APIs

Este é um dos pontos mais importantes do plan porque **muda a contract de todo código Rust que tocar `garraia-workspace` daqui pra frente**.

### 13.1 Contrato que GAR-391 (`garraia-auth`) herda

Após merge desta migration, **todo extractor Axum que produz um `Principal`** deve:

1. **No início de cada request**, antes de qualquer query contra tabelas RLS, abrir uma transação e executar:
   ```rust
   sqlx::query("SET LOCAL app.current_group_id = $1").bind(principal.group_id).execute(&mut *tx).await?;
   sqlx::query("SET LOCAL app.current_user_id = $1").bind(principal.user_id).execute(&mut *tx).await?;
   ```
2. **Nunca** cachear esses settings entre requests — `SET LOCAL` só vive até `COMMIT`/`ROLLBACK`.
3. **Retornar 500** se `Principal.group_id` for `None` para rotas que requerem group scope — não confiar que "0 rows no RLS" é equivalente a "rota válida com 0 resultados". Esse é o ponto que o security review do GAR-407 H2 destacou.
4. **Login flow** (autenticar credencial contra `user_identities`) precisa de uma role especial com `BYPASSRLS` ou uma função security-definer que lê `user_identities.password_hash` sem passar por RLS. Este plan documenta o gap via COMMENT mas não resolve — é responsabilidade de `garraia-auth` quando o ADR 0005 (GAR-375) for escrito.

### 13.2 Contrato que GAR-393 (`/v1/groups` API) herda

- Todo endpoint REST que opera em tabelas RLS deve passar pelo extractor antes de emitir SQL.
- Testes de integração de `/v1/*` devem incluir cenários cross-group idênticos aos do smoke test de migration 007 — provar que a API + o DB juntos fail-closed corretamente.
- Paginação não pode cachear resultados entre requests de usuários diferentes.

### 13.3 Contrato que ADR 0005 (GAR-375) deve cobrir

Quando o ADR 0005 (Identity Provider) for escrito, ele DEVE endereçar:
- Como o login flow lê `user_identities.password_hash` apesar da RLS.
- Qual role Postgres é usada pelo login endpoint (BYPASSRLS? security-definer fn?).
- Como a sessão é criada e `app.current_user_id` é setado na request de retorno.

### 13.4 Contrato que `garraia-cli migrate workspace` (GAR-413) herda

- A migration tool roda como superuser (necessário para CREATE EXTENSION e criação de tabelas).
- Durante o import SQLite → Postgres, o tool NÃO deve estar sob RLS — precisa inserir dados "como todos os usuários" em bulk.
- Pós-migração, o tool roda uma validação `SELECT count(*)` para cada tabela e compara com o source SQLite — essa query também precisa bypassar RLS ou setar `SET LOCAL` por grupo.

---

## 14. Next recommended issue (depois de GAR-408 merged)

Com RLS ativo em todas as tenant-scoped tables, o caminho crítico muda de "mais migrations" para "escrever código Rust real". Duas opções imediatas:

- **GAR-390 — Migration 006 tasks** (2-3h). Última migration mecânica. Completaria o schema set antes de qualquer API. Usa FORCE RLS no próprio arquivo (não precisa de migration 008 separada para retrofit).
- **GAR-375 — ADR 0005 Identity Provider** (research, ~3-4h). Destrava GAR-391 (`garraia-auth`). Precisa decidir login flow contra `user_identities` com RLS ligada — este é o blocker real identificado pelo security review desta migration.

**Recomendação firme:** **GAR-390 primeiro (migration 006 tasks)**, depois **GAR-375** (ADR identity).

**Rationale:** GAR-390 é trabalho mecânico de 2-3h que fecha o schema set completo de Fase 3 e pode incluir RLS diretamente (não precisa voltar pra migration 008). Depois disso, TODAS as tabelas tenant-scoped existem com RLS. A partir daí, o próximo passo é research (ADR 0005) que destrava `garraia-auth`, e a partir daí o trabalho vira predominantemente Rust.

Alternativa: se você preferir **destravar auth antes de tasks** — "tenho mais urgência em ver login funcionando do que em ter tasks" — então **GAR-375 primeiro**. É um trade-off legítimo de prioridade produto.

---

**Aguardando sua aprovação.** Se aprovar como está, respondo as 7 open questions com os defaults recomendados e executo seguindo o §10. Se quiser cortar escopo (ex.: "só 7 tabelas em vez de 10, deixa sessions/api_keys/user_identities para follow-up", "pula cenários 6/7/8 no smoke, deixa só os 5 primeiros"), me diga antes que eu toque em código.
