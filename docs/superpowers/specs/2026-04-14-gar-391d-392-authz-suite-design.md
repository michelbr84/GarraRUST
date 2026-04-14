# GAR-391d + GAR-392 — Cross-Group Authz Suite (Design)

**Status:** Accepted — 2026-04-14 (America/New_York)
**Owners:** `garraia-auth`, `garraia-workspace`
**Closes:** epic GAR-391
**Related:** ADR 0005 (Identity Provider), `CLAUDE.md` Rules 10 / 11 / 12 / 13
**Approved approach:** table-driven harness compartilhado, dual matrix (app-layer + RLS)
**Process:** produced via `superpowers:brainstorming` skill (9-step checklist, HARD-GATE respected)

---

## 0. Objective

Close the GAR-391 epic by delivering a cross-group authorization test suite that exercises, against a real Postgres instance, **two explicit layers**:

1. **App-layer matrix (GAR-391d)** — HTTP requests through the real `axum` router, the `Principal` extractor, and `RequirePermission`, validating `fn can()` decisions across the new `Relationship` dimension.
2. **RLS matrix (GAR-392)** — direct SQL against the three dedicated Postgres roles (`garraia_app`, `garraia_login`, `garraia_signup`), validating that database-layer enforcement is correct regardless of the Rust code above it.

Both matrices share one testcontainer, one harness module, and one set of tenant fixtures. They live side-by-side in `crates/garraia-auth/tests/`.

---

## 1. File layout

```
crates/garraia-auth/tests/
├── common/
│   ├── mod.rs              # re-exports
│   ├── harness.rs          # SharedPg (OnceCell<Arc<Harness>>), container boot, migrations 001..010
│   ├── tenants.rs          # Tenant::new(&harness) → cria group + 4 users nas 4 relationships
│   ├── http.rs             # spawn do axum app em porta efêmera, client, wrappers
│   ├── action_http.rs      # route_for(action) + action_target() + render_path()
│   └── cases.rs            # tipos específicos da suite (importa Role/Action do crate real)
├── authz_cross_group.rs    # matriz app-layer (GAR-391d)   — ~120 casos
└── rls_matrix.rs           # matriz RLS pura   (GAR-392)   — ~84 casos
```

**Princípios aplicados:**

- **Separação lógica entre matrizes.** `authz_cross_group.rs` e `rls_matrix.rs` são arquivos distintos, cada um com sua própria `const MATRIX`. Compartilham `common/harness.rs` mas **nunca** `const`s entre si.
- **Isolamento de detalhes frágeis em `common/`.** Portas, URLs, IDs de Postgres role, IDs de group seed ficam dentro de `Tenant` / `Harness`. As matrizes só conhecem enums (`Role::GroupOwner`, `Action::ChatMessagePost`, `Relationship::CrossTenant`, etc.).
- **Reuso da infra existente.** `harness.rs` aproveita o pattern já validado em `crates/garraia-auth/tests/signup_flow.rs` e `crates/garraia-workspace/tests/migration_smoke.rs` (`testcontainers` + `pgvector/pgvector:pg16`). A única adição é o `OnceCell<Arc<Harness>>` para compartilhamento de container entre cenários.
- **Sem duplicação de tipos de domínio.** `common/cases.rs` faz `pub use garraia_auth::{Role, Action}` — zero drift entre a suite e o código de produção. A suite adiciona **apenas** `Relationship`, `Expected`, `DenyKind`, `AppCase`, `RlsCase`, `DbRole`, `SqlOp`, `TenantCtx`, `RlsExpected`.
- **Testes legados permanecem intocados.** `signup_flow.rs`, `verify_internal.rs`, `extractor.rs`, `concurrent_upgrade.rs` continuam subindo container próprio. Motivo: eles testam signup/upgrade com state mutável de schema (migrations parciais, dummy hash), incompatível com um container compartilhado pós-migration.

---

## 2. Harness e Tenant (API interna de `common/`)

### 2.1 `common/harness.rs`

```rust
use std::sync::Arc;
use tokio::sync::OnceCell;

static SHARED: OnceCell<Arc<Harness>> = OnceCell::const_new();

pub struct Harness {
    _container: ContainerAsync<PgImage>,   // mantido vivo pelo Arc
    pub admin_url: String,                  // superuser — só migrations + setup
    pub app_pool: PgPool,                   // role garraia_app (RLS-enforced)
    pub login_pool: LoginPool,              // newtype sobre role garraia_login
    pub signup_pool: SignupPool,            // newtype sobre role garraia_signup
    pub http: HttpFixture,                  // axum app em porta efêmera
}

impl Harness {
    /// Idempotente. Primeira chamada: boot container, aplica migrations 001..010,
    /// cria os 3 pools tipados, sobe o axum app. Demais chamadas: retorna o mesmo Arc.
    pub async fn get() -> Arc<Self> {
        SHARED.get_or_init(Self::boot).await.clone()
    }
}
```

