//! #1733: a resposta de `POST /api/sessions/{id}/messages` diz quem respondeu.
//!
//! O que o runtime registrou para o turno (`last_turn_stats`) e a fonte — a
//! config so diz o configurado, e com fallback os dois divergem.

use axum::Json;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use garraia_agents::turn_stats::TurnStats;
use serde::Serialize;

#[derive(Serialize)]
pub struct SendMessageResponse {
    pub session_id: String,
    pub content: String,
    /// Aditivo: ausentes quando o runtime nao registrou o turno.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// `true` quando o primario falhou e um fallback serviu.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback: Option<bool>,
}

/// Monta o `200` do turno: corpo com `provider`/`model`/`fallback` e os
/// headers `X-AI-Provider`/`X-AI-Model` por turno. O middleware do router so
/// preenche o default da config quando o handler nao os pos.
pub fn montar(session_id: String, content: String, stats: Option<TurnStats>) -> Response {
    let corpo = SendMessageResponse {
        session_id,
        content,
        provider: stats.as_ref().map(|t| t.provider.clone()),
        model: stats.as_ref().map(|t| t.model.clone()),
        fallback: stats.as_ref().map(|t| t.fallback),
    };
    let mut resposta = (StatusCode::OK, Json(corpo)).into_response();
    if let Some(t) = &stats {
        aplicar_headers_do_turno(&mut resposta, t);
    }
    resposta
}

/// Poe os headers `X-AI-Provider`/`X-AI-Model` do turno numa resposta ja
/// montada, direto das `TurnStats`.
///
/// Extraido de [`montar`] para o shim OpenAI-compat (`openai_api.rs`) reusar a
/// mesma logica em `/v1/chat/completions` — os dois caminhos poem os mesmos
/// headers por turno, e o middleware do router so preenche o default da config
/// quando nenhum handler os pos (#1733).
pub fn aplicar_headers_do_turno(resposta: &mut Response, stats: &TurnStats) {
    for (nome, valor) in [
        ("X-AI-Provider", &stats.provider),
        ("X-AI-Model", &stats.model),
    ] {
        if let Ok(v) = HeaderValue::from_str(valor) {
            resposta.headers_mut().insert(nome, v);
        }
    }
}
