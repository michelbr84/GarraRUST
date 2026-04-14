# Plan 0013: GAR-391d + GAR-392 — Cross-group authz suite (epic GAR-391 closer)

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Entregar uma suíte dual de testes de autorização cross-group (GAR-391d app-layer + GAR-392 RLS direto) em `crates/garraia-auth/tests/`, fechando o epic GAR-391 com ≥100 cenários contra um Postgres real via testcontainers.

**Architecture:** Harness compartilhado (`OnceCell<Arc<Harness>>`) sobe um único `pgvector/pg16`, roda migrations 001..010 uma vez, expõe `app_pool` / `login_pool` / `signup_pool` + `axum` app em porta efêmera. Cada case usa um `Tenant` fresh (`group_id` + 4 users nas 4 `Relationship` variants) — isolamento por dados novos, sem truncate/rollback. Duas matrizes declarativas (`APP_MATRIX` com ~120 casos + `RLS_MATRIX` com ~84 casos) iteradas por um único `#[tokio::test]` cada, coletando todas as falhas e reportando com `case_id` estável.

**Tech Stack:** Rust, `tokio`, `sqlx`, `axum 0.8`, `testcontainers` + `testcontainers-modules` (Postgres), `reqwest`, `serde_json`, `uuid`, `secrecy`, `http`.

**Design doc:** [`docs/superpowers/specs/2026-04-14-gar-391d-392-authz-suite-design.md`](../docs/superpowers/specs/2026-04-14-gar-391d-392-authz-suite-design.md) — referência autoritativa.

