# Plan 0048 — GAR-413 Stage 6 (`migrate workspace` messages batched)

**Status:** Aprovado 2026-04-22 — Lote B-A (parallel com 0047)
**Autor:** Claude Opus 4.7 (sessão autônoma, ClaudeMaxPower + Superpowers, Lote B)
**Data:** 2026-04-22 (America/New_York)
**Issue:** [GAR-413](https://linear.app/chatgpt25/issue/GAR-413) (reaberta 2026-04-22 22:28 UTC) — parcial, stage 6/10
**Branch:** `plan/0048-stage6-messages`
**Pré-requisitos:** Lote A merged (plans 0039/0040/0045 cobrem stages 1+3+5); Gate Zero cargo-audit verde ([run #24805793514](https://github.com/michelbr84/GarraRUST/actions/runs/24805793514)). 0049 (clippy strict) **não** bloqueia — este PR roda sob `continue-on-error: true` atual se mergear antes; se mergear depois, satisfaz gate estrito desde o dia 1.
**Unblocks:** Stage 7 (`memory_items` + `memory_embeddings`) que consome `MessageMapping` exposto aqui para resolver legacy memory refs.

---

## 1. Goal

Implementar §7.6 do plan 0034 (GAR-413 spec) — migração batched de `messages` do SQLite legacy para o schema Postgres workspace — **com amendments normativos** derivados do estado real do código (plan 0034 foi escrito antes da implementação dos stages 1/3/5 e algumas suposições precisam corrigir).

Entrega:

1. **Novo `run_stage6_messages`** chamado após `run_stage5_chats` **na mesma transação Postgres** (atomicidade preservada: failure arrasta stages 1+3+5+6 juntos).
2. **Batched reader** de SQLite `messages` (batch default 500 rows/query — herda `--batch-size` flag existente de plan 0034 §5.2).
3. **Role/direction-aware skip** (ver §5.1 para mapeamento real SQLite → Postgres — amendment normativo).
4. **Resolução `chat_id`** via `ChatMapping { session_id → chat_id }` exposto em memória pelo Stage 5 (plan 0045 comment `6b9a57fc`).
5. **Resolução `sender_user_id`** via `UserMapping { legacy_sqlite_id → uuid }` exposto pelo Stage 1 (plan 0039 comment `b201e3e2`).
6. **UNNEST arrays** em `sqlx::query!` para batched insert de alta perf (> 10k msgs em single tx).
7. **Audit atômico** `message.imported_from_sqlite` — 1 row agregado por batch (`count + min_id + max_id`) em `audit_events`, `WHERE NOT EXISTS` para idempotência.
8. **Expõe `MessageMapping { legacy_message_id → new_message_id: Uuid }`** em memória para Stage 7 consumir.
9. **PII redaction obrigatório:** `tracing::instrument(skip(body, batch))` + zero field de log imprime `message.content`. Body é PII user-generated (CLAUDE.md regra #6); verificado via new snapshot test `no_message_body_in_logs`.
10. **Split objetivo de `migrate_workspace.rs`** se o arquivo ultrapassar 1.800 LOC **ou** o stage 6 sozinho acrescentar > 500 LOC novas (§6 — regra obrigatória deste slice, aprovada pelo usuário 2026-04-22).
11. **Advisory lock** `pg_try_advisory_lock(hashtext('garraia_migrate_workspace'))` no topo do fluxo — impede duas runs concorrentes corromperem audit (follow-up aberto desde plan 0039; fechado neste slice).

**O que NÃO entra neste slice:**
- Stage 7 (`memory_items` + `memory_embeddings`) — slice futuro consumirá `MessageMapping`.
- Stage 8 (`sessions` ephemeral), Stage 9 (`api_keys`), Stage 10 (`audit` retrofit) — slices futuros.
- `--batch-size` sendo aplicado aos stages 1/3/5 (já merged sem batch; refactor cosmético deferido).
- `--resume-from-offset` para retomada pós-falha — plan 0034 §14 aceitou O(rows_já_importadas) em re-run via `ON CONFLICT`.
- Migração de `conversations` (tabela que plan 0034 §7.6 imaginava mas não existe — amendment §5.1).

## 2. Non-goals

- Zero mudança no schema Postgres (`messages`, `chats`, `group_members` intocados — migration 004 já suporta).
- Zero mudança no schema SQLite legacy (read-only — CLAUDE.md regra #10 + plan 0034 §2).
- Zero mudança em API de providers (OpenAI/Anthropic/OpenRouter) — é só ETL.
- Zero dependência Rust nova (uuid, sqlx, rusqlite, tracing já em workspace).
- Zero breaking change nas APIs públicas do CLI `garraia migrate workspace`.
- Zero mudança no fluxo runtime de chat (messages Postgres intocado por este PR).

## 3. Scope

**Arquivos modificados:**

- `crates/garraia-cli/src/migrate_workspace.rs` — extensão com `run_stage6_messages` + helpers (estimativa ~450 LOC novas; **aciona gatilho de split** da §6 se `migrate_workspace.rs` ultrapassar 1.800 LOC total, o que é esperado — ver §6).
- `crates/garraia-db/src/session_store.rs` — **zero mudança** esperada; a leitura legacy usa rusqlite direto (sem método novo em `SessionStore`). Se precisar de `get_messages_batch(session_id, offset, limit)`, adiciona ali como método read-only.

**Arquivos novos (caso o split §6 seja acionado):**

- `crates/garraia-cli/src/migrate_workspace/mod.rs` (novo arquivo do módulo)
- `crates/garraia-cli/src/migrate_workspace/stages/mod.rs`
- `crates/garraia-cli/src/migrate_workspace/stages/stage1.rs` (movido do atual)
- `crates/garraia-cli/src/migrate_workspace/stages/stage3.rs` (movido)
- `crates/garraia-cli/src/migrate_workspace/stages/stage5.rs` (movido)
- `crates/garraia-cli/src/migrate_workspace/stages/stage6.rs` (novo)
- `crates/garraia-cli/src/migrate_workspace/preflight.rs` (movido do atual)
- `crates/garraia-cli/src/migrate_workspace/report.rs` (movido do atual)
- `crates/garraia-cli/tests/migrate_workspace_stage6_integration.rs` (novo test binary)
- `plans/0048-gar-413-stage6-messages.md` (este arquivo).

**Atualização do índice:**

- `plans/README.md` — entrada 0048.

**Zero nova dependência Rust.**

## 4. Acceptance criteria

1. `cargo check -p garraia-cli` verde.
2. `cargo fmt --check --all` verde.
3. `cargo clippy -p garraia-cli --all-targets -- -D warnings` verde (satisfaz 0049 se já mergeado).
4. `cargo test -p garraia-cli --lib` verde (unit tests novos).
5. `cargo test -p garraia-cli --test migrate_workspace_stage6_integration` verde via testcontainer pgvector+pg16 real aplicando migrations 001-014.
6. `cargo test -p garraia-cli --test migrate_workspace_integration` (existentes dos stages 1+3+5) **continuam verdes** — regression gate.
7. **Split §6 satisfeito:** se `migrate_workspace.rs` delta crescer > 1.800 LOC total OU stage6 > 500 LOC novas, split é aplicado; acceptance verificada por `wc -l crates/garraia-cli/src/migrate_workspace/stages/*.rs` vs `crates/garraia-cli/src/migrate_workspace.rs` antes do merge.
8. Grep `skip(body)` em `tracing::instrument` do stage6 retorna ≥ 1 match; grep `message.content` em tracing fields retorna zero.
9. `migrate_report.messages_imported` + `migrate_report.messages_skipped_assistant` + `migrate_report.messages_skipped_unsupported_role` + `migrate_report.messages_skipped_no_chat_mapping` somados == rows totais de `sqlite.messages` (invariante de conservação).
10. `run_stage6_messages` rerun em DB já migrado → 0 INSERTs novos (idempotência via `ON CONFLICT (legacy_sqlite_id) DO NOTHING`).
11. Advisory lock: segunda run concorrente em outra conexão retorna `StageError::ConcurrentMigrationInProgress` (exit 75 = `EX_TEMPFAIL` per sysexits.h) sem tocar o DB.
12. `@code-reviewer` e `@security-auditor` APPROVE.
13. CI 9/9 green no PR.

## 5. Implementation details

### 5.1 Amendment normativo ao plan 0034 §7.6 — coluna real é `direction`, não `role`

Plan 0034 §7.6 linhas 307-313 pressupõem uma coluna `role` em `sqlite.messages`. **O schema real é diferente.** Evidência em `crates/garraia-db/src/session_store.rs:118-131`:

```sql
CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    direction TEXT NOT NULL,     -- ← coluna real
    content TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    metadata TEXT DEFAULT '{}',
    source TEXT, provider TEXT, model TEXT,
    tokens_in INTEGER, tokens_out INTEGER,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

Também: `session_id` (não `conversation_id` do plan 0034). Precedente: plan 0045 fez exatamente o mesmo amendment para Stage 5 (`sessions` em vez de `conversations`) — seguimos o mesmo pattern.

**Mapeamento direction → attribution rule (revisão ao plan 0034 §7.6):**

| SQLite `direction` | Postgres `messages.sender_user_id` | Ação |
|---|---|---|
| `"inbound"` (user → sistema) | `first_migrated_user_id` do session owner (via `UserMapping`) | **INSERT** com `sender_label = users.display_name` |
| `"outbound"` (sistema → user) | N/A — IA não é `users.id` | **SKIP**; `report.messages_skipped_assistant += 1` |
| `""` / `NULL` / outro | — | **SKIP**; `report.messages_skipped_unsupported_direction += 1` |

**Valores reais do `direction`** esperados com base no código atual de `session_store.rs` (métodos `persist_inbound` / `persist_outbound`): `"inbound"` e `"outbound"`. Primeira task do slice: **verificar via `SELECT DISTINCT direction FROM messages;`** em um dump de produção/dev antes de codar o branch completo. Se houver variantes (`"user"`, `"assistant"`, `"system"`), tratar explicitamente.

**Efeito prático:** 0 mensagens de resposta da IA migram para Postgres. O valor histórico de replies do assistant é limitado (conforme argumento plan 0034 §7.6: "Mensagens históricas da IA NÃO são authoritative"). Operators que queiram preservar replies podem re-rodar export LGPD sobre o .db antes do migrate (plan 0034 §5.4).

### 5.2 `chat_id` resolution — amendment ao plan 0034

Plan 0034 §7.6 linha 322 diz `row.conversation_id`. O schema real é `row.session_id` (FK → `sessions.id`). Mapping feito via `ChatMapping { session_to_chat: HashMap<String, Uuid> }` exposto em memória pelo Stage 5 (plan 0045 entregou).

**Se `session_id` não está em `ChatMapping`** (ex.: session migrado por Stage 5 pulou por órfão → counter `sessions_skipped_no_user`), mensagem é **SKIP** + `report.messages_skipped_no_chat_mapping += 1`. Invariante de conservação na acceptance #9 depende dessa categoria.

### 5.3 `sender_user_id` + `sender_label` resolution

- `session.user_id` do SQLite mapeia via `UserMapping { legacy_sqlite_id → uuid }` (do Stage 1).
- `sender_label` = `users.display_name` lido via subquery inline: `(SELECT display_name FROM users WHERE id = $N)`. Prefere subquery à cache em memória para não duplicar estado e simplificar batch UNNEST.

Quando display_name é NULL/empty no users row (edge case), cai para `"Unknown legacy user"` fixo (documentado em §12 open question 1).

### 5.4 Batched INSERT via UNNEST (perf)

Pattern usado para `--batch-size 500` rows/batch:

```rust
sqlx::query!(
    r#"
    INSERT INTO messages
        (id, chat_id, group_id, sender_user_id, sender_label, body, created_at, legacy_sqlite_id)
    SELECT * FROM UNNEST(
        $1::uuid[], $2::uuid[], $3::uuid[], $4::uuid[],
        $5::text[], $6::text[], $7::timestamptz[], $8::text[]
    )
    ON CONFLICT (legacy_sqlite_id) DO NOTHING
    "#,
    &ids[..],
    &chat_ids[..],
    &group_ids[..],
    &sender_user_ids[..],
    &sender_labels[..],
    &bodies[..],      // ← SEC CRÍTICO: zero PII em tracing fields. skip(bodies).
    &timestamps[..],
    &legacy_ids[..],
).execute(&mut *tx).await?;
```

**Pré-requisito do schema:** `messages` Postgres tem coluna `legacy_sqlite_id text UNIQUE`? **Verificar na primeira task.** Se não tem, **plan 0034 §7.6 precisa de migration 015** para adicionar (migration forward-only). Abordagem: preferir `legacy_sqlite_id` para estar em paridade com `users.legacy_sqlite_id` (Stage 1, migration 001). Se adicionamos migration 015, atualiza-se acceptance #5 (testcontainer aplica até 015).

**Alternativa sem migration:** usar UUIDs v7 derivados determinísticamente de `sqlite.messages.id` (via `Uuid::new_v5(&NAMESPACE_OID, legacy_id_bytes)`) e idempotência via `ON CONFLICT (id) DO NOTHING`. **Requer UUID v5 não v7 para determinismo.** Trade-off: v5 quebra o pattern v7 dos outros stages (sortable by time). Open question §12.2.

### 5.5 Audit aggregation

Plan 0034 pattern: 1 row aggregate por batch, não 1 por mensagem. Evita inflar `audit_events` para `10^5` rows.

```sql
INSERT INTO audit_events (
    id, group_id, actor_user_id, action, target, metadata, created_at
) SELECT
    gen_random_uuid(),
    $1::uuid,
    NULL,  -- system-authored action
    'message.imported_from_sqlite',
    format('batch_%s_to_%s', $2::text, $3::text),
    jsonb_build_object(
        'batch_count', $4::int,
        'min_legacy_id', $2::text,
        'max_legacy_id', $3::text,
        'skipped_assistant', $5::int,
        'skipped_no_mapping', $6::int
    ),
    now()
WHERE NOT EXISTS (
    SELECT 1 FROM audit_events
    WHERE action = 'message.imported_from_sqlite'
      AND target = format('batch_%s_to_%s', $2::text, $3::text)
);
```

Idempotência via `WHERE NOT EXISTS` + composite key (action, target).

### 5.6 Advisory lock

No topo do fluxo completo (`run()` function), antes de qualquer BEGIN:

```rust
let advisory_key = "garraia_migrate_workspace".hash_i64();
let lock_acquired: bool = sqlx::query_scalar!(
    "SELECT pg_try_advisory_lock($1)",
    advisory_key
).fetch_one(&pool).await?.unwrap_or(false);

if !lock_acquired {
    return Err(StageError::ConcurrentMigrationInProgress);
}

let _guard = AdvisoryLockGuard::new(advisory_key, &pool);  // releases on Drop
```

`AdvisoryLockGuard` custom RAII que chama `pg_advisory_unlock($1)` no Drop.

**Novo `StageError::ConcurrentMigrationInProgress`** com exit code **75** (`EX_TEMPFAIL` — sysexits.h).

### 5.7 Tracing redaction

```rust
#[tracing::instrument(
    name = "stage6.run",
    skip(tx, chat_mapping, user_mapping, options, report),
    fields(
        sqlite_path = %options.sqlite_path.display(),
        batch_size = options.batch_size,
    )
)]
pub async fn run_stage6_messages(...) -> Result<(), StageError> { ... }
```

E dentro do loop de batch:

```rust
#[tracing::instrument(
    name = "stage6.batch",
    skip(tx, rows, chat_mapping, user_mapping),  // ← rows contém bodies PII; skip obrigatório
    fields(batch_offset, batch_len = rows.len())
)]
async fn commit_batch(...) { ... }
```

**Snapshot test `no_message_body_in_logs`:**
- Captura logs de um run test via `tracing_subscriber::fmt::with_writer(Vec<u8>)`.
- Assert `!logs.contains(&fixture_message_body)` para body plantado propositalmente no SQLite test fixture.

### 5.8 `MessageMapping` exposto

```rust
pub struct MessageMapping {
    pub legacy_to_new: HashMap<String, Uuid>,
    pub skipped_legacy_ids: HashSet<String>,
}
```

Public field `migration_context.messages` (ou analog nomenclatura); lifetime cobre até o fim da tx (retornado junto com `StageReport`). Stage 7 consumirá em slice futuro quando memory_items estiverem ligadas a messages via `memory_items.source_message_id`.

## 6. Regra objetiva de split de `migrate_workspace.rs` (aprovado pelo usuário 2026-04-22)

**Gatilho:** se após o fim do stage 6 qualquer uma das condições for true:

- (a) `wc -l crates/garraia-cli/src/migrate_workspace.rs` > **1.800 LOC** total, **OU**
- (b) delta do stage 6 sozinho > **500 LOC novas**,

então **split obrigatório neste PR** (não é opcional, não é "considerar", não é follow-up 0048-b).

**Estrutura de split:**

```
crates/garraia-cli/src/
  migrate_workspace.rs        ← REMOVED
  migrate_workspace/
    mod.rs                    ← pub use + run() principal (~200 LOC)
    preflight.rs              ← run_preflight_checks (~250 LOC, movido do atual)
    report.rs                 ← StageReport, RunOptions (~100 LOC, movido)
    errors.rs                 ← StageError enum (~50 LOC, movido)
    stages/
      mod.rs                  ← pub use stage1, stage3, stage5, stage6
      stage1.rs               ← run_stage1_users + identities (~400 LOC, movido)
      stage3.rs               ← run_stage3_groups (~350 LOC, movido)
      stage5.rs               ← run_stage5_chats (~450 LOC, movido)
      stage6.rs               ← run_stage6_messages (~450 LOC, novo)
```

**Custo esperado do split:** ~50 LOC de import/re-export bookkeeping + zero mudança de comportamento. Validado por toda a suite de integration tests existentes (migrate_workspace_integration.rs) passando sem modificação de teste (move-only).

**Caso (a) e (b) NÃO sejam satisfeitos** (improvável — stage 6 LOC estimado 450 + migrate_workspace.rs atual 1.590 = ~2.040 > 1.800): mantemos tudo em `migrate_workspace.rs` e abrimos follow-up 0048-b para revisitar split em Stage 7.

**Decisão congelada no commit 1 do PR:** medir LOC inicial pós-stage6-draft, decidir split ou não antes do review. Plan file atualizado com a escolha.

## 7. Estratégia de testes

### 7.1 Unit tests (novos, em `crates/garraia-cli/src/migrate_workspace[/stages]/stage6.rs` ou `migrate_workspace.rs`)

- `test_direction_to_role_inbound_is_user` (pure fn test).
- `test_direction_to_role_outbound_is_skip`.
- `test_direction_to_role_unknown_is_skip`.
- `test_chat_mapping_lookup_miss_increments_counter`.
- `test_user_mapping_lookup_miss_skips_message`.
- `test_batch_chunks_exactly_batch_size`.
- `test_batch_chunks_empty_sqlite`.

### 7.2 Integration tests (novo binary `crates/garraia-cli/tests/migrate_workspace_stage6_integration.rs`)

Usando `testcontainers-modules::postgres` (pgvector/pg16) + rusqlite tempdir:

1. **`stage6_happy_path_1k_messages`** — SQLite com 1 session + 1000 inbound + 200 outbound messages; verificar 1000 rows em `pg.messages` + 200 em `report.messages_skipped_assistant` + audit aggregate count correto.
2. **`stage6_idempotent_rerun`** — roda 2×, asserta 0 novas rows no segundo run + audit não duplica.
3. **`stage6_batched_boundary`** — `--batch-size 500` com 1001 mensagens inbound: 3 batches (500, 500, 1), todos commit OK.
4. **`stage6_skip_orphan_session`** — SQLite com message cujo session_id não foi migrado (stage 5 pulou) → message também skip + counter correto.
5. **`stage6_skip_outbound`** — SQLite só com outbound → 0 rows no pg.messages + counter == total.
6. **`stage6_atomic_rollback`** — simular falha mid-batch (ex.: body excede CHECK 100000 chars) → rollback arrasta stages 1+3+5+6, DB volta ao estado pré-migrate.
7. **`stage6_no_pii_in_logs`** — snapshot test: body fixture conhecido não aparece em logs capturados.
8. **`stage6_advisory_lock_blocks_concurrent`** — run A segura lock; run B em thread separada retorna `ConcurrentMigrationInProgress` exit 75.

### 7.3 Regression (existentes)

- `migrate_workspace_integration` (stages 1+3+5): **continua passando sem modificação**. Split §6 é move-only, não altera call paths.

### 7.4 End-to-end (manual, pré-merge)

- `garraia migrate workspace --from-sqlite <real-dev-dump.db> --to-postgres <testcontainer>` em DB com 10k mensagens; mede tempo < 30s + `report` stdout bate com `SELECT COUNT(*) FROM pg.messages`.

## 8. Rollback plan

- **Reversível parcial:** código é reverível via `git revert`, mas schema changes (se houver `legacy_sqlite_id` migration 015) são forward-only — rollback do código mantém o DDL em `main`. Operators que rodaram `garraia migrate workspace --dry-run` não são afetados (tx rolled-back).
- **Operators que executaram live migration** (`--confirm-backup`) com stage 6 precisariam de **script manual** para `DELETE FROM messages WHERE legacy_sqlite_id IS NOT NULL` + revert código. **Isso é uma regressão de segurança** — rollback destrói dados importados. Documentado claramente no PR description.
- **Melhor prática operacional:** snapshot Postgres ANTES de rodar stage 6 live (Postgres pgBasebackup ou `pg_dump`). Plan 0034 §10 já recomenda backup pré-migrate; Stage 6 reforça.
- **Advisory lock:** falha do processo deixa lock órfão que só libera ao final da conexão (Postgres liberates on session end). Docs.

## 9. Impacto em docs

- `CLAUDE.md` — §garraia-cli adicionar "Plan 0048 (GAR-413 Stage 6) acrescenta `run_stage6_messages` batched + split condicional de `migrate_workspace.rs` em `mod stages`".
- `plans/README.md` — entrada 0048.
- `plans/0034-gar-413-migrate-workspace-spec.md` — **amendment inline** no §7.6 corrigindo (`role` → `direction`, `conversation_id` → `session_id`). Amendment, não reescrita; plan 0034 é imutável per plans/README.md regra "Imutável após merge" — a correção vai como "Amendment 2026-04-22" no topo do §7.6.
- Eventual ADR nova se migration 015 for necessária: **não necessário** — mudança de schema forward-only não quebra ADR existente.

## 10. Impacto em workflows Linear

- GAR-413 continua **In Progress** (reaberta hoje 22:28 UTC por plan 0048 lote).
- Comentário de abertura deste plan: já inserido em `8aa67995-7bc7-49b6-8a9a-229cd097eea4`.
- Comentário de shipado (após merge): padrão Lote A stage-by-stage, `Stage 6 shipped — 6/10 stages shipped; Stage 7 memory_items next`.

## 11. Critério claro de pronto

- PR #XX mergeado em `main`, squash commit.
- CI 9/9 green.
- `@code-reviewer` APPROVE (ou APPROVE WITH NITS — blockers endereçados).
- `@security-auditor` APPROVE (foco: PII redaction, SQL injection no UNNEST, advisory lock race, audit atomicity).
- GAR-413 com comment "Stage 6 shipped" + reabertura para Stage 7 (ou fecha se user decidir stop-here; default: manter pattern "reopen per stage").
- `migrate_workspace.rs` ou split `migrate_workspace/**` passa `wc -l` per §6.
- Integration test `stage6_happy_path_1k_messages` roda em < 10s em CI.

## 12. Open questions

1. **`sender_label` fallback quando `users.display_name` é NULL/empty.** Proposta: `"Unknown legacy user"` fixo (log warn). Alternativa: skip message + counter (mais strict). Decidir no commit 1.
2. **UUID v7 vs v5 determinístico para `messages.id`.** Se schema NÃO tem `legacy_sqlite_id`: preferir migration 015 adicionando a coluna + UUID v7. Alternativa: UUID v5 derivado de `NAMESPACE_OID + sqlite_message_id` (determinístico, idempotente sem coluna extra). Recomendação forte: **migration 015 + v7** (alinha com stages 1/3/5, mantém pattern de sortable IDs). Decidir no commit 1 após inspecionar schema atual.
3. **Valores reais de `direction` em `sqlite.messages`.** Confirmar via `SELECT DISTINCT direction` em dev dump antes de codar branch completo. Se houver `"user"`/`"assistant"` em dados legados (antes da refactor persistent), mapear também.
4. **Batch de audit (1 agregado vs N individuais).** Plan usa aggregate 1-per-batch por perf. `@security-auditor` no plan 0034 aceitou. Revisitar se security review do 0048 apontar granularidade insuficiente.
5. **Split imediato vs condicional.** §6 é objetivo e será decidido no commit 1. Se o LOC real ficar abaixo de 1.800 e stage6 < 500 LOC, **não splitamos neste PR** — tracking em 0048-b.

## 13. Métricas esperadas

- **LOC delta (líquido):** +450 (stage6 novo) + até +400 (split bookkeeping se acionado) = +450 a +850.
- **Testes novos:** 7 unit + 8 integration = 15 testes novos.
- **Cobertura esperada em `migrate_workspace/stages/stage6.rs`:** ≥ 80% linhas (alvo plan 0049 Q.1 baseline).
- **Dependências novas:** 0.
- **Migration novas:** 0 ou 1 (015 adiciona `messages.legacy_sqlite_id` se open question #2 escolher v7 — decidido no commit 1).
- **Performance alvo:** 10k mensagens migradas em < 30s em testcontainer pg16 local (baseline plan 0039: 3 users + 3 identities = 10ms; scale extrapolação 30k ops/s → 10k msgs em ~350ms, overhead comfort 100×).

## 14. Follow-ups conhecidos

- **Stage 7 (memory_items + memory_embeddings)** — consome `MessageMapping`. Plan 0053+ futuro.
- **0048-b** — se §6 split não acionar neste PR, follow-up revisitar em Stage 7 (provavelmente acima de 1.800 LOC aí).
- **`--resume-from-offset` flag** para retomada pós-falha mid-batch — plan 0034 §14 deferido; decisão empírica baseada em se re-run com `ON CONFLICT` provou ser demasiado lenta em produção.
- **Rollback helper** `garraia migrate workspace --rollback` — deferido desde plan 0039.
