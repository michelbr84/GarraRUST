# Plan 0045 — GAR-413 Stage 5 (`migrate workspace` chats + chat_members)

**Status:** Em execução
**Autor:** Claude Opus 4.7 (sessão autônoma, ClaudeMaxPower + Superpowers, Lote A-2)
**Data:** 2026-04-22 (America/New_York)
**Issues:** [GAR-413](https://linear.app/chatgpt25/issue/GAR-413) — stage 5/10
**Branch:** `feat/0045-gar-413-stage5-chats`
**Worktree:** `.worktrees/0045-stage5-chats`
**Spec normativa:** [plan 0034](0034-gar-413-migrate-workspace-spec.md) §7.5 (com amendment §5.1 deste plan)
**Pré-requisitos:** [plan 0040](0040-gar-413-stage3-groups.md) merged (Stages 1–3 shipped).
**Unblocks:** slice futuro Stage 6 (`messages` batched) e Stage 7 (`memory`).

---

## 1. Goal

Entregar o **stage 5** do comando `garraia migrate workspace` cobrindo `chats` + `chat_members` conforme §7.5 do plan 0034, com um único esclarecimento normativo: o ADR e o plan 0034 citavam "SQLite `conversations`", mas a verificação empírica em `crates/garraia-db/src/session_store.rs:105` mostra que a tabela legacy real se chama **`sessions`** (`sessions.id` é a chave da conversa; `sessions.channel_id` é o canal de origem). Este plan formaliza o mapping `sessions → chats` (v. §5.1 "Amendment ao plan 0034 §7.5").

1. Ler rows de `sessions` do SQLite (tenant/channel/user/metadata/timestamps).
2. Inserir 1 row por session em Postgres `chats` (type = `'channel'`, name derivado de `metadata.title` ou fallback determinístico, `group_id = legacy_group_id` do stage 3, `created_by = owner_user_id` do stage 3).
3. Inserir 1 row em `chat_members` por session por user migrado elegível: o `sessions.user_id` legacy é mapeado ao `users.legacy_sqlite_id` em Postgres; o resultado vira `chat_members (chat_id, user_id, role='owner', joined_at=sessions.created_at)`. Users migrados **sem** sessions legacy não recebem `chat_members` rows neste stage — o acesso aos chats vem da policy RLS `chats_group_isolation`.
4. Audit atômico inline: `chats.imported_from_sqlite` (1 row por chat criado) + `chat_members.imported_from_sqlite` (1 row por membership criado). Mesma transação do INSERT do chat/membership.
5. Mapa de tradução `legacy_session_id → new_chat_id` **exposto em memória** para o stage 6 (messages) usar em slice futuro; persistência não requerida neste slice.
6. Idempotência: `ON CONFLICT DO NOTHING` + `WHERE NOT EXISTS` em audit (mesmo padrão do stage 3).
7. Edge cases:
   - Tabela `sessions` ausente no SQLite (instalação antiga) → skip com WARN + exit 0 sem tocar Postgres.
   - `sessions` sem rows → skip silencioso.
   - `sessions.user_id` não tem match em `users.legacy_sqlite_id` (user legacy que não foi migrado pelo stage 1) → skip a session (não cria chat) + WARN counter.

Este slice é **executável** — roda automaticamente após stages 1+2+3 na mesma invocação do comando, dentro da mesma transação.

## 2. Non-goals

- **Não** implementa stages 6 (messages) — embora o map `session_id → chat_id` seja exposto para ele.
- **Não** migra `chat_summaries` (SQLite — chat sync summaries) — fora do escopo; possível slice futuro.
- **Não** migra `chat_session_keys` (mapping externo ↔ session) — esse é metadata de canal, não conversa.
- **Não** populate `chat_members.last_read_at`/`muted` — defaults NULL/false são corretos.
- **Não** diferencia `type='channel' | 'dm' | 'thread'`; tudo entra como `'channel'` (SQLite não tem essa distinção). Documentado em §5.2.
- **Não** toca no schema Postgres (migrations 001–014 já cobrem).
- **Não** altera `StageReport` fields existentes — estende com contadores novos.
- **Não** suporta `--only chats`/`--skip chats` (fica para o slice que introduzir flags de stage selection).
- **Não** importa `metadata` JSON do SQLite sessions para o `chats.settings` (campos incompatíveis; deferido).

## 3. Scope

**Arquivos modificados:**

- `crates/garraia-cli/src/migrate_workspace.rs` — novo `run_stage5_chats` chamado após `run_stage3_groups`; nova struct `ChatMapping { legacy_session_id → new_chat_id }` retornada; extensão de `StageReport` com `chats_inserted`, `chats_skipped_conflict`, `chat_members_inserted`, `chat_members_skipped_conflict`, `chat_audit_events_inserted`, `sessions_skipped_no_user`.
- `crates/garraia-cli/src/main.rs` — nenhuma alteração de flags (stage 5 roda por default, sem flag adicional).
- `crates/garraia-cli/tests/migrate_workspace_integration.rs` — 4 novos cenários (detalhados em §6.2).
- `plans/0045-gar-413-stage5-chats.md` (este arquivo).
- `plans/README.md` — entrada 0045.
- `CLAUDE.md` — atualizar descrição de `garraia-cli` mencionando Stage 5.

**Arquivos novos:** nenhum.

Zero dependência Rust nova.

## 4. Acceptance criteria

1. `cargo check -p garraia` verde.
2. `cargo clippy -p garraia --all-targets -- -D warnings` verde.
3. `cargo fmt --check` verde.
4. `cargo test -p garraia --lib` verde.
5. `cargo test -p garraia --test migrate_workspace_integration` verde localmente quando Docker disponível; skip graceful caso contrário.
6. Stage 5 roda por default após stages 1+2+3 — sem flag adicional.
7. Stage 5 insere exatamente 1 row em `chats` por session SQLite elegível (com `user_id` que bate em `users.legacy_sqlite_id`).
8. Sessions cujo `user_id` não tem correspondente em Postgres users são skipadas + counter `sessions_skipped_no_user` incrementado + WARN log.
9. Stage 5 insere exatamente 1 row em `chat_members` por session elegível (role=`owner`, joined_at = sessions.created_at).
10. Audit rows: `chats.imported_from_sqlite` e `chat_members.imported_from_sqlite` (1 cada por row de chats/chat_members inserido).
11. Idempotência: segunda execução sobre o mesmo SQLite não cria novos chats, chat_members ou audit rows.
12. Edge case: SQLite **sem** tabela `sessions` → WARN + exit 0 (stage 5 skipado). Outros stages não são afetados.
13. Edge case: SQLite com `sessions` vazia → exit 0 silencioso sem tocar Postgres.
14. Edge case: SQLite com zero users migrados (stage 3 emitiu WARN) → stage 5 skipado também com WARN (não há owner possível — chat ficaria órfão).
15. Stage 5 roda **na mesma transação** dos stages 1+2+3 — falha em chats rollback todo o run.
16. Mapa `ChatMapping` retornado pela função tem `.len() == chats_inserted` — smoke test que não há divergência entre inserts e mapping.
17. `@code-reviewer` APPROVE.
18. `@security-auditor` APPROVE ≥ 8.0/10.
19. CI 9/9 green.
20. Linear GAR-413 comentada (stage 5/10 done).
21. `plans/README.md` + `CLAUDE.md` atualizados.

## 5. Design rationale

### 5.1 Amendment ao plan 0034 §7.5 — `sessions` é o nome real

Plan 0034 §7.5 menciona "SQLite `conversations`". Na base legacy real (`session_store.rs:105-116`), a tabela se chama **`sessions`** com colunas:

```
sessions (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    channel_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    metadata TEXT DEFAULT '{}'
);
```

A semântica é idêntica ("container de mensagens de uma conversa dentro de um canal"). Plan 0045 usa `sessions` como fonte e **amenda o plan 0034** — quando o stage 6 for materializado, ele reutilizará o mesmo mapping `session_id → chat_id` gerado aqui (plan 0034 §7.6 `row.conversation_id` vira `row.session_id`).

Evidência: `garraia-db/src/session_store.rs:105-116`. A tabela `conversations` **não existe** no schema legacy. Plan 0034 foi escrito quando essa verificação ainda não tinha rodado; §7.5 assume nome aspiracional.

### 5.2 Todos os chats migrados viram `type='channel'`

Postgres `chats.type` CHECK aceita `'channel' | 'dm' | 'thread'`. SQLite sessions não tem essa distinção — eram uma conversa single-user por desenho. Escolha: `type='channel'` porque `dm` exige 2+ members (`chat_members`) e `thread` exige root message. Operadores que quiserem retag depois rodam UPDATE manual.

### 5.3 `chats.name` derivado com fallback

Prioridade:

1. `metadata.title` se presente e não vazio (JSON parse do SQLite `sessions.metadata`).
2. `"Chat {channel_id}"` (e.g., `"Chat mobile"`) — usa `sessions.channel_id` como hint de origem.
3. `"Legacy chat"` — fallback absoluto (não deve ocorrer se channel_id sempre presente).

Rationale: chats sem `name` em Postgres seriam `NOT NULL` violation — CHECK da migration 004.

### 5.4 `chats.created_by` = owner_user_id do stage 3

Migration 004 `chats.created_by` é `NOT NULL REFERENCES users(id)`. O owner do grupo (definido no stage 3) é o candidato natural — mesmo quando a session foi de outro user migrado, o `created_by` reflete o "proprietário do bucket" e o `chat_members.user_id = sessions.user_id` reflete quem usou. Esta divergência é intencional: `created_by` é attribution de criação do container; `chat_members.owner` é quem tem ACL de usuário.

Alternativa considerada e rejeitada: `chats.created_by = sessions.user_id`. Problema: se o user migrado específico for purgado (LGPD erasure), o `chats.created_by` fica NULL-ado (migration 004 linha 18 é NOT NULL) — não podemos usar NULL. Usando owner_user_id do bucket, o attribution sobrevive à remoção de qualquer user individual do bucket (exceto do próprio owner, o que é compliance-aceitável).

### 5.5 `chat_members.role = 'owner'` para o user da session

Migration 004 `chat_members.role` CHECK `'owner' | 'moderator' | 'member' | 'viewer'`. O legacy user que **era** o dono da conversa (SQLite single-user) deve ter controle total no novo schema. `'owner'` é a escolha natural.

### 5.6 Skip sessions sem user migrado → WARN counter

Se `sessions.user_id` não tem correspondente em `users.legacy_sqlite_id` (ex.: user legacy foi deletado do SQLite mas a session ficou órfã), não há como criar `chats.created_by` nem `chat_members.user_id`. Skip + WARN + `report.sessions_skipped_no_user += 1`. Não aborta o stage.

### 5.7 Audit atômico

Mesmo padrão dos stages 2 e 3: `WHERE NOT EXISTS (SELECT 1 FROM audit_events WHERE action='chats.imported_from_sqlite' AND resource_id = $chat_id::text)`. Idempotência garantida sem `UNIQUE` em audit_events.

`audit_events.metadata`:
- Para `chats.imported_from_sqlite`: `{source:'sessions', legacy_session_id, chat_type, channel_id}`.
- Para `chat_members.imported_from_sqlite`: `{source:'sessions', legacy_session_id, role, user_id, chat_id}`.

Zero PII em `metadata` (email, display_name não entram; `channel_id` é `'mobile'/'telegram'/...` — categoria, não PII).

### 5.8 `ChatMapping` estrutura

`pub struct ChatMapping { pub session_to_chat: HashMap<String, Uuid> }`. Construído durante o loop do stage 5; retornado por `run_stage5_chats`. Stage 6 (futuro) consumirá via parâmetro. Não persistido em DB.

### 5.9 Ordem determinística

`SELECT ... FROM sessions ORDER BY created_at ASC, id ASC` — determinístico. Matches pattern de `run_stage3_groups` para owner selection.

### 5.10 `chats.id` é UUID v7 gerado no Rust

Migration 004 `chats.id uuid DEFAULT gen_random_uuid()`. Gerar no Rust (`Uuid::now_v7()`) mantém ordenação temporal no PK + evita extensão `uuid-ossp` (não instalada; só `pgcrypto`). Mesmo padrão do stage 3.

## 6. Testing strategy

### 6.1 Unit

- `session_name_from_metadata(r#"{"title":"Hello"}"#)` → `"Hello"`.
- `session_name_from_metadata(r#"{}"#, channel_id="mobile")` → `"Chat mobile"`.
- `session_name_from_metadata` rejeita JSON malformado fail-closed → `"Legacy chat"`.
- `ChatMapping::default()` é vazio.

### 6.2 Integration (testcontainer pgvector + rusqlite tempdir)

Todos rodam via `cargo test -p garraia --test migrate_workspace_integration --features docker`.

- `stage5_happy_path_3_sessions_creates_chats_and_members`:
  - SQLite com 2 users + 3 sessions (2 do user A, 1 do user B).
  - Pós-run: 3 rows em `chats`, 3 rows em `chat_members` (2 para user A, 1 para user B).
  - 3 audit `chats.imported_from_sqlite` + 3 `chat_members.imported_from_sqlite`.
  - `ChatMapping.len() == 3`.

- `stage5_idempotent_rerun`:
  - Roda 2×. Counts idênticos após segunda.

- `stage5_skips_when_no_sessions_table`:
  - SQLite que só tem `mobile_users` (sem `sessions`) → stages 1–3 rodam; stage 5 emite WARN + skip sem erro.

- `stage5_skips_sessions_without_migrated_user`:
  - SQLite com 1 user migrado A + 2 sessions (1 do A, 1 órfã com user_id="deleted-legacy-user").
  - Pós-run: 1 chat + 1 chat_member; `report.sessions_skipped_no_user == 1` + WARN.

- `stage5_rollback_on_failure_preserves_stages_1_2_3`:
  - Simula erro injetando violation em chat_members (impossível de provocar com migrations normais; alternativa: `--dry-run` inspection + assert que nada foi persistido). Cenário substituível por "dry_run retorna report com contadores mas não commits".

### 6.3 End-to-end

- `migrate_then_list_chats_via_rest_v1` (deferido para slice futuro quando `GET /v1/chats` existir).

## 7. Security review triggers

- **SEC-H same-tx audit**: stage 5 escreve chats + chat_members + audit na **mesma tx** dos stages 1+2+3. Falha de qualquer INSERT rollback tudo. Regression guard na integration test `stage5_happy_path` via `COUNT(audit) == COUNT(chats) + COUNT(chat_members)`.
- **SEC-M session metadata parsing**: `session_name_from_metadata` faz `serde_json::from_str` com fallback silencioso (não fail-hard em JSON malformado — SQLite legacy pode ter strings não-JSON). Unit test cobre. Zero panic.
- **SEC-M SQLite query injection**: `sqlx::query!` não aplica (migrate_workspace_rs hoje usa `sqlx::query` não-macro por causa de schema dinâmico — mas zero concat manual; todas as queries parameterizadas com `$N`). CLAUDE.md rule #5 satisfeita.
- **SEC-L audit metadata PII**: `legacy_session_id` e `channel_id` (`'mobile'`, `'telegram'`) são categorias, não PII. `user_id` é UUID v7. Display names e emails **não** entram em metadata.
- **SEC-L `sessions.metadata` vs `chats.settings`**: metadata JSON legacy pode conter campos sensíveis (ex.: device ID, tokens). **Não** é copiado para `chats.settings` — documentado em §2 non-goals e no código.
- **SEC-L owner attribution`: `chats.created_by = owner_user_id` do bucket preserva attribution mesmo após erasure de um user migrado (§5.4 rationale). Stage 1 + 3 + 5 em mesma tx garantem que o owner está sempre presente quando stage 5 executa.

## 8. Rollback plan

Revertível por `git revert` — extensão pura de módulo Rust + 4 integration tests + 2 unit tests. Zero schema change.

Rollback de dados para um tenant que já rodou stage 5:

```sql
-- Delete chat_members (compound FK ON DELETE CASCADE de chats não os cobre sozinho — user precisa permanecer).
DELETE FROM chat_members
WHERE user_id IN (SELECT id FROM users WHERE legacy_sqlite_id IS NOT NULL)
  AND chat_id IN (
      SELECT id FROM chats
      WHERE created_by IN (SELECT id FROM users WHERE legacy_sqlite_id IS NOT NULL)
  );

-- Delete chats criados por stage 5 (identificáveis via audit_events).
DELETE FROM chats
WHERE id IN (
    SELECT resource_id::uuid FROM audit_events
    WHERE action = 'chats.imported_from_sqlite'
);

-- audit_events rows persistem by design (plan 0034 §8, LGPD art. 37).
```

Documentado em `migrate_workspace.rs` como docstring do stage 5.

## 9. Risk assessment

| Risco | Severidade | Mitigação |
|---|---|---|
| Session com `metadata` malformada quebra JSON parse | BAIXO | `serde_json::from_str` retorna `Err` → fallback para `"Chat {channel_id}"`; unit test cobre. |
| User legacy deletado do SQLite mas session preservada → órfã | BAIXO | §5.6 skip + WARN counter. Integration test `stage5_skips_sessions_without_migrated_user` cobre. |
| Cliente fez uma session inválida com `channel_id=NULL` | BAIXO | SQLite migration 004 do `garraia-db` declara `channel_id TEXT NOT NULL` — nunca NULL. Fallback ainda aplica. |
| Sessions >10k no SQLite lento em single-tx | MÉDIO | Volume real <100 esperado (single-user dev). Futuro `--batch-size` (flag dos stages 6+) mitiga. |
| `chats.created_by = owner_user_id` cria attribution esquisita se owner_user_id ≠ sessions.user_id | BAIXO | §5.4 documenta o trade-off (attribution survival ≥ precision). |
| Runs concorrentes duplicam chats | MÉDIO | Plan 0039 F-02: concurrent runs não suportados. Stage 5 herda (mesma tx). |
| Stage 5 falha parcialmente e rollback perde progresso dos stages 1–3 | BAIXO | By-design: mesma tx. Operator roda novamente; idempotência garante zero duplicate. |

## 10. Open questions

- **Q1**: Devemos copiar `sessions.updated_at` para `chats.updated_at`? → **Sim**; reflete o estado legacy. Zero custo.
- **Q2**: Devemos criar `chat_members` para **todos** os users migrados (não só o `sessions.user_id`)? → **Não**; chat_members é opt-in explícito no produto (migration 004 comment: "user may be in a group but not subscribed to every chat"). Pattern "group_members → all → chats → only creator" preserva princípio least-surprise.
- **Q3**: Devemos migrar `chat_session_keys` (mapping externo ↔ session)? → **Não** neste slice. É metadata de canal (Telegram/Discord chat IDs) — relevante só quando o gateway reconectar canais. Fora do escopo do migrate workspace.

## 11. Future work

- **Slice Stage 6** — messages batched com role-aware skip (plan 0034 §7.6). Reusa `ChatMapping` deste slice.
- **`--only` / `--skip`** — flags de stage selection introduzidos quando stages 6+ acumularem.
- **`chat_summaries` import** — se operator pedir, pode virar slice opcional para preservar sumários de chat sync.
- **`chat_session_keys` import** — se o reconnect-to-channels fluxo exigir, é issue separada.

## 12. Work breakdown

| Task | Arquivo | Estimativa |
|---|---|---|
| T1 | Helper `session_name_from_metadata` + unit tests | 20 min |
| T2 | Query `SELECT ... FROM sessions` com fallback "tabela ausente" | 25 min |
| T3 | `run_stage5_chats` core: loop inserindo chats/chat_members com audit atômico + `ChatMapping` | 70 min |
| T4 | Estender `StageReport` com contadores novos + `print_summary` | 20 min |
| T5 | Wire `run_stage5_chats` em `run` após `run_stage3_groups` | 15 min |
| T6 | Integration tests (4 cenários §6.2) | 90 min |
| T7 | CLAUDE.md + plans/README.md | 15 min |
| T8 | `@code-reviewer` + `@security-auditor` pass + fix findings | 60 min |

Total estimado: ~5h. Executado em worktree isolado, paralelo com A-1 (plan 0044).

## 13. Definition of done

- [ ] Todos os `Acceptance criteria` §4 verdes.
- [ ] `@code-reviewer` APPROVE.
- [ ] `@security-auditor` APPROVE ≥ 8.0/10.
- [ ] CI 9/9 green.
- [ ] PR aberto com link para este plan.
- [ ] PR merged em `main`.
- [ ] Linear GAR-413 atualizada (comentário Stage 5/10 done).
- [ ] `plans/README.md` entrada 0045 marcada `✅`.
- [ ] `CLAUDE.md` atualizado (menção garraia-cli Stage 5).
- [ ] `.garra-estado.md` atualizado ao fim da sessão.
