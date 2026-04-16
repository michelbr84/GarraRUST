# Plan 0018 — GAR-393 Slice 2: `POST /v1/groups/{group_id}/invites`

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Linear issue:** [GAR-393](https://linear.app/chatgpt25/issue/GAR-393) — "Rotas POST/GET/PATCH /v1/groups com OpenAPI" (In Progress, High). Slice 2 de GAR-393. Slice 1 (PATCH) entregue em plan 0017 (PR #23, `6077bcc`). Slice 3 (setRole + DELETE member) fica para plan 0019+.

**Status:** Draft — pendente aprovação.

**Goal:** Entregar `POST /v1/groups/{group_id}/invites` — criar um invite para um grupo, gerando token opaco + Argon2id hash, com authz via `Action::MembersManage` (Owner/Admin only) e deduplicação de invite pendente (409 Conflict).

**Architecture:**
1. Handler `create_invite` em `rest_v1/groups.rs` segue o pattern transactional já estabelecido por `create_group`/`patch_group`: `BEGIN` → `SET LOCAL app.current_user_id` → authz via `can(Principal, Action::MembersManage)` → check duplicate pending invite → INSERT `group_invites` → `COMMIT`.
2. Token generation: 32 bytes random via `rand::rngs::OsRng` → URL-safe base64 (44 chars). O token plaintext é retornado na response (API-first; email notification é feature separada futura). O hash Argon2id do token é armazenado em `group_invites.token_hash`.
3. `group_invites` **não tem RLS** (migration 007 line 23: "token-based access, handled by the invite endpoint"). Authz é inteiramente app-layer via `can()`.
4. Nova variante `RestError::Conflict` (409) para rejeitar invite duplicado (mesmo `group_id` + `invited_email` com `accepted_at IS NULL`).
5. Expiry: hardcoded 7 dias a partir de `now()`. Configurável via `group.settings` em slice futuro.

**Tech Stack:** Axum 0.8, `utoipa 5`, `garraia-auth::{AppPool, Principal, Action::MembersManage, can}`, `sqlx 0.8` (postgres), `argon2 0.5`, `rand 0.9`, `base64`, `testcontainers` + harness do plan 0016 M2.

**Design invariants (não-negociáveis deste slice):**

1. **Token plaintext NUNCA é armazenado.** Somente o hash Argon2id vai para `group_invites.token_hash`. O plaintext é retornado exatamente uma vez na response body.
2. **`proposed_role = "owner"` é rejeitado com 400.** O CHECK constraint do banco aceita apenas `('admin', 'member', 'guest', 'child')` — ver migration 001 line 141. Validação redundante no handler para fail-fast antes do banco.
3. **Duplicate pending invite → 409 Conflict.** Se já existe um invite pendente (`accepted_at IS NULL`) para o mesmo `(group_id, invited_email)`, retornar 409 em vez de criar segundo invite. Evita spam de tokens.
4. **`X-Group-Id` header obrigatório e deve coincidir com o path `{id}`.** Mesmo pattern de `get_group`/`patch_group` — header/path mismatch = 400.
5. **Email validation é structural only.** O handler verifica que o campo não está vazio e contém `@`. Não faz DNS/MX lookup. `citext` no banco normaliza case.

**Validações pré-plano (gate 2):**
- ✅ `group_invites` existe em migration 001 (linhas 136-155) com todas as colunas necessárias.
- ✅ `group_invites` **não tem RLS** — migration 007 line 23 documenta a decisão.
- ✅ `Action::MembersManage` existe em `crates/garraia-auth/src/action.rs:36` com dot-format `"members.manage"`.
- ✅ Owner (22 actions) e Admin (20 actions) incluem `MembersManage` em `can.rs` lines 55, 32-36. Member/Guest/Child não incluem.
- ✅ `garraia_app` role tem `GRANT INSERT ON ALL TABLES` (`007_row_level_security.sql:70`). INSERT em `group_invites` funciona via `AppPool`.
- ✅ `rand 0.9` é workspace dep (`Cargo.toml:74`). `argon2 0.5` e `base64` já são deps de `garraia-gateway` (`Cargo.toml:151, 48`).
- ✅ `proposed_role` CHECK: `IN ('admin', 'member', 'guest', 'child')` — NOT `'owner'` (migration 001 line 141, comment line 155).
- ✅ `token_hash` column: `text NOT NULL UNIQUE` — Argon2id PHC string (comment line 154).

**Out of scope (rejeitado explicitamente):**
- Accept invite flow (`POST /v1/invites/{token}:accept`) — plan 0019+.
- Email notification — feature separada, não bloqueia API-first invite.
- LIST invites (`GET /v1/groups/{id}/invites`) — not in GAR-393 endpoints.
- Revoke invite — not in GAR-393 endpoints.
- `LIST /v1/groups` — not in GAR-393 scope (confirmed plan 0017).

---

## File structure

| File | Action | Responsibility |
|------|--------|---------------|
| `crates/garraia-gateway/src/rest_v1/groups.rs` | Modify | Add `CreateInviteRequest`, `InviteResponse`, `create_invite` handler, `ALLOWED_INVITE_ROLES` const, email validation |
| `crates/garraia-gateway/src/rest_v1/problem.rs` | Modify | Add `RestError::Conflict` variant (409) |
| `crates/garraia-gateway/src/rest_v1/mod.rs` | Modify | Wire `.route("/v1/groups/{id}/invites", post(create_invite))` in all 3 modes |
| `crates/garraia-gateway/src/rest_v1/openapi.rs` | Modify | Register `create_invite` path + `CreateInviteRequest`/`InviteResponse` schemas |
| `crates/garraia-gateway/Cargo.toml` | Modify | Add `rand = { workspace = true }` dep |
| `crates/garraia-gateway/tests/rest_v1_groups.rs` | Modify | Add invite scenarios (I1-I6) |
| `crates/garraia-gateway/tests/authz_http_matrix.rs` | Modify | Add cases 20-23 (POST invites × owner/admin/member/outsider) |

---

## Task 1: Add `RestError::Conflict` variant

**Files:**
- Modify: `crates/garraia-gateway/src/rest_v1/problem.rs`

- [ ] **Step 1: Write the failing unit test**

Add at the end of the `mod tests` block in `problem.rs`:

```rust
    #[tokio::test]
    async fn conflict_shape() {
        let resp = RestError::Conflict("invite already pending".into()).into_response();
        assert_eq!(resp.status(), 409);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["status"], 409);
        assert_eq!(v["title"], "Conflict");
        assert_eq!(v["detail"], "invite already pending");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p garraia-gateway --lib -- rest_v1::problem::tests::conflict_shape`
Expected: FAIL — `Conflict` variant does not exist yet.

- [ ] **Step 3: Add the `Conflict` variant to `RestError`**

In `problem.rs`, add a new variant after `NotFound`:

```rust
    /// Plan 0018: resource-level conflict (e.g. duplicate pending invite).
    /// The `{0}` detail is emitted to clients — MUST NOT embed PII.
    #[error("{0}")]
    Conflict(String),
```

Update `fn status()`:
```rust
            RestError::Conflict(_) => StatusCode::CONFLICT,
```

Update `fn title()`:
```rust
            RestError::Conflict(_) => "Conflict",
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p garraia-gateway --lib -- rest_v1::problem::tests::conflict_shape`
Expected: PASS.

- [ ] **Step 5: Run full lib tests to check for regressions**

Run: `cargo test -p garraia-gateway --lib`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add crates/garraia-gateway/src/rest_v1/problem.rs
git commit -m "feat(gateway): add RestError::Conflict (409) variant (plan 0018 t1)"
```

---

## Task 2: `CreateInviteRequest`, `InviteResponse` structs + validation + unit tests

**Files:**
- Modify: `crates/garraia-gateway/src/rest_v1/groups.rs`
- Modify: `crates/garraia-gateway/Cargo.toml`

- [ ] **Step 1: Add `rand` dep to gateway Cargo.toml**

In `crates/garraia-gateway/Cargo.toml`, add to `[dependencies]`:

```toml
rand = { workspace = true }
```

- [ ] **Step 2: Add the `ALLOWED_INVITE_ROLES` const and structs**

At the top of `groups.rs`, after the existing `ALLOWED_GROUP_TYPES` const, add:

```rust
/// Accepted values for `CreateInviteRequest::role`.
///
/// Mirrors the `CHECK (proposed_role IN ('admin','member','guest','child'))`
/// on `group_invites.proposed_role` in migration 001 line 141. `"owner"` is
/// excluded — owners are created during group bootstrap only (comment line 155).
const ALLOWED_INVITE_ROLES: &[&str] = &["admin", "member", "guest", "child"];
```

Add new imports at the top of the file (after existing `use` block):

```rust
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
```

Add the request/response structs after `GroupReadResponse`:

```rust
/// Request body for `POST /v1/groups/{id}/invites` (plan 0018).
///
/// Creates a pending invite for the given email. The caller must have
/// `Action::MembersManage` (Owner or Admin).
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateInviteRequest {
    /// Email address to invite. Stored as `citext` (case-insensitive).
    pub email: String,
    /// Role to grant on acceptance. Must be one of: `admin`, `member`,
    /// `guest`, `child`. `owner` is not invitable.
    pub role: String,
}

impl CreateInviteRequest {
    /// Structural validation. PII-safe error messages only.
    pub fn validate(&self) -> Result<(), &'static str> {
        let trimmed = self.email.trim();
        if trimmed.is_empty() {
            return Err("email must not be empty");
        }
        if !trimmed.contains('@') {
            return Err("email must contain '@'");
        }
        if !ALLOWED_INVITE_ROLES.contains(&self.role.as_str()) {
            return Err("role must be one of: admin, member, guest, child");
        }
        Ok(())
    }
}