### 2.2 Estratégia de isolamento — regra escrita

> O `Harness` é **compartilhado por todo o processo de teste**. Ele **nunca** é resetado, truncado ou rolado-back entre cenários. O isolamento entre casos vem exclusivamente de **dados novos por cenário**: cada case chama `Tenant::new(&harness).await`, que cria um `group_id` UUID fresh, 4 users novos com emails UUID (`test-{uuid}@garraia.test`), e todos os dados subsequentes (chats, messages, memory, tasks) são criados dentro desse group. Como 18 tabelas estão sob `FORCE RLS` com `app.current_group_id`, um tenant nunca vê o outro — é o mesmo mecanismo que produção usa.
>
> **Não há `TRUNCATE`, não há `ROLLBACK`, não há schema drop.** Se um caso falha deixando lixo, o lixo fica lá e o próximo caso ignora porque tem `group_id` diferente. Fidelidade > microperformance, por escolha consciente (Brainstorming Q3 → opção B).

### 2.3 `common/tenants.rs`

```rust
pub struct Tenant {
    pub group_id: Uuid,
    pub owner:        TestUser,  // Role::GroupOwner
    pub member:       TestUser,  // Role::GroupMember
    pub outsider:     TestUser,  // autenticado, mas sem membership
    pub cross_tenant: TestUser,  // owner de OUTRO group
}

pub struct TestUser {
    pub user_id: Uuid,
    pub email: String,
    pub password: SecretString,
    pub jwt: String,   // access token pronto para `Authorization: Bearer ...`
}

impl Tenant {
    pub async fn new(h: &Harness) -> Self {
        let group_a = Uuid::new_v4();
        let group_b = Uuid::new_v4();

        // 4 signups em paralelo — emails UUID, zero colisão, zero ordering implícita.
        let (owner_u, member_u, outsider_u, cross_u) = tokio::try_join!(
            signup(h, Some(group_a), "owner"),
            signup(h, Some(group_a), "member"),
            signup(h, None,          "outsider"),
            signup(h, Some(group_b), "cross"),
        ).expect("tenant signup");

        // Promoções de role — sequenciais, mesma tabela group_members.
        promote(h, &owner_u,  Role::GroupOwner,  group_a).await;
        promote(h, &member_u, Role::GroupMember, group_a).await;
        promote(h, &cross_u,  Role::GroupOwner,  group_b).await;

        // 4 logins em paralelo (cada um abre sua própria transação de lazy-upgrade).
        let (owner, member, outsider, cross_tenant) = tokio::try_join!(
            login(h, owner_u),
            login(h, member_u),
            login(h, outsider_u),
            login(h, cross_u),
        ).expect("tenant login");

        Self { group_id: group_a, owner, member, outsider, cross_tenant }
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
```

### 2.4 Paralelização de `Tenant::new` — política oficial

- **Modo padrão:** `tokio::try_join!` para os 4 signups e para os 4 logins. Emails são `test-{uuid}@garraia.test`, distintos por construção — zero race em `UNIQUE(email)`. As promoções entre signups e logins são sequenciais (trivialmente correto, diferença desprezível).
- **Modo serial (fallback oficial, não gambiarra):** quando a env var `GARRAIA_AUTHZ_SUITE_SERIAL=1` está setada, `Tenant::new` troca os `try_join!` por `.await` em série. Comportamento **permitido e documentado**; CI pode ativar sem recompilar se detectar flakiness. Ambas as modalidades são tratadas como primeira-classe.

### 2.5 `common/http.rs` — comportamento de bodies

```rust
pub struct HttpFixture {
    pub base_url: String,           // http://127.0.0.1:{ephemeral}
    pub client: reqwest::Client,
}

impl HttpFixture {
    pub async fn call(
        &self,
        method: Method,
        path: &str,
        jwt: &str,
        body: Option<Value>,
    ) -> (StatusCode, Option<Value>) {
        // 204/205/304 ou content-length 0 → (status, None).
        // Demais → parse JSON; body vazio → None; body inválido → panic imediato.
    }
}
```

Regra: **matriz sempre compara status primeiro.** Body só é inspecionado em asserts específicos (rotas de coleção, Seção 5). `204 No Content` retorna `Option::None` — nenhum caso precisa fazer parsing frágil de body vazio.

### 2.6 `common/cases.rs` — tipos específicos da suite

