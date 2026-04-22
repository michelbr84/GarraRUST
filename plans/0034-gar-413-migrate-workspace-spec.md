# Plan 0034 — GAR-413: `garraia-cli migrate workspace` spec (SQLite → Postgres)

**Status:** Em execução (spec-only)
**Autor:** Claude Opus 4.7 (sessão autônoma, ClaudeMaxPower + Superpowers)
**Data:** 2026-04-21 (America/New_York)
**Issues:** GAR-413
**Branch:** `feat/0034-gar-413-migrate-workspace-spec`
**Pré-requisitos:** ADR 0003 (aceito), ADR 0005 (aceito), migrations 001..013 (aplicadas). Slot 003 de migration (GAR-387 / plan 0033) entregue.
**Unlocks:** implementação real do subcommand em slice futuro (0034.impl) e sunset do `garraia-db` SQLite como fonte-primária (mantido como fallback single-user per ADR 0003).

---

## 1. Goal

Formalizar, **em detalhe normativo**, como `garraia-cli migrate workspace` transporta dados de `garraia-db` (SQLite) para `garraia-workspace` (Postgres 16), incluindo a rota especial `mobile_users → users + user_identities` do ADR 0005 §Migration strategy. O output é um plano que a implementação real (slice seguinte) copia passo-a-passo: linha de comando, pré-condições, steps transacionais, reversão parcial, exit codes, observabilidade, e surface de teste/review. Zero código Rust neste slice.

O objetivo é encerrar a ambiguidade entre ADR 0003 §Migration ("Tool: garraia-cli migrate workspace") e ADR 0005 §Migration strategy ("mobile_users → user_identities") — hoje cada ADR descreve metade do algoritmo. Este plan é a fonte única de verdade para a implementação.

## 2. Non-goals

- **Não implementa** nenhuma linha Rust. O subcommand `garraia migrate openclaw` (legacy para OpenClaw) continua em `crates/garraia-cli/src/migrate.rs` sem alteração.
- **Não toca** em `garraia-db` SQLite schema — é leitura read-only.
- **Não cria** migration Postgres nova — usa schema já fechado em migrations 001..013 (+ 003 deste ciclo).
- **Não decreta** sunset automático do SQLite. ADR 0003 explícito: `garraia-db` permanece como historical record e CLI single-user fallback.
- **Não entrega** reconciliação bidirecional (dual-write) — rejeitado pela ADR 0003 §Migration §3.
- **Não cobre** import de `chat_sync` do SQLite (tabela legacy de channel sync; fica para dev decidir depois se vale a pena rescatar).
- **Não força** password reset em usuários migrados — ADR 0005 §PBKDF2 → Argon2id transition explícito: lazy upgrade no próximo login.
- **Não emite** e-mails nem notifica usuários da migração — escopo puramente técnico.
- **Não garante** idempotência total em todas as tabelas do SQLite (sessions/messages históricas podem duplicar em re-run); idempotência é garantida apenas para `users` + `user_identities` (via UNIQUE em `email`).

## 3. Scope

**Arquivos novos:**

- `plans/0034-gar-413-migrate-workspace-spec.md` (este).
- (Follow-up slice — não neste PR) `crates/garraia-cli/src/migrate_workspace.rs` com o código.

**Arquivos atualizados:**

- `plans/README.md` — entrada 0034.
- `docs/adr/0003-database-for-workspace.md` — amendment "linkar para plan 0034 como spec normativa" (opcional; pode ser deferido).
- `docs/adr/0005-identity-provider.md` — amendment "linkar para plan 0034 como spec normativa do §Migration strategy" (opcional).

Zero código. Zero schema change. Zero dependency change.

## 4. Acceptance criteria

1. O plan descreve a **linha de comando completa** com todas as flags, defaults, exit codes e error paths.
2. O plan descreve o **algoritmo de migração ponto-a-ponto** em pseudo-código (não Rust), cobrindo:
   - Pré-flight checks (Postgres reachable, schema version ≥ expected, SQLite file readable).
   - Ordem de import (por FK: users → user_identities → groups → group_members → chats → messages → memory_items → memory_embeddings → sessions → api_keys → audit_events).
   - Transação única ou por-tabela? (decidido abaixo).
   - Dry-run semantics.
   - Idempotency strategy (UNIQUE collisions → UPSERT para users/user_identities; skip para sessions históricas).
   - Rollback / retry.
3. O plan descreve o **mapeamento SQLite → Postgres por tabela** (column-level).
4. O plan cobre a **rota `mobile_users` → `users + user_identities`** replicando fielmente ADR 0005 §Migration strategy + adiciona invariantes de segurança.
5. O plan especifica **o que NÃO é migrado** (e por quê).
6. O plan especifica a **surface de testes** que a implementação deve exercer (unit, integration, end-to-end).
7. O plan especifica os **security review triggers**.
8. O plan especifica os **exit codes** (0, 1, 2, 64..78 EX_*) e a **observabilidade** (tracing spans + audit_events).
9. Review: `@code-reviewer` APPROVE + `@security-auditor` APPROVE (plan-level).
10. CI green (doc-only PR — o único gate é format/markdown visual).

