# Plan 0033 — GAR-387: Migration 003 — files, folders, file_versions

**Status:** Em execução
**Autor:** Claude Opus 4.7 (sessão autônoma, ClaudeMaxPower + Superpowers)
**Data:** 2026-04-21 (America/New_York)
**Issues:** GAR-387
**Branch:** `feat/0033-gar-387-migration-003-files`
**Unblocks:** GAR-394 (`garraia-storage`), GAR-395 (tus), `message_attachments` / `task_attachments` (deferred migrations)

---

## 1. Goal

Materializar a migração **003** (slot reservado desde migration 004) com o esquema mínimo do Fase 3.5 que ADR 0004 já congelou: **`folders`**, **`files`**, **`file_versions`**. Entrega é puramente de schema + RLS + testes; o crate `garraia-storage` (GAR-394), o handler REST de upload/download, presigned URLs e o integrity HMAC runtime ficam em slices posteriores.

O objetivo é **destravar** 4 caminhos cruzados (crate storage, endpoints de arquivos, anexos de chat, anexos de task) e adicionar 3 tabelas ao conjunto FORCE RLS (18 → 21).

## 2. Non-goals

- **Não implementa** `file_shares` — v1 da ADR 0004 mantém o sharing para slice futuro (semantics de permission/share-token merecem design próprio).
- **Não implementa** `message_attachments` nem `task_attachments` — migrations separadas quando o contrato do crate `garraia-storage` estabilizar.
- **Não integra** pgvector ou FTS em arquivos — busca por arquivos é ADR 0006 / Fase 3.6, e fica em plan separado.
- **Não escreve** código Rust de CRUD — nem no `garraia-workspace`, nem no `garraia-gateway`. Este plan é schema + test-only.
- **Não materializa** o trait `ObjectStore` — é o escopo de GAR-394.
- **Não mexe** em `.env` nem em `garraia-config` — nenhuma env var nova é necessária para a migração sozinha.

## 3. Scope

**Arquivos novos:**

- `crates/garraia-workspace/migrations/003_files_and_folders.sql`
- `plans/0033-gar-387-migration-003-files.md` (este arquivo)

**Arquivos atualizados:**

- `crates/garraia-workspace/tests/migration_smoke.rs` — adiciona seção **“Migration 003 validation”** com:
  - 3 novas tabelas existem
  - 5 novos índices existem
  - 21 tabelas FORCE RLS (18 anteriores + `folders` + `files` + `file_versions`)
  - Cenário de positive-scope: mesmo grupo enxerga folder + file + version
  - Cenário de cross-group bloqueado para as 3 tabelas
  - Cenário de fail-closed (sem GUCs) para as 3 tabelas
  - Compound FK drift: `files.folder_id → folders(id, group_id)` bloqueia folder de outro grupo
  - Compound FK drift: `file_versions.file_id → files(id, group_id)` bloqueia version de arquivo de outro grupo
  - CHECK de `checksum_sha256` e `integrity_hmac` rejeitam formato inválido
  - CHECK de `size_bytes` rejeita negativo e > 5 GiB
  - Soft-delete de `files` (deleted_at) NÃO cascata para `file_versions` (invariante de auditoria)
  - Hard-delete de `files` cascata para `file_versions` via `ON DELETE CASCADE`

- `CLAUDE.md` — atualiza estrutura de crates: migration 003 deixa de ser “slot reservado” e entra como entregue; adiciona `folders/files/file_versions` ao inventário de tabelas sob FORCE RLS (18 → 21).
- `plans/README.md` — adiciona entrada 0033 apontando para este arquivo.
- `docs/adr/README.md` — marca GAR-387 como entregue (link para PR) no índice de ADRs afetados por 0004.

## 4. Acceptance criteria

1. Arquivo `003_files_and_folders.sql` aplica com sucesso em `pgvector/pgvector:pg16` via `sqlx::migrate!` quando rodado em sequência com todas as outras migrations (001..013).
2. `cargo test -p garraia-workspace --test migration_smoke` termina **green** com a nova seção de validação de 003.
3. As 3 novas tabelas estão **ENABLE + FORCE** Row-Level Security com policy **AS PERMISSIVE FOR ALL** e **USING + WITH CHECK** idênticos (evita regressão tipo migration 013).
4. As 3 tabelas estão cobertas por `GRANT SELECT, INSERT, UPDATE, DELETE ... TO garraia_app` explícito — **não** depende do `ALTER DEFAULT PRIVILEGES` de 007 (007 roda depois de 003 em ordem lexicográfica). Este é o mesmo padrão da migration 006.
5. `folders` tem self-FK `parent_id → folders(id) ON DELETE CASCADE`.
6. `files` tem **compound FK** `(folder_id, group_id) → folders(id, group_id)` — cross-group drift é impossível.
7. `file_versions` tem **compound FK** `(file_id, group_id) → files(id, group_id) ON DELETE CASCADE` — cross-group drift impossível E hard-delete cascata.
8. `file_versions.object_key` é UNIQUE global (ADR 0004 §Key schema).
9. `file_versions.checksum_sha256` tem CHECK `~ '^[0-9a-f]{64}$'` (64 hex chars).
10. `file_versions.integrity_hmac` tem CHECK `~ '^[0-9a-f]{64}$'` (HMAC-SHA256 hex).
11. `file_versions.size_bytes` tem CHECK `>= 0 AND <= 5368709120` (5 GiB cap runtime-fail-safe).
12. `files.deleted_at` é soft-delete; versões permanecem queryable para auditoria/restauração.
13. Todas as tabelas têm `created_by REFERENCES users(id) ON DELETE SET NULL` + `created_by_label` (erasure survival, mesmo padrão de `messages.sender_label`, `tasks.created_by_label`).
14. Nenhum uso de `DROP TABLE` ou `ALTER … DROP`. Forward-only.
15. `cargo clippy --workspace -- -D warnings` verde.
16. Review de `@code-reviewer` aprovado.
17. Review de `@security-auditor` aprovado (tabelas novas com PII risk + RLS + HMAC check + compound FK).

