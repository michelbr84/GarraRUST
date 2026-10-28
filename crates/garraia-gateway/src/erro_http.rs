//! Status HTTP para um erro vindo do agente/provider (#1726).
//!
//! `api.rs` e `openai_api.rs` devolviam `500` para QUALQUER `Err` do runtime,
//! inclusive o `400` do provider dizendo que o modelo pedido nao existe: erro
//! do cliente virava "Internal Server Error", com o corpo bruto do provider
//! atras — que no OpenRouter carrega o `user_id` da conta do operador. O log
//! do dono tem o caso real (`"garraia is not a valid model ID"` -> 500) vindo
//! de um cliente OpenAI-compat.
//!
//! A classe sai do texto (`status=NNN`), como `is_retryable_error` ja faz no
//! runtime: o `Error::Agent` do provider nao e tipado por status, e a leitura
//! por texto e o contrato que a crate de agentes expoe em
//! [`garraia_agents::status_http_do_erro`]. O corpo que chega aqui ja vem
//! sanitizado na origem (`providers::corpo_de_erro_sanitizado`).

use axum::http::StatusCode;
use garraia_common::Error;

/// O status HTTP que o gateway responde para `e`.
///
/// - `400`/`404`/`413`/`422` do provider: o PEDIDO esta errado (modelo
///   inexistente, request malformado, contexto acima do limite) -> `400`.
/// - `429` -> `429`: o cliente pode esperar e repetir.
/// - `401`/`403` do provider: credencial do OPERADOR, nao do cliente -> `502`.
/// - `5xx` do provider e falha de transporte (`Error::Transport`) -> `502`.
/// - qualquer outra coisa -> `500`, como antes.
pub fn status_para_erro_do_agente(e: &Error) -> StatusCode {
    if matches!(e, Error::Transport(_)) {
        return StatusCode::BAD_GATEWAY;
    }
    match garraia_agents::status_http_do_erro(e) {
        Some(400 | 404 | 413 | 422) => StatusCode::BAD_REQUEST,
        Some(429) => StatusCode::TOO_MANY_REQUESTS,
        Some(401 | 403) => StatusCode::BAD_GATEWAY,
        Some(500..=599) => StatusCode::BAD_GATEWAY,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// O `type` do objeto de erro no formato da OpenAI para o status escolhido.
pub fn tipo_openai(status: StatusCode) -> &'static str {
    match status {
        StatusCode::BAD_REQUEST => "invalid_request_error",
        StatusCode::TOO_MANY_REQUESTS => "rate_limit_error",
        _ => "server_error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agente(msg: &str) -> Error {
        Error::Agent(msg.to_string())
    }

    /// #1726: o caso que motivou — modelo invalido e erro do cliente.
    #[test]
    fn modelo_invalido_e_400_nao_500() {
        let e = agente(
            "openai API error: status=400 Bad Request, body=garraia is not a valid model ID (code=400)",
        );
        assert_eq!(status_para_erro_do_agente(&e), StatusCode::BAD_REQUEST);
        assert_eq!(
            tipo_openai(StatusCode::BAD_REQUEST),
            "invalid_request_error"
        );
    }

    #[test]
    fn tabela_de_status() {
        for (status, esperado) in [
            (404, StatusCode::BAD_REQUEST),
            (413, StatusCode::BAD_REQUEST),
            (422, StatusCode::BAD_REQUEST),
            (429, StatusCode::TOO_MANY_REQUESTS),
            (401, StatusCode::BAD_GATEWAY),
            (403, StatusCode::BAD_GATEWAY),
            (500, StatusCode::BAD_GATEWAY),
            (503, StatusCode::BAD_GATEWAY),
            (529, StatusCode::BAD_GATEWAY),
        ] {
            let e = agente(&format!("openai API error: status={status} X, body=y"));
            assert_eq!(status_para_erro_do_agente(&e), esperado, "status={status}");
        }
    }

    /// Transporte e `502` sem precisar de status; erro sem status continua
    /// `500` — o comportamento anterior, para o que nao sabemos classificar.
    #[test]
    fn transporte_e_502_e_sem_status_continua_500() {
        let t = Error::Transport("openai request failed: error sending request".into());
        assert_eq!(status_para_erro_do_agente(&t), StatusCode::BAD_GATEWAY);
        assert_eq!(
            status_para_erro_do_agente(&agente("falha ao interpretar resposta")),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            tipo_openai(StatusCode::INTERNAL_SERVER_ERROR),
            "server_error"
        );
        assert_eq!(tipo_openai(StatusCode::BAD_GATEWAY), "server_error");
    }
}