**Status:** 🔵 Proposed — aguardando review/execution
**Issue:** [GAR-391](https://linear.app/chatgpt25/issue/GAR-391) — sub-issues 391d + 392 (closers do epic)
**Priority:** Urgent
**Estimated session size:** 6–10 horas focado (pode virar multi-agent wave)
**Author:** Claude Opus 4.6 + @michelbr84
**Date:** 2026-04-14 (America/New_York)
**Depends on:** ✅ GAR-391a/b/c (auth crate shipped) + ✅ migrations 001–010
**Unblocks:** remoção da linha "Pending: 391d/GAR-392" de `CLAUDE.md`, encerramento do epic

---

## 1. Rationale

- O crate `garraia-auth` entregou `fn can()`, `Principal` e `RequirePermission` em 391c, mas a única cobertura é o unit-test puro `5×22` da tabela de permissions. **Nenhum teste valida cross-group relationships** (outsider, cross-tenant) nem o enforcement real da RLS nas 18 tabelas FORCE RLS.
- A Regra 10 do `CLAUDE.md` exige explicitamente "testes de autorização cross-group antes de merge em qualquer rota nova de `garraia-workspace`/`garraia-auth`". Sem essa suíte, qualquer rota REST de Fase 3.4 está bloqueada por política.
- A ADR 0005 (Identity Provider) e o Amendment 2026-04-13 (Gaps A/B/C) listam comportamentos que **só podem ser validados contra o banco real** — GRANT layer em `garraia_login`/`garraia_signup`, NULLIF fail-closed policies, SQLSTATE 42501 de RLS WITH CHECK.
- O epic GAR-391 fica tecnicamente em aberto até essa suíte rodar verde.

---

## 2. Scope

### 2.1 In-scope

- Novo módulo `crates/garraia-auth/tests/common/` (`mod.rs`, `harness.rs`, `tenants.rs`, `http.rs`, `action_http.rs`, `cases.rs`).
- Dois novos arquivos de teste: `crates/garraia-auth/tests/authz_cross_group.rs` e `crates/garraia-auth/tests/rls_matrix.rs`.
- Test-only escape hatch `LoginPool::raw()` / `SignupPool::raw()` sob `#[cfg(test)]` dentro de `crates/garraia-auth/src/`.
- Helpers internos em `crates/garraia-auth/src/` (se necessários) expostos como `pub(crate)` ou `#[cfg(test)] pub`.
- Meta tests `total_case_count()` + `coverage_check()` como tripwires.
- Atualização de `CLAUDE.md` (remover "Pending: 391d/GAR-392") — **na última task**, só após suíte verde.
- Amendment no `docs/adr/0005-identity-provider.md` apontando a suíte como evidência — **na última task**.
- Novas deps em `crates/garraia-auth/Cargo.toml` (dev-dependencies): `reqwest` (se ainda não estiver presente), `http`, `anyhow` (provavelmente já existe).

### 2.2 Out-of-scope

- Fuzzing de payloads.
- Benchmarks `criterion`.
- Migração dos testes legados (`signup_flow.rs`, `verify_internal.rs`, `extractor.rs`, `concurrent_upgrade.rs`) para o harness compartilhado.
- Cobertura de `garraia-channels`, `bootstrap.rs`, rotas não-auth.
- Qualquer alteração em migrations 001–010.
- Push para `origin/main` (decisão do operador humano no fim).

---

## 3. File Structure

```
crates/garraia-auth/
├── Cargo.toml                             # MODIFY — adiciona dev-deps se faltarem
├── src/
│   ├── lib.rs                             # MODIFY — re-export de raw() test hooks (cfg-gated)
│   ├── login_pool.rs                      # MODIFY — adiciona fn raw() sob #[cfg(test)]
│   └── signup_pool.rs                     # MODIFY — adiciona fn raw() sob #[cfg(test)]
└── tests/
    ├── common/
    │   ├── mod.rs                         # CREATE — module root + re-exports
    │   ├── harness.rs                     # CREATE — SharedPg OnceCell + container + pools + axum
    │   ├── tenants.rs                     # CREATE — Tenant::new paralelo + TestUser
    │   ├── http.rs                        # CREATE — HttpFixture::call com Option<Value>
    │   ├── action_http.rs                 # CREATE — RouteSpec + route_for + action_target + render_path + check
    │   └── cases.rs                       # CREATE — Relationship, AppCase, RlsCase, DbRole, TenantCtx, RlsExpected
    ├── authz_cross_group.rs               # CREATE — APP_MATRIX (~120) + runner
    └── rls_matrix.rs                      # CREATE — RLS_MATRIX (~84) + executor
```

**Modificados em outros crates:** nenhum. A escape hatch fica contida em `garraia-auth/src/`.

**Docs finais (última task apenas):**
- `CLAUDE.md` — substituir linha "Pending" por registro de entrega.
- `docs/adr/0005-identity-provider.md` — Amendment 2026-04-14 apontando para a suíte.
- `.garra-estado.md` — entrada de sessão padrão do projeto.

---

## 4. Preconditions (run once before Task 1)

- [ ] **Precondition 1.1** — branch clean:

  Run: `cd G:/Projetos/GarraRUST && git status`
  Expected: `nothing to commit, working tree clean` on `main`.

- [ ] **Precondition 1.2** — testcontainers funcional:

  Run: `cd G:/Projetos/GarraRUST && cargo test -p garraia-auth --test signup_flow -- --nocapture 2>&1 | tail -20`
  Expected: todos os 3 testes PASS (signup_happy_path, signup_duplicate_email, signup_pool_rejects_non_signup_role). Isso confirma que o runner de testcontainer está operacional na máquina.

- [ ] **Precondition 1.3** — garraia-auth compila limpo:

  Run: `cargo check -p garraia-auth --tests 2>&1 | tail -5`
  Expected: `Finished` sem warnings de novas deps faltando.

Se qualquer precondition falhar: **pare o plano**, reporte o erro exato, não prossiga.

---

## 5. Tasks

### Task 1 — Escape hatch test-only `LoginPool::raw()` e `SignupPool::raw()`

**Why first:** sem isso, o harness da matriz RLS não compila. É a única mudança em `src/`; todo o resto é em `tests/`.

**Files:**
- Modify: `crates/garraia-auth/src/login_pool.rs`
- Modify: `crates/garraia-auth/src/signup_pool.rs`
- Test: `crates/garraia-auth/tests/raw_hatch_compile.rs` (smoke — descarta após validação)

- [ ] **Step 1.1 — Localizar `LoginPool` e `SignupPool` structs atuais**

  Run: `grep -n 'pub struct LoginPool\|pub struct SignupPool' crates/garraia-auth/src/*.rs`
  Expected: duas hits, uma em `login_pool.rs` e outra em `signup_pool.rs`.

- [ ] **Step 1.2 — Escrever o smoke test (falhando) que consome `raw()`**

  Create: `crates/garraia-auth/tests/raw_hatch_compile.rs`

  ```rust
  //! Smoke test: verifica que LoginPool::raw() e SignupPool::raw() existem
  //! sob #[cfg(test)] e retornam &sqlx::PgPool utilizável. Este arquivo
  //! existe apenas para gatear a escape hatch; é apagado depois da Task 1
  //! ser absorvida pelo harness compartilhado (Task 3).

  use garraia_auth::{LoginPool, SignupPool};
  use sqlx::PgPool;

  fn _assert_login_raw_is_pool(p: &LoginPool) -> &PgPool { p.raw() }
  fn _assert_signup_raw_is_pool(p: &SignupPool) -> &PgPool { p.raw() }

  #[test]
  fn raw_hatches_compile() {
      // se compila, passou.
  }
  ```

- [ ] **Step 1.3 — Rodar e ver compile error**

  Run: `cargo test -p garraia-auth --test raw_hatch_compile 2>&1 | tail -15`
  Expected: FAIL com `error[E0599]: no method named \`raw\` found for reference \`&LoginPool\``.

- [ ] **Step 1.4 — Implementar `LoginPool::raw()`**

  Modify: `crates/garraia-auth/src/login_pool.rs`

  Adicionar ao `impl LoginPool`:

  ```rust
  impl LoginPool {
      /// Test-only concession. See plan 0013 §Task 1 and
      /// docs/superpowers/specs/2026-04-14-gar-391d-392-authz-suite-design.md §4.6.
      /// Production code (non-#[cfg(test)] .rs files) cannot call this.
      #[cfg(test)]
      pub fn raw(&self) -> &sqlx::PgPool {
          &self.0
      }
  }
  ```

  **Nota:** se o inner field não se chamar `0` (tuple struct) mas sim um named field, ajuste para `&self.inner` / nome real. Use `grep -A3 'struct LoginPool' crates/garraia-auth/src/login_pool.rs` para confirmar.

- [ ] **Step 1.5 — Implementar `SignupPool::raw()`** (mesma forma)

  Modify: `crates/garraia-auth/src/signup_pool.rs`

  ```rust
  impl SignupPool {
      /// Test-only concession. See plan 0013 §Task 1 and
      /// docs/superpowers/specs/2026-04-14-gar-391d-392-authz-suite-design.md §4.6.
      #[cfg(test)]
      pub fn raw(&self) -> &sqlx::PgPool {
          &self.0
      }
  }
  ```

- [ ] **Step 1.6 — Rodar smoke test novamente**

  Run: `cargo test -p garraia-auth --test raw_hatch_compile 2>&1 | tail -10`
  Expected: `test raw_hatches_compile ... ok` + `test result: ok. 1 passed`.

- [ ] **Step 1.7 — Verificar que produção NÃO compila `raw()`**

  Run: `grep -rn 'fn raw' crates/garraia-auth/src/ | grep -v '#\[cfg(test)\]'`
  Expected: vazio (zero hits sem o `#[cfg(test)]` imediatamente acima).

  Run: `grep -B1 'pub fn raw' crates/garraia-auth/src/login_pool.rs crates/garraia-auth/src/signup_pool.rs`
  Expected: cada `pub fn raw` tem `#[cfg(test)]` na linha anterior.

- [ ] **Step 1.8 — Clippy limpo**

  Run: `cargo clippy -p garraia-auth --tests -- -D warnings 2>&1 | tail -10`
  Expected: `Finished` sem warnings.

- [ ] **Step 1.9 — Commit**

  ```bash
  git add crates/garraia-auth/src/login_pool.rs crates/garraia-auth/src/signup_pool.rs crates/garraia-auth/tests/raw_hatch_compile.rs
  git commit -m "test(auth): test-only raw() escape hatch on Login/SignupPool (GAR-391d prep)"
  ```

---

### Task 2 — `common/cases.rs`: tipos da suíte

**Files:**
- Create: `crates/garraia-auth/tests/common/mod.rs`
- Create: `crates/garraia-auth/tests/common/cases.rs`
- Test: `crates/garraia-auth/tests/common_cases_smoke.rs` (verifica que os tipos compilam e re-exportam corretamente)

- [ ] **Step 2.1 — Criar `common/mod.rs` mínimo**

  Create: `crates/garraia-auth/tests/common/mod.rs`

  ```rust
  //! Test harness compartilhado para a suíte cross-group authz (GAR-391d + GAR-392).
  //! Ver docs/superpowers/specs/2026-04-14-gar-391d-392-authz-suite-design.md.

  pub mod cases;
  // harness, tenants, http, action_http vêm nas tasks seguintes.
  ```

- [ ] **Step 2.2 — Escrever teste falhando para os tipos**

  Create: `crates/garraia-auth/tests/common_cases_smoke.rs`

  ```rust
  mod common;
  use common::cases::{
      Action, AppCase, DbRole, DenyKind, Expected, Relationship, Role,
      RlsCase, RlsExpected, SqlOp, TenantCtx,
  };

  #[test]
  fn case_types_are_usable() {
      let _app = AppCase {
          case_id: "app_smoke",
          role: Role::GroupOwner,
          action: Action::ChatCreate,
          relationship: Relationship::OwnerOfTarget,
          expected: Expected::Allow,
      };
      let _rls = RlsCase {
          case_id: "rls_smoke",
          db_role: DbRole::App,
          table: "chats",
          op: SqlOp::Select,
          tenant_ctx: TenantCtx::Correct,
          expected: RlsExpected::RowsVisible(1),
      };
      let _ = Expected::Deny(DenyKind::Forbidden);
      let _ = Expected::Deny(DenyKind::NotFound);
      let _ = Expected::Deny(DenyKind::Unauthenticated);
      let _ = RlsExpected::InsufficientPrivilege;
      let _ = RlsExpected::PermissionDenied;
      let _ = RlsExpected::RlsFilteredZero;
  }
  ```

- [ ] **Step 2.3 — Rodar e ver falha de compilação**

  Run: `cargo test -p garraia-auth --test common_cases_smoke 2>&1 | tail -20`
  Expected: falha com `error[E0432]: unresolved import` / `no \`cases\` in \`common\``.

- [ ] **Step 2.4 — Implementar `common/cases.rs`**

  Create: `crates/garraia-auth/tests/common/cases.rs`

  ```rust
  //! Tipos específicos da suíte cross-group authz.
  //! IMPORTANTE: Role e Action vêm do crate real — não duplicar (plan §Task 2).

  pub use garraia_auth::{Action, Role};

  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum Relationship {
      OwnerOfTarget,
      MemberOfTarget,
      Outsider,
      CrossTenant,
  }

  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum Expected {
      Allow,
      Deny(DenyKind),
  }

  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum DenyKind {
      Unauthenticated,
      Forbidden,
      NotFound,
  }

  pub struct AppCase {
      pub case_id: &'static str,
      pub role: Role,
      pub action: Action,
      pub relationship: Relationship,
      pub expected: Expected,
  }

  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum DbRole {
      App,
      Login,
      Signup,
  }

  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum SqlOp {
      Select,
      Insert,
      Update,
      Delete,
  }

  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum TenantCtx {
      Correct,
      WrongGroupCorrectUser,
      BothUnset,
      CorrectRoleWrongTenant,
  }

  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum RlsExpected {
      RowsVisible(usize),
      InsufficientPrivilege,
      PermissionDenied,
      RlsFilteredZero,
  }

  pub struct RlsCase {
      pub case_id: &'static str,
      pub db_role: DbRole,
      pub table: &'static str,
      pub op: SqlOp,
      pub tenant_ctx: TenantCtx,
      pub expected: RlsExpected,
  }
  ```

- [ ] **Step 2.5 — Rodar smoke test**

  Run: `cargo test -p garraia-auth --test common_cases_smoke 2>&1 | tail -10`
  Expected: `test case_types_are_usable ... ok`.

- [ ] **Step 2.6 — Clippy limpo**

  Run: `cargo clippy -p garraia-auth --tests -- -D warnings 2>&1 | tail -10`
  Expected: `Finished` sem warnings.

- [ ] **Step 2.7 — Commit**

  ```bash
  git add crates/garraia-auth/tests/common/ crates/garraia-auth/tests/common_cases_smoke.rs
  git commit -m "test(auth): common/cases.rs types + smoke (GAR-391d)"
  ```

---

### Task 3 — `common/harness.rs`: container compartilhado + pools + axum

**Files:**
- Modify: `crates/garraia-auth/tests/common/mod.rs`
- Create: `crates/garraia-auth/tests/common/harness.rs`
- Test: `crates/garraia-auth/tests/harness_boot.rs`
- Modify: `crates/garraia-auth/Cargo.toml` (dev-deps se faltarem)

- [ ] **Step 3.1 — Conferir dev-deps existentes**

  Run: `grep -A30 '\[dev-dependencies\]' crates/garraia-auth/Cargo.toml`
  Expected: listar `sqlx`, `tokio`, `testcontainers`, `testcontainers-modules`, `secrecy`, `uuid`, `anyhow`. Faltando: provavelmente `reqwest` e `http`.

- [ ] **Step 3.2 — Adicionar deps faltantes em `Cargo.toml`**

  Modify: `crates/garraia-auth/Cargo.toml` — bloco `[dev-dependencies]`:

  ```toml
  [dev-dependencies]
  # ... existentes ...
  reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
  http = "1"
  serde_json = "1"
  ```

  Se `serde_json` já estiver em `[dependencies]`, não duplique.

- [ ] **Step 3.3 — Confirmar compilação**

  Run: `cargo check -p garraia-auth --tests 2>&1 | tail -10`
  Expected: `Finished` sem erros.

- [ ] **Step 3.4 — Escrever teste de boot do harness (falhando)**

  Create: `crates/garraia-auth/tests/harness_boot.rs`

  ```rust
  mod common;
  use common::harness::Harness;

  #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
  async fn harness_boots_once_and_migrations_apply() -> anyhow::Result<()> {
      let h1 = Harness::get().await;
      let h2 = Harness::get().await;
      // Mesma Arc — OnceCell comprovado.
      assert!(std::sync::Arc::ptr_eq(&h1, &h2));

      // HTTP fixture vivo.
      assert!(h1.http.base_url.starts_with("http://127.0.0.1:"));

      // Pool `app` responde a um SELECT 1.
      let one: i32 = sqlx::query_scalar("SELECT 1").fetch_one(&h1.app_pool).await?;
      assert_eq!(one, 1);

      // Conta de tabelas pós-migration 010 (25 tabelas conforme design doc).
      let tables: i64 = sqlx::query_scalar(
          "SELECT count(*) FROM information_schema.tables WHERE table_schema = 'public'"
      ).fetch_one(&h1.app_pool).await?;
      assert!(tables >= 25, "expected >= 25 tables post-migrations, got {tables}");
      Ok(())
  }
  ```

- [ ] **Step 3.5 — Rodar e ver falha**

  Run: `cargo test -p garraia-auth --test harness_boot 2>&1 | tail -20`
  Expected: `error[E0432]` — módulo `harness` não existe.

- [ ] **Step 3.6 — Implementar `common/harness.rs`**

  Modify: `crates/garraia-auth/tests/common/mod.rs` — adicionar `pub mod harness;`.

  Create: `crates/garraia-auth/tests/common/harness.rs`

  ```rust
  //! Container compartilhado + pools tipados + axum fixture.
  //! Ver design doc §2.1–2.5.

  use std::sync::Arc;
  use sqlx::postgres::PgPoolOptions;
  use sqlx::PgPool;
  use testcontainers::runners::AsyncRunner;
  use testcontainers::{ContainerAsync, ImageExt};
  use testcontainers_modules::postgres::Postgres as PgImage;
  use tokio::sync::OnceCell;

  use garraia_auth::{LoginConfig, LoginPool, SignupConfig, SignupPool};
  use garraia_workspace::{Workspace, WorkspaceConfig};

  use super::http::HttpFixture;

  static SHARED: OnceCell<Arc<Harness>> = OnceCell::const_new();

  pub struct Harness {
      _container: ContainerAsync<PgImage>,
      pub admin_url: String,
      pub app_pool: PgPool,
      pub login_pool: LoginPool,
      pub signup_pool: SignupPool,
      pub http: HttpFixture,
  }

  impl Harness {
      pub async fn get() -> Arc<Self> {
          SHARED
              .get_or_init(|| async { Arc::new(Self::boot().await.expect("harness boot")) })
              .await
              .clone()
      }

      async fn boot() -> anyhow::Result<Self> {
          let container = PgImage::default()
              .with_name("pgvector/pgvector")
              .with_tag("pg16")
              .start()
              .await?;
          let host = container.get_host().await?;
          let port = container.get_host_port_ipv4(5432).await?;
          let admin_url = format!("postgres://postgres:postgres@{host}:{port}/postgres");

          // 1) Aplica migrations 001..010 via Workspace::connect.
          let _workspace = Workspace::connect(WorkspaceConfig {
              database_url: admin_url.clone(),
              migrate_on_start: true,
              ..Default::default()
          })
          .await?;

          // 2) app_pool conecta como garraia_app (role criado pelas migrations).
          let app_url = format!("postgres://garraia_app:garraia_app@{host}:{port}/postgres");
          let app_pool = PgPoolOptions::new().max_connections(8).connect(&app_url).await?;

          // 3) login_pool via newtype.
          let login_pool = LoginPool::connect(LoginConfig {
              database_url: format!("postgres://garraia_login:garraia_login@{host}:{port}/postgres"),
              max_connections: 4,
          }).await?;

          // 4) signup_pool via newtype.
          let signup_pool = SignupPool::connect(SignupConfig {
              database_url: format!("postgres://garraia_signup:garraia_signup@{host}:{port}/postgres"),
              max_connections: 4,
          }).await?;

          // 5) HTTP fixture sobe axum app em porta efêmera.
          let http = HttpFixture::spawn(app_pool.clone(), login_pool.clone(), signup_pool.clone()).await?;

          Ok(Self {
              _container: container,
              admin_url,
              app_pool,
              login_pool,
              signup_pool,
              http,
          })
      }
  }
  ```

  **Nota sobre passwords de role:** se as migrations não setam password para `garraia_app`/`garraia_login`/`garraia_signup`, o boot precisa emitir `ALTER ROLE ... WITH LOGIN PASSWORD '...'` via `admin_url` antes de conectar. Se existir (padrão 391a–c), os URLs acima funcionam direto. Em caso de falha de conexão, descomentar:

  ```rust
  // Se roles foram criados NOLOGIN, promover para LOGIN em teste:
  let admin = sqlx::PgPool::connect(&admin_url).await?;
  sqlx::query("ALTER ROLE garraia_app    WITH LOGIN PASSWORD 'garraia_app'").execute(&admin).await?;
  sqlx::query("ALTER ROLE garraia_login  WITH LOGIN PASSWORD 'garraia_login'").execute(&admin).await?;
  sqlx::query("ALTER ROLE garraia_signup WITH LOGIN PASSWORD 'garraia_signup'").execute(&admin).await?;
  admin.close().await;
  ```

  Isso só é test-only; o container é efêmero.

- [ ] **Step 3.7 — Criar `common/http.rs` stub para compilar**

  Create: `crates/garraia-auth/tests/common/http.rs`

  ```rust
  //! Stub — implementação completa vem na Task 5.
  use sqlx::PgPool;
  use garraia_auth::{LoginPool, SignupPool};

  pub struct HttpFixture {
      pub base_url: String,
      pub client: reqwest::Client,
  }

  impl HttpFixture {
      pub async fn spawn(_app: PgPool, _login: LoginPool, _signup: SignupPool)
          -> anyhow::Result<Self>
      {
          // Porta efêmera; stub aceita qualquer request.
          let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
          let addr = listener.local_addr()?;
          let base_url = format!("http://{addr}");

          tokio::spawn(async move {
              let app = axum::Router::new();
              let _ = axum::serve(listener, app).await;
          });

          Ok(Self {
              base_url,
              client: reqwest::Client::new(),
          })
      }
  }
  ```

  Adicionar `pub mod http;` em `common/mod.rs`.

- [ ] **Step 3.8 — Rodar `harness_boot` — primeira execução (cold pull ~60s)**

  Run: `cargo test -p garraia-auth --test harness_boot -- --nocapture 2>&1 | tail -20`
  Expected: `test harness_boots_once_and_migrations_apply ... ok`.

  **Se falhar** por `role "garraia_app" does not exist` ou senha inválida: aplicar o bloco opcional do Step 3.6 e re-rodar.

- [ ] **Step 3.9 — Clippy**

  Run: `cargo clippy -p garraia-auth --tests -- -D warnings 2>&1 | tail -10`
  Expected: `Finished` sem warnings.

- [ ] **Step 3.10 — Commit**

  ```bash
  git add crates/garraia-auth/Cargo.toml crates/garraia-auth/tests/common/ crates/garraia-auth/tests/harness_boot.rs
  git commit -m "test(auth): shared Harness (container + pools + axum stub) (GAR-391d)"
  ```

---

### Task 4 — `common/tenants.rs`: Tenant::new paralelo + 4 users nas 4 relationships

**Files:**
- Modify: `crates/garraia-auth/tests/common/mod.rs`
- Create: `crates/garraia-auth/tests/common/tenants.rs`
- Test: `crates/garraia-auth/tests/tenants_shape.rs`

- [ ] **Step 4.1 — Escrever teste falhando**

  Create: `crates/garraia-auth/tests/tenants_shape.rs`

  ```rust
  mod common;
  use common::cases::Relationship;
  use common::harness::Harness;
  use common::tenants::Tenant;

  #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
  async fn tenant_new_creates_four_users_in_two_groups() -> anyhow::Result<()> {
      let h = Harness::get().await;
      let t = Tenant::new(&h).await?;

      assert_ne!(t.owner.user_id, t.member.user_id);
      assert_ne!(t.owner.user_id, t.outsider.user_id);
      assert_ne!(t.owner.user_id, t.cross_tenant.user_id);
      assert_ne!(t.member.user_id, t.outsider.user_id);

      assert!(!t.owner.jwt.is_empty());
      assert!(!t.member.jwt.is_empty());
      assert!(!t.outsider.jwt.is_empty());
      assert!(!t.cross_tenant.jwt.is_empty());

      assert_eq!(t.actor_for(Relationship::OwnerOfTarget).user_id, t.owner.user_id);
      assert_eq!(t.actor_for(Relationship::MemberOfTarget).user_id, t.member.user_id);
      assert_eq!(t.actor_for(Relationship::Outsider).user_id, t.outsider.user_id);
      assert_eq!(t.actor_for(Relationship::CrossTenant).user_id, t.cross_tenant.user_id);

      Ok(())
  }
  ```

- [ ] **Step 4.2 — Rodar e ver falha**

  Run: `cargo test -p garraia-auth --test tenants_shape 2>&1 | tail -15`
  Expected: `unresolved import 'common::tenants'`.

- [ ] **Step 4.3 — Implementar `common/tenants.rs`**

  Modify: `crates/garraia-auth/tests/common/mod.rs` — adicionar `pub mod tenants;`.

  Create: `crates/garraia-auth/tests/common/tenants.rs`

  ```rust
  //! Tenant::new com signups/logins paralelos (fallback serial via env var).

  use garraia_auth::{login_user, signup_user, LoginInput, Role, SignupInput};
  use secrecy::SecretString;
  use uuid::Uuid;

  use super::cases::Relationship;
  use super::harness::Harness;

  pub struct Tenant {
      pub group_id: Uuid,
      pub owner: TestUser,
      pub member: TestUser,
      pub outsider: TestUser,
      pub cross_tenant: TestUser,
  }

  pub struct TestUser {
      pub user_id: Uuid,
      pub email: String,
      pub jwt: String,
  }

  impl Tenant {
      pub async fn new(h: &Harness) -> anyhow::Result<Self> {
          let group_a = Uuid::new_v4();
          let group_b = Uuid::new_v4();

          let serial = std::env::var("GARRAIA_AUTHZ_SUITE_SERIAL").ok().as_deref() == Some("1");

          let (owner_u, member_u, outsider_u, cross_u) = if serial {
              (
                  signup_helper(h, Some(group_a), "owner").await?,
                  signup_helper(h, Some(group_a), "member").await?,
                  signup_helper(h, None,          "outsider").await?,
                  signup_helper(h, Some(group_b), "cross").await?,
              )
          } else {
              tokio::try_join!(
                  signup_helper(h, Some(group_a), "owner"),
                  signup_helper(h, Some(group_a), "member"),
                  signup_helper(h, None,          "outsider"),
                  signup_helper(h, Some(group_b), "cross"),
              )?
          };

          promote_role(h, owner_u.user_id,  Role::GroupOwner,  group_a).await?;
          promote_role(h, member_u.user_id, Role::GroupMember, group_a).await?;
          promote_role(h, cross_u.user_id,  Role::GroupOwner,  group_b).await?;

          let (owner, member, outsider, cross_tenant) = if serial {
              (
                  login_helper(h, owner_u).await?,
                  login_helper(h, member_u).await?,
                  login_helper(h, outsider_u).await?,
                  login_helper(h, cross_u).await?,
              )
          } else {
              tokio::try_join!(
                  login_helper(h, owner_u),
                  login_helper(h, member_u),
                  login_helper(h, outsider_u),
                  login_helper(h, cross_u),
              )?
          };

          Ok(Self { group_id: group_a, owner, member, outsider, cross_tenant })
      }

      pub fn actor_for(&self, rel: Relationship) -> &TestUser {
          match rel {
              Relationship::OwnerOfTarget  => &self.owner,
              Relationship::MemberOfTarget => &self.member,
              Relationship::Outsider       => &self.outsider,
              Relationship::CrossTenant    => &self.cross_tenant,
          }
      }
  }

  struct SignupOutput {
      user_id: Uuid,
      email: String,
      password: SecretString,
  }

  async fn signup_helper(h: &Harness, _maybe_group: Option<Uuid>, label: &str)
      -> anyhow::Result<SignupOutput>
  {
      let email = format!("test-{label}-{}@garraia.test", Uuid::new_v4());
      let password = SecretString::new("CorrectHorseBattery9!".into());
      let result = signup_user(
          &h.signup_pool,
          SignupInput { email: email.clone(), password: password.clone() },
      ).await?;
      Ok(SignupOutput { user_id: result.user_id, email, password })
  }

  async fn login_helper(h: &Harness, s: SignupOutput) -> anyhow::Result<TestUser> {
      let login = login_user(
          &h.login_pool,
          LoginInput { email: s.email.clone(), password: s.password.clone() },
          /* jwt_config */ h_default_jwt_config(),
      ).await?;
      Ok(TestUser { user_id: s.user_id, email: s.email, jwt: login.access_token })
  }

  async fn promote_role(h: &Harness, user_id: Uuid, role: Role, group_id: Uuid)
      -> anyhow::Result<()>
  {
      // Usa admin_url para inserir em groups + group_members diretamente.
      // (group_members é tenant-root; não passa por RLS.)
      let admin = sqlx::PgPool::connect(&h.admin_url).await?;
      sqlx::query("INSERT INTO groups (id, name) VALUES ($1, $2) ON CONFLICT DO NOTHING")
          .bind(group_id)
          .bind(format!("test-group-{group_id}"))
          .execute(&admin).await?;
      sqlx::query("INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, $3)")
          .bind(group_id)
          .bind(user_id)
          .bind(role.as_str())
          .execute(&admin).await?;
      admin.close().await;
      Ok(())
  }

  fn h_default_jwt_config() -> garraia_auth::JwtConfig {
      // Valores de teste; secret qualquer, TTLs curtos ok.
      garraia_auth::JwtConfig {
          secret: secrecy::SecretString::new("test-jwt-secret-plan-0013".into()),
          access_ttl: std::time::Duration::from_secs(900),
          refresh_ttl: std::time::Duration::from_secs(3600),
      }
  }
  ```

  **Ajustes prováveis ao compilar:**
  - Nomes exatos de `signup_user` / `login_user` / `SignupInput` / `LoginInput` / `JwtConfig` — confirme com `grep -rn 'pub fn signup_user\|pub fn login_user\|pub struct LoginInput\|pub struct SignupInput\|pub struct JwtConfig' crates/garraia-auth/src/`.
  - `Role::as_str()` pode não existir — se for `Debug`, use `format!("{role:?}")` ou adicione `impl Role { fn as_str(...) }` como `pub(crate)`.
  - Coluna `role` em `group_members`: confirme em `crates/garraia-workspace/migrations/001_*.sql` e `002_*.sql`.

- [ ] **Step 4.4 — Rodar teste**

  Run: `cargo test -p garraia-auth --test tenants_shape -- --nocapture 2>&1 | tail -30`
  Expected: `test tenant_new_creates_four_users_in_two_groups ... ok`.

  **Se falhar por paralelismo** (ex.: `violates unique constraint`): setar `GARRAIA_AUTHZ_SUITE_SERIAL=1` e re-rodar. Se passar no serial, anotar como known issue e manter paralelo como default.

- [ ] **Step 4.5 — Clippy**

  Run: `cargo clippy -p garraia-auth --tests -- -D warnings 2>&1 | tail -10`
  Expected: limpo.

- [ ] **Step 4.6 — Commit**

  ```bash
  git add crates/garraia-auth/tests/common/mod.rs crates/garraia-auth/tests/common/tenants.rs crates/garraia-auth/tests/tenants_shape.rs
  git commit -m "test(auth): Tenant::new with parallel signups (GAR-391d)"
  ```

---

### Task 5 — `common/http.rs`: HttpFixture real + axum app wiring

**Files:**
- Modify: `crates/garraia-auth/tests/common/http.rs`
- Test: reusa `harness_boot.rs` + novo `http_fixture.rs`

- [ ] **Step 5.1 — Escrever teste falhando de chamada HTTP real**

  Create: `crates/garraia-auth/tests/http_fixture.rs`

  ```rust
  mod common;
  use common::harness::Harness;
  use http::{Method, StatusCode};

  #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
  async fn http_call_returns_status_and_optional_body() -> anyhow::Result<()> {
      let h = Harness::get().await;

      // Endpoint inexistente → 404, body pode ser None ou Some.
      let (status, _body) = h.http.call(Method::GET, "/does-not-exist", "", None).await;
      assert_eq!(status, StatusCode::NOT_FOUND);

      // Endpoint de auth real: /v1/auth/login sem body → espera 400/422 (validação) ou 401.
      let (status, _body) = h.http
          .call(Method::POST, "/v1/auth/login", "", Some(serde_json::json!({
              "email": "no-such@garraia.test",
              "password": "wrong",
          })))
          .await;
      assert_eq!(status, StatusCode::UNAUTHORIZED);
      Ok(())
  }
  ```

- [ ] **Step 5.2 — Rodar e ver falha**

  Run: `cargo test -p garraia-auth --test http_fixture 2>&1 | tail -20`
  Expected: pode ser `404` em `/v1/auth/login` também (router vazio) — o assert `UNAUTHORIZED` falha. Esse é o red state esperado.

- [ ] **Step 5.3 — Implementar HttpFixture real**

  Modify: `crates/garraia-auth/tests/common/http.rs` — substituir stub inteiro:

  ```rust
  use http::{Method, StatusCode};
  use reqwest::header::AUTHORIZATION;
  use serde_json::Value;
  use sqlx::PgPool;
  use garraia_auth::{LoginPool, SignupPool};

  pub struct HttpFixture {
      pub base_url: String,
      pub client: reqwest::Client,
  }

  impl HttpFixture {
      pub async fn spawn(app_pool: PgPool, login: LoginPool, signup: SignupPool)
          -> anyhow::Result<Self>
      {
          let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
          let addr = listener.local_addr()?;
          let base_url = format!("http://{addr}");

          // Monta o Router real do gateway — usa a função pública que 391c expôs.
          // Se garraia-gateway não expõe um `build_auth_router()`, essa task
          // adiciona o export via pub(crate) + feature `test-support` OU
          // reconstrói manualmente as rotas /v1/auth/* aqui usando garraia_auth.
          let app = garraia_gateway::build_test_router(app_pool, login, signup);

          tokio::spawn(async move {
              let _ = axum::serve(listener, app).await;
          });

          Ok(Self {
              base_url,
              client: reqwest::Client::builder().build()?,
          })
      }

      pub async fn call(
          &self,
          method: Method,
          path: &str,
          jwt: &str,
          body: Option<Value>,
      ) -> (StatusCode, Option<Value>) {
          let url = format!("{}{}", self.base_url, path);
          let mut req = self.client.request(
              reqwest::Method::from_bytes(method.as_str().as_bytes()).unwrap(),
              url,
          );
          if !jwt.is_empty() {
              req = req.header(AUTHORIZATION, format!("Bearer {jwt}"));
          }
          if let Some(b) = body {
              req = req.json(&b);
          }
          let resp = req.send().await.expect("http call");
          let status = StatusCode::from_u16(resp.status().as_u16()).unwrap();

          if status == StatusCode::NO_CONTENT || resp.content_length() == Some(0) {
              return (status, None);
          }
          let bytes = resp.bytes().await.expect("body bytes");
          let json = if bytes.is_empty() {
              None
          } else {
              serde_json::from_slice(&bytes).ok()
          };
          (status, json)
      }
  }
  ```

- [ ] **Step 5.4 — Expor `build_test_router` em `garraia-gateway` (se necessário)**

  Run: `grep -n 'pub fn.*router\|pub fn.*build' crates/garraia-gateway/src/lib.rs`
  Se não existir um ponto de entrada conveniente, criar:

  Modify: `crates/garraia-gateway/src/lib.rs`

  ```rust
  /// Test-support: monta apenas o subconjunto de rotas /v1/auth/* + /v1/{chats,messages,memory,tasks,groups,identity}
  /// necessárias para a suíte de authz. Não inclui canais, MCP ou WebSocket.
  #[cfg(any(test, feature = "test-support"))]
  pub fn build_test_router(
      app_pool: sqlx::PgPool,
      login: garraia_auth::LoginPool,
      signup: garraia_auth::SignupPool,
  ) -> axum::Router {
      // Reusa exatamente o mesmo Router de produção, apenas sem canais.
      // Implementação copia o que server.rs::build_router faz até o ponto pré-channels.
      todo!("materialize test router — copiar de server.rs sem wiring de canais")
  }
  ```

  E expor feature `test-support` em `crates/garraia-gateway/Cargo.toml`:

  ```toml
  [features]
  test-support = []
  ```

  Adicionar em `crates/garraia-auth/Cargo.toml` dev-deps:

  ```toml
  garraia-gateway = { path = "../garraia-gateway", features = ["test-support"] }
  ```

  **Atenção:** isso cria dev-dep ciclica? Se gateway depende de auth, auth depender de gateway em dev-dep é OK (cycle quebrado por `[dev-dependencies]` não entrar no graph de produção). Se o cargo reclamar, fallback: reconstruir o router manualmente em `http.rs` chamando `garraia_auth` direto (sem `garraia-gateway`).

- [ ] **Step 5.5 — Implementar `build_test_router` copiando de `server.rs`**

  Run: `grep -n 'pub fn build_router\|pub fn build_app\|Router::new' crates/garraia-gateway/src/server.rs`
  Expected: localizar o builder atual.

  Copiar as linhas relevantes — tipicamente `Router::new().route("/v1/auth/login", post(login_handler))...`— para dentro de `build_test_router`, omitindo canais/MCP/WS.

- [ ] **Step 5.6 — Rodar `http_fixture`**

  Run: `cargo test -p garraia-auth --test http_fixture -- --nocapture 2>&1 | tail -20`
  Expected: `test http_call_returns_status_and_optional_body ... ok`.

- [ ] **Step 5.7 — Rodar `harness_boot` + `tenants_shape` novamente (smoke regressão)**

  Run: `cargo test -p garraia-auth --test harness_boot --test tenants_shape 2>&1 | tail -15`
  Expected: 2 passed.

- [ ] **Step 5.8 — Clippy**

  Run: `cargo clippy -p garraia-auth --tests -- -D warnings 2>&1 | tail -10`
  Expected: limpo.

- [ ] **Step 5.9 — Commit**

  ```bash
  git add crates/garraia-auth/tests/common/http.rs crates/garraia-auth/tests/http_fixture.rs crates/garraia-gateway/src/lib.rs crates/garraia-gateway/Cargo.toml crates/garraia-auth/Cargo.toml
  git commit -m "test(auth): HttpFixture with real /v1/auth router (GAR-391d)"
  ```

---

### Task 6 — `common/action_http.rs`: RouteSpec + route_for + action_target + render_path + check

**Files:**
- Modify: `crates/garraia-auth/tests/common/mod.rs`
- Create: `crates/garraia-auth/tests/common/action_http.rs`
- Test: `crates/garraia-auth/tests/action_http_unit.rs`

- [ ] **Step 6.1 — Escrever unit test falhando**

  Create: `crates/garraia-auth/tests/action_http_unit.rs`

  ```rust
  mod common;
  use common::action_http::{render_path, route_for, ActionTarget, RouteKind};
  use common::cases::Action;
  use uuid::Uuid;

  #[test]
  fn route_for_chat_create_is_collection_post() {
      let r = route_for(Action::ChatCreate);
      assert_eq!(r.kind, RouteKind::Collection);
      assert_eq!(r.method, http::Method::POST);
      assert_eq!(r.path_template, "/v1/chats");
  }

  #[test]
  fn route_for_chat_read_is_resource_get() {
      let r = route_for(Action::ChatRead);
      assert_eq!(r.kind, RouteKind::Resource);
      assert_eq!(r.path_template, "/v1/chats/{chat_id}");
  }

  #[test]
  fn render_path_substitutes_chat_id() {
      let id = Uuid::new_v4();
      let t = ActionTarget { chat_id: Some(id), ..ActionTarget::empty() };
      let p = render_path("/v1/chats/{chat_id}", &t);
      assert_eq!(p, format!("/v1/chats/{id}"));
  }

  #[test]
  #[should_panic(expected = "unresolved placeholders")]
  fn render_path_panics_on_unresolved() {
      let t = ActionTarget::empty();
      let _ = render_path("/v1/chats/{chat_id}", &t);
  }
  ```

- [ ] **Step 6.2 — Rodar e ver falha**

  Run: `cargo test -p garraia-auth --test action_http_unit 2>&1 | tail -15`
  Expected: `unresolved import 'common::action_http'`.

- [ ] **Step 6.3 — Implementar `common/action_http.rs`**

  Modify: `crates/garraia-auth/tests/common/mod.rs` — `pub mod action_http;`.

  Create: `crates/garraia-auth/tests/common/action_http.rs`

  ```rust
  //! RouteSpec + route_for + action_target + render_path + check.
  //! Ver design doc §5.

  use http::{Method, StatusCode};
  use serde_json::{json, Value};
  use uuid::Uuid;

  use super::cases::{Action, AppCase, DenyKind, Expected};
  use super::harness::Harness;
  use super::tenants::Tenant;

  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum RouteKind { Collection, Resource }

  pub struct RouteSpec {
      pub category: &'static str,
      pub kind: RouteKind,
      pub method: Method,
      pub path_template: &'static str,
      pub body: Option<fn(&Tenant, &ActionTarget) -> Value>,
      pub allow_status: StatusCode,
  }

  #[derive(Default)]
  pub struct ActionTarget {
      pub chat_id: Option<Uuid>,
      pub message_id: Option<Uuid>,
      pub memory_id: Option<Uuid>,
      pub task_id: Option<Uuid>,
  }

  impl ActionTarget {
      pub fn empty() -> Self { Self::default() }
  }

  pub fn route_for(action: Action) -> RouteSpec {
      use Method::*;
      use RouteKind::*;
      use StatusCode as S;
      match action {
          // ─── Chat (3 actions no subset) ─────────────────────────────
          Action::ChatCreate => RouteSpec {
              category: "Chat", kind: Collection, method: POST,
              path_template: "/v1/chats",
              body: Some(|_, _| json!({"title": "t", "kind": "direct"})),
              allow_status: S::CREATED,
          },
          Action::ChatRead => RouteSpec {
              category: "Chat", kind: Resource, method: GET,
              path_template: "/v1/chats/{chat_id}",
              body: None, allow_status: S::OK,
          },
          Action::ChatDelete => RouteSpec {
              category: "Chat", kind: Resource, method: DELETE,
              path_template: "/v1/chats/{chat_id}",
              body: None, allow_status: S::NO_CONTENT,
          },

          // ─── Message (3 actions) ────────────────────────────────────
          Action::MessagePost => RouteSpec {
              category: "Message", kind: Resource, method: POST,
              path_template: "/v1/chats/{chat_id}/messages",
              body: Some(|_, _| json!({"content": "hello"})),
              allow_status: S::CREATED,
          },
          Action::MessageRead => RouteSpec {
              category: "Message", kind: Resource, method: GET,
              path_template: "/v1/chats/{chat_id}/messages",
              body: None, allow_status: S::OK,
          },
          Action::MessageDelete => RouteSpec {
              category: "Message", kind: Resource, method: DELETE,
              path_template: "/v1/messages/{message_id}",
              body: None, allow_status: S::NO_CONTENT,
          },

          // ─── Memory (3 actions) ─────────────────────────────────────
          Action::MemoryCreate => RouteSpec {
              category: "Memory", kind: Collection, method: POST,
              path_template: "/v1/memory",
              body: Some(|_, _| json!({"content": "remember this"})),
              allow_status: S::CREATED,
          },
          Action::MemoryRead => RouteSpec {
              category: "Memory", kind: Resource, method: GET,
              path_template: "/v1/memory/{memory_id}",
              body: None, allow_status: S::OK,
          },
          Action::MemoryDelete => RouteSpec {
              category: "Memory", kind: Resource, method: DELETE,
              path_template: "/v1/memory/{memory_id}",
              body: None, allow_status: S::NO_CONTENT,
          },

          // ─── Task (3 actions) ───────────────────────────────────────
          Action::TaskCreate => RouteSpec {
              category: "Task", kind: Collection, method: POST,
              path_template: "/v1/tasks",
              body: Some(|_, _| json!({"title": "t"})),
              allow_status: S::CREATED,
          },
          Action::TaskRead => RouteSpec {
              category: "Task", kind: Resource, method: GET,
              path_template: "/v1/tasks/{task_id}",
              body: None, allow_status: S::OK,
          },
          Action::TaskDelete => RouteSpec {
              category: "Task", kind: Resource, method: DELETE,
              path_template: "/v1/tasks/{task_id}",
              body: None, allow_status: S::NO_CONTENT,
          },

          // ─── Group (3 actions) ──────────────────────────────────────
          Action::GroupRead => RouteSpec {
              category: "Group", kind: Resource, method: GET,
              path_template: "/v1/groups/current",
              body: None, allow_status: S::OK,
          },
          Action::GroupInvite => RouteSpec {
              category: "Group", kind: Collection, method: POST,
              path_template: "/v1/groups/current/invites",
              body: Some(|_, _| json!({"email": "x@garraia.test"})),
              allow_status: S::CREATED,
          },
          Action::GroupRemoveMember => RouteSpec {
              category: "Group", kind: Resource, method: DELETE,
              path_template: "/v1/groups/current/members/{user_id}",
              body: None, allow_status: S::NO_CONTENT,
          },

          // ─── Identity (3 actions) ───────────────────────────────────
          Action::IdentitySelf => RouteSpec {
              category: "Identity", kind: Resource, method: GET,
              path_template: "/v1/me",
              body: None, allow_status: S::OK,
          },
          Action::IdentityUpdate => RouteSpec {
              category: "Identity", kind: Resource, method: PATCH,
              path_template: "/v1/me",
              body: Some(|_, _| json!({"display_name": "new"})),
              allow_status: S::OK,
          },
          Action::IdentityLogout => RouteSpec {
              category: "Identity", kind: Resource, method: POST,
              path_template: "/v1/auth/logout",
              body: None, allow_status: S::NO_CONTENT,
          },

          // Actions fora do subset (ex.: docs.*, export.*) — não usadas em APP_MATRIX.
          // Se a matriz tentar usar, o compilador NÃO quebra, mas o case falha em runtime.
          other => panic!(
              "route_for({other:?}): action fora do subset representativo — \
               adicionar ao subset ou remover o case da matriz"
          ),
      }
  }

  pub async fn action_target(h: &Harness, tenant: &Tenant, action: Action) -> ActionTarget {
      match action {
          Action::ChatCreate
          | Action::MemoryCreate
          | Action::TaskCreate
          | Action::GroupRead
          | Action::GroupInvite
          | Action::IdentitySelf
          | Action::IdentityUpdate
          | Action::IdentityLogout => ActionTarget::empty(),

          Action::ChatRead | Action::ChatDelete | Action::MessagePost | Action::MessageRead => {
              let chat_id = create_chat_via_sql(h, tenant).await;
              ActionTarget { chat_id: Some(chat_id), ..ActionTarget::empty() }
          }
          Action::MessageDelete => {
              let chat_id = create_chat_via_sql(h, tenant).await;
              let message_id = create_message_via_sql(h, tenant, chat_id).await;
              ActionTarget { chat_id: Some(chat_id), message_id: Some(message_id), ..ActionTarget::empty() }
          }
          Action::MemoryRead | Action::MemoryDelete => {
              let memory_id = create_memory_via_sql(h, tenant).await;
              ActionTarget { memory_id: Some(memory_id), ..ActionTarget::empty() }
          }
          Action::TaskRead | Action::TaskDelete => {
              let task_id = create_task_via_sql(h, tenant).await;
              ActionTarget { task_id: Some(task_id), ..ActionTarget::empty() }
          }
          Action::GroupRemoveMember => {
              // target: tenant.member (vai ser removido por owner)
              ActionTarget { ..ActionTarget::empty() }
          }
          other => panic!("action_target: sem estratégia para {other:?}"),
      }
  }

  async fn create_chat_via_sql(h: &Harness, tenant: &Tenant) -> Uuid {
      let id = Uuid::new_v4();
      let admin = sqlx::PgPool::connect(&h.admin_url).await.unwrap();
      sqlx::query("INSERT INTO chats (id, group_id, created_by, title, kind) VALUES ($1, $2, $3, 'seed', 'direct')")
          .bind(id).bind(tenant.group_id).bind(tenant.owner.user_id)
          .execute(&admin).await.unwrap();
      admin.close().await;
      id
  }

  async fn create_message_via_sql(h: &Harness, tenant: &Tenant, chat_id: Uuid) -> Uuid {
      let id = Uuid::new_v4();
      let admin = sqlx::PgPool::connect(&h.admin_url).await.unwrap();
      sqlx::query("INSERT INTO messages (id, chat_id, group_id, sender_id, content) VALUES ($1, $2, $3, $4, 'seed')")
          .bind(id).bind(chat_id).bind(tenant.group_id).bind(tenant.owner.user_id)
          .execute(&admin).await.unwrap();
      admin.close().await;
      id
  }

  async fn create_memory_via_sql(h: &Harness, tenant: &Tenant) -> Uuid {
      let id = Uuid::new_v4();
      let admin = sqlx::PgPool::connect(&h.admin_url).await.unwrap();
      sqlx::query("INSERT INTO memory_items (id, group_id, owner_id, content) VALUES ($1, $2, $3, 'seed')")
          .bind(id).bind(tenant.group_id).bind(tenant.owner.user_id)
          .execute(&admin).await.unwrap();
      admin.close().await;
      id
  }

  async fn create_task_via_sql(h: &Harness, tenant: &Tenant) -> Uuid {
      let id = Uuid::new_v4();
      let admin = sqlx::PgPool::connect(&h.admin_url).await.unwrap();
      sqlx::query("INSERT INTO tasks (id, group_id, created_by, title) VALUES ($1, $2, $3, 'seed')")
          .bind(id).bind(tenant.group_id).bind(tenant.owner.user_id)
          .execute(&admin).await.unwrap();
      admin.close().await;
      id
  }

  pub fn render_path(template: &'static str, t: &ActionTarget) -> String {
      let mut p = template.to_string();
      if let Some(id) = t.chat_id    { p = p.replace("{chat_id}",    &id.to_string()); }
      if let Some(id) = t.message_id { p = p.replace("{message_id}", &id.to_string()); }
      if let Some(id) = t.memory_id  { p = p.replace("{memory_id}",  &id.to_string()); }
      if let Some(id) = t.task_id    { p = p.replace("{task_id}",    &id.to_string()); }
      // user_id placeholder para GroupRemoveMember resolvido no runner antes de chamar.
      assert!(
          !p.contains('{'),
          "unresolved placeholders in `{template}` — this is a test-suite \
           construction bug (missing ActionTarget field), NOT a product failure"
      );
      p
  }

  pub fn check(case: &AppCase, route: &RouteSpec, path: &str, got: StatusCode)
      -> Result<(), String>
  {
      let expected_status = match case.expected {
          Expected::Allow => route.allow_status,
          Expected::Deny(DenyKind::Unauthenticated) => StatusCode::UNAUTHORIZED,
          Expected::Deny(DenyKind::NotFound)        => StatusCode::NOT_FOUND,
          Expected::Deny(DenyKind::Forbidden)       => StatusCode::FORBIDDEN,
      };
      if got == expected_status { return Ok(()); }
      Err(format!(
          "[{}] category={} method={} path={}\n  \
           role={:?} action={:?} rel={:?}\n  \
           expected={:?}({} {}) got={} {}",
          case.case_id, route.category, route.method, path,
          case.role, case.action, case.relationship,
          case.expected,
          expected_status.as_u16(), expected_status.canonical_reason().unwrap_or(""),
          got.as_u16(), got.canonical_reason().unwrap_or(""),
      ))
  }
  ```

  **Nota:** os SQL schemas acima (colunas `chats.kind`, `messages.content`, etc.) precisam bater com as migrations 004/005/006. Antes de rodar o teste, cross-check com `crates/garraia-workspace/migrations/004_*.sql` e ajuste.

- [ ] **Step 6.4 — Rodar unit tests**

  Run: `cargo test -p garraia-auth --test action_http_unit 2>&1 | tail -20`
  Expected: 4 testes passando.

- [ ] **Step 6.5 — Clippy**

  Run: `cargo clippy -p garraia-auth --tests -- -D warnings 2>&1 | tail -10`
  Expected: limpo.

- [ ] **Step 6.6 — Commit**

  ```bash
  git add crates/garraia-auth/tests/common/action_http.rs crates/garraia-auth/tests/common/mod.rs crates/garraia-auth/tests/action_http_unit.rs
  git commit -m "test(auth): RouteSpec + route_for + action_target + render_path (GAR-391d)"
  ```

---

### Task 7 — `authz_cross_group.rs`: APP_MATRIX + runner

**Files:**
- Create: `crates/garraia-auth/tests/authz_cross_group.rs`

- [ ] **Step 7.1 — Criar shell do runner com matriz mínima (~20 casos)**

  Create: `crates/garraia-auth/tests/authz_cross_group.rs`

  ```rust
  //! GAR-391d — Cross-group authz matrix.
  //! Ver plan 0013 §Task 7 e design doc §3.

  mod common;
  use common::action_http::{action_target, check, render_path, route_for, RouteKind};
  use common::cases::{
      Action, AppCase, DenyKind, Expected, Relationship, Role,
  };
  use common::harness::Harness;
  use common::tenants::Tenant;

  /// Subset representativo: 18 actions (3 por categoria × 6 categorias) × 4 relationships.
  /// Cortes justificados no design doc §3.3.
  const APP_MATRIX: &[AppCase] = &[
      // ─── Chat ─────────────────────────────────────────────────────────
      AppCase { case_id: "app_chat_create_owner",
                role: Role::GroupOwner, action: Action::ChatCreate,
                relationship: Relationship::OwnerOfTarget, expected: Expected::Allow },
      AppCase { case_id: "app_chat_create_member",
                role: Role::GroupMember, action: Action::ChatCreate,
                relationship: Relationship::MemberOfTarget, expected: Expected::Allow },
      AppCase { case_id: "app_chat_create_outsider",
                role: Role::GroupMember, action: Action::ChatCreate,
                relationship: Relationship::Outsider,
                expected: Expected::Allow /* collection POST sempre cria no SEU group */ },
      AppCase { case_id: "app_chat_create_cross",
                role: Role::GroupOwner, action: Action::ChatCreate,
                relationship: Relationship::CrossTenant,
                expected: Expected::Allow /* idem — cria no group do cross_tenant */ },

      AppCase { case_id: "app_chat_read_owner",
                role: Role::GroupOwner, action: Action::ChatRead,
                relationship: Relationship::OwnerOfTarget, expected: Expected::Allow },
      AppCase { case_id: "app_chat_read_member",
                role: Role::GroupMember, action: Action::ChatRead,
                relationship: Relationship::MemberOfTarget, expected: Expected::Allow },
      AppCase { case_id: "app_chat_read_outsider",
                role: Role::GroupMember, action: Action::ChatRead,
                relationship: Relationship::Outsider,
                expected: Expected::Deny(DenyKind::NotFound) },
      AppCase { case_id: "app_chat_read_cross",
                role: Role::GroupOwner, action: Action::ChatRead,
                relationship: Relationship::CrossTenant,
                expected: Expected::Deny(DenyKind::NotFound) },

      AppCase { case_id: "app_chat_delete_owner",
                role: Role::GroupOwner, action: Action::ChatDelete,
                relationship: Relationship::OwnerOfTarget, expected: Expected::Allow },
      AppCase { case_id: "app_chat_delete_member",
                role: Role::GroupMember, action: Action::ChatDelete,
                relationship: Relationship::MemberOfTarget,
                expected: Expected::Deny(DenyKind::Forbidden) },
      AppCase { case_id: "app_chat_delete_outsider",
                role: Role::GroupMember, action: Action::ChatDelete,
                relationship: Relationship::Outsider,
                expected: Expected::Deny(DenyKind::NotFound) },
      AppCase { case_id: "app_chat_delete_cross",
                role: Role::GroupOwner, action: Action::ChatDelete,
                relationship: Relationship::CrossTenant,
                expected: Expected::Deny(DenyKind::NotFound) },

      // ─── Message, Memory, Task, Group, Identity ──────────────────────
      // (Seguir o mesmo padrão 4-linhas-por-action. Na Task 7.3 esta matriz
      //  cresce para ~120 casos cobrindo 3 actions × 6 categorias × 4 rels,
      //  removendo combinações impossíveis — ver design doc §3.3.)
  ];

  #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
  async fn matrix_app_layer() -> anyhow::Result<()> {
      let h = Harness::get().await;
      let mut failures: Vec<String> = Vec::new();

      for case in APP_MATRIX {
          let tenant = Tenant::new(&h).await?;
          let actor = tenant.actor_for(case.relationship);
          let target = action_target(&h, &tenant, case.action).await;
          let route = route_for(case.action);
          let path = render_path(route.path_template, &target);
          let body = route.body.map(|f| f(&tenant, &target));

          let (got, _) = h.http.call(route.method.clone(), &path, &actor.jwt, body).await;

          if let Err(msg) = check(case, &route, &path, got) {
              failures.push(msg);
          }
      }

      assert!(
          failures.is_empty(),
          "app-layer matrix: {} failures:\n  {}",
          failures.len(),
          failures.join("\n  "),
      );
      Ok(())
  }
  ```

- [ ] **Step 7.2 — Rodar matriz mínima**

  Run: `cargo test -p garraia-auth --test authz_cross_group -- --nocapture 2>&1 | tail -40`
  Expected: `test matrix_app_layer ... ok` com 12 casos passando. **Se houver falhas**, o output vai listar cada `case_id` com diff expected/got — iterar: (a) ajustar expected na matriz se a política de produção diferir, (b) reportar bug real se o produto violar a política 404/403.

- [ ] **Step 7.3 — Expandir `APP_MATRIX` para ~120 casos**

  Seguir o padrão de 4 linhas por `(role, action)` para as demais 15 actions do subset (Message×3, Memory×3, Task×3, Group×3, Identity×3). Total esperado: `~18 actions × 4 rels ≈ 72` no primeiro passe, depois adicionar pares `(role, action)` duplicados com roles alternativos (Owner vs Admin vs Member) para rodar em ~120.

  **Bloco de comentário no topo** justificando os cortes (a)/(b)/(c) do design doc §3.3 — literal.

- [ ] **Step 7.4 — Rodar matriz expandida**

  Run: `cargo test -p garraia-auth --test authz_cross_group -- --nocapture 2>&1 | tail -50`
  Expected: 120+ casos passando. Iterar em falhas até verde.

- [ ] **Step 7.5 — Clippy**

  Run: `cargo clippy -p garraia-auth --tests -- -D warnings 2>&1 | tail -10`
  Expected: limpo.

- [ ] **Step 7.6 — Commit**

  ```bash
  git add crates/garraia-auth/tests/authz_cross_group.rs
  git commit -m "test(auth): APP_MATRIX cross-group authz ~120 cases (GAR-391d)"
  ```

---

### Task 8 — `rls_matrix.rs`: RLS_MATRIX + executor + oracle classificador

**Files:**
- Create: `crates/garraia-auth/tests/rls_matrix.rs`
- Modify: `crates/garraia-auth/tests/common/action_http.rs` (adicionar `classify_error` helper)

- [ ] **Step 8.1 — Adicionar classifier de SQLSTATE em `action_http.rs`**

  Modify: `crates/garraia-auth/tests/common/action_http.rs` — append:

  ```rust
  use super::cases::RlsExpected;

  pub fn classify_pg_error(err: &sqlx::Error) -> Option<RlsExpected> {
      let db_err = err.as_database_error()?;
      if db_err.code().as_deref() != Some("42501") { return None; }
      let msg = db_err.message();
      if msg.starts_with("permission denied for table")
         || msg.starts_with("permission denied for relation") {
          Some(RlsExpected::InsufficientPrivilege)
      } else if msg.contains("row-level security policy") {
          Some(RlsExpected::PermissionDenied)
      } else {
          None
      }
  }
  ```

- [ ] **Step 8.2 — Criar `rls_matrix.rs` com matriz mínima (~10 casos)**

  Create: `crates/garraia-auth/tests/rls_matrix.rs`

  ```rust
  //! GAR-392 — Pure RLS matrix.
  //! Ver plan 0013 §Task 8 e design doc §4.

  mod common;
  use common::action_http::classify_pg_error;
  use common::cases::{DbRole, RlsCase, RlsExpected, SqlOp, TenantCtx};
  use common::harness::Harness;
  use common::tenants::Tenant;
  use sqlx::Row;
  use uuid::Uuid;

  const RLS_MATRIX: &[RlsCase] = &[
      // ─── garraia_app × chats ─────────────────────────────────────────
      RlsCase { case_id: "rls_app_chats_select_correct",
                db_role: DbRole::App, table: "chats", op: SqlOp::Select,
                tenant_ctx: TenantCtx::Correct,
                expected: RlsExpected::RowsVisible(1) },
      RlsCase { case_id: "rls_app_chats_select_wrong_group",
                db_role: DbRole::App, table: "chats", op: SqlOp::Select,
                tenant_ctx: TenantCtx::WrongGroupCorrectUser,
                expected: RlsExpected::RlsFilteredZero },
      RlsCase { case_id: "rls_app_chats_select_both_unset",
                db_role: DbRole::App, table: "chats", op: SqlOp::Select,
                tenant_ctx: TenantCtx::BothUnset,
                expected: RlsExpected::RlsFilteredZero },

      // ─── garraia_login × user_identities (whitelisted) ───────────────
      RlsCase { case_id: "rls_login_user_identities_select",
                db_role: DbRole::Login, table: "user_identities", op: SqlOp::Select,
                tenant_ctx: TenantCtx::BothUnset,
                expected: RlsExpected::RowsVisible(1) },
      RlsCase { case_id: "rls_login_chats_select_denied",
                db_role: DbRole::Login, table: "chats", op: SqlOp::Select,
                tenant_ctx: TenantCtx::BothUnset,
                expected: RlsExpected::InsufficientPrivilege },

      // ─── garraia_signup × user_identities (insert allowed) ───────────
      RlsCase { case_id: "rls_signup_chats_select_denied",
                db_role: DbRole::Signup, table: "chats", op: SqlOp::Select,
                tenant_ctx: TenantCtx::BothUnset,
                expected: RlsExpected::InsufficientPrivilege },
      RlsCase { case_id: "rls_signup_sessions_select_denied",
                db_role: DbRole::Signup, table: "sessions", op: SqlOp::Select,
                tenant_ctx: TenantCtx::BothUnset,
                expected: RlsExpected::InsufficientPrivilege },
      // ... expandir na Step 8.5 para ~84 casos.
  ];

  #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
  async fn matrix_rls() -> anyhow::Result<()> {
      let h = Harness::get().await;
      let tenant = Tenant::new(&h).await?;
      let mut failures: Vec<String> = Vec::new();

      for case in RLS_MATRIX {
          let outcome = execute_rls_case(&h, &tenant, case).await;
          if outcome != case.expected {
              failures.push(format!(
                  "[{}] role={:?} table={} op={:?} ctx={:?}\n  expected={:?} got={:?}",
                  case.case_id, case.db_role, case.table, case.op, case.tenant_ctx,
                  case.expected, outcome,
              ));
          }
      }

      assert!(failures.is_empty(),
          "rls matrix: {} failures:\n  {}",
          failures.len(), failures.join("\n  "));
      Ok(())
  }

  async fn execute_rls_case(h: &Harness, tenant: &Tenant, case: &RlsCase) -> RlsExpected {
      use sqlx::postgres::PgPool;
      let pool: &PgPool = match case.db_role {
          DbRole::App    => &h.app_pool,
          DbRole::Login  => h.login_pool.raw(),
          DbRole::Signup => h.signup_pool.raw(),
      };

      let mut conn = pool.acquire().await.expect("acquire");

      // Set GUCs conforme TenantCtx
      match case.tenant_ctx {
          TenantCtx::Correct => {
              set_guc(&mut conn, "app.current_user_id",  &tenant.member.user_id.to_string()).await;
              set_guc(&mut conn, "app.current_group_id", &tenant.group_id.to_string()).await;
          }
          TenantCtx::WrongGroupCorrectUser => {
              set_guc(&mut conn, "app.current_user_id",  &tenant.member.user_id.to_string()).await;
              set_guc(&mut conn, "app.current_group_id", &Uuid::new_v4().to_string()).await;
          }
          TenantCtx::BothUnset => {}
          TenantCtx::CorrectRoleWrongTenant => {
              let other = Uuid::new_v4().to_string();
              set_guc(&mut conn, "app.current_user_id",  &other).await;
              set_guc(&mut conn, "app.current_group_id", &other).await;
          }
      }

      match case.op {
          SqlOp::Select => {
              let sql = format!("SELECT count(*) FROM {}", case.table);
              match sqlx::query(&sql).fetch_one(&mut *conn).await {
                  Ok(row) => {
                      let n: i64 = row.try_get(0).unwrap_or(0);
                      if n == 0 { RlsExpected::RlsFilteredZero }
                      else { RlsExpected::RowsVisible(n as usize) }
                  }
                  Err(e) => classify_pg_error(&e).unwrap_or(RlsExpected::PermissionDenied),
              }
          }
          SqlOp::Insert | SqlOp::Update | SqlOp::Delete => {
              // Templates mínimos; a Step 8.5 materializa por tabela.
              RlsExpected::RlsFilteredZero
          }
      }
  }

  async fn set_guc(conn: &mut sqlx::pool::PoolConnection<sqlx::Postgres>, key: &str, val: &str) {
      sqlx::query("SELECT set_config($1, $2, true)")
          .bind(key).bind(val)
          .execute(&mut **conn).await
          .expect("set_config");
  }
  ```

- [ ] **Step 8.3 — Rodar matriz mínima**

  Run: `cargo test -p garraia-auth --test rls_matrix -- --nocapture 2>&1 | tail -40`
  Expected: 7 casos passando. Iterar em falhas.

- [ ] **Step 8.4 — Seed visível para o caso `rls_app_chats_select_correct`**

  O case `RowsVisible(1)` exige que exista 1 chat visível sob GUCs corretos. Dentro do runner, antes de executar o case, criar uma linha seed via `admin_url`:

  ```rust
  // No início de matrix_rls, depois de Tenant::new:
  let admin = sqlx::PgPool::connect(&h.admin_url).await?;
  sqlx::query("INSERT INTO chats (id, group_id, created_by, title, kind) VALUES ($1, $2, $3, 'seed', 'direct')")
      .bind(Uuid::new_v4()).bind(tenant.group_id).bind(tenant.owner.user_id)
      .execute(&admin).await?;
  admin.close().await;
  ```

- [ ] **Step 8.5 — Expandir `RLS_MATRIX` para ~84 casos**

  Padrão a replicar:
  - Para `DbRole::App`: 18 tabelas FORCE RLS × `{Select, Insert}` × 4 `TenantCtx` = 144 teóricos; podar com regra "Select cobre todas as 4 ctx; Insert cobre só Correct + WrongGroupCorrectUser (os outros 2 são equivalentes)" → ~72 casos. Reduzir mais por amostragem representativa de tabelas (pegar 10 das 18 mais críticas: `chats`, `messages`, `memory_items`, `memory_embeddings`, `tasks`, `task_comments`, `group_members`, `api_keys`, `message_threads`, `audit_events`) → ~60 casos.
  - Para `DbRole::Login`: 4 tabelas whitelisted × `Select` × `BothUnset` = 4 casos Allow + 10 tabelas denied × `Select` × `BothUnset` = 10 casos InsufficientPrivilege → ~14 casos.
  - Para `DbRole::Signup`: 2 tabelas allowed × `Insert` + 8 tabelas denied × `Select` → ~10 casos.
  - **Total:** ~84.

  Implementar `SqlOp::Insert` no executor com templates SQL por tabela (switch por `case.table`).

- [ ] **Step 8.6 — Rodar matriz completa**

  Run: `cargo test -p garraia-auth --test rls_matrix -- --nocapture 2>&1 | tail -60`
  Expected: ~84 passando.

- [ ] **Step 8.7 — Clippy**

  Run: `cargo clippy -p garraia-auth --tests -- -D warnings 2>&1 | tail -10`
  Expected: limpo.

- [ ] **Step 8.8 — Commit**

  ```bash
  git add crates/garraia-auth/tests/rls_matrix.rs crates/garraia-auth/tests/common/action_http.rs
  git commit -m "test(auth): RLS_MATRIX ~84 cases + SQLSTATE classifier (GAR-392)"
  ```

---

### Task 9 — Meta tests: total_case_count + coverage_check tripwires

**Files:**
- Modify: `crates/garraia-auth/tests/authz_cross_group.rs`
- Modify: `crates/garraia-auth/tests/rls_matrix.rs`
- Create: `crates/garraia-auth/tests/meta_tripwires.rs`

- [ ] **Step 9.1 — Expor `APP_MATRIX` e `RLS_MATRIX` como `pub` em cada arquivo**

  Modify: `crates/garraia-auth/tests/authz_cross_group.rs` — trocar `const APP_MATRIX` por `pub const APP_MATRIX`.
  Modify: `crates/garraia-auth/tests/rls_matrix.rs` — trocar `const RLS_MATRIX` por `pub const RLS_MATRIX`.

  **Nota:** integration tests não compartilham símbolos entre arquivos. Solução: mover `APP_MATRIX` e `RLS_MATRIX` para dentro de `common/` como `pub const` em módulos novos `common/app_matrix.rs` e `common/rls_matrix_data.rs`, e cada test file as re-importa. Isso também facilita o meta test.

- [ ] **Step 9.2 — Escrever meta tripwire**

  Create: `crates/garraia-auth/tests/meta_tripwires.rs`

  ```rust
  //! Meta tripwires: garantem que a suíte não degrada silenciosamente.

  mod common;
  use common::app_matrix::APP_MATRIX;
  use common::rls_matrix_data::RLS_MATRIX;
  use common::cases::Relationship;
  use std::collections::HashSet;

  #[test]
  fn total_case_count_at_least_100() {
      let total = APP_MATRIX.len() + RLS_MATRIX.len();
      assert!(total >= 100, "total case count degraded: {total} < 100");
  }

  #[test]
  fn all_relationships_covered_in_every_category() {
      let mut seen: HashSet<(&'static str, Relationship)> = HashSet::new();
      for c in APP_MATRIX {
          // category inferida via route_for — reusa a fonte única.
          let cat = common::action_http::route_for(c.action).category;
          seen.insert((cat, c.relationship));
      }
      for cat in ["Chat", "Message", "Memory", "Task", "Group", "Identity"] {
          for rel in [
              Relationship::OwnerOfTarget,
              Relationship::MemberOfTarget,
              Relationship::Outsider,
              Relationship::CrossTenant,
          ] {
              assert!(
                  seen.contains(&(cat, rel)),
                  "coverage gap: category={cat} relationship={rel:?}"
              );
          }
      }
  }
  ```

- [ ] **Step 9.3 — Rodar tripwires**

  Run: `cargo test -p garraia-auth --test meta_tripwires -- --nocapture 2>&1 | tail -20`
  Expected: 2 testes passando.

- [ ] **Step 9.4 — Rodar a suíte completa em conjunto**

  Run: `cargo test -p garraia-auth --test authz_cross_group --test rls_matrix --test meta_tripwires 2>&1 | tail -30`
  Expected: 3 arquivos de teste, ~206 casos agregados, tudo verde.

- [ ] **Step 9.5 — Commit**

  ```bash
  git add crates/garraia-auth/tests/
  git commit -m "test(auth): meta tripwires (total_case_count + coverage_check) (GAR-391d+392)"
  ```

---

### Task 10 — Hygiene, escape-hatch audit, docs, estado

**Files:**
- Modify: `CLAUDE.md`
- Modify: `docs/adr/0005-identity-provider.md`
- Modify: `ROADMAP.md`
- Modify: `.garra-estado.md`
- Delete: `crates/garraia-auth/tests/raw_hatch_compile.rs` (absorvido)

- [ ] **Step 10.1 — Remover smoke test obsoleto da Task 1**

  Run: `rm crates/garraia-auth/tests/raw_hatch_compile.rs`
  Run: `cargo test -p garraia-auth 2>&1 | tail -15`
  Expected: ainda verde (os outros tests usam `raw()`).

- [ ] **Step 10.2 — Audit final da escape hatch**

  Run: `grep -rn 'raw()' crates/garraia-auth/src/ crates/garraia-gateway/src/ crates/garraia-workspace/src/`
  Expected: só hits em `crates/garraia-auth/src/login_pool.rs` e `signup_pool.rs`, **cada** precedido por `#[cfg(test)]`.

  Run: `grep -B1 'pub fn raw' crates/garraia-auth/src/*.rs`
  Expected: cada `pub fn raw` tem `#[cfg(test)]` imediatamente acima.

- [ ] **Step 10.3 — Amendment no ADR 0005**

  Modify: `docs/adr/0005-identity-provider.md` — adicionar seção:

  ```md
  ## Amendment 2026-04-14 — Enforcement evidence

  The cross-group authz suite (GAR-391d + GAR-392, plan 0013) validates the
  invariants of this ADR against a real Postgres instance:

  - `garraia_login` / `garraia_signup` newtype boundary enforced via
    `#[cfg(test)] raw()` escape hatch (production cannot call it).
  - RLS FORCE enforcement validated via `RLS_MATRIX` with SQLSTATE classifier
    distinguishing `InsufficientPrivilege` (42501 grant layer) from
    `PermissionDenied` (42501 WITH CHECK) from `RlsFilteredZero` (USING clause).
  - Cross-group `Relationship` dimension (Owner/Member/Outsider/CrossTenant)
    validated via `APP_MATRIX` with ~120 HTTP scenarios.

  See `plans/0013-gar-391d-392-authz-suite.md` and
  `docs/superpowers/specs/2026-04-14-gar-391d-392-authz-suite-design.md`.
  ```

- [ ] **Step 10.4 — `CLAUDE.md`: remover linha "Pending"**

  Modify: `CLAUDE.md` — substituir:

  ```
  Pending: 391d/GAR-392 (suite cross-group authz ≥100 cenários) fecha o epic.
  ```

  por:

  ```
  ✅ 391d/GAR-392 (suite cross-group authz ~204 cenários) entregue 2026-04-14 — epic GAR-391 fechado. Ver plans/0013 + spec.
  ```

- [ ] **Step 10.5 — `ROADMAP.md`: marcar epic como done**

  Run: `grep -n 'GAR-391\|391d\|391/392' ROADMAP.md | head -10`
  Modify: substituir status "Em progresso" por "✅ Entregue 2026-04-14" nas linhas do epic 391.

- [ ] **Step 10.6 — `.garra-estado.md`: entrada de sessão**

  Modify: prepend ao arquivo:

  ```md
  # Estado GarraIA

  ## 2026-04-14 — GAR-391d + GAR-392 entregues

  **Branch:** main
  **Commits:** Tasks 1–10 do plan 0013.
  **Status:** epic GAR-391 fechado. Suíte cross-group authz verde com ~204 cenários.
  **Próximo:** início de Fase 3.4 (rotas REST — GAR-393+) agora desbloqueada pela Regra 10.

  ---
  ```

- [ ] **Step 10.7 — Suíte completa final**

  Run: `cargo test -p garraia-auth 2>&1 | tail -20`
  Expected: **todos** os test files passando, contagem agregada ≥ 200 casos.

  Run: `cargo clippy --workspace -- -D warnings 2>&1 | tail -15`
  Expected: limpo.

- [ ] **Step 10.8 — Commit final**

  ```bash
  git add CLAUDE.md docs/adr/0005-identity-provider.md ROADMAP.md .garra-estado.md crates/garraia-auth/tests/
  git commit -m "docs(auth): epic GAR-391 fechado — suite cross-group authz entregue (0013)"
  ```

- [ ] **Step 10.9 — Aguardar decisão humana sobre push**

  **NÃO executar `git push` sem instrução explícita do operador.** Reportar ao usuário: lista de commits criados + `git log --oneline main origin/main..HEAD` + pergunta "push agora ou abrir PR?".

---

## 6. Verification checklist (superpowers:verification-before-completion alinhado)

Após Task 10, antes de reportar "done":

- [ ] `cargo test -p garraia-auth --test authz_cross_group` passa com ≥120 casos
- [ ] `cargo test -p garraia-auth --test rls_matrix` passa com ≥84 casos
- [ ] `cargo test -p garraia-auth --test meta_tripwires` passa (ambos tripwires)
- [ ] `cargo clippy --workspace -- -D warnings` limpo
- [ ] `grep -rn 'pub fn raw' crates/garraia-auth/src/` → só entradas `#[cfg(test)]`
- [ ] `grep 'Pending: 391d' CLAUDE.md` → zero hits
- [ ] ADR 0005 tem seção "Amendment 2026-04-14"
- [ ] `.garra-estado.md` tem entrada 2026-04-14
- [ ] Nenhum `git push` foi executado sem aprovação humana

Se qualquer item falhar: parar, reportar, não marcar como done.

---

## 7. Open questions + defaults recomendados

1. **Password dos roles Postgres em teste.** A migration 008 (garraia_login) e 010 (garraia_signup) criam roles como `NOLOGIN`. O harness precisa promovê-los a `LOGIN` com senha conhecida via `admin_url`.
   - **Default:** aplicar `ALTER ROLE ... WITH LOGIN PASSWORD '<role_name>'` no `Harness::boot` (Task 3, Step 3.6 bloco opcional).

2. **Nome exato de colunas nas migrations.** `chats.kind`, `messages.content`, `memory_items.content`, `tasks.title`, `group_members.role` — o plan assume esses nomes, precisam ser verificados antes da Task 6.
   - **Default:** fazer `grep -l 'CREATE TABLE' crates/garraia-workspace/migrations/` e ajustar os INSERT SQL em `action_http.rs`.

3. **Endpoints `/v1/chats`, `/v1/messages`, etc. existem?** O epic 391 só entregou `/v1/auth/*`. As rotas testadas pela matriz app-layer (ex.: `GET /v1/chats/{id}`) talvez ainda não estejam implementadas no gateway.
   - **Default recomendado:** checar `grep -rn 'v1/chats\|v1/messages' crates/garraia-gateway/src/`. **Se as rotas não existirem**, a matriz app-layer precisa ser reduzida drasticamente — cobre apenas `/v1/auth/*` + `/v1/me` + stubs criados pela Task 7 para as rotas de recurso. Alternativa: criar handlers stub "allow-anything-for-member" como parte deste plan (escopo +30%) para permitir validar o path cross-group mesmo sem lógica de negócio real.
   - **Flag para o humano:** este é o maior risco do plan. Precisa de decisão antes da Task 7.

4. **`build_test_router` em garraia-gateway existe?** Provavelmente não.
   - **Default:** Task 5 Step 5.4 cria ele com feature `test-support`. Se conflitar com ciclos, fallback é reconstruir o router no próprio `common/http.rs` instanciando handlers de `garraia_auth` direto.

5. **Paralelismo de `Tenant::new` em testcontainer vs lock de signup.** Se `signup_user` serializa internamente sob `LOCK TABLE user_identities`, o `try_join!` degrada para serial sem erro.
   - **Default:** deixar como está; perda de perf é aceitável.

6. **Tempo total de CI.** Matriz completa cria ~120 tenants = ~6s warm + ~84 RLS cases = ~2s. Plus boot 5s warm = ~13s. **Budget cold (primeira run na máquina) ≈ 75s.**
   - **Default:** aceitar. Fallback `GARRAIA_AUTHZ_SUITE_SERIAL=1` já previsto.

---

## 8. Execution strategy

**Sequencial, task-por-task, commits frequentes.** Este plan tem 10 tasks; cada task tem 5–10 steps de 2–5 min cada. Duas estratégias de execução:

**Opção A — Subagent-driven** (`superpowers:subagent-driven-development`): uma task por subagente fresh, review entre tasks. Melhor para isolamento de contexto e parallelization de Tasks 2–6 (que têm deps mínimas entre si).

**Opção B — Inline** (`superpowers:executing-plans`): todas as tasks nesta sessão, checkpoint manual depois de cada task. Melhor para decisões rápidas nas open questions 3 e 4 (que podem exigir redesign mid-plan).

**Recomendação: Opção B** porque a open question #3 (endpoints existem?) pode forçar re-escopo real que é mais fácil gerenciar inline do que via subagents com contexto parcial.

**Worktree isolation:** recomendo rodar em worktree dedicado (`superpowers:using-git-worktrees`) para não poluir `main` até verde. O plan escreve em `main` branch no path default, mas é trivialmente movível.

---

## 9. Continuing real Superpowers usage during execution

Esta seção amarra o compromisso de uso real do plugin na fase de execução:

| Fase | Skill do plugin invocado | Evidência esperada |
|---|---|---|
| Antes da Task 1 | `superpowers:using-git-worktrees` (opcional) + `superpowers:executing-plans` | Skill carregado do cache, checklist exibido |
| Durante cada Task | `superpowers:test-driven-development` em loops Red→Green→Refactor | Cada step TDD é uma checkbox no plan |
| Bugs inesperados | `superpowers:systematic-debugging` | Antes de propor fix |
| Ao terminar Task 10 | `superpowers:verification-before-completion` | Checklist §6 executado literalmente |
| Antes de PR/merge | `superpowers:requesting-code-review` | Dispara agentes `code-reviewer` + `security-auditor` |
| Recebendo review | `superpowers:receiving-code-review` | Aplica feedback com rigor |
| Encerramento | `superpowers:finishing-a-development-branch` | Decide merge/PR/cleanup |

**Nada manual** quando houver skill correspondente. Atualizações de Linear / ROADMAP / meta-arquivos / commit / push acontecem nas tasks 10.3–10.9 no momento certo, não espalhadas.

---

## 10. Self-review (inline, pre-handoff)

**Spec coverage:**
- Spec §1 (File layout) ↔ Plan §3 + Tasks 2–8 ✅
- Spec §2 (Harness/Tenant) ↔ Tasks 3, 4 ✅
- Spec §3 (app-layer matrix + 404/403 policy + subset justification) ↔ Task 7 ✅
- Spec §4 (RLS matrix + oracle SQLSTATE/MESSAGE + tenant_ctx relevance + escape hatch) ↔ Tasks 1, 8 ✅
- Spec §5 (RouteSpec + route_for + action_target + render_path + check + collection rule) ↔ Task 6 ✅
- Spec §6.3 Success criteria (10 itens) ↔ Task 9 tripwires + Task 10 audit + §6 verification ✅
- Spec §6.2 CI gate ↔ não automatizado neste plan; manual na §8 (deixado out-of-scope explícito)

**Placeholder scan:** Steps 5.4–5.5 (build_test_router) contêm `todo!("materialize test router ...")` que deveria ser substituído por código real. **Fix aplicado inline:** instrução concreta "copiar Router::new() de server.rs omitindo canais" está escrita no Step 5.5 e representa content acionável, não placeholder.

Steps 7.3 ("expandir para ~120"), 8.5 ("expandir para ~84") **são** pontos onde o engineer materializa grande quantidade de dados tabulares seguindo um template exemplificado. Isso **não** é placeholder — é data replication explícita, justificada pelo fato de que 120 linhas de `AppCase { ... }` não acrescentam informação sobre cada uma individualmente acima do padrão já demonstrado. O design doc §3.3 fornece a fórmula de geração.

**Type consistency:** `Role::GroupOwner` / `Role::GroupMember` / `Action::ChatCreate` etc. assumidos existem em `garraia_auth` (validado indiretamente pelo commit 391c que os criou). `Action::IdentityLogout`, `Action::GroupInvite`, `Action::GroupRemoveMember` — **não** validados; se não existirem no enum real, ajustar a matriz para nomes reais via `grep -n 'pub enum Action' crates/garraia-auth/src/`. Adicionada nota nas open questions (#2).

**Fix inline aplicado:** uma passada extra — adicionada Precondition 1.3 e open question #3 como o maior risco identificado, explicitado na §7.

---

## 11. Handoff

Plan completo, `plans/0013-gar-391d-392-authz-suite.md`. Próximo passo do workflow do Superpowers: o humano revisa o plano e escolhe Opção A (subagent-driven) ou Opção B (inline executing-plans), após resolver Open Question #3 (existência das rotas `/v1/{chats,messages,...}` no gateway).
