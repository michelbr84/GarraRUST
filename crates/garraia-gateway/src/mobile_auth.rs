//! GAR-335: Mobile Auth — JWT-based register/login/me for Garra Cloud Alpha.
//! GAR-382 (plan 0036): Argon2id replaces PBKDF2 for new writes;
//! legacy PBKDF2 users are transparently upgraded on first successful login.
//!
//! Endpoints:
//!   POST /auth/register  — create account (email + password)
//!   POST /auth/login     — return JWT bearer token
//!   GET  /me             — return authenticated user info
//!
//! Hash format invariants:
//!   - New writes: `$argon2id$...` PHC string in `password_hash`; `salt = ""`.
//!   - Legacy reads: `password_hash` is base64 PBKDF2-SHA256(600k iter);
//!     `salt` is base64(32 bytes). Detected by "hash does not start with
//!     `$argon2id$` AND `salt` is non-empty".
//!   - After a successful legacy verify, `verify_password_and_maybe_upgrade`
//!     best-effort rewrites the row as Argon2id PHC + empty salt.

use axum::{
    Json,
    extract::{FromRequestParts, State},
    http::{StatusCode, request::Parts},
    response::IntoResponse,
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use garraia_auth::hashing::{consume_dummy_hash, hash_argon2id, verify_argon2id};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use ring::pbkdf2;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use std::sync::Arc;
use tracing::warn;
use uuid::Uuid;

use crate::state::{AppState, AuthConfigMissing};

const PBKDF2_ITERATIONS: u32 = 600_000;
/// JWT expiry: 30 days in seconds.
const JWT_EXPIRY_SECS: u64 = 30 * 24 * 3600;

// ── Request / Response types ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub user_id: String,
    pub email: String,
}

#[derive(Debug, Serialize)]
pub struct MeResponse {
    pub user_id: String,
    pub email: String,
    pub created_at: String,
}

// ── JWT Claims ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MobileClaims {
    pub sub: String, // user UUID
    pub email: String,
    pub exp: u64, // unix timestamp
    pub iat: u64,
}

// ── Axum extractor: authenticated mobile user ────────────────────────────────

/// Extracts and validates the mobile JWT from the `Authorization: Bearer <token>` header.
pub struct MobileAuth(pub MobileClaims);

impl FromRequestParts<Arc<AppState>> for MobileAuth {
    type Rejection = (StatusCode, Json<serde_json::Value>);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if !auth_header.starts_with("Bearer ") {
            return Err(unauthorized("missing bearer token"));
        }

        let token = &auth_header["Bearer ".len()..];
        // Plan 0046 slice 3: fail-closed. No hardcoded fallback — if
        // `AuthConfig` is not wired (fail-soft dev mode), the extractor
        // returns 503 rather than accepting tokens signed with an
        // insecure default.
        let secret = match state.jwt_signing_secret() {
            Ok(s) => s,
            Err(_) => {
                warn!("mobile_auth extractor: AuthConfig unavailable; returning 503");
                return Err(auth_unconfigured());
            }
        };
        let key = DecodingKey::from_secret(secret.expose_secret().as_bytes());
        // SEC-H-1 (plan 0036 audit): pin the algorithm list explicitly so the
        // guard against algorithm-confusion (e.g. `alg: none`) is closed
        // regardless of future jsonwebtoken defaults. The fluxo Postgres em
        // `garraia-auth/jwt.rs` já faz isso; aqui espelhamos.
        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = true;

        match decode::<MobileClaims>(token, &key, &validation) {
            Ok(data) => Ok(MobileAuth(data.claims)),
            Err(e) => {
                warn!("mobile JWT validation failed: {e}");
                Err(unauthorized("invalid or expired token"))
            }
        }
    }
}

fn unauthorized(msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({"error": msg})),
    )
}

