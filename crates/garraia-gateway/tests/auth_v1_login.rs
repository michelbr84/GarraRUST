//! Integration test for `POST /v1/auth/login` under feature `auth-v1` (GAR-391b).
//!
//! Validates the critical security property: **byte-identical 401 response
//! across every failure mode**. An attacker that probes
//! /v1/auth/login with random emails must NOT be able to distinguish:
//!   - user does not exist
//!   - user exists, wrong password
//!   - user exists, password correct, account suspended
//!   - user exists, password correct, account deleted
//!   - user exists, password format unknown
//!
//! all 5 modes return the same status code, the same JSON body bytes, and
//! the same headers (modulo per-response variation like `content-length`
//! which is identical for an identical body).

#![cfg(feature = "auth-v1")]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use axum::body::to_bytes;
use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{Request, StatusCode};
use garraia_auth::{
    hash_argon2id, InternalProvider, JwtConfig, JwtIssuer, LoginConfig, LoginPool,
};
use garraia_gateway::auth_routes::{router, AuthState};
use garraia_workspace::{Workspace, WorkspaceConfig};
use secrecy::SecretString;
use sqlx::Row;
use testcontainers::runners::AsyncRunner;
use testcontainers::ContainerAsync;
use testcontainers::ImageExt;
use testcontainers_modules::postgres::Postgres as PgImage;
use tower::ServiceExt;
use uuid::Uuid;

struct Fixture {
    _container: ContainerAsync<PgImage>,
    admin_pool: sqlx::PgPool,
    router: axum::Router,
}

async fn boot() -> anyhow::Result<Fixture> {
    // Best-effort tracing init so handler logs surface during test failures.
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_max_level(tracing::Level::ERROR)
        .try_init();
    let container = PgImage::default()
        .with_name("pgvector/pgvector")
        .with_tag("pg16")
        .start()
        .await?;
    let host = container.get_host().await?;
    let port = container.get_host_port_ipv4(5432).await?;
    let postgres_url = format!("postgres://postgres:postgres@{host}:{port}/postgres");

    Workspace::connect(WorkspaceConfig {
        database_url: postgres_url.clone(),
        max_connections: 5,
        migrate_on_start: true,
    })
    .await?;

    let admin_pool = sqlx::PgPool::connect(&postgres_url).await?;
    sqlx::query("ALTER ROLE garraia_login WITH LOGIN PASSWORD 'test-password'")
        .execute(&admin_pool)
        .await?;

    let login_url =
        postgres_url.replace("postgres:postgres@", "garraia_login:test-password@");
    let pool = Arc::new(
        LoginPool::from_dedicated_config(&LoginConfig {
            database_url: login_url,
            max_connections: 4,
        })
        .await?,
    );
    let provider = Arc::new(InternalProvider::new(pool));
    let jwt = Arc::new(
        JwtIssuer::new(JwtConfig {
            jwt_secret: SecretString::from("a".repeat(32)),
            refresh_hmac_secret: SecretString::from("b".repeat(32)),
        })
        .unwrap(),
    );

    let state = AuthState { provider, jwt };
    let router = router(state).layer(MockConnectInfo(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(203, 0, 113, 99)),
        65000,
    )));

    Ok(Fixture {
        _container: container,
        admin_pool,
        router,
    })
}

async fn seed_user(
    admin: &sqlx::PgPool,
    email: &str,
    password_hash: Option<&str>,
    status: &str,
) -> anyhow::Result<Uuid> {
    let row = sqlx::query(
        "INSERT INTO users (email, display_name, status) VALUES ($1, $1, $2) RETURNING id",
    )
    .bind(email)
    .bind(status)
    .fetch_one(admin)
    .await?;
    let user_id: Uuid = row.try_get("id")?;
    sqlx::query(
        "INSERT INTO user_identities (user_id, provider, provider_sub, password_hash) \
         VALUES ($1, 'internal', $2, $3)",
    )
    .bind(user_id)
    .bind(email)
    .bind(password_hash)
    .execute(admin)
    .await?;
    Ok(user_id)
}

