# Plan 0004: GAR-386 — Migration 002 RBAC + audit_events

> **Status:** 📋 Awaiting approval
> **Issue:** [GAR-386](https://linear.app/chatgpt25/issue/GAR-386)
> **Project:** Fase 3 — Group Workspace
> **Labels:** `epic:ws-schema`, `security`
> **Priority:** High
> **Estimated session size:** 2-3 horas de trabalho focado
> **Author:** Claude Opus 4.6 + @michelbr84
> **Date:** 2026-04-13
> **Depends on:** ✅ GAR-407 (migration 001 users/groups, merged `4c0f07e`)
> **Unblocks:** GAR-391 (`garraia-auth`), GAR-392 (cross-group authz tests), parcialmente GAR-387/388/389/390/408
> **Absorbs:** GAR-414 M1 (single-owner partial unique index) + M3 (audit trail para CASCADE deletes)

---

## 1. Goal (one sentence)

Adicionar `migration 002_rbac_and_audit.sql` ao crate `garraia-workspace` criando 4 tabelas (`roles`, `permissions`, `role_permissions`, `audit_events`), seedando 5 roles × ~20 permissions com mapping canônico da seção §3.3 do ROADMAP, fechando o gap de single-owner constraint do GAR-414 M1, e estendendo o smoke test existente para validar que a migration aplica, seed está presente e audit row pode ser inserido — tudo sob o mesmo padrão estabelecido em GAR-407, em uma sessão curta.

---

## 2. Rationale — por que esse agora

1. **Caminho crítico direto.** Com migration 001 merged, a próxima fatia é óbvia. GAR-391 (`garraia-auth`) precisa de `roles`/`permissions`/`role_permissions` e `audit_events` como base de dados antes que o Principal::can() possa ser escrito.
2. **Padrão validado.** GAR-407 estabeleceu: scaffold, migrator, testcontainer, smoke test, review loop. Este plan só estende o que já funciona — zero research, zero plumbing novo.
3. **Tamanho cabível.** 2-3h contra 4-6h de GAR-407. Mais mecânico: uma migration SQL + seed + extension do teste + reviews.
4. **Absorve GAR-414.** Os dois must-fix-after-merge do security review de GAR-407 (M1 single-owner, M3 audit trail) se encaixam naturalmente nesta migration. Um único PR resolve três issues (GAR-386 + GAR-414).
5. **Fechamento de loop de defesa.** Sem `audit_events` não há como cumprir LGPD art. 8 §5 / GDPR art. 17(1) para CASCADE deletes. Cada dia que isso fica aberto é risco de compliance.

---

## 3. Scope & Non-Scope

### In scope

- **Migration 002** em `crates/garraia-workspace/migrations/002_rbac_and_audit.sql` com:
  - Tabela `roles` (seed: owner, admin, member, guest, child)
  - Tabela `permissions` (seed: ~20 capabilities por §3.3 ROADMAP + §3.8 Tasks)
  - Tabela `role_permissions` (seed: mapping canônico)
  - Tabela `audit_events` (para trilhas de auditoria cross-cutting)
  - **Partial unique index** `group_members_single_owner_idx ON group_members(group_id) WHERE role = 'owner'` — fecha GAR-414 M1
  - Forward-only, sem DROP, sem destructive ALTER
- **Extensão do smoke test** `tests/migration_smoke.rs`:
  - Novos asserts: 4 novas tabelas existem, índice `group_members_single_owner_idx` existe
  - Verifica seed: `SELECT count(*) FROM roles` = 5, `permissions` = ~20, `role_permissions` > 0
  - Teste de violação: inserir dois rows em `group_members` com role='owner' para o mesmo group_id falha com SQLSTATE 23505
  - Teste de audit insert: criar um audit_events row minimamente válido e ler de volta
- **Update do README do crate** mencionando migration 002 no resumo.
- **Update do ROADMAP.md** §3.2: adicionar linha `[x] roles`, `[x] permissions`, `[x] role_permissions`, `[x] audit_events` (as entradas já estão no ROADMAP mas desmarcadas; marcar aqui).
- **Linear:**
  - GAR-386 → Done após merge
  - GAR-414 → Done após merge (absorvido neste PR)

### Out of scope

- ❌ **Sem `garraia-auth` crate.** Esse é GAR-391, próximo issue. Este plan só cria as tabelas + seed, não implementa `Principal::can()`.
- ❌ **Sem enforcement de audit trail em CASCADE delete.** A tabela existe, mas nenhuma API chama `INSERT INTO audit_events` ainda — isso vem com as funções Rust de CRUD (GAR-391 ou GAR-393). Este plan entrega a infra; o loop de compliance fecha quando o primeiro call site chamar a API de audit.
- ❌ **Sem file/chat/memory/task migrations.** Cada um tem seu issue: GAR-387, GAR-388, GAR-389, GAR-390.
- ❌ **Sem RLS.** `audit_events` precisa de RLS eventualmente (só membros do grupo veem auditoria do grupo), mas a policy vem em migration 007 (GAR-408) junto com todas as outras scoped tables.
- ❌ **Sem mudanças em migration 001.** Forward-only: a correção do `group_members.role` single-owner é feita como índice novo adicional, não alterando a tabela existente.
- ❌ **Sem Rust API nova.** Nenhuma função `create_role()`, `list_permissions()`. Só schema + seed.
- ❌ **Sem gateway wiring.** Mesmo padrão de GAR-407.
- ❌ **Sem GitHub Actions CI.** Ainda deferido.

---

## 4. Acceptance criteria (verificáveis)

- [ ] `cargo check --workspace` verde.
- [ ] `cargo check --workspace --no-default-features` verde.
- [ ] `cargo clippy -p garraia-workspace --all-targets -- -D warnings` verde.
- [ ] `cargo test -p garraia-workspace` verde — 5 unit (config) + 1 integration (smoke estendido) = 6+ tests passing.
- [ ] Smoke test wall time ≤ 15 segundos (era 7s em GAR-407; +1 migration adiciona ~1-2s no seed).
- [ ] Migration 002 aplica do zero (após migration 001) sem erros em ≤ 500ms.
- [ ] 4 tabelas novas existem após migration: `roles`, `permissions`, `role_permissions`, `audit_events`.
- [ ] Seed verificado: `roles` tem exatamente 5 rows (owner, admin, member, guest, child); `permissions` tem ≥ 18 rows; `role_permissions` tem ≥ 30 rows (owner tem tudo, admin/member menos, guest/child mínimo).
- [ ] Partial unique index `group_members_single_owner_idx` existe em `pg_indexes`.
- [ ] Teste de violação single-owner: inserir dois owners para o mesmo group_id falha com SQLSTATE 23505.
- [ ] Teste de audit_events: insert mínimo (apenas colunas NOT NULL) + SELECT bem sucedido.
- [ ] Migration é forward-only (sem DROP TABLE, sem destructive ALTER).
- [ ] `#[tracing::instrument]` coverage mantido (nenhum regression).
- [ ] Review verde de `@code-reviewer` + `@security-auditor`.
- [ ] GAR-386 e GAR-414 movidos para Done após merge.
- [ ] ROADMAP.md §3.2 atualizado com os 4 novos `[x]`.

---

## 5. File-level changes

### 5.1 Novos arquivos

```
crates/garraia-workspace/
  migrations/
    002_rbac_and_audit.sql    # ★ a nova migration
```

**Nenhum outro arquivo novo.** O crate já existe, o test infra já existe, a Cargo.toml já tem tudo.

### 5.2 Edits em arquivos existentes

- `crates/garraia-workspace/tests/migration_smoke.rs`: adicionar ~30 linhas para validar migration 002 (4 novas asserts de tabelas, seed counts, single-owner violation test, audit insert test).
- `crates/garraia-workspace/README.md`: adicionar uma linha no §Scope mencionando migration 002.
- `ROADMAP.md` §3.2: marcar `roles`, `permissions`, `role_permissions`, `audit_events` como `[x]`.

### 5.3 Zero edits em Rust source

- `src/lib.rs`, `src/config.rs`, `src/error.rs`, `src/store.rs` — intocados. `sqlx::migrate!("./migrations")` automaticamente pega o novo arquivo porque é macro que escaneia o diretório em compile-time.

---

## 6. Schema details (o SQL completo)

### 6.1 roles

Lookup table com seed estático. A CHECK constraint em `group_members.role` da migration 001 é a autoridade runtime; esta tabela existe para join queries, admin UI e `role_permissions` FK.

```sql
CREATE TABLE roles (
    id           text        PRIMARY KEY,
    display_name text        NOT NULL,
    description  text        NOT NULL,
    tier         int         NOT NULL,
    created_at   timestamptz NOT NULL DEFAULT now()
);

COMMENT ON TABLE roles IS 'Lookup table of capability tiers. Runtime authority is group_members.role CHECK constraint from migration 001.';
COMMENT ON COLUMN roles.tier IS 'Numeric ordering: higher = more privilege. owner=100, admin=80, member=50, guest=20, child=10.';

INSERT INTO roles (id, display_name, description, tier) VALUES
    ('owner',  'Owner',         'Full control. Can delete the group, manage members, manage billing. Exactly one per group enforced by partial unique index.', 100),
    ('admin',  'Admin',         'Manage members and settings, moderate chats, manage folders.', 80),
    ('member', 'Member',        'Create/edit content, send messages, upload to permitted folders.', 50),
    ('guest',  'Guest',         'Read + limited contribution to explicitly-shared resources.', 20),
    ('child',  'Child',         'Guest capabilities + content filter + no export/share.', 10);
```

### 6.2 permissions

Capability strings grouped by resource. Seed matches §3.3 and §3.8 of ROADMAP.

```sql
CREATE TABLE permissions (
    id          text        PRIMARY KEY,
    resource    text        NOT NULL,
    action      text        NOT NULL,
    description text        NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (resource, action)
);

COMMENT ON TABLE permissions IS 'Canonical capability strings. Application layer (garraia-auth, GAR-391) loads these into a compile-time map and uses them for fn can(principal, action) checks.';

INSERT INTO permissions (id, resource, action, description) VALUES
    -- Files (Fase 3.5)
    ('files.read',          'files',    'read',     'List and download files in permitted folders.'),
    ('files.write',         'files',    'write',    'Upload, rename, and replace files.'),
    ('files.delete',        'files',    'delete',   'Soft-delete files (moves to trash).'),
    ('files.share',         'files',    'share',    'Create share links or grant cross-group access.'),
    -- Chats (Fase 3.6)
    ('chats.read',          'chats',    'read',     'Read messages in subscribed channels.'),
    ('chats.write',         'chats',    'write',    'Send messages and replies.'),
    ('chats.moderate',      'chats',    'moderate', 'Delete others messages, pin, ban spammers.'),
    -- Memory (Fase 3.7)
    ('memory.read',         'memory',   'read',     'Query shared memory items by scope.'),
    ('memory.write',        'memory',   'write',    'Insert new memory items.'),
    ('memory.delete',       'memory',   'delete',   'Remove memory items (subject to retention policy).'),
    -- Tasks (Fase 3.8 Tier 1)
    ('tasks.read',          'tasks',    'read',     'View task lists and cards.'),
    ('tasks.write',         'tasks',    'write',    'Create and edit tasks.'),
    ('tasks.assign',        'tasks',    'assign',   'Assign tasks to members.'),
    ('tasks.delete',        'tasks',    'delete',   'Delete tasks or lists.'),
    -- Docs (Fase 3.8 Tier 2)
    ('docs.read',           'docs',     'read',     'Read collaborative doc pages.'),
    ('docs.write',          'docs',     'write',    'Edit doc pages and blocks.'),
    ('docs.delete',         'docs',     'delete',   'Archive or delete doc pages.'),
    -- Group admin
    ('members.manage',      'members',  'manage',   'Invite, remove, suspend, change roles of members.'),
    ('group.settings',      'group',    'settings', 'Modify group settings, retention policies, external sharing.'),
    ('group.delete',        'group',    'delete',   'Permanently delete the group and all its data.'),
    -- Export / compliance
    ('export.self',         'export',   'self',     'Export own data (LGPD / GDPR right to data portability).'),
    ('export.group',        'export',   'group',    'Export all group data.');
```

### 6.3 role_permissions

Many-to-many mapping. Owner gets everything. Others get subsets aligned with ROADMAP §3.3 + §3.8.

```sql
CREATE TABLE role_permissions (
    role_id       text NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    permission_id text NOT NULL REFERENCES permissions(id) ON DELETE CASCADE,
    PRIMARY KEY (role_id, permission_id)
);

-- Owner: full control (all permissions).
INSERT INTO role_permissions (role_id, permission_id)
SELECT 'owner', id FROM permissions;

-- Admin: everything except group.delete and export.group.
INSERT INTO role_permissions (role_id, permission_id) VALUES
    ('admin', 'files.read'),    ('admin', 'files.write'),    ('admin', 'files.delete'),    ('admin', 'files.share'),
    ('admin', 'chats.read'),    ('admin', 'chats.write'),    ('admin', 'chats.moderate'),
    ('admin', 'memory.read'),   ('admin', 'memory.write'),   ('admin', 'memory.delete'),
    ('admin', 'tasks.read'),    ('admin', 'tasks.write'),    ('admin', 'tasks.assign'),    ('admin', 'tasks.delete'),
    ('admin', 'docs.read'),     ('admin', 'docs.write'),     ('admin', 'docs.delete'),
    ('admin', 'members.manage'),('admin', 'group.settings'),
    ('admin', 'export.self');

-- Member: content create/edit, no member management, no delete on others.
INSERT INTO role_permissions (role_id, permission_id) VALUES
    ('member', 'files.read'),   ('member', 'files.write'),
    ('member', 'chats.read'),   ('member', 'chats.write'),
    ('member', 'memory.read'),  ('member', 'memory.write'),
    ('member', 'tasks.read'),   ('member', 'tasks.write'),
    ('member', 'docs.read'),    ('member', 'docs.write'),
    ('member', 'export.self');

-- Guest: read + contribute to explicit channels only (semantic guard in app layer).
INSERT INTO role_permissions (role_id, permission_id) VALUES
    ('guest', 'files.read'),
    ('guest', 'chats.read'),    ('guest', 'chats.write'),
    ('guest', 'tasks.read'),
    ('guest', 'docs.read'),
    ('guest', 'export.self');

-- Child: read + write chats/tasks (supervised), NO file/memory/docs/export/share.
INSERT INTO role_permissions (role_id, permission_id) VALUES
    ('child', 'chats.read'),    ('child', 'chats.write'),
    ('child', 'tasks.read'),    ('child', 'tasks.write');
```

### 6.4 audit_events

Central audit trail. Cross-cuts every tenant action. No FK to `groups` (nullable `group_id` for user-level audits like login/logout) to avoid coupling every failure to the group existing.

```sql
CREATE TABLE audit_events (
    id              uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    group_id        uuid,
    actor_user_id   uuid,
    actor_label     text,
    action          text        NOT NULL,
    resource_type   text        NOT NULL,
    resource_id     text,
    ip              inet,
    user_agent      text,
    metadata        jsonb       NOT NULL DEFAULT '{}'::jsonb,
    created_at      timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX audit_events_group_created_idx ON audit_events(group_id, created_at DESC)
    WHERE group_id IS NOT NULL;
CREATE INDEX audit_events_actor_created_idx ON audit_events(actor_user_id, created_at DESC)
    WHERE actor_user_id IS NOT NULL;
CREATE INDEX audit_events_action_idx ON audit_events(action);

COMMENT ON TABLE audit_events IS 'Central audit trail. No FK — rows survive CASCADE deletes so erasure is demonstrable. RLS added in migration 007 (GAR-408) so members only see their group events.';
COMMENT ON COLUMN audit_events.group_id IS 'NULL for user-level events (login, logout, self-export). Set for group-scoped events.';
COMMENT ON COLUMN audit_events.actor_user_id IS 'NULL when the actor is deleted (preserved via ON DELETE SET NULL — explicitly no FK to let the row survive hard erasure).';
COMMENT ON COLUMN audit_events.actor_label IS 'Cached display label at event time. Lets auditors see "who did what" after user deletion.';
COMMENT ON COLUMN audit_events.action IS 'Verb string: users.delete, files.download, roles.setRole, etc. Canonical from permissions.id where applicable.';
COMMENT ON COLUMN audit_events.resource_id IS 'Text, not uuid, to support non-UUID resources (paths, external IDs).';
COMMENT ON COLUMN audit_events.metadata IS 'Free-form jsonb for event-specific context. Must never contain secrets or raw PII beyond what audit requires.';
```

### 6.5 Single-owner constraint (closes GAR-414 M1)

```sql
-- Enforces exactly one 'owner' per group at the database layer. Complements
-- the CHECK constraint from migration 001 which only validates the enum value.
-- A second INSERT with role='owner' for the same group_id fails with SQLSTATE 23505.
CREATE UNIQUE INDEX group_members_single_owner_idx
    ON group_members(group_id)
    WHERE role = 'owner';
```

### Design notes

1. **No FK from `audit_events` to `users` or `groups`.** Deliberately. Audit rows must survive CASCADE deletes so erasure is demonstrable. `actor_user_id` is plain `uuid` without a constraint; `actor_label` caches the display name at event time for post-deletion visibility.
2. **`roles` and `permissions` as string PKs.** Cheaper joins, self-documenting in psql output, matches how Rust code will reference them (`"owner"`, `"files.write"`).
3. **Partial unique index, not UNIQUE on tuple.** `(group_id, role)` UNIQUE would block multiple admins per group. The partial index only restricts role=owner.
4. **`ip inet` type.** Postgres has a native IP type that handles both v4 and v6. Better than text.
5. **Seeded `role_permissions` count:** owner ≈ 22, admin ≈ 20, member ≈ 11, guest ≈ 6, child ≈ 4. Total ≈ 63 rows after seed.
6. **`audit_events.resource_id` as text, not uuid.** File paths, external IDs, non-UUID resources need to be representable.

---

## 7. Test plan

### 7.1 Extensions to `tests/migration_smoke.rs`

After the existing migration 001 assertions, add:

```rust
// ─── Migration 002 validation ──────────────────────────────────────────

// New tables exist.
for expected in &["roles", "permissions", "role_permissions", "audit_events"] {
    assert!(names.contains(expected), "missing table from migration 002: {expected}");
}

// Partial unique index exists.
assert!(
    index_names.contains(&"group_members_single_owner_idx"),
    "missing partial unique index group_members_single_owner_idx"
);

// Seed counts.
let roles_count: i64 = sqlx::query_scalar("SELECT count(*) FROM roles")
    .fetch_one(workspace.pool()).await?;
assert_eq!(roles_count, 5, "expected 5 seeded roles");

let perms_count: i64 = sqlx::query_scalar("SELECT count(*) FROM permissions")
    .fetch_one(workspace.pool()).await?;
assert!(perms_count >= 18, "expected at least 18 seeded permissions, got {perms_count}");

let owner_perms: i64 = sqlx::query_scalar(
    "SELECT count(*) FROM role_permissions WHERE role_id = 'owner'"
).fetch_one(workspace.pool()).await?;
assert_eq!(owner_perms, perms_count, "owner should have all permissions");

let child_perms: i64 = sqlx::query_scalar(
    "SELECT count(*) FROM role_permissions WHERE role_id = 'child'"
).fetch_one(workspace.pool()).await?;
assert!(child_perms < owner_perms, "child should have strictly fewer than owner");

// Single-owner constraint violation.
// Setup: create a group owned by the test user, try to add a second owner.
let group_id: uuid::Uuid = sqlx::query_scalar(
    "INSERT INTO groups (name, type, created_by) VALUES ($1, 'family', $2) RETURNING id"
)
.bind("Test Family")
.bind(user_id)
.fetch_one(workspace.pool()).await?;

sqlx::query(
    "INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'owner')"
)
.bind(group_id).bind(user_id)
.execute(workspace.pool()).await?;

// Create a second user and try to add them as another owner of the same group.
let user2_id: uuid::Uuid = sqlx::query_scalar(
    "INSERT INTO users (email, display_name) VALUES ($1, $2) RETURNING id"
)
.bind("second@example.com").bind("Second User")
.fetch_one(workspace.pool()).await?;

let dup_owner = sqlx::query(
    "INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'owner')"
)
.bind(group_id).bind(user2_id)
.execute(workspace.pool()).await
.expect_err("second owner for same group must be rejected");

let db_err = dup_owner.as_database_error().expect("database-layer error");
assert_eq!(
    db_err.code().as_deref(), Some("23505"),
    "expected unique_violation for single-owner constraint"
);

// Audit event insert + read-back.
let audit_id: uuid::Uuid = sqlx::query_scalar(
    "INSERT INTO audit_events (group_id, actor_user_id, actor_label, action, resource_type, resource_id, metadata) \
     VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb) RETURNING id"
)
.bind(group_id)
.bind(user_id)
.bind("Test User")
.bind("group.create")
.bind("group")
.bind(group_id.to_string())
.bind(r#"{"source":"smoke_test"}"#)
.fetch_one(workspace.pool()).await?;
assert!(!audit_id.is_nil());
```

### 7.2 What we are NOT testing

- No concurrent write tests. Single-owner enforcement is a unique index; Postgres handles concurrency natively.
- No full permission enforcement matrix. That's `garraia-auth` territory (GAR-391).
- No audit RLS. RLS comes in migration 007 (GAR-408).
- No `ON DELETE SET NULL` from users to audit_events. Audit rows are intentionally FK-less.

---

## 8. Rollback plan

Same as GAR-407, trivial at 3 levels:

1. **Before merge:** close the PR.
2. **After merge, before any downstream consumer:** `git revert`. The migration file is removed, `sqlx::migrate!()` no longer sees it, next `Workspace::connect` with `migrate_on_start=true` doesn't try to apply it. Existing production data (if any) with the new tables gets orphaned — but since there is no production yet, this is free.
3. **After downstream consumers ship:** forward-fix only. Write `003_rbac_remove.sql` that drops the tables in a controlled way. But this plan assumes we don't hit this — the design is conservative.

Zero secrets, zero breaking API changes, additive only.

---

## 9. Risks & mitigations

| Risco | Prob. | Impacto | Mitigação |
|---|---|---|---|
| Seed data fica desalinhado entre DB e Rust code de garraia-auth (GAR-391) | Média | Médio | GAR-391 usa os mesmos strings como constantes; adicionar sanity check no startup do auth que lista `permissions` e compara com a Rust const |
| Partial unique index tem bug em edge case (role='Owner' vs 'owner') | Baixa | Médio | CHECK constraint em migration 001 força lowercase; teste do smoke inclui assert explícito |
| `audit_events.metadata` vira dump de PII | Média | Alto | COMMENT ON COLUMN explícito proíbe; `garraia-auth` vai oferecer helper que sanitiza; flag no security review |
| Seed de role_permissions desalinhado com ROADMAP §3.3 | Média | Baixo | Security auditor revisa mapping contra ROADMAP §3.3 |
| Smoke test fica grande e frágil (muitos asserts em uma função) | Baixa | Baixo | Dividir em helpers internos se passar de ~150 linhas |
| Migration 002 não aplica porque migration 001 mudou ordem | Baixa | Alto | sqlx migrate macro garante ordem lexicográfica por filename; 001 < 002 |

---

## 10. Sequence of work (ordem proposta quando aprovado)

### Wave 1 — migration SQL + test extension (~1.5h, single agent)

1. Criar `crates/garraia-workspace/migrations/002_rbac_and_audit.sql` literalmente conforme §6.
2. Estender `crates/garraia-workspace/tests/migration_smoke.rs` com os asserts de §7.1.
3. Atualizar `crates/garraia-workspace/README.md` mencionando migration 002 no §Scope.
4. Rodar `cargo test -p garraia-workspace`. Iterar até verde.
5. Rodar `cargo clippy -p garraia-workspace --all-targets -- -D warnings`. Verde.
6. Verificar wall time do smoke test ≤ 15s.

### Wave 2 — parallel review (~20min wall, 2 agents background)

7. Spawn `@code-reviewer` com foco em SQL correctness, seed sanity, test validity.
8. Spawn `@security-auditor` com foco em audit_events PII prevention, RLS gap documentation, single-owner index semantics, seed review contra ROADMAP §3.3.

### Wave 3 — fixes + ROADMAP + commit (~30min, me)

9. Aplicar findings dos reviews inline.
10. Atualizar `ROADMAP.md §3.2` marcando 4 linhas como `[x]`.
11. Commit seguindo padrão dos anteriores.
12. Push.
13. Linear: GAR-386 → Done, GAR-414 → Done (absorvido).

**Total estimado: 2-3 horas.** Budget menor que GAR-407 porque não tem research, scaffold ou dep resolution.

---

## 11. Definition of Done

- [ ] Todos os 15 itens do §4 marcados.
- [ ] PR merged em `main`.
- [ ] Review verde de ambos agentes.
- [ ] Linear GAR-386 → Done com link para commit.
- [ ] Linear GAR-414 → Done (absorvido neste PR).
- [ ] Follow-up filed se algum review levantou algo fora de escopo.
- [ ] Próxima sessão pode abrir GAR-391 (`garraia-auth`) sem bloqueio de schema.

---

## 12. Open questions (preciso da sua resposta antes de começar)

1. **`roles`/`permissions` como lookup estático (seed) ou mutável (admin pode criar roles)?** Recomendo **estático nesta migration**. Mutabilidade futura entra como migration separada + endpoint admin. KISS. Confirma?

2. **Incluir permissions que ainda não têm backend (files, memory, docs)?** Recomendo **sim** — são as capability strings canônicas do ROADMAP §3.3 + §3.8. GAR-391 (`garraia-auth`) e migrations subsequentes esperam esses strings existirem. Seedar agora evita uma migration 003 só para adicionar linhas de permissão. Confirma?

3. **`audit_events` RLS agora ou em migration 007?** Recomendo **migration 007**. Consistente com todas as outras tenant-scoped tables: RLS centralizado em 007 facilita revisão. Até lá, audit queries são privilegiadas (app layer).

4. **`audit_events.actor_user_id` FK ou plain uuid?** Recomendo **plain uuid, sem FK** — rows sobrevivem hard erasure do usuário, que é o ponto central do audit. `actor_label` caches o nome para visibilidade post-deletion. Confirma?

5. **Tier numeric em `roles` (owner=100, admin=80, ...)?** Recomendo **sim**. Simplifica queries "usuários com tier >= 50 podem fazer X" e ordena naturalmente em listagens. Custo: 1 coluna int. Confirma?

6. **Seed exato de `role_permissions` do §6.3 está OK?** Revise a matriz:
   - Owner: tudo
   - Admin: tudo EXCETO `group.delete` + `export.group`
   - Member: content create/edit, sem `members.manage`, sem `files.delete/share`, sem `tasks.assign/delete`, sem `docs.delete`
   - Guest: read + `chats.write` + `tasks.read` + `docs.read` + `export.self`
   - Child: só `chats.read/write` + `tasks.read/write`

   Flag qualquer permission que você ache que deveria ser diferente. Particularmente: member deveria poder `files.delete` seus próprios arquivos? Isso vira enforcement app-layer (ownership check), não permission stamp. Mantenho como está.

7. **Include `group_members.role CHECK constraint` change?** Migration 001 tem `CHECK (role IN ('owner', 'admin', 'member', 'guest', 'child'))`. Devo **modificar** essa constraint via `ALTER TABLE ... DROP CONSTRAINT ... ADD CONSTRAINT ...` para referenciar `roles` table via FK? Recomendo **NÃO** — CHECK constraint já faz o trabalho, FK adicionaria overhead sem ganho, e mexer em migration 001 via ALTER viola forward-only-spirit. A tabela `roles` serve para joins/admin/documentation, não para integridade referencial. Confirma?

---

## 13. Next recommended issue (depois de GAR-386 merged)

Caminho crítico continua óbvio. Duas opções imediatas:

- **GAR-391 `garraia-auth` crate** (3-5 dias) — com `users`, `group_members`, `roles`, `permissions`, `role_permissions` e `audit_events` todos existindo, agora dá para escrever `Principal::can(action)`, extractor Axum, suite de testes authz. Depende de GAR-375 (ADR 0005 identity) que ainda não foi escrito — pode ser feito junto ou antes.
- **GAR-387/388/389/390 migrations 003-006** em paralelo (files, chats, memory, tasks) — trabalho mecânico seguindo o padrão, ~2-3h cada, zero research. Destravam mais da Fase 3 sem precisar do auth crate.

**Recomendação minha:** **GAR-388 migration 004 chats + FTS** seguido de **GAR-389 migration 005 memory + pgvector HNSW**. Motivos: chat é o core experience do Group Workspace; memory é o diferencial AI. Files (387) e tasks (390) vêm em seguida. `garraia-auth` (391) depois que as tenant-scoped tables todas existirem, porque o authz loop só fica completo com dados para proteger.

Alternativamente, `garraia-auth` agora cria demanda para as migrations subsequentes. Ordem é menos crítica a partir daqui — escolha baseada no que você quer ver funcionando primeiro.

---

**Aguardando sua aprovação.** Se aprovar como está, respondo as 7 open questions com os defaults recomendados (a menos que você ajuste) e começo pelo passo 1 do §10. Se quiser cortar escopo (ex.: "skip `audit_events`, faz só roles/permissions", "não seede `role_permissions`, deixa vazio"), me diga antes que eu toque em código.
