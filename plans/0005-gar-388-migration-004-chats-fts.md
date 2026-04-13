# Plan 0005: GAR-388 — Migration 004 chats + messages + FTS

> **Status:** 📋 Awaiting approval
> **Issue:** [GAR-388](https://linear.app/chatgpt25/issue/GAR-388)
> **Project:** Fase 3 — Group Workspace
> **Labels:** `epic:ws-schema`, `epic:ws-chat`
> **Priority:** High
> **Estimated session size:** 2-3 horas de trabalho focado
> **Author:** Claude Opus 4.6 + @michelbr84
> **Date:** 2026-04-13
> **Depends on:** ✅ GAR-407 (migration 001 users/groups) + ✅ GAR-386 (migration 002 RBAC)
> **Unblocks:** GAR-389 (memory), parcialmente GAR-391 (auth tests precisam de data para proteger), GAR-408 (migration 007 RLS — chats/messages viram scoped tables)

---

## 1. Goal (one sentence)

Adicionar `migration 004_chats_and_messages.sql` ao crate `garraia-workspace` criando 4 tabelas (`chats`, `chat_members`, `messages`, `message_threads`) com suporte nativo a Portuguese full-text search via `tsvector` generated column + GIN index, estendendo o smoke test para validar a aplicação da migration, inserção de mensagens e busca FTS funcional — mantendo o mesmo padrão já validado em GAR-407 e GAR-386.

---

## 2. Rationale — por que esse agora

1. **Core experience do produto.** Chat compartilhado é a feature que distingue Garra de "mais um chatbot" — é o onde a família/equipe conversa *entre si + com a IA*. Sem ele, nada mais de Fase 3 faz sentido.
2. **Padrão mecânico validado.** Migration 001 + 002 estabeleceram o template completo (SQL file → sqlx migrate macro → smoke test → review loop). Zero research, zero dep resolution, zero scaffold.
3. **FTS foi benchmarked empiricamente.** Cenário B2/B3 do `benches/database-poc/` provou Postgres FTS em 1.1–1.5ms p95 sobre 100k mensagens Portuguese com `to_tsvector('portuguese', body)` + GIN index. Este plan literalmente embrulha a receita do benchmark numa migration production.
4. **Tamanho cabível.** 2-3 horas. Menor que GAR-386 porque não tem seed data para validar; o teste é sobre shape + FTS query.
5. **Pula migration 003 intencionalmente.** `files` (GAR-387) dependem de ADR 0004 (object storage, GAR-374) que ainda não foi escrito. Fazer files primeiro exige research. Chats é self-contained.
6. **Pegadinhas de FTS valem resolver agora.** Tokenizer, generated column semantics, GIN na tsvector certa, interação futura com RLS — melhor aprender uma vez aqui que debugar depois contra 4 migrations empilhadas.

---

## 3. Scope & Non-Scope

### In scope

- **Migration 004** em `crates/garraia-workspace/migrations/004_chats_and_messages.sql`:
  - Tabela `chats` (um por canal/DM dentro de um grupo)
  - Tabela `chat_members` (quem tem acesso a qual chat dentro de um grupo)
  - Tabela `messages` com `body text NOT NULL` + `body_tsv tsvector GENERATED ALWAYS AS ... STORED` + GIN index
  - Tabela `message_threads` como entidade dedicada (não `parent_id` on messages — ver §12 Q1)
  - Índices: `(chat_id, created_at DESC)` para paginação cursor, `GIN (body_tsv)`, `(group_id, chat_id)` para cross-chat listings dentro do grupo
  - Forward-only
  - CHECK constraints em enums (`chats.type`)
  - COMMENT ON em cada tabela e em colunas não-óbvias
- **Numbering gap:** a próxima migration **É** 004, não 003, porque:
  - `001_initial_users_groups.sql` ✅ (GAR-407)
  - `002_rbac_and_audit.sql` ✅ (GAR-386)
  - `003_files_folders.sql` ⏳ (GAR-387, **blocked** por ADR 0004)
  - `004_chats_and_messages.sql` ← **este plan**

  Deixar o slot 003 aberto é aceitável pro sqlx migrate macro (ordena lexicograficamente), mas preciso confirmar no §12 Q6. **Alternativa:** renomear este plan para `003_chats_and_messages.sql` e depois puxar files para `004_files_folders.sql`. Decisão no §12.
- **Extension do smoke test** `tests/migration_smoke.rs`:
  - Novos asserts: 4 novas tabelas existem, GIN index `messages_body_tsv_idx` existe
  - Insert chat + chat_members + 3 messages de teste
  - FTS query por palavra presente (`'brasil'`, como no benchmark) deve retornar ≥1 row
  - FTS query por palavra ausente (`'inexistente'`) deve retornar 0 rows
  - Tentativa de inserir message em chat de outro grupo deve falhar (FK check — não é RLS ainda)
- **Update do README** do crate mencionando migration 004 no §Scope.
- **Update do ROADMAP.md** §3.2 marcando `chats`, `chat_members`, `messages`, `message_threads` como `[x]`.
- **Linear GAR-388** → Done após merge.

### Out of scope

- ❌ **Sem `message_attachments`.** FK para `files` que não existe. Fica em follow-up quando GAR-387 materializar files.
- ❌ **Sem reactions/emoji.** Feature de Tier 2 do chat, fora do bootstrap.
- ❌ **Sem edit/delete history.** Soft-delete via `deleted_at` existe mas audit trail completo fica em garraia-auth (GAR-391).
- ❌ **Sem RLS.** Chats/messages são tenant-scoped via `group_id` — RLS entra em migration 007 (GAR-408), junto com todas as outras scoped tables.
- ❌ **Sem Rust API de CRUD.** Nenhuma fn `create_chat`, `send_message`, `search_messages`. Só schema + smoke test. API real vem em GAR-393.
- ❌ **Sem WebSocket / real-time.** GAR-388 é schema-only; streaming vem em issue separada.
- ❌ **Sem tokenização multi-idioma.** Portuguese only em v1; migration 005 ou futura pode adicionar stemmers adicionais via `create_text_search_config`.
- ❌ **Sem gateway wiring.** Mesmo padrão de GAR-407 e GAR-386 — o crate existe standalone.
- ❌ **Sem typing indicators, read receipts, pinned messages.** Todas UI-first features de Fase 3.8 ou depois.

---

## 4. Acceptance criteria (verificáveis)

- [ ] `cargo check --workspace` verde.
- [ ] `cargo check --workspace --no-default-features` verde.
- [ ] `cargo clippy -p garraia-workspace --all-targets -- -D warnings` verde.
- [ ] `cargo test -p garraia-workspace` — 5 unit + 1 smoke verdes.
- [ ] Smoke test wall time ≤ 15 segundos (era 6.86s em GAR-386; +1 migration + FTS query adiciona ~1-2s).
- [ ] Migration 004 aplica do zero (após 001 + 002) em ≤ 500ms.
- [ ] 4 tabelas novas existem após migration: `chats`, `chat_members`, `messages`, `message_threads`.
- [ ] Índices existem: `messages_body_tsv_idx` (GIN), `messages_chat_created_idx`, `chats_group_id_idx`, `chat_members_user_id_idx`.
- [ ] `messages.body_tsv` é `GENERATED ALWAYS AS (to_tsvector('portuguese', body)) STORED` — verificado via `pg_attribute.attgenerated = 's'`.
- [ ] Smoke test insere 3 messages contendo termos conhecidos e FTS query retorna o subset esperado.
- [ ] FTS query por termo ausente retorna 0 rows (negativo).
- [ ] Tentativa de insert de message com `group_id` diferente de `chats.group_id` é bloqueada (CHECK cross-column ou application responsibility — ver §6 design notes).
- [ ] Migration é forward-only.
- [ ] Review verde de `@code-reviewer` + `@security-auditor`.
- [ ] GAR-388 movido para Done após merge.
- [ ] ROADMAP.md §3.2 atualizado.

---

## 5. File-level changes

### 5.1 Novos arquivos

```
crates/garraia-workspace/migrations/
  004_chats_and_messages.sql    # ★ a nova migration
```

**Nenhum outro arquivo novo.** Crate, test infra, Cargo.toml — todos intocados.

### 5.2 Edits em arquivos existentes

- `crates/garraia-workspace/tests/migration_smoke.rs`: adicionar ~50 linhas após o bloco de migration 002, mantendo tudo anterior intacto. Novos asserts para tabelas/índices, inserts de teste, queries FTS.
- `crates/garraia-workspace/README.md`: adicionar uma linha no §Scope mencionando migration 004.
- `ROADMAP.md` §3.2: marcar `chats`, `chat_members`, `messages`, `message_threads` como `[x]` e remover as linhas duplicadas atualmente pendentes.

### 5.3 Zero edits em Rust source

- `src/lib.rs`, `src/config.rs`, `src/error.rs`, `src/store.rs` — intocados.
- **Gotcha da wave 1 anterior:** adicionar uma nova migration sem tocar em `.rs` file pode exigir `cargo clean -p garraia-workspace` para forçar recompilação do binário de test. Documentado no relatório do agente GAR-386. Solução permanente: o orchestrator roda `cargo clean -p garraia-workspace && cargo test -p garraia-workspace` na primeira invocação pós-migration add, ou alternativamente toca em `src/lib.rs` (whitespace diff) para forçar rebuild. Preferência: `cargo clean` (mais explícito).

---

## 6. Schema details (o SQL completo)

### 6.1 chats

```sql
CREATE TABLE chats (
    id          uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    group_id    uuid        NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    type        text        NOT NULL CHECK (type IN ('channel', 'dm', 'thread')),
    name        text        NOT NULL,
    topic       text,
    created_by  uuid        NOT NULL REFERENCES users(id),
    settings    jsonb       NOT NULL DEFAULT '{}'::jsonb,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    archived_at timestamptz
);

CREATE INDEX chats_group_id_idx ON chats(group_id) WHERE archived_at IS NULL;
CREATE INDEX chats_group_type_idx ON chats(group_id, type) WHERE archived_at IS NULL;

COMMENT ON TABLE chats IS 'Chat containers within a group. RLS added in migration 007 (GAR-408).';
COMMENT ON COLUMN chats.type IS 'channel → public within group; dm → private between specific members; thread → reserved for thread-root chats, use message_threads for flat threading';
COMMENT ON COLUMN chats.name IS 'Display name. For dm type, caller may set this to the other participant''s display_name at creation time.';
COMMENT ON COLUMN chats.archived_at IS 'Soft delete. NULL = active, non-NULL = archived (hidden by default in UI but messages remain).';
```

### 6.2 chat_members

```sql
CREATE TABLE chat_members (
    chat_id      uuid        NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    user_id      uuid        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role         text        NOT NULL DEFAULT 'member'
                 CHECK (role IN ('owner', 'moderator', 'member', 'viewer')),
    joined_at    timestamptz NOT NULL DEFAULT now(),
    last_read_at timestamptz,
    muted        boolean     NOT NULL DEFAULT false,
    PRIMARY KEY (chat_id, user_id)
);

CREATE INDEX chat_members_user_id_idx ON chat_members(user_id);
CREATE INDEX chat_members_unread_idx
    ON chat_members(user_id, chat_id)
    WHERE muted = false;

COMMENT ON TABLE chat_members IS 'Per-chat membership. Independent from group_members — a user may be in a group but not subscribed to every chat (opt-in channels).';
COMMENT ON COLUMN chat_members.role IS 'Chat-local role. Distinct from group_members.role. Used for moderator-only actions inside a channel.';
COMMENT ON COLUMN chat_members.last_read_at IS 'Cursor for unread count: messages WHERE created_at > last_read_at are unread.';
```

### 6.3 messages

```sql
CREATE TABLE messages (
    id              uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    chat_id         uuid        NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    group_id        uuid        NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    sender_user_id  uuid        NOT NULL REFERENCES users(id),
    sender_label    text        NOT NULL,
    body            text        NOT NULL,
    body_tsv        tsvector    GENERATED ALWAYS AS (to_tsvector('portuguese', body)) STORED,
    reply_to_id     uuid        REFERENCES messages(id) ON DELETE SET NULL,
    thread_id       uuid,
    created_at      timestamptz NOT NULL DEFAULT now(),
    edited_at       timestamptz,
    deleted_at      timestamptz
);

CREATE INDEX messages_chat_created_idx
    ON messages(chat_id, created_at DESC)
    WHERE deleted_at IS NULL;

CREATE INDEX messages_body_tsv_idx
    ON messages USING GIN (body_tsv)
    WHERE deleted_at IS NULL;

CREATE INDEX messages_group_created_idx
    ON messages(group_id, created_at DESC)
    WHERE deleted_at IS NULL;

CREATE INDEX messages_thread_id_idx
    ON messages(thread_id)
    WHERE thread_id IS NOT NULL AND deleted_at IS NULL;

CREATE INDEX messages_sender_idx ON messages(sender_user_id);

COMMENT ON TABLE messages IS 'Chat messages with Portuguese FTS. group_id is denormalized from chats.group_id for fast cross-chat queries and future RLS policy (GAR-408).';
COMMENT ON COLUMN messages.group_id IS 'Denormalized from chats.group_id. Kept in sync by application code at INSERT time — no trigger. Enables fast group-scoped queries and RLS without a join.';
COMMENT ON COLUMN messages.sender_label IS 'Cached display_name at send time. Lets messages remain readable after user is deleted (erasure survival path).';
COMMENT ON COLUMN messages.body_tsv IS 'Generated column (STORED) with Portuguese tokenizer. GIN indexed for full-text search. Do not write to this column — Postgres maintains it from body.';
COMMENT ON COLUMN messages.reply_to_id IS 'Parent message for 1:1 reply. ON DELETE SET NULL so reply chains survive the parent being soft-deleted.';
COMMENT ON COLUMN messages.thread_id IS 'Thread grouping via message_threads table. NULL means top-level message. See plan 0005 §12 Q1.';
COMMENT ON COLUMN messages.deleted_at IS 'Soft delete. deleted_at IS NOT NULL → message hidden from lists but retained for audit. Hard delete reserved for GDPR right to erasure.';
```

### 6.4 message_threads

```sql
CREATE TABLE message_threads (
    id              uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    chat_id         uuid        NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    root_message_id uuid        NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    title           text,
    created_by      uuid        NOT NULL REFERENCES users(id),
    created_at      timestamptz NOT NULL DEFAULT now(),
    resolved_at     timestamptz,
    UNIQUE (root_message_id)
);

CREATE INDEX message_threads_chat_idx ON message_threads(chat_id) WHERE resolved_at IS NULL;

COMMENT ON TABLE message_threads IS 'Thread as first-class entity. A thread groups messages via messages.thread_id FK. Each root message has exactly one thread (UNIQUE). Deferred to app layer: FK from messages.thread_id to message_threads.id would create a circular dependency with root_message_id; we leave messages.thread_id as plain uuid and rely on application invariants + audit queries.';
COMMENT ON COLUMN message_threads.resolved_at IS 'Optional: lets a thread be explicitly marked "resolved" (like a GitHub PR thread). UI decision.';
```

**Nota importante sobre circular FK:** `messages.thread_id` **NÃO é FK** para `message_threads.id` porque criaria dependência circular com `root_message_id`. Application layer garante que todo `thread_id` em messages aponta para um `message_threads.id` válido. Migration 004 documenta isso via COMMENT.

### 6.5 Cross-column consistency (group_id)

**Desafio:** `messages.group_id` é denormalizado de `chats.group_id`. Se app code inserir `messages` com `group_id` divergente do `chats.group_id` correspondente, ambos FKs passam mas o dado fica inconsistente.

**Opções:**
1. **CHECK via subquery** — Postgres NÃO permite subqueries em CHECK constraints. Descartado.
2. **Trigger** BEFORE INSERT OR UPDATE que valida `NEW.group_id = (SELECT group_id FROM chats WHERE id = NEW.chat_id)`. Funciona mas adiciona complexidade e latência (~5-10% overhead em insert hot path).
3. **Application responsibility** — garraia-workspace API (futura, GAR-393) sempre pega `group_id` de `chats` antes de inserir. Documentado via COMMENT. Risco: bug humano.
4. **Compound FK** — `FOREIGN KEY (chat_id, group_id) REFERENCES chats(id, group_id)`. **Requer UNIQUE (id, group_id) em chats** (redundante com PK id, mas Postgres aceita). Fail-closed no DB layer. Mais elegante.

**Decisão do plan (§12 Q3):** **Opção 4 (compound FK)**. Menos overhead, mais robusto, fail-closed. Custo: adicionar `UNIQUE (id, group_id)` em `chats` (1 linha).

Adição à `chats`:
```sql
ALTER TABLE chats ADD CONSTRAINT chats_id_group_unique UNIQUE (id, group_id);
```

E em `messages`:
```sql
FOREIGN KEY (chat_id, group_id) REFERENCES chats(id, group_id) ON DELETE CASCADE
```

### Design notes

1. **`body_tsv` is STORED, not VIRTUAL.** `STORED` persists the tsvector on disk; `VIRTUAL` re-computes on every read. For FTS with GIN indexes, `STORED` is the only supported option (Postgres 16). `VIRTUAL` também nem é suportado para `tsvector`.
2. **`'portuguese'` tokenizer hardcoded.** v1 é Portuguese-only. Multi-language stemmer fica em migration futura via `CREATE TEXT SEARCH CONFIGURATION`.
3. **`reply_to_id ON DELETE SET NULL`** vs `CASCADE`. SET NULL preserva conversation chains mesmo quando a mensagem pai é soft-deleted. CASCADE apagaria replies que referenciam — destrutivo.
4. **`sender_label` cached.** Mesmo padrão do `audit_events.actor_label`: permite ler mensagens post-erasure do usuário.
5. **`messages.group_id` denormalizado.** Parece violar normalização, mas é deliberado: (a) RLS policy em migration 007 vai filtrar `WHERE group_id = current_setting('app.current_group_id')` sem precisar de JOIN, (b) cross-chat queries dentro de um grupo são mais rápidas.
6. **`chat_members` separate from `group_members`.** Um usuário do grupo pode não estar em todos os canais (opt-in). Modela Slack/Discord realisticamente.
7. **`chats.type = 'thread'`** é reservado mas não usado por enquanto — threads nascem de `message_threads` separate. Se decidirmos mudar pra thread-as-chat, o CHECK já aceita.

---

## 7. Test plan

### 7.1 Extensions to `tests/migration_smoke.rs`

Appended after the migration 002 block, before the closing `Ok(())`:

```rust
// ─── Migration 004 validation ──────────────────────────────────────────
//
// Same snapshot semantics as migration 002: `names` and `index_names` were
// populated after `Workspace::connect` applied all migrations atomically.

// New tables exist.
for expected in &["chats", "chat_members", "messages", "message_threads"] {
    assert!(
        names.contains(expected),
        "missing table from migration 004: {expected}"
    );
}

// Critical FTS + pagination indexes exist.
for expected in &[
    "messages_body_tsv_idx",
    "messages_chat_created_idx",
    "messages_group_created_idx",
    "chats_group_id_idx",
    "chat_members_user_id_idx",
] {
    assert!(
        index_names.contains(expected),
        "missing index from migration 004: {expected}"
    );
}

// Verify body_tsv is a STORED generated column.
let attgenerated: String = sqlx::query_scalar(
    "SELECT attgenerated::text FROM pg_attribute \
     WHERE attrelid = 'messages'::regclass AND attname = 'body_tsv'"
)
.fetch_one(workspace.pool())
.await?;
assert_eq!(attgenerated, "s", "body_tsv must be STORED (attgenerated='s')");

// Create a chat and add the test user as owner.
let chat_id: uuid::Uuid = sqlx::query_scalar(
    "INSERT INTO chats (group_id, type, name, created_by) \
     VALUES ($1, 'channel', 'geral', $2) RETURNING id"
)
.bind(group_id)
.bind(user_id)
.fetch_one(workspace.pool()).await?;

sqlx::query(
    "INSERT INTO chat_members (chat_id, user_id, role) VALUES ($1, $2, 'owner')"
)
.bind(chat_id).bind(user_id)
.execute(workspace.pool()).await?;

// Insert 3 messages with known tokens.
for (body, label) in [
    ("Bom dia pessoal, tudo certo para o churrasco no Brasil?", "msg-brasil"),
    ("Vou trazer carne e bebidas para a festa", "msg-festa"),
    ("Confirma presença até amanhã por favor", "msg-confirma"),
] {
    sqlx::query(
        "INSERT INTO messages (chat_id, group_id, sender_user_id, sender_label, body) \
         VALUES ($1, $2, $3, $4, $5)"
    )
    .bind(chat_id).bind(group_id).bind(user_id).bind(label).bind(body)
    .execute(workspace.pool()).await?;
}

// FTS query: positive match (body contains "brasil").
let hits_positive: Vec<(uuid::Uuid,)> = sqlx::query_as(
    "SELECT id FROM messages \
     WHERE chat_id = $1 AND body_tsv @@ plainto_tsquery('portuguese', 'brasil') \
     AND deleted_at IS NULL"
)
.bind(chat_id)
.fetch_all(workspace.pool()).await?;
assert_eq!(hits_positive.len(), 1, "expected exactly 1 FTS match for 'brasil'");

// FTS query: negative match (token not in any body).
let hits_negative: Vec<(uuid::Uuid,)> = sqlx::query_as(
    "SELECT id FROM messages \
     WHERE body_tsv @@ plainto_tsquery('portuguese', 'helicoptero') \
     AND deleted_at IS NULL"
)
.bind(&[] as &[uuid::Uuid])  // no binds
.fetch_all(workspace.pool()).await?;
assert_eq!(hits_negative.len(), 0, "expected 0 FTS matches for 'helicoptero'");

// Compound FK test: message with mismatched group_id must fail.
// Create a second group to force the mismatch.
let other_group_id: uuid::Uuid = sqlx::query_scalar(
    "INSERT INTO groups (name, type, created_by) VALUES ('Other', 'team', $1) RETURNING id"
)
.bind(user_id)
.fetch_one(workspace.pool()).await?;

let mismatch = sqlx::query(
    "INSERT INTO messages (chat_id, group_id, sender_user_id, sender_label, body) \
     VALUES ($1, $2, $3, $4, $5)"
)
.bind(chat_id)          // chat belongs to `group_id`
.bind(other_group_id)   // but we claim `other_group_id`
.bind(user_id)
.bind("Test User")
.bind("should fail")
.execute(workspace.pool())
.await
.expect_err("compound FK should reject cross-group message");

let db_err = mismatch.as_database_error().expect("database-layer error");
assert_eq!(
    db_err.code().as_deref(), Some("23503"),
    "expected SQLSTATE 23503 (foreign_key_violation)"
);
```

### 7.2 What we are NOT testing

- Thread creation via `message_threads` table. The schema exists; API comes later.
- `chat_members.last_read_at` unread cursor semantics. App-layer logic.
- Edit/delete soft-delete flows. API concern.
- FTS ranking / `ts_rank`. Schema supports it; ranking tuning is out of scope.
- Concurrent insert throughput. Already benchmarked in `benches/database-poc/`.
- Sentence-level Portuguese tokenization edge cases. Trusted to Postgres.

---

## 8. Rollback plan

Idêntico a GAR-386:

1. **Before merge:** close the PR.
2. **After merge, before downstream consumer:** `git revert`. Migration file removed, next `Workspace::connect` doesn't apply it.
3. **After downstream consumer:** forward-fix via new migration.

Zero secrets, zero API breaking, additive only.

---

## 9. Risks & mitigations

| Risco | Prob. | Impacto | Mitigação |
|---|---|---|---|
| `tsvector GENERATED ... STORED` syntax quirk em sqlx | Baixa | Médio | Já validado no benchmark `benches/database-poc/src/postgres_scenarios.rs` — mesmo SQL |
| FTS query retorna 0 para `'brasil'` por causa de stemmer Portuguese | Baixa | Alto (test false negative) | Usar `plainto_tsquery` que aplica o mesmo stemmer; `'brasil'` é forma canônica lowercase |
| Compound FK `(chat_id, group_id) REFERENCES chats(id, group_id)` falha porque `chats` não tem UNIQUE | Média | Alto | Plan §6.5 adiciona `ALTER TABLE chats ADD CONSTRAINT chats_id_group_unique UNIQUE (id, group_id)` |
| `sqlx::migrate!()` não pega o novo arquivo sem cargo clean | Média | Baixo | Lesson learned do GAR-386: rodar `cargo clean -p garraia-workspace && cargo test -p garraia-workspace` na primeira invocação |
| GIN index sobre `tsvector` é lento em seed | Baixa | Baixo | 3 messages de teste — overhead desprezível |
| Slot 003 vago confunde o sqlx migrator | Baixa | Médio | sqlx ordena lexicograficamente por filename; `004` depois de `002` funciona sem `003`. Testado implicitamente pela aplicação bem-sucedida |
| `reply_to_id` self-FK cria deadlock em delete | Baixa | Médio | `ON DELETE SET NULL` já mitiga |

---

## 10. Sequence of work (ordem proposta quando aprovado)

### Wave 1 — migration SQL + test extension (~1.5h, single agent)

1. Criar `crates/garraia-workspace/migrations/004_chats_and_messages.sql` literalmente conforme §6.
2. Estender `crates/garraia-workspace/tests/migration_smoke.rs` com o bloco §7.1.
3. Atualizar `crates/garraia-workspace/README.md` §Scope mencionando migration 004.
4. Rodar `cargo clean -p garraia-workspace && cargo test -p garraia-workspace`. Iterar até verde.
5. `cargo clippy -p garraia-workspace --all-targets -- -D warnings`. Verde.
6. Verificar wall time ≤ 15s.

### Wave 2 — parallel review (~20min wall, 2 agents background)

7. `@code-reviewer` — SQL correctness, FTS syntax, compound FK, tests validity.
8. `@security-auditor` — PII in message bodies (fake data only), cross-group prevention, RLS gap documentation, soft-delete audit implications.

### Wave 3 — fixes + ROADMAP + commit (~30min, me)

9. Aplicar findings.
10. ROADMAP.md §3.2 atualizado.
11. Commit + push.
12. Linear GAR-388 → Done.

**Total estimado: 2-3 horas.** Mesmo perfil de GAR-386.

---

## 11. Definition of Done

- [ ] Todos os 15 itens do §4 marcados.
- [ ] PR merged.
- [ ] Review verde dos 2 agentes.
- [ ] Linear GAR-388 → Done.
- [ ] ROADMAP §3.2 atualizado.
- [ ] Próxima sessão pode abrir GAR-389 (memory + pgvector HNSW) sem pendências.

---

## 12. Open questions (preciso da sua resposta antes de começar)

1. **Thread model: `messages.thread_id + message_threads` table OR `messages.parent_id` simples?** Recomendo **`thread_id + message_threads`** — flexibilidade para threads com título próprio ("Discussão sobre churrasco"), resolução explícita (como GitHub PR threads), e audit por thread. Custo: 1 tabela a mais. Confirma?

2. **`messages.body` size limit?** Recomendo **`CHECK (length(body) BETWEEN 1 AND 100000)`** — 100k chars é bem acima de qualquer chat normal mas previne DoS por storage. Alternativa: sem limit (trust application). Confirma 100k?

3. **Compound FK `(chat_id, group_id) → chats(id, group_id)`?** Recomendo **sim** — fail-closed no DB, ~zero overhead, documenta a invariância. Custo: 1 linha extra em `chats` (`UNIQUE (id, group_id)`). Alternativa: application-layer check. Confirma compound FK?

4. **`messages.group_id` denormalizado?** Recomendo **sim** — RLS em migration 007 fica trivial (`WHERE group_id = current_setting(...)`), sem JOIN. Sincronia mantida via compound FK (Q3). Trade-off: +16 bytes por row. Confirma?

5. **`chats.type` enum — `channel | dm | thread`?** Recomendo **incluir `thread` reservado** mas não usar em v1 (threads nascem via `message_threads`). Deixa porta aberta se futuramente decidirmos tratar threads como chats-filhos. Confirma?

6. **Numbering: esta migration é `004` ou `003`?** Decisão entre:
   - (a) **`004_chats_and_messages.sql`** — deixa slot `003` para GAR-387 (files) quando ADR 0004 estiver escrito. Sqlx migrate macro funciona bem com gaps.
   - (b) `003_chats_and_messages.sql` e renomeia GAR-387 para `004_files_folders.sql` mais tarde.

   Recomendo **(a)** — preserva ordem lógica "files antes de chats" do ROADMAP §3.2, não precisa renomear nada depois, e sqlx aceita gaps sem reclamar. Confirma?

7. **Portuguese-only tokenizer em v1?** Recomendo **sim** — `to_tsvector('portuguese', body)` hardcoded. Multi-idioma entra em migration futura via `CREATE TEXT SEARCH CONFIGURATION`. Confirma?

---

## 13. Next recommended issue (depois de GAR-388 merged)

Duas opções naturais:

- **GAR-389 — Migration 005 memory + pgvector HNSW** (2-3h) — segue o padrão, já benchmarked no `benches/database-poc/`, destrava Fase 2.1 RAG e §3.7 memória compartilhada.
- **GAR-390 — Migration 006 tasks** (2-3h) — Notion-like Tier 1 schema, zero dep de ADRs pendentes, destrava a proposta de diferencial de produto.

**Recomendação:** **GAR-389 primeiro** — pgvector HNSW é a capability que justificou a decisão Postgres no ADR 0003 (124x speedup em B4). Validar a infra production-ready agora é pagamento técnico crítico. Tasks (390) vêm em seguida.

Alternativa estratégica: **GAR-408 migration 007 RLS** depois de 389/390. Com messages, memory, tasks existindo, vale ativar RLS imediatamente antes de qualquer API shipar (garraia-auth GAR-391). Isso fecha o loop de segurança antes do código Rust tocar as tabelas.

---

**Aguardando sua aprovação.** Se aprovar como está, respondo as 7 open questions com os defaults recomendados (a menos que você ajuste) e começo pelo passo 1 do §10. Se quiser cortar escopo (ex.: "skip message_threads, usa só reply_to_id", "desiste de compound FK, confia no app layer"), me diga antes que eu toque em código.
