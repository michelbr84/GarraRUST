# Plan 0009: GAR-375 — ADR 0005 Identity Provider

> **Status:** 📋 Awaiting approval
> **Issue:** [GAR-375](https://linear.app/chatgpt25/issue/GAR-375)
> **Project:** Fase 3 — Group Workspace
> **Labels:** `adr-needed`, `epic:ws-authz`, `security`
> **Priority:** Urgent
> **Estimated session size:** 3-4 horas de research + escrita de ADR
> **Author:** Claude Opus 4.6 + @michelbr84
> **Date:** 2026-04-13
> **Depends on:** ✅ GAR-407 (`user_identities` table exists) + ✅ GAR-408 (`user_identities_owner_only` RLS policy + `NULLIF` pattern + hard blocker doc) + ✅ GAR-390 (Fase 3 schema set complete)
> **Unblocks:** **GAR-391** (`garraia-auth` crate — login flow precisa do BYPASSRLS pattern definido), GAR-413 (migration tool precisa saber qual role usar para credentials)

---

## 1. Goal (one sentence)

Produzir e mergear o ADR `docs/adr/0005-identity-provider.md` que decide (a) **BYPASSRLS dedicated role vs SECURITY DEFINER function** para o login flow contra `user_identities` sob RLS, (b) **JWT interno HS256 + trait IdentityProvider plugável** para futuros adapters OIDC/SAML, (c) **estratégia de migração `mobile_users` → `user_identities`** preservando hashes legacy, e (d) **transição PBKDF2 → Argon2id via lazy upgrade no login** com fallback dual-verify — fechando o último hard blocker arquitetural identificado pelos reviews de GAR-408 e GAR-390 antes de GAR-391 (`garraia-auth`) começar.

---

## 2. Rationale — por que esse agora

1. **Único hard blocker remanescente da Fase 3.** O schema está completo (25 tabelas, 6 migrations). O review de GAR-408 documentou explicitamente que `user_identities` está sob RLS `user_id = current_user_id`, e ao tempo do login `current_user_id` ainda não é conhecido — qualquer SELECT retorna 0 rows. Sem decidir o BYPASS pattern agora, GAR-391 (`garraia-auth`) não pode escrever o login endpoint.
2. **Decisão de segurança crítica que merece ADR.** Choosing entre BYPASSRLS role e SECURITY DEFINER function tem implicações de blast radius (o que vaza se a role/função for comprometida), auditoria (como `audit_events` registra), e operação (qual role aplica migrations vs verifica credenciais). Não é decisão para sair em PR de feature.
3. **Destrava cadeia inteira.** GAR-391 depende disso → GAR-393 (API /v1/groups) depende de GAR-391 → toda a UI multi-tenant depende de GAR-393. Cada dia que o ADR atrasa, o caminho crítico inteiro fica parado.
4. **Mobile auth migration.** O `mobile_users` em SQLite (GAR-334/335) precisa migrar para `user_identities` em Postgres. A janela ideal é antes de qualquer novo usuário entrar — a estratégia precisa ser decidida aqui.
5. **PBKDF2 → Argon2id gap.** O `mobile_users` atual usa PBKDF2_HMAC_SHA256 (600k iter via ring). O `user_identities.password_hash` é `text` genérico, mas a recomendação é Argon2id (RFC 9106). Decidir agora se vamos forçar migração ou fazer lazy upgrade.
6. **Research-only, escopo cabível.** 3-4h. Sem código Rust. Sem migration nova. Apenas:
   - `docs/adr/0005-identity-provider.md` (ADR completo MADR)
   - Update do `docs/adr/README.md` index
   - Update do `ROADMAP.md` Fase 3.3 com link
   - Update do `CLAUDE.md` documentando o padrão de role login para futuros agentes
   - Linear GAR-375 → Done, GAR-391 desbloqueado

---

## 3. Scope & Non-Scope

### In scope

- **`docs/adr/0005-identity-provider.md`** seguindo template MADR (mesmo formato do ADR 0003) com seções:
  - Status (proposed → accepted após review)
  - Context and Problem Statement (RLS bloqueia login, PBKDF2 legacy precisa migrar, JWT precisa estar pronto para OIDC)
  - Decision Drivers (8 critérios ponderados)
  - Considered Options (4 opções para login flow + 3 para JWT algorithm + 2 para password migration)
  - Decision Outcome (escolhas explícitas com rationale)
  - Consequences (positive / negative / neutral)
  - **Login role specification** — exato comando `CREATE ROLE` + `BYPASSRLS` + `GRANT EXECUTE` pattern
  - **Migration strategy** SQLite mobile_users → Postgres user_identities (algoritmo passo-a-passo)
  - **PBKDF2 → Argon2id transition** com pseudocódigo Rust do dual-verify
  - **`IdentityProvider` trait shape** que GAR-391 vai implementar
  - Rollback plan
  - Links

- **Update `docs/adr/README.md`** marcando ADR 0005 como `accepted` no índice.

- **Update `ROADMAP.md` §3.3 Runtime Scopes & RBAC** marcando o sub-item de identity provider como `[x]`.

- **Update `CLAUDE.md` "Regras absolutas"** adicionando:
  - "11. **SEMPRE** usar `garraia_login` BYPASSRLS role exclusivamente em paths de credential verification — nunca para queries normais"
  - "12. **NUNCA** ler `user_identities.password_hash` no app pool role (ele será filtrado por RLS) — usar `garraia_login` via login endpoint"

- **Linear GAR-375 → Done** com link para o ADR.

### Out of scope

- ❌ **Implementação Rust do `garraia-auth` crate.** Esse é GAR-391, próximo issue. Este plano só decide e documenta — zero código Rust.
- ❌ **Migration SQL nova.** O `garraia_login` role + `IdentityProvider::verify_credential` SECURITY DEFINER fn (se essa for a escolha) entram em uma migration futura junto com GAR-391, não aqui. Este ADR só especifica.
- ❌ **OIDC adapter implementation.** O ADR define o trait `IdentityProvider` e a impl `Internal`. Adapters concretos (Keycloak, Auth0, Google, Authelia) ficam para um futuro **ADR 0009** ou diretamente para sub-issues de GAR-391.
- ❌ **SAML.** Mencionado como "futuro enterprise" mas não decidido.
- ❌ **MFA / 2FA / TOTP.** Merece ADR próprio (futuro ADR 0010).
- ❌ **WebAuthn / passkeys.** Mesma razão.
- ❌ **Magic links / passwordless.** Mesma razão.
- ❌ **Account lockout / brute-force protection.** Operacional, não arquitetural — vai em GAR-391 implementação + rate limiter de tower-governor.
- ❌ **Password reset flow** (email link, etc.). Operacional, GAR-391 ou follow-up.
- ❌ **Session management detalhes** (refresh rotation, device fingerprinting). GAR-391 implementation.
- ❌ **Rate limiting do login endpoint.** Concern de GAR-391 + tower-governor.
- ❌ **JWT key rotation.** Operacional, follow-up de GAR-391.
- ❌ **`mobile_users` DROP.** O SQLite legacy fica intacto durante e depois da migração — `garraia-db` continua existindo per ADR 0003.

---

## 4. Acceptance criteria (verificáveis)

- [ ] `docs/adr/0005-identity-provider.md` existe, > 300 linhas, segue template MADR.
- [ ] ADR tem TODAS as 7 seções obrigatórias (Status, Context, Decision Drivers, Considered Options, Decision Outcome, Consequences, Links).
- [ ] **Decisão explícita** sobre login flow: BYPASSRLS dedicated role OR SECURITY DEFINER function. Não "a ser definido".
- [ ] **Decisão explícita** sobre JWT algorithm: HS256 vs RS256 vs EdDSA. Não "a ser definido".
- [ ] **Decisão explícita** sobre password algorithm: Argon2id (com parâmetros: memory, iterations, parallelism).
- [ ] **`IdentityProvider` trait** especificado em pseudocódigo Rust com signature de `verify_credential`, `create_user`, `find_by_provider_sub`.
- [ ] **`Internal` impl** especificada — qual SQL roda, qual role, dual-verify path.
- [ ] **Login role specification:** `CREATE ROLE garraia_login NOLOGIN BYPASSRLS;` ou equivalente, GRANTs mínimos, REVOKE PUBLIC.
- [ ] **Migration script** SQLite `mobile_users` → Postgres `user_identities` documentado em pseudocódigo (não SQL real — isso é GAR-413).
- [ ] **PBKDF2 → Argon2id dual-verify** pseudocódigo Rust mostrando como detectar formato de hash no login.
- [ ] **Anti-pattern documented:** "empty result from app pool reading user_identities ≠ user not found".
- [ ] Pelo menos **4 alternativas** consideradas para login flow + pelo menos **3** para JWT.
- [ ] Risk register com pelo menos **6 riscos** (key compromise, BYPASSRLS leakage, dual-verify bug, PBKDF2 leak post-migration, JWT secret rotation, OIDC adapter drift).
- [ ] **Rollback plan** documentado em níveis (revert ADR vs revert código).
- [ ] `docs/adr/README.md` atualizado marcando ADR 0005 como `accepted`.
- [ ] `ROADMAP.md §3.3` atualizado marcando ADR 0005 como `[x]`.
- [ ] `CLAUDE.md` regras absolutas atualizadas com 2 novas regras (11, 12).
- [ ] Review verde de `@security-auditor` (foco: BYPASSRLS blast radius, Argon2id parameters, JWT key mgmt, dual-verify race conditions).
- [ ] Linear GAR-375 → Done.
- [ ] **Issue follow-up filed para GAR-391** (ou anotado no plan 0010 quando vier) listando os deliverables exatos: criar `garraia_login` role, implementar `IdentityProvider` trait, dual-verify path, audit events na login.

---

## 5. File-level changes

### 5.1 Novos arquivos

```
docs/adr/
  0005-identity-provider.md    # ★ o ADR principal
```

### 5.2 Edits em arquivos existentes

- `docs/adr/README.md` — entrada para ADR 0005 marcada `✅ accepted` na tabela index.
- `ROADMAP.md` §3.1 — `0005-identity-provider.md` marcado `[x]` com link.
- `ROADMAP.md` §3.3 — sub-item correspondente à decisão de identity provider marcado `[x]`.
- `CLAUDE.md` — adicionar regras 11 e 12 sobre `garraia_login` role e proibição de leitura de `password_hash` no app pool.
- `plans/0009-gar-375-adr-0005-identity-provider.md` — este arquivo (committed junto).

### 5.3 Zero edits em código Rust ou SQL

- Nenhuma migration nova.
- Nenhum crate touched.
- Nenhum teste rodado neste plan.

---

## 6. ADR outline (o que vai no arquivo)

Esta é a estrutura que o ADR vai seguir. **O conteúdo final é escrito por mim em wave 1**, não por um agente.

### 6.1 Header
```markdown
# 5. Identity Provider — login flow under RLS

- Status: proposed → accepted após review
- Deciders: @michelbr84, review @security-auditor
- Date: 2026-04-XX
- Tags: fase-3, security, ws-authz, gar-375
```

### 6.2 Context and Problem Statement

- Migration 007 (GAR-408) coloca `user_identities` sob FORCE ROW LEVEL SECURITY com policy `user_id = current_user_id`.
- Login flow (verificar credencial) precisa ler `password_hash` ANTES de saber `current_user_id` — é o que está sendo determinado.
- App pool role (`garraia_app`) sob essa policy retorna 0 rows. Tratar 0 rows como "user not found" é um anti-pattern: significa "RLS bloqueou".
- Precisamos definir COMO o login flow lê `user_identities` sem violar a postura de segurança nem deixar o credential store exposto.
- Adicionalmente: `mobile_users` em SQLite (GAR-334/335) tem PBKDF2 e precisa migrar; queremos Argon2id para futuros usuários; e precisamos preparar o terreno para OIDC sem rewriting depois.

### 6.3 Decision Drivers (ponderados)

1. ★★★★★ **Defense-in-depth** — login não pode ser o ponto fraco da arquitetura RLS
2. ★★★★★ **Compliance LGPD/GDPR** — `password_hash` é dado pessoal sensível
3. ★★★★ **Operational simplicity** — quanto mais simples, menos chance de bug humano
4. ★★★★ **Audit trail** — todo login attempt deve gerar `audit_events` row
5. ★★★ **Backward compatibility** — usuários atuais (mobile_users PBKDF2) não podem ser invalidados
6. ★★★ **Future OIDC plugability** — não fechar a porta para Keycloak/Auth0/Google sem rewriting
7. ★★ **Performance** — login p95 < 200ms inclui Argon2id verification
8. ★★ **Rollback safety** — se a decisão for errada, quanto custa reverter

### 6.4 Considered Options

#### Login flow access pattern (4 opções)

**A) BYPASSRLS dedicated role (`garraia_login`)** — recomendado v1
- `CREATE ROLE garraia_login NOLOGIN BYPASSRLS;`
- `GRANT SELECT ON user_identities TO garraia_login;` + `GRANT INSERT, UPDATE ON user_identities, sessions TO garraia_login;`
- Connection pool dedicated to this role used ONLY by login endpoint
- Argon2id verification in Rust app code (after the SELECT returns the hash)
- **Pros:** simple, single role boundary, audit via tracing, rollback trivial (DROP ROLE)
- **Cons:** if pool credentials leak, `user_identities` is fully exposed; mitigation = network isolation + audit + rotation

