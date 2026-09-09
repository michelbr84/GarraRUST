//! Handler HTTP do webhook do Google Chat (#1050).
//!
//! Estrutura igual a do LINE (`line_channel::webhook`), com **uma**
//! diferenca de fundo: a prova de autenticidade nao esta no corpo, esta no
//! header. O LINE assina os bytes crus com um segredo compartilhado, entao
//! la o corpo tem de chegar intacto e `Bytes` e obrigatorio. Aqui a prova e
//! um JWT no `Authorization`, independente do corpo — entao `Json<Value>`
//! seria seguro.
//!
//! Ainda assim o corpo chega como `Bytes`, por dois motivos:
//!
//! - **A verificacao acontece antes do parse.** Com `Json<Value>` o Axum
//!   desserializa o corpo *antes* de o handler rodar, ou seja antes de
//!   qualquer autenticacao — um POST anonimo exercitaria o parser de JSON,
//!   que e superficie de ataque de graca. Com `Bytes`, nada e interpretado
//!   ate o token conferir.
//! - **Uma forma so para os dois canais push.** O proximo leitor nao
//!   precisa decidir, canal a canal, se aquele usa o corpo cru ou nao.
//!
//! ## Invariantes
//!
//! - **401 uniforme.** Header ausente, `alg` errado, `kid` desconhecido,
//!   assinatura ruim, `aud` de outra app, token expirado: tudo responde o
//!   mesmo 401 com o mesmo corpo. O motivo vai so para o log.
//! - **O `aud` e conferido.** Todo webhook do Chat, de toda app do mundo, e
//!   assinado pela mesma chave do Google. Sem `aud`, um token legitimo
//!   emitido para a app de outra pessoa — que essa pessoa pode encaminhar
//!   para ca — passaria. Ver `auth.rs`.
//! - **O token escolhe o canal.** Com mais de um canal Google Chat
//!   configurado, e o `aud` que diz de quem e a requisicao, nao um campo do
//!   corpo, que quem manda controla.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use tracing::{info, warn};

use super::GoogleChatChannel;
use super::auth::extrair_bearer;

/// Estado compartilhado com o handler: os canais Google Chat configurados.
pub type GoogleChatState = Arc<Vec<Arc<GoogleChatChannel>>>;

/// Corpo do 401. Constante: nada do pedido e ecoado, e todos os motivos de
/// recusa respondem exatamente isto.
const UNAUTHORIZED_BODY: &str = "google chat: invalid or missing bearer token";

/// POST do webhook do Google Chat.
pub async fn google_chat_webhook(
    State(channels): State<GoogleChatState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let Some(token) = extrair_bearer(
        headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok()),
    ) else {
        warn!("google chat: webhook sem Authorization: Bearer");
        return (StatusCode::UNAUTHORIZED, UNAUTHORIZED_BODY);
    };

    let Some(channel) = canal_do_token(&channels, token).await else {
        // Um `warn!` so, e nao um por canal: quem forja controla a
        // frequencia das requisicoes, e um log por canal multiplicaria isso.
        warn!(
            canais = channels.len(),
            "google chat: webhook recusado — nenhum canal aceita este token"
        );
        return (StatusCode::UNAUTHORIZED, UNAUTHORIZED_BODY);
    };

    // So agora o corpo vira JSON.
    let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&body) else {
        warn!(
            canal = channel.name(),
            "google chat: token valido mas corpo que nao e JSON"
        );
        // Quem chegou aqui tem um token valido para este canal: e erro de
        // formato, nao forja. 400, nao 401 — e a distincao nao vaza nada
        // para quem nao tem token, que sai no 401 antes.
        return (StatusCode::BAD_REQUEST, "google chat: malformed body");
    };

    let Some((space, user_id, user_name, texto)) = mensagem_de_texto(&payload) else {
        // ADDED_TO_SPACE, REMOVED_FROM_SPACE, CARD_CLICKED, mensagem sem
        // texto: nada a responder, e adivinhar seria pior.
        info!(
            canal = channel.name(),
            "google chat: evento sem mensagem de texto, ignorado"
        );
        return (StatusCode::OK, "ok");
    };

    let (space, user_id, user_name, texto) = (
        space.to_string(),
        user_id.to_string(),
        user_name.to_string(),
        texto.to_string(),
    );
    let canal = Arc::clone(channel);

    // Responder rapido e trabalhar depois: o webhook tem timeout, e esperar
    // o turno do agente aqui o estouraria.
    tokio::spawn(async move {
        match canal
            .handle_incoming(&space, &user_id, &user_name, &texto)
            .await
        {
            Ok(resposta) => {
                if let Err(e) = canal.send_to_space(&space, &resposta).await {
                    warn!(canal = canal.name(), "google chat: falha ao responder: {e}");
                }
            }
            // `__blocked__` e "recusado em silencio", como nos outros
            // canais: allowlist, pareamento, injecao de prompt.
            Err(motivo) if motivo == "__blocked__" => {}
            Err(motivo) => {
                warn!(
                    canal = canal.name(),
                    "google chat: erro do agente: {motivo}"
                );
            }
        }
    });

    info!(canal = channel.name(), "google chat: webhook aceito");
    (StatusCode::OK, "ok")
}