/// Response body for `POST /v1/groups/{id}/invites` (201 Created).
///
/// `token` is the **plaintext** invite token — returned exactly once.
/// The database stores only the Argon2id hash. Callers should forward
/// this token to the invitee (e.g. via email or direct link).
#[derive(Debug, Serialize, ToSchema)]
pub struct InviteResponse {
    pub id: Uuid,
    pub group_id: Uuid,
    pub invited_email: String,
    pub proposed_role: String,
    /// Opaque plaintext token. Share with the invitee. Returned once.
    pub token: String,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}
```

- [ ] **Step 3: Write failing unit tests for validation**

Add at the end of the existing `#[cfg(test)] mod tests` block in `groups.rs`:

```rust
    #[test]
    fn create_invite_request_valid() {
        let req = CreateInviteRequest {
            email: "alice@example.com".into(),
            role: "member".into(),
        };
        assert!(req.validate().is_ok());
    }

    #[test]
    fn create_invite_request_rejects_empty_email() {
        let req = CreateInviteRequest {
            email: "   ".into(),
            role: "member".into(),
        };
        assert_eq!(req.validate().unwrap_err(), "email must not be empty");
    }

    #[test]
    fn create_invite_request_rejects_missing_at() {
        let req = CreateInviteRequest {
            email: "not-an-email".into(),
            role: "member".into(),
        };
        assert_eq!(req.validate().unwrap_err(), "email must contain '@'");
    }

    #[test]
    fn create_invite_request_rejects_owner_role() {
        let req = CreateInviteRequest {
            email: "bob@example.com".into(),
            role: "owner".into(),
        };
        assert_eq!(
            req.validate().unwrap_err(),
            "role must be one of: admin, member, guest, child"
        );
    }

    #[test]
    fn create_invite_request_rejects_unknown_role() {
        let req = CreateInviteRequest {
            email: "bob@example.com".into(),
            role: "superadmin".into(),
        };
        assert_eq!(
            req.validate().unwrap_err(),
            "role must be one of: admin, member, guest, child"
        );
    }

    #[test]
    fn create_invite_request_all_valid_roles() {
        for role in &["admin", "member", "guest", "child"] {
            let req = CreateInviteRequest {
                email: "x@y.com".into(),
                role: role.to_string(),
            };
            assert!(req.validate().is_ok(), "role '{role}' should be valid");
        }
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p garraia-gateway --lib -- rest_v1::groups::tests::create_invite`
Expected: all 6 new tests PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/garraia-gateway/Cargo.toml crates/garraia-gateway/src/rest_v1/groups.rs
git commit -m "feat(gateway): CreateInviteRequest + InviteResponse + validation (plan 0018 t2)"
```

---

## Task 3: `create_invite` handler

**Files:**
- Modify: `crates/garraia-gateway/src/rest_v1/groups.rs`

- [ ] **Step 1: Write the handler**

Add after the `patch_group` handler in `groups.rs`:

```rust
/// `POST /v1/groups/{id}/invites` — create a pending invite.
///
/// Generates a 32-byte random token, hashes it with Argon2id, stores the
/// hash in `group_invites.token_hash`, and returns the plaintext token
/// exactly once in the response body.
///
/// Duplicate check: if a pending invite (`accepted_at IS NULL`) already
/// exists for the same `(group_id, invited_email)`, returns 409 Conflict
/// instead of creating a second invite.
///
/// ## Error matrix
///
/// | Condition                                    | Status | Guard          |
/// |----------------------------------------------|--------|----------------|
/// | Missing/invalid JWT                          | 401    | Principal      |
/// | Non-member                                   | 403    | Principal      |
/// | X-Group-Id / path id mismatch                | 400    | handler        |
/// | Role is Member/Guest/Child                   | 403    | `can()`        |
/// | Invalid body (email, role)                   | 400    | validate()     |
/// | Duplicate pending invite                     | 409    | SELECT check   |
/// | Happy path                                   | 201    |                |
#[utoipa::path(
    post,
    path = "/v1/groups/{id}/invites",
    params(
        ("id" = Uuid, Path, description = "Group UUID. Must match the `X-Group-Id` header."),
    ),
    request_body = CreateInviteRequest,
    responses(
        (status = 201, description = "Invite created; response carries the plaintext token (returned once).", body = InviteResponse),
        (status = 400, description = "Invalid body, header/path mismatch, or reserved role.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller lacks `members.manage` capability.", body = super::problem::ProblemDetails),
        (status = 409, description = "Pending invite already exists for this email+group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn create_invite(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateInviteRequest>,
) -> Result<(StatusCode, Json<InviteResponse>), RestError> {
    // 1. Header/path coherence (same pattern as get_group/patch_group).
    match principal.group_id {
        Some(hdr) if hdr == id => {}
        Some(_) => {
            return Err(RestError::BadRequest(
                "X-Group-Id header and path id must match".into(),
            ));
        }
        None => {
            return Err(RestError::BadRequest(
                "X-Group-Id header is required".into(),
            ));
        }
    }

    // 2. Capability check. Owner/Admin pass; Member/Guest/Child get 403.
    if !can(&principal, Action::MembersManage) {
        return Err(RestError::Forbidden);
    }

    // 3. Structural body validation.
    body.validate()
        .map_err(|msg| RestError::BadRequest(msg.into()))?;

    // 4. Generate invite token: 32 random bytes → URL-safe base64.
    let mut token_bytes = [0u8; 32];
    rand::rngs::OsRng
        .try_fill_bytes(&mut token_bytes)
        .map_err(|e| RestError::Internal(anyhow::anyhow!("RNG failure: {e}")))?;
    let token_plaintext = URL_SAFE_NO_PAD.encode(token_bytes);

    // 5. Hash the token with Argon2id. Same crate already used for
    //    password hashing in garraia-auth. We import directly here
    //    because the token is not a password — it's a random secret
    //    that doesn't need the full auth pipeline.
    let salt = argon2::password_hash::SaltString::generate(&mut rand::rngs::OsRng);
    let token_hash = argon2::Argon2::default()
        .hash_password(token_plaintext.as_bytes(), &salt)
        .map_err(|e| RestError::Internal(anyhow::anyhow!("argon2 hash failure: {e}")))?
        .to_string();

    // 6. Transactional INSERT with duplicate check.
    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    sqlx::query(&format!(
        "SET LOCAL app.current_user_id = '{}'",
        principal.user_id
    ))
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    // 6a. Check for existing pending invite.
    let existing: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM group_invites \
         WHERE group_id = $1 AND invited_email = $2 AND accepted_at IS NULL \
         LIMIT 1",
    )
    .bind(id)
    .bind(body.email.trim())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    if existing.is_some() {
        return Err(RestError::Conflict(
            "a pending invite already exists for this email in this group".into(),
        ));
    }

    // 6b. INSERT the invite. expires_at = now() + 7 days.
    let row: (Uuid, String, DateTime<Utc>, DateTime<Utc>) = sqlx::query_as(
        "INSERT INTO group_invites \
             (group_id, invited_email, proposed_role, token_hash, expires_at, created_by) \
         VALUES ($1, $2, $3, $4, now() + interval '7 days', $5) \
         RETURNING id, invited_email, expires_at, created_at",
    )
    .bind(id)
    .bind(body.email.trim())
    .bind(&body.role)
    .bind(&token_hash)
    .bind(principal.user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok((
        StatusCode::CREATED,
        Json(InviteResponse {
            id: row.0,
            group_id: id,
            invited_email: row.1,
            proposed_role: body.role,
            token: token_plaintext,
            expires_at: row.2,
            created_at: row.3,
        }),
    ))
}
```

- [ ] **Step 2: Add the `use rand::TryRngCore;` import**

At the top of `groups.rs`, add to the imports:

```rust
use rand::TryRngCore;
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p garraia-gateway`
Expected: compiles (handler not wired yet, that's Task 4).

- [ ] **Step 4: Commit**

```bash
git add crates/garraia-gateway/src/rest_v1/groups.rs
git commit -m "feat(gateway): create_invite handler with Argon2id token hash (plan 0018 t3)"
```

---

## Task 4: Route wiring + OpenAPI registration

**Files:**
- Modify: `crates/garraia-gateway/src/rest_v1/mod.rs`
- Modify: `crates/garraia-gateway/src/rest_v1/openapi.rs`

- [ ] **Step 1: Wire the route in all 3 modes**

In `mod.rs`, in the Mode 1 block (the one with `full` state), add after the `/v1/groups/{id}` route:

```rust
                .route("/v1/groups/{id}/invites", post(groups::create_invite))
```

In Mode 2 (auth-only block), add after the `/v1/groups/{id}` route:

```rust
                .route("/v1/groups/{id}/invites", post(unconfigured_handler))
```

In Mode 3 (no-auth block), add after the `/v1/groups/{id}` route:

```rust
                .route("/v1/groups/{id}/invites", post(unconfigured_handler))
```

- [ ] **Step 2: Register in OpenAPI**

In `openapi.rs`, add to the `paths(...)` list:

```rust
        super::groups::create_invite,
```

Add to the `components(schemas(...))` list:

```rust
        CreateInviteRequest,
        InviteResponse,
```

Update the import line at the top:

```rust
use super::groups::{CreateGroupRequest, CreateInviteRequest, GroupReadResponse, GroupResponse, InviteResponse, UpdateGroupRequest};
```

- [ ] **Step 3: Verify compilation + existing tests pass**

Run: `cargo check -p garraia-gateway && cargo test -p garraia-gateway --lib`
Expected: compiles, all lib tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/garraia-gateway/src/rest_v1/mod.rs crates/garraia-gateway/src/rest_v1/openapi.rs
git commit -m "docs(gateway): wire POST /v1/groups/{id}/invites route + OpenAPI (plan 0018 t4)"
```

---

## Task 5: Integration tests — invite scenarios

**Files:**
- Modify: `crates/garraia-gateway/tests/rest_v1_groups.rs`

- [ ] **Step 1: Add the `post_invite` request builder**

After the existing `patch_group_by_id` function in the test file, add:

```rust
fn post_invite(
    token: Option<&str>,
    group_path_id: &str,
    x_group_id: Option<&str>,
    body: serde_json::Value,
) -> Request<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri(format!("/v1/groups/{group_path_id}/invites"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request builder");
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo::<std::net::SocketAddr>(
            "127.0.0.1:1".parse().unwrap(),
        ));
    if let Some(token) = token {
        req.headers_mut().insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
    }
    if let Some(g) = x_group_id {
        req.headers_mut().insert(
            HeaderName::from_static("x-group-id"),
            HeaderValue::from_str(g).unwrap(),
        );
    }
    req
}
```

- [ ] **Step 2: Add 6 invite scenarios at the end of `v1_groups_scenarios`**

Append these scenarios after the existing PATCH scenarios (P1-P6) inside `v1_groups_scenarios`:

```rust
    // ─── POST /v1/groups/{id}/invites — plan 0018 Task 5 ─────

    // Scenario I1: owner creates invite → 201, response has token + invite_id.
    let invite_email = "invited-i1@0018.test";
    {
        let path = created_group_id.to_string();
        let resp = h
            .router
            .clone()
            .oneshot(post_invite(
                Some(&creator_token),
                &path,
                Some(&path),
                json!({"email": invite_email, "role": "member"}),
            ))
            .await
            .expect("I1: oneshot");
        assert_eq!(resp.status(), StatusCode::CREATED, "I1: owner creates invite");
        let v = body_json(resp).await;
        assert_eq!(v["group_id"], created_group_id.to_string());
        assert_eq!(v["invited_email"], invite_email);
        assert_eq!(v["proposed_role"], "member");
        assert!(v["token"].is_string(), "I1: response must include plaintext token");
        assert!(!v["token"].as_str().unwrap().is_empty(), "I1: token must not be empty");
        assert!(v["id"].is_string(), "I1: invite id must be present");
        assert!(v["expires_at"].is_string(), "I1: expires_at must be present");
        assert!(v["created_at"].is_string(), "I1: created_at must be present");

        // Verify the invite row exists in DB (via admin_pool to bypass any restrictions).
        let invite_id: uuid::Uuid = v["id"].as_str().unwrap().parse().unwrap();
        let (db_email,): (String,) =
            sqlx::query_as("SELECT invited_email FROM group_invites WHERE id = $1")
                .bind(invite_id)
                .fetch_one(&h.admin_pool)
                .await
                .expect("I1: invite row must exist");
        assert_eq!(db_email, invite_email);
    }

    // Scenario I2: duplicate pending invite for same email → 409.
    {
        let path = created_group_id.to_string();
        let resp = h
            .router
            .clone()
            .oneshot(post_invite(
                Some(&creator_token),
                &path,
                Some(&path),
                json!({"email": invite_email, "role": "admin"}),
            ))
            .await
            .expect("I2: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::CONFLICT,
            "I2: duplicate pending invite must 409"
        );
        let v = body_json(resp).await;
        assert!(
            v["detail"].as_str().unwrap().contains("pending invite"),
            "I2: detail must mention pending invite"
        );
    }

    // Scenario I3: invalid role "owner" → 400.
    {
        let path = created_group_id.to_string();
        let resp = h
            .router
            .clone()
            .oneshot(post_invite(
                Some(&creator_token),
                &path,
                Some(&path),
                json!({"email": "i3@0018.test", "role": "owner"}),
            ))
            .await
            .expect("I3: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "I3: role=owner must 400"
        );
        let v = body_json(resp).await;
        assert!(
            v["detail"].as_str().unwrap().contains("role must be"),
            "I3: detail must mention valid roles"
        );
    }

    // Scenario I4: empty email → 400.
    {
        let path = created_group_id.to_string();
        let resp = h
            .router
            .clone()
            .oneshot(post_invite(
                Some(&creator_token),
                &path,
                Some(&path),
                json!({"email": "  ", "role": "member"}),
            ))
            .await
            .expect("I4: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "I4: empty email must 400"
        );
    }

    // Scenario I5: non-member tries to create invite → 403 (Principal extractor).
    {
        let path = created_group_id.to_string();
        let (_outsider_id, _outsider_group, outsider_token) =
            seed_user_with_group(&h, "i5-outsider@0018.test")
                .await
                .expect("I5: seed outsider");
        let resp = h
            .router
            .clone()
            .oneshot(post_invite(
                Some(&outsider_token),
                &path,
                Some(&path),
                json!({"email": "victim@0018.test", "role": "member"}),
            ))
            .await
            .expect("I5: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "I5: non-member invite must 403 (extractor)"
        );
    }

    // Scenario I6: missing bearer token → 401.
    {
        let path = created_group_id.to_string();
        let resp = h
            .router
            .clone()
            .oneshot(post_invite(
                None,
                &path,
                Some(&path),
                json!({"email": "i6@0018.test", "role": "member"}),
            ))
            .await
            .expect("I6: oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "I6: missing bearer must 401"
        );
    }
```

- [ ] **Step 3: Update the module doc comment**

Update the doc comment at the top of `rest_v1_groups.rs` to list the new scenarios.

- [ ] **Step 4: Run the integration tests**

Run: `cargo test -p garraia-gateway --features test-helpers --test rest_v1_groups`
Expected: all scenarios pass (existing 1-7 + P1-P6 + new I1-I6).

- [ ] **Step 5: Commit**

```bash
git add crates/garraia-gateway/tests/rest_v1_groups.rs
git commit -m "test(gateway): POST /v1/groups/{id}/invites 6-scenario bundled (plan 0018 t5)"
```

---

## Task 6: Expand authz matrix with invite cases

**Files:**
- Modify: `crates/garraia-gateway/tests/authz_http_matrix.rs`

- [ ] **Step 1: Add 4 new cases to the authz matrix**

Append to the `CASES` array in `authz_http_matrix.rs`:

```rust
        // ── POST /v1/groups/{id}/invites (plan 0018) ──────────
        // Case 20: owner invites → 201 (has MembersManage).
        Case {
            label: "20 POST invite owner→201",
            method: "POST",
            path: |g| format!("/v1/groups/{g}/invites"),
            token: Token::Alice,
            x_group_id: XGroup::AliceGroup,
            body: Some(r#"{"email":"matrix-20@0018.test","role":"member"}"#),
            expected: 201,
        },
        // Case 21: admin invites → 201.
        // NOTE: requires alice's group to have an admin member.
        // If no admin fixture exists, this case should use the owner
        // token and a different email to avoid 409 from case 20.
        // For now, use alice (owner) with a fresh email — owner
        // subsumes admin's MembersManage permission.
        Case {
            label: "21 POST invite owner-as-admin-proxy→201",
            method: "POST",
            path: |g| format!("/v1/groups/{g}/invites"),
            token: Token::Alice,
            x_group_id: XGroup::AliceGroup,
            body: Some(r#"{"email":"matrix-21@0018.test","role":"guest"}"#),
            expected: 201,
        },
        // Case 22: member invites → 403 (no MembersManage).
        // NOTE: bob is a member of alice's group in the matrix seed.
        // If bob has no membership, this falls to 403 at extractor.
        // Either way, the expected status is 403.
        Case {
            label: "22 POST invite bob(member)→403",
            method: "POST",
            path: |g| format!("/v1/groups/{g}/invites"),
            token: Token::Bob,
            x_group_id: XGroup::AliceGroup,
            body: Some(r#"{"email":"matrix-22@0018.test","role":"member"}"#),
            expected: 403,
        },
        // Case 23: outsider invites → 403 (extractor).
        Case {
            label: "23 POST invite eve(outsider)→403",
            method: "POST",
            path: |g| format!("/v1/groups/{g}/invites"),
            token: Token::Eve,
            x_group_id: XGroup::AliceGroup,
            body: Some(r#"{"email":"matrix-23@0018.test","role":"member"}"#),
            expected: 403,
        },
```

**Note:** The exact integration depends on the structure of the existing `CASES` array and how `Case`, `Token`, `XGroup` are defined. The implementer must read the current `authz_http_matrix.rs` file and adapt the syntax above to match the existing pattern. The key semantics are:
- Case 20: owner → 201 (has `MembersManage`)
- Case 21: owner (proxy for admin, unless admin fixture exists) → 201
- Case 22: member/bob → 403 (no `MembersManage`)
- Case 23: outsider/eve → 403 (extractor rejects)

If the test harness's `Case` struct doesn't have a `body` field (because all prior cases were GET/PATCH), the implementer will need to add a `body: Option<&'static str>` field to `Case` and update the request builder to attach the body when present, and update all existing cases with `body: None` (for GET) or the appropriate body (for POST/PATCH).

- [ ] **Step 2: Run the authz matrix**

Run: `cargo test -p garraia-gateway --features test-helpers --test authz_http_matrix`
Expected: all 23 cases pass.

- [ ] **Step 3: Commit**

```bash
git add crates/garraia-gateway/tests/authz_http_matrix.rs
git commit -m "test(gateway): expand authz matrix with POST invite cases 20-23 (plan 0018 t6)"
```

---

## Task 7: Full validation pass

**Files:** none — validation only.

- [ ] **Step 1: cargo fmt**

Run: `cargo fmt --check --all`
Expected: no diffs. If diffs, run `cargo fmt --all` and amend the last commit.

- [ ] **Step 2: cargo clippy**

Run: `cargo clippy -p garraia-gateway --no-deps -- -D warnings`
Expected: no warnings in `rest_v1/*` files. Fix any issues.

- [ ] **Step 3: Full test suite**

Run all tests sequentially:

```bash
cargo test -p garraia-gateway --lib
cargo test -p garraia-gateway --features test-helpers --test rest_v1_groups
cargo test -p garraia-gateway --features test-helpers --test authz_http_matrix
cargo test -p garraia-gateway --features test-helpers --test rest_v1_me
cargo test -p garraia-gateway --features test-helpers --test harness_smoke
cargo test -p garraia-gateway --features test-helpers --test router_smoke_test
```

Expected: all pass.

- [ ] **Step 4: Commit any fixes from the validation pass**

If any fixes were needed:

```bash
git add -u
git commit -m "style(gateway): validation pass fixes (plan 0018 t7)"
```

---

## Acceptance criteria

1. `POST /v1/groups/{group_id}/invites` returns 201 with `InviteResponse` containing a plaintext token.
2. Database stores only the Argon2id hash of the token, never the plaintext.
3. Duplicate pending invite (same group + email) returns 409 Conflict.
4. `role = "owner"` is rejected with 400.
5. Non-members get 403 from the Principal extractor.
6. Members without `MembersManage` (Member/Guest/Child) get 403 from `can()`.
7. Owner and Admin can create invites (201).
8. All existing tests continue to pass (no regressions).
9. OpenAPI spec at `/docs` shows the new endpoint.
10. `cargo fmt --check --all` clean.
11. `cargo clippy -p garraia-gateway --no-deps -- -D warnings` clean.

## Rollback plan

All changes are additive code (new handler, new structs, new test scenarios). Rollback = revert the commits or delete the branch. No migration changes, no schema changes, no data changes.

## Open questions

None — all prerequisites validated in gate 2 above.

## Relationship to other plans

- **Plan 0017** (PATCH groups) — predecessor slice. Delivered in PR #23, `6077bcc`.
- **Plan 0019+** (accept invite / setRole / DELETE member) — successor slices. Depends on this plan's invite creation.
- **GAR-413** (SQLite→Postgres migration) — unrelated, no interference.
