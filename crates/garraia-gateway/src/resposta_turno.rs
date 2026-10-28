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
        model: stats
            .as_ref()
            .and_then(|t| modelo_do_turno(t).map(str::to_string)),
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
        ("X-AI-Provider", Some(stats.provider.as_str())),
        ("X-AI-Model", modelo_do_turno(stats)),
    ] {
        // Vazio nao e header: fica para o middleware do router preencher o
        // default da config, como quando o handler nao sabe.
        let Some(valor) = valor.filter(|v| !v.is_empty()) else {
            continue;
        };
        if let Ok(v) = HeaderValue::from_str(valor) {
            resposta.headers_mut().insert(nome, v);
        }
    }
}

/// O modelo que o turno registrou, ou `None` quando o runtime nao soube dizer.
///
/// `TurnStats::model` copia o `model` da resposta do provider, e ha providers
/// que devolvem a string vazia quando o pedido nao nomeou modelo (o cliente
/// quer o default do servidor — #1541). Propagar `""` como "quem respondeu"
/// seria mentir com cara de dado; quem chama decide o fallback: o rotulo
/// pedido no OpenAI-compat, campo omitido no `POST /api/sessions/{id}/messages`.
pub fn modelo_do_turno(stats: &TurnStats) -> Option<&str> {
    let modelo = stats.model.trim();
    (!modelo.is_empty()).then_some(modelo)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(model: &str) -> TurnStats {
        TurnStats {
            provider: "anota".to_string(),
            model: model.to_string(),
            ..TurnStats::default()
        }
    }

    /// #1541 x #1733: provider que ecoa o pedido vazio nao vira `"model": ""`.
    #[test]
    fn modelo_vazio_e_desconhecido_nao_string_vazia() {
        assert_eq!(modelo_do_turno(&stats("")), None);
        assert_eq!(modelo_do_turno(&stats("   ")), None);
        assert_eq!(modelo_do_turno(&stats("anota-1")), Some("anota-1"));
    }

    #[test]
    fn headers_pulam_o_modelo_vazio_mas_poem_o_provider() {
        let mut resposta = StatusCode::OK.into_response();
        aplicar_headers_do_turno(&mut resposta, &stats(""));
        assert_eq!(
            resposta
                .headers()
                .get("X-AI-Provider")
                .and_then(|v| v.to_str().ok()),
            Some("anota")
        );
        assert!(
            resposta.headers().get("X-AI-Model").is_none(),
            "modelo vazio fica para o default do router"
        );
    }

    #[test]
    fn montar_omite_o_modelo_que_o_runtime_nao_soube() {
        let resposta = montar("s".to_string(), "oi".to_string(), Some(stats("")));
        assert!(resposta.headers().get("X-AI-Model").is_none());
        assert_eq!(
            resposta
                .headers()
                .get("X-AI-Provider")
                .and_then(|v| v.to_str().ok()),
            Some("anota")
        );
    }
}