**B) SECURITY DEFINER function (`verify_credential`)**
- `CREATE FUNCTION verify_credential(p_email citext, p_password text) RETURNS uuid SECURITY DEFINER LANGUAGE plpgsql AS $$ ... $$;`
- Function owned by privileged role, runs as that role
- App pool calls `SELECT verify_credential($1, $2)` — gets back a `user_id` or NULL
- Argon2id verification happens INSIDE the function via `pgcrypto.crypt()` or via PL/Python
- **Pros:** narrowest possible surface — only the function can read the hash
- **Cons:** crypto verification in DB (we lose Rust crypto stack control), function ownership management, harder audit, Argon2 requires extension

**C) Hybrid: SECURITY DEFINER returns hash, Rust verifies**
- Function returns `(user_id, password_hash, status)` tuple
- App layer (Rust) does Argon2id verify
- **Pros:** keeps crypto in Rust, narrows DB surface
- **Cons:** still exposes hash to app pool indirectly via function return; arguably no real security gain over option A; more complex than A

**D) Skip RLS on `user_identities` entirely**
- `ALTER TABLE user_identities DISABLE ROW LEVEL SECURITY;` revert from migration 007
- Rely entirely on app-layer authz
- **Pros:** simplest, login flow trivial
- **Cons:** **REJECTED.** Defeats the entire point of GAR-408 — the table holds password hashes, it MUST be the most protected. RLS is the last line of defense.