/// Plan 0046 slice 3: `503 Service Unavailable` used when the gateway
/// is running in fail-soft mode (no `GARRAIA_JWT_SECRET` /
/// `GarraIA_VAULT_PASSPHRASE` set). The response body stays JSON for
/// consistency with the other mobile-auth errors.
fn auth_unconfigured() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({"error": "auth not configured"})),
    )
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// POST /auth/register
pub async fn register(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RegisterRequest>,
) -> impl IntoResponse {
    let email = req.email.trim().to_lowercase();
    if email.is_empty() || !email.contains('@') {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid email"})),
        );
    }
    if req.password.len() < 8 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "password must be at least 8 characters"})),
        );
    }

    let phc = match hash_password(&req.password) {
        Ok(v) => v,
        Err(e) => {
            warn!("register: hash_password failed: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            );
        }
    };

    let user_id = Uuid::new_v4().to_string();

    let db_result = {
        let Some(store_arc) = &state.session_store else {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "database unavailable"})),
            );
        };
        let store = store_arc.lock().await;
        // salt = "" for Argon2id: the PHC string embeds its own salt.
        store.create_mobile_user(&user_id, &email, &phc, "")
    };

    if let Err(e) = db_result {
        let msg = e.to_string();
        if msg.contains("already registered") {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({"error": "email already registered"})),
            );
        }
        warn!("register: DB error: {msg}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "internal error"})),
        );
    }

    let token = match issue_jwt(&state, &user_id, &email) {
        Ok(t) => t,
        Err(JwtIssueError::AuthUnconfigured) => {
            warn!("register: AuthConfig unavailable; returning 503");
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "auth not configured"})),
            );
        }
        Err(JwtIssueError::Jwt(e)) => {
            warn!("register: JWT issue failed: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            );
        }
    };

    (
        StatusCode::CREATED,
        Json(serde_json::json!(AuthResponse {
            token,
            user_id,
            email
        })),
    )
}

/// POST /auth/login
pub async fn login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LoginRequest>,
) -> impl IntoResponse {
    let email = req.email.trim().to_lowercase();

    let Some(store_arc) = &state.session_store else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "database unavailable"})),
        );
    };

    // Anti-enumeration: fetch user then run a constant-time verify. If the
    // user doesn't exist we still consume equivalent latency via
    // `consume_dummy_hash` so a timing oracle cannot distinguish "user not
    // found" from "wrong password".
    let user_opt = {
        let store = store_arc.lock().await;
        store.find_mobile_user_by_email(&email)
    };

    let user = match user_opt {
        Ok(Some(u)) => u,
        Ok(None) => {
            // Match the latency of a real verify so the absence of the row
            // cannot be detected by a timing side-channel.
            let secret = SecretString::from(req.password);
            if let Err(e) = consume_dummy_hash(&secret) {
                warn!("login: dummy-hash consume failed: {e}");
            }
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "invalid credentials"})),
            );
        }
        Err(e) => {
            warn!("login: DB error: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            );
        }
    };

    if !verify_password_and_maybe_upgrade(
        store_arc,
        &user.id,
        &user.password_hash,
        &user.salt,
        &req.password,
    )
    .await
    {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid credentials"})),
        );
    }

    let token = match issue_jwt(&state, &user.id, &user.email) {
        Ok(t) => t,
        Err(JwtIssueError::AuthUnconfigured) => {
            warn!("login: AuthConfig unavailable; returning 503");
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "auth not configured"})),
            );
        }
        Err(JwtIssueError::Jwt(e)) => {
            warn!("login: JWT issue failed: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            );
        }
    };

    (
        StatusCode::OK,
        Json(serde_json::json!(AuthResponse {
            token,
            user_id: user.id,
            email: user.email,
        })),
    )
}