## 5. Design rationale

### 5.1 Escopo mínimo, fiel à ADR 0004

ADR 0004 §Versionamento exige: toda escrita cria nova versão (v1, v2...); `file_versions` registra `version`, `object_key`, `etag`, `checksum_sha256`, `integrity_hmac`. Este plan entrega exatamente isso — nada além.

### 5.2 `object_key` fica em `file_versions`, não em `files`

A ADR 0004 define chave como `{group_id}/{folder_path}/{file_uuid}/v{N}` — o sufixo `v{N}` significa que cada versão tem sua própria chave. Colocar `object_key` só no nível de `file` forçaria overwrite in-place e invalidaria o modelo de versionamento imutável. Cada row em `file_versions` carrega sua própria chave S3.

### 5.3 `current_version` como ponteiro denormalizado em `files`

Permite `SELECT files WHERE id = $1` retornar o ponteiro da versão vigente sem JOIN contra `file_versions`. Aplicação sobe o ponteiro via UPDATE quando novo upload entra. Alternativa (MAX(version) via subquery sempre) seria O(n) em cada read — descartada. Invariante: `current_version ≤ total_versions` é app-layer.

### 5.4 `folder_id` é NULLABLE

Root-level files têm `folder_id = NULL`. Aligned com ADR 0004 §Key schema “root files têm apenas `{group_id}/{file_uuid}/vN`”.

### 5.5 Compound FK em vez de trigger

Anti-cross-tenant via FK composta `(folder_id, group_id) → folders(id, group_id)` é o mesmo padrão que `messages.(chat_id, group_id) → chats` (migration 004) e `tasks.(list_id, group_id) → task_lists` (migration 006). Enforcement no DB, não em app code.

### 5.6 FORCE RLS com WITH CHECK explícito

Migração 013 estabeleceu o padrão: `AS PERMISSIVE FOR ALL` com `USING + WITH CHECK` **explícitos e idênticos**. Sem o WITH CHECK explícito, uma conversão futura para `AS RESTRICTIVE` destruiria silenciosamente o write-guard. Todas as 3 policies novas seguem esse contrato.

### 5.7 `file_versions.group_id` denormalizado

Mesmo padrão de `messages.group_id` e `tasks.group_id`. Habilita RLS direct policy sem JOIN recursivo a `files`. Compound FK mantém consistência obrigatória.

### 5.8 `garraia_app` role

Migration 003 roda **antes** de 007 em ordem lexicográfica. O `ALTER DEFAULT PRIVILEGES ... TO garraia_app` de 007 só cobre tabelas criadas **depois** de 007. Portanto 003 precisa:

1. Criar `garraia_app NOLOGIN` idempotente (o bloco é no-op quando 006 já criou).
2. `GRANT SELECT, INSERT, UPDATE, DELETE` explícito nas 3 tabelas novas.

Mesmo padrão que 006.

### 5.9 `object_key` UNIQUE global

ADR 0004 §Key schema define chaves como `{group_id}/{folder_path}/{file_uuid}/v{N}`. Como `file_uuid` é UUID v4 globalmente único e `vN` é monotônico, collision só aconteceria se o caller reescrevesse uma chave (bug). UNIQUE global no DB transforma esse bug em `23505 unique_violation` em vez de silent overwrite. O custo do índice é desprezível comparado à superfície de bug que ele fecha.

## 6. Work breakdown

| Task | Arquivo | Estimativa | Reviewer |
|------|---------|-----------|----------|
| T1 | `plans/0033-gar-387-migration-003-files.md` (este) | 10 min | — |
| T2 | `crates/garraia-workspace/migrations/003_files_and_folders.sql` | 30 min | code-reviewer + security-auditor |
| T3 | `crates/garraia-workspace/tests/migration_smoke.rs` — seção 003 | 40 min | code-reviewer |
| T4 | `CLAUDE.md` update | 5 min | — |
| T5 | `plans/README.md` + `docs/adr/README.md` | 5 min | — |
| T6 | `cargo test -p garraia-workspace --test migration_smoke` | 5-10 min | — |
| T7 | Agent review pass (code-reviewer + security-auditor em paralelo) | 15 min | — |
| T8 | Fix findings | 10 min | — |
| T9 | Commit + PR + CI | 10 min | — |
| T10 | Merge + atualizar `.garra-estado.md` + comentário Linear | 5 min | — |

