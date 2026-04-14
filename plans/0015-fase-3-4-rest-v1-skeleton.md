# Plan 0015 — Fase 3.4 REST `/v1` Skeleton (slice 1: `GET /v1/me`)

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Linear issue:** [GAR-WS-API](https://linear.app/chatgpt25/team/GAR) (Fase 3.4 / API REST `/v1` + OpenAPI) — this plan is the **skeleton slice 1** of that epic. Pré-condição para o `plan 0014` (GAR-391d, app-layer cross-group matrix via HTTP), que permanece reservado.

**Status:** ⏳ Draft — aprovado 2026-04-14 (Florida). Plano imutável após primeiro commit.

**Goal:** Nascer a superfície REST versionada `/v1` no `garraia-gateway` com um único endpoint real (`GET /v1/me`), OpenAPI 3.1 gerada por `utoipa`, Swagger UI em `/docs`, erros no formato RFC 9457 Problem Details, e testes de integração cobrindo 401/200/403 — sem alterar nenhuma rota existente.

**Architecture:** Novo módulo `crate::rest_v1` em `garraia-gateway` com roteador próprio (`Router<Arc<AppState>>`) montado via `.merge()` no router principal. Fail-soft: quando `AuthConfig` não está configurado, todas as rotas `/v1/*` devolvem 503 Problem Details (mesmo padrão que `auth_routes.rs`). O extractor `garraia_auth::Principal` é reaproveitado via um `SubState` (`FromRef`) que expõe `Arc<JwtIssuer>` e `Arc<LoginPool>` desembrulhados dos `Option` do `AppState`. Zero breaking change: endpoints legados (`mobile_auth`, `mobile_chat`, `auth_routes /v1/auth/*`) permanecem intactos.

**Tech Stack:** Axum 0.8, `utoipa 5.x` (OpenAPI derive), `utoipa-swagger-ui 8.x`, `garraia-auth` (extractor + JwtIssuer + LoginPool), `serde`, `thiserror`, `sqlx` (já no workspace), `tokio`, `testcontainers` + `testcontainers-modules` (já nos dev-deps).

**Numbering note:** o slot `0014` permanece **reservado para o GAR-391d** (matriz app-layer cross-group via HTTP), conforme decidido no encerramento do plano 0013. Este plano (pré-condição da matriz) toma o próximo slot livre, `0015`. Ordem cronológica preservada: 0014 foi **criado primeiro** como reserva; 0015 é criado agora como o skeleton que **destrava** a execução futura do 0014. Nada na linha do 0014 no `plans/README.md` é alterado por este plano.

**Out of scope (próximas fatias do epic, NÃO deste plano):**
- `POST/GET /v1/groups*` (fatia 2 — primeiro write + RLS `garraia_app` end-to-end)
- `/v1/chats`, `/v1/messages`, `/v1/memory`, `/v1/tasks`, WebSocket stream
- Contract tests via `schemathesis` (entra na fatia final do epic)
- Object storage / files (bloqueado por ADR 0004 / GAR-374)
- Rate limiting dedicado `/v1`
- Autorização via `RequirePermission` em handlers (fatia 2 usa)

**Rollback plan:** Tudo é aditivo. Reverter o plano = `git revert` dos commits. Sem migrations novas, sem schema change, sem alteração em rotas existentes, sem env var nova. O feature flag implícito é o próprio `AuthConfig` (já existente): sem as env vars o `/v1/*` devolve 503 e o gateway continua operacional nas rotas legadas.

---

## File Structure

**Criar:**
- `crates/garraia-gateway/src/rest_v1/mod.rs` — submódulo raiz, `router()` função pública, `RestV1State` sub-state
- `crates/garraia-gateway/src/rest_v1/problem.rs` — `ProblemDetails` (RFC 9457) + `RestError` enum + `IntoResponse`
- `crates/garraia-gateway/src/rest_v1/openapi.rs` — `#[derive(OpenApi)]` agregador + `ApiDoc` struct
- `crates/garraia-gateway/src/rest_v1/me.rs` — handler `get_me` + DTO `MeResponse`
- `crates/garraia-gateway/tests/rest_v1_me.rs` — testes de integração (401, 200, 403, 503, shape OpenAPI)

**Modificar:**
- `crates/garraia-gateway/Cargo.toml` — adicionar `utoipa`, `utoipa-swagger-ui`, `thiserror`
- `crates/garraia-gateway/src/lib.rs` — `pub mod rest_v1;`
- `crates/garraia-gateway/src/router.rs` — `.merge(crate::rest_v1::router(state.clone()))`
- `plans/README.md` — adicionar entrada `0015` (sem tocar na linha do `0014`)
- `ROADMAP.md` — marcar `[x] GET /v1/me` na checklist §3.4 (linha ~281 ou equivalente)

**NÃO tocar:** `CLAUDE.md` (sem nova regra operacional), `docs/adr/*`, migrations, `garraia-auth` (já tem tudo que precisamos), `mobile_*.rs`, `auth_routes.rs`.

---

## Tasks

### Task 0: Registrar `0015` no índice `plans/README.md`

**Files:**
- Modify: `plans/README.md`

**Regras:**
- **NÃO tocar** na linha do `0014` (que permanece reservada ao GAR-391d).
- Adicionar **uma linha nova** logo depois do `0014`, referenciando este plano como `0015`.

- [ ] **Step 1: Adicionar a linha do 0015**

Inserir, imediatamente após a linha atual do `0014`, na tabela do índice de `plans/README.md`:

```markdown
| 0015 | [Fase 3.4 — REST `/v1` skeleton (slice 1: `GET /v1/me`)](0015-fase-3-4-rest-v1-skeleton.md) | GAR-WS-API (pré-condição GAR-391d) | ⏳ Aprovado 2026-04-14 |
```

Validar com `grep -n "0014\|0015" plans/README.md` que:
- A linha do `0014` está **idêntica** à versão anterior (sem alterações).
- A linha do `0015` foi adicionada logo abaixo.

- [ ] **Step 2: Commit**

```bash
git add plans/README.md plans/0015-fase-3-4-rest-v1-skeleton.md
git commit -m "docs(plans): add plan 0015 (Fase 3.4 REST /v1 skeleton, slice 1)"
```

---

### Task 1: Adicionar deps `utoipa` no `garraia-gateway`

**Files:**
- Modify: `crates/garraia-gateway/Cargo.toml`

- [ ] **Step 1: Adicionar deps**

No bloco `[dependencies]`, após a linha `jsonwebtoken = { workspace = true }`, inserir:

```toml
# Fase 3.4 REST /v1 (plan 0015): OpenAPI generation + Swagger UI.
utoipa = { version = "5", features = ["axum_extras", "uuid", "chrono"] }
utoipa-swagger-ui = { version = "8", features = ["axum"] }
thiserror = { workspace = true }
```

Verificar que `thiserror` já existe em `Cargo.toml` raiz do workspace em `[workspace.dependencies]`. Se não existir, parar e reportar (ele está em uso em vários crates do projeto, então é provável que exista — `grep -r "thiserror" Cargo.toml`).

- [ ] **Step 2: Verificar build**

Run: `cargo check -p garraia-gateway`
Expected: PASS (só baixa as deps novas)

- [ ] **Step 3: Commit**

```bash
git add crates/garraia-gateway/Cargo.toml Cargo.lock
git commit -m "chore(gateway): add utoipa + swagger-ui deps for REST /v1 skeleton"
```

---

### Task 2: Criar `rest_v1/problem.rs` (RFC 9457 Problem Details) — **TDD**

**Files:**
- Create: `crates/garraia-gateway/src/rest_v1/mod.rs` (stub mínimo só para o módulo compilar)
- Create: `crates/garraia-gateway/src/rest_v1/problem.rs`
- Modify: `crates/garraia-gateway/src/lib.rs`

- [ ] **Step 1: Stub mínimo do módulo**

Criar `crates/garraia-gateway/src/rest_v1/mod.rs`:

```rust
//! REST `/v1` surface (Fase 3.4, plan 0015).
//!
//! Versioned HTTP API. All errors follow RFC 9457 Problem Details.
//! OpenAPI 3.1 spec is generated via `utoipa`; Swagger UI is served at `/docs`.

pub mod problem;
```

Adicionar em `crates/garraia-gateway/src/lib.rs` (ou onde os outros `pub mod` moram):

```rust
pub mod rest_v1;
```

- [ ] **Step 2: Escrever o teste unitário primeiro (RED)**

Criar `crates/garraia-gateway/src/rest_v1/problem.rs` com apenas o módulo de testes:

```rust
//! RFC 9457 Problem Details for the `/v1` surface.

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;
    use http_body_util::BodyExt;

    #[tokio::test]
    async fn unauthenticated_serializes_to_rfc9457_shape() {
        let resp = RestError::Unauthenticated.into_response();
        assert_eq!(resp.status(), 401);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "application/problem+json",
        );
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["type"], "about:blank");
        assert_eq!(v["title"], "Unauthenticated");
        assert_eq!(v["status"], 401);
        assert!(v["detail"].is_string());
    }

    #[tokio::test]
    async fn service_unavailable_shape() {
        let resp = RestError::AuthUnconfigured.into_response();
        assert_eq!(resp.status(), 503);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["status"], 503);
        assert_eq!(v["title"], "Service Unavailable");
    }
}
```

- [ ] **Step 3: Rodar e ver falhar**

Run: `cargo test -p garraia-gateway rest_v1::problem`
Expected: FAIL — `RestError` não existe.

- [ ] **Step 4: Implementação mínima**

Prefixar o arquivo `problem.rs` (antes do `#[cfg(test)]`):

```rust
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use thiserror::Error;
use utoipa::ToSchema;

/// RFC 9457 Problem Details body.
///
/// `type` defaults to `about:blank`, which per the RFC means the only
/// semantic information is in `status` + `title`. Future slices can add
/// concrete `type` URIs pointing to a public error taxonomy.
#[derive(Debug, Serialize, ToSchema)]
pub struct ProblemDetails {
    #[serde(rename = "type")]
    pub type_uri: &'static str,
    pub title: &'static str,
    pub status: u16,
    pub detail: String,
}

/// Canonical error type for the `/v1` surface.
///
/// Every variant maps to exactly one HTTP status + Problem Details body.
/// New variants must be added here — handlers never hand-roll responses.
#[derive(Debug, Error)]
pub enum RestError {
    #[error("unauthenticated")]
    Unauthenticated,
    #[error("forbidden")]
    Forbidden,
    #[error("auth not configured")]
    AuthUnconfigured,
    #[error("internal error")]
    Internal(#[source] anyhow::Error),
}

impl RestError {
    fn status(&self) -> StatusCode {
        match self {
            RestError::Unauthenticated => StatusCode::UNAUTHORIZED,
            RestError::Forbidden => StatusCode::FORBIDDEN,
            RestError::AuthUnconfigured => StatusCode::SERVICE_UNAVAILABLE,
            RestError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn title(&self) -> &'static str {
        match self {
            RestError::Unauthenticated => "Unauthenticated",
            RestError::Forbidden => "Forbidden",
            RestError::AuthUnconfigured => "Service Unavailable",
            RestError::Internal(_) => "Internal Server Error",
        }
    }
}

impl IntoResponse for RestError {
    fn into_response(self) -> Response {
        let status = self.status();
        let body = ProblemDetails {
            type_uri: "about:blank",
            title: self.title(),
            status: status.as_u16(),
            detail: self.to_string(),
        };
        // Log internal errors before dropping the source; PII-safe because
        // Display on RestError never includes the inner anyhow::Error body.
        if let RestError::Internal(ref e) = self {
            tracing::error!(error = %e, "rest_v1 internal error");
        }
        let json = serde_json::to_vec(&body).unwrap_or_else(|_| b"{}".to_vec());
        (
            status,
            [("content-type", "application/problem+json")],
            json,
        )
            .into_response()
    }
}
```

- [ ] **Step 5: Verificar testes passam (GREEN)**

Run: `cargo test -p garraia-gateway rest_v1::problem`
Expected: PASS (2 tests)

- [ ] **Step 6: Clippy limpo**

Run: `cargo clippy -p garraia-gateway -- -D warnings`
Expected: sem warnings novos.

- [ ] **Step 7: Commit**

```bash
git add crates/garraia-gateway/src/lib.rs crates/garraia-gateway/src/rest_v1/
git commit -m "feat(gateway): RFC 9457 Problem Details for REST /v1 (plan 0015 t2)"
```

---

### Task 3: Criar `RestV1State` sub-state com `FromRef`

**Files:**
- Modify: `crates/garraia-gateway/src/rest_v1/mod.rs`

- [ ] **Step 1: Adicionar sub-state**

Adicionar ao final de `rest_v1/mod.rs`:

```rust
use std::sync::Arc;

use axum::extract::FromRef;

use garraia_auth::{JwtIssuer, LoginPool};

use crate::state::AppState;

/// Sub-state para o router `/v1`. Contém exatamente os Arcs que o extractor
/// `garraia_auth::Principal` exige via `FromRef`. Construído a partir do
/// `AppState` quando `AuthConfig` está presente; quando ausente o router
/// `/v1` inteiro é substituído por um fallback que devolve 503 Problem
/// Details em todas as rotas.
#[derive(Clone)]
pub struct RestV1State {
    pub jwt_issuer: Arc<JwtIssuer>,
    pub login_pool: Arc<LoginPool>,
}

impl RestV1State {
    /// Tenta construir a partir do AppState. Retorna `None` em fail-soft
    /// mode (AuthConfig ausente).
    pub fn from_app_state(app: &AppState) -> Option<Self> {
        Some(Self {
            jwt_issuer: app.jwt_issuer.clone()?,
            login_pool: app.login_pool.clone()?,
        })
    }
}

impl FromRef<RestV1State> for Arc<JwtIssuer> {
    fn from_ref(s: &RestV1State) -> Self {
        s.jwt_issuer.clone()
    }
}

impl FromRef<RestV1State> for Arc<LoginPool> {
    fn from_ref(s: &RestV1State) -> Self {
        s.login_pool.clone()
    }
}
```

- [ ] **Step 2: Verificar build**

Run: `cargo check -p garraia-gateway`
Expected: PASS.

> Nota: `login_pool` no `AppState` é `pub(crate)`; como `rest_v1` vive no mesmo crate, o acesso é válido. Idem `jwt_issuer`.

- [ ] **Step 3: Commit**

```bash
git add crates/garraia-gateway/src/rest_v1/mod.rs
git commit -m "feat(gateway): RestV1State sub-state with FromRef (plan 0015 t3)"
```

---

### Task 4: Handler `GET /v1/me` com DTO — **TDD shape test**

**Files:**
- Create: `crates/garraia-gateway/src/rest_v1/me.rs`
- Modify: `crates/garraia-gateway/src/rest_v1/mod.rs` (`pub mod me;`)

- [ ] **Step 1: Declarar o submódulo**

Adicionar no topo de `rest_v1/mod.rs`:

```rust
pub mod me;
```

- [ ] **Step 2: Escrever o DTO + teste de serialização (RED)**

Criar `crates/garraia-gateway/src/rest_v1/me.rs`:

```rust
//! `GET /v1/me` — returns the authenticated caller's identity.
//!
//! Read-only. Uses `garraia_auth::Principal` extractor; no SQL of its own
//! beyond the group_members membership lookup the extractor already does.

use axum::extract::State;
use axum::Json;
use garraia_auth::Principal;
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::problem::RestError;
use super::RestV1State;

/// Response body for `GET /v1/me`.
#[derive(Debug, Serialize, ToSchema)]
pub struct MeResponse {
    /// UUID of the authenticated user (from the JWT `sub` claim).
    pub user_id: Uuid,
    /// Active group UUID if the caller supplied `X-Group-Id` and is a
    /// member; `None` otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<Uuid>,
    /// Group role string (e.g. `"owner"`, `"member"`). `None` when
    /// `group_id` is `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

#[utoipa::path(
    get,
    path = "/v1/me",
    responses(
        (status = 200, description = "Authenticated identity", body = MeResponse),
        (status = 401, description = "Missing or invalid JWT", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller not a member of X-Group-Id", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn get_me(
    State(_state): State<RestV1State>,
    principal: Principal,
) -> Result<Json<MeResponse>, RestError> {
    Ok(Json(MeResponse {
        user_id: principal.user_id,
        group_id: principal.group_id,
        role: principal.role.map(|r| r.as_str().to_string()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn me_response_serializes_without_group_when_absent() {
        let body = MeResponse {
            user_id: Uuid::nil(),
            group_id: None,
            role: None,
        };
        let v = serde_json::to_value(&body).unwrap();
        assert_eq!(v["user_id"], "00000000-0000-0000-0000-000000000000");
        assert!(v.get("group_id").is_none(), "absent group_id must be skipped");
        assert!(v.get("role").is_none());
    }

    #[test]
    fn me_response_serializes_with_group_when_present() {
        let gid = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        let body = MeResponse {
            user_id: Uuid::nil(),
            group_id: Some(gid),
            role: Some("owner".into()),
        };
        let v = serde_json::to_value(&body).unwrap();
        assert_eq!(v["group_id"], "11111111-1111-1111-1111-111111111111");
        assert_eq!(v["role"], "owner");
    }
}
```

- [ ] **Step 3: Rodar testes unitários**

Run: `cargo test -p garraia-gateway rest_v1::me`
Expected: PASS (2 tests) — os testes cobrem só o DTO, o handler é exercitado nos testes de integração da Task 6.

> Nota: verificar que `Role::as_str()` existe em `garraia-auth`. Se não existir, usar `format!("{:?}", r).to_lowercase()` ou adicionar um método trivial. Fazer `grep -n "as_str" crates/garraia-auth/src/role.rs` antes de codar. Se precisar adicionar, fazer como commit separado no próprio `garraia-auth` antes desta task.

- [ ] **Step 4: Commit**

```bash
git add crates/garraia-gateway/src/rest_v1/
git commit -m "feat(gateway): GET /v1/me handler + MeResponse DTO (plan 0015 t4)"
```

---

### Task 5: OpenAPI aggregator + router `/v1` + Swagger UI

**Files:**
- Create: `crates/garraia-gateway/src/rest_v1/openapi.rs`
- Modify: `crates/garraia-gateway/src/rest_v1/mod.rs`
- Modify: `crates/garraia-gateway/src/router.rs`

- [ ] **Step 1: Criar `openapi.rs`**

```rust
//! OpenAPI 3.1 aggregator for the `/v1` surface.
//!
//! New endpoints get added to `paths(...)` and their request/response
//! types added to `components(schemas(...))`.

use utoipa::OpenApi;

use super::me::{get_me, MeResponse};
use super::problem::ProblemDetails;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "GarraIA REST /v1",
        version = "0.1.0",
        description = "Versioned GarraIA gateway REST surface (Fase 3.4)."
    ),
    paths(get_me),
    components(schemas(MeResponse, ProblemDetails))
)]
pub struct ApiDoc;
```

- [ ] **Step 2: Declarar submódulo + função `router`**

Atualizar `rest_v1/mod.rs` adicionando:

```rust
pub mod openapi;

use axum::routing::get;
use axum::Router;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use self::openapi::ApiDoc;

/// Build the `/v1` router. Takes the full `AppState` so we can fail-soft
/// when auth is not configured: in that case every `/v1/*` path answers
/// 503 Problem Details.
pub fn router(app_state: std::sync::Arc<AppState>) -> Router {
    match RestV1State::from_app_state(&app_state) {
        Some(sub) => Router::new()
            .route("/v1/me", get(me::get_me))
            .with_state(sub)
            .merge(
                SwaggerUi::new("/docs").url("/v1/openapi.json", ApiDoc::openapi()),
            ),
        None => {
            // Fail-soft: catch-all that returns 503 Problem Details.
            // Matches the pattern used by auth_routes.rs when AuthConfig
            // env vars are missing.
            Router::new().fallback(|| async { problem::RestError::AuthUnconfigured })
        }
    }
}
```

> Importante: o `fallback` só captura rotas que começam com `/v1` e `/docs` porque o router principal faz `.merge()` — não é um catch-all global. Verificar no Task 6 com um teste 503.

- [ ] **Step 3: Montar no router principal**

Em `crates/garraia-gateway/src/router.rs` logo após a linha `.merge(crate::auth_routes::router().with_state(state.clone()))` (linha ~215), adicionar:

```rust
        .merge(crate::rest_v1::router(state.clone()))
```

- [ ] **Step 4: Verificar build**

Run: `cargo check -p garraia-gateway`
Expected: PASS.

- [ ] **Step 5: Clippy**

Run: `cargo clippy -p garraia-gateway -- -D warnings`
Expected: sem warnings novos.

- [ ] **Step 6: Commit**

```bash
git add crates/garraia-gateway/src/rest_v1/ crates/garraia-gateway/src/router.rs
git commit -m "feat(gateway): mount /v1 router with OpenAPI + Swagger UI (plan 0015 t5)"
```

---

### Task 6: Testes de integração `rest_v1_me.rs`

**Files:**
- Create: `crates/garraia-gateway/tests/rest_v1_me.rs`

Objetivo: exercitar o handler contra um Postgres real (pgvector/pg16 via testcontainers) com migrations 001..010 aplicadas, idêntico ao padrão já usado em `tests/auth_integration.rs` (que você deve abrir e copiar como referência antes de escrever este arquivo — NÃO duplicar a lógica de bootstrap, extraí-la para uma função helper se ainda não existir).

- [ ] **Step 1: Localizar o helper de bootstrap existente**

Run: `rg -n "testcontainers|pgvector" crates/garraia-gateway/tests/`
Ler o arquivo de teste de auth existente (`tests/auth_integration.rs` ou nome similar). Identificar a função que:
1. Sobe o container pgvector
2. Aplica migrations 001..010 via `sqlx::migrate!` ou script
3. Cria o `AppState` com `set_auth_components`
4. Retorna um `TestServer` / `Router` pronto.

Se o helper está em-arquivo, extrair para `crates/garraia-gateway/tests/common/mod.rs` como `pub async fn spawn_test_gateway() -> TestGateway` (TestGateway segura o container + pool + router). **Fazer a extração em um commit separado antes deste teste** (`refactor(tests): extract common gateway bootstrap`).

- [ ] **Step 2: Escrever os testes**

```rust
//! Integration tests for `GET /v1/me` (plan 0015, Fase 3.4 slice 1).

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use common::{spawn_test_gateway, seed_user_with_group};

#[tokio::test]
async fn get_v1_me_without_bearer_returns_401_problem_details() {
    let gw = spawn_test_gateway().await;
    let resp = gw
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/me")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let ct = resp.headers().get("content-type").unwrap();
    assert_eq!(ct, "application/problem+json");
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["status"], 401);
    assert_eq!(v["title"], "Unauthenticated");
}

#[tokio::test]
async fn get_v1_me_with_valid_bearer_no_group_returns_200_minimal_shape() {
    let gw = spawn_test_gateway().await;
    let (user_id, _group_id, token) = seed_user_with_group(&gw, "alice@example.com").await;

    let resp = gw
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/me")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["user_id"], user_id.to_string());
    assert!(v.get("group_id").is_none(), "no X-Group-Id → group_id must be absent");
    assert!(v.get("role").is_none());
}

#[tokio::test]
async fn get_v1_me_with_valid_bearer_and_member_group_returns_200_with_role() {
    let gw = spawn_test_gateway().await;
    let (user_id, group_id, token) = seed_user_with_group(&gw, "bob@example.com").await;

    let resp = gw
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/me")
                .header("authorization", format!("Bearer {token}"))
                .header("x-group-id", group_id.to_string())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["user_id"], user_id.to_string());
    assert_eq!(v["group_id"], group_id.to_string());
    assert!(v["role"].is_string(), "role must be present when group matches");
}

#[tokio::test]
async fn get_v1_me_with_valid_bearer_but_non_member_group_returns_403_problem_details() {
    let gw = spawn_test_gateway().await;
    let (_user_id, _group_id, token) = seed_user_with_group(&gw, "carol@example.com").await;
    let foreign_group = uuid::Uuid::new_v4();

    let resp = gw
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/me")
                .header("authorization", format!("Bearer {token}"))
                .header("x-group-id", foreign_group.to_string())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn openapi_spec_lists_get_v1_me() {
    let gw = spawn_test_gateway().await;
    let resp = gw
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(v["paths"]["/v1/me"]["get"].is_object());
    assert_eq!(v["info"]["version"], "0.1.0");
}
```

> `seed_user_with_group` deve ser adicionado a `tests/common/mod.rs`. Contrato: cria um usuário via `garraia_signup` pool ou INSERT direto, cria um group, cria row em `group_members` com `status='active'` e `role='owner'`, emite um JWT válido via o `JwtIssuer` exposto no `TestGateway`, retorna `(user_id, group_id, token_string)`.

- [ ] **Step 3: Rodar testes**

Run: `cargo test -p garraia-gateway --test rest_v1_me`
Expected: 5 testes PASS. Primeira execução pode demorar (download do container pgvector).

- [ ] **Step 4: Se algum teste falhar, diagnosticar antes de "corrigir"**

Não usar `unwrap_or`, não silenciar. Ler a mensagem, ver se é:
- (a) bug no handler → corrigir handler
- (b) wiring do router falhando → logar o `axum::Router` structure
- (c) extractor rejeitando por motivo diferente de 401 → adicionar `tracing_test::traced_test` no teste

- [ ] **Step 5: Commit**

```bash
git add crates/garraia-gateway/tests/rest_v1_me.rs crates/garraia-gateway/tests/common/
git commit -m "test(gateway): integration tests for GET /v1/me (plan 0015 t6)"
```

---

### Task 7: Smoke manual do `/docs` + `/v1/openapi.json`

**Files:** nenhum

- [ ] **Step 1: Subir o gateway localmente**

Exportar as env vars mínimas (mesmo conjunto que o `/v1/auth/*` usa):

```bash
export GARRAIA_JWT_SECRET="test-jwt-secret-at-least-32-bytes-long-xxxx"
export GARRAIA_REFRESH_HMAC_SECRET="test-refresh-hmac-secret-at-least-32-bytes"
export GARRAIA_LOGIN_DATABASE_URL="postgres://garraia_login@localhost:5432/garraia_dev"
export GARRAIA_SIGNUP_DATABASE_URL="postgres://garraia_signup@localhost:5432/garraia_dev"
```

Run: `cargo run -p garraia-cli -- gateway` (ou o entrypoint canônico — verificar no README do projeto).

- [ ] **Step 2: Curl `/v1/me` sem token**

Run: `curl -i http://127.0.0.1:3888/v1/me`
Expected: `HTTP/1.1 401 Unauthorized`, `content-type: application/problem+json`, body com `"title":"Unauthenticated"`.

- [ ] **Step 3: Abrir Swagger UI**

Abrir `http://127.0.0.1:3888/docs` no browser. Verificar:
- Título "GarraIA REST /v1"
- Endpoint `GET /v1/me` listado
- Schema `MeResponse` visível em Components
- Schema `ProblemDetails` visível em Components

- [ ] **Step 4: Baixar a spec e validar**

Run: `curl -s http://127.0.0.1:3888/v1/openapi.json | jq '.paths."/v1/me".get.responses | keys'`
Expected: `["200","401","403"]`

- [ ] **Step 5: Registro manual do smoke**

Anotar no `.garra-estado.md` (append-only) o timestamp + resultado do smoke. Nada a commitar aqui a menos que `.garra-estado.md` seja versionado.

---

### Task 8: Marcar checkbox no ROADMAP

**Files:**
- Modify: `ROADMAP.md`

- [ ] **Step 1: Editar só o item `GET /v1/me`**

Procurar a linha `- [ ] \`GET /v1/me\`` em `ROADMAP.md` (deve estar na subsessão "Grupos" ou em uma subsessão nova "Me" — se não existir, **não criar**; apenas verificar que o endpoint `GET /v1/me` está listado em algum lugar de §3.4. Se não estiver, adicionar **uma linha apenas** sob "Grupos": `- [x] \`GET /v1/me\``).

Não tocar em nenhum outro checkbox. Não tocar em `CLAUDE.md`.

- [ ] **Step 2: Commit**

```bash
git add ROADMAP.md
git commit -m "docs(roadmap): mark GET /v1/me shipped (plan 0015 slice 1)"
```

---

### Task 9: Validação final end-to-end

- [ ] **Step 1: Workspace check**

Run: `cargo check --workspace`
Expected: PASS.

- [ ] **Step 2: Clippy workspace**

Run: `cargo clippy --workspace -- -D warnings`
Expected: PASS (sem warnings novos).

- [ ] **Step 3: Testes do crate**

Run: `cargo test -p garraia-gateway`
Expected: todos os testes passam, incluindo `rest_v1_me` (5 novos) + suíte legada intacta.

- [ ] **Step 4: Verificar zero breaking change**

Run: `cargo test -p garraia-gateway --test auth_integration` (ou o nome do teste de `/v1/auth/*` existente).
Expected: PASS — rotas `/v1/auth/*` intactas.

- [ ] **Step 5: Git log sanity**

Run: `git log --oneline origin/main..HEAD`
Expected: ver os commits das Tasks 0-8 na ordem, cada um com escopo bem delimitado.

---

## §8 Rollback plan

Totalmente reversível. `git revert` dos commits das Tasks 0-8 (em ordem inversa) restaura o estado pré-plano. Não há:
- migration nova (sem schema change)
- env var nova (reusa `AuthConfig` existente)
- alteração em rotas legadas (só `.merge()` aditivo)
- mudança em `garraia-auth` (só consumo)

Se só parte do plano merged e o restante precisar ser desfeito, reverter do commit mais novo para o mais antigo — cada task é um commit independente.

## §12 Open questions (pré-start)

1. **`Role::as_str()` existe?** — validar com `grep -n "as_str\|impl Role" crates/garraia-auth/src/role.rs`. Se não, adicionar como parte da Task 4 (commit separado em `garraia-auth`).
2. **Helper de bootstrap de testes já extraído?** — Task 6 Step 1 decide; se já estiver em `tests/common/mod.rs`, reusar; se estiver inline, extrair primeiro.
3. **Versão exata do `utoipa` compatível com Axum 0.8?** — `utoipa 5.x` é a linha atual e declara suporte a Axum 0.8 via `axum_extras`. Se `cargo check` falhar na Task 1, fixar em `utoipa = "5.3"` explicitamente e reportar.

Estas são as únicas dúvidas bloqueantes. Nenhuma delas invalida o plano — são checkpoints da Task 1/4/6.

---

## Self-review checklist (executado pelo autor do plano, 2026-04-14)

- **Spec coverage:** única fatia é `GET /v1/me` + OpenAPI + Problem Details + 503 fail-soft. Task 4 cobre o handler, Task 2 o Problem Details, Task 5 o OpenAPI + router + `/docs`, Task 6 os testes de integração (5 cenários: 401, 200 sem grupo, 200 com grupo, 403 foreign group, spec listada). ✅
- **Placeholder scan:** sem TBD/TODO. Todo step com código mostra o código. ✅
- **Type consistency:** `RestError` enum usado consistentemente em Task 2 e 4. `RestV1State` definido Task 3 e consumido Task 4/5. `MeResponse` definido Task 4 e exportado Task 5. ✅
- **Ambiguidade:** Task 6 Step 1 explicita o que fazer se o helper existir vs. não existir. Task 8 explicita o que fazer se o checkbox não existir. ✅
