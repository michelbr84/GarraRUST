//! TOTP (2FA) do painel admin — #1121.
//!
//! O `crate::totp` ja implementava RFC 6238, mas so era alcancavel pelo fluxo
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
//! Politica de auditoria do 2FA: o `audit_log` registra operacoes que mudam
//! estado (setup pendente, enable, disable — sempre na mesma transacao, via
//! metodos `*_audited` da store) e recusas de autenticacao (login, verify,
//! disable e as saidas antecipadas de setup, best-effort via
//! `audit::log_auth_failure`). Consulta de estado e leitura, nao decisao:
//! `GET /2fa/status` nao gera evento, como qualquer outra rota de leitura
//! do painel.
//!
//! O segredo fica **em claro** no `admin.db` (base32, sem cifrar). Nao e
//! paridade com o fluxo mobile, que cifra (`totp_secret_enc`); a justificativa
//! real e o proprio `admin.db` ja guardar token de sessao em texto puro. Ver
//! o aviso completo em `store::AdminStore::get_totp_secret`.

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use serde::Deserialize;

use super::audit::log_auth_failure;
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
    // Estado ilegivel nao vira "2FA desligado": a resposta mentiria sobre
    // seguranca. Sem leitura, sem resposta util — 500 (#1121).
    let enabled = match guard.is_totp_enabled(&admin.user_id) {
        Ok(v) => v,
        Err(e) => {
            drop(guard);
            tracing::warn!("admin 2fa status: state unreadable: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            );
        }
    };
    drop(guard);

    (
        StatusCode::OK,
        Json(serde_json::json!({ "enabled": enabled })),
    )
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
    let guard = state.store.lock().await;

    // A geracao vem depois do lock para que a recusa tambem deixe trilha:
    // tentativa de setup e operacao de admin mesmo quando o RNG falha (#1121).
    let secret = match crate::totp::generate_totp_secret() {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("admin 2fa setup: failed to generate secret: {e}");
            log_auth_failure(
                &guard,
                Some(&admin.user_id),
                Some(&admin.username),
                "2fa.setup",
                "secret generation failed",
                ip.as_deref(),
            );
            drop(guard);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            );
        }
    };

    // Girar o segredo exige desligar antes, com o codigo atual. So substituir
    // o segredo por baixo do 2FA ligado deixaria o dono travado fora do
    // proprio painel (o app dele aponta para o segredo antigo) sem que nada
    // pedisse confirmacao.
    let ja_ligado = match guard.is_totp_enabled(&admin.user_id) {
        Ok(v) => v,
        Err(e) => {
            // Recusar e o caminho fechado: com o estado ilegivel, atender
            // seria decidir autenticacao no escuro (#1121).
            tracing::warn!("admin 2fa setup: state unreadable: {e}");
            log_auth_failure(
                &guard,
                Some(&admin.user_id),
                Some(&admin.username),
                "2fa.setup",
                "totp state unreadable",
                ip.as_deref(),
            );
            drop(guard);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            );
        }
    };
    if ja_ligado {
        // Pedido legitimo de quem esqueceu que ja tem 2FA, mas tambem o jeito
        // barato de um invasor com a sessao trocar o app do dono: vai para o
        // audit como falha, como qualquer outra recusa daqui.
        log_auth_failure(
            &guard,
            Some(&admin.user_id),
            Some(&admin.username),
            "2fa.setup",
            "already enabled",
            ip.as_deref(),
        );
        drop(guard);
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "2fa already enabled",
                "hint": "disable it with a valid code before rotating the secret",
            })),
        );
    }

    // Segredo pendente + evento de auditoria na mesma transacao SQLite:
    // sem o evento gravado, a mudanca nao confirma (#1121).
    if let Err(e) = guard.set_pending_totp_secret_audited(
        &admin.user_id,
        &secret,
        &admin.username,
        ip.as_deref(),
    ) {
        drop(guard);
        tracing::warn!("admin 2fa setup: failed to store secret: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "internal error"})),
        );
    }
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
        Ok(Some(s)) if !s.is_empty() => s,
        Ok(None) => {
            // Recusa tambem e evento de auditoria — operacao fora de ordem
            // sem trilha e abuso invisivel (#1121).
            log_auth_failure(
                &guard,
                Some(&admin.user_id),
                Some(&admin.username),
                "2fa.verify",
                "no pending secret",
                ip.as_deref(),
            );
            drop(guard);
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "2fa not set up"})),
            );
        }
        Ok(_) => {
            // Segredo vazio e estado inconsistente, nao "codigo invalido":
            // avaliar o codigo contra um segredo que nao existe polui o
            // lockout e mente o motivo da recusa (#1121, pass-4).
            tracing::warn!("admin 2fa verify: secret empty");
            log_auth_failure(
                &guard,
                Some(&admin.user_id),
                Some(&admin.username),
                "2fa.verify",
                "totp secret empty",
                ip.as_deref(),
            );
            drop(guard);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            );
        }
        Err(e) => {
            // Erro de leitura nao e "2FA nao configurado": e estado que nao
            // pode ser lido — responder "nao configurado" esconderia
            // indisponibilidade e gravaria trilha errada (#1121, pass-3).
            tracing::warn!("admin 2fa verify: secret unreadable: {e}");
            log_auth_failure(
                &guard,
                Some(&admin.user_id),
                Some(&admin.username),
                "2fa.verify",
                "totp secret unreadable",
                ip.as_deref(),
            );
            drop(guard);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            );
        }
    };

    if guard.totp_attempts_exhausted(&admin.user_id) {
        log_auth_failure(
            &guard,
            Some(&admin.user_id),
            Some(&admin.username),
            "2fa.verify",
            "too many attempts",
            ip.as_deref(),
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
        log_auth_failure(
            &guard,
            Some(&admin.user_id),
            Some(&admin.username),
            "2fa.verify",
            "invalid code",
            ip.as_deref(),
        );
        drop(guard);
        return unauthorized("invalid code");
    }

    // Ligar o 2FA + gravar o evento na mesma transacao: o commit so roda
    // com os dois, entao nenhuma confirmacao de verify acontece sem
    // trilha de auditoria (#1121).
    if let Err(e) = guard.enable_totp_audited(&admin.user_id, &admin.username, ip.as_deref()) {
        drop(guard);
        tracing::warn!("admin 2fa verify: failed to enable: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "internal error"})),
        );
    }
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
        Ok(Some(s)) if !s.is_empty() => s,
        Ok(None) => {
            // Recusa tambem e evento de auditoria — mesma regra do verify
            // (#1121).
            log_auth_failure(
                &guard,
                Some(&admin.user_id),
                Some(&admin.username),
                "2fa.disable",
                "not enabled",
                ip.as_deref(),
            );
            drop(guard);
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "2fa not enabled"})),
            );
        }
        Ok(_) => {
            // Mesma regra do verify: segredo vazio e estado inconsistente.
            // Chamar verify_totp com o segredo vazio deixaria o 2FA ligado
            // irrecuperavel pelo endpoint, respondendo "codigo invalido"
            // (#1121, pass-4).
            tracing::warn!("admin 2fa disable: secret empty");
            log_auth_failure(
                &guard,
                Some(&admin.user_id),
                Some(&admin.username),
                "2fa.disable",
                "totp secret empty",
                ip.as_deref(),
            );
            drop(guard);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            );
        }
        Err(e) => {
            // Mesma regra do verify: erro de leitura e 500, nao
            // "2FA nao ligado" (#1121, pass-3).
            tracing::warn!("admin 2fa disable: secret unreadable: {e}");
            log_auth_failure(
                &guard,
                Some(&admin.user_id),
                Some(&admin.username),
                "2fa.disable",
                "totp secret unreadable",
                ip.as_deref(),
            );
            drop(guard);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            );
        }
    };

    if guard.totp_attempts_exhausted(&admin.user_id) {
        log_auth_failure(
            &guard,
            Some(&admin.user_id),
            Some(&admin.username),
            "2fa.disable",
            "too many attempts",
            ip.as_deref(),
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
        log_auth_failure(
            &guard,
            Some(&admin.user_id),
            Some(&admin.username),
            "2fa.disable",
            "invalid code",
            ip.as_deref(),
        );
        drop(guard);
        return unauthorized("invalid code");
    }

    // Desligar + gravar o evento na mesma transacao — desligar o 2FA e
    // exatamente o que um invasor com a senha tentaria; sem trilha
    // gravada, a operacao nao confirma (#1121).
    if let Err(e) = guard.disable_totp_audited(&admin.user_id, &admin.username, ip.as_deref()) {
        drop(guard);
        tracing::warn!("admin 2fa disable: db error: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "internal error"})),
        );
    }
    drop(guard);

    (
        StatusCode::OK,
        Json(serde_json::json!({"status": "2fa disabled", "enabled": false})),
    )
}