## 5. Command surface

### 5.1 Invocação

```
garraia migrate workspace --from-sqlite <PATH> --to-postgres <URL> [OPTIONS]
```

### 5.2 Flags

| Flag | Default | Semantics |
|---|---|---|
| `--from-sqlite <PATH>` | (obrigatório) | Caminho do arquivo SQLite (tipicamente `~/.local/share/garraia/garraia.db` no Linux, `%APPDATA%\GarraIA\garraia.db` no Windows). |
| `--to-postgres <URL>` | (obrigatório) | `postgres://user:pass@host:port/db` — DEVE ter role com `BYPASSRLS` ou ser superuser (migração grava em tenant-scoped tables). |
| `--dry-run` | `false` | Roda todas as queries como `EXPLAIN`/count, gera report, NÃO INSERTa. |
| `--only <stages>` | (todos) | Lista comma-separated de stages: `users,identities,groups,chats,messages,memory,sessions,api_keys,audit`. Útil para retomar após falha. |
| `--skip <stages>` | (nenhum) | Inverso de `--only`. |
| `--target-group-name <STR>` | `Legacy Personal Workspace` | Nome do grupo-bucket que recebe mensagens/memory legadas (SQLite single-user → grupo único em Postgres). |
| `--target-group-type <STR>` | `personal` | Type do grupo criado; validado contra CHECK de `groups.type`. |
| `--batch-size <N>` | `500` | Rows por transação de INSERT em tabelas de histórico (messages, memory_items, audit_events). Aplicado também aos stages `users` + `identities` quando `count(mobile_users) > 5000` (safety threshold — security audit SEC-M-1). |
| `--verbose` | `false` | Log de cada row importada (debug). |
| `--confirm-backup` | `false` | **Requerido** em Postgres non-empty. Nome alinhado com ADR 0003 §Migration Step 1 (evidência de que o operador tem backup SQLite antes de correr). Fail-closed anti-foot-gun. Alias: `--confirm` aceita mas emite WARN de deprecation. |

### 5.3 Exit codes

| Code | Meaning |
|---|---|
| `0` | Success — migração completa (ou dry-run OK). |
| `1` | Generic error (e.g., pânico). |
| `2` | Invalid CLI usage (missing required flag, bad URL shape). |
| `64` (EX_USAGE) | Pre-flight check falhou: schema version mismatch, missing tables, bad auth. |
| `65` (EX_DATAERR) | SQLite corrompido ou com row inválida (viola constraint de destination). |
| `67` (EX_NOUSER) | `--to-postgres` user não tem BYPASSRLS / superuser. |
| `74` (EX_IOERR) | I/O error lendo SQLite ou escrevendo Postgres. |
| `75` (EX_TEMPFAIL) | Postgres connection perdida mid-transação. Retry safe. |
| `78` (EX_CONFIG) | `--confirm` ausente em Postgres non-empty, OU conflito de dados que requer intervenção manual. |

### 5.4 Auditoria de invocação

Toda execução (dry-run inclusive) emite:
- **tracing span** `migrate_workspace.run` com fields `{sqlite_path (skip), postgres_url (skip), dry_run, stages, start_at, end_at, rows_imported_total}`.
- **audit_events** row (após pré-flight OK) com:
  - `actor_user_id = NULL`
  - `actor_label = 'system.migrate_workspace'`
  - `action = 'system.migrate_workspace.started' | '.completed' | '.failed'`
  - `metadata = {"cli_version": "...", "stages": [...], "dry_run": bool, "rows": {...}}`

## 6. Pré-flight checks

Executados sequencialmente antes de qualquer INSERT. Falha em qualquer check → abort com exit code 64.

1. **SQLite file**: existe, é readable, é um SQLite 3.x (magic bytes) e abre com o schema esperado do `garraia-db` (tabelas `mobile_users`, `sessions`, `messages`, `memory_items`, `chat_sync`, `conversations` etc. que existem hoje no crate). Check soft: tabelas que NÃO existem no SQLite são skippadas silenciosamente (caso SQLite tenha versão antiga).
2. **Postgres URL**: conecta, `SELECT 1` responde.
3. **Permissão**: `SELECT rolbypassrls, rolsuper FROM pg_roles WHERE rolname = current_user`.
   - **(true, _)** OU **(_, true)** → OK.
   - Zero rows retornadas (role não existe no catálogo) → exit 67 (fail-closed anti-confusion).
   - `(false, false)` → exit 67.
   - **Invariante anti-race-condition (security audit SEC-H-1):** o check DEVE rodar na **mesma conexão** usada pelas queries de INSERT, e **DEVE ser re-executado** dentro da primeira transação de dados (stage 7.1) via `SELECT pg_has_role(current_user, 'pg_signal_backend', 'USAGE') OR rolsuper` — prevê a janela onde um DBA revoga BYPASSRLS entre T0 (pre-flight) e T1 (primeiro INSERT). A ferramenta NÃO usa pool; mantém **uma conexão exclusiva** para o run inteiro.
   - `--dry-run` não requer BYPASSRLS (só SELECTs e EXPLAIN rodam). Contagens podem vir menores que o real sob RLS — `--dry-run` emite WARN nesse caso (security audit SEC-L-1).
