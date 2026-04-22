# Plan 0036 — GAR-382: Argon2id replace PBKDF2 em `mobile_auth.rs` (dual-verify + lazy upgrade)

**Status:** Em execução
**Autor:** Claude Opus 4.7 (sessão autônoma, ClaudeMaxPower + Superpowers)
**Data:** 2026-04-21 (America/New_York)
**Issues:** [GAR-382](https://linear.app/chatgpt25/issue/GAR-382)
**Branch:** `feat/0036-gar-382-argon2id-mobile-auth`
**Unblocks:** GAR-413 impl (migration de `mobile_users → user_identities` — PHC Argon2id elimina o passo de PHC-reassembly); hardening geral da superfície `/auth/*` do mobile

---

## 1. Goal

Fechar o último caminho onde o projeto ainda escreve PBKDF2-SHA256 (600k iter)
em SQLite — o endpoint `/auth/register` em `mobile_auth.rs` que serve o app
Flutter. O slice reusa o módulo `garraia_auth::hashing` (Argon2id RFC 9106
primary + PBKDF2 dual-verify), já produzindo e verificando PHC strings para
o fluxo Postgres desde o plan 0011 (GAR-391b).

Contrato de migração:

- **Registros novos:** SEMPRE gravam `$argon2id$...` PHC em
  `mobile_users.password_hash`. O campo `mobile_users.salt` fica como
  `""` (string vazia — preserva o schema existente com coluna NOT NULL).
- **Usuários antigos (PBKDF2):** continuam logando sem intervenção. Após
  o primeiro `verify_password` bem-sucedido, o hash é **atomicamente**
  re-gravado como Argon2id PHC e o `salt` é zerado. Se o update falhar,
  o login ainda sucede — lazy upgrade é best-effort.
- **Detecção de formato:** `password_hash` que começa com `$argon2id$`
  → Argon2id; qualquer outra coisa + `salt != ""` → PBKDF2 legacy.
- **Anti-enumeration:** mesmo quando o e-mail não existe, o handler
  de login executa `verify_argon2id` contra o `DUMMY_HASH` do
  `garraia_auth::hashing` para manter latência constante (já implementado
  em `InternalProvider::verify_credential`; aqui reusamos).

## 2. Non-goals

- **Não** altera a forma como o fluxo **Postgres** de `/v1/auth/*` faz
  hashing (isso já é Argon2id + dual-verify desde 391b; nada a mudar).
- **Não** deprecia nem remove a tabela `mobile_users` do SQLite — essa
  migração é escopo de GAR-413 impl (0034-a-implementar).
- **Não** muda o schema SQL de `mobile_users` (sem migration SQLite nova).
  A coluna `salt` continua existindo como TEXT NOT NULL; gravamos `""`
  quando o hash é Argon2id PHC (que já contém o salt embedded).
- **Não** mexe em `oauth.rs` nem `totp.rs` (não usam `hash_password`
  — só chamam `issue_jwt_pub`).
- **Não** bumpa a dependência `ring` — continuamos permitindo verify
  PBKDF2 via `ring::pbkdf2` *ou* via `garraia_auth::hashing::verify_pbkdf2`
  (este último espera PHC string; para legacy base64 precisamos manter
  o código `ring` porque o formato já gravado em prod é base64 cru).

## 3. Scope

**Arquivos modificados:**

- `crates/garraia-gateway/src/mobile_auth.rs`:
  - `hash_password(password) -> (String, String)` — passa a retornar
    `(argon2id_phc, "".to_string())`. Usa
    `garraia_auth::hashing::hash_argon2id`.
  - Nova função `verify_password_and_maybe_upgrade(state, user, password)
    -> bool`:
    - detecta formato do hash armazenado
    - verifica Argon2id (se `$argon2id$...`) OU PBKDF2 legacy via `ring`
      (se `salt != ""`)
    - se PBKDF2 verificou com sucesso, chama novo
      `SessionStore::upgrade_mobile_user_hash` com o novo PHC Argon2id
      (best-effort — erro é logado em `warn!` mas não falha login)
  - `login()` handler passa a chamar a nova função.
  - Mantém o módulo `ring` para o path PBKDF2 legacy (via função dedicada
    `verify_pbkdf2_legacy`).
  - Adiciona pseudo-verify contra um hash dummy (Argon2id) no path
    "user not found" para neutralizar enumeration — usando
    `garraia_auth::hashing::consume_dummy_hash`.
- `crates/garraia-db/src/session_store.rs`:
  - Nova função `update_mobile_user_hash(&self, id: &str, new_phc: &str)
    -> Result<usize>` que faz `UPDATE mobile_users SET password_hash=?1,
    salt='' WHERE id=?2`. Retorna `Ok(0)` silenciosamente quando 0 linhas
    foram atualizadas (o caller loga warning com `user_id`); retorno
    `usize` em vez de `()` permite distinguir "zero rows" de "erro" e
    preserva a semântica best-effort do lazy upgrade.
- `crates/garraia-gateway/Cargo.toml`: já tem `garraia-auth = { workspace = true }`
  como dep; precisa adicionar `secrecy = { workspace = true }` (para
  `SecretString` passado para `hash_argon2id`). Verificar se já está.

**Arquivos novos:**

- `crates/garraia-gateway/tests/mobile_auth_hash_migration.rs` — suite
  de integração cobrindo:
  - new register → hash é Argon2id PHC
  - register → login roundtrip (Argon2id)
  - legacy PBKDF2 user pode fazer login (semeado via raw INSERT com
    hash + salt base64)
  - após login PBKDF2, `password_hash` no DB é agora Argon2id PHC e
    `salt` é `""`
  - segundo login (após upgrade) usa path Argon2id e continua funcionando
  - password errada em user Argon2id → 401
  - password errada em user PBKDF2 (pré-upgrade) → 401 **sem** upgradar
- `plans/0036-gar-382-argon2id-mobile-auth.md` — este arquivo.

**Arquivos atualizados:**

- `plans/README.md` — entrada 0036.
- `CLAUDE.md` — nota breve de que `mobile_auth.rs` agora é dual-verify
  + lazy upgrade, como o fluxo Postgres.

## 4. Acceptance criteria

1. `cargo check -p garraia-gateway -p garraia-db` verde.
2. `cargo clippy -p garraia-gateway -p garraia-db -- -D warnings` verde
   (as `too_many_arguments` pré-existentes em `session_store.rs` continuam
   cobertas por `continue-on-error` no CI — escopo fora deste slice).
3. `cargo test -p garraia-gateway` verde, incluindo nova suite de migração.
4. Novo user registra com Argon2id (teste `register_emits_argon2id_phc`).
5. Legacy PBKDF2 user consegue fazer login com password correta (teste
   `legacy_pbkdf2_login_succeeds`).
6. Após login legacy PBKDF2 OK, hash no DB muda para Argon2id PHC (teste
   `legacy_login_triggers_lazy_upgrade`).
7. Login com password errada em user PBKDF2 pré-upgrade **não** atualiza
   o hash (teste `wrong_password_does_not_upgrade`).
8. Anti-enumeration: tempo de resposta para "user not found" ≈ tempo de
   Argon2id verify (teste opcional `anti_enumeration_latency_smoke`,
   tolerante a ruído de CI — não é invariant técnico, mas um guard).
9. `@code-reviewer` APPROVE.
10. `@security-auditor` APPROVE (≥ 8.5/10).
11. CI 9/9 green.

## 5. Design rationale

### 5.1 Por que **não** adicionar coluna nova ao schema

A opção "adicionar `hash_format` enum" foi considerada e descartada:
tornaria a migração forward-only em SQLite (não há `ALTER TABLE DROP
COLUMN` antes do SQLite 3.35). O schema atual já permite dual-format
via convenção "salt vazio ⇒ PHC embedded". O custo é um comentário de
documentação; o benefício é zero migração SQL.

### 5.2 Best-effort upgrade

Se o `UPDATE` do lazy upgrade falhar (ex.: concorrência, DB lock), o
login ainda deve suceder — o upgrade é opportunistic. Isso evita criar
um DoS de login por um bug de upgrade. Log de `warn!` é suficiente para
detecção em produção. Isso espelha o comportamento de
`InternalProvider::verify_credential` no fluxo Postgres (plan 0011.5).

### 5.3 Anti-enumeration mantida

O código existente retorna `UNAUTHORIZED` com a mesma resposta para
"user not found" e "wrong password" — bom. Porém o **timing** hoje é
diferente (user not found = sem hash = retorno instantâneo; user found
= PBKDF2 600k iter ≈ 100-300ms). Argon2id é ainda mais lento (~100-500ms).
O fix é: no path "user not found" chamamos `consume_dummy_hash` (já
existe no `garraia_auth::hashing`) para absorver latência equivalente a
um verify real.

### 5.4 PBKDF2 verify continua via `ring`, não via `garraia_auth`

`garraia_auth::hashing::verify_pbkdf2` espera PHC string `$pbkdf2-sha256$...`.
O formato gravado em `mobile_users` é **base64 cru** (hash) + **base64**
(salt) em colunas separadas — pré-existente desde GAR-334 (cycle 11).
Reusar a lógica `ring::pbkdf2` preserva binary compatibility com DBs
existentes. O slice GAR-413 impl é quem converte esses registros para
PHC string durante a migração SQLite→Postgres.

## 6. Testing strategy

- **Unit tests** em `mobile_auth.rs` (mesma filosofia de
  `verify_password` antigos): em `#[cfg(test)]`:
  - `hash_password_produces_argon2id_phc`
  - `verify_password_accepts_argon2id_and_pbkdf2_paths`
- **Integration test** novo em `crates/garraia-gateway/tests/mobile_auth_hash_migration.rs`:
  - Cria `AppState` com `SessionStore` em SQLite em memória.
  - Semeia um user legacy via raw INSERT (hash/salt base64 produzidos
    com o mesmo `ring::pbkdf2`).
  - Testa o full roundtrip via `login()` handler.
- **Non-goal:** não teste de JWT issuance (coberto por GAR-335 tests).

## 7. Security review triggers

- Confirmar que `hash_password` NUNCA retorna PBKDF2 para novos registros.
- Confirmar que `verify_password_and_maybe_upgrade` constant-time: o
  path "user not found" deve consumir tempo comparável ao verify real.
- Confirmar redaction: `password` nunca loga via `warn!`/`error!`.
- Confirmar que `update_mobile_user_hash` usa `params!` (sem concatenação
  SQL).
- Confirmar que o PHC Argon2id gravado usa parâmetros RFC 9106 first
  recommendation (m=64 MiB, t=3, p=4) — `hash_argon2id` já garante.
- Confirmar que o teste de latência (§4 critério 8) tolera variabilidade
  de CI sem ficar flakey.

## 8. Rollback plan

Reversível via `git revert <merge-commit>`. Não há migration SQL, não
há mudança de schema. Após revert:

- Novos registros voltam a usar PBKDF2.
- Users que já foram upgradados para Argon2id ficarão com
  `password_hash = $argon2id$...` e `salt = ""` — o código PBKDF2-only
  revertido **não** consegue fazer login desses users (verify falhará).
- **Mitigação:** antes de revert em prod, operador tem que
  (a) manter uma flag de config para manter dual-verify, OU
  (b) aceitar que users upgradados vão precisar reset de senha.
- **Recomendação:** revert sem mitigação só é seguro se zero users
  fizeram login após o deploy. Documentar no PR + runbook.

## 9. Risk assessment

| Risco | Severidade | Mitigação |
|---|---|---|
| Lazy upgrade race (duas requests concorrentes upgradando o mesmo user) | LOW | Último UPDATE vence. Ambos os PHCs Argon2id são válidos. Sem corrupção. |
| Argon2id aumenta latência de login (~3-5x) | LOW | RFC 9106 params escolhidos; p95 esperado < 500ms em hardware razoável. Já aceito para fluxo Postgres. |
| Rollback quebra users upgradados | MEDIUM | §8 documenta; PR exige explicit-ack antes de revert em prod. |
| PBKDF2 base64 dual-format detecta mal | MEDIUM | Testes cobrem o oráculo `starts_with("$argon2id$")`. |

## 10. Open questions

Nenhuma. O fluxo Postgres já estabeleceu todos os precedents.

## 11. Future work

- Slice posterior (GAR-413 impl): eliminar totalmente a tabela
  `mobile_users` migrando users para `users + user_identities` em
  Postgres. Quando isso ocorrer, este código morre — `mobile_auth.rs`
  pode ser deletado ou redirecionar para o fluxo `/v1/auth/*`.
- Retrofit do tempo constante: instrumentar métrica Prometheus
  `auth_verify_latency_seconds` no path mobile (paralelo ao que existe
  no fluxo Postgres).

## 12. Definition of done

- [x] Plan mergeado (este arquivo).
- [ ] Código implementado.
- [ ] Testes (unit + integration) verdes locally.
- [ ] Clippy + fmt verdes.
- [ ] Code review aprovado.
- [ ] Security audit aprovado ≥ 8.5/10.
- [ ] CI 9/9 green.
- [ ] PR mergeado em `main`.
- [ ] Linear GAR-382 **fechada** (slice único cobre todos os AC da issue).
- [ ] `plans/README.md` atualizado.
- [ ] `CLAUDE.md` atualizado.
- [ ] `.garra-estado.md` atualizado ao fim da sessão.
