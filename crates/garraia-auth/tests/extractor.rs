//! Integration tests for the `Principal` Axum extractor (GAR-391c).
//!
//! Seven scenarios:
//!   1. Valid JWT + no X-Group-Id → Principal with `role=None`.
//!   2. Missing Authorization → 401 unauthenticated.
//!   3. Malformed JWT → 401 unauthenticated.
//!   4. Expired JWT → 401 unauthenticated.
//!   5. Valid JWT + malformed X-Group-Id → 400 invalid X-Group-Id.
//!   6. Valid JWT + UUID for a group the user is not a member of → 403 forbidden.
//!   7. `RequirePermission` positive + negative.
//!
//! Tests that need membership lookup boot a `pgvector/pg16` testcontainer
//! and apply the workspace migration set — same pattern as
//! `tests/verify_internal.rs`.

use std::sync::Arc;

use axum::extract::{FromRef, FromRequestParts};
use axum::http::{HeaderValue, Request, StatusCode, header};
use chrono::Utc;
use garraia_auth::{
    Action, JwtConfig, JwtIssuer, LoginConfig, LoginPool, Principal, Role, require_permission,
};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use secrecy::SecretString;
use serde::Serialize;
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, ImageExt};
use testcontainers_modules::postgres::Postgres as PgImage;
use uuid::Uuid;

/// Test application state exposing `Arc<JwtIssuer>` and `Arc<LoginPool>`.
#[derive(Clone)]
struct TestState {
    jwt: Arc<JwtIssuer>,
    login: Arc<LoginPool>,
}

impl FromRef<TestState> for Arc<JwtIssuer> {
    fn from_ref(s: &TestState) -> Self {
        s.jwt.clone()
    }
}

impl FromRef<TestState> for Arc<LoginPool> {
    fn from_ref(s: &TestState) -> Self {
        s.login.clone()
    }
}

fn jwt_cfg() -> JwtConfig {
    JwtConfig {
        jwt_secret: SecretString::from("a".repeat(32)),
        refresh_hmac_secret: SecretString::from("b".repeat(32)),
    }
}

async fn boot_login_pool() -> anyhow::Result<(ContainerAsync<PgImage>, sqlx::PgPool, Arc<LoginPool>)>
{
    let container = PgImage::default()
        .with_name("pgvector/pgvector")
        .with_tag("pg16")
        .start()
        .await?;
    let host = container.get_host().await?;
    let port = container.get_host_port_ipv4(5432).await?;
    let postgres_url = format!("postgres://postgres:postgres@{host}:{port}/postgres");

    garraia_workspace::Workspace::connect(garraia_workspace::WorkspaceConfig {
        database_url: postgres_url.clone(),
        max_connections: 5,
        migrate_on_start: true,
    })
    .await?;

    let admin = sqlx::PgPool::connect(&postgres_url).await?;
    sqlx::query("ALTER ROLE garraia_login WITH LOGIN PASSWORD 'test-password'")
        .execute(&admin)
        .await?;

    let login_url = postgres_url.replace("postgres:postgres@", "garraia_login:test-password@");
    let pool = Arc::new(
        LoginPool::from_dedicated_config(&LoginConfig {
            database_url: login_url,
            max_connections: 4,
        })
        .await?,
    );
    Ok((container, admin, pool))
}

fn make_state(jwt: Arc<JwtIssuer>, login: Arc<LoginPool>) -> TestState {
    TestState { jwt, login }
}

async fn extract_principal(
    state: &TestState,
    req: Request<()>,
) -> Result<Principal, (StatusCode, &'static str)> {
    let (mut parts, _) = req.into_parts();
    Principal::from_request_parts(&mut parts, state).await
}

/// Expired access-claim synthesizer: we sign directly with the JWT secret so
/// the resulting token has `exp` in the past but passes the HS256 signature.
#[derive(Serialize)]
struct RawClaims {
    sub: Uuid,
    iat: i64,
    exp: i64,
    iss: String,
}

