//! GAR-335: Mobile Auth — JWT-based register/login/me for Garra Cloud Alpha.
//!
//! Endpoints:
//!   POST /auth/register  — create account (email + password)
//!   POST /auth/login     — return JWT bearer token
//!   GET  /me             — return authenticated user info

use axum::{
    Json,
    extract::{FromRequestParts, State},
    http::{StatusCode, request::Parts},
    response::IntoResponse,
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use ring::pbkdf2;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use std::sync::Arc;
use tracing::warn;
use uuid::Uuid;

use crate::state::AppState;

const PBKDF2_ITERATIONS: u32 = 600_000;
const SALT_LEN: usize = 32;
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
        _state: &Arc<AppState>,
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
        let secret = jwt_secret();
        let key = DecodingKey::from_secret(secret.as_bytes());
        let mut validation = Validation::default();
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

    let (hash, salt) = match hash_password(&req.password) {
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
        store.create_mobile_user(&user_id, &email, &hash, &salt)
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

    let token = match issue_jwt(&user_id, &email) {
        Ok(t) => t,
        Err(e) => {
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

    let user_opt = {
        let Some(store_arc) = &state.session_store else {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "database unavailable"})),
            );
        };
        let store = store_arc.lock().await;
        store.find_mobile_user_by_email(&email)
    };

    let user = match user_opt {
        Ok(Some(u)) => u,
        Ok(None) => {
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

    if !verify_password(&req.password, &user.password_hash, &user.salt) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid credentials"})),
        );
    }

    let token = match issue_jwt(&user.id, &user.email) {
        Ok(t) => t,
        Err(e) => {
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

fn hash_password(password: &str) -> Result<(String, String), String> {
    let rng = SystemRandom::new();
    let mut salt = vec![0u8; SALT_LEN];
    rng.fill(&mut salt)
        .map_err(|_| "failed to generate salt".to_string())?;

    let iterations = NonZeroU32::new(PBKDF2_ITERATIONS).expect("iterations > 0");
    let mut hash = vec![0u8; 32];
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        iterations,
        &salt,
        password.as_bytes(),
        &mut hash,
    );

    Ok((BASE64.encode(&hash), BASE64.encode(&salt)))
}

fn verify_password(password: &str, stored_hash: &str, stored_salt: &str) -> bool {
    let Ok(salt) = BASE64.decode(stored_salt) else {
        return false;
    };
    let Ok(expected_hash) = BASE64.decode(stored_hash) else {
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

fn jwt_secret() -> String {
    std::env::var("GARRAIA_JWT_SECRET").unwrap_or_else(|_| {
        std::env::var("GarraIA_VAULT_PASSPHRASE")
            .unwrap_or_else(|_| "garraia-insecure-default-jwt-secret-change-me".to_string())
    })
}

/// Public re-export so that oauth.rs and totp.rs can issue tokens without duplicating logic.
pub fn issue_jwt_pub(user_id: &str, email: &str) -> Result<String, jsonwebtoken::errors::Error> {
    issue_jwt(user_id, email)
}

fn issue_jwt(user_id: &str, email: &str) -> Result<String, jsonwebtoken::errors::Error> {
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

    let secret = jwt_secret();
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
}
