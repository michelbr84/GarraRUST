use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use serde::Deserialize;
use subtle::ConstantTimeEq;
use tracing::{info, warn};

use super::WhatsAppChannel;
use super::api;
use super::signature::{SignatureError, verify_signature};

/// Header em que a Cloud API manda a assinatura do corpo.
const SIGNATURE_HEADER: &str = "X-Hub-Signature-256";

/// Corpo unico de toda recusa. Um texto por motivo viraria um oraculo que
/// diz a quem sonda o quao perto chegou; o motivo vai so para o log.
const FORBIDDEN_BODY: &str = "forbidden";

/// Shared state passed to WhatsApp webhook handlers.
pub type WhatsAppState = Arc<Vec<Arc<WhatsAppChannel>>>;

#[derive(Deserialize)]
pub struct VerifyParams {
    #[serde(rename = "hub.mode")]
    pub mode: Option<String>,
    #[serde(rename = "hub.verify_token")]
    pub verify_token: Option<String>,
    #[serde(rename = "hub.challenge")]
    pub challenge: Option<String>,
}

/// GET handler for WhatsApp webhook verification.
pub async fn whatsapp_verify(
    State(channels): State<WhatsAppState>,
    Query(params): Query<VerifyParams>,
) -> impl IntoResponse {
    let mode = params.mode.as_deref().unwrap_or("");
    let token = params.verify_token.as_deref().unwrap_or("");
    let challenge = params.challenge.as_deref().unwrap_or("");

    if mode != "subscribe" {
        return (StatusCode::FORBIDDEN, "invalid mode".to_string());
    }

    // Check token against any configured channel.
    //
    // #1070: comparacao em tempo constante. O `==` de String saia cedo no
    // primeiro byte diferente, e o tempo de resposta dizia quantos bytes
    // iniciais o atacante tinha acertado — um oraculo que permite descobrir
    // o token byte a byte. `fold` em vez de `any` de proposito: `any` faria
    // short-circuit no primeiro canal que casa, reintroduzindo o vazamento
    // quando ha mais de um canal configurado.
    let valid = channels.iter().fold(false, |acc, ch| {
        let hit: bool = ch.verify_token().as_bytes().ct_eq(token.as_bytes()).into();
        acc | hit
    });

    if valid {
        info!("whatsapp: webhook verified");
        (StatusCode::OK, challenge.to_string())
    } else {
        warn!("whatsapp: webhook verification failed — token mismatch");
        (StatusCode::FORBIDDEN, "invalid verify token".to_string())
    }
}