fn sign_expired(secret: &[u8], user_id: Uuid) -> String {
    let now = Utc::now().timestamp();
    let claims = RawClaims {
        sub: user_id,
        iat: now - 7200,
        exp: now - 3600,
        iss: "garraia-gateway".into(),
    };
    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn valid_jwt_without_group_header_returns_principal_with_no_role() -> anyhow::Result<()> {
    let (_c, _admin, pool) = boot_login_pool().await?;
    let issuer = Arc::new(JwtIssuer::new(jwt_cfg())?);
    let state = make_state(issuer.clone(), pool);

    let user_id = Uuid::now_v7();
    let (token, _) = issuer.issue_access(user_id)?;

    let req = Request::builder()
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(())?;
    let p = extract_principal(&state, req).await.expect("extractor ok");
    assert_eq!(p.user_id, user_id);
    assert!(p.group_id.is_none());
    assert!(p.role.is_none());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_authorization_header_returns_401() -> anyhow::Result<()> {
    let (_c, _admin, pool) = boot_login_pool().await?;
    let issuer = Arc::new(JwtIssuer::new(jwt_cfg())?);
    let state = make_state(issuer, pool);

    let req = Request::builder().body(())?;
    let err = extract_principal(&state, req).await.unwrap_err();
    assert_eq!(err.0, StatusCode::UNAUTHORIZED);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_jwt_returns_401() -> anyhow::Result<()> {
    let (_c, _admin, pool) = boot_login_pool().await?;
    let issuer = Arc::new(JwtIssuer::new(jwt_cfg())?);
    let state = make_state(issuer, pool);

    let req = Request::builder()
        .header(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer not.a.jwt"),
        )
        .body(())?;
    let err = extract_principal(&state, req).await.unwrap_err();
    assert_eq!(err.0, StatusCode::UNAUTHORIZED);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn expired_jwt_returns_401() -> anyhow::Result<()> {
    let (_c, _admin, pool) = boot_login_pool().await?;
    let issuer = Arc::new(JwtIssuer::new(jwt_cfg())?);
    let state = make_state(issuer, pool);

    let token = sign_expired(&"a".repeat(32).into_bytes(), Uuid::now_v7());
    let req = Request::builder()
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(())?;
    let err = extract_principal(&state, req).await.unwrap_err();
    assert_eq!(err.0, StatusCode::UNAUTHORIZED);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_group_id_header_returns_400() -> anyhow::Result<()> {
    let (_c, _admin, pool) = boot_login_pool().await?;
    let issuer = Arc::new(JwtIssuer::new(jwt_cfg())?);
    let state = make_state(issuer.clone(), pool);

    let (token, _) = issuer.issue_access(Uuid::now_v7())?;
    let req = Request::builder()
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header("x-group-id", HeaderValue::from_static("not-a-uuid"))
        .body(())?;
    let err = extract_principal(&state, req).await.unwrap_err();
    assert_eq!(err.0, StatusCode::BAD_REQUEST);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn non_member_group_returns_403() -> anyhow::Result<()> {
    let (_c, _admin, pool) = boot_login_pool().await?;
    let issuer = Arc::new(JwtIssuer::new(jwt_cfg())?);
    let state = make_state(issuer.clone(), pool);

    let user_id = Uuid::now_v7();
    let (token, _) = issuer.issue_access(user_id)?;
    // Use a random group_id the user was never added to.
    let group_id = Uuid::now_v7();
    let req = Request::builder()
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header("x-group-id", group_id.to_string())
        .body(())?;
    let err = extract_principal(&state, req).await.unwrap_err();
    assert_eq!(err.0, StatusCode::FORBIDDEN);
    Ok(())
}

#[test]
fn require_permission_positive_and_negative() {
    let owner = Principal {
        user_id: Uuid::now_v7(),
        group_id: Some(Uuid::now_v7()),
        role: Some(Role::Owner),
    };
    assert!(require_permission(&owner, Action::GroupDelete).is_ok());

    let child = Principal {
        user_id: Uuid::now_v7(),
        group_id: Some(Uuid::now_v7()),
        role: Some(Role::Child),
    };
    let err = require_permission(&child, Action::FilesWrite).unwrap_err();
    assert_eq!(err.0, StatusCode::FORBIDDEN);

    // Positive child path too.
    assert!(require_permission(&child, Action::ChatsWrite).is_ok());
}