4. **Schema version**: `_sqlx_migrations` contém pelo menos `{001, 002, 003, 004, 005, 006, 007, 008, 009, 010, 011, 012, 013}` (conferir dinamicamente contra a lista compilada do binário). Migration faltando → exit 64.
5. **Destination emptiness**: se `SELECT count(*) FROM users WHERE legacy_sqlite_id IS NOT NULL` > 0 AND nenhuma combinação de `--only`/`--skip` foi passada → migração JÁ rodou antes; exit 78 orienta o operador a usar `--only <stage>` para resume. Com `--only` ou `--skip` explícito, o check é **skip**ado — permite retomar stages parciais (code review item 6).
6. **Confirmation gate**: se `SELECT count(*) FROM users` > 0 AND `--confirm-backup` ausente → exit 78. (`--dry-run` bypassa este gate.)
7. **Target group resolution**: verifica se existe `groups` com `name = --target-group-name AND type = --target-group-type`. Se não existir, será criado no stage 3 (`groups`). Se existir, captura `group_id` para reuso.

## 7. Stages (execution order)

### Convenção de transações

- Stages que tocam dados canônicos (users, user_identities, groups, group_members): **uma transação por stage**, commit ao final. Rollback anula o stage inteiro.
- Stages que tocam dados históricos (messages, memory_items, audit_events legacy): **batched**, `--batch-size` rows por transação. Falha mid-batch reporta `rows_imported_so_far` e aborta; retomada via `--only <stage>`.
- Audit row de início/fim é INSERT fora das transações de dados (não pode ser rollback junto com falha de dados).

### 7.1 Stage `users` (single tx)

Replica ADR 0005 §Migration strategy §644-656 — bullet points 1 e 2.

```
BEGIN;
FOR EACH row IN sqlite.mobile_users:
    new_user_id = uuid_v7();
    INSERT INTO pg.users
        (id, email, display_name, status, legacy_sqlite_id, created_at, updated_at)
    VALUES
        (new_user_id,
         LOWER(row.email),                               -- citext column
         SPLIT_PART(row.email, '@', 1),                  -- best-effort display_name
         'active',
         row.id::text,                                   -- audit bridge
         row.created_at,
         row.created_at)
    ON CONFLICT (email) DO UPDATE SET
        legacy_sqlite_id = COALESCE(users.legacy_sqlite_id, EXCLUDED.legacy_sqlite_id);
    -- ON CONFLICT preserves an already-migrated row; idempotency.
COMMIT;
```

**Invariantes:**
- `users.legacy_sqlite_id` é populado SOMENTE para rows vindas do SQLite — users criados via signup (pós-migração) deixam NULL.
- Conflitos por email (pré-existência de signup com mesmo email) → UPSERT seta `legacy_sqlite_id` mas NÃO sobrescreve outros campos. Audit log flagrante.

### 7.2 Stage `identities` (single tx, com audit atômico inline)

Replica ADR 0005 §Migration strategy bullet 3 — **com correção crítica (security audit SEC-H-3)**:

**IMPORTANTE — Legacy hash format**:

Verificação empírica em `crates/garraia-gateway/src/mobile_auth.rs:300-316` confirmou que o `mobile_users` SQLite **não armazena PHC string**. As colunas são:
- `password_hash` — raw PBKDF2-HMAC-SHA256 digest (32 bytes), base64 **STANDARD** (com padding `=`).
- `salt` — raw salt (32 bytes, per `SALT_LEN`), base64 **STANDARD** (com padding).
- Iterações: `600_000` (constante `PBKDF2_ITERATIONS`).

Copiar `password_hash` verbatim para `user_identities.password_hash` torna o hash **inutilizável** pelo `garraia-auth::verify_pbkdf2` (que exige PHC string). Resultado: usuários migrados nunca conseguiriam logar — exatamente o cenário que ADR 0005 rejeita (§Migration bullet 1 "hash format is preserved").

**Correção obrigatória — reassembly em PHC format antes do INSERT:**

