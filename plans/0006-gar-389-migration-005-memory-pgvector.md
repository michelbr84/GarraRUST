# Plan 0006: GAR-389 — Migration 005 memory_items + memory_embeddings (pgvector HNSW)

> **Status:** 📋 Awaiting approval
> **Issue:** [GAR-389](https://linear.app/chatgpt25/issue/GAR-389)
> **Project:** Fase 3 — Group Workspace
> **Labels:** `epic:ws-schema`, `epic:ws-memory`
> **Priority:** High
> **Estimated session size:** 2-3 horas de trabalho focado
> **Author:** Claude Opus 4.6 + @michelbr84
> **Date:** 2026-04-13
> **Depends on:** ✅ GAR-407 (groups/users) + ✅ GAR-386 (audit_events) + ✅ GAR-388 (chats for scope=chat)
> **Unblocks:** GAR-391 (garraia-auth pode consultar memory por scope), GAR-408 (migration 007 RLS — memory_items é target scoped-tenant), Fase 2.1 RAG pipeline

---

## 1. Goal (one sentence)

Adicionar `migration 005_memory_items_and_embeddings.sql` ao crate `garraia-workspace` criando as 2 tabelas de memória compartilhada da IA (`memory_items` com scope tri-nível `user|group|chat` e `memory_embeddings` com coluna `vector(768)` + índice HNSW cosseno), estendendo o smoke test para validar a extensão `vector`, a aplicação da migration, inserção de um embedding sintético e uma consulta ANN top-k — fechando o loop técnico do ADR 0003 cujo benchmark provou 124x speedup de HNSW sobre plain scan.

---

## 2. Rationale — por que esse agora

1. **Fecha a dívida técnica do ADR 0003.** O benchmark B4 em `benches/database-poc/` provou pgvector HNSW em 5.53ms p95 contra SQLite 685ms — o "porquê" do Postgres. Até agora, nenhuma tabela de produção usa pgvector. Esta migration materializa a capability cujo benefício sustentou a decisão de backend.
2. **Diferencial de produto.** Memory IA compartilhada é o pilar §3.7 do Group Workspace. "GarraIA lembra o que minha família discutiu" é a feature que distingue de chatbots genéricos. Sem `memory_items`, o agente não tem contexto cross-session.
3. **Scope tri-nível é único.** `scope_type IN ('user', 'group', 'chat')` + `scope_id` implementa a regra "Chat > Group > User" do ROADMAP §3.3 no nível de schema. Crítico para LGPD: memória pessoal **não** pode vazar para o grupo.
4. **Padrão bench-validado.** Schema, HNSW index, `pgvector::Vector` sqlx binding — tudo já validado em `benches/database-poc/src/postgres_scenarios.rs`. Zero research.
5. **Tamanho cabível.** 2-3h. Menor que GAR-388 (2 tabelas em vez de 4, sem FTS tokenizer novo).
6. **Próximo marco de segurança.** Depois de GAR-389, TODAS as tabelas tenant-scoped existem (chats/messages/memory). GAR-408 (migration 007 RLS) pode aplicar `FORCE ROW LEVEL SECURITY` em todas de uma vez, fechando o loop de compliance.

---

## 3. Scope & Non-Scope

### In scope

- **Migration 005** em `crates/garraia-workspace/migrations/005_memory_items_and_embeddings.sql`:
  - `CREATE EXTENSION IF NOT EXISTS vector;` — idempotente
  - Tabela `memory_items` com scope tri-nível + sensitivity + TTL
  - Tabela `memory_embeddings` com `vector(768)` + HNSW cosine index
  - Forward-only, CHECK constraints, COMMENT ON tudo não-óbvio
  - Consistência cross-column: `(scope_type, scope_id)` deve ser coerente com scopeing rules (documentação app-layer por enquanto, validação via trigger é out-of-scope)
- **Extension do smoke test** `tests/migration_smoke.rs`:
  - Assert extension `vector` está instalada (`pg_extension.extname`)
  - Assert 2 novas tabelas existem + HNSW index
  - Insert 3 memory_items (1 por scope) + 3 embeddings sintéticos normalizados
  - ANN query `ORDER BY embedding <=> $1 LIMIT 5` retorna os inserts
  - Validate que um embedding com dimensão errada (ex: vector(512)) é rejeitado
  - TTL check (se `ttl_expires_at IS NOT NULL`, validar CHECK de que é future)
- **Update do README** do crate mencionando migration 005.
- **Update do ROADMAP.md** §3.2 marcando `memory_items` e `memory_embeddings` como `[x]`.
- **Linear GAR-389** → Done após merge.

### Out of scope

- ❌ **Sem embedding provider integration.** `garraia-embeddings` crate (Fase 2.1, GAR-372) é quem gera os vectors. Este plan só cria o schema que recebe os bytes — nenhum dep em embedding model.
- ❌ **Sem RAG pipeline.** Retrieval logic (top-k + re-rank + prompt injection) é `garraia-agents` tool, não este plan.
- ❌ **Sem RLS.** Memory é a maior preocupação LGPD (memória pessoal vs grupo) mas RLS centralizado em migration 007 (GAR-408). O plan documenta o gap explicitamente.
- ❌ **Sem Rust API de CRUD.** Nenhuma fn `save_memory`, `recall_memory`. API vem em GAR-391 ou Fase 2.1.
- ❌ **Sem trigger validando `scope_id` vs `scope_type`.** Ex: `scope_type='group'` deveria ter `scope_id` apontando para `groups.id`. Postgres não permite FK condicional por valor de coluna — trigger seria necessário mas adiciona overhead + complexidade. Application responsibility, documentado via COMMENT.
- ❌ **Sem dimensões múltiplas.** `vector(768)` hardcoded — matches `mxbai-embed-large-v1` planeado para Fase 2.1. Multi-dim (ex: vector(1536) para OpenAI) fica em migration futura.
- ❌ **Sem sparse vectors ou hybrid search.** Scope focado em dense embeddings ANN.
- ❌ **Sem gateway wiring, sem garraia-auth dependency.** Crate standalone.

---

## 4. Acceptance criteria (verificáveis)

- [ ] `cargo check --workspace` verde.
- [ ] `cargo check --workspace --no-default-features` verde.
- [ ] `cargo clippy -p garraia-workspace --all-targets -- -D warnings` verde.
- [ ] `cargo test -p garraia-workspace` — 5 unit + 1 smoke verdes.
- [ ] Smoke test wall time ≤ 20 segundos (era 7.09s; HNSW index build + 3 embedding inserts adicionam ~2-5s).
- [ ] Migration 005 aplica do zero sem erros em ≤ 1s.
- [ ] `pg_extension.extname = 'vector'` existe após a migration.
- [ ] 2 tabelas novas existem: `memory_items`, `memory_embeddings`.
- [ ] HNSW index existe: `memory_embeddings_embedding_hnsw_idx`.
- [ ] Insert de 3 memory_items (scope=user, group, chat) + 3 embeddings retorna IDs válidos.
- [ ] ANN query `ORDER BY embedding <=> $1 LIMIT 5` retorna ≥ 1 row com o embedding sintético inserido.
- [ ] Insert de embedding com dimensão errada (ex: `vector(512)` quando coluna é `vector(768)`) falha com erro de dimensão.
- [ ] CHECK constraint `scope_type IN ('user','group','chat')` bloqueia valor inválido (teste negativo).
- [ ] Migration é forward-only.
- [ ] Review verde de `@code-reviewer` + `@security-auditor`.
- [ ] GAR-389 movido para Done após merge.
- [ ] ROADMAP.md §3.2 marcando as 2 tabelas.

---

## 5. File-level changes

### 5.1 Novo arquivo

```
crates/garraia-workspace/migrations/
  005_memory_items_and_embeddings.sql    # ★ a nova migration
```

### 5.2 Edits em arquivos existentes

- `crates/garraia-workspace/tests/migration_smoke.rs` — append ~60 linhas após o bloco de migration 004, antes do `Ok(())` final.
- `crates/garraia-workspace/README.md` — §Scope adiciona mention à migration 005; §Running the tests atualizado.
- `ROADMAP.md` §3.2: marcar `memory_items` e `memory_embeddings` como `[x]`.
- **Cargo.toml do crate:** ADICIONAR `pgvector = { version = "0.4", features = ["sqlx"] }` como dep. **Este é o único crate change real deste plan** — precisa para o smoke test fazer binding de `Vec<f32>` → `vector(768)`.

### 5.3 Zero edits em Rust source

- `src/lib.rs`, `src/config.rs`, `src/error.rs`, `src/store.rs` — intocados.
- Mesma lesson learned do GAR-386/388: `cargo clean -p garraia-workspace` antes do primeiro test porque `sqlx::migrate!` é compile-time macro.

---

## 6. Schema details (o SQL completo)

### 6.1 Extension

```sql
-- Must be idempotent. migration 001 already CREATEd pgcrypto and citext;
-- vector is a separate extension, provided by pgvector/pgvector:pg16 image.
CREATE EXTENSION IF NOT EXISTS vector;
```

### 6.2 memory_items

```sql
CREATE TABLE memory_items (
    id               uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    scope_type       text        NOT NULL
                     CHECK (scope_type IN ('user', 'group', 'chat')),
    scope_id         uuid        NOT NULL,
    group_id         uuid        REFERENCES groups(id) ON DELETE CASCADE,
    created_by       uuid        NOT NULL REFERENCES users(id),
    created_by_label text        NOT NULL,
    kind             text        NOT NULL
                     CHECK (kind IN ('fact', 'preference', 'note', 'reminder', 'rule', 'profile')),
    content          text        NOT NULL CHECK (length(content) BETWEEN 1 AND 10000),
    sensitivity      text        NOT NULL DEFAULT 'private'
                     CHECK (sensitivity IN ('public', 'group', 'private', 'secret')),
    source_chat_id   uuid        REFERENCES chats(id) ON DELETE SET NULL,
    source_message_id uuid       REFERENCES messages(id) ON DELETE SET NULL,
    ttl_expires_at   timestamptz,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now(),
    deleted_at       timestamptz,
    CHECK (ttl_expires_at IS NULL OR ttl_expires_at > created_at)
);

CREATE INDEX memory_items_scope_idx
    ON memory_items(scope_type, scope_id)
    WHERE deleted_at IS NULL;

CREATE INDEX memory_items_group_idx
    ON memory_items(group_id, created_at DESC)
    WHERE deleted_at IS NULL AND group_id IS NOT NULL;

CREATE INDEX memory_items_ttl_idx
    ON memory_items(ttl_expires_at)
    WHERE ttl_expires_at IS NOT NULL AND deleted_at IS NULL;

CREATE INDEX memory_items_created_by_idx ON memory_items(created_by);

COMMENT ON TABLE memory_items IS 'AI memory with three-level scope (user/group/chat). RLS NOT enabled in this migration — migration 007 (GAR-408) adds the policy filtering by group_id AND by app.current_user_id for user-scope rows. Personal memories (scope_type=user) MUST NOT leak into group retrieval per LGPD art. 46 segregation requirement.';
COMMENT ON COLUMN memory_items.scope_type IS 'user → personal memory (only the creator sees it); group → shared within the group; chat → bound to a specific chat/channel. Resolution rule (see ROADMAP §3.3): Chat > Group > User when multiple scopes intersect.';
COMMENT ON COLUMN memory_items.scope_id IS 'Points to users.id, groups.id, or chats.id depending on scope_type. No FK — Postgres does not support conditional FKs on scalar columns. Application layer (garraia-auth, GAR-391) enforces that scope_id is a valid row of the table implied by scope_type.';
COMMENT ON COLUMN memory_items.group_id IS 'Denormalized group context for RLS in migration 007. For scope_type=group, equals scope_id. For scope_type=chat, equals chats.group_id. For scope_type=user, NULL (personal memories are not group-scoped).';
COMMENT ON COLUMN memory_items.created_by_label IS 'Cached display_name at save time. Lets memory remain attributable after user deletion (erasure survival path, same pattern as audit_events.actor_label).';
COMMENT ON COLUMN memory_items.kind IS 'Semantic category — helps the agent filter retrieval (e.g., fetch only profile facts when introducing the assistant).';
COMMENT ON COLUMN memory_items.content IS 'Plain text. CHECK (length 1..10000) is a DoS mitigation. Any observability span that captures this column MUST route through garraia-telemetry redaction — memories often contain PII (names, schedules, preferences, secrets).';
COMMENT ON COLUMN memory_items.sensitivity IS 'public → safe to include in any retrieval; group → only within the group; private → only with the creator present; secret → never included in LLM prompts automatically (manual retrieval only). garraia-auth must enforce.';
COMMENT ON COLUMN memory_items.ttl_expires_at IS 'Optional expiration. A scheduled worker (Fase 2.1 or later) hard-deletes rows where ttl_expires_at < now() after a grace period. CHECK (ttl_expires_at > created_at) prevents accidentally-expired-on-insert rows.';
COMMENT ON COLUMN memory_items.source_chat_id IS 'Optional provenance: which chat this memory was extracted from. ON DELETE SET NULL so memory survives chat deletion for audit.';
COMMENT ON COLUMN memory_items.source_message_id IS 'Optional provenance: which specific message triggered this memory.';
COMMENT ON COLUMN memory_items.updated_at IS 'Caller responsibility — no trigger. Same pattern as users.updated_at and groups.updated_at.';
```

### 6.3 memory_embeddings

Separated from `memory_items` so we can later support multiple embedding models (one row per model per item) without rewriting the parent table.

```sql
CREATE TABLE memory_embeddings (
    memory_item_id uuid        NOT NULL REFERENCES memory_items(id) ON DELETE CASCADE,
    model          text        NOT NULL,
    embedding      vector(768) NOT NULL,
    created_at     timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (memory_item_id, model)
);

-- HNSW index for approximate nearest neighbor search, cosine distance.
-- Same config as benches/database-poc/ which benchmarked 5.53ms p95 on 100k vectors.
CREATE INDEX memory_embeddings_embedding_hnsw_idx
    ON memory_embeddings USING hnsw (embedding vector_cosine_ops);

CREATE INDEX memory_embeddings_model_idx ON memory_embeddings(model);

COMMENT ON TABLE memory_embeddings IS 'Dense embedding vectors for memory_items. Separated from the parent to support multiple models per item (PK includes model). HNSW cosine index gives ~5ms p95 top-k for 100k vectors per benchmark in benches/database-poc/ (GAR-373).';
COMMENT ON COLUMN memory_embeddings.model IS 'Embedding model identifier, e.g. "mxbai-embed-large-v1". Allows side-by-side comparison when migrating between models without re-embedding everything at once.';
COMMENT ON COLUMN memory_embeddings.embedding IS 'vector(768) dimension matches mxbai-embed-large-v1 (Fase 2.1, GAR-372). Different-dim models require a new column or a new table — this migration does not support multi-dim in one column.';
```

### Design notes

1. **`vector(768)` hardcoded.** Matches `mxbai-embed-large-v1` planned for Fase 2.1. Trade-off: OpenAI (1536), Cohere (1024), Voyage (1024) não cabem aqui. Justificativa: local-first é o north star; mxbai é o default. Multi-model entries via `model` column + future `memory_embeddings_1536` table if needed.

2. **`scope_type` enum + `scope_id` uuid without FK.** Postgres doesn't support conditional FKs. Application layer must enforce `scope_type='user' → scope_id references users.id`. Documented in COMMENT. Future option: trigger validation, deferred to garraia-auth (GAR-391) audit queries.

3. **`group_id` denormalized** — same pattern as `messages.group_id`. Enables migration 007 RLS policy `WHERE group_id = current_setting('app.current_group_id')` without a JOIN. For `scope_type='user'` rows, `group_id IS NULL` — the RLS policy must handle that case (user-scope rows visible only via `app.current_user_id`, not `app.current_group_id`).

4. **`source_chat_id` / `source_message_id` ON DELETE SET NULL.** Memory often outlives the chat that spawned it. Preserving the memory even when the source chat is archived is the right call.

5. **`sensitivity` enum with 4 levels.** `secret` is the important one: `garraia-auth` must never auto-inject secret memories into LLM prompts — only explicit user request. Documented via COMMENT.

6. **HNSW index with defaults.** pgvector 0.7+ uses `m=16, ef_construction=64` defaults. Validated in benchmark. Future tuning is an op knob, not a schema decision.

7. **Separate `memory_embeddings` table.** Normalized. If a single item has embeddings from 2 models (during migration), both coexist. PK `(memory_item_id, model)` prevents duplicates per model.

---

## 7. Test plan

### 7.1 Extensions to `tests/migration_smoke.rs`

Appended after the migration 004 compound-FK test, before `Ok(())`:

```rust
// ─── Migration 005 validation ──────────────────────────────────────────

// Extension `vector` is installed.
let has_vector: bool = sqlx::query_scalar(
    "SELECT EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'vector')"
)
.fetch_one(workspace.pool()).await?;
assert!(has_vector, "pgvector extension must be installed by migration 005");

// New tables exist.
for expected in &["memory_items", "memory_embeddings"] {
    assert!(
        names.contains(expected),
        "missing table from migration 005: {expected}"
    );
}

// HNSW index exists.
assert!(
    index_names.contains(&"memory_embeddings_embedding_hnsw_idx"),
    "missing HNSW index from migration 005"
);

// Insert 3 memory_items (1 per scope) + embeddings.
// Use unit-normalized random vectors for deterministic cosine behavior.
let memory_fact_id: uuid::Uuid = sqlx::query_scalar(
    "INSERT INTO memory_items (scope_type, scope_id, group_id, created_by, \
     created_by_label, kind, content) \
     VALUES ('group', $1, $2, $3, 'Test User', 'fact', 'A família gosta de churrasco aos domingos') \
     RETURNING id"
)
.bind(group_id)  // scope_id points to groups.id
.bind(group_id)
.bind(user_id)
.fetch_one(workspace.pool()).await?;

let memory_pref_id: uuid::Uuid = sqlx::query_scalar(
    "INSERT INTO memory_items (scope_type, scope_id, group_id, created_by, \
     created_by_label, kind, content) \
     VALUES ('user', $1, NULL, $2, 'Test User', 'preference', 'Prefere respostas curtas') \
     RETURNING id"
)
.bind(user_id)  // scope_id points to users.id
.bind(user_id)
.fetch_one(workspace.pool()).await?;

let memory_note_id: uuid::Uuid = sqlx::query_scalar(
    "INSERT INTO memory_items (scope_type, scope_id, group_id, created_by, \
     created_by_label, kind, content) \
     VALUES ('chat', $1, $2, $3, 'Test User', 'note', 'Combinamos churrasco dia 20') \
     RETURNING id"
)
.bind(chat_id)  // from migration 004 block
.bind(group_id)
.bind(user_id)
.fetch_one(workspace.pool()).await?;

// Generate 3 deterministic unit-normalized 768-d vectors.
use pgvector::Vector;
fn unit_vector(seed: u64) -> Vector {
    use rand::SeedableRng;
    use rand::Rng;
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let mut v: Vec<f32> = (0..768).map(|_| rng.gen_range(-1.0..1.0)).collect();
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 { for x in v.iter_mut() { *x /= norm; } }
    Vector::from(v)
}

for (item_id, seed) in [(memory_fact_id, 1), (memory_pref_id, 2), (memory_note_id, 3)] {
    sqlx::query(
        "INSERT INTO memory_embeddings (memory_item_id, model, embedding) \
         VALUES ($1, $2, $3)"
    )
    .bind(item_id)
    .bind("mxbai-embed-large-v1")
    .bind(unit_vector(seed))
    .execute(workspace.pool()).await?;
}

// ANN query: query with seed=1 (same as first insert) should hit it first.
let query_vec = unit_vector(1);
let top_k: Vec<(uuid::Uuid,)> = sqlx::query_as(
    "SELECT memory_item_id FROM memory_embeddings \
     ORDER BY embedding <=> $1 LIMIT 3"
)
.bind(query_vec)
.fetch_all(workspace.pool()).await?;
assert_eq!(top_k.len(), 3, "expected 3 ANN results");
assert_eq!(top_k[0].0, memory_fact_id, "nearest neighbor should be seed=1 vector");

// Negative test: scope_type CHECK blocks invalid value.
let bad_scope = sqlx::query(
    "INSERT INTO memory_items (scope_type, scope_id, group_id, created_by, \
     created_by_label, kind, content) \
     VALUES ('invalid_scope', $1, $2, $3, 'X', 'fact', 'bad')"
)
.bind(user_id).bind(group_id).bind(user_id)
.execute(workspace.pool())
.await
.expect_err("scope_type CHECK should reject 'invalid_scope'");
let db_err = bad_scope.as_database_error().expect("database error");
assert_eq!(db_err.code().as_deref(), Some("23514"), "expected check_violation");

// Negative test: TTL in the past.
let bad_ttl = sqlx::query(
    "INSERT INTO memory_items (scope_type, scope_id, group_id, created_by, \
     created_by_label, kind, content, ttl_expires_at) \
     VALUES ('user', $1, NULL, $2, 'X', 'fact', 'expired', now() - interval '1 day')"
)
.bind(user_id).bind(user_id)
.execute(workspace.pool())
.await
.expect_err("TTL in past should be rejected");
assert_eq!(
    bad_ttl.as_database_error().and_then(|e| e.code().map(|c| c.to_string())).as_deref(),
    Some("23514"),
    "expected check_violation for past TTL"
);
```

### 7.2 What we are NOT testing

- HNSW query performance at scale (benchmark in `benches/database-poc/` already covered 100k).
- Scope resolution rules (Chat > Group > User) — app-layer concern.
- Cross-model embedding coexistence (single model in test).
- Retrieval relevance beyond nearest-neighbor ordering.
- RLS (migration 007).

---

## 8. Rollback plan

Identical to previous plans:

1. **Before merge:** close the PR.
2. **After merge, before downstream consumer:** `git revert`. The `CREATE EXTENSION IF NOT EXISTS vector` is idempotent-safe — if reverted, the extension stays installed in the DB but no tables reference it.
3. **After downstream consumer:** forward-fix via new migration.

**pgvector dep addition** to `Cargo.toml` is reversible via `git revert` — the dep doesn't bleed into other crates (it's only in `garraia-workspace`).