/// POST handler for incoming WhatsApp messages.
///
/// # Ordem dos extratores
///
/// `Bytes` e o **ultimo** de proposito: a assinatura da Meta cobre os bytes
/// crus do corpo, e `Json<Value>` consumiria o corpo ja parseado. Conferir o
/// HMAC contra o JSON re-serializado (espacos, ordem de chaves) recusaria
/// toda requisicao legitima — ou, pior, conferiria contra bytes diferentes
/// dos recebidos.
///
/// # Ordem das operacoes
///
/// A assinatura e verificada **antes** de qualquer `from_slice`. Um POST
/// forjado nunca alcanca o parser (#1070).
///
/// # Qual canal
///
/// Quando ha mais de um canal WhatsApp configurado, e a **assinatura** que
/// escolhe qual deles recebeu a mensagem — nunca um campo do corpo. Por
/// `phone_number_id` a escolha seria de quem manda o POST, que apontaria
/// para o canal de segredo mais fraco.
pub async fn whatsapp_webhook(
    State(channels): State<WhatsAppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let header = headers
        .get(SIGNATURE_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // A assinatura escolhe o canal. `fold` e nao `find`: sem short-circuit,
    // o tempo nao diz quantos canais foram tentados antes do acerto.
    let mut matched: Option<&std::sync::Arc<WhatsAppChannel>> = None;
    let mut last_err = SignatureError::MissingSecret;
    for ch in channels.iter() {
        match verify_signature(ch.app_secret(), &body, header) {
            Ok(()) => {
                if matched.is_none() {
                    matched = Some(ch);
                }
            }
            Err(e) => last_err = e,
        }
    }

    let Some(_channel) = matched else {
        warn!(
            reason = last_err.as_str(),
            "whatsapp: webhook rejeitado — assinatura invalida"
        );
        return (StatusCode::FORBIDDEN, FORBIDDEN_BODY).into_response();
    };

    // So a partir daqui o corpo e parseado.
    let body: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            // Corpo assinado mas ilegivel: 400, nao 403 — a autenticidade
            // esta provada, o formato e que esta errado.
            warn!("whatsapp: corpo assinado mas nao e JSON valido");
            return (StatusCode::BAD_REQUEST, "bad request").into_response();
        }
    };

    // WhatsApp sends: { "entry": [{ "changes": [{ "value": { "messages": [...] } }] }] }
    let entries = match body.get("entry").and_then(|v| v.as_array()) {
        Some(e) => e,
        None => return StatusCode::OK.into_response(),
    };

    for entry in entries {
        let changes = match entry.get("changes").and_then(|v| v.as_array()) {
            Some(c) => c,
            None => continue,
        };

        for change in changes {
            let value = match change.get("value") {
                Some(v) => v,
                None => continue,
            };

            // Get the phone_number_id this message was sent to
            let metadata_phone_id = value
                .get("metadata")
                .and_then(|m| m.get("phone_number_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let messages = match value.get("messages").and_then(|v| v.as_array()) {
                Some(m) => m,
                None => continue,
            };

            // Get contacts for display names
            let contacts = value.get("contacts").and_then(|v| v.as_array());

            for msg in messages {
                let msg_type = msg.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if msg_type != "text" {
                    continue;
                }

                let from = msg
                    .get("from")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let message_id = msg
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let text = msg
                    .get("text")
                    .and_then(|v| v.get("body"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                if text.trim().is_empty() {
                    continue;
                }

                // Try to get display name from contacts
                let user_name = contacts
                    .and_then(|c| {
                        c.iter().find_map(|contact| {
                            let wa_id = contact.get("wa_id").and_then(|v| v.as_str())?;
                            if wa_id == from {
                                contact
                                    .get("profile")
                                    .and_then(|p| p.get("name"))
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string())
                            } else {
                                None
                            }
                        })
                    })
                    .unwrap_or_else(|| from.clone());

                info!(
                    "whatsapp: message from {} ({}): {} chars",
                    user_name,
                    from,
                    text.len()
                );

                // Find the matching channel by phone_number_id
                let channel = channels
                    .iter()
                    .find(|ch| ch.phone_number_id() == metadata_phone_id)
                    .or_else(|| channels.first());

                let Some(channel) = channel else {
                    warn!(
                        "whatsapp: no channel configured for phone_number_id {metadata_phone_id}"
                    );
                    continue;
                };

                // Mark as read
                let client = channel.client();
                let token = channel.access_token();
                let phone_id = channel.phone_number_id().to_string();

                let read_client = client.clone();
                let read_token = token.to_string();
                let read_phone_id = phone_id.clone();
                let read_msg_id = message_id.clone();
                tokio::spawn(async move {
                    let _ =
                        api::mark_as_read(&read_client, &read_token, &read_phone_id, &read_msg_id)
                            .await;
                });

                // Process message
                let channel = Arc::clone(channel);
                let from_clone = from.clone();
                tokio::spawn(async move {
                    match channel
                        .handle_incoming(&from_clone, &user_name, &text)
                        .await
                    {
                        Ok(response) => {
                            if let Err(e) = api::send_text_message(
                                channel.client(),
                                channel.access_token(),
                                channel.phone_number_id(),
                                &from_clone,
                                &response,
                            )
                            .await
                            {
                                warn!("whatsapp: failed to send reply: {e}");
                            }
                        }
                        Err(e) if e == "__blocked__" => {
                            // Silently drop — unauthorized user
                        }
                        Err(e) => {
                            warn!("whatsapp: error processing message: {e}");
                            let _ = api::send_text_message(
                                channel.client(),
                                channel.access_token(),
                                channel.phone_number_id(),
                                &from_clone,
                                "Sorry, an error occurred processing your message.",
                            )
                            .await;
                        }
                    }
                });
            }
        }
    }

    StatusCode::OK.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::whatsapp::WhatsAppOnMessageFn;
    use crate::whatsapp::signature::compute_signature;
    use axum::Router;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::post;
    use std::sync::Arc;
    use tower::ServiceExt as _;

    const SEGREDO_A: &str = "app-secret-do-canal-a";
    const SEGREDO_B: &str = "app-secret-do-canal-b";
    const CORPO: &[u8] = br#"{"entry":[{"changes":[{"value":{"messages":[]}}]}]}"#;

    fn canal(secret: &str) -> Arc<WhatsAppChannel> {
        let on_msg: WhatsAppOnMessageFn =
            Arc::new(|_f, _u, _t, _d| Box::pin(async { Ok(String::new()) }));
        Arc::new(
            WhatsAppChannel::new(
                "token".into(),
                "123456".into(),
                "verify".into(),
                secret.into(),
                on_msg,
            )
            .expect("app_secret nao vazio"),
        )
    }

    fn app(canais: Vec<Arc<WhatsAppChannel>>) -> Router {
        Router::new()
            .route("/webhooks/whatsapp", post(whatsapp_webhook))
            .with_state(Arc::new(canais) as WhatsAppState)
    }

    /// `assinatura: None` = sem o header, que e como chega o primeiro POST
    /// de quem so descobriu a URL.
    fn requisicao(corpo: &[u8], assinatura: Option<&str>) -> Request<Body> {
        let mut req = Request::builder()
            .method("POST")
            .uri("/webhooks/whatsapp")
            .header("content-type", "application/json");
        if let Some(sig) = assinatura {
            req = req.header(SIGNATURE_HEADER, sig);
        }
        req.body(Body::from(corpo.to_vec()))
            .expect("requisicao bem formada")
    }

    async fn resposta(
        canais: Vec<Arc<WhatsAppChannel>>,
        req: Request<Body>,
    ) -> (StatusCode, String) {
        let resp = app(canais).oneshot(req).await.expect("router responde");
        let status = resp.status();
        let corpo = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .expect("corpo cabe");
        (status, String::from_utf8_lossy(&corpo).into_owned())
    }

    #[tokio::test]
    async fn assinatura_correta_e_aceita() {
        let sig = compute_signature(SEGREDO_A, CORPO);
        let (status, _) = resposta(vec![canal(SEGREDO_A)], requisicao(CORPO, Some(&sig))).await;
        assert_eq!(status, StatusCode::OK);
    }

    /// **O teste da #1070.** Este e o POST que o mundo inteiro podia mandar
    /// antes do fix: sem header nenhum. Com o fix revertido (voltando
    /// `Json<Value>` e tirando a verificacao) ele passa a responder 200 e
    /// este assert cai.
    #[tokio::test]
    async fn post_sem_assinatura_nao_passa() {
        let (status, corpo) = resposta(vec![canal(SEGREDO_A)], requisicao(CORPO, None)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(corpo, FORBIDDEN_BODY);
    }

    /// Todo modo de falha responde o **mesmo** 403 com o **mesmo** corpo.
    /// Uma mensagem por motivo diria a quem sonda se errou o prefixo, o hex,
    /// o tamanho ou so o segredo — um oraculo que encurta o ataque.
    #[tokio::test]
    async fn toda_falha_de_assinatura_da_o_mesmo_403() {
        let sig_de_outro = compute_signature(SEGREDO_B, CORPO);
        let hex_sem_prefixo = sig_de_outro
            .strip_prefix("sha256=")
            .expect("tem prefixo")
            .to_string();
        let casos: [(&str, Option<String>); 6] = [
            ("sem header", None),
            ("header vazio", Some(String::new())),
            ("sem o prefixo sha256=", Some(hex_sem_prefixo)),
            ("nao e hex", Some("sha256=!!!nao-e-hex!!!".into())),
            ("hex de tamanho errado", Some("sha256=deadbeef".into())),
            ("assinatura de outro segredo", Some(sig_de_outro.clone())),
        ];
        for (motivo, header) in casos {
            let (status, corpo) =
                resposta(vec![canal(SEGREDO_A)], requisicao(CORPO, header.as_deref())).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "caso: {motivo}");
            assert_eq!(corpo, FORBIDDEN_BODY, "caso: {motivo}");
        }
    }

    /// Corpo adulterado depois de assinado. A assinatura cobre os bytes
    /// crus, entao mudar um byte invalida — e o handler nem chega ao parser.
    #[tokio::test]
    async fn corpo_adulterado_nao_passa() {
        let sig = compute_signature(SEGREDO_A, CORPO);
        let outro = br#"{"entry":[{"changes":[{"value":{"messages":[{"x":1}]}}]}]}"#;
        let (status, _) = resposta(vec![canal(SEGREDO_A)], requisicao(outro, Some(&sig))).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    /// Com dois canais configurados, e a **assinatura** que escolhe qual
    /// recebeu — nao um campo do corpo. Por `phone_number_id` a escolha
    /// seria de quem manda o POST, que apontaria para o segredo mais fraco.
    #[tokio::test]
    async fn a_assinatura_escolhe_o_canal_nao_o_corpo() {
        let canais = vec![canal(SEGREDO_A), canal(SEGREDO_B)];
        for segredo in [SEGREDO_A, SEGREDO_B] {
            let sig = compute_signature(segredo, CORPO);
            let (status, _) = resposta(canais.clone(), requisicao(CORPO, Some(&sig))).await;
            assert_eq!(status, StatusCode::OK, "segredo {segredo} devia ser aceito");
        }
        // Um terceiro segredo, que nao e de nenhum canal, nao entra.
        let sig = compute_signature("segredo-de-ninguem", CORPO);
        let (status, _) = resposta(canais, requisicao(CORPO, Some(&sig))).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    /// Corpo assinado mas ilegivel e 400, nao 403: a autenticidade esta
    /// provada, o formato e que esta errado. Distinguir aqui nao vaza nada
    /// — quem chegou neste ponto ja tem o segredo.
    #[tokio::test]
    async fn corpo_assinado_mas_nao_json_da_400() {
        let lixo = b"isto nao e json";
        let sig = compute_signature(SEGREDO_A, lixo);
        let (status, _) = resposta(vec![canal(SEGREDO_A)], requisicao(lixo, Some(&sig))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}