```
FOR EACH row IN sqlite.mobile_users:
    // Decode legacy base64-STANDARD (com padding).
    raw_hash_bytes = base64_STANDARD.decode(row.password_hash)  // 32 bytes
    raw_salt_bytes = base64_STANDARD.decode(row.salt)           // 32 bytes
    // Re-encode em base64-STANDARD-NO-PAD (formato PHC).
    hash_nopad = base64_STANDARD_NO_PAD.encode(raw_hash_bytes)  // strip `=`
    salt_nopad = base64_STANDARD_NO_PAD.encode(raw_salt_bytes)
    phc_string = format!(
        "$pbkdf2-sha256$i=600000,l=32${salt_nopad}${hash_nopad}"
    )
    // phc_string é o que vai para user_identities.password_hash.
```

Este é o formato aceito por `PasswordHash::new()` do crate `password-hash` (verificado em `crates/garraia-auth/src/hashing.rs:80-92` e no test fixture linha 155).

**Stage execution (security audit SEC-H-2 fix — audit atômico com identity):**

```
BEGIN;  -- SINGLE TX wrapping identity + audit for each user
FOR EACH row IN sqlite.mobile_users:
    SELECT id INTO user_uuid FROM pg.users
        WHERE legacy_sqlite_id = row.id::text;
    IF NOT FOUND: abort stage (stage 7.1 was skipped).

    phc_string = reassemble_phc(row.password_hash, row.salt);

    INSERT INTO pg.user_identities
        (id, user_id, provider, provider_sub, password_hash, created_at, hash_upgraded_at)
    VALUES
        (uuid_v7(),
         user_uuid,
         'internal',
         user_uuid::text,
         phc_string,                                     -- PHC reassembled; ready for lazy upgrade
         row.created_at,
         NULL)                                           -- lazy upgrade: NULL = never upgraded
    ON CONFLICT (provider, provider_sub) DO NOTHING;

    -- Audit row MUST be inserted in same tx (LGPD art. 18 — ADR 0005 §691).
    INSERT INTO pg.audit_events
        (group_id, actor_user_id, actor_label, action, resource_type, resource_id, metadata)
    SELECT NULL, NULL, 'system.migrate_workspace',
           'users.imported_from_sqlite', 'user', user_uuid::text,
           jsonb_build_object(
               'source', 'mobile_users',
               'legacy_id', row.id,
               'hash_algorithm', 'pbkdf2-sha256',
               'iterations', 600000,
               'lazy_upgrade_pending', true)
    WHERE NOT EXISTS (
        SELECT 1 FROM pg.audit_events
        WHERE action = 'users.imported_from_sqlite'
          AND resource_id = user_uuid::text
    );  -- Idempotency guard (code review item 2 / SEC-L-3) — sem WHERE NOT EXISTS
        -- o ON CONFLICT seria no-op por não haver unique index semântico.
COMMIT;
```

**Invariantes:**

- **PHC reassembly é obrigatório** — ver caixa acima. Implementação DEVE escrever teste unit cobrindo round-trip: `(legacy_hash, legacy_salt) → PHC → verify_pbkdf2(PHC, original_password) == true` com fixture real de `mobile_auth.rs::hash_password`.
- **Audit é atômico com identity**. Falha de audit → rollback da identity → user fica em Postgres mas sem credential → próxima execução retoma via ON CONFLICT DO NOTHING (identity) + WHERE NOT EXISTS (audit). Zero estado inconsistente.
- `hash_upgraded_at IS NULL` para todos. Lazy upgrade no primeiro login (ADR 0005 §PBKDF2 → Argon2id transition).
- ON CONFLICT DO NOTHING em `(provider, provider_sub)` é a chave natural — re-runs não duplicam identity.
- `WHERE NOT EXISTS` em audit substitui o `ON CONFLICT DO NOTHING` anterior (que era no-op, ver Code review item 2).
- Coluna `hash_upgraded_at` foi adicionada pela migration 009 (plan 0011.5), DEPOIS da redação da ADR 0005 — a ausência no INSERT pseudo-code da ADR é histórica, não lacuna.

### 7.3 Stage `audit` (migrado para dentro de §7.2)

**Security audit SEC-H-2 reescopou este stage:** audit row não é mais um stage separado — foi **inlineado dentro da transação do stage 7.2** (identities). Razão: LGPD art. 18 exige trail para cada user migrado; "users migrados + audit best-effort" (design anterior) cria estado inconsistente onde um user existe em Postgres sem registro de origem. A solução correta é **atomicidade**: identity e audit fazem commit juntos, ou nenhum dos dois.

Este stage continua existindo no CLI como nome `--only audit` por compat, mas o comportamento é no-op (todo audit já foi escrito pelo stage 7.2). `--only identities` sozinho JÁ cobre a invariante.

### 7.4 Stage `groups` (single tx)

Resolve o `--target-group-name`.