---

## 9. Risks & mitigations

| Risco | Prob. | Impacto | Mitigação |
|---|---|---|---|
| `pgvector` crate sqlx feature API quirks | Baixa | Médio | Já validado em `benches/database-poc/` — copiar padrão literal |
| HNSW index build time dispara smoke test timeout | Baixa | Médio | Só 3 rows no teste — construção trivial |
| `vector(768)` dimension mismatch em `.bind()` | Média | Baixo | Teste negativo valida, mas o happy path usa `unit_vector(...)` que retorna sempre 768 |
| `scope_id` sem FK permite dangling references | Média | Médio | Documentado via COMMENT; garraia-auth (GAR-391) vai adicionar audit query de orphans |
| Memória pessoal vaza para group retrieval por bug de RLS futuro | Média | **Alto (LGPD)** | RLS centralizado em migration 007; até lá, documentado explicitamente que app layer É a barreira |
| Smoke test não determinístico (HNSW approx) | Baixa | Médio | HNSW é aproximado mas para 3 rows retorna todos corretamente; ordem é determinística pela distance cosseno exata |

---

## 10. Sequence of work (ordem proposta quando aprovado)

### Wave 1 — migration SQL + Cargo.toml dep + test extension (~1.5h, single agent)

1. Adicionar `pgvector = { version = "0.4", features = ["sqlx"] }` a `crates/garraia-workspace/Cargo.toml` + `rand = "0.8"` + `rand_chacha = "0.3"` (dev-deps para `unit_vector`).
2. Criar `crates/garraia-workspace/migrations/005_memory_items_and_embeddings.sql` literalmente conforme §6.
3. Estender `tests/migration_smoke.rs` com o bloco §7.1.
4. Update `README.md` §Scope + §Running the tests.
5. `cargo clean -p garraia-workspace && cargo test -p garraia-workspace`. Iterar.
6. `cargo clippy -p garraia-workspace --all-targets -- -D warnings`. Verde.
7. Wall time ≤ 20s.