/// Escolhe o canal dono do webhook **pelo token**.
///
/// Cada canal tem a sua `audience`, e o `aud` do token so casa com a do
/// projeto que o emitiu. Fazer a escolha por um campo do corpo (`space.name`,
/// por exemplo) entregaria a decisao a quem manda o POST, que apontaria para
/// o canal de configuracao mais frouxa.
///
/// `async` e sequencial de proposito: na esmagadora maioria das instalacoes
/// ha um canal so, e o primeiro que casa encerra o laco.
async fn canal_do_token<'a>(
    channels: &'a [Arc<GoogleChatChannel>],
    token: &str,
) -> Option<&'a Arc<GoogleChatChannel>> {
    for canal in channels {
        if canal.verificar_token(token).await.is_ok() {
            return Some(canal);
        }
    }
    None
}

/// Extrai `(space, user_id, user_name, texto)` de um evento `MESSAGE` com
/// texto. Devolve `None` para qualquer outra coisa.
///
/// O `space` sai de `message.space.name` (ex.: `spaces/AAAA`), que e o que
/// a API REST espera de volta em `send_to_space`.
fn mensagem_de_texto(payload: &serde_json::Value) -> Option<(&str, &str, &str, &str)> {
    if payload.get("type")?.as_str()? != "MESSAGE" {
        return None;
    }
    let message = payload.get("message")?;
    let texto = message.get("text")?.as_str()?;
    if texto.is_empty() {
        return None;
    }
    let space = message.get("space")?.get("name")?.as_str()?;
    let sender = message.get("sender")?;
    let user_id = sender.get("name")?.as_str()?;
    // `displayName` e opcional no payload; sem ele o proprio id serve, como
    // no LINE.
    let user_name = sender
        .get("displayName")
        .and_then(|v| v.as_str())
        .unwrap_or(user_id);
    Some((space, user_id, user_name, texto))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::google_chat::{GoogleChatConfig, GoogleChatOnMessageFn};
    use axum::Router;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::post;
    use tower::ServiceExt as _;

    fn canal(nome: &str, audience: &str) -> Arc<GoogleChatChannel> {
        let on_msg: GoogleChatOnMessageFn =
            Arc::new(|_s, _u, _n, _t, _d| Box::pin(async { Ok(String::new()) }));
        Arc::new(
            GoogleChatChannel::new(
                GoogleChatConfig {
                    webhook_url: None,
                    service_account_key_path: None,
                    service_account_token: String::new(),
                    audience: audience.into(),
                    name: nome.into(),
                },
                on_msg,
            )
            .expect("audience nao vazia"),
        )
    }

    fn app(canais: Vec<Arc<GoogleChatChannel>>) -> Router {
        Router::new()
            .route("/webhooks/google-chat", post(google_chat_webhook))
            .with_state(Arc::new(canais) as GoogleChatState)
    }

    fn requisicao(corpo: &[u8], authorization: Option<&str>) -> Request<Body> {
        let mut req = Request::builder()
            .method("POST")
            .uri("/webhooks/google-chat")
            .header("content-type", "application/json");
        if let Some(a) = authorization {
            req = req.header("authorization", a);
        }
        req.body(Body::from(corpo.to_vec()))
            .expect("requisicao bem formada")
    }

    async fn resposta(
        canais: Vec<Arc<GoogleChatChannel>>,
        req: Request<Body>,
    ) -> (StatusCode, String) {
        let resp = app(canais).oneshot(req).await.expect("router responde");
        let status = resp.status();
        let corpo = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .expect("corpo cabe");
        (status, String::from_utf8_lossy(&corpo).into_owned())
    }

    const CORPO: &[u8] = br#"{"type":"MESSAGE"}"#;

    /// **O teste que prova o buraco fechado.** Antes deste PR a rota nao
    /// existia; se ela for montada sem autenticacao — o que seria o
    /// "wiring" ingenuo, e o que o WhatsApp faz hoje (#1070) — um POST sem
    /// header nenhum chega ao agente. Aqui ele para no 401.
    #[tokio::test]
    async fn post_sem_authorization_nao_passa() {
        let (status, corpo) =
            resposta(vec![canal("gc", "1234567890")], requisicao(CORPO, None)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(corpo, UNAUTHORIZED_BODY);
    }

    /// Todos os modos de falha respondem o **mesmo** 401 com o **mesmo**
    /// corpo. Uma mensagem por motivo diria a quem sonda o quao perto
    /// chegou — se falta o header, se o `alg` esta errado, se o `aud` e de
    /// outra app.
    #[tokio::test]
    async fn toda_falha_de_token_da_o_mesmo_401() {
        let casos: [(&str, Option<&str>); 8] = [
            ("sem header", None),
            ("header vazio", Some("")),
            ("so o esquema", Some("Bearer")),
            ("esquema errado", Some("Basic dXNlcjpwYXNz")),
            ("token que nao e jwt", Some("Bearer nao-sou-um-jwt")),
            // {"alg":"none","typ":"JWT","kid":"k"}
            (
                "alg none",
                Some("Bearer eyJhbGciOiJub25lIiwidHlwIjoiSldUIiwia2lkIjoiayJ9.e30."),
            ),
            // {"alg":"HS256","typ":"JWT","kid":"chave-1"}
            (
                "alg HS256",
                Some("Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCIsImtpZCI6ImNoYXZlLTEifQ.e30.x"),
            ),
            // {"alg":"RS256","typ":"JWT"} — sem kid
            (
                "RS256 sem kid",
                Some("Bearer eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.e30.x"),
            ),
        ];
        for (caso, auth) in casos {
            let (status, corpo) =
                resposta(vec![canal("gc", "1234567890")], requisicao(CORPO, auth)).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{caso} deveria dar 401");
            assert_eq!(
                corpo, UNAUTHORIZED_BODY,
                "{caso} deveria dar o corpo constante"
            );
        }
    }

    /// Sem nenhum canal configurado a rota nao pode virar buraco aberto.
    #[tokio::test]
    async fn sem_canais_configurados_tudo_e_recusado() {
        let (status, corpo) = resposta(
            Vec::new(),
            requisicao(CORPO, Some("Bearer qualquer.coisa.aqui")),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(corpo, UNAUTHORIZED_BODY);
    }

    /// O corpo nao e nem olhado antes do token. Um payload que quebraria o
    /// parser sai no 401, nao no 400 — o parser nunca roda.
    #[tokio::test]
    async fn corpo_invalido_sem_token_sai_no_401_nao_no_400() {
        let (status, _) = resposta(
            vec![canal("gc", "1234567890")],
            requisicao(b"{ isto nao fecha", None),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "o parse nao pode acontecer antes da autenticacao"
        );
    }

    #[test]
    fn mensagem_de_texto_completa_e_aceita() {
        let p = serde_json::json!({
            "type": "MESSAGE",
            "message": {
                "text": "oi",
                "space": {"name": "spaces/AAAA"},
                "sender": {"name": "users/111", "displayName": "Fulano"},
            }
        });
        assert_eq!(
            mensagem_de_texto(&p),
            Some(("spaces/AAAA", "users/111", "Fulano", "oi"))
        );
    }

    /// Sem `displayName` o proprio id serve — o payload nem sempre traz.
    #[test]
    fn sem_display_name_cai_no_id() {
        let p = serde_json::json!({
            "type": "MESSAGE",
            "message": {
                "text": "oi",
                "space": {"name": "spaces/AAAA"},
                "sender": {"name": "users/111"},
            }
        });
        assert_eq!(
            mensagem_de_texto(&p),
            Some(("spaces/AAAA", "users/111", "users/111", "oi"))
        );
    }

    #[test]
    fn o_que_nao_e_mensagem_de_texto_e_ignorado() {
        let casos = [
            serde_json::json!({"type": "ADDED_TO_SPACE"}),
            serde_json::json!({"type": "REMOVED_FROM_SPACE"}),
            serde_json::json!({"type": "CARD_CLICKED"}),
            // Sem texto nao ha o que perguntar ao agente.
            serde_json::json!({
                "type": "MESSAGE",
                "message": {"space": {"name": "spaces/A"}, "sender": {"name": "users/1"}}
            }),
            // Texto vazio idem.
            serde_json::json!({
                "type": "MESSAGE",
                "message": {"text": "", "space": {"name": "spaces/A"}, "sender": {"name": "users/1"}}
            }),
            // Sem space nao ha para onde responder.
            serde_json::json!({
                "type": "MESSAGE",
                "message": {"text": "oi", "sender": {"name": "users/1"}}
            }),
            // Sem sender nao ha identidade para a allowlist.
            serde_json::json!({
                "type": "MESSAGE",
                "message": {"text": "oi", "space": {"name": "spaces/A"}}
            }),
            serde_json::json!({}),
        ];
        for caso in casos {
            assert_eq!(mensagem_de_texto(&caso), None, "deveria ignorar: {caso}");
        }
    }
}
