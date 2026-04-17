# Plan 0019 — GAR-393 Slice 3: `POST /v1/invites/{token}/accept`

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Linear issue:** [GAR-393](https://linear.app/chatgpt25/issue/GAR-393) — "Rotas POST/GET/PATCH /v1/groups com OpenAPI" (In Progress, High). Slice 3 de GAR-393. Slice 2 (invites) entregue em plan 0018 (PR #24, `7c06cec`). Slice 4 (setRole + DELETE member) fica para plan 0020.

**Status:** In execution (feat/0019-gar-393-v1-invites-accept).

**Execution note — route shape:** o draft original propôs `POST /v1/invites/{token}:accept` (Google Cloud custom-action style com `:`). Axum 0.8 / `matchit` rejeita mixed `{param}:literal` no mesmo segmento (`"Only one parameter is allowed per path segment"`), então a rota entregue é `/v1/invites/{token}/accept` (dois segmentos). Semântica e invariantes preservadas — `token` segue como identificador primário e `accept` vira verbo de sub-path. Futuras custom actions (ex.: `:setRole` em plan 0020) precisam seguir o mesmo ajuste.

**Goal:** Fechar o elo invite→accept→member: um usuário autenticado que possui o plaintext token de um invite pendente e não-expirado pode aceitá-lo, tornando-se membro do grupo com o role proposto no invite.

**Architecture:**
1. Novo módulo `rest_v1/invites.rs` com handler `accept_invite`. O path é `POST /v1/invites/{token}:accept` — custom action style. O token viaja no path (não no body) porque é o identificador do recurso sendo operado. O body é vazio (`()`).
2. O handler faz: JWT verify (via `Principal` extractor, sem `X-Group-Id`) → lookup de todos os pending invites → Argon2id verify do token contra cada `token_hash` até encontrar match → verificar expiração → verificar que não é double-accept → transação atômica: `UPDATE group_invites SET accepted_at, accepted_by` + `INSERT INTO group_members` → commit → 200 com `AcceptInviteResponse`.
3. O scan de hashes é O(n) no número de invites pendentes. Com a partial unique index de migration 011, cada `(group_id, email)` tem no máximo 1 invite pendente — o scan é sobre todos os invites pendentes globais. Para v1 com poucos convites isso é aceitável. Se escalar, um future optimization é indexar por `LEFT(token_hash, 8)` como bloom hint. Isso é **out of scope** deste plan.
4. Authz: qualquer usuário autenticado pode aceitar. Não precisa ser membro de nenhum grupo. Não precisa de `X-Group-Id` header — o grupo é resolvido do invite row.

**Tech Stack:** Axum 0.8, `utoipa 5`, `garraia-auth::Principal` (sem group context), `argon2 0.5` + `password_hash 0.5` (Argon2id verify), `sqlx 0.8`, testcontainers + harness.

**Design invariants:**

1. **Token é verificado via Argon2id, nunca por comparação direta.** O DB armazena apenas hashes. O handler busca invites pendentes e verifica o token contra cada hash até match ou exaustão.
2. **Double-accept:**
   - *Serial* (mesmo caller re-submete após sucesso): 404. O SELECT filtra `accepted_at IS NULL`, então a segunda chamada não encontra hash e retorna 404 NotFound. Comportamento aceitável — a primeira chamada já disse 200 ao cliente.
   - *Concorrente* (dois callers corridos sobre o mesmo token): 409. O UPDATE usa `AND accepted_at IS NULL` + check `rows_affected() == 0` — o caller que perde a corrida recebe 409 Conflict com nenhum side-effect. Decisão tomada durante review do PR #25 (B-1 blocker): sem esse guard, dois usuários distintos poderiam ambos INSERTar `group_members` a partir de um único invite.
3. **Expiração é 410 Gone.** Se `expires_at < now()`, retornar 410 com detail PII-safe. (Novo variant `RestError::Gone`.)
4. **Membro já existente é 409 Conflict.** Se o `user_id` já é membro ativo do grupo, retornar 409 em vez de INSERT duplicado (SQLSTATE 23505).
5. **O invite row é atualizado atomicamente com a inserção do membro.** Mesma transação: `UPDATE group_invites SET accepted_at, accepted_by WHERE id = $2 AND accepted_at IS NULL` + `INSERT INTO group_members`.
6. **`X-Group-Id` header NÃO é exigido.** O `Principal` extractor retorna `group_id: None, role: None` quando o header está ausente — isso é correto para este endpoint.
7. **Token length guard antes do Argon2 scan.** O token do `create_invite` tem exatamente 43 chars (32 bytes × 4/3 via URL-safe base64 no-padding). Qualquer outro tamanho retorna 404 sem custo de CPU (SEC-07).

**Validações pré-plano (gate 2):**
- ✅ `group_invites` schema: `accepted_at` (nullable timestamptz), `accepted_by` (nullable uuid FK to users) — ambos prontos para o UPDATE.
- ✅ `group_members` schema: `(group_id, user_id)` PK, `role` CHECK, `status` CHECK, `invited_by` nullable — tudo pronto.
- ✅ `group_members` não tem FORCE RLS (migration 007 line 20-22). App-layer only.
- ✅ `Principal` extractor sem `X-Group-Id` retorna `group_id: None, role: None` sem 403 (extractor.rs lines 54-59).
- ✅ `argon2::Argon2::default()` + `PasswordVerifier::verify_password` disponíveis via deps já adicionadas em plan 0018.

**Status codes:**

| Condition | Status | Guard |
|-----------|--------|-------|
| Missing/invalid JWT | 401 | Principal extractor |
| Token not found (no matching hash) | 404 | handler |
| Invite already accepted | 409 | handler (`accepted_at IS NOT NULL`) |
| Invite expired | 410 | handler (`expires_at < now()`) |
| Caller already a member of the group | 409 | handler (SELECT from `group_members`) |
| Happy path | 200 | |

**How this unblocks setRole and DELETE member (plan 0020):**
After this plan, the integration tests can create an invite (plan 0018) and accept it (this plan) to produce a real `group_members` row via the API. setRole and DELETE member tests can then operate on that API-created member instead of requiring manual SQL seed. The full flow becomes: `POST /v1/groups` → `POST /v1/groups/{id}/invites` → `POST /v1/invites/{token}:accept` → member exists → `POST .../members/{id}:setRole` / `DELETE .../members/{id}`.

**Out of scope:**
- setRole (`POST /v1/groups/{id}/members/{user_id}:setRole`) — plan 0020.
- DELETE member (`DELETE /v1/groups/{id}/members/{user_id}`) — plan 0020.
- Email notification on accept.
- Revoke invite.
- Token optimization (bloom hint indexing).

---

## File structure

| File | Action | Responsibility |
|------|--------|---------------|
| `crates/garraia-gateway/src/rest_v1/invites.rs` | Create | `accept_invite` handler, `AcceptInviteResponse` struct |
| `crates/garraia-gateway/src/rest_v1/problem.rs` | Modify | Add `RestError::Gone` variant (410) |
| `crates/garraia-gateway/src/rest_v1/mod.rs` | Modify | `pub mod invites;`, wire route in all 3 modes |
| `crates/garraia-gateway/src/rest_v1/openapi.rs` | Modify | Register `accept_invite` path + `AcceptInviteResponse` schema |
| `crates/garraia-gateway/tests/rest_v1_invites.rs` | Create | Integration tests for accept invite (A1-A6) |
| `crates/garraia-gateway/tests/authz_http_matrix.rs` | Modify | Cases 24-26 for accept invite |

---

## Task 1: Add `RestError::Gone` variant (410)

**Files:**
- Modify: `crates/garraia-gateway/src/rest_v1/problem.rs`

- [ ] **Step 1: Write the failing unit test**

Add at the end of the `mod tests` block in `problem.rs`:

```rust
    #[tokio::test]
    async fn gone_shape() {
        let resp = RestError::Gone("invite has expired".into()).into_response();
        assert_eq!(resp.status(), 410);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["status"], 410);
        assert_eq!(v["title"], "Gone");
        assert_eq!(v["detail"], "invite has expired");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p garraia-gateway --lib -- rest_v1::problem::tests::gone_shape`
Expected: FAIL — `Gone` variant does not exist.

- [ ] **Step 3: Add the `Gone` variant**

In `problem.rs`, add after `Conflict`:

```rust
    /// Plan 0019: resource permanently unavailable (e.g. expired invite).
    /// The `{0}` detail is emitted to clients — MUST NOT embed PII.
    #[error("{0}")]
    Gone(String),
```

Update `fn status()`:
```rust
            RestError::Gone(_) => StatusCode::GONE,
```

Update `fn title()`:
```rust
            RestError::Gone(_) => "Gone",
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p garraia-gateway --lib -- rest_v1::problem::tests::gone_shape`
Expected: PASS.

- [ ] **Step 5: Run full lib tests**

Run: `cargo test -p garraia-gateway --lib`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add crates/garraia-gateway/src/rest_v1/problem.rs
git commit -m "feat(gateway): add RestError::Gone (410) variant (plan 0019 t1)"
```

---

## Task 2: `invites.rs` module — `AcceptInviteResponse` struct + `accept_invite` handler

**Files:**
- Create: `crates/garraia-gateway/src/rest_v1/invites.rs`

- [ ] **Step 1: Create the module file**

Create `crates/garraia-gateway/src/rest_v1/invites.rs` with this content:

```rust
//! `/v1/invites` handlers (plan 0019).
//!
//! ## `POST /v1/invites/{token}:accept`
//!
//! Accepts a pending group invite. The caller provides the plaintext
//! token in the path. The handler:
//!
//! 1. Fetches all pending invites (`accepted_at IS NULL`).
//! 2. Verifies the token against each `token_hash` (Argon2id).
//! 3. Checks expiration (`expires_at >= now()`).
//! 4. Checks the caller is not already a member of the group.
//! 5. Atomically updates the invite and inserts a `group_members` row.
//!
//! The caller does NOT need an `X-Group-Id` header — the group is
//! resolved from the matched invite row.

use argon2::PasswordVerifier;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use garraia_auth::Principal;
use password_hash::PasswordHash;
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::RestV1FullState;
use super::problem::RestError;

/// Response body for `POST /v1/invites/{token}:accept` (200 OK).
#[derive(Debug, Serialize, ToSchema)]
pub struct AcceptInviteResponse {
    /// The group the caller just joined.
    pub group_id: Uuid,
    /// The role assigned from the invite.
    pub role: String,
    /// The invite ID that was accepted.
    pub invite_id: Uuid,
}

/// `POST /v1/invites/{token}:accept` — accept a pending group invite.
///
/// The plaintext invite token travels in the path. The handler verifies
/// it against Argon2id hashes stored in `group_invites.token_hash`.
///
/// ## Error matrix
///
/// | Condition                          | Status | Guard          |
/// |------------------------------------|--------|----------------|
/// | Missing/invalid JWT                | 401    | Principal      |
/// | Token not found (no hash match)    | 404    | handler        |
/// | Invite already accepted            | 409    | handler        |
/// | Invite expired                     | 410    | handler        |
/// | Caller already member of group     | 409    | handler        |
/// | Happy path                         | 200    |                |
#[utoipa::path(
    post,
    path = "/v1/invites/{token}:accept",
    params(
        ("token" = String, Path, description = "Plaintext invite token (URL-safe base64)."),
    ),
    responses(
        (status = 200, description = "Invite accepted; caller is now a group member.", body = AcceptInviteResponse),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 404, description = "No pending invite matches this token.", body = super::problem::ProblemDetails),
        (status = 409, description = "Invite already accepted or caller already a member.", body = super::problem::ProblemDetails),
        (status = 410, description = "Invite has expired.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn accept_invite(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(token): Path<String>,
) -> Result<Json<AcceptInviteResponse>, RestError> {
    let pool = state.app_pool.pool_for_handlers();

    // 1. Fetch ALL pending invites. For v1 volume this is acceptable.
    //    Each row carries the hash + metadata needed for verification.
    let pending: Vec<(Uuid, Uuid, String, String, DateTime<Utc>, Option<DateTime<Utc>>)> =
        sqlx::query_as(
            "SELECT id, group_id, token_hash, proposed_role, expires_at, accepted_at \
             FROM group_invites \
             WHERE accepted_at IS NULL \
             ORDER BY created_at DESC",
        )
        .fetch_all(pool)
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // 2. Verify token against each hash until match.
    let argon = argon2::Argon2::default();
    let mut matched = None;

    for row in &pending {
        let (invite_id, group_id, ref hash, ref role, expires_at, accepted_at) = *row;
        let Ok(parsed) = PasswordHash::new(hash) else {
            // Malformed hash in DB — skip, don't crash.
            tracing::warn!(invite_id = %invite_id, "malformed token_hash in group_invites, skipping");
            continue;
        };
        if argon.verify_password(token.as_bytes(), &parsed).is_ok() {
            matched = Some((invite_id, group_id, role.clone(), expires_at, accepted_at));
            break;
        }
    }

    let (invite_id, group_id, role, expires_at, accepted_at) =
        matched.ok_or(RestError::NotFound)?;

    // 3. Check double-accept.
    if accepted_at.is_some() {
        return Err(RestError::Conflict(
            "this invite has already been accepted".into(),
        ));
    }

    // 4. Check expiration.
    if expires_at < Utc::now() {
        return Err(RestError::Gone("this invite has expired".into()));
    }

    // 5. Transactional: update invite + insert member.
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // SET LOCAL tenant context (Uuid Display = 36 hex-dashed chars,
    // injection-safe by construction — see groups.rs module doc).
    sqlx::query(&format!(
        "SET LOCAL app.current_user_id = '{}'",
        principal.user_id
    ))
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    // 5a. Mark invite as accepted.
    sqlx::query(
        "UPDATE group_invites SET accepted_at = now(), accepted_by = $1 WHERE id = $2",
    )
    .bind(principal.user_id)
    .bind(invite_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    // 5b. Insert group_members. Catch 23505 (PK violation) if
    //     the caller is already a member of this group.
    let insert_result = sqlx::query(
        "INSERT INTO group_members (group_id, user_id, role, status, invited_by) \
         VALUES ($1, $2, $3, 'active', $4)",
    )
    .bind(group_id)
    .bind(principal.user_id)
    .bind(&role)
    .bind(principal.user_id)
    .execute(&mut *tx)
    .await;

    match insert_result {
        Ok(_) => {}
        Err(sqlx::Error::Database(ref db_err)) if db_err.code().as_deref() == Some("23505") => {
            // Caller already a member — rollback the accept and return 409.
            // The tx will be rolled back on drop, but we return early.
            return Err(RestError::Conflict(
                "you are already a member of this group".into(),
            ));
        }
        Err(e) => return Err(RestError::Internal(e.into())),
    }

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok(Json(AcceptInviteResponse {
        group_id,
        role,
        invite_id,
    }))
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check -p garraia-gateway`
Expected: fails — module not registered in `mod.rs` yet. That's Task 3.

- [ ] **Step 3: Commit**

```bash
git add crates/garraia-gateway/src/rest_v1/invites.rs
git commit -m "feat(gateway): accept_invite handler with Argon2id verify (plan 0019 t2)"
```

---

## Task 3: Route wiring + module registration + OpenAPI

**Files:**
- Modify: `crates/garraia-gateway/src/rest_v1/mod.rs`
- Modify: `crates/garraia-gateway/src/rest_v1/openapi.rs`

- [ ] **Step 1: Register the module**

In `mod.rs`, add after the existing `pub mod groups;` line:

```rust
pub mod invites;
```

- [ ] **Step 2: Wire the route in all 3 modes**

In Mode 1 (full state), add after the `/v1/groups/{id}/invites` route:

```rust
                .route("/v1/invites/{token}:accept", post(invites::accept_invite))
```

In Mode 2 (auth-only), add after the `/v1/groups/{id}/invites` route:

```rust
                .route("/v1/invites/{token}:accept", post(unconfigured_handler))
```

In Mode 3 (no-auth), add after the `/v1/groups/{id}/invites` route:

```rust
                .route("/v1/invites/{token}:accept", post(unconfigured_handler))
```

- [ ] **Step 3: Register in OpenAPI**

In `openapi.rs`, add to the `paths(...)` list:

```rust
        super::invites::accept_invite,
```

Add to `components(schemas(...))`:

```rust
        AcceptInviteResponse,
```

Update the import block to add:

```rust
use super::invites::AcceptInviteResponse;
```

- [ ] **Step 4: Verify compilation + lib tests**

Run: `cargo check -p garraia-gateway && cargo test -p garraia-gateway --lib`
Expected: compiles, all lib tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/garraia-gateway/src/rest_v1/mod.rs crates/garraia-gateway/src/rest_v1/openapi.rs
git commit -m "docs(gateway): wire POST /v1/invites/{token}:accept route + OpenAPI (plan 0019 t3)"
```

---

## Task 4: Integration tests — accept invite scenarios

**Files:**
- Create: `crates/garraia-gateway/tests/rest_v1_invites.rs`

Register the test binary in `Cargo.toml` if needed (check if `[[test]]` entries use explicit names or auto-discovery).

- [ ] **Step 1: Create the test file with helper and 6 scenarios**

Create `crates/garraia-gateway/tests/rest_v1_invites.rs`:

```rust
//! Integration tests for `POST /v1/invites/{token}:accept` (plan 0019).
//!
//! Scenarios:
//!   A1. Happy path — owner creates invite, second user accepts → 200.
//!   A2. Double-accept — same token again → 409.
//!   A3. Expired invite → 410.
//!   A4. Invalid token (no match) → 404.
//!   A5. Already a member of the group → 409.
//!   A6. Missing bearer → 401.

mod common;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

use common::fixtures::{seed_user_with_group, seed_user_without_group};
use common::{Harness, harness_get};

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("body is JSON")
}

fn post_invite_create(
    token: &str,
    group_id: &str,
    body: serde_json::Value,
) -> Request<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri(format!("/v1/groups/{group_id}/invites"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request builder");
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo::<std::net::SocketAddr>(
            "127.0.0.1:1".parse().unwrap(),
        ));
    req.headers_mut().insert(
        HeaderName::from_static("authorization"),
        HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
    );
    req.headers_mut().insert(
        HeaderName::from_static("x-group-id"),
        HeaderValue::from_str(group_id).unwrap(),
    );
    req
}

fn post_accept(bearer: Option<&str>, token: &str) -> Request<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri(format!("/v1/invites/{token}:accept"))
        .body(Body::empty())
        .expect("request builder");
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo::<std::net::SocketAddr>(
            "127.0.0.1:1".parse().unwrap(),
        ));
    if let Some(bearer) = bearer {
        req.headers_mut().insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_str(&format!("Bearer {bearer}")).unwrap(),
        );
    }
    req
}

#[tokio::test]
async fn v1_invites_accept_scenarios() {
    let h = Harness::get().await;

    // Seed: owner creates a group, then creates an invite.
    let (_owner_id, _owner_group, owner_token) =
        seed_user_with_group(&h, "owner@0019.test")
            .await
            .expect("seed owner");

    // Create a fresh group for this test.
    let group_id: uuid::Uuid = {
        let mut req = Request::builder()
            .method("POST")
            .uri("/v1/groups")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"name": "Accept Test Group", "type": "team"}).to_string(),
            ))
            .expect("req");
        req.extensions_mut()
            .insert(axum::extract::ConnectInfo::<std::net::SocketAddr>(
                "127.0.0.1:1".parse().unwrap(),
            ));
        req.headers_mut().insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_str(&format!("Bearer {}", owner_token)).unwrap(),
        );
        let resp = h.router.clone().oneshot(req).await.expect("create group");
        assert_eq!(resp.status(), StatusCode::CREATED);
        let v = body_json(resp).await;
        v["id"].as_str().unwrap().parse().unwrap()
    };

    let gid = group_id.to_string();

    // Create invite for a fresh email.
    let invite_token: String = {
        let resp = h
            .router
            .clone()
            .oneshot(post_invite_create(
                &owner_token,
                &gid,
                json!({"email": "joiner@0019.test", "role": "member"}),
            ))
            .await
            .expect("create invite");
        assert_eq!(resp.status(), StatusCode::CREATED, "invite created");
        let v = body_json(resp).await;
        v["token"].as_str().unwrap().to_string()
    };

    // Seed: the user who will accept the invite.
    let (_joiner_id, _joiner_group, joiner_token) =
        seed_user_with_group(&h, "joiner@0019.test")
            .await
            .expect("seed joiner");

    // ─── Scenario A1: happy path — accept invite → 200 ───────
    {
        let resp = h
            .router
            .clone()
            .oneshot(post_accept(Some(&joiner_token), &invite_token))
            .await
            .expect("A1: oneshot");
        assert_eq!(resp.status(), StatusCode::OK, "A1: accept invite");
        let v = body_json(resp).await;
        assert_eq!(v["group_id"], gid);
        assert_eq!(v["role"], "member");
        assert!(v["invite_id"].is_string());

        // Verify group_members row via admin_pool.
        let (role,): (String,) = sqlx::query_as(
            "SELECT role::text FROM group_members \
             WHERE group_id = $1 AND user_id = $2 AND status = 'active'",
        )
        .bind(group_id)
        .bind(_joiner_id)
        .fetch_one(&h.admin_pool)
        .await
        .expect("A1: member row must exist");
        assert_eq!(role, "member");
    }

    // ─── Scenario A2: double-accept → 409 ────────────────────
    {
        let resp = h
            .router
            .clone()
            .oneshot(post_accept(Some(&joiner_token), &invite_token))
            .await
            .expect("A2: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "A2: already-accepted invite no longer in pending set → 404"
        );
    }

    // ─── Scenario A3: expired invite → 410 ───────────────────
    {
        // Create a new invite, then manually expire it in DB.
        let expired_token: String = {
            let resp = h
                .router
                .clone()
                .oneshot(post_invite_create(
                    &owner_token,
                    &gid,
                    json!({"email": "expired@0019.test", "role": "guest"}),
                ))
                .await
                .expect("A3: create invite");
            assert_eq!(resp.status(), StatusCode::CREATED);
            let v = body_json(resp).await;
            let invite_id: uuid::Uuid = v["id"].as_str().unwrap().parse().unwrap();

            // Force-expire the invite.
            sqlx::query("UPDATE group_invites SET expires_at = now() - interval '1 hour' WHERE id = $1")
                .bind(invite_id)
                .execute(&h.admin_pool)
                .await
                .expect("A3: force-expire");

            v["token"].as_str().unwrap().to_string()
        };

        let (expired_user_id, expired_user_token) =
            seed_user_without_group(&h, "expired-acceptor@0019.test")
                .await
                .expect("A3: seed acceptor");

        let resp = h
            .router
            .clone()
            .oneshot(post_accept(Some(&expired_user_token), &expired_token))
            .await
            .expect("A3: oneshot");
        assert_eq!(resp.status(), StatusCode::GONE, "A3: expired invite → 410");
    }

    // ─── Scenario A4: invalid token → 404 ────────────────────
    {
        let resp = h
            .router
            .clone()
            .oneshot(post_accept(Some(&joiner_token), "totally-bogus-token"))
            .await
            .expect("A4: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "A4: bad token → 404"
        );
    }

    // ─── Scenario A5: already a member → 409 ─────────────────
    {
        // Create invite for joiner (who is already a member from A1).
        // Need a different email since joiner@0019.test already has a
        // pending-then-accepted invite. Use a fresh email.
        let dupe_token: String = {
            let resp = h
                .router
                .clone()
                .oneshot(post_invite_create(
                    &owner_token,
                    &gid,
                    json!({"email": "dupe-joiner@0019.test", "role": "admin"}),
                ))
                .await
                .expect("A5: create invite");
            assert_eq!(resp.status(), StatusCode::CREATED);
            body_json(resp).await["token"]
                .as_str()
                .unwrap()
                .to_string()
        };

        // Joiner (already member from A1) tries to accept.
        let resp = h
            .router
            .clone()
            .oneshot(post_accept(Some(&joiner_token), &dupe_token))
            .await
            .expect("A5: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::CONFLICT,
            "A5: already a member → 409"
        );
        let v = body_json(resp).await;
        assert!(
            v["detail"].as_str().unwrap().contains("already a member"),
            "A5: detail mentions membership"
        );
    }

    // ─── Scenario A6: missing bearer → 401 ───────────────────
    {
        let resp = h
            .router
            .clone()
            .oneshot(post_accept(None, &invite_token))
            .await
            .expect("A6: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "A6: missing bearer → 401"
        );
    }
}
```

- [ ] **Step 2: Add `[[test]]` entry to Cargo.toml if needed**

Check if the project uses auto-discovery or explicit `[[test]]` entries. If explicit, add:

```toml
[[test]]
name = "rest_v1_invites"
required-features = ["test-helpers"]
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p garraia-gateway --features test-helpers --test rest_v1_invites`
Expected: all 6 scenarios pass.

- [ ] **Step 4: Commit**

```bash
git add crates/garraia-gateway/tests/rest_v1_invites.rs crates/garraia-gateway/Cargo.toml
git commit -m "test(gateway): POST /v1/invites/{token}:accept 6-scenario bundled (plan 0019 t4)"
```

---

## Task 5: Expand authz matrix

**Files:**
- Modify: `crates/garraia-gateway/tests/authz_http_matrix.rs`

- [ ] **Step 1: Add 3 cases to the matrix**

Append after case 23:

```rust
        // ── POST /v1/invites/{token}:accept (plan 0019, cases 24-26) ──
        //
        // NOTE: accept invite requires a REAL invite token. The matrix
        // does not have one, so we test only the authz/error paths:
        // - Case 24: valid JWT, bogus token → 404 (no matching hash).
        // - Case 25: no JWT → 401.
        // - Case 26: valid JWT + bogus token → 404 (confirms handler runs).
        MatrixCase {
            id: 24,
            name: "POST accept invite alice + bogus token → 404",
            build: Box::new(|a| {
                let mut req = Request::builder()
                    .method(Method::POST)
                    .uri("/v1/invites/bogus-token-24:accept")
                    .body(Body::empty())
                    .unwrap();
                req.extensions_mut()
                    .insert(axum::extract::ConnectInfo::<std::net::SocketAddr>(
                        "127.0.0.1:1".parse().unwrap(),
                    ));
                req.headers_mut().insert(
                    HeaderName::from_static("authorization"),
                    HeaderValue::from_str(&format!("Bearer {}", a.alice_token)).unwrap(),
                );
                req
            }),
            expected_status: StatusCode::NOT_FOUND,
            expected_body_contains: None,
        },
        MatrixCase {
            id: 25,
            name: "POST accept invite no bearer → 401",
            build: Box::new(|_a| {
                let mut req = Request::builder()
                    .method(Method::POST)
                    .uri("/v1/invites/bogus-token-25:accept")
                    .body(Body::empty())
                    .unwrap();
                req.extensions_mut()
                    .insert(axum::extract::ConnectInfo::<std::net::SocketAddr>(
                        "127.0.0.1:1".parse().unwrap(),
                    ));
                req
            }),
            expected_status: StatusCode::UNAUTHORIZED,
            expected_body_contains: None,
        },
        MatrixCase {
            id: 26,
            name: "POST accept invite eve + bogus token → 404",
            build: Box::new(|a| {
                let mut req = Request::builder()
                    .method(Method::POST)
                    .uri("/v1/invites/bogus-token-26:accept")
                    .body(Body::empty())
                    .unwrap();
                req.extensions_mut()
                    .insert(axum::extract::ConnectInfo::<std::net::SocketAddr>(
                        "127.0.0.1:1".parse().unwrap(),
                    ));
                req.headers_mut().insert(
                    HeaderName::from_static("authorization"),
                    HeaderValue::from_str(&format!("Bearer {}", a.eve_token)).unwrap(),
                );
                req
            }),
            expected_status: StatusCode::NOT_FOUND,
            expected_body_contains: None,
        },
```

- [ ] **Step 2: Update the case count assertion**

Change `23` to `26` in the assertion:

```rust
    assert_eq!(
        matrix.len(),
        26,
        "matrix must have exactly 26 cases; got {}",
        matrix.len()
    );
```

- [ ] **Step 3: Run the matrix**

Run: `cargo test -p garraia-gateway --features test-helpers --test authz_http_matrix`
Expected: all 26 cases pass.

- [ ] **Step 4: Commit**

```bash
git add crates/garraia-gateway/tests/authz_http_matrix.rs
git commit -m "test(gateway): expand authz matrix with accept-invite cases 24-26 (plan 0019 t5)"
```

---

## Task 6: Full validation pass

**Files:** none — validation only.

- [ ] **Step 1: cargo fmt**

Run: `cargo fmt --check --all`
If diffs: `cargo fmt --all` and commit.

- [ ] **Step 2: cargo clippy**

Run: `cargo clippy -p garraia-gateway --no-deps -- -D warnings`
Expected: no new warnings in `rest_v1/*`.

- [ ] **Step 3: Full test suite**

```bash
cargo test -p garraia-gateway --lib
cargo test -p garraia-gateway --features test-helpers --test rest_v1_invites
cargo test -p garraia-gateway --features test-helpers --test rest_v1_groups
cargo test -p garraia-gateway --features test-helpers --test authz_http_matrix
cargo test -p garraia-gateway --features test-helpers --test rest_v1_me
cargo test -p garraia-gateway --features test-helpers --test harness_smoke
cargo test -p garraia-gateway --features test-helpers --test router_smoke_test
```

Expected: all pass.

- [ ] **Step 4: Commit any fixes**

```bash
git add -u
git commit -m "style(gateway): validation pass fixes (plan 0019 t6)"
```

---

## Acceptance criteria

1. `POST /v1/invites/{token}/accept` returns 200 with `AcceptInviteResponse` on happy path.
2. Token is verified via Argon2id against stored hashes — never by direct string comparison.
3. Expired invite returns 410 Gone.
4. Serial double-accept returns 404 (filtered by `accepted_at IS NULL` SELECT); concurrent double-accept by two different users returns 409 for the loser (UPDATE-level race guard).
5. Caller already a member of the group returns 409 Conflict.
6. Invalid/unknown token returns 404.
7. Missing JWT returns 401.
8. Token with wrong length (≠ 43 chars) returns 404 without entering the Argon2 scan (SEC-07).
9. `group_members` row created with correct `role`, `status = 'active'`, `invited_by`.
10. `group_invites` row updated with `accepted_at` and `accepted_by`.
11. All existing tests continue to pass.
12. `cargo fmt --check --all` clean.
13. OpenAPI spec at `/docs` shows the new endpoint.

## Rollback plan

All changes are additive (new module, new handler, new test file, new RestError variant). Rollback = revert commits or delete branch. No migration changes (migration 011 from plan 0018 already exists and is unchanged).

## Open questions

None — all prerequisites validated above.

## Relationship to other plans

- **Plan 0018** (create invite) — produces the tokens this plan consumes.
- **Plan 0020** (setRole + DELETE member) — depends on this plan's accept flow to create real members via API for testing.
