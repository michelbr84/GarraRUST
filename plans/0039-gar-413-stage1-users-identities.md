# Plan 0039 — GAR-413 implementation slice 1: Stage 1 (users + user_identities + PHC reassembly)

**Status:** Em execução
**Autor:** Claude Opus 4.7 (sessão autônoma, ClaudeMaxPower + Superpowers)
**Data:** 2026-04-21 (America/New_York)
**Issues:** [GAR-413](https://linear.app/chatgpt25/issue/GAR-413)
**Branch:** `feat/0039-gar-413-stage1-users-identities`
**Spec normativa:** [plan 0034](0034-gar-413-migrate-workspace-spec.md) §5–§7.2
**Unblocks:** slices futuros de GAR-413 (stages 3–10: groups, chats, messages, memory, sessions, api_keys, audit retrofit).

---

## 1. Goal

Entregar a **primeira fatia executável** do comando `garraia migrate workspace` cobrindo exatamente os stages que o plan 0034 §7.1 e §7.2 exigem:

1. **`users`** — copia cada row de `mobile_users` (SQLite) para `users` (Postgres) com UPSERT em `email` preservando `legacy_sqlite_id`.
2. **`identities`** — cria um `user_identities` row com o password hash **reassembled em PHC format** (PBKDF2-SHA256, 600k iterações, l=32, salt/hash base64-STANDARD-NO-PAD) + INSERT atômico em `audit_events` na mesma transação (ADR 0005 §Migration strategy + plan 0034 §7.2 SEC-H-3).
3. Pré-flight checks §6 (1–6 do plan 0034 — confirmation gate + schema version + BYPASSRLS/superuser re-check in-tx).
4. Exit codes sysexits (0 / 2 / 64 / 65 / 67 / 74 / 78).

Este slice é **executável** (roda `cargo run -- migrate workspace --from-sqlite … --to-postgres …`) mas **não cobre** stages 3+ — operators com dados de mensagens/memória vão precisar esperar slices seguintes ou fazer import manual.

## 2. Non-goals

- **Não** implementa stages 3+ (groups, chats, messages, memory, sessions, api_keys, audit retrofit) — slices seguintes de GAR-413.
- **Não** suporta flags `--only` / `--skip` neste slice; o command corre sempre os 2 stages. (Adicionadas em slice seguinte quando >2 stages existirem.)
- **Não** suporta `--verbose` / `--batch-size` — volumes de `mobile_users` em prática são <1000 rows (baseline mobile alpha).
- **Não** remove nem modifica nenhuma row do SQLite (leitura read-only, preservando ADR 0003 §Migration §Rollback).
- **Não** força password reset — hashes PBKDF2 são mantidos com `hash_upgraded_at = NULL` para lazy upgrade no próximo login (plan 0036 GAR-382 já resolve o upgrade em `mobile_auth.rs`, e o equivalente Postgres via `garraia-auth` desde GAR-391b).
- **Não** cria grupo `Legacy Personal Workspace` — isso fica para stage 3 (groups).
- **Não** emite e-mail nem notifica usuários.
- **Não** migra `mobile_auth.rs` para ler de Postgres — esse switch é GAR-332+ (mobile API v2).

## 3. Scope

**Arquivos novos:**

- `crates/garraia-cli/src/migrate_workspace.rs` — implementação (~500 LOC incluindo docs).
- `crates/garraia-cli/tests/migrate_workspace_integration.rs` — integration test com `testcontainers-modules::postgres` + temp SQLite.
- `plans/0039-gar-413-stage1-users-identities.md` (este arquivo).

**Arquivos modificados:**

- `crates/garraia-cli/src/main.rs` — adiciona `MigrateCommands::Workspace { … }` + handler match-arm.
- `crates/garraia-cli/src/migrate.rs` — re-export incluindo o novo módulo se necessário (provavelmente zero mudanças — novo módulo é peer).
- `crates/garraia-cli/Cargo.toml` — adiciona `sqlx`, `rusqlite`, `base64` (runtime); adiciona `testcontainers*` (dev).
- `plans/README.md` — entrada 0039.
- `CLAUDE.md` — menção do `migrate workspace` em `garraia-cli/`.

## 4. Acceptance criteria

1. `cargo check -p garraia` verde.
2. `cargo clippy -p garraia --all-targets -- -D warnings` verde.
3. `cargo fmt --check` verde.
4. `cargo test -p garraia --lib` verde (unit tests em `migrate_workspace` — PHC reassembly round-trip).
5. `cargo test -p garraia --test migrate_workspace_integration` verde localmente quando Docker disponível; skip graceful caso contrário.
6. `garraia migrate workspace --help` exibe a linha de comando completa.
7. Pré-flight checks §6.1–§6.6 do plan 0034 implementados: SQLite file readable, Postgres reachable, schema version mínimo (≥003), BYPASSRLS/superuser check (com re-check in-tx no stage 1 — SEC-H-1), confirmation gate (`--confirm-backup` obrigatório quando `users.count > 0`).
8. PHC reassembly produz string aceita por `garraia_auth::hashing::verify_pbkdf2` (verified via unit test com fixture real de `mobile_auth::hash_password`).
9. Audit row `action='users.imported_from_sqlite'` emitido atomicamente com o INSERT em `user_identities` (SEC-H-2).
10. Idempotência: re-rodar o comando em Postgres já migrado resulta em 0 rows inseridos, 0 erros (UPSERT no users + `ON CONFLICT DO NOTHING` em identities + `WHERE NOT EXISTS` em audit).
11. Exit codes: 0 (sucesso), 65 (PBKDF2 fields corrompidos no SQLite), 67 (sem BYPASSRLS), 78 (confirmation missing).
12. `--dry-run` não faz nenhum INSERT; imprime counts que serão migrados.
13. `@code-reviewer` APPROVE.
14. `@security-auditor` APPROVE ≥ 8.5/10.
15. CI 9/9 green.
16. Linear GAR-413 comentada (issue reaberta com comentário "Stage 1 shipped; stages 3+ pending in follow-up slices").

## 5. Design rationale

### 5.1 PHC reassembly no caller, não em `garraia-auth`

`garraia-auth` expõe `verify_pbkdf2(phc, password)` que consome PHC strings. Ele não oferece um helper "dado hash+salt raw bytes, produza PHC" porque o caso normal em `garraia-auth` é gerar PHC nativo. A tool de migração é o único consumer desse reverse-mapping. Em vez de alargar a API de `garraia-auth` (superfície pública), a lógica fica contida no módulo `migrate_workspace` com um unit test que assere round-trip `verify_pbkdf2(reassembled, original_pw) == true`.

### 5.2 Conexão exclusiva por invocação

Plan 0034 §6.3 SEC-H-1 exige **uma única conexão** para o run inteiro. Uso `PgPool::connect_with(max_connections=1)` em vez de `sqlx::PgConnection::connect` porque `PgPool` já integra com `sqlx::Transaction` e preserva connection reuse automaticamente. Com `max_connections=1` o pool degenera para uma conexão — suficiente para o invariante anti-race.

### 5.3 Sem commit em `--dry-run`

`sqlx::Transaction::rollback()` é o escape hatch: o dry-run roda exatamente os mesmos INSERTs, mas nunca chama `commit()`. Garante que o plano `EXPLAIN` não divirja do caminho de prod (ao contrário de `EXPLAIN` standalone, que omite efeitos colaterais).

### 5.4 `clap` subcommand dentro de `migrate`, não top-level

O command `migrate openclaw` já existe. Adicionar `migrate workspace` como segundo subcommand preserva consistência: `garraia migrate workspace …` / `garraia migrate openclaw …`. Top-level `garraia migrate-workspace` criaria um segundo comando sibling de `migrate` — ruim para UX.

### 5.5 Integration test usa testcontainer Postgres local

Mesmo pattern de `garraia-workspace/tests/migration_smoke.rs` — spin a Postgres container com pgvector, aplicar todas as migrations 001–013, popular um SQLite temp file com 3 rows, rodar o command real via `Command::cargo_bin`, asserir rows em ambos os DBs. O test é gated por Docker availability.

## 6. Testing strategy

- **Unit test `phc_reassembly_roundtrip`** (em `migrate_workspace.rs` `#[cfg(test)]`):
  - Gera uma senha random.
  - Chama `garraia_gateway::mobile_auth::hash_password(pw)` → `(hash_b64_standard, salt_b64_standard)`.
  - Reassembla via `pbkdf2_legacy_to_phc(hash_b64, salt_b64)`.
  - Chama `garraia_auth::hashing::verify_pbkdf2(phc, pw) == Ok(true)`.
  - Repete com 2 fixtures hardcoded (cobre determinismo).
- **Unit test `phc_reassembly_rejects_invalid_base64`**: hash com padding faltando ou charset inválido retorna erro descritivo.
- **Unit test `preflight_rejects_schema_version_missing`**: simula `_sqlx_migrations` com versão 002 ausente — pré-flight aborta com exit 64.
- **Integration test `migrate_workspace_stage1_happy_path`**: 3 mobile_users no SQLite → 3 users + 3 user_identities + 3 audit_events em Postgres. Re-run → 0 novos inserts.
- **Integration test `migrate_workspace_stage1_confirmation_gate`**: Postgres com `users.count > 0` sem `--confirm-backup` → exit 78.
- **Integration test `migrate_workspace_dry_run_no_side_effects`**: `--dry-run` com 3 rows → rows count=0 em Postgres após execução, exit=0, stdout mostra "would migrate 3 users, 3 identities".

## 7. Security review triggers

- **SEC-H PHC reassembly correctness**: erro aqui = todos os usuários migrados não conseguem logar. Test unit com fixture real de `mobile_auth::hash_password` + assert via `verify_pbkdf2` é o único gate confiável. Sem round-trip test, o slice NÃO merge.
- **SEC-H atomic audit**: identity + audit em mesma transação. Se audit falhar, rollback descarta identity. Test integration assere: após migração, `COUNT(user_identities) == COUNT(audit_events WHERE action='users.imported_from_sqlite')`.
- **SEC-H BYPASSRLS race window**: re-check dentro da primeira tx (plan 0034 §6.3). Documentado no código + test integration com role sem BYPASSRLS retornando exit 67.
- **SEC-M log redaction**: `postgres_url` pode conter password — nunca logado em clear. `tracing::Span` usa `skip` em `postgres_url`, `sqlite_path`. PHC string nunca logada.
- **SEC-M base64 padding confusion**: `mobile_auth.rs` usa `base64::STANDARD` (com padding), `password-hash` usa `STANDARD_NO_PAD`. Decoder + reencoder explícitos em `pbkdf2_legacy_to_phc` com teste asserindo que inputs com padding (`…==`) e sem padding (`…`) produzem PHC diferentes — indicação de erro que passou.
- **SEC-L legacy_sqlite_id PII**: coluna `users.legacy_sqlite_id` é um UUID v4 do SQLite — não é PII per se, mas é um join key entre dois sistemas. Documentar em ADR 0005 amendment (deferido).

## 8. Rollback plan

Reversível via `git revert` do commit (implementação é um novo módulo Rust + 1 match-arm em `main.rs`; remover não quebra nenhum caminho existente).

Para um tenant que já rodou a migração, rollback de dados é manual:
```sql
DELETE FROM users WHERE legacy_sqlite_id IS NOT NULL;
-- Cascades to user_identities (FK). audit_events persist by design (plan 0034 §8).
```
Documentado em `migrate_workspace.rs` como docstring do módulo.

## 9. Risk assessment

| Risco | Severidade | Mitigação |
|---|---|---|
| PHC reassembly incorreto → users não logam | CRITICAL | Round-trip unit test obrigatório (acceptance #8). |
| Audit row orfão (stage 7.1 falha depois de insert users) | HIGH | UPSERT em `users`; retry é no-op. Audit vem no stage 7.2, no mesmo tx da identity. |
| BYPASSRLS revogado mid-run | HIGH | Re-check in-tx. |
| Sqlx v0.8 breaking change futuro | MEDIUM | Pin `0.8` minor; cargo-audit CI (plan 0026). |
| Postgres connection timeout > `PGCONNECT_TIMEOUT` default (8s) | LOW | Documentar em `--help`: operator pode setar `PGCONNECT_TIMEOUT`. |
| `rusqlite` version drift entre `garraia-db` e `garraia-cli` | MEDIUM | Declarar `rusqlite = { workspace = true }` — sync com `garraia-db`. |
| `mobile_users.email` colide com signup pós-migração | MEDIUM | UPSERT `ON CONFLICT (email)` preserva a row existente, seta `legacy_sqlite_id`. Audit log captura o conflito. |

## 10. Open questions

- **Q1**: Deveria aceitar `--from-sqlite` apontando para diretório (e descobrir `garraia.db`)? → Decisão: **não**; caller passa path literal. Reduz ambiguity em Windows.
- **Q2**: Expor `--use-ssl` para Postgres URL? → **Não**; URL `postgres://...?sslmode=require` cobre. Documentado em `--help`.

## 11. Future work

- **Slice seguinte**: Stages 3 (groups) + 4 (groups resolve + group_members). Com Stage 1 estável, Stage 3 é independente.
- **Slice N**: Stages 5–10 (chats, messages, memory, sessions, api_keys, audit retrofit) + `--only`/`--skip` + `--batch-size`.
- **Slice final**: rollback helper `garraia migrate workspace --rollback` (DELETE FROM users WHERE legacy_sqlite_id IS NOT NULL + audit row "rollback").

## 12. Definition of done

- [x] Plan mergeado (este arquivo).
- [ ] Módulo `migrate_workspace.rs` implementado.
- [ ] CLI subcommand wired em `main.rs`.
- [ ] Unit tests (PHC round-trip) verdes.
- [ ] Integration test (testcontainer Postgres) verde local.
- [ ] `cargo check/clippy/fmt/test` verdes workspace-wide.
- [ ] `@code-reviewer` APPROVE.
- [ ] `@security-auditor` APPROVE ≥ 8.5/10.
- [ ] PR aberto.
- [ ] CI 9/9 green.
- [ ] PR merged.
- [ ] Linear GAR-413 comentada (issue reaberta; slice 1/N done).
- [ ] `CLAUDE.md` + `plans/README.md` atualizados.
- [ ] `.garra-estado.md` atualizado ao fim da sessão.