**Decision: A (BYPASSRLS dedicated role).** Rationale documented below.

#### JWT signature algorithm (3 opções)

**1) HS256 (HMAC-SHA256, symmetric)** — recomendado v1
- Same secret signs and verifies (`GARRAIA_JWT_SECRET`)
- Matches current `mobile_auth.rs` implementation
- Single-node deployment friendly
- **Pros:** simple key mgmt (1 secret), fast, current code already uses it
- **Cons:** any verifier needs the signing secret — multi-instance deployments need shared secret distribution

**2) RS256 (RSA-SHA256, asymmetric)**
- Private key signs, public key verifies
- Verifiers don't need the signing secret
- **Pros:** scales to multi-instance/federation; public key can be distributed widely (JWKS)
- **Cons:** slower, more code to manage key pairs, overkill for single-instance v1

**3) EdDSA (Ed25519)** — modern alternative to RS256
- Smaller keys, faster than RS256
- **Pros:** modern crypto best practice
- **Cons:** less ecosystem support than HS256/RS256

**Decision: HS256 v1.** Migrate to RS256/EdDSA when multi-instance federation lands (Fase 7).

#### Password algorithm transition (2 opções)

**X) Lazy upgrade dual-verify** — recomendado
- New users: Argon2id only
- Existing PBKDF2 users: detect format via PHC string prefix, verify with PBKDF2 first, then re-hash with Argon2id and UPDATE the row
- After 6 months, audit `WHERE password_hash LIKE '$pbkdf2%'` and force-expire stragglers
- **Pros:** zero user disruption, gradual migration
- **Cons:** dual code path lives until straggler audit