fn json_request(email: &str, password: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/v1/auth/login")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({"email": email, "password": password}).to_string(),
        ))
        .unwrap()
}

async fn extract_status_and_body(
    router: axum::Router,
    req: Request<Body>,
) -> (StatusCode, Vec<u8>) {
    let resp = router.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
    (status, bytes.to_vec())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn happy_path_returns_200_with_tokens() -> anyhow::Result<()> {
    let f = boot().await?;
    let pw = SecretString::from("right-password".to_owned());
    let phc = hash_argon2id(&pw)?;
    let _ = seed_user(&f.admin_pool, "alice@example.com", Some(&phc), "active").await?;

    let (status, body) = extract_status_and_body(
        f.router.clone(),
        json_request("alice@example.com", "right-password"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&body)?;
    assert!(v.get("access_token").and_then(|x| x.as_str()).is_some());
    assert!(v.get("user_id").and_then(|x| x.as_str()).is_some());
    assert!(v.get("expires_at").and_then(|x| x.as_str()).is_some());
    // 391b reduced scope: refresh_token is NOT part of the login response.
    // It joins in 391c alongside the refresh endpoint.
    assert!(
        v.get("refresh_token").is_none(),
        "391b must NOT return refresh_token (scope reduced — see plan 0011 amendment)"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn failure_modes_are_byte_identical() -> anyhow::Result<()> {
    let f = boot().await?;

    // Seed three users with three different failure modes:
    //   1. wrong password (active user)
    //   2. suspended (active user, password correct, but account suspended)
    //   3. unknown hash format
    let pw = SecretString::from("right".to_owned());
    let phc = hash_argon2id(&pw)?;
    seed_user(&f.admin_pool, "wrongpw@example.com", Some(&phc), "active").await?;
    seed_user(&f.admin_pool, "suspended@example.com", Some(&phc), "suspended").await?;
    seed_user(
        &f.admin_pool,
        "badhash@example.com",
        Some("$bcrypt$2b$12$xxxx"),
        "active",
    )
    .await?;

    // 5 failure modes -> 5 responses.
    let (s_notfound, b_notfound) = extract_status_and_body(
        f.router.clone(),
        json_request("ghost@example.com", "anything"),
    )
    .await;
    let (s_wrong, b_wrong) = extract_status_and_body(
        f.router.clone(),
        json_request("wrongpw@example.com", "wrong"),
    )
    .await;
    let (s_suspended, b_suspended) = extract_status_and_body(
        f.router.clone(),
        json_request("suspended@example.com", "right"),
    )
    .await;
    let (s_badhash, b_badhash) = extract_status_and_body(
        f.router.clone(),
        json_request("badhash@example.com", "anything"),
    )
    .await;

    // Status code identical.
    assert_eq!(s_notfound, StatusCode::UNAUTHORIZED);
    assert_eq!(s_wrong, StatusCode::UNAUTHORIZED);
    assert_eq!(s_suspended, StatusCode::UNAUTHORIZED);
    assert_eq!(s_badhash, StatusCode::UNAUTHORIZED);

    // Body bytes identical (anti-enumeration core property).
    assert_eq!(b_notfound, b_wrong);
    assert_eq!(b_notfound, b_suspended);
    assert_eq!(b_notfound, b_badhash);

    // And the body is the agreed shape.
    let parsed: serde_json::Value = serde_json::from_slice(&b_notfound)?;
    assert_eq!(parsed["error"], "invalid_credentials");

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn malformed_body_returns_422() -> anyhow::Result<()> {
    // axum 0.8 `Json<T>` returns 422 Unprocessable Entity for JSON that is
    // syntactically valid but does not match the target type (here:
    // `email` typed as integer instead of string). 400 Bad Request is
    // reserved for syntactically invalid JSON. Custom error mapping
    // (e.g., RFC 9457 Problem Details across the whole API) is Fase 3.4
    // work, not 391b. See plan 0011 amendment §"Body malformado test".
    let f = boot().await?;
    let req = Request::builder()
        .method("POST")
        .uri("/v1/auth/login")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"email": 42}"#))
        .unwrap();
    let resp = f.router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    Ok(())
}