```rust
// Importa do crate real — NUNCA duplicar.
pub use garraia_auth::{Role, Action};

#[derive(Debug, Clone, Copy)]
pub enum Relationship { OwnerOfTarget, MemberOfTarget, Outsider, CrossTenant }

#[derive(Debug, Clone, Copy)]
pub enum Expected { Allow, Deny(DenyKind) }

#[derive(Debug, Clone, Copy)]
pub enum DenyKind { Unauthenticated, Forbidden, NotFound }

pub struct AppCase {
    pub case_id: &'static str,   // label estável — aparece em todo panic/assert
    pub role: Role,
    pub action: Action,
    pub relationship: Relationship,
    pub expected: Expected,
}

#[derive(Debug, Clone, Copy)]
pub enum DbRole { App, Login, Signup }

#[derive(Debug, Clone, Copy)]
pub enum SqlOp { Select, Insert, Update, Delete }

#[derive(Debug, Clone, Copy)]
pub enum TenantCtx {
    /// app.current_user_id + app.current_group_id ambos corretos (membership real)
    Correct,
    /// current_user_id correto, current_group_id aponta para OUTRO group
    WrongGroupCorrectUser,
    /// nem current_user_id nem current_group_id definidos
    BothUnset,
    /// role correto para a operação, mas GUCs de outro tenant
    CorrectRoleWrongTenant,
}

#[derive(Debug, Clone, Copy)]
pub enum RlsExpected {
    RowsVisible(usize),      // exatamente N linhas / linhas afetadas, sem erro
    InsufficientPrivilege,   // SQLSTATE 42501 — role não tem GRANT na tabela
    PermissionDenied,        // SQLSTATE 42501 — GRANT existe, WITH CHECK rejeitou
    RlsFilteredZero,         // query OK, 0 rows — USING clause filtrou silenciosamente
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

**Convenção de `case_id`:**

- App-layer: `"app_{role}_{action}_{rel}"` — ex.: `"app_group_owner_chat_message_post_owner_of_target"`.
- RLS: `"rls_{db_role}_{table}_{op}_{ctx}"` — ex.: `"rls_app_chats_select_wrong_group"`.

Labels são literais `&'static str` para aparecerem em todo `panic!` / `assert!` e permitirem `grep` direto no source.

---

## 3. Matriz app-layer (`authz_cross_group.rs`)

### 3.1 Política 401 / 404 / 403 — regra escrita

| Situação do sujeito                                            | Response        |
|----------------------------------------------------------------|-----------------|
| Sem JWT ou JWT inválido/expirado                               | **401**         |
| JWT válido, usuário **não é membro** do group do recurso       | **404 NotFound**|
| JWT válido, é membro, mas **role não tem a permission**        | **403 Forbidden**|
| JWT válido, é membro, role tem permission → `Allow`            | **2xx**         |

**Justificativa do 404 em vez de 403** para outsider/cross-tenant: evita *resource enumeration* — se outsiders recebessem 403, saberiam que o recurso existe em outro group. 404 é indistinguível de "não existe".

**Regra de escopo do 404:** essa política se aplica **apenas a recursos endereçáveis por ID** (`GET /v1/{resource}/{id}`). **Rotas de coleção** (`GET /v1/{resource}`) **sempre retornam `200` com array vazio** quando o sujeito não tem visibilidade — **nunca** 404. Isso é oráculo dual: status E body (Seção 5.4).

### 3.2 Tenant fresh por case — escolha consciente

`matrix_app_layer()` chama `Tenant::new(&h)` **uma vez por case**. Compartilhar tenants entre cases introduziria acoplamento de ordem (um case mutando estado que outro observa), exatamente o que a estratégia de isolamento da Seção 2.2 proíbe. Custo ≈ 120 tenants × ~50ms = **~6s no container warm**, aceitável. **Fidelidade > microperformance** é declarado explicitamente como escolha arquitetural.

### 3.3 Subset representativo — justificativa explícita

A matriz exaustiva seria `5 Roles × 22 Actions × 4 Relationships = 440` casos. O alvo é **~120** com cortes justificados:

**Fundamento do corte (cristalino, não arbitrário):**

> O unit-test `5 roles × 22 actions` documentado em `CLAUDE.md` e residente em `garraia-auth` já prova exaustivamente a **tabela pura `fn can()`**. Esta suite **não re-prova isso**. A dimensão *nova* trazida por GAR-391d é `Relationship` — por isso é a única que é testada exaustivamente em cada par `(role, action)` selecionado. As outras dimensões são reduzidas a um subset representativo **porque o teste pure-function já cobre a combinatória remanescente**.

**Cortes aplicados:**

**(a) Collapse de roles equivalentes por action.** Para `Action::ChatMessagePost`, `GroupOwner` e `GroupAdmin` recebem o mesmo veredicto. Testo **apenas `GroupOwner`** como representante. Corte: ~40% dos pares role.