**Y) Forced batch re-hash**
- Big bang migration: re-hash all PBKDF2 to Argon2id at deployment
- **REJECTED.** Cannot re-hash without the plaintext password, which we don't have. Only viable form is "force password reset for all users", which is a UX disaster.

**Decision: X (lazy upgrade dual-verify).**

### 6.5 Decision Outcome

**Login flow:** `garraia_login` BYPASSRLS dedicated role + Argon2id verification in Rust.

**JWT:** HS256 with `GARRAIA_JWT_SECRET` (same env var as current mobile_auth). 30-day refresh token, 15-min access token (subject to GAR-391 implementation tuning).

**Password algorithm:** Argon2id with parameters: memory 64 MiB, iterations 3, parallelism 4 (RFC 9106 first recommendation). Stored as PHC string format `$argon2id$v=19$m=65536,t=3,p=4$...`.

**PBKDF2 transition:** lazy upgrade via dual-verify path on next login.

**`IdentityProvider` trait** (pseudocódigo Rust, vai no ADR e depois em GAR-391):

```rust
#[async_trait]
pub trait IdentityProvider: Send + Sync {
    /// Provider identifier — 'internal', 'oidc', 'saml', etc.
    fn id(&self) -> &str;

    /// Look up an identity by (provider, provider_sub) — used post-OIDC callback.
    async fn find_by_provider_sub(&self, sub: &str) -> Result<Option<Identity>, AuthError>;

    /// Verify a credential and return the user_id if valid.
    /// For Internal: PBKDF2/Argon2id verify.
    /// For OIDC: validates ID token signature + claims.
    async fn verify_credential(&self, credential: &Credential) -> Result<Option<Uuid>, AuthError>;

    /// Create a new identity for an existing user.
    async fn create_identity(&self, user_id: Uuid, credential: &Credential) -> Result<(), AuthError>;
}

pub struct InternalProvider {
    /// Pool dedicated to garraia_login BYPASSRLS role.
    /// MUST NOT be shared with the app pool.
    login_pool: PgPool,
    argon2: argon2::Argon2<'static>,
}

impl InternalProvider {
    /// Verifies a (email, password) pair. Handles BOTH PBKDF2 (legacy) and
    /// Argon2id (current) hashes via PHC string prefix detection. On a
    /// successful PBKDF2 verify, lazy-upgrades the hash to Argon2id and
    /// UPDATEs the row in the same transaction.
    async fn verify_internal(&self, email: &str, password: &str) -> Result<Option<Uuid>, AuthError> {
        // 1. SELECT user_id, password_hash FROM user_identities
        //    WHERE provider = 'internal' AND provider_sub = (
        //      SELECT id::text FROM users WHERE email = $1
        //    );
        //    Uses login_pool which is BYPASSRLS — sees all rows.
        // 2. Detect hash format from PHC prefix.
        // 3. Verify with the matching algorithm.
        // 4. If PBKDF2 succeeded: re-hash with Argon2id + UPDATE in same tx.
        // 5. INSERT audit_events row (login.success or login.failure).
        // 6. Return Some(user_id) on success, None on failure.
    }
}
```

