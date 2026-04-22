# Plan 0040 — GAR-413 implementation slice 2: Stage 3 (groups + group_members)

**Status:** Em execução
**Autor:** Claude Opus 4.7 (sessão autônoma, ClaudeMaxPower + Superpowers)
**Data:** 2026-04-22 (America/New_York)
**Issues:** [GAR-413](https://linear.app/chatgpt25/issue/GAR-413)
**Branch:** `feat/0040-gar-413-stage3-groups`
**Spec normativa:** [plan 0034](0034-gar-413-migrate-workspace-spec.md) §7.4
**Pré-requisitos:** [plan 0039](0039-gar-413-stage1-users-identities.md) merged (Stage 1 shipped).
**Unblocks:** slices futuros de GAR-413 (stages 5–10: chats, messages, memory, sessions, api_keys, audit retrofit).

---

## 1. Goal

Entregar o **stage 3** do comando `garraia migrate workspace` cobrindo exatamente o §7.4 do plan 0034:

1. Resolver (ou criar) o grupo-bucket legado a partir de `--target-group-name <STR>` + `--target-group-type <STR>`.
2. Criar row em `groups` (se ausente) com `type='personal'` (default), `created_by` = primeiro user migrado (por `users.created_at ASC`), `settings = '{}'::jsonb`.
3. Criar row em `group_members` para cada user migrado: primeiro = `owner`, demais = `member`, `status='active'`.
4. Emitir audit rows atômicos `groups.imported_from_sqlite` (1 por run) e `group_members.imported_from_sqlite` (1 por user).
5. Idempotência: `ON CONFLICT DO NOTHING` em `group_members (group_id, user_id)`; `SELECT ... FOR UPDATE` do lock serializando runs concorrentes.
6. Edge case "zero migrated users" → skip com WARN, exit 0.

Este slice é **executável** — roda automaticamente após os stages 1 e 2 já entregues pelo plan 0039, na mesma invocação do comando.

## 2. Non-goals

- **Não** implementa stages 5+ (chats, messages, memory, sessions, api_keys, audit retrofit).
- **Não** suporta `--only` / `--skip` (fica para o slice que introduzir stages 5+).
- **Não** suporta `--batch-size` (volume de users legacy tipicamente <1000, 1-stage tx).
- **Não** migra `mobile_users.role` para `group_members.role` — SQLite não tem esse campo; primeiro user = `owner` (ordem `created_at ASC`), demais = `member`. ADR 0003 é silent sobre ownership transfer; esta é a decisão documentada no plan 0034 §7.4.
- **Não** cria rows em `group_invites` — grupo é fechado, membros já viraram ativos.
- **Não** seta `group_members.invited_by` — NULL para membership de origem legacy (não existe invite history).
- **Não** customiza `groups.settings` — default `{}` deixa o produto preencher depois.
- **Não** altera o schema de `groups` ou `group_members` (migrations 001–013 já cobrem).

## 3. Scope

**Arquivos modificados:**

- `crates/garraia-cli/src/migrate_workspace.rs` — novo `run_stage3_groups` + extensão da função `run` para chamar-a após os stages 1+2 + extensão do `Stage1Report` → `StageReport` acumulando contadores.
- `crates/garraia-cli/src/main.rs` — novos flags `--target-group-name` (default `"Legacy Personal Workspace"`) + `--target-group-type` (default `"personal"`) no subcomando `Workspace`.
- `crates/garraia-cli/tests/migrate_workspace_integration.rs` — 3 novos cenários:
  - `stage3_creates_group_and_members_happy_path` (3 users → 1 group, 1 owner + 2 members, 1 + 3 audit rows).
  - `stage3_idempotent_rerun` (segunda execução não duplica nada).
  - `stage3_resolves_preexisting_group_by_name` (cenário operator-driven: grupo já existe com mesmo nome+type, é reaproveitado).
- `plans/0040-gar-413-stage3-groups.md` (este arquivo).
- `plans/README.md` — entrada 0040.
- `CLAUDE.md` — menção do Stage 3 em `garraia-cli/` na descrição do crate.

**Arquivos não alterados:**

- Nenhuma migration nova (schema 001–013 já cobre).
- Nenhuma dependência de crate adicionada.
- `garraia-auth`, `garraia-workspace`, `garraia-gateway`: zero mudanças.

## 4. Acceptance criteria

1. `cargo check -p garraia` verde.
2. `cargo clippy -p garraia --all-targets -- -D warnings` verde.
3. `cargo fmt --check` verde.
4. `cargo test -p garraia --lib` verde (unit tests).
5. `cargo test -p garraia --test migrate_workspace_integration` verde localmente quando Docker disponível; skip graceful caso contrário.
6. `garraia migrate workspace --help` exibe os novos flags.
7. Comando auto-runs Stage 3 após Stages 1+2 — sem flag opt-in necessária.
8. Stage 3 cria exatamente 1 row em `groups` (primeira invocação) + N rows em `group_members` (N = count de users migrados).
9. Exactamente 1 user migrado recebe `role='owner'`; demais recebem `role='member'`. Owner é o user com menor `created_at`.
10. Audit rows: `groups.imported_from_sqlite` (count = 1 quando grupo foi criado; 0 quando reaproveitado) + `group_members.imported_from_sqlite` (count = N na primeira execução, 0 em rerun).
11. Idempotência: segunda execução com mesmo SQLite não cria nenhuma row nova em `groups`, `group_members`, nem audit.
12. Edge case: SQLite vazio → Stage 3 emite WARN `"no migrated users; skipping stage 3"` + exit 0.
13. Edge case: grupo preexistente com `name=target AND type=target` → reaproveitado, novos members inseridos, `created_by` preservado (não sobrescrito).
14. Stage 3 reusa a mesma conexão (PgPool `max_connections=1`) dos stages 1+2 — invariante SEC-H-1 preservada.
15. Stage 3 roda em **single tx** — se audit falhar, o grupo/membership também rollback.
16. `@code-reviewer` APPROVE.
17. `@security-auditor` APPROVE ≥ 8.0/10.
18. CI 9/9 green.
19. Linear GAR-413 comentada (stage 3/10 done).

## 5. Design rationale

### 5.1 Stage 3 sempre roda — não é feature-flag

Plan 0034 §7 lista stages ordenados por FK. Stage 3 é pré-requisito para stages 5+ (chats, messages, memory) — todos eles referenciam `group_id`. Deixar Stage 3 como opt-in criaria estado inconsistente (users migrados sem grupo bucket → stages posteriores ficam sem group_id válido). Decisão: auto-run.

Caso operator queira skippar, o mecanismo futuro é `--skip groups` (slice que introduzir o flag). Até lá, operators com setup especial (ex.: multi-tenant Postgres preexistente com grupos já populados) devem evitar rodar `migrate workspace` nessa infra.

### 5.2 Lock strategy: `SELECT ... FOR UPDATE` em `groups`

Dois processos invocando o comando simultaneamente (não suportado per plan 0039 F-02, mas devemos ser resilientes) poderiam ambos detectar "grupo não existe" e tentar inserir. Mitigação:

1. Abre tx.
2. `SELECT id FROM groups WHERE name=$1 AND type=$2 FOR UPDATE` — lock da row se existir, ou zero rows (qualquer tx futura que tentar o mesmo SELECT fica bloqueada se o INSERT chegar primeiro via predicate lock).
3. Se zero rows: `INSERT INTO groups ... RETURNING id`.

Postgres NÃO tem predicate locks nativos para SELECT WHERE de row inexistente (gap de serializable isolation em `READ COMMITTED`, que é nosso default). Para fechar a janela completamente exigiria `UNIQUE (name, type)` em groups — mudança de schema que não cabe neste slice. **Decisão**: documentar a janela como known, aceitar que runs concorrentes são não suportados (plan 0039 F-02 já declara), e mover adiante. Migration 014+ pode adicionar o UNIQUE se produto pedir.

### 5.3 Owner selection por `users.created_at ASC`

Plan 0034 §7.4 já documenta a decisão. A alternative (random pick, or first in SQLite `rowid` order) foi descartada porque `created_at` é determinística + compreensível para o operator (users antigos viram owners).

Query fechada:
```sql
SELECT id FROM users
WHERE legacy_sqlite_id IS NOT NULL
ORDER BY created_at ASC
LIMIT 1;
```

### 5.4 Stage 3 auditing: 1 row "groups.imported_from_sqlite" + 1 row per membership

Audit granularidade fica entre "1 row por run" (muito pobre) e "1 row por user + 1 row por grupo" (fiel ao princípio de least-surprise em LGPD art. 18). Escolha: a granularidade por membership é barata (dezenas de rows/run) e bate diretamente com o evento real de "user X foi adicionado ao grupo Y".

**Idempotency**: `WHERE NOT EXISTS (SELECT 1 FROM audit_events WHERE action='groups.imported_from_sqlite' AND resource_id=$group_id)` espelha o padrão do Stage 1.

### 5.5 `personal` como `groups.type` default

Migration 001 comment em `groups.type`:
> `'personal'` → RESERVED for GAR-413 SQLite→PG migration tool fallback. The API layer (GAR-393) must not expose `'personal'` as a user-selectable option — owner-only, programmatic.

Este slice usa exatamente essa porta reservada. Se o operator passar `--target-group-type team` ou `--target-group-type family`, o valor é validado no DB via CHECK constraint (falha 65 / EX_DATAERR se tipo inválido).

### 5.6 `groups.created_by` = primeiro user migrado (NOT NULL)

Migration 001 linha 106: `created_by uuid NOT NULL REFERENCES users(id)`. Não podemos passar NULL. Primeira opção: primeiro user (já escolhido para owner). Segunda opção (se eventualmente removermos NOT NULL): marcar como system row. Decisão: primeiro user — congruente com ownership e respeita o constraint sem mudança de schema.

## 6. Testing strategy

### 6.1 Unit (`crates/garraia-cli/src/migrate_workspace.rs`)

- `stage3_report_default_is_zero`: struct default report = zeros.
- `stage3_merge_combines_counts`: se existirem helpers de composição de `StageReport` internos, asserir que cada stage só escreve seu próprio campo.

### 6.2 Integration (testcontainer + garraia binary)

- `stage3_creates_group_and_members_happy_path` (3 users):
  - Pós-run: 1 row em `groups` (`type='personal'`, `name='Legacy Personal Workspace'`).
  - 3 rows em `group_members`: 1 owner + 2 members.
  - Owner é o user com menor `created_at`.
  - 1 audit `groups.imported_from_sqlite` + 3 audit `group_members.imported_from_sqlite`.

- `stage3_idempotent_rerun`:
  - Roda 2×. Counts idênticos após a segunda run.

- `stage3_resolves_preexisting_group_by_name`:
  - Operator cria manualmente o grupo antes da migração (ex.: via seed script).
  - Run não cria grupo novo mas adiciona as memberships com user migrated.
  - Owner do grupo preexistente é preservado (não sobrescrito).
  - Primeiro user migrado vira `member` (não `owner`) — porque já existe owner.

- `stage3_skips_when_no_legacy_users`:
  - SQLite vazio (sem rows em `mobile_users`).
  - Exit 0. Zero rows em groups/group_members/audit.

## 7. Security review triggers

- **SEC-H same-tx audit**: groups INSERT + all group_members INSERTs + todos audit rows ficam na **mesma transação**. Falha de qualquer um rolla tudo. Regression guard: integration test assere `COUNT(audit) == COUNT(memberships) + (1 if group_created else 0)`.
- **SEC-H BYPASSRLS re-check**: a mesma conexão do stage 1 é usada (pool `max_connections=1`); a re-check já aconteceu no início do stage 1.
- **SEC-M CHECK constraint error path**: `--target-group-type invalid-value` falha o INSERT no Postgres com SQLSTATE 23514 → mapear para exit 65 (`EX_DATAERR`) com mensagem amigável "`invalid group type`".
- **SEC-M `--target-group-name` injection surface**: operator-provided, mas passa por `sqlx::query` bind ($1 param). Zero concat. Documentar em code comment.
- **SEC-L `groups.created_by` attribution**: primeiro user migrado é o `created_by` — o audit row captura quem foi (`resource_id=group_id`, metadata inclui `{created_by_user_id, owner_user_id}`).
- **SEC-L rerun safety in presence of membership soft-deletes**: se um operator tiver feito `UPDATE group_members SET status='removed'` entre runs, o segundo run NÃO reativa (ON CONFLICT DO NOTHING pula). Documentado — reactivation é operator responsibility.

## 8. Rollback plan

Revertível via `git revert` — módulo Rust puramente extendido (nova função + 2 flags no clap + 3 testes). Zero schema change.

Para um tenant que já rodou a migração incluindo Stage 3, rollback de dados:

```sql
-- Delete memberships created by this stage (identifiable by legacy_sqlite_id em users).
DELETE FROM group_members
WHERE user_id IN (SELECT id FROM users WHERE legacy_sqlite_id IS NOT NULL)
  AND group_id IN (
      SELECT id FROM groups
      WHERE name = 'Legacy Personal Workspace' AND type = 'personal'
  );

-- Delete the group itself (cascades to any remaining memberships + audit rows persist by design).
DELETE FROM groups
WHERE name = 'Legacy Personal Workspace' AND type = 'personal';
-- audit_events rows ('groups.imported_from_sqlite', 'group_members.imported_from_sqlite')
-- persist by design (plan 0034 §8, LGPD art. 37).
```

Documentado em `migrate_workspace.rs` como docstring do módulo.

## 9. Risk assessment

| Risco | Severidade | Mitigação |
|---|---|---|
| Runs concorrentes criam grupos duplicados | MÉDIO | Plan 0039 F-02 já documenta non-support de concurrent runs; Stage 3 herda. Futuro: UNIQUE(name,type) em groups. |
| `--target-group-type` inválido passa para Postgres e trava mid-tx | BAIXO | CHECK constraint dispara SQLSTATE 23514; mapeamos para exit 65 com mensagem clara. |
| User migrado com `created_at` NULL ou igual entre users | BAIXO | Migration 001 exige `DEFAULT now()`; plan 0034 §7.1 setamos `created_at` vindo do SQLite. Ties por `created_at` são quebrados pela ordem natural do LIMIT 1 (não determinística mas consistente no run). |
| Audit row falha (ex.: group_id inválido) | BAIXO | Mesma-tx rollback descarta Stage 3 inteiro; operator vê erro com SQLSTATE e pode retomar. |
| Grupo preexistente com `name` colidente mas `type` diferente | BAIXO | SELECT filtra por `(name, type)`. Grupo different-type = outro bucket; criamos o novo com target-type. Documentado em integration test. |
| Primeiro user migrado não existe em Postgres (stages 1+2 falharam silenciosamente) | CRÍTICO em tese, BAIXO em prática | Stage 3 começa com query `SELECT id FROM users WHERE legacy_sqlite_id IS NOT NULL ORDER BY created_at ASC LIMIT 1`. Zero rows → emite WARN + exit 0 sem tocar em groups/members. |
| Segundo user migrado vira owner ao invés de first | BAIXO | Owner é definido por `ORDER BY created_at ASC LIMIT 1` — determinístico. |

## 10. Open questions

- **Q1**: Deveria fazer `UPDATE groups SET updated_at = now()` quando reaproveitando grupo preexistente? → **Não** — o migrate workspace não é uma edit do grupo; reuso é "best-effort find, then just add members". Deixar `updated_at` inalterado evita sinalizar mudança semântica que não aconteceu.
- **Q2**: Deveria aceitar `--target-group-id <UUID>` como alternativa ao `--target-group-name`? → **Não** neste slice; operator conhece o nome natural do grupo mais frequentemente que UUIDs. Pode entrar num slice futuro quando houver caso de uso real.

## 11. Future work

- **Slice N+1** — stages 5–10 (chats, messages, memory, sessions, api_keys, audit retrofit).
- **Slice flag refactor** — introduzir `--only`/`--skip` + `--batch-size` quando existirem >3 stages.
- **Schema hardening** — eventual `UNIQUE (name, type)` em groups para blindar runs concorrentes.

## 12. Definition of done

- [ ] Módulo `migrate_workspace.rs` estendido com `run_stage3_groups` + `StageReport` substitui `Stage1Report`.
- [ ] CLI `main.rs` ganha `--target-group-name` + `--target-group-type`.
- [ ] Unit tests verdes.
- [ ] Integration tests verdes (4 cenários Stage 3).
- [ ] `cargo check/clippy/fmt/test` verdes workspace-wide.
- [ ] `@code-reviewer` APPROVE.
- [ ] `@security-auditor` APPROVE ≥ 8.0/10.
- [ ] PR aberto.
- [ ] CI 9/9 green.
- [ ] PR merged.
- [ ] Linear GAR-413 comentada (Stage 3 done, 3/10).
- [ ] `CLAUDE.md` + `plans/README.md` atualizados.
- [ ] `.garra-estado.md` atualizado ao fim da sessão.