**(b) Agrupamento de actions por categoria.** As 22 actions caem em 6 categorias: `Chat*`, `Message*`, `Memory*`, `Task*`, `Group*`, `Identity*`. Dentro de cada categoria escolho **uma action read + uma action write + uma action destrutiva**. Ex.: Chat → `ChatRead`, `ChatCreate`, `ChatDelete`. Corte: 22 → ~18 actions efetivas.

**(c) Relationships não são podadas.** Todas as 4 (`Owner`, `Member`, `Outsider`, `CrossTenant`) são testadas em **cada** par `(role, action)` selecionado.

**Resultado:** `~5 roles × ~18 actions × 4 relationships ≈ 360`, menos combinações que o `can()` declara impossíveis, converge para **~120 casos com propósito distinto**. Acima do ≥100 exigido.

**No topo de `APP_MATRIX`** fica um bloco de comentário listando os cortes (a), (b), (c) e **por que** cada categoria não tem mais casos — auditabilidade para revisores futuros.

### 3.4 Runner

```rust
#[tokio::test(flavor = "multi_thread")]
async fn matrix_app_layer() -> anyhow::Result<()> {
    let h = Harness::get().await;
    let mut failures: Vec<String> = Vec::new();

    for case in APP_MATRIX {
        let tenant = Tenant::new(&h).await;
        let actor  = tenant.actor_for(case.relationship);
        let target = action_target(&tenant, case.action).await;
        let route  = route_for(case.action);
        let path   = render_path(route.path_template, &target);
        let body   = route.body.map(|f| f(&tenant, &target));

        let (got, _body_opt) = h.http.call(route.method.clone(), &path, &actor.jwt, body).await;

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

**Todas as falhas são reportadas** (não aborta no primeiro) — um bug sistêmico revela-se de uma vez.

---

## 4. Matriz RLS pura (`rls_matrix.rs`)

Esta matriz **não passa pelo axum**, não tem JWT, não invoca `fn can()`. Conecta direto ao Postgres como um dos 3 roles e valida o enforcement no banco — é o segundo pilar aprovado na Brainstorming Q1 → opção B.

### 4.1 Oracle RLS — regra escrita de distinção

A suite distingue **três modos de "negação"** diferentes do Postgres, porque confundi-los mascara bugs reais:

| Resultado esperado       | Quando                                                                                                                     | Como é detectado                                                                              |
|---|---|---|
| `InsufficientPrivilege` | O role **não tem `GRANT`** na tabela/coluna. Postgres recusa antes de sequer avaliar a policy.                              | SQLSTATE **42501** com `MESSAGE` começando em `"permission denied for table"`.                |
| `PermissionDenied`      | O role **tem `GRANT`**, mas a RLS `WITH CHECK` (write path) rejeita a linha porque os GUCs apontam para outro tenant.        | SQLSTATE **42501** com `MESSAGE` começando em `"new row violates row-level security policy"`. |
| `RlsFilteredZero`       | A `USING` clause filtrou a linha silenciosamente — query sucede, retorna 0 rows ou 0 rows afetadas.                          | Resultado sem erro, `rows == 0` / `rows_affected == 0`.                                       |
| `RowsVisible(n)`        | Sucesso. `SELECT` retorna `n` linhas OU `INSERT`/`UPDATE`/`DELETE` afeta `n` linhas **e** uma leitura posterior confirma.    | Sem erro; contagem bate exatamente.                                                            |

Strings do Postgres 16 são estáveis entre patch releases; a suite comenta esse acoplamento e isola a comparação em um helper `classify_error(&PgError)` → `RlsOutcome`.

### 4.2 Oracle de writes — dupla validação

- **`Insert` esperado `RowsVisible(n)`** → o `INSERT` executa, e **na mesma conexão** (mesmos GUCs, sem commit intermediário) um `SELECT count(*)` confirma que a linha é visível. Dupla validação: (i) o INSERT não disparou erro, (ii) a `USING` lê de volta.
- **`Update` / `Delete` esperado sucesso** → comparação de `rows_affected` do `sqlx::QueryResult`. Se `RowsVisible(0)` for esperado, a query precisa retornar `rows_affected == 0` **sem erro** (distingue de `PermissionDenied`).
- **Erros esperados** → oracle é SQLSTATE + prefixo da `MESSAGE` conforme tabela 4.1. Nenhum oracle cai em catch-all genérico.

### 4.3 Relevância semântica de `tenant_ctx` por role

| Role             | `tenant_ctx` é relevante? | Tratamento na matriz                                                                                   |
|---|---|---|
| `garraia_app`    | **Sim** — todas as 4 variants testadas. É o role RLS-enforced.                                          | Todos os casos variam `tenant_ctx` através das 4 opções.                                              |
| `garraia_login`  | **Não** — role é `BYPASSRLS`, único enforcement é GRANT-layer.                                           | Casos fixam `tenant_ctx: BothUnset`. Campo mantido no struct para uniformidade; nunca variado.         |
| `garraia_signup` | **Não** — mesma razão; signup acontece **antes** de haver qualquer tenant.                                | Idem: `tenant_ctx: BothUnset` fixo.                                                                   |

Isso elimina a dimensão decorativa: `tenant_ctx` nunca é variado onde não significa nada.

### 4.4 Contagem e distribuição

| Bloco                                                                                               | Casos |
|---|---|
| `garraia_app` × 18 tabelas FORCE RLS × subset `{Select, Insert, Update, Delete}` × 4 `TenantCtx`     | ~60   |
| `garraia_login` × tabelas whitelisted (`users`, `user_identities`, `sessions`, `group_members`)      | ~10   |
| `garraia_signup` × `users`/`user_identities` (Allow) + tudo fora (InsufficientPrivilege)              | ~8    |
| NULLIF fail-closed policies (`BothUnset` + `garraia_app` em tabelas tenant-scoped → `RlsFilteredZero`) | ~6   |
| **Total**                                                                                            | **~84** |

**Somado à Seção 3 (~120 casos): ~204 casos totais**, bem acima do ≥100 exigido.

### 4.5 Executor

```rust
async fn execute_rls_case(h: &Harness, tenant: &Tenant, case: &RlsCase) -> RlsOutcome {
    let mut conn = match case.db_role {
        DbRole::App    => h.app_pool.acquire().await.unwrap(),
        DbRole::Login  => h.login_pool.raw().acquire().await.unwrap(),
        DbRole::Signup => h.signup_pool.raw().acquire().await.unwrap(),
    };

    match case.tenant_ctx {
        TenantCtx::Correct => {
            set_guc(&mut conn, "app.current_user_id",  &tenant.member.user_id).await;
            set_guc(&mut conn, "app.current_group_id", &tenant.group_id).await;
        }
        TenantCtx::WrongGroupCorrectUser => {
            set_guc(&mut conn, "app.current_user_id",  &tenant.member.user_id).await;
            set_guc(&mut conn, "app.current_group_id", &Uuid::new_v4()).await;
        }
        TenantCtx::BothUnset => { /* não seta nada */ }
        TenantCtx::CorrectRoleWrongTenant => {
            let other = Uuid::new_v4();
            set_guc(&mut conn, "app.current_user_id",  &other).await;
            set_guc(&mut conn, "app.current_group_id", &other).await;
        }
    }

    match case.op {
        SqlOp::Select => run_select(&mut conn, case.table).await,
        SqlOp::Insert => run_insert_and_readback(&mut conn, case.table, tenant).await,
        SqlOp::Update => run_update(&mut conn, case.table, tenant).await,
        SqlOp::Delete => run_delete(&mut conn, case.table, tenant).await,
    }
}
```

### 4.6 Test-only escape hatch — boundary declarada

```rust
// crates/garraia-auth/src/login_pool.rs
impl LoginPool {
    #[cfg(test)]
    pub fn raw(&self) -> &PgPool { &self.0 }
}
```

> **Test-only concession:** `LoginPool::raw()` e `SignupPool::raw()` existem **apenas sob `#[cfg(test)]` dentro do crate `garraia-auth`**. Código de produção (qualquer `.rs` não-teste, em qualquer crate do workspace) **não compila** chamadas a esses métodos. A boundary da Regra 11 do `CLAUDE.md` (*"acesso ao role só via newtype"*) fica preservada em produção; em teste, a escape hatch é reconhecida, explicitamente documentada, e auditável via `grep raw() crates/garraia-auth/src/**` no CI gate (Success Criteria §7.6).