### 6.6 Login role specification (exato)

```sql
-- Migration 008 (future, part of GAR-391 implementation, NOT this plan)
-- Run AFTER migration 007 (RLS already in effect).

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'garraia_login') THEN
        CREATE ROLE garraia_login NOLOGIN BYPASSRLS;
    END IF;
END
$$;

-- Login role can:
--   1. SELECT user_identities to verify credentials
--   2. UPDATE user_identities to lazy-upgrade PBKDF2 → Argon2id
--   3. INSERT sessions to issue refresh tokens
--   4. INSERT audit_events to log login attempts
-- Login role CANNOT:
--   - Read messages, files, memory, tasks, etc. (RLS irrelevant — no GRANT)
--   - Read users.email beyond what's needed for credential lookup
--   - Read other users' identities
GRANT SELECT, UPDATE ON user_identities TO garraia_login;
GRANT SELECT ON users TO garraia_login;
GRANT INSERT, UPDATE ON sessions TO garraia_login;
GRANT INSERT ON audit_events TO garraia_login;

-- Production deployments MUST run the login pool with credentials for
-- garraia_login that are NEVER shared with the main app pool.
```

### 6.7 Migration strategy: `mobile_users` → `user_identities`

Pseudocódigo (real implementation in GAR-413):

```
For each row in SQLite mobile_users:
    1. user_id = uuid_v7()
    2. INSERT INTO Postgres users
       (id, email, display_name, status, legacy_sqlite_id, created_at)
       VALUES (user_id, mobile_users.email, mobile_users.email_local_part,
               'active', mobile_users.id, mobile_users.created_at);
    3. INSERT INTO Postgres user_identities
       (id, user_id, provider, provider_sub, password_hash, created_at)
       VALUES (uuid_v7(), user_id, 'internal', user_id::text,
               mobile_users.password_hash, mobile_users.created_at);
       NOTE: password_hash stays in PBKDF2 PHC format. Lazy upgrade on next login.
    4. AUDIT: INSERT INTO audit_events
       (group_id, actor_user_id, actor_label, action, resource_type, resource_id, metadata)
       VALUES (NULL, NULL, 'system', 'users.imported', 'user', user_id::text,
               jsonb_build_object('source', 'mobile_users_sqlite',
                                  'legacy_id', mobile_users.id));
```

The migration tool (`garraia-cli migrate workspace`, GAR-413) runs as superuser (needed to bypass RLS for the bulk INSERTs). No login flow involvement.

### 6.8 PBKDF2 → Argon2id dual-verify (pseudocódigo)