Total: ~2h 15min.

## 7. Verification

- `cargo check -p garraia-workspace --tests` verde.
- `cargo clippy --workspace -- -D warnings` verde.
- `cargo test -p garraia-workspace --test migration_smoke -- --nocapture` verde (testcontainers pulls image na primeira vez).
- Inspeção visual no Postgres: `\d+ folders`, `\d+ files`, `\d+ file_versions`.
- `SELECT tablename FROM pg_tables WHERE rowsecurity = true` retorna 21 linhas.
- `SELECT tablename FROM pg_tables WHERE forcerowsecurity = true` retorna 21 linhas.
- CI GitHub Actions verde no branch antes do merge.

## 8. Rollback plan

Forward-only por política do projeto. Não há migration down. Rollback real significa:

- Se defeito detectado **antes** de deploy em ambiente compartilhado: revert do commit + PR novo (feasível durante window de sessão).
- Se defeito detectado **depois** de deploy: migration compensatória (004.X) que adiciona colunas/constraints corretivos; nunca drop. Padrão do projeto (ver CLAUDE.md regra 9).

Como a migração adiciona tabelas novas sem backfill de produção, rollback via revert é trivial enquanto o PR não está mergeado.

## 9. Risk assessment

| Risco | Likelihood | Impact | Mitigação |
|-------|-----------|--------|-----------|
| sqlx rejeita migration 003 inserida após 004-013 já aplicadas | Baixo | Alto | Comportamento confirmado em `sqlx-core 0.8.6` migrator.rs:173-182 — só valida `applied ⊂ source`, aceita source com versões menores que já aplicadas. CI fresh DB aplica na ordem correta. |
| Compound FK quebra inserts legítimos em app-layer | Baixo | Médio | Cobertura dos testes novos (positive + cross-group + drift) + mesmo padrão já em produção em migrations 004/006. |
| `checksum_sha256` CHECK rejeita upper-case hex | Médio | Baixo | Regex `^[0-9a-f]{64}$` é lowercase-only. Call sites no futuro crate `garraia-storage` devem normalizar para lowercase antes do INSERT. Decisão documentada no comentário da coluna. |
| `current_version` e `total_versions` divergirem do count real de `file_versions` | Médio | Médio | Invariante app-layer (mesmo padrão de `messages.sender_label` — cached label). Audit query sugerida no COMMENT: `SELECT f.id FROM files f JOIN (SELECT file_id, MAX(version) mv, COUNT(*) ct FROM file_versions GROUP BY file_id) v ON f.id = v.file_id WHERE f.current_version <> v.mv OR f.total_versions <> v.ct;`. GAR-394 CRUD deve impor a invariante. |
| Compound FK a `files(id, group_id)` exige UNIQUE `(id, group_id)` em `files` | Confirmado | Alto | A migration declara `CONSTRAINT files_id_group_unique UNIQUE (id, group_id)` — mesmo padrão de `chats`/`task_lists`/`tasks`. Sem esse UNIQUE, o Postgres rejeita a FK composta com `42830 (there is no unique constraint matching given keys)`. |
| Índice `folders_unique_name_per_parent_idx` via `COALESCE(parent_id, UUID)` viola partial uniqueness se soft-deletes não filtrados | Baixo | Médio | Index é `WHERE deleted_at IS NULL`. Soft-deletes ficam fora do check — comportamento intencional (permite restaurar com mesmo nome após criar substituto). |

## 10. Security review trigger

**security-auditor APPROVE obrigatório** antes do merge. Superfície tocada:
- RLS em 3 tabelas novas, com WITH CHECK explícito.
- `object_key` + `etag` + `checksum_sha256` + `integrity_hmac` — primitivas de integridade que o runtime vai consumir (GAR-394).
- Compound FK para cross-tenant.
- `created_by` nullable + `created_by_label` cache (erasure survival — GDPR art. 17 / LGPD art. 18).
- `size_bytes` cap (DoS mitigation).
- Sem secrets, sem PII direta, mas tabelas **vão armazenar PII** quando o crate `garraia-storage` ficar online — plano aceito pela ADR 0004 e DPIA (plan 0031).

## 11. Changelog notes

`.garra-estado.md` deve ganhar entrada:
- Migration 003 (folders/files/file_versions) aplicada via GAR-387.
- FORCE RLS agora em 21 tabelas (18 → 21).
- Slot 003 do workspace fechado; caminho para GAR-394 limpo.
- `message_attachments` e `task_attachments` continuam diferidos.

Linear:
- GAR-387 → Done com link para PR + commit de merge.
- Comentário linkando este plan e a ADR 0004.

## 12. Open questions

Nenhuma — escopo fechado por ADR 0004. Decisões não cobertas (share tokens, presigned URL TTL, mime allow-list runtime) moram em slices do crate `garraia-storage`.
