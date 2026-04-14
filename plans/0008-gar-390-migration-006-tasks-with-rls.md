# Plan 0008: GAR-390 — Migration 006 Tasks (Tier 1) com RLS embutido

> **Status:** 📋 Awaiting approval
> **Issue:** [GAR-390](https://linear.app/chatgpt25/issue/GAR-390)
> **Project:** Fase 3 — Group Workspace
> **Labels:** `epic:ws-schema`, `epic:ws-tasks`, `security`
> **Priority:** High
> **Estimated session size:** 2-3 horas de trabalho focado
> **Author:** Claude Opus 4.6 + @michelbr84
> **Date:** 2026-04-13
> **Depends on:** ✅ GAR-407 (users/groups) + ✅ GAR-386 (audit_events) + ✅ GAR-388 (chats/messages — mesmo padrão compound FK) + ✅ GAR-389 (memory — referência de scope tri-nível) + ✅ GAR-408 (RLS FORCE + NULLIF pattern + `garraia_app` role + `ALTER DEFAULT PRIVILEGES`)
> **Unblocks:** Fase 3.8 Tier 1 completa, GAR-391 (`garraia-auth` ganha mais uma superfície de testes authz), GAR-393 (API `/v1/task-lists`) do ROADMAP §3.8

---

## 1. Goal (one sentence)

Adicionar `migration 006_tasks_with_rls.sql` ao crate `garraia-workspace` criando 8 tabelas do módulo Notion-like Tier 1 (`task_lists`, `tasks`, `task_assignees`, `task_labels`, `task_label_assignments`, `task_comments`, `task_subscriptions`, `task_activity`) com subtasks via `parent_task_id` self-FK, compound FK `(list_id, group_id)` para fail-closed cross-group drift, enums CHECK-constrained para status/priority/activity kind, índices parciais para paginação e "my tasks", e **RLS FORCE aplicado na mesma migration** (sem retrofit) seguindo o padrão estabelecido em GAR-408 — fechando o schema set mecânico da Fase 3 antes de mudar de marcha para auth/API code.

---

## 2. Rationale — por que esse agora

1. **Última migration mecânica da Fase 3.** Depois desta, todas as tenant-scoped tables (messages/chats/memory/tasks/audit) existem com RLS ativo. A próxima classe de trabalho vira Rust real (garraia-auth GAR-391, API GAR-393).
2. **RLS embutido evita retrofit.** Diferente de GAR-388/389 que criaram tabelas sem RLS esperando migration 007, GAR-390 aproveita que o padrão está consolidado (NULLIF wrapping, COMMENT ON POLICY, ALTER DEFAULT PRIVILEGES já cobre grants para tasks automaticamente). Migration 006 inclui ENABLE + FORCE + CREATE POLICY direto — zero dívida de retrofit.
3. **Diferencial de produto.** Tasks/Notion-like é um dos pilares do ROADMAP §3.8 que distingue Garra de um chatbot puro. Fechar o schema permite que o frontend/UI comece a prototipar contra tabelas reais.
4. **Padrão 100% bench-validado.** Compound FK `(list_id, group_id)` vem de GAR-388 (`messages`). Denormalized `group_id` vem de GAR-388/389. Subtasks via `parent_task_id ON DELETE CASCADE` vem do pattern de `messages.reply_to_id`. RLS classes (direct/JOIN/user) vêm de GAR-408. Zero research.
5. **Tamanho cabível.** 2-3h. 8 tabelas é mais que GAR-389 (2 tabelas), menor que GAR-408 (10 tabelas mas todas existentes). Templates de policy e smoke test já existem — copiar-adaptar.
6. **Destrava GAR-391 parcialmente.** `garraia-auth` precisa de dados tenant-scoped para escrever authz tests reais. Com messages + memory + tasks todos existindo, a suite authz cross-group fica significativa.

---

## 3. Scope & Non-Scope

### In scope

- **Migration 006** em `crates/garraia-workspace/migrations/006_tasks_with_rls.sql`:
  - **8 tabelas novas:**
    1. `task_lists` — container (kanban/list/calendar view) por grupo
    2. `tasks` — a unidade de trabalho com subtasks via `parent_task_id`
    3. `task_assignees` — M:N user ↔ task
    4. `task_labels` — labels coloridas escopadas por grupo
    5. `task_label_assignments` — M:N label ↔ task
    6. `task_comments` — thread de comentários por task
    7. `task_subscriptions` — quem recebe notificações quando task muda
    8. `task_activity` — timeline UI-facing de eventos por task
  - **Enums CHECK-constrained:**
    - `task_lists.type IN ('list', 'board', 'calendar')`
    - `tasks.status IN ('backlog', 'todo', 'in_progress', 'review', 'done', 'canceled')`
    - `tasks.priority IN ('none', 'low', 'medium', 'high', 'urgent')`
    - `task_activity.kind IN ('created', 'status_changed', 'priority_changed', 'assigned', 'unassigned', 'labeled', 'unlabeled', 'commented', 'due_changed', 'archived', 'deleted', 'restored')`
  - **Compound FK** `tasks(list_id, group_id) REFERENCES task_lists(id, group_id)` — fail-closed contra cross-group drift, padrão de `messages`. Requer `UNIQUE (id, group_id)` em `task_lists`.
  - **`group_id` denormalizado** em `tasks`, `task_labels`, `task_activity` para RLS direct policy sem JOIN.
  - **Subtasks:** `tasks.parent_task_id uuid REFERENCES tasks(id) ON DELETE CASCADE`. Self-FK, depth limit enforçado via app layer (não schema). Subtask deve estar na mesma `list_id` que o parent — CHECK cross-column impossível, documentado como responsabilidade de app layer + audit query em GAR-391.
  - **Soft delete** via `deleted_at timestamptz` em `task_lists`, `tasks`, `task_comments`. `archived_at` em `task_lists` também (archive ≠ delete).
  - **`body_md CHECK length`** em `task_comments` (1..50000 chars — maior que messages por ser rich markdown, menor que docs). `description_md CHECK length` em `tasks` (1..50000).
  - **Caching patterns para erasure survival:** `task_comments.author_label`, `task_activity.actor_label` cached no insert time. Plain uuid `author_user_id`/`actor_user_id` sem FK em `task_activity` (sobrevive hard delete de user, igual `audit_events`). `task_comments.author_user_id` com `ON DELETE SET NULL`.
  - **Recurrence (RRULE):** `tasks.recurrence_rrule text` NULL-able + `CHECK (recurrence_rrule IS NULL OR recurrence_rrule ~ '^[A-Z:;,=0-9+-]+$')` — validação leve de formato RFC 5545, parsing real no app layer.
  - **Índices críticos:**
    - `tasks(list_id, status) WHERE deleted_at IS NULL` — listagem por status dentro de uma lista (kanban)
    - `tasks(group_id, status) WHERE deleted_at IS NULL` — "minhas tarefas em aberto no grupo"
    - `tasks(due_at) WHERE deleted_at IS NULL AND due_at IS NOT NULL` — "what's due soon"
    - `tasks(parent_task_id) WHERE parent_task_id IS NOT NULL AND deleted_at IS NULL` — listagem de subtasks
    - `task_assignees(user_id, assigned_at DESC)` — "tasks assigned to me" cross-list
    - `task_comments(task_id, created_at DESC) WHERE deleted_at IS NULL` — timeline de comments
    - `task_activity(task_id, created_at DESC)` — timeline UI
    - `task_activity(group_id, created_at DESC)` — activity feed do grupo
    - `task_labels(group_id)` — picker de labels no UI
    - `task_subscriptions(user_id)` — envio de notificações
  - **RLS FORCE embutido na mesma migration**, seguindo o padrão GAR-408:
    - Direct class: `task_lists`, `tasks`, `task_labels`, `task_activity` — via `group_id` denormalizado + NULLIF
    - JOIN class: `task_assignees`, `task_label_assignments`, `task_comments`, `task_subscriptions` — subquery via `tasks.group_id` ou `task_labels.group_id`
    - NULLIF wrapping em todo `current_setting` por decisão consolidada em GAR-408
    - COMMENT ON POLICY em cada policy documentando classe + contexto
  - **Sem novo role** (`garraia_app` + `ALTER DEFAULT PRIVILEGES` do GAR-408 já cobrem as tabelas novas automaticamente).
  - Forward-only, sem DROP, sem destructive ALTER.

- **Extension do smoke test** `tests/migration_smoke.rs`:
  - Assert 8 novas tabelas em `names`
  - Assert ~10 índices críticos em `index_names`
  - Assert `pg_class.relforcerowsecurity = true` para as 8 tabelas novas (extender o array existente do bloco GAR-408)
  - **Cenário de hierarquia (subtasks):** criar um parent task + 2 subtasks → CASCADE delete do parent → subtasks desaparecem
  - **Cenário de compound FK:** tentar criar task com `list_id` de lista A mas `group_id` de grupo B → SQLSTATE 23503
  - **Cenário RLS positive:** dentro de `rls_scope(group, user)`, inserir task + assignee + label + comment + activity, queriear e ver tudo
  - **Cenário RLS cross-group blocked:** via bypass, criar task em grupo B. Dentro de scope(A, user), `SELECT count(*) FROM tasks WHERE id = $other_task_id` → 0
  - **Cenário RLS JOIN (task_assignees):** via bypass, insert assignee de task do grupo B. Dentro de scope(A), count → 0
  - **Cenário RLS activity:** insert activity row do grupo B via bypass. Count dentro de scope(A) → 0
  - **Cenário negative (enum violation):** insert task com `status='invalid'` → SQLSTATE 23514
  - **Todos os cenários** reaproveitam `rls_scope` helper de GAR-408, `other_group_id` + `user_b_id` das fixtures cross-group já existentes

- **Update do README** do crate: §Scope ganha migration 006 + GAR-390. §Running the tests atualiza wall time para ≤ 30s.
- **Update do ROADMAP.md §3.8 Tier 1:** marcar as 9 tabelas de tasks como `[x]` (8 criadas + o `task_activity` já listado como sub-item). Marcar "single-owner constraint" N/A (tasks não tem owner único).
- **Linear GAR-390** → Done após merge.

### Out of scope

- ❌ **`task_attachments`.** FK para `files` que não existe (bloqueado por GAR-387 + ADR 0004 object storage). Defere explicitamente — documentado via COMMENT ON TABLE de `tasks` notando que attachments chegam quando files materializar.
- ❌ **Recurrence engine.** A coluna `recurrence_rrule` existe + CHECK de formato leve, mas o parser/expander RRULE é lógica de aplicação futura (Fase 2 ou worker dedicado).
- ❌ **Notification sending.** `task_subscriptions` só armazena quem deve receber — o fan-out para `garraia-channels` (Telegram/Discord/mobile push) é trabalho de GAR-397 (digest worker, já filed).
- ❌ **Task templates** / kanban swimlanes / burndown charts — features de Tier 2/3 do ROADMAP §3.8.
- ❌ **Rust API de CRUD.** Nenhuma fn `create_task`, `list_tasks`, `assign_task`. API vem em GAR-393.
- ❌ **WebSocket para kanban colaborativo.** Schema suporta, streaming é GAR-393 extension.
- ❌ **Workflow automation** (ex.: "quando task vai para done, notificar no chat"). Tier 3.
- ❌ **Integration com garraia-agents** (`@garra` mentions em comments disparando agente). Scope do ROADMAP §3.8 mas depende de API.
- ❌ **Migration retroativa para messages/chats/memory** — todas as tenant-scoped existentes já têm RLS via GAR-408.
- ❌ **Sem alterações em migrations 001-005 ou 007.**

---

## 4. Acceptance criteria (verificáveis)

- [ ] `cargo check --workspace` verde.
- [ ] `cargo check --workspace --no-default-features` verde.
- [ ] `cargo clippy -p garraia-workspace --all-targets -- -D warnings` verde.
- [ ] `cargo test -p garraia-workspace` — 5 unit + 1 smoke verdes.
- [ ] Smoke test wall time ≤ 30 segundos (era 9.48s em GAR-408; +8 cenários de tasks + RLS adicionam ~3-6s).
- [ ] Migration 006 aplica do zero (após 001+002+004+005) sem erros em ≤ 1s.
- [ ] 8 tabelas novas existem: `task_lists`, `tasks`, `task_assignees`, `task_labels`, `task_label_assignments`, `task_comments`, `task_subscriptions`, `task_activity`.
- [ ] `pg_class.relforcerowsecurity = true` para as 8 tabelas novas (array da assertion existente cresce de 10 para 18).
- [ ] Pelo menos 10 índices críticos existem (listados em §3).
- [ ] Subtask hierarchy test: parent + 2 subtasks inseridos, parent soft-deleted → subtasks ficam visíveis (soft delete não cascata); parent hard-deleted → subtasks desaparecem via CASCADE.
- [ ] Compound FK test: task com `list_id` de grupo A + `group_id` de grupo B é rejeitada com SQLSTATE 23503.
- [ ] RLS positive test: dentro do scope, 1 task + 1 assignee + 1 label + 1 comment + 1 activity visíveis.
- [ ] RLS cross-group test: 0 rows para cada uma das 8 tabelas quando consultadas fora do scope correto.
- [ ] RLS JOIN test (`task_assignees`/`task_comments`): 0 rows para registros cross-group.
- [ ] Enum CHECK test: `status='invalid'` retorna SQLSTATE 23514.
- [ ] Migration é forward-only.
- [ ] COMMENT ON POLICY em cada policy nova.
- [ ] README §Scope + §Running the tests atualizados.
- [ ] ROADMAP §3.8 Tier 1 com linhas `[x]`.
- [ ] Review verde de `@code-reviewer` + `@security-auditor`.
- [ ] GAR-390 → Done.

---

## 5. File-level changes

### 5.1 Novo arquivo

```
crates/garraia-workspace/migrations/
  006_tasks_with_rls.sql    # ★ 8 tabelas + 8 policies + índices
```

**Nota de ordenação:** `sqlx::migrate!` aplica em ordem lexicográfica. Em um container fresh, a sequência vira `001 → 002 → 004 → 005 → 006 → 007`. Isso significa que migration 006 executa **antes** de 007. 007 foi escrita com uma lista hardcoded de 10 tabelas para aplicar RLS — ela NÃO toca em nenhuma das novas tabelas de 006, então não há conflito. As tabelas de 006 entram sob RLS porque a própria 006 emite ENABLE + FORCE + CREATE POLICY. `garraia_app` já existe (criado em 007... espera, 007 roda DEPOIS de 006 na ordem lexicográfica!).

**Risco identificado:** migration 006 referencia `garraia_app` (via grants implícitos do `ALTER DEFAULT PRIVILEGES`) e usa o pattern `SET LOCAL ROLE garraia_app` nos testes. Mas `garraia_app` é criado em 007, que ainda não rodou quando 006 executa.

**Resolução:** Migration 006 **cria `garraia_app` ela mesma**, idempotente via `DO $$ IF NOT EXISTS $$`. 007 já tem o mesmo bloco idempotente — rodar duas vezes não é problema. Alternativa seria mover a role creation para uma migration ainda mais cedo (ex.: ampliar 002), mas isso viola forward-only no sentido semântico. Criar na 006 com IF NOT EXISTS é seguro e explícito.

**Grants via ALTER DEFAULT PRIVILEGES:** 007 emitiu `ALTER DEFAULT PRIVILEGES ... GRANT ... TO garraia_app` para cobrir tabelas futuras. Mas ALTER DEFAULT PRIVILEGES só afeta tabelas criadas **depois** da sua emissão. Como 007 roda **depois** de 006, as tabelas de 006 NÃO são cobertas automaticamente. Migration 006 precisa emitir GRANT explícito para `garraia_app` nas 8 tabelas novas.

Isso é um detalhe de ordenação que o plan endereça explicitamente no §6.

### 5.2 Edits em arquivos existentes

- `crates/garraia-workspace/tests/migration_smoke.rs` — append ~180 linhas após o bloco de RLS de GAR-408, antes do `Ok(())` final. Reuso máximo das fixtures cross-group (`other_group_id`, `user_b_id`, `other_chat_id`) que já existem.
- `crates/garraia-workspace/README.md` — §Scope + §Running the tests.
- `ROADMAP.md` §3.8 Tier 1 — marcar schema tables como `[x]`.

### 5.3 Zero edits em Rust source

- `src/lib.rs`, `src/config.rs`, `src/error.rs`, `src/store.rs` — intocados.
- `cargo clean -p garraia-workspace && cargo test -p garraia-workspace` padrão.

---

## 6. Schema details (o SQL completo)

### 6.0 Header + role creation (idempotent)

```sql
-- 006_tasks_with_rls.sql
-- GAR-390 — Migration 006: Tasks Tier 1 (Notion-like) com RLS FORCE
-- embutido desde o dia zero (sem retrofit via migration 007+).
-- Plan:     plans/0008-gar-390-migration-006-tasks-with-rls.md
-- Depends:  migrations 001 (users, groups), 002 (audit_events ref pattern),
--           004 (compound FK pattern de messages), 005 (scope patterns).
-- Forward-only. No DROP TABLE, no destructive ALTER.
--
-- ─── Scope decisions ──────────────────────────────────────────────────────
--
-- IN scope (8 tables with ENABLE + FORCE + CREATE POLICY):
--   task_lists, tasks, task_assignees, task_labels, task_label_assignments,
--   task_comments, task_subscriptions, task_activity
--
-- OUT of scope:
--   task_attachments → blocked by GAR-387 (files) + ADR 0004 (object storage)
--   Recurrence engine → app layer, column exists with format CHECK only
--   Notification sending → GAR-397 digest worker handles fan-out
--   Rust CRUD API → GAR-393
--
-- ─── Role dependency ──────────────────────────────────────────────────────
--
-- This migration runs BEFORE 007_row_level_security.sql in lexicographic
-- order. That means `garraia_app` role does not yet exist when 006 runs,
-- AND the `ALTER DEFAULT PRIVILEGES ... TO garraia_app` from 007 does not
-- cover tables created here (ADP only affects tables created after its
-- emission). Therefore migration 006:
--   1. Creates `garraia_app NOLOGIN` idempotently (same DO block as 007)
--   2. Explicitly GRANTs SELECT/INSERT/UPDATE/DELETE on every new table
-- When 007 runs, its own idempotent block is a no-op for the role, and its
-- ALTER DEFAULT PRIVILEGES still applies going forward for tables created
-- by any migration > 007.

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'garraia_app') THEN
        CREATE ROLE garraia_app NOLOGIN;
    END IF;
END
$$;
```

### 6.1 task_lists

```sql
CREATE TABLE task_lists (
    id          uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    group_id    uuid        NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    name        text        NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    type        text        NOT NULL CHECK (type IN ('list', 'board', 'calendar')),
    description text,
    created_by  uuid        NOT NULL REFERENCES users(id),
    settings    jsonb       NOT NULL DEFAULT '{}'::jsonb,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    archived_at timestamptz,
    CONSTRAINT task_lists_id_group_unique UNIQUE (id, group_id)
);

CREATE INDEX task_lists_group_idx ON task_lists(group_id)
    WHERE archived_at IS NULL;

COMMENT ON TABLE task_lists IS 'Container for tasks, analogous to a Notion database or a Linear project. type determines the default view. RLS direct policy via group_id; archived lists are filtered at query layer (not RLS).';
COMMENT ON COLUMN task_lists.type IS 'list → flat todo list; board → kanban with status columns; calendar → calendar view driven by due_at.';
COMMENT ON COLUMN task_lists.archived_at IS 'Soft archive. Archived lists remain queryable but hidden from default UI.';
COMMENT ON COLUMN task_lists.updated_at IS 'Caller responsibility — no trigger. Same pattern as users.updated_at.';
```

### 6.2 tasks

```sql
CREATE TABLE tasks (
    id                uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    list_id           uuid        NOT NULL,
    group_id          uuid        NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    parent_task_id    uuid        REFERENCES tasks(id) ON DELETE CASCADE,
    title             text        NOT NULL CHECK (length(title) BETWEEN 1 AND 500),
    description_md    text        CHECK (description_md IS NULL OR length(description_md) BETWEEN 1 AND 50000),
    status            text        NOT NULL DEFAULT 'todo'
                      CHECK (status IN ('backlog', 'todo', 'in_progress', 'review', 'done', 'canceled')),
    priority          text        NOT NULL DEFAULT 'none'
                      CHECK (priority IN ('none', 'low', 'medium', 'high', 'urgent')),
    due_at            timestamptz,
    started_at        timestamptz,
    completed_at      timestamptz,
    estimated_minutes int         CHECK (estimated_minutes IS NULL OR estimated_minutes BETWEEN 0 AND 100000),
    recurrence_rrule  text        CHECK (recurrence_rrule IS NULL OR recurrence_rrule ~ '^[A-Z:;,=0-9+\-]+$'),
    created_by        uuid        NOT NULL REFERENCES users(id),
    created_by_label  text        NOT NULL,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now(),
    deleted_at        timestamptz,
    FOREIGN KEY (list_id, group_id) REFERENCES task_lists(id, group_id) ON DELETE CASCADE,
    CONSTRAINT tasks_id_group_unique UNIQUE (id, group_id)
);

CREATE INDEX tasks_list_status_idx ON tasks(list_id, status) WHERE deleted_at IS NULL;
CREATE INDEX tasks_group_status_idx ON tasks(group_id, status) WHERE deleted_at IS NULL;
CREATE INDEX tasks_due_idx ON tasks(due_at) WHERE deleted_at IS NULL AND due_at IS NOT NULL;
CREATE INDEX tasks_parent_idx ON tasks(parent_task_id) WHERE parent_task_id IS NOT NULL AND deleted_at IS NULL;
CREATE INDEX tasks_completed_idx ON tasks(group_id, completed_at DESC) WHERE deleted_at IS NULL AND status = 'done';

COMMENT ON TABLE tasks IS 'Unit of work. Subtasks via parent_task_id self-FK (ON DELETE CASCADE — subtask dies with parent). Compound FK (list_id, group_id) fails-closed against cross-group drift — application code cannot silently insert a task claiming a different group_id than its list. RLS direct policy via group_id denormalized from task_lists.';
COMMENT ON COLUMN tasks.group_id IS 'Denormalized from task_lists.group_id and enforced by compound FK. Enables RLS direct policy without JOIN. Same pattern as messages.group_id.';
COMMENT ON COLUMN tasks.parent_task_id IS 'Self-FK for subtasks. Cross-list parenting is NOT enforced at schema level — app layer (GAR-391/GAR-393) must validate parent.list_id = child.list_id. Schema depth is unlimited; UI typically caps at 3 levels.';
COMMENT ON COLUMN tasks.status IS 'backlog → unrefined; todo → ready; in_progress → actively worked; review → awaiting review; done → complete; canceled → not going to happen. Transition rules (e.g., canceled↛done) are app-layer.';
COMMENT ON COLUMN tasks.description_md IS 'Markdown-rendered description. CHECK (1..50000) is a DoS mitigation — larger than message body but smaller than collaborative docs (Tier 2).';
COMMENT ON COLUMN tasks.recurrence_rrule IS 'Optional RFC 5545 RRULE string. CHECK validates charset only — full parsing and expansion is app-layer responsibility (future recurrence engine).';
COMMENT ON COLUMN tasks.created_by_label IS 'Cached display_name. Erasure survival — task remains readable after user hard-delete.';
COMMENT ON COLUMN tasks.deleted_at IS 'Soft delete. deleted_at IS NOT NULL hides from lists but subtasks and comments remain queryable for audit. Hard delete cascades via ON DELETE CASCADE to children and related rows.';
```

### 6.3 task_assignees (M:N user ↔ task)

```sql
CREATE TABLE task_assignees (
    task_id     uuid        NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    user_id     uuid        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    assigned_at timestamptz NOT NULL DEFAULT now(),
    assigned_by uuid        REFERENCES users(id) ON DELETE SET NULL,
    PRIMARY KEY (task_id, user_id)
);

CREATE INDEX task_assignees_user_idx ON task_assignees(user_id, assigned_at DESC);

COMMENT ON TABLE task_assignees IS 'M:N between tasks and users. Assignment is symmetric — no role (assignee vs owner vs watcher). Notification routing is task_subscriptions, not this table. RLS JOIN policy via tasks.group_id.';
COMMENT ON COLUMN task_assignees.assigned_by IS 'Who triggered the assignment. SET NULL on hard-delete so the history row survives but loses attribution.';
```

### 6.4 task_labels + task_label_assignments

```sql
CREATE TABLE task_labels (
    id         uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    group_id   uuid        NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    name       text        NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    color      text        NOT NULL CHECK (color ~ '^#[0-9a-fA-F]{6}$'),
    created_by uuid        NOT NULL REFERENCES users(id),
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (group_id, name)
);

CREATE INDEX task_labels_group_idx ON task_labels(group_id);

COMMENT ON TABLE task_labels IS 'Per-group task labels. UNIQUE (group_id, name) prevents duplicate label names within a group. RLS direct policy via group_id.';
COMMENT ON COLUMN task_labels.color IS 'Hex color for UI rendering, CHECK validates #RRGGBB format.';

CREATE TABLE task_label_assignments (
    task_id     uuid        NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    label_id    uuid        NOT NULL REFERENCES task_labels(id) ON DELETE CASCADE,
    assigned_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (task_id, label_id)
);

CREATE INDEX task_label_assignments_label_idx ON task_label_assignments(label_id);

COMMENT ON TABLE task_label_assignments IS 'M:N between tasks and labels. RLS JOIN policy via tasks.group_id (NOT via task_labels — both composites must match, but tasks is the anchor).';
```

### 6.5 task_comments

```sql
CREATE TABLE task_comments (
    id              uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id         uuid        NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    author_user_id  uuid        REFERENCES users(id) ON DELETE SET NULL,
    author_label    text        NOT NULL,
    body_md         text        NOT NULL CHECK (length(body_md) BETWEEN 1 AND 50000),
    created_at      timestamptz NOT NULL DEFAULT now(),
    edited_at       timestamptz,
    deleted_at      timestamptz
);

CREATE INDEX task_comments_task_created_idx
    ON task_comments(task_id, created_at DESC)
    WHERE deleted_at IS NULL;

COMMENT ON TABLE task_comments IS 'Thread of markdown comments per task. Soft delete via deleted_at. Author attribution survives user erasure via cached author_label + ON DELETE SET NULL on author_user_id. RLS JOIN policy via tasks.group_id.';
COMMENT ON COLUMN task_comments.author_user_id IS 'ON DELETE SET NULL so GDPR right to erasure does not violate FK. author_label preserves who wrote the comment post-erasure.';
COMMENT ON COLUMN task_comments.body_md IS 'Markdown comment body. CHECK (1..50000) is a DoS mitigation and also forces empty-body UI validation.';
```

### 6.6 task_subscriptions

```sql
CREATE TABLE task_subscriptions (
    task_id       uuid        NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    user_id       uuid        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    subscribed_at timestamptz NOT NULL DEFAULT now(),
    muted         boolean     NOT NULL DEFAULT false,
    PRIMARY KEY (task_id, user_id)
);

CREATE INDEX task_subscriptions_user_idx ON task_subscriptions(user_id) WHERE muted = false;

COMMENT ON TABLE task_subscriptions IS 'Who receives notifications when a task changes. Decoupled from task_assignees — a user can watch a task they are not assigned to. The actual fan-out to garraia-channels (Telegram/Discord/mobile push) is handled by the digest worker in GAR-397. RLS JOIN policy via tasks.group_id.';
```

### 6.7 task_activity

```sql
CREATE TABLE task_activity (
    id            uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id       uuid        NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    group_id      uuid        NOT NULL,
    actor_user_id uuid,
    actor_label   text        NOT NULL,
    kind          text        NOT NULL
                  CHECK (kind IN (
                      'created', 'status_changed', 'priority_changed',
                      'assigned', 'unassigned', 'labeled', 'unlabeled',
                      'commented', 'due_changed', 'archived', 'deleted', 'restored'
                  )),
    payload       jsonb       NOT NULL DEFAULT '{}'::jsonb,
    created_at    timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX task_activity_task_created_idx ON task_activity(task_id, created_at DESC);
CREATE INDEX task_activity_group_created_idx ON task_activity(group_id, created_at DESC);
CREATE INDEX task_activity_kind_idx ON task_activity(kind);

COMMENT ON TABLE task_activity IS 'UI-facing timeline of task events. Complements audit_events (which is global compliance audit). task_activity cascades with tasks — if a task is hard-deleted, its activity goes with it. Actor_user_id is plain uuid (no FK) so rows survive user erasure. RLS direct policy via group_id denormalized.';
COMMENT ON COLUMN task_activity.group_id IS 'Denormalized from tasks.group_id. Kept in sync by the caller (no trigger). Enables RLS direct policy without JOIN.';
COMMENT ON COLUMN task_activity.actor_user_id IS 'Plain uuid — NO FK. Rows survive hard deletion of the user (same pattern as audit_events.actor_user_id). actor_label caches the display name at insert time.';
COMMENT ON COLUMN task_activity.kind IS 'Event category. New event types require a migration to extend the CHECK constraint.';
COMMENT ON COLUMN task_activity.payload IS 'Event-specific context as jsonb (e.g., {old_status:"todo", new_status:"in_progress"} for a status_changed event). Must never contain secrets or unredacted PII.';
```

### 6.8 RLS — ENABLE + FORCE + 8 policies

```sql
-- Direct class (group_id denormalized)
ALTER TABLE task_lists ENABLE ROW LEVEL SECURITY;
ALTER TABLE task_lists FORCE ROW LEVEL SECURITY;
CREATE POLICY task_lists_group_isolation ON task_lists
    USING (group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid);
COMMENT ON POLICY task_lists_group_isolation ON task_lists IS
    'Class: direct. Context: app.current_group_id. Fail-closed via NULLIF. Same pattern as chats_group_isolation from migration 007.';

ALTER TABLE tasks ENABLE ROW LEVEL SECURITY;
ALTER TABLE tasks FORCE ROW LEVEL SECURITY;
CREATE POLICY tasks_group_isolation ON tasks
    USING (group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid);
COMMENT ON POLICY tasks_group_isolation ON tasks IS
    'Class: direct. Context: app.current_group_id. group_id is denormalized and enforced by compound FK (list_id, group_id) → task_lists.';

ALTER TABLE task_labels ENABLE ROW LEVEL SECURITY;
ALTER TABLE task_labels FORCE ROW LEVEL SECURITY;
CREATE POLICY task_labels_group_isolation ON task_labels
    USING (group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid);
COMMENT ON POLICY task_labels_group_isolation ON task_labels IS
    'Class: direct. Context: app.current_group_id.';

ALTER TABLE task_activity ENABLE ROW LEVEL SECURITY;
ALTER TABLE task_activity FORCE ROW LEVEL SECURITY;
CREATE POLICY task_activity_group_isolation ON task_activity
    USING (group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid);
COMMENT ON POLICY task_activity_group_isolation ON task_activity IS
    'Class: direct. Context: app.current_group_id. Denormalized group_id is set by the caller at insert time — no trigger enforces synchronization with tasks.group_id. Caller (GAR-391/GAR-393) is the single source of truth.';

-- JOIN class (via tasks or task_labels)
ALTER TABLE task_assignees ENABLE ROW LEVEL SECURITY;
ALTER TABLE task_assignees FORCE ROW LEVEL SECURITY;
CREATE POLICY task_assignees_through_tasks ON task_assignees
    USING (task_id IN (SELECT id FROM tasks));
COMMENT ON POLICY task_assignees_through_tasks ON task_assignees IS
    'Class: JOIN (implicit recursive). The subquery against tasks is itself RLS-protected by tasks_group_isolation, so the composition filters to current group transparently.';

ALTER TABLE task_label_assignments ENABLE ROW LEVEL SECURITY;
ALTER TABLE task_label_assignments FORCE ROW LEVEL SECURITY;
CREATE POLICY task_label_assignments_through_tasks ON task_label_assignments
    USING (task_id IN (SELECT id FROM tasks));
COMMENT ON POLICY task_label_assignments_through_tasks ON task_label_assignments IS
    'Class: JOIN. Anchors on tasks (not task_labels) because tasks.group_id is denormalized and the direct policy is cheaper to evaluate.';

ALTER TABLE task_comments ENABLE ROW LEVEL SECURITY;
ALTER TABLE task_comments FORCE ROW LEVEL SECURITY;
CREATE POLICY task_comments_through_tasks ON task_comments
    USING (task_id IN (SELECT id FROM tasks));
COMMENT ON POLICY task_comments_through_tasks ON task_comments IS
    'Class: JOIN. Recursive RLS composition via tasks. Soft-deleted comments (deleted_at IS NOT NULL) are still visible through this policy — filtering out soft deletes is a query-layer responsibility.';

ALTER TABLE task_subscriptions ENABLE ROW LEVEL SECURITY;
ALTER TABLE task_subscriptions FORCE ROW LEVEL SECURITY;
CREATE POLICY task_subscriptions_through_tasks ON task_subscriptions
    USING (task_id IN (SELECT id FROM tasks));
COMMENT ON POLICY task_subscriptions_through_tasks ON task_subscriptions IS
    'Class: JOIN. A user only sees subscriptions for tasks they can see — cross-group subscription leakage is structurally impossible.';

-- Grants for the 8 new tables. ALTER DEFAULT PRIVILEGES from migration 007
-- does NOT cover these because 006 runs BEFORE 007 in lexicographic order.
GRANT SELECT, INSERT, UPDATE, DELETE ON
    task_lists, tasks, task_assignees, task_labels,
    task_label_assignments, task_comments, task_subscriptions, task_activity
    TO garraia_app;
```

### Design notes

1. **Compound FK `(list_id, group_id)` + `UNIQUE (id, group_id)` on `task_lists`.** Mirrors the messages/chats pattern from GAR-388. Prevents application code from inserting a task with a `group_id` that doesn't match its `list_id.group_id`, closing the drift gap at the DB layer. Write cost: 1 extra unique index.

2. **Subtasks via self-FK, not separate `task_hierarchies` table.** Simpler. Recursive CTEs (`WITH RECURSIVE`) handle the tree query efficiently. Depth limit is not enforced at schema; app layer caps at 3 or 5 levels in the UI.

3. **Cross-list subtask prevention is deferred.** A `CHECK (parent.list_id = child.list_id)` requires a subquery, which Postgres CHECK does not support. A trigger would work but adds complexity. Instead, the GAR-391 Axum extractor validates at insert time, and an audit query in follow-up catches orphans.

4. **`task_activity` uses the same no-FK actor pattern as `audit_events`.** Rows survive user hard deletion — critical for LGPD right to erasure, where the audit trail must be preserved. `actor_label` caches the display name.

5. **`task_comments.author_user_id ON DELETE SET NULL`** vs `task_activity.actor_user_id no FK`. Comments are user-facing (readable) so the FK gives referential integrity + the SET NULL handles erasure. Activity is log-style (append-only audit) so no FK lets the row survive any deletion path.

6. **Soft delete strategy.** `task_lists.deleted_at` absent because we have `archived_at`. `tasks.deleted_at` soft-hides from lists but subtasks/comments/activity remain visible via direct queries (RLS doesn't filter). Hard DELETE cascades via ON DELETE CASCADE — recovery is not possible after hard delete.

7. **`task_subscriptions` is not identical to `task_assignees`.** A watcher may not be an assignee; an assignee may choose to mute notifications. Separating the two lets the UI show both states cleanly.

8. **RLS policy anchoring.** `task_label_assignments` could anchor on `task_labels` OR `tasks`. The plan anchors on `tasks` because `tasks.group_id` is denormalized and the policy is O(1) via index, while the `task_labels.group_id` policy would require a second subquery level.

9. **`ALTER DEFAULT PRIVILEGES` not applied here.** Migration 007 emits it, but because 007 runs AFTER 006, the default privileges only cover tables created by migrations numbered > 007. The plan explicitly GRANTs on the 8 new tables to avoid silent grant gaps.

---

## 7. Test plan

### 7.1 Extension blocks in `tests/migration_smoke.rs`

Appended after the migration 007 RLS block, before `Ok(())`:

**Metadata extension:** extend the existing `relforcerowsecurity` check array from 10 to 18 tables by adding the 8 new task tables.

**Schema assertions:**
- 8 new tables asserted in `names`
- 10 critical indexes asserted in `index_names`

**Subtask cascade test** (bypass pool — tests schema, not policy):
```rust
let list_id: Uuid = /* INSERT task_lists ... RETURNING id */;
let parent_id: Uuid = /* INSERT tasks ... (list_id, group_id, ...) */;
let child1_id: Uuid = /* INSERT tasks ... (parent_task_id = parent_id) */;
let child2_id: Uuid = /* INSERT tasks ... (parent_task_id = parent_id) */;
// Soft delete parent — subtasks SHOULD still be queryable.
sqlx::query("UPDATE tasks SET deleted_at = now() WHERE id = $1").bind(parent_id);
let surviving: i64 = sqlx::query_scalar("SELECT count(*) FROM tasks WHERE parent_task_id = $1")
    .bind(parent_id).fetch_one(pool).await?;
assert_eq!(surviving, 2, "soft delete must not cascade to subtasks");

// Hard delete parent — subtasks SHOULD cascade via ON DELETE CASCADE.
sqlx::query("DELETE FROM tasks WHERE id = $1").bind(parent_id);
let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM tasks WHERE id IN ($1, $2)")
    .bind(child1_id).bind(child2_id).fetch_one(pool).await?;
assert_eq!(remaining, 0, "hard delete must cascade to subtasks");
```

**Compound FK test** (bypass pool):
```rust
// Try to insert a task with list_id from group A but claiming group_id from group B.
let bad = sqlx::query("INSERT INTO tasks (list_id, group_id, title, created_by, created_by_label) VALUES ($1, $2, 'fail', $3, 'x')")
    .bind(own_list_id)         // belongs to `group_id`
    .bind(other_group_id)       // but we claim other_group_id
    .bind(user_id)
    .execute(pool).await
    .expect_err("compound FK should block cross-group drift");
assert_eq!(bad.as_database_error().unwrap().code().as_deref(), Some("23503"));
```

**Enum CHECK test** (bypass pool):
```rust
let bad_status = sqlx::query("INSERT INTO tasks (list_id, group_id, title, status, created_by, created_by_label) VALUES ($1, $2, 'x', 'invalid', $3, 'x')")
    .bind(own_list_id).bind(group_id).bind(user_id)
    .execute(pool).await.expect_err("invalid status must be rejected");
assert_eq!(bad_status.as_database_error().unwrap().code().as_deref(), Some("23514"));
```

**RLS positive test:**
```rust
// Setup via bypass: 1 task + 1 assignee + 1 label + 1 comment + 1 activity, all in group_id.
// Then rls_scope(group_id, user_id):
let mut tx = rls_scope(pool, Some(group_id), Some(user_id)).await?;
let task_count: i64 = sqlx::query_scalar("SELECT count(*) FROM tasks WHERE id = $1")
    .bind(test_task_id).fetch_one(&mut *tx).await?;
assert_eq!(task_count, 1, "positive read: own task visible");

let assignee_count: i64 = sqlx::query_scalar("SELECT count(*) FROM task_assignees WHERE task_id = $1")
    .bind(test_task_id).fetch_one(&mut *tx).await?;
assert_eq!(assignee_count, 1, "positive read: own assignee visible");

// Similarly for task_comments, task_subscriptions, task_activity, task_labels,
// task_label_assignments.
tx.rollback().await?;
```

**RLS cross-group test:**
```rust
// Setup via bypass: task in other_group. Then rls_scope(group_id, user_id).
let mut tx = rls_scope(pool, Some(group_id), Some(user_id)).await?;
for table in &["tasks", "task_lists", "task_assignees", "task_labels",
               "task_label_assignments", "task_comments",
               "task_subscriptions", "task_activity"] {
    // For tables with direct uuid PK, count by explicit cross-group id.
    // For M:N tables, count rows whose task_id is in other_group.
    // Each count must be 0.
}
```

**RLS JOIN composition check:**
Already covered by the cross-group test above because `task_assignees`/`task_comments`/`task_subscriptions`/`task_label_assignments` all go through the `tasks` RLS policy.

### 7.2 What we are NOT testing

- Recursive CTE subtask traversal at scale (>100 levels)
- RRULE expansion
- Notification fan-out (GAR-397)
- Activity payload schema validation
- Concurrent assignee updates
- Task ordering within a board (no `position` column yet — out of scope)

---

## 8. Rollback plan

Three levels, same as all prior migrations:

1. **Before merge:** close PR.
2. **After merge, before downstream:** `git revert`. Migration 006 file removed; sqlx no longer applies it on fresh installs. `garraia_app` role creation is idempotent so it remains — harmless.
3. **After downstream (GAR-391/393 shipping):** forward-fix only. Soft-delete columns allow recovery of mistaken deletes; hard ALTER TABLE requires a new migration.

No secrets, no destructive change, additive only.

---

## 9. Risks & mitigations

| Risco | Prob. | Impacto | Mitigação |
|---|---|---|---|
| Lexicographic ordering 006 < 007 causa falha no smoke test | Média | Alto | Plan §6.0 aborda explicitamente: `garraia_app` criado idempotente em 006, GRANTs explícitos para as 8 tabelas novas |
| Compound FK `(list_id, group_id)` causa erro de sintaxe sqlx | Baixa | Médio | Padrão já validado em `messages` (GAR-388) — copiar literal |
| Recursive RLS composition tem perf degradada em `task_assignees` / `task_comments` | Baixa | Baixo | Mesma técnica já validada em `memory_embeddings → memory_items` (GAR-408) |
| RRULE CHECK regex bloqueia strings válidas | Baixa | Baixo | Regex é permissivo (`[A-Z:;,=0-9+-]+`) — cobre FREQ=DAILY;INTERVAL=2;BYDAY=MO,WE,FR. Teste negativo valida |
| Cross-list subtask invariante viola em app bug | Média | Médio | Documentado como responsabilidade de app layer + audit query filed para GAR-391 |
| `task_activity.group_id` drift (app esquece de preencher) | Média | Médio | Documentado via COMMENT ON COLUMN; GAR-391 Axum extractor deve validar ao inserir |
| Smoke test wall time explode com +8 cenários de tasks | Baixa | Baixo | Budget folgado (30s vs 9.48s atual). Inserts bypass são rápidos |

---

## 10. Sequence of work (ordem proposta quando aprovado)

### Wave 1 — migration SQL + smoke test extension (~1.5-2h, single agent)

1. Criar `crates/garraia-workspace/migrations/006_tasks_with_rls.sql` literalmente conforme §6.
2. Estender `tests/migration_smoke.rs` com:
   - Update do array `relforcerowsecurity` check de 10 para 18 tabelas
   - Extension do `index_names` check com 10 novos índices
   - Fixture setup: create list, task, assignee, label, comment, activity (no `group_id` + no `other_group_id`)
   - Subtask cascade test (soft delete preserva, hard delete cascata)
   - Compound FK negative test
   - Enum CHECK negative test
   - RLS positive + cross-group loop
3. Atualizar `README.md` §Scope + §Running the tests.
4. `cargo clean -p garraia-workspace && cargo test -p garraia-workspace`. Iterar.
5. `cargo clippy -p garraia-workspace --all-targets -- -D warnings`. Verde.
6. Wall time ≤ 30s.

### Wave 2 — parallel review (~25min wall)

7. `@security-auditor` — foco: RLS coverage em 8 tabelas, JOIN policy composition, subtask cross-list invariance gap, `task_activity.group_id` drift risk, erasure survival pattern consistency, out-of-scope rationale.
8. `@code-reviewer` — SQL correctness, compound FK syntax, self-FK cascade semantics, enum CHECK coverage, test fixture reuse, clippy hygiene.

### Wave 3 — fixes + ROADMAP + commit (~30min)

9. Aplicar findings.
10. ROADMAP §3.8 Tier 1: marcar as tabelas como `[x]`.
11. Commit + push.
12. Linear GAR-390 → Done.

**Total: 2-3 horas.**

---

## 11. Definition of Done

- [ ] Todos os itens do §4 acceptance criteria.
- [ ] PR merged.
- [ ] Review verde de ambos agentes.
- [ ] Linear GAR-390 → Done.
- [ ] ROADMAP §3.8 Tier 1 atualizado.
- [ ] Schema set da Fase 3 **completo** — próxima sessão muda de marcha para `garraia-auth` (GAR-391) via ADR 0005 (GAR-375) ou para `files` via ADR 0004 (GAR-374).

---

## 12. Open questions (preciso da sua resposta antes de começar)

1. **`task_activity` cascata com task (CASCADE) ou sobrevive (no FK como audit_events)?** Recomendo **CASCADE** — atividade é UI-facing (timeline "quem mudou o quê, quando"), se a task some o histórico de atividade também some. audit_events continua cobrindo compliance cross-cut (erasure demonstrável). Confirma?

2. **Subtask model: `parent_task_id` self-FK ou tabela separada `task_hierarchies`?** Recomendo **self-FK** — simpler, recursive CTEs funcionam bem, depth cap é problema de UI. Confirma?

3. **`description_md` / `body_md` size limit (50k chars)?** Recomendo **50000** — maior que `messages.body` (100k) é overkill para task descriptions, menor que docs (sem limit no v1). 50k dá ~10 páginas de prose, suficiente para qualquer task. Confirma?

4. **`recurrence_rrule` como coluna livre ou enum?** Recomendo **coluna text com CHECK de charset apenas** — RRULE é RFC 5545 complexa, parser no app layer. Schema só valida que não é lixo arbitrário. Confirma?

5. **Cross-list subtask: trigger CHECK ou app layer only?** Recomendo **app layer only** — trigger adiciona overhead em insert hot path, postgres CHECK não suporta subquery, e GAR-391 pode adicionar audit query que detecta orphans periodicamente. Confirma?

6. **Task ordering within a board (`position` column)?** Recomendo **out of scope** — sem `position` por enquanto. Quando vier kanban drag-and-drop (GAR-396), migration futura adiciona via `ALTER TABLE ADD COLUMN position float` ou `fractional_indexing` strategy. Confirma?

7. **`task_lists.type` inclui apenas `list|board|calendar` ou também `gallery|timeline|gantt`?** Recomendo **apenas 3** — suficientes para MVP. Mais tipos = migration futura. Confirma?

---

## 13. Impact on GAR-391 and future APIs

### 13.1 Contract para garraia-auth (GAR-391)

O extractor Axum precisa continuar setando `app.current_group_id` + `app.current_user_id` no início de cada request. As 8 novas tabelas herdam o mesmo contrato estabelecido em GAR-408:

- **Direct policies** (`task_lists`, `tasks`, `task_labels`, `task_activity`): a request vê só rows do grupo atual — zero código adicional.
- **JOIN policies** (`task_assignees`, `task_label_assignments`, `task_comments`, `task_subscriptions`): composição recursiva via `tasks` — zero código adicional.
- **`task_activity.group_id` invariance:** garraia-auth deve setar `group_id` em todo INSERT de `task_activity`. Não há trigger. Documentado via COMMENT.
- **Subtask cross-list validation:** antes de insertar um `parent_task_id`, validar que `parent.list_id == new.list_id`. Não há CHECK. Audit query filed for future.

### 13.2 Contract para a API REST (GAR-393)

O módulo `/v1/task-lists` / `/v1/tasks` (ROADMAP §3.8) ganha:

- `POST /v1/groups/{group_id}/task-lists` — cria lista (Direct policy protege insert via group_id)
- `GET /v1/groups/{group_id}/task-lists` — lista listas (RLS filter aplica)
- `POST /v1/task-lists/{list_id}/tasks` — cria task (compound FK garante group_id coerente)
- `GET /v1/task-lists/{list_id}/tasks?status=...` — paginação via `tasks_list_status_idx`
- `PATCH /v1/tasks/{task_id}` — status/priority/assignees updates
- `POST /v1/tasks/{task_id}/comments` — adicionar comment
- `POST /v1/tasks/{task_id}:subscribe` — subscrever para notificações
- WebSocket `/v1/task-lists/{list_id}/stream` — kanban collaborative (futuro)

### 13.3 Contract para ADR 0005 / GAR-375 (Identity Provider)

Sem impacto — tasks não tem superfície de login. O hard blocker documentado em GAR-408 (login flow vs `user_identities` RLS) continua sendo o único gating factor para GAR-391 production rollout.

### 13.4 Contract para GAR-397 (digest worker)

`task_subscriptions` + `task_activity` são os dois inputs do worker de notificações:

1. Worker roda periodicamente (cron) ou reativamente (LISTEN/NOTIFY).
2. Para cada `task_activity` row criada desde o último sweep, encontra os `task_subscriptions` do task cujo `muted = false`.
3. Para cada subscriber, enfileira notification via `garraia-channels`.
4. Audit via `audit_events` (não `task_activity`, que é UI-facing).

Tudo isso é código futuro — migration 006 só garante que as tabelas existem com os índices corretos.

---

## 14. Next recommended issue (depois de GAR-390 merged)

Com GAR-390 merged, o **schema set da Fase 3 fica completo** (7 migrations, 18+8 = 26 tabelas tenant-scoped com RLS FORCE). Três caminhos viáveis:

- **GAR-375 — ADR 0005 Identity Provider** (research, ~3-4h) — destrava GAR-391, fecha o gap documentado do login flow.
- **GAR-374 — ADR 0004 Object Storage** (research, ~3-4h) — destrava GAR-387 (files migration) e GAR-394 (`garraia-storage` crate).
- **GAR-391 — `garraia-auth` crate** (3-5 dias) — precisa de GAR-375 primeiro (login flow design).

**Recomendação firme: GAR-375 (ADR 0005 Identity)** — é research puro, ~3-4h, com escopo bem delimitado pelo hard blocker que GAR-408 já identificou. Output é um ADR que responde "qual role Postgres verifica credenciais?" e "JWT interno vs OIDC adapter?". Depois disso, GAR-391 fica desbloqueado e o trabalho vira Rust real.

Alternativa: **GAR-374 (ADR 0004 Object Storage)** primeiro se você prefere continuar preenchendo o schema set (files) antes de auth. Trade-off: auth destrava mais produto, storage destrava mais infraestrutura.

---

**Aguardando sua aprovação.** Se aprovar como está, respondo as 7 open questions com os defaults recomendados e executo seguindo o §10. Se quiser cortar escopo (ex.: "deixa task_subscriptions para follow-up", "pula task_activity, só Tier 0"), me diga antes que eu toque em código.