### 4.7 Failure output

```
[rls_app_chats_select_wrong_group] role=garraia_app table=chats op=Select
  tenant_ctx=WrongGroupCorrectUser
  expected=RlsFilteredZero got=RowsVisible(1)   ← CVE potencial
```

Um mismatch como esse acima é potencial cross-tenant data leak; a mensagem precisa ser inequívoca e incluir `case_id` literal.

---

## 5. Mapeamento `Action → HTTP` + `action_target()`

### 5.1 `RouteSpec` com `RouteKind` explícito

```rust
pub enum RouteKind { Collection, Resource }

pub struct RouteSpec {
    pub category: &'static str,              // Chat | Message | Memory | Task | Group | Identity
    pub kind: RouteKind,                     // Collection vs Resource — nunca inferido por regex
    pub method: http::Method,
    pub path_template: &'static str,         // "/v1/chats/{chat_id}"
    pub body: Option<fn(&Tenant, &ActionTarget) -> Value>,
    pub allow_status: http::StatusCode,      // status esperado quando Expected::Allow (201/200/204)
}
```

**`RouteKind` é declarado literalmente** em cada entrada de `route_for()`. A matriz nunca infere coleção/recurso por regex no `path_template` — essa distinção é parte do design da suite e precisa aparecer na fonte.