```
BEGIN;
SELECT id INTO legacy_group_id FROM pg.groups
    WHERE name = :target_group_name AND type = :target_group_type;
IF NOT FOUND:
    -- First user created becomes owner.
    SELECT id INTO first_user_id FROM pg.users
        WHERE legacy_sqlite_id IS NOT NULL
        ORDER BY created_at ASC LIMIT 1;
    INSERT INTO pg.groups (id, name, type, created_by, ...) VALUES (...)
        RETURNING id INTO legacy_group_id;
    INSERT INTO pg.group_members (group_id, user_id, role, status)
        VALUES (legacy_group_id, first_user_id, 'owner', 'active');
END IF;
-- All other migrated users become 'member' (fail-open idempotent).
FOR each migrated_user:
    INSERT INTO pg.group_members (group_id, user_id, role, status)
        VALUES (legacy_group_id, migrated_user.id, 'member', 'active')
        ON CONFLICT (group_id, user_id) DO NOTHING;
COMMIT;
```

**Decisão:** primeiro user migrado (ordenado por `created_at ASC`) vira `owner`; demais viram `member`. Se a instalação tiver `mobile_users.role` indicador (não tem hoje), pode mudar no futuro. ADR 0003 §Migration é silent sobre ownership transfer — esta é a decisão deste plan.

### 7.5 Stage `chats` (single tx)

Mapping: SQLite `conversations` → Postgres `chats` (se tabela `conversations` existir no SQLite; caso contrário skip).

```
FOR EACH conv IN sqlite.conversations:
    INSERT INTO pg.chats
        (id, group_id, type, name, created_by, created_at)
    VALUES
        (uuid_v7(),
         legacy_group_id,
         'channel',                         -- legacy SQLite had no channel/dm distinction
         conv.title OR 'Untitled chat',
         first_migrated_user_id,
         conv.created_at)
    ON CONFLICT DO NOTHING;
    -- Save mapping conv.id → new chat_id for stage 7.6.
```

**Invariante:** `chat_members` são populados apenas para `first_user`; demais users do grupo ganham ACL view via `group_members` + a policy `chats_group_isolation` (migration 007).

### 7.6 Stage `messages` (batched, `--batch-size` rows/tx)

Replica cada SQLite message para Postgres com `group_id` denormalizado + compound FK preservation.

**Regra de attribution (code review item 4):** `messages.sender_user_id` é `NOT NULL REFERENCES users(id)` (migration 004 linha 61). Assistant messages não podem ser atribuídas ao `first_migrated_user_id` sem corromper semântica (atribuir turnos de IA a um humano). Tratamento por role:

| SQLite `role` | Ação |
|---|---|
| `user` (ou ausente/NULL) | INSERT com `sender_user_id = first_migrated_user_id` + `sender_label = first_migrated_user.display_name`. |
| `assistant` | **SKIP** (não importa). Registrar count em `migration_report.messages_skipped_assistant`. Justificativa: sender só pode ser `users.id`; criar "synthetic system user" adiciona uma identidade sintética que pollui listas de membros e cross-tenant queries. Mensagens históricas da IA NÃO são authoritative — o valor delas é limitado. |
| `system` / outro | SKIP, incrementa `messages_skipped_unsupported_role`. |

```
FOR EACH batch OF --batch-size rows IN sqlite.messages:
    BEGIN;
    FOR EACH row IN batch:
        IF row.role IN ('assistant', 'system') OR row.role NOT IN (NULL, '', 'user'):
            report.messages_skipped_* += 1;
            CONTINUE;  -- skip, do not INSERT
        SELECT chat_id_new FROM mapping WHERE chat_id_old = row.conversation_id;
        INSERT INTO pg.messages
            (id, chat_id, group_id, sender_user_id, sender_label, body, created_at, deleted_at)
        VALUES
            (uuid_v7(),
             chat_id_new,
             legacy_group_id,
             first_migrated_user_id,
             first_migrated_user.display_name,
             row.content,
             row.created_at,
             NULL)
        ON CONFLICT DO NOTHING;
    COMMIT;
```

**Invariantes:** FTS index `body_tsv` é populado pelo GENERATED column automaticamente. Caller DEVE emitir `migration_report` com os counts (`messages_imported`, `messages_skipped_assistant`, `messages_skipped_unsupported_role`) — audit trail para auditores descobrirem porque X mensagens do SQLite não apareceram no Postgres.

### 7.7 Stage `memory` (batched)

**Divergência deliberada de ADR 0003 §Migration (code review item 8):**

ADR 0003 linha 261 sugere `memory_facts → memory_items with scope_type = 'user'` (single-owner). O plan escolhe `scope_type = 'group'` porque o destino Postgres é multi-usuário (o legacy single-user já foi promovido a um grupo), e manter memórias como user-scope as isola dos outros members do mesmo grupo — o que contraria o propósito do grupo bucket. Decisão registrada aqui, não silenciada.

Operator que prefira preservar user-scope pode usar `--only users,identities,groups,chats,messages` (skip memory) e migrar manualmente depois com políticas customizadas.

