//! TOTP (2FA) do painel admin — #1121.
//!
//! O `crate::totp` ja implementava RFC 6238, mas so era alcançavel pelo fluxo
//! mobile (`/auth/2fa/*`): o login do painel (`POST /admin/api/login`) parava
//! na senha e nunca perguntava o segundo fator. Estes handlers trazem o mesmo
//! segredo e a mesma verificacao para a porta admin.
//!
//! Rotas (todas dentro do router autenticado, atras de `require_admin_auth` +
//! `require_csrf`, em `routes.rs`):
//!   GET  /admin/api/2fa/status  — este usuario tem 2FA ligado?
//!   POST /admin/api/2fa/setup   — gera e guarda um segredo **pendente**
//!   POST /admin/api/2fa/verify  — valida um codigo e liga o 2FA
//!   POST /admin/api/2fa/disable — valida um codigo e desliga
//!
//! O *consumo* do segundo fator nao mora aqui: esta no `handlers::login`,
//! porque e la que a sessao e criada. Um segredo pendente nao vale nada —
//! so passa a ser exigido depois que `enable_totp` roda.
//!
//! Como no fluxo mobile, o segredo fica em claro no `admin.db` (base32). Ver
//! o aviso em `store::AdminStore::get_totp_secret` e no modulo `crate::totp`.

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use serde::Deserialize;

use super::handlers::AdminState;
use super::middleware::{AuthenticatedAdmin, extract_ip};

#[derive(Debug, Deserialize)]
pub struct TotpCodeRequest {
    /// Codigo de 6 digitos do app autenticador.
    pub code: String,
}

fn unauthorized(error: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({"error": error})),
    )
}

/// GET /admin/api/2fa/status
pub async fn totp_status(
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    State(state): State<AdminState>,
) -> impl IntoResponse {
    let guard = state.store.lock().await;
    let enabled = guard.is_totp_enabled(&admin.user_id);
    drop(guard);

    Json(serde_json::json!({ "enabled": enabled }))
}

/// POST /admin/api/2fa/setup
///
/// Devolve o segredo **uma vez**. Ele fica pendente: se o usuario nunca
/// chamar `/verify`, o login continua exigindo so a senha.
pub async fn totp_setup(
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    State(state): State<AdminState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let ip = extract_ip(&headers, None);
    let secret = match crate::totp::generate_totp_secret() {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("admin 2fa setup: failed to generate secret: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            );
        }
    };

    let guard = state.store.lock().await;

    // Girar o segredo exige desligar antes, com o codigo atual. So substituir
    // o segredo por baixo do 2FA ligado deixaria o dono travado fora do
    // proprio painel (o app dele aponta para o segredo antigo) sem que nada
    // pedisse confirmacao.
    if guard.is_totp_enabled(&admin.user_id) {
        drop(guard);
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "2fa already enabled",
                "hint": "disable it with a valid code before rotating the secret",
            })),
        );
    }

    if let Err(e) = guard.set_pending_totp_secret(&admin.user_id, &secret) {
        drop(guard);
        tracing::warn!("admin 2fa setup: failed to store secret: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "internal error"})),
        );
    }

    let _ = guard.append_audit(
        Some(&admin.user_id),
        Some(&admin.username),
        "2fa.setup",
        "auth",
        None,
        None,
        ip.as_deref(),
        "success",
    );
    drop(guard);

    let qr_uri = crate::totp::generate_totp_qr(&secret, &admin.username);

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "secret": secret,
            "qr_uri": qr_uri,
            "issuer": "GarraIA",
            "enabled": false,
        })),
    )
}

/// POST /admin/api/2fa/verify
///
/// Primeiro codigo valido liga o segundo fator. A partir daqui, `login`
/// passa a exigir o codigo.
pub async fn totp_verify(
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(req): Json<TotpCodeRequest>,
) -> impl IntoResponse {
    let ip = extract_ip(&headers, None);
    let mut guard = state.store.lock().await;

    let secret = match guard.get_totp_secret(&admin.user_id) {
        Some(s) => s,
        None => {
            drop(guard);
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "2fa not set up"})),
            );
        }
    };

    if guard.totp_attempts_exhausted(&admin.user_id) {
        let _ = guard.append_audit(
            Some(&admin.user_id),
            Some(&admin.username),
            "2fa.verify",
            "auth",
            None,
            Some("too many attempts"),
            ip.as_deref(),
            "failure",
        );
        drop(guard);
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({"error": "too many attempts"})),
        );
    }

    let ok = crate::totp::verify_totp(&secret, &req.code);
    guard.record_totp_attempt(&admin.user_id, ok);

    if !ok {
        let _ = guard.append_audit(
            Some(&admin.user_id),
            Some(&admin.username),
            "2fa.verify",
            "auth",
            None,
            Some("invalid code"),
            ip.as_deref(),
            "failure",
        );
        drop(guard);
        return unauthorized("invalid code");
    }

    if let Err(e) = guard.enable_totp(&admin.user_id) {
        drop(guard);
        tracing::warn!("admin 2fa verify: failed to enable: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "internal error"})),
        );
    }

    let _ = guard.append_audit(
        Some(&admin.user_id),
        Some(&admin.username),
        "2fa.verify",
        "auth",
        None,
        None,
        ip.as_deref(),
        "success",
    );
    drop(guard);

    (
        StatusCode::OK,
        Json(serde_json::json!({"status": "2fa enabled", "enabled": true})),
    )
}

/// POST /admin/api/2fa/disable
///
/// Exige o codigo atual: desligar o segundo fator e exatamente a operacao que
/// um invasor com a senha tentaria, entao nao pode ser uma simples confirmacao
/// de sessao.
pub async fn totp_disable(
    axum::Extension(admin): axum::Extension<AuthenticatedAdmin>,
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(req): Json<TotpCodeRequest>,
) -> impl IntoResponse {
    let ip = extract_ip(&headers, None);
    let mut guard = state.store.lock().await;

    let secret = match guard.get_totp_secret(&admin.user_id) {
        Some(s) => s,
        None => {
            drop(guard);
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "2fa not enabled"})),
            );
        }
    };

    if guard.totp_attempts_exhausted(&admin.user_id) {
        drop(guard);
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({"error": "too many attempts"})),
        );
    }

    let ok = crate::totp::verify_totp(&secret, &req.code);
    guard.record_totp_attempt(&admin.user_id, ok);

    if !ok {
        let _ = guard.append_audit(
            Some(&admin.user_id),
            Some(&admin.username),
            "2fa.disable",
            "auth",
            None,
            Some("invalid code"),
            ip.as_deref(),
            "failure",
        );
        drop(guard);
        return unauthorized("invalid code");
    }

    if let Err(e) = guard.disable_totp(&admin.user_id) {
        drop(guard);
        tracing::warn!("admin 2fa disable: db error: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "internal error"})),
        );
    }

    let _ = guard.append_audit(
        Some(&admin.user_id),
        Some(&admin.username),
        "2fa.disable",
        "auth",
        None,
        None,
        ip.as_deref(),
        "success",
    );
    drop(guard);

    (
        StatusCode::OK,
        Json(serde_json::json!({"status": "2fa disabled", "enabled": false})),
    )
}