### 5.2 `route_for(action)` — `match` exaustivo

```rust
pub fn route_for(action: Action) -> RouteSpec {
    use http::Method::*;
    use http::StatusCode::*;
    match action {
        Action::ChatCreate => RouteSpec {
            category: "Chat", kind: RouteKind::Collection,
            method: POST, path_template: "/v1/chats",
            body: Some(|_, _| json!({"title": "t", "kind": "direct"})),
            allow_status: CREATED,
        },
        Action::ChatRead => RouteSpec {
            category: "Chat", kind: RouteKind::Resource,
            method: GET, path_template: "/v1/chats/{chat_id}",
            body: None, allow_status: OK,
        },
        Action::ChatDelete => RouteSpec {
            category: "Chat", kind: RouteKind::Resource,
            method: DELETE, path_template: "/v1/chats/{chat_id}",
            body: None, allow_status: NO_CONTENT,
        },
        Action::ChatList => RouteSpec {
            category: "Chat", kind: RouteKind::Collection,
            method: GET, path_template: "/v1/chats",
            body: None, allow_status: OK,
        },
        Action::MessagePost => RouteSpec {
            category: "Message", kind: RouteKind::Resource,
            method: POST, path_template: "/v1/chats/{chat_id}/messages",
            body: Some(|_, _| json!({"content": "hello"})),
            allow_status: CREATED,
        },
        // ... 18 actions representativas no subset
    }
}
```

**Por que `match` exaustivo:** força o compilador a quebrar se uma `Action` nova for adicionada sem mapeamento. Uma trait `HttpRoute for ChatCreate` perderia essa garantia.

### 5.3 `action_target()` — HTTP real vs helper SQL

```rust
pub async fn action_target(tenant: &Tenant, action: Action) -> ActionTarget {
    let owner_jwt = &tenant.owner.jwt;
    match action {
        Action::ChatCreate | Action::ChatList => ActionTarget::empty(),
        Action::ChatRead | Action::ChatDelete | Action::MessagePost => {
            let chat_id = create_chat_via_http(owner_jwt).await;
            ActionTarget { chat_id: Some(chat_id), ..ActionTarget::empty() }
        }
        Action::MemoryDelete => {
            // MemoryCreate não está no subset → usa SQL helper com GUCs do owner.
            let memory_id = insert_memory_via_sql(&tenant.group_id, &tenant.owner.user_id).await;
            ActionTarget { memory_id: Some(memory_id), ..ActionTarget::empty() }
        }
        // ...
    }
}
```

**Regra escrita:**

> Preferir HTTP real sempre que o endpoint de criação **está no subset testado pela matriz**. Cair para SQL helper (via `h.app_pool` com GUCs do owner) **apenas** quando (i) o endpoint de setup está fora do subset ou (ii) um pré-requisito via HTTP criaria dependência circular entre casos. Cada case SQL-helper é comentado com a razão.

`action_target()` **sempre cria o recurso no escopo do `tenant.owner`** (ou seja, dentro de `tenant.group_id`). Isso é o que torna a dimensão `Relationship` significativa:

- `OwnerOfTarget` → `tenant.owner` acessa seu próprio recurso → `Allow`.
- `MemberOfTarget` → `tenant.member` acessa o mesmo recurso → `Allow` ou `Forbidden` (depende de `fn can()`).
- `Outsider` → `tenant.outsider` acessa → `NotFound` (Resource) ou `200 []` (Collection).
- `CrossTenant` → `tenant.cross_tenant` acessa → `NotFound` (Resource) ou `200 []` (Collection).

### 5.4 Regra formal de rotas de coleção

> **Rotas `RouteKind::Collection` sempre retornam `200 OK` para qualquer sujeito autenticado.** O oracle é **dual**: status == 200 **e** body array com tamanho esperado — `0` para `Outsider` / `CrossTenant`, `≥1` para `Owner` / `Member` quando `action_target()` criou dado compatível no group. **Nunca retornam 404.** Resource enumeration é bloqueado pela invisibilidade do dado, não pelo status.

### 5.5 Substituição de placeholders — erro de construção, não de produto

```rust
fn render_path(template: &'static str, t: &ActionTarget) -> String {
    let mut p = template.to_string();
    if let Some(id) = t.chat_id    { p = p.replace("{chat_id}",    &id.to_string()); }
    if let Some(id) = t.message_id { p = p.replace("{message_id}", &id.to_string()); }
    if let Some(id) = t.memory_id  { p = p.replace("{memory_id}",  &id.to_string()); }
    if let Some(id) = t.task_id    { p = p.replace("{task_id}",    &id.to_string()); }
    debug_assert!(
        !p.contains('{'),
        "route template `{template}` has unresolved placeholders — this is a test-suite \
         construction bug (missing ActionTarget field), NOT a product behavior failure",
    );
    p
}
```