```
FOR EACH batch IN sqlite.memory_items:
    BEGIN;
    FOR EACH row IN batch:
        INSERT INTO pg.memory_items
            (id, scope_type, scope_id, group_id, created_by, created_by_label,
             kind, content, sensitivity, created_at)
        VALUES
            (uuid_v7(),
             'group',                        -- divergência intencional de ADR 0003
             legacy_group_id::text,
             legacy_group_id,
             first_migrated_user_id,
             first_migrated_user.display_name,
             row.kind OR 'note',
             row.content,
             row.sensitivity OR 'normal',
             row.created_at)
        ON CONFLICT DO NOTHING;
    COMMIT;
```

**Non-goal:** embeddings legacy NÃO são migrados — dimensão/modelo pode diferir. Se `sqlite.memory_embeddings` existir, é skippado com WARN. Embeddings novos serão computados lazy quando o embeddings provider estabilizar (GAR-372).

### 7.8 Stage `sessions` (single tx, best-effort)

```
-- Sessions são ephemeral. Migração para histórico, não para re-auth.
INSERT INTO pg.sessions (id, user_id, token_hash, created_at, last_seen_at, expires_at)
SELECT ... FROM sqlite.sessions ...
ON CONFLICT DO NOTHING;
```

**Decisão:** sessões legadas NÃO preservam `token_hash` válido (o cliente mobile vai precisar re-login depois da migração). Mas preservar o row para audit histórico é barato. Se preferir skip total, `--skip sessions`.

### 7.9 Stage `api_keys` (single tx)

Idêntico a sessions — `key_hash` legacy é provavelmente inválido em Postgres schema (formato mudou). Skip por padrão; `--only api_keys` força import com aviso.

### 7.10 Stage `audit` (retrofit — outros eventos não-user)

Se SQLite tiver `audit_log` (algumas versões legacy têm), replicar como `audit_events` com `action` prefixado `legacy.<original_action>`. Se não tiver, stage é no-op.

## 8. Rollback / retry

Por decisão arquitetural (ADR 0003 §Migration §"Rollback"), a fonte SQLite **nunca** é modificada. Rollback real é:

```
DELETE FROM users WHERE legacy_sqlite_id IS NOT NULL;
-- Cascades to user_identities, group_members (via FK CASCADE).
-- Chats/messages/memory permanecem associados ao legacy_group_id mas sem created_by
--    (ON DELETE SET NULL). Limpeza completa requer:
DELETE FROM groups WHERE name = '--target-group-name' AND type = '--target-group-type';
-- Cascades to chats, messages, memory (mas NÃO audit_events — ver abaixo).
```

**Rows órfãos por design (security audit SEC-L-2):** `audit_events` NÃO tem FK para `users` nem para `groups` — é append-only e imutável por desenho (migration 002 linha ~audit_events usa `actor_user_id uuid` plain, sem REFERENCES). Portanto:

- O audit row `action = 'users.imported_from_sqlite'` **persiste** mesmo após o rollback. Isso é intencional e compliance-correto: a trilha "migração foi tentada e revertida" é informação que deve permanecer (LGPD art. 37 — tratamento de dados pessoais deve ter registro; GDPR art. 30 — records of processing).
- Reviewers que auditem o Postgres pós-rollback verão um "órfão" semântico. A única ação aceitável é deixá-lo; deletar audit rows por conveniência operacional é violação de compliance.

Esta receita fica documentada em `docs/compliance/data-erasure.md` (futuro GAR-400) mas é citada aqui para completude.

**Retry parcial** via `--only <stage>`: se stage 7.6 falhou em row 4,200 de 10,000, `--only messages` retoma do início do stage. Custo: O(`rows_already_imported`) em `ON CONFLICT DO NOTHING` — aceitável em re-run raro. Risk table §14 registra como mitigado por idempotência, não por resume-from-offset (que exigiria state externa).

## 9. Surface de testes (para a implementação slice)

### 9.1 Unit

- `detect_garraia_db_schema(path)` — identifica versão do SQLite schema (heurística via `SELECT sql FROM sqlite_master`).
- `map_mobile_user_to_users_row(mobile_user)` — transformação determinística.
- `resolve_legacy_group(pg, name, type)` — create-or-get.

### 9.2 Integration (testcontainers Postgres + sqlite tempfile)