### Wave 2 — parallel review (~20min wall)

8. `@code-reviewer` — SQL correctness, HNSW index syntax, pgvector 0.4 binding, test validity, scope_id FK-less documentation.
9. `@security-auditor` — LGPD memory segregation posture, scope_type enforcement gap, sensitivity='secret' handling, PII in content COMMENT.

### Wave 3 — fixes + ROADMAP + commit (~30min)

10. Aplicar findings.
11. ROADMAP.md §3.2: marcar memory_items + memory_embeddings como `[x]`.
12. Commit + push.
13. Linear GAR-389 → Done.

**Total: 2-3 horas.**

---

## 11. Definition of Done

- [ ] Todos os 16 itens do §4 marcados.
- [ ] PR merged em `main`.
- [ ] Review verde de ambos agentes.
- [ ] Linear GAR-389 → Done.
- [ ] ROADMAP §3.2 atualizado.
- [ ] Próxima sessão pode abrir GAR-408 (migration 007 RLS) com TODAS as tenant-scoped tables existindo.

---

## 12. Open questions (preciso da sua resposta antes de começar)

1. **`vector(768)` hardcoded ou `vector` sem dimensão?** Recomendo **`vector(768)`** — pgvector 0.7+ suporta dimension-less mas perde a validação de shape no nível do schema. Fixar em 768 matches `mxbai-embed-large-v1` (Fase 2.1 default). Multi-dim entra via nova coluna/tabela. Confirma?