**Regra escrita:**

> Placeholders não resolvidos em `render_path` são erro de construção da matriz, não comportamento do produto. O `debug_assert!` panica com o `case_id` anexado pelo caller para forçar correção da linha da matriz. Nenhuma análise de produto é feita sobre esse caso — a suite assume que a matriz está bem-formada e trata placeholder pendente como "bug do teste".

### 5.6 `check()` — oracle único com contexto de falha rico

```rust
fn check(case: &AppCase, route: &RouteSpec, path: &str, got: http::StatusCode)
    -> Result<(), String>
{
    let expected_status = match case.expected {
        Expected::Allow                            => route.allow_status,
        Expected::Deny(DenyKind::Unauthenticated)  => StatusCode::UNAUTHORIZED,
        Expected::Deny(DenyKind::NotFound)         => StatusCode::NOT_FOUND,
        Expected::Deny(DenyKind::Forbidden)        => StatusCode::FORBIDDEN,
    };

    if got == expected_status { return Ok(()); }

    Err(format!(
        "[{}] category={} method={} path={}\n  \
         role={:?} action={:?} rel={:?}\n  \
         expected={:?}({} {}) got={} {}",
        case.case_id, route.category, route.method, path,
        case.role, case.action, case.relationship,
        case.expected, expected_status.as_u16(), expected_status.canonical_reason().unwrap_or(""),
        got.as_u16(), got.canonical_reason().unwrap_or(""),
    ))
}
```

Exemplo de saída de falha:

```
[app_member_chat_create_member] category=Chat method=POST path=/v1/chats
  role=GroupMember action=ChatCreate rel=MemberOfTarget
  expected=Allow(201 Created) got=403 Forbidden
```

Triagem rápida: o operador sabe **qual** case, **qual** categoria, **qual** path, **qual** status esperado e **qual** recebido, tudo numa linha.

---

## 6. Execução, tempo, CI e success criteria

### 6.1 Orçamento de tempo

| Etapa                                              | Custo estimado        |
|---|---|
| Boot do container `pgvector/pg16` (cold pull)       | ~60s primeira vez     |
| Boot warm + migrations 001..010                     | ~5s                   |
| Boot do axum em porta efêmera                        | <100ms                |
| Criação de 1 tenant (4 signups + 4 promoções + 4 logins, paralelos) | ~50ms |
| ~120 casos app-layer = 120 tenants fresh            | ~6s                   |
| ~84 casos RLS reusando tenants                      | ~2s                   |
| **Total suite (warm)**                               | **~15s**              |
| **Total suite (cold)**                               | **~75s**              |

Cold acontece uma vez por máquina; warm acontece em todo rerun.

### 6.2 CI

- **Comando:** `cargo test -p garraia-auth --test authz_cross_group --test rls_matrix`.
- **Gate:** bloqueia merge em qualquer PR que toque `crates/garraia-auth/**`, `crates/garraia-workspace/migrations/**` ou `crates/garraia-gateway/src/mobile_auth*`. Implementação literal da Regra 10 do `CLAUDE.md`.
- **Fallback serial:** `GARRAIA_AUTHZ_SUITE_SERIAL=1` troca para execução sequencial sem recompilar.
- **Não roda em `pre-commit`** (caro demais); roda no workflow de PR e em `cargo test --workspace` nightly.

### 6.3 Success criteria (verificáveis)

A entrega está **Done** quando **todos** os itens abaixo são verdadeiros:

1. `cargo test -p garraia-auth --test authz_cross_group` passa com **≥120 casos**, `failures.is_empty()`.
2. `cargo test -p garraia-auth --test rls_matrix` passa com **≥84 casos**, cobrindo os 3 `DbRole` e as 4 `TenantCtx` variants aplicáveis.
3. `cargo clippy -p garraia-auth --tests -- -D warnings` limpo.
4. **Total ≥ 100 casos** comprovado por `fn total_case_count()` — `assert!(APP_MATRIX.len() + RLS_MATRIX.len() >= 100)` (tripwire contra degradação silenciosa).
5. Cada caso falho reporta `case_id` + `category` + `method` + `path` + `expected_status` + `got` (Seção 5.6).
6. Regra 11 do `CLAUDE.md` preservada: `grep -rn 'fn raw' crates/garraia-auth/src/` retorna **apenas** declarações sob `#[cfg(test)]` (auditado manualmente no review).
7. Todas as 4 `Relationship` variants exercitam pelo menos uma action em cada uma das 6 categorias (`Chat`/`Message`/`Memory`/`Task`/`Group`/`Identity`) — tripwire via `fn coverage_check()`.
8. Oracle RLS distingue `InsufficientPrivilege` vs `PermissionDenied` vs `RlsFilteredZero` por SQLSTATE + MESSAGE prefix; nenhum catch-all de "erro genérico".
9. `docs/adr/0005-identity-provider.md` ganha linha no Amendment apontando para esta suite como evidência do enforcement.
10. `CLAUDE.md` tem a linha `"Pending: 391d/GAR-392"` substituída por registro de entrega com data America/New_York.