```rust
fn verify_password(hash: &str, password: &str) -> Result<bool, AuthError> {
    if hash.starts_with("$argon2id$") {
        let parsed = argon2::PasswordHash::new(hash)?;
        Ok(argon2::Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
    } else if hash.starts_with("$pbkdf2-sha256$") {
        // Legacy mobile_users format: parse PHC, verify with ring or pbkdf2 crate.
        let valid = pbkdf2_phc_verify(hash, password)?;
        if valid {
            // Caller is expected to upgrade the hash to Argon2id and UPDATE.
            // Returning Ok(true) but caller should check `needs_upgrade()`.
        }
        Ok(valid)
    } else {
        Err(AuthError::UnknownHashFormat)
    }
}

async fn login(email: &str, password: &str, pool: &PgPool) -> Result<Uuid, AuthError> {
    let mut tx = pool.begin().await?;
    // SET LOCAL ROLE garraia_login is implicit because pool is bound to that role.

    let row = sqlx::query!(
        "SELECT u.id, ui.password_hash
         FROM users u
         JOIN user_identities ui ON ui.user_id = u.id
         WHERE u.email = $1 AND ui.provider = 'internal'",
        email
    )
    .fetch_optional(&mut *tx)
    .await?;

    let row = match row {
        Some(r) => r,
        None => {
            // 0 rows here = user truly doesn't exist (we're in BYPASSRLS pool).
            // NOT the same as the empty-result-from-RLS pitfall documented in GAR-408.
            audit_login(&mut tx, None, "login.failure_user_not_found").await?;
            tx.commit().await?;
            return Err(AuthError::InvalidCredentials);
        }
    };

    if !verify_password(&row.password_hash, password)? {
        audit_login(&mut tx, Some(row.id), "login.failure_bad_password").await?;
        tx.commit().await?;
        return Err(AuthError::InvalidCredentials);
    }

    // Lazy upgrade if PBKDF2.
    if row.password_hash.starts_with("$pbkdf2-sha256$") {
        let new_hash = argon2_hash(password)?;
        sqlx::query!(
            "UPDATE user_identities SET password_hash = $1 WHERE user_id = $2",
            new_hash,
            row.id
        )
        .execute(&mut *tx)
        .await?;
        audit_login(&mut tx, Some(row.id), "login.password_hash_upgraded").await?;
    }

    audit_login(&mut tx, Some(row.id), "login.success").await?;
    tx.commit().await?;
    Ok(row.id)
}
```

### 6.9 Consequences

#### Positive
- Single, narrow login surface — `garraia_login` role used ONLY by login endpoint
- Lazy upgrade path zero-disrupts existing users
- Argon2id parameters match RFC 9106 first recommendation
- JWT internal pluggable for future OIDC
- Compliance: empty result from app pool ≠ user lookup (no information leak)
- Audit trail in `audit_events` for every login attempt
- Rollback trivial: `DROP ROLE garraia_login` + revert ADR

#### Negative
- Two pools to manage in production (app pool + login pool)
- BYPASSRLS role compromise = full credential store exposure (mitigation: network isolation, audit, rotation)
- PBKDF2 dual-verify code path lives until straggler audit (~6 months)
- HS256 needs shared secret for multi-instance (acceptable v1, plan migration to RS256 in Fase 7)

#### Neutral
- `mobile_users` table stays in SQLite (`garraia-db`) post-migration as historical record per ADR 0003
- OIDC adapter remains a future ADR — this one only ships the trait shape

### 6.10 Risk register

| Risk | Probability | Impact | Mitigation |
|---|---|---|---|
| `garraia_login` pool credentials leak | Low | **Critical** | Network isolation, distinct credential vault, rotation policy in GAR-410, audit pgaudit logs |
| Lazy upgrade UPDATE race condition | Low | Medium | UPDATE in same transaction as the verify SELECT — no window |
| PBKDF2 stragglers never log in again | Low | Low | After 6mo, audit `WHERE password_hash LIKE '$pbkdf2%'` and force password reset email |
| JWT `GARRAIA_JWT_SECRET` rotation breaks live sessions | Medium | Medium | Document rotation procedure (kid header + key set), defer to GAR-410 Vault implementation |
| `argon2` crate version drift breaks PHC parsing | Low | High | Pin version in workspace `Cargo.toml`, integration test on every cargo upgrade |
| BYPASSRLS role accidentally used by non-login code | Medium | **High** | `garraia-auth` crate exposes `LoginPool` newtype that is NOT `From<PgPool>` and only constructable via specific config path |
| audit_events INSERT fails mid-login → silent compromise | Low | Medium | Audit insert in same tx as verify; failure rolls back the entire login |

### 6.11 Validation