2. **`memory_items.group_id` nullable?** Recomendo **sim** — para `scope_type='user'`, memória pessoal não tem grupo. Nullable permite RLS em migration 007 distinguir "group-visible" (`WHERE group_id = ...`) de "user-only" (`WHERE created_by = ... AND group_id IS NULL`). Confirma?

3. **`source_chat_id` / `source_message_id` ON DELETE SET NULL ou CASCADE?** Recomendo **SET NULL** — memórias sobrevivem deleção da fonte (audit + retenção). Trade-off: perde a provenance exata, mas ganha retenção. Confirma?

4. **Trigger para validar `scope_id` vs `scope_type`?** Recomendo **NÃO** — Postgres triggers adicionam 5-10% overhead em insert hot path; scope_id integrity é problema do app layer e será auditado por `garraia-auth` (GAR-391). Confirma?

5. **`sensitivity` com 4 níveis (public/group/private/secret) ou 3 (public/private/secret)?** Recomendo **4 níveis** — `group` é semanticamente distinto de `private` (partilhado dentro do grupo mas não com outsiders). Confirma?

6. **`kind` enum — 6 valores (fact/preference/note/reminder/rule/profile) ou deixar `text` livre?** Recomendo **enum CHECK** — força o agente de extração a categorizar, facilita retrieval filtrado. Confirma?