### 6.4 Riscos e mitigações

| Risco                                                        | Mitigação                                                                                          |
|---|---|
| Container cold pull (~60s) bloqueia CI                       | Image pré-cacheada no runner; warm starts ~5s                                                      |
| Paralelização de `Tenant::new` revela race                   | Fallback `GARRAIA_AUTHZ_SUITE_SERIAL=1` já oficial (Seção 2.4)                                    |
| `fn can()` permite o que a RLS policy nega                   | **É exatamente o que a suite deve pegar** — não é risco, é caso de teste                          |
| `Action` nova sem entrada em `route_for()`                   | `match` exaustivo — compilador garante                                                             |
| Placeholder de path não populado                             | `debug_assert!` com `case_id` (Seção 5.5)                                                         |
| Escape hatch `raw()` vazar para produção                     | Review manual + grep no CI gate (Success Criteria §6.3.6)                                          |
| Mudança de string `MESSAGE` em Postgres patch release        | Comentário inline + helper `classify_error()` isolado — patch único se Postgres mudar             |

### 6.5 Out of scope

- **Fuzzing** de payloads — esta é matriz de authz, não input validation.
- **Performance benchmarks** — tempo é orçado, não medido por `criterion`.
- **Migração dos testes legados** (`signup_flow.rs`, `verify_internal.rs`, `extractor.rs`, `concurrent_upgrade.rs`) para o harness compartilhado — mantêm container próprio.
- **Cobertura de `garraia-channels`, `bootstrap.rs`, rotas não-auth** — foco é fechar epic de auth.
- **Fase 391e+** — gaps descobertos durante execução viram tickets novos, não escopo deste plano.

---

## 7. Self-review (inline — passo 7 do brainstorming)

**Placeholder scan:** nenhum `TBD`, `TODO`, `FIXME` no corpo do design. A única menção de `TODO` é um comentário de fallback dentro de um snippet de código ("fallback to sequential if flaky") que reflete comportamento intencional já promovido a feature-flag oficial (Seção 2.4) — não é uma pendência do doc.

**Consistência interna:**
- Política 404 (§3.1) é consistente com regra de coleção (§5.4): Resource=404, Collection=200 `[]`. Nenhuma contradição.
- Oracle RLS (§4.1) é consistente com escape hatch test-only (§4.6): os casos RLS usam pools reais via `raw()`, declarado como test-only.
- `Tenant::new` paralelo (§2.4) é consistente com orçamento de tempo (§6.1): 50ms por tenant é possível apenas porque signups são `try_join!`.
- Subset representativo (§3.3) é consistente com unit-test existente (`5×22`) — este design referencia explicitamente.
- Contagem ~120 + ~84 = ~204 ≥ 100 (§3.3 + §4.4 + §6.3.4).

**Scope check:** uma entrega, dois arquivos de teste, um módulo `common/`, fecha um epic existente. Não decomponha. Não inflacione.

**Ambiguidade:**
- `"subset representativo"` é definido em §3.3 por critérios (a)/(b)/(c) com números concretos.
- `"contexto tenant errado"` nas 4 `TenantCtx` variants é tabelado em §2.6 e executado em §4.5.
- `"body não-vazio quando setup criou dado compatível"` em §5.4 depende de `action_target()` — o link está explícito.
- `"erro genérico"` em §6.3.8 é proibido por §4.1 (oracle específico obrigatório).

**Pontos que sobraram intencionalmente abertos (não ambiguidade, são decisões de plan):**
- Lista final exata dos 18 actions do subset → será materializada no plano de implementação (`writing-plans`).
- Exato SQL template de `run_insert_and_readback` por tabela → plano.
- Valor exato de `expected` em cada linha das matrizes → plano (viria do `fn can()` + políticas de 404/403).

**Fix inline aplicado:** nenhum — o doc sobreviveu ao self-review sem edições.

---

## 8. Transição para implementação

Próximo passo do workflow do skill `superpowers:brainstorming`: **passo 8 — usuário revisa o spec escrito**. Após aprovação, **passo 9** invoca `superpowers:writing-plans` para gerar o plano detalhado de implementação desta suite.

Nenhuma outra skill de implementação é invocada a partir deste documento. A HARD-GATE do `brainstorming` só libera para `writing-plans`.