Since this is a research/decision ADR, validation comes via review (not benchmark like ADR 0003). Acceptance:
- Security review by `@security-auditor` covering blast radius, Argon2id parameters, audit completeness
- ADR is referenced from `garraia-workspace::user_identities` COMMENT (which already mentions GAR-391/ADR 0005 as the dependency)

### 6.12 Links
- [GAR-375](https://linear.app/chatgpt25/issue/GAR-375)
- [GAR-391](https://linear.app/chatgpt25/issue/GAR-391) — implementation that this ADR enables
- [GAR-413](https://linear.app/chatgpt25/issue/GAR-413) — migration tool
- [GAR-408](https://linear.app/chatgpt25/issue/GAR-408) — RLS migration that created the blocker
- [docs/adr/0003-database-for-workspace.md](../docs/adr/0003-database-for-workspace.md) — Postgres decision context
- [crates/garraia-workspace/migrations/007_row_level_security.sql](../crates/garraia-workspace/migrations/007_row_level_security.sql) — `user_identities_owner_only` policy
- [crates/garraia-workspace/README.md](../crates/garraia-workspace/README.md) — "HARD BLOCKER for GAR-391 production rollout" section
- RFC 9106 (Argon2)
- RFC 7519 (JWT)
- OWASP Authentication Cheat Sheet

---

## 7. Execution plan (no smoke test — this is a doc)

### Wave 1 — write the ADR (~2-2.5h, me)

1. Create `docs/adr/0005-identity-provider.md` following the §6 outline literally.
2. Verify cross-references to GAR-407, GAR-408, GAR-391, GAR-413 by reading the existing files.
3. Pseudocódigo Rust no Trait shape — verifying syntax against `argon2`, `sqlx`, and `async-trait` crate docs.
4. Write the migration strategy section (algorithm only — real SQL is GAR-413).

### Wave 2 — review (~30min wall, single agent background)

5. Spawn `@security-auditor` with focused scope on the ADR file:
   - BYPASSRLS blast radius
   - Argon2id parameter choice (RFC 9106)
   - JWT key management
   - Dual-verify race conditions
   - Audit completeness
   - Rollback plan integrity

### Wave 3 — fixes + meta-files + commit (~30min, me)

6. Apply security findings inline.
7. Update `docs/adr/README.md` index.
8. Update `ROADMAP.md` §3.1 + §3.3.
9. Update `CLAUDE.md` regras 11/12.
10. Commit + push.
11. Linear GAR-375 → Done.
12. Confirm in summary that GAR-375 is the only issue moved (GAR-391 stays Backlog but is now Unblocked).

**Total: 3-4 hours.** Sem código Rust, sem testes, sem migration.

---

## 8. Rollback plan

Three levels:

1. **Before merge:** close PR.
2. **After merge, before GAR-391 ships:** `git revert` the ADR commit. The decision is officially undecided again. Nothing in production yet.
3. **After GAR-391 ships:** the ADR is the historical record of why the decision was made. To change it, write `0005-superseded.md` ADR following standard MADR superseding pattern. The login flow code in GAR-391 must be refactored — non-trivial, requires its own plan.

Zero code touched in this plan. Rollback at level 1 or 2 is free.

---

## 9. Risks & mitigations (for the plan execution itself)

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| ADR hand-waves the BYPASSRLS vs SECURITY DEFINER decision | Baixa | Alto | §4 acceptance criteria exige decisão explícita; review pass enforced |
| Pseudocódigo Rust no Trait diverge da impl real em GAR-391 | Média | Médio | Pseudocódigo validado contra crates `argon2 0.5`, `sqlx 0.8`, `async-trait 0.1` na sessão |
| PBKDF2 PHC format detection é ambíguo | Baixa | Médio | `ring`/`pbkdf2` crates documentam PHC prefix `$pbkdf2-sha256$` — testado mentalmente contra mobile_auth.rs atual |
| Argon2id parameters causa login >1s p95 | Baixa | Médio | RFC 9106 first recommendation é otimizada para login (~50-100ms); parametrização documentada com nota de tunability |
| Security review descobre que BYPASSRLS é insuficiente para LGPD | Baixa | Alto | Hybrid option C (SECURITY DEFINER returns hash) está pronta como fallback |
| OIDC trait shape inadequado para Keycloak/Auth0 | Média | Baixo | Trait é v1, futuros adapters podem evoluir; documentado como "subject to GAR-391 implementation feedback" |

---

## 10. Definition of Done

- [ ] `docs/adr/0005-identity-provider.md` merged em `main`.
- [ ] `docs/adr/README.md` mostra ADR 0005 como `accepted`.
- [ ] `ROADMAP.md` §3.1 e §3.3 atualizados.
- [ ] `CLAUDE.md` ganha regras 11 e 12 sobre `garraia_login` role.
- [ ] Review verde de `@security-auditor`.
- [ ] Linear GAR-375 → **Done** com link para o commit do ADR.
- [ ] Resumo final ao usuário inclui seção "Linear — issues atualizadas" conforme regra nova.
- [ ] GAR-391 (`garraia-auth`) está pronto para ter um plan próprio iniciado — sem hard blockers arquiteturais remanescentes.

---

## 11. Open questions (preciso da sua resposta antes de começar)

1. **BYPASSRLS dedicated role vs SECURITY DEFINER function?** Recomendo **BYPASSRLS dedicated role (`garraia_login` NOLOGIN BYPASSRLS)** — mais simples, mantém crypto em Rust (single Argon2id implementation), surface audit clara via tracing. SECURITY DEFINER fica como hardening v2 se compliance auditor pedir mais isolation. Confirma?

2. **JWT algorithm: HS256 (atual mobile_auth) ou RS256/EdDSA?** Recomendo **HS256 v1** — matches código atual de `garraia-gateway/mobile_auth.rs`, single-instance friendly, simpler key mgmt. Migração para RS256/EdDSA fica como item de Fase 7 (multi-region/federation). Confirma?

3. **Argon2id parameters: RFC 9106 first recommendation (m=64MiB, t=3, p=4) ou agressivo (m=128MiB, t=4)?** Recomendo **RFC 9106 first recommendation** — balanceia segurança vs latência de login (~50-100ms typical). Tunable em config se compliance pedir. Confirma?

4. **PBKDF2 → Argon2id: lazy upgrade no login OR forced batch re-hash?** Recomendo **lazy upgrade dual-verify** — único caminho viável (não temos plaintexts para batch re-hash). Force-expire stragglers depois de 6 meses via password reset email. Confirma?

5. **`IdentityProvider` trait: design só `Internal` impl no ADR ou também esboçar `Oidc`?** Recomendo **só `Internal` neste ADR**. `Oidc` adapter merece ADR próprio (futuro 0009) quando alguém pedir Keycloak/Auth0 integration. Trait é deliberadamente shape-only para deixar a porta aberta. Confirma?

6. **Refresh token hash: Argon2id (slow) ou HMAC-SHA256 (fast) com site key?** Recomendo **HMAC-SHA256 com site key** — refresh tokens são random high-entropy strings (não dictionary-attackable), Argon2 é overkill e adiciona latência em token refresh hot path. `sessions.refresh_token_hash` no schema atual de GAR-407 é `text` genérico — funciona para qualquer formato. Confirma?

7. **`garraia_login` role tem GRANT SELECT em `users.email` ou em `users` inteiro?** Recomendo **`SELECT ON users`** (whole table) — login precisa de email para lookup, mas também pode precisar de display_name para audit_events.actor_label. Trade-off: row-level access via BYPASSRLS já é total, então column-level GRANT é cosmético. Confirma whole table?

---

## 12. Next recommended issue (depois de GAR-375 merged)

**GAR-391 — `garraia-auth` crate** (3-5 dias)

Com o ADR 0005 mergeado, GAR-391 fica completamente desbloqueado. O escopo será:

- Novo crate `crates/garraia-auth/`
- Migration 008 (ou equivalente) criando `garraia_login` role com BYPASSRLS + grants do §6.6
- `IdentityProvider` trait + `InternalProvider` impl seguindo §6.5 do ADR
- Login endpoint `/v1/auth/login` com dual-verify path
- Refresh endpoint `/v1/auth/refresh`
- Logout endpoint `/v1/auth/logout`
- Axum extractor `Principal` que sets `app.current_group_id` + `app.current_user_id` per request
- Suite de testes de authz cross-group (GAR-392 já filed)
- Migration tool integration com GAR-413 para `mobile_users` import

**Alternativa paralela viável:** **GAR-374 ADR 0004 Object Storage** (3-4h research) se você prefere desbloquear `garraia-storage` / files antes de auth. Mas o caminho de auth é mais valioso porque destrava toda a API REST do Group Workspace.

**Recomendação firme: GAR-391 imediatamente após GAR-375.**

---

**Aguardando sua aprovação.** Se aprovar como está, respondo as 7 open questions com os defaults recomendados e escrevo o ADR seguindo a estrutura §6. Sem código, sem migration, sem teste — só um documento decisional em `docs/adr/0005-identity-provider.md` + os 4 meta-arquivos updates listados.