7. **HNSW cosine vs L2 vs inner product?** Recomendo **cosine (`vector_cosine_ops`)** — matches benchmark + matches como `mxbai-embed-large-v1` foi treinado (normalized embeddings, cosine similarity). Confirma?

---

## 13. Next recommended issue (depois de GAR-389 merged)

**GAR-408 — Migration 007 RLS em TODAS as tenant-scoped tables** (2-3h)

**Por quê firmemente recomendado:**
- Com memory, chats, messages, message_threads todos existindo, este é o momento único de aplicar `ENABLE ROW LEVEL SECURITY + FORCE + CREATE POLICY` em UMA migration só, fechando o loop de isolação LGPD/GDPR antes que qualquer API (GAR-393) ou auth logic (GAR-391) toque as tabelas.
- Se GAR-408 atrasar, corremos o risco de API code ser escrito presumindo isolação app-layer, e depois ter que refactor quando RLS for ligado.
- Sessão curta, bem delimitada. Padrão estabelecido — só emitir `ALTER TABLE ... ENABLE RLS` + `FORCE RLS` + `CREATE POLICY ... USING (group_id = current_setting(...))` para cada tabela.

**Alternativa (se preferir diferir RLS):** **GAR-390 migration 006 tasks** — completa o set de migrations antes de RLS. Mais 1 migration mecânica, ~2h. Mas atrasa o fechamento de segurança.

**Minha recomendação firme: GAR-408 imediatamente.** Todo risco de API drift é evitado.

---

**Aguardando sua aprovação.** Se aprovar como está, respondo as 7 open questions com os defaults recomendados e começo pelo passo 1 do §10. Se quiser cortar escopo ou mudar uma decisão, me diga antes que eu toque em código.