/// GET /me
pub async fn me(
    MobileAuth(claims): MobileAuth,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let user_opt = {
        let Some(store_arc) = &state.session_store else {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "database unavailable"})),
            );
        };
        let store = store_arc.lock().await;
        store.find_mobile_user_by_id(&claims.sub)
    };

    match user_opt {
        Ok(Some(u)) => (
            StatusCode::OK,
            Json(serde_json::json!(MeResponse {
                user_id: u.id,
                email: u.email,
                created_at: u.created_at,
            })),
        ),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "user not found"})),
        ),
        Err(e) => {
            warn!("me: DB error: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Produce a fresh Argon2id PHC string for `password` (GAR-382).
///
/// The returned string is stored directly in `mobile_users.password_hash`;
/// the legacy `salt` column is expected to be `""` for Argon2id rows.
fn hash_password(password: &str) -> Result<String, String> {
    let secret = SecretString::from(password.to_owned());
    hash_argon2id(&secret).map_err(|e| e.to_string())
}

/// Verify `password` against the row stored for `user_id`. If the stored
/// hash is PBKDF2 (legacy) and verification succeeds, best-effort replaces
/// it with an Argon2id PHC string under a fresh salt.
///
/// Returns `true` iff the password matches.
async fn verify_password_and_maybe_upgrade(
    store_arc: &Arc<tokio::sync::Mutex<garraia_db::SessionStore>>,
    user_id: &str,
    stored_hash: &str,
    stored_salt: &str,
    password: &str,
) -> bool {
    if stored_hash.starts_with("$argon2id$") {
        let secret = SecretString::from(password.to_owned());
        // Code review LOW-1 (plan 0036): surface malformed-PHC errors in
        // logs instead of silently folding them into a 401 — aids diagnosing
        // DB-side hash corruption.
        return match verify_argon2id(stored_hash, &secret) {
            Ok(b) => b,
            Err(e) => {
                warn!("verify_argon2id error (uid={user_id}): {e}");
                false
            }
        };
    }

    // Legacy PBKDF2 path: dual-verify with ring, then lazy upgrade.
    if !verify_pbkdf2_legacy(password, stored_hash, stored_salt) {
        return false;
    }

    match hash_password(password) {
        Ok(new_phc) => {
            let store = store_arc.lock().await;
            match store.update_mobile_user_hash(user_id, &new_phc) {
                Ok(n) if n >= 1 => {}
                Ok(_) => {
                    // SEC-M-1 (plan 0036 audit): include the user_id (UUID, not
                    // PII) so operators can correlate orphan warnings.
                    warn!("lazy_upgrade: zero rows for uid={user_id} (best-effort; proceeding)");
                }
                Err(e) => {
                    warn!("lazy_upgrade: DB update failed: {e}");
                }
            }
        }
        Err(e) => {
            warn!("lazy_upgrade: argon2id hash failed: {e}");
        }
    }

    true
}

/// Verify a base64-PBKDF2-SHA256 password against the (hash, salt) columns
/// stored in the legacy `mobile_users` layout.
fn verify_pbkdf2_legacy(password: &str, stored_hash_b64: &str, stored_salt_b64: &str) -> bool {
    if stored_salt_b64.is_empty() {
        return false;
    }
    let Ok(salt) = BASE64.decode(stored_salt_b64) else {
        return false;
    };
    let Ok(expected_hash) = BASE64.decode(stored_hash_b64) else {
        return false;
    };
    let iterations = NonZeroU32::new(PBKDF2_ITERATIONS).expect("iterations > 0");
    pbkdf2::verify(
        pbkdf2::PBKDF2_HMAC_SHA256,
        iterations,
        &salt,
        password.as_bytes(),
        &expected_hash,
    )
    .is_ok()
}

/// Produce a base64(PBKDF2-SHA256 hash) + base64(salt) pair in the legacy
/// layout. **Only exposed to tests** so the integration suite can seed a
/// pre-GAR-382 user to exercise the lazy-upgrade path.
#[cfg(any(test, feature = "test-helpers"))]
pub fn legacy_hash_password_for_tests(password: &str) -> (String, String) {
    use ring::rand::{SecureRandom, SystemRandom};
    let rng = SystemRandom::new();
    let mut salt = vec![0u8; 32];
    rng.fill(&mut salt).expect("fill salt");
    let iterations = NonZeroU32::new(PBKDF2_ITERATIONS).expect("iterations > 0");
    let mut hash = vec![0u8; 32];
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        iterations,
        &salt,
        password.as_bytes(),
        &mut hash,
    );
    (BASE64.encode(&hash), BASE64.encode(&salt))
}

/// Plan 0046 slice 3: unified error surface for `issue_jwt` callers.
///
/// `AuthUnconfigured` is a fail-closed signal — the gateway is running
/// in dev mode without `GARRAIA_JWT_SECRET` / `GarraIA_VAULT_PASSPHRASE`.
/// Handlers map this to `503 Service Unavailable`. `Jwt` wraps the
/// upstream `jsonwebtoken` error; handlers map this to 500.
#[derive(Debug)]
pub enum JwtIssueError {
    AuthUnconfigured,
    Jwt(jsonwebtoken::errors::Error),
}

impl std::fmt::Display for JwtIssueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JwtIssueError::AuthUnconfigured => {
                f.write_str("auth not configured: GARRAIA_JWT_SECRET is absent")
            }
            JwtIssueError::Jwt(e) => write!(f, "jwt encode error: {e}"),
        }
    }
}

impl std::error::Error for JwtIssueError {}