- `migrate_empty_sqlite_no_op` — SQLite vazio → migração zera rows inseridos.
- `migrate_single_user_happy_path` — 1 mobile_user → 1 users + 1 user_identities + 1 audit_events + 1 groups + 1 group_members.
- `migrate_idempotent_rerun` — roda 2x, segunda rodada inserta zero rows novos (tudo ON CONFLICT).
- `migrate_with_preexisting_signup` — user já criado via signup, `legacy_sqlite_id` setado no UPSERT mas outros campos preservados.
- `migrate_dry_run_no_writes` — `--dry-run` → zero rows em qualquer tabela Postgres após execução.
- `migrate_missing_schema_exits_64` — aponta para banco sem migrations aplicadas.
- `migrate_non_bypassrls_user_exits_67` — usuário sem BYPASSRLS.
- `migrate_missing_confirm_on_non_empty_exits_78`.
- `migrate_only_stage_users_skips_rest` — flag `--only users`.
- `migrate_batch_size_respected` — mensagens 2,500 com `--batch-size=500` → 5 transações.
- `migrate_audit_events_count_matches_imports` — para cada user importado, exatamente 1 `users.imported_from_sqlite` row.

### 9.3 End-to-end

- `migrate_then_login_succeeds_with_pbkdf2_hash` — migrar user → bater login com mesma senha → succeed → `user_identities.hash_upgraded_at` preenchido (Argon2id agora).
- `migrate_then_login_fails_with_wrong_password` — 401, hash não muda.

## 10. Security review triggers

Implementação DEVE requerer `@security-auditor` review para:

1. **Role assumption / race** — conexão Postgres usa role BYPASSRLS/superuser, check em §6.3 roda na **mesma conexão** dos INSERTs e é **re-executado** dentro da first data tx (anti-race). Zero rows de `pg_roles` → exit 67. `--dry-run` dispensa BYPASSRLS.
2. **Password hash handling** — ADR 0005 §Anti-patterns: nunca logar `password_hash`, nunca retornar em output, nunca copiar para arquivo temporário. `#[instrument(skip(hash, salt, phc))]` em toda função que toque esses campos.
3. **PHC reassembly** — §7.2 exige round-trip test obrigatório em CI (SEC-H-3). Sem test green, implementação não merge.
4. **Atomicidade identity + audit** — §7.2 executa identity INSERT + audit INSERT na mesma transação. Falha de qualquer INSERT rollbackeia ambos (SEC-H-2, LGPD art. 18).
5. **PII in audit metadata** — o campo `jsonb_build_object(...)` do audit event NÃO pode conter email, display_name ou qualquer PII. Apenas `legacy_id` + algoritmo + iterations + flags.
6. **Connection string exposure** — `--to-postgres` nunca é logado plano; redacted writer aplicado. Tracing span marca como skip.
7. **SQLite lock** — a migração abre SQLite em `SQLITE_OPEN_READONLY` para evitar dirty write em caso de concurrent CLI.
8. **Audit immutability** — `audit_events` INSERTs deste stage usam `WHERE NOT EXISTS` idempotency (não `ON CONFLICT`, que seria no-op sem unique idx). NUNCA `DO UPDATE`. Audit é append-only por design.
9. **Exit code leakage** — `EX_DATAERR`/`EX_CONFIG` não vazam detalhe PII no stderr (apenas row IDs legacy, não emails).

## 11. Work breakdown (plan-only; implementação é slice futuro)

| Task | Arquivo | Estimativa | Reviewer |
|---|---|---|---|
| T1 | Este plan (0034) — redação | 50 min | code-reviewer + security-auditor |
| T2 | `plans/README.md` entrada | 3 min | — |
| T3 | Commit + PR plan-only | 5 min | code-reviewer |
| T4 | CI (doc-only — format/markdown) | 3 min | — |

**Follow-up slice (0034.impl) — NÃO parte deste PR:**

| Task | Arquivo | Estimativa |
|---|---|---|
| I1 | `crates/garraia-cli/src/migrate_workspace.rs` — skeleton + clap + subcomando `Workspace { ... }` adicionado ao enum `Commands` em `main.rs` (code review item 5) | 40 min |
| I2 | Pré-flight checks (6.1..6.7), incluindo invariante de conexão única + re-check em first data tx (SEC-H-1) | 50 min |
| I3 | PHC reassembly helper (SEC-H-3) com teste unit de round-trip contra `mobile_auth::hash_password` | 40 min |
| I4 | Stages 7.1–7.2 (users + identities com audit atômico inline, SEC-H-2) | 60 min |
| I5 | Stages 7.4–7.5 (groups + chats) | 50 min |
| I6 | Stages 7.6–7.10 (history batched, role-aware skip em messages) | 90 min |
| I7 | Integration tests 9.2 (11 cenários) + CI workflow adicionando job para `garraia-cli` + Postgres testcontainer (code review item 5) | 120 min |
| I8 | End-to-end tests 9.3 (login pós-migração, wrong-password, lazy upgrade) | 50 min |
| I9 | Review pass + fix findings | 60 min |

## 12. Verification (para este PR, plan-only)

- [ ] Plan cobre TODAS as seções obrigatórias do plans/README convention.
- [ ] Linkagem cruzada com ADR 0003 + ADR 0005 verificada.
- [ ] Exit codes seguem convenção Unix (sysexits.h) — 64/65/67/74/75/78 mapeados.
- [ ] Zero código, zero dependência Rust, zero schema change.
- [ ] `@code-reviewer` APPROVE.
- [ ] `@security-auditor` APPROVE (trigger da regra 8 do CLAUDE.md).

## 13. Rollback plan

100% reversível: revert do commit. Zero estado fora do plan markdown.

## 14. Risk assessment

| Risco | Likelihood | Impact | Mitigação |
|---|---|---|---|
| PHC reassembly code do §7.2 produz string rejeitada por `password-hash::PasswordHash::new` | Baixa | **Alto** (usuários não conseguem logar) | Teste unit de round-trip obrigatório na slice de implementação (I3): `verify_pbkdf2(reassemble_phc(hash, salt), original_password) == true` com fixture produzida por `mobile_auth::hash_password`. Zero code merge sem o test green. |
| Race condition entre pré-flight §6.3 e primeira tx de dados (DBA revoga BYPASSRLS mid-run) | Baixa | Alto | §6.3 exige conexão única + re-check em first data tx (SEC-H-1). |
| Audit insert falha mid-identity tx, deixando user sem trail | Baixa | Alto (LGPD art. 18 violation) | §7.2 inlineou audit na mesma tx de identity (SEC-H-2). Falha de qualquer uma rollbackeia ambas. |
| Stage `users` em single tx estoura `statement_timeout` com dataset grande (>5k rows) | Média | Médio | `--batch-size` aplicado automaticamente a `users` + `identities` quando `count > 5000` (SEC-M-1). |
| Mensagens de IA atribuídas ao primeiro user humano → contaminação de audit trail | Alta (sem a regra de role) | Médio | §7.6 explicitamente skip de assistant/system/other roles, count em `migration_report`. |
| Operator usa `--only memory` após alterar `memory_items.scope_type` manualmente | Baixa | Médio | §7.7 documenta divergência de ADR 0003; implementação DEVE logar WARN se detectar rows com `scope_type = 'user'` pre-existentes no target (seria evidência de patch manual). |
| Spec divergir de ADR 0005 §Migration strategy | Baixa | Alto | §7.1–7.2 replica textualmente + amendment inline sobre `hash_upgraded_at` (post-ADR). Review agent pattern-match linha por linha. |
| Mudança futura no schema Postgres invalida pré-flight §6.4 | Alta | Médio | Lista dinâmica — `_sqlx_migrations` é verificado contra a lista embarcada do binário, não contra uma constante hardcoded no plan. |
| Exit codes 78 (EX_CONFIG) colide com outro subcomando do CLI | Baixa | Baixo | `garraia-cli` hoje só usa 0/1; sysexits padrão é safe. |
| Implementação aplica `DO UPDATE` em `audit_events` por engano | Baixa | Alto (audit violation) | §10 item 6 veda; review pattern-match. |
| User migra SQLite com mobile_users vazio → stage 7.4 não tem `first_user` para owner | Média | Baixo | Pré-flight opcional: se `count(mobile_users) = 0`, skip 7.4 com WARN. |
| Email collision no ON CONFLICT do stage 7.1 sobrescreve display_name de user signup | Baixa | Médio | §7.1 explícito: `ON CONFLICT DO UPDATE SET legacy_sqlite_id = COALESCE(...)` — NUNCA sobrescreve outras colunas. |
| Crash mid-batch em §7.6 deixa estado inconsistente entre batches | Baixa | Baixo | Retry via `--only messages` + `ON CONFLICT DO NOTHING` é idempotente. Custo: O(rows_já_importadas). |
| Rollback apaga users mas audit rows persistem como órfãos | Confirmado (design) | Baixo | §8 documenta explicitamente (SEC-L-2); é compliance-correto. |

## 15. Changelog notes

`.garra-estado.md` deve ganhar entrada:
- Plan 0034 merged — spec normativa de `garraia-cli migrate workspace` documentada.
- GAR-413 permanece **aberto** (este PR é spec-only; implementação é follow-up).
- Destrava a implementação do subcommand em slice futuro.

Linear:
- GAR-413 status mantém "In Progress" ou similar; comentário linkando a este plan.

## 16. Open questions

1. **Dual-writeback para SQLite durante transição?** — ADR 0003 rejeita. Mantido como non-goal.
2. **Password reset forçado para mobile_users cujo hash vier corrompido?** — out of scope; assumimos SQLite válido. Hash corrupto falha lazy upgrade no login real (usuário precisa reset).
3. **Migration de OpenClaw data?** — separate subcommand `garraia migrate openclaw` já existe em `crates/garraia-cli/src/migrate.rs`; não mexemos.
4. **Post-migration, cleanup do SQLite?** — out of scope do GAR-413. Potencial GAR futuro "sunset SQLite" quando Postgres for único path.

Nenhuma bloqueia este PR spec-only.