impl From<AuthConfigMissing> for JwtIssueError {
    fn from(_: AuthConfigMissing) -> Self {
        JwtIssueError::AuthUnconfigured
    }
}

impl From<jsonwebtoken::errors::Error> for JwtIssueError {
    fn from(e: jsonwebtoken::errors::Error) -> Self {
        JwtIssueError::Jwt(e)
    }
}

/// Public re-export so that oauth.rs and totp.rs can issue tokens
/// without duplicating logic. Plan 0046 slice 3: signature updated to
/// take `&AppState` — the JWT secret is no longer read from env inside
/// this module. Handlers MUST pass the shared state.
pub fn issue_jwt_pub(
    state: &AppState,
    user_id: &str,
    email: &str,
) -> Result<String, JwtIssueError> {
    issue_jwt(state, user_id, email)
}

fn issue_jwt(state: &AppState, user_id: &str, email: &str) -> Result<String, JwtIssueError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let claims = MobileClaims {
        sub: user_id.to_string(),
        email: email.to_string(),
        iat: now,
        exp: now + JWT_EXPIRY_SECS,
    };

    // Plan 0046 slice 3: fail-closed — `AuthConfigMissing` converts to
    // `JwtIssueError::AuthUnconfigured` which the handler maps to 503.
    let secret = state.jwt_signing_secret()?;
    let encoded = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.expose_secret().as_bytes()),
    )?;
    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use garraia_agents::AgentRuntime;
    use garraia_channels::ChannelRegistry;
    use garraia_config::{AppConfig, AuthConfig};
    use garraia_db::SessionStore;

    fn test_state_without_auth() -> AppState {
        AppState::new(
            AppConfig::default(),
            AgentRuntime::new(),
            ChannelRegistry::new(),
        )
    }

    fn test_state_with_auth_secret(secret: &str) -> AppState {
        let mut state = AppState::new(
            AppConfig::default(),
            AgentRuntime::new(),
            ChannelRegistry::new(),
        );
        let cfg = AuthConfig {
            jwt_secret: SecretString::from(secret.to_string()),
            refresh_hmac_secret: SecretString::from("r".repeat(32)),
            login_database_url: SecretString::from(
                "postgres://garraia_login:pw@localhost/garraia".to_string(),
            ),
            signup_database_url: SecretString::from(
                "postgres://garraia_signup:pw@localhost/garraia".to_string(),
            ),
            app_database_url: None,
        };
        state.set_auth_config(Arc::new(cfg));
        state
    }

    #[test]
    fn issue_jwt_without_auth_config_returns_auth_unconfigured() {
        // Plan 0046 slice 3: fail-closed. Zero hardcoded fallback —
        // when AppState has no AuthConfig, issue_jwt surfaces the
        // `AuthConfigMissing` sentinel as `JwtIssueError::AuthUnconfigured`.
        let state = test_state_without_auth();
        let err = issue_jwt(&state, "u-1", "alice@example.test")
            .expect_err("expected AuthUnconfigured, got Ok");
        assert!(
            matches!(err, JwtIssueError::AuthUnconfigured),
            "expected JwtIssueError::AuthUnconfigured, got {err:?}"
        );
    }

    #[test]
    fn issue_jwt_with_auth_config_returns_valid_hs256_token() {
        let state = test_state_with_auth_secret(&"S".repeat(32));
        let token = issue_jwt(&state, "u-2", "bob@example.test").expect("issue");
        // HS256 JWTs have exactly 2 dots.
        assert_eq!(token.matches('.').count(), 2, "token = {token}");

        // Round-trip: decode with the same secret.
        let key = DecodingKey::from_secret(&b"S".repeat(32));
        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = true;
        let data = decode::<MobileClaims>(&token, &key, &validation).expect("decode");
        assert_eq!(data.claims.sub, "u-2");
        assert_eq!(data.claims.email, "bob@example.test");
    }

    #[test]
    fn hash_password_produces_argon2id_phc() {
        let phc = hash_password("correct-horse-battery-staple").expect("hash");
        assert!(
            phc.starts_with("$argon2id$"),
            "expected argon2id PHC, got {phc}"
        );
        assert!(phc.contains("m=65536,t=3,p=4"));
    }

    #[test]
    fn verify_pbkdf2_legacy_roundtrip() {
        let password = "legacy-password-1234";
        let (hash_b64, salt_b64) = legacy_hash_password_for_tests(password);
        assert!(verify_pbkdf2_legacy(password, &hash_b64, &salt_b64));
        assert!(!verify_pbkdf2_legacy("wrong", &hash_b64, &salt_b64));
    }

    #[test]
    fn verify_pbkdf2_legacy_rejects_empty_salt() {
        assert!(!verify_pbkdf2_legacy("anything", "deadbeef", ""));
    }

    fn fresh_store() -> Arc<tokio::sync::Mutex<SessionStore>> {
        Arc::new(tokio::sync::Mutex::new(
            SessionStore::in_memory().expect("in-memory store"),
        ))
    }

    async fn seed_mobile_user(
        store: &Arc<tokio::sync::Mutex<SessionStore>>,
        user_id: &str,
        email: &str,
        password_hash: &str,
        salt: &str,
    ) {
        let guard = store.lock().await;
        guard
            .create_mobile_user(user_id, email, password_hash, salt)
            .expect("seed mobile user");
    }

    async fn read_stored_row(
        store: &Arc<tokio::sync::Mutex<SessionStore>>,
        email: &str,
    ) -> (String, String) {
        let guard = store.lock().await;
        let user = guard
            .find_mobile_user_by_email(email)
            .expect("find user")
            .expect("user exists");
        (user.password_hash, user.salt)
    }

    #[tokio::test]
    async fn legacy_pbkdf2_login_succeeds_and_triggers_lazy_upgrade() {
        let store = fresh_store();
        let user_id = "u-legacy";
        let password = "legacy-password-1234";
        let (hash_b64, salt_b64) = legacy_hash_password_for_tests(password);
        seed_mobile_user(&store, user_id, "legacy@example.test", &hash_b64, &salt_b64).await;

        // Pre-login state: PBKDF2 base64 hash, non-empty salt.
        let (pre_hash, pre_salt) = read_stored_row(&store, "legacy@example.test").await;
        assert_eq!(pre_hash, hash_b64);
        assert_eq!(pre_salt, salt_b64);
        assert!(!pre_hash.starts_with("$argon2id$"));

        let ok = verify_password_and_maybe_upgrade(&store, user_id, &pre_hash, &pre_salt, password)
            .await;
        assert!(
            ok,
            "legacy PBKDF2 verify should succeed for correct password"
        );

        // Post-login state: Argon2id PHC, empty salt (lazy upgrade applied).
        let (post_hash, post_salt) = read_stored_row(&store, "legacy@example.test").await;
        assert!(
            post_hash.starts_with("$argon2id$"),
            "expected argon2id PHC after upgrade, got {post_hash}"
        );
        assert_eq!(post_salt, "");
    }

    #[tokio::test]
    async fn wrong_password_on_legacy_user_does_not_upgrade() {
        let store = fresh_store();
        let user_id = "u-legacy-2";
        let (hash_b64, salt_b64) = legacy_hash_password_for_tests("right-password-abcd");
        seed_mobile_user(
            &store,
            user_id,
            "legacy2@example.test",
            &hash_b64,
            &salt_b64,
        )
        .await;

        let ok = verify_password_and_maybe_upgrade(
            &store,
            user_id,
            &hash_b64,
            &salt_b64,
            "wrong-password",
        )
        .await;
        assert!(!ok);

        let (post_hash, post_salt) = read_stored_row(&store, "legacy2@example.test").await;
        assert_eq!(
            post_hash, hash_b64,
            "hash should not be upgraded on failure"
        );
        assert_eq!(post_salt, salt_b64);
    }

    #[tokio::test]
    async fn argon2id_register_and_login_roundtrip() {
        let store = fresh_store();
        let user_id = "u-argon";
        let password = "argon-password-42";
        let phc = hash_password(password).expect("hash");
        seed_mobile_user(&store, user_id, "argon@example.test", &phc, "").await;

        let ok = verify_password_and_maybe_upgrade(&store, user_id, &phc, "", password).await;
        assert!(ok, "argon2id verify should succeed");

        // Hash should remain the same (no upgrade path for already-argon2id).
        let (post_hash, post_salt) = read_stored_row(&store, "argon@example.test").await;
        assert_eq!(post_hash, phc);
        assert_eq!(post_salt, "");

        // Wrong password → false.
        let bad = verify_password_and_maybe_upgrade(&store, user_id, &phc, "", "nope").await;
        assert!(!bad);
    }

    // ── Plan 0046 slice 3: handler-level integration tests ────────────────

    use axum::Router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::routing::post;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn router_with_state(state: Arc<AppState>) -> Router {
        Router::new()
            .route("/auth/register", post(register))
            .route("/auth/login", post(login))
            .with_state(state)
    }

    fn seed_state_with_store(state: &mut AppState) -> Arc<tokio::sync::Mutex<SessionStore>> {
        let store = Arc::new(tokio::sync::Mutex::new(
            SessionStore::in_memory().expect("in-memory store"),
        ));
        state.set_session_store(store.clone());
        store
    }

    async fn post_json(
        router: &Router,
        path: &str,
        body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let req = Request::builder()
            .uri(path)
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let resp = router.clone().oneshot(req).await.expect("oneshot");
        let status = resp.status();
        let body = resp.into_body().collect().await.expect("body").to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).expect("json body");
        (status, v)
    }

    #[tokio::test]
    async fn login_without_jwt_secret_returns_503() {
        // Plan 0046 §7.2: the gateway refuses to sign tokens with a hardcoded
        // fallback. When AppState has no AuthConfig, /auth/register (which
        // reaches issue_jwt) fails closed with 503 — never 200 with an insecure JWT.
        let mut state = test_state_without_auth();
        seed_state_with_store(&mut state);
        let state = Arc::new(state);
        let router = router_with_state(state).await;

        let (status, body) = post_json(
            &router,
            "/auth/register",
            serde_json::json!({"email": "a@b.test", "password": "password-1234"}),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "body = {body}");
        assert_eq!(body["error"], "auth not configured");
    }

    #[tokio::test]
    async fn login_with_standard_env_works() {
        // Plan 0046 §7.2: baseline — when AuthConfig carries a valid
        // jwt_secret, /auth/register succeeds and issues a real HS256 JWT.
        let mut state = test_state_with_auth_secret(&"S".repeat(32));
        seed_state_with_store(&mut state);
        let state = Arc::new(state);
        let router = router_with_state(state).await;

        let (status, body) = post_json(
            &router,
            "/auth/register",
            serde_json::json!({"email": "alice@b.test", "password": "password-1234"}),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "body = {body}");
        let token = body["token"].as_str().expect("token string");
        assert_eq!(
            token.matches('.').count(),
            2,
            "HS256 JWT must have exactly 2 dots"
        );
    }

    #[tokio::test]
    async fn login_with_legacy_passphrase_works() {
        // Plan 0046 §7.2: GarraIA_VAULT_PASSPHRASE fallback must still
        // issue valid tokens when wired into AuthConfig. The env-var
        // fallback is covered in garraia-config::auth::tests; here we
        // confirm the handler path accepts any AuthConfig regardless of
        // which env var fed it.
        let mut state = test_state_with_auth_secret(&"V".repeat(32));
        seed_state_with_store(&mut state);
        let state = Arc::new(state);
        let router = router_with_state(state).await;

        let (status, body) = post_json(
            &router,
            "/auth/register",
            serde_json::json!({"email": "legacy@b.test", "password": "password-1234"}),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "body = {body}");
        let token = body["token"].as_str().expect("token string");
        // Decode with the same secret to confirm signing key.
        let key = DecodingKey::from_secret(&b"V".repeat(32));
        let mut v = Validation::new(Algorithm::HS256);
        v.validate_exp = true;
        decode::<MobileClaims>(token, &key, &v).expect("must decode with legacy secret");
    }

    #[tokio::test]
    async fn second_login_after_upgrade_still_works() {
        let store = fresh_store();
        let user_id = "u-sequential";
        let password = "seq-password-xyz";
        let (hash_b64, salt_b64) = legacy_hash_password_for_tests(password);
        seed_mobile_user(&store, user_id, "seq@example.test", &hash_b64, &salt_b64).await;

        // First login: upgrades to Argon2id.
        let first =
            verify_password_and_maybe_upgrade(&store, user_id, &hash_b64, &salt_b64, password)
                .await;
        assert!(first);

        let (second_hash, second_salt) = read_stored_row(&store, "seq@example.test").await;
        assert!(second_hash.starts_with("$argon2id$"));

        // Second login: runs via Argon2id path (same password).
        let second = verify_password_and_maybe_upgrade(
            &store,
            user_id,
            &second_hash,
            &second_salt,
            password,
        )
        .await;
        assert!(second);
    }
}
