//! Handler HTTP do webhook do Bot Framework / Microsoft Teams (#1050).
//!
//! Mesma forma do `google_chat::webhook`: `Bytes` como ultimo extrator para
//! nada ser desserializado antes de o token conferir, 401 uniforme, o token
//! escolhendo o canal.
//!
//! ## A checagem que so existe aqui
//!
//! No Google Chat a resposta vai para `chat.googleapis.com`, host constante.
//! No Teams ela vai para o `serviceUrl` **que veio no corpo**, com o bearer
//! do bot junto. Se esse valor nao for amarrado a algo assinado, quem manda
//! a atividade escolhe para qual servidor o gateway entrega a credencial do
//! bot.
//!
//! Por isso [`super::TeamsChannel::verificar_token`] devolve o `serviceurl`
//! **assinado pela Microsoft**, e o handler exige que ele case com o
//! `serviceUrl` do corpo antes de despachar. Divergiu, e 401 como qualquer
//! outra falha de autenticacao: um corpo que discorda do proprio token nao e
//! um erro de formato, e uma tentativa de redirecionar a resposta.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use tracing::{info, warn};

use super::TeamsChannel;
use super::auth::{extrair_bearer, mesma_service_url};

/// Estado compartilhado com o handler: os canais Teams configurados.
pub type TeamsState = Arc<Vec<Arc<TeamsChannel>>>;

/// Corpo do 401. Constante: nada do pedido e ecoado.
const UNAUTHORIZED_BODY: &str = "teams: invalid or missing bearer token";

/// POST do webhook do Bot Framework.
pub async fn teams_webhook(
    State(channels): State<TeamsState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let Some(token) = extrair_bearer(
        headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok()),
    ) else {
        warn!("teams: webhook sem Authorization: Bearer");
        return (StatusCode::UNAUTHORIZED, UNAUTHORIZED_BODY);
    };

    let Some((channel, service_url_assinada)) = canal_do_token(&channels, token).await else {
        warn!(
            canais = channels.len(),
            "teams: webhook recusado — nenhum canal aceita este token"
        );
        return (StatusCode::UNAUTHORIZED, UNAUTHORIZED_BODY);
    };

    // So agora o corpo vira JSON.
    let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&body) else {
        warn!(
            canal = channel.name(),
            "teams: token valido mas corpo que nao e JSON"
        );
        return (StatusCode::BAD_REQUEST, "teams: malformed body");
    };

    let service_url_do_corpo = payload
        .get("serviceUrl")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    // O corpo tem de concordar com o token. Divergencia aqui e 401, e nao
    // 400: nao e formato malformado, e um pedido para responder num lugar
    // que o token nao autoriza — com o bearer do bot junto.
    if !mesma_service_url(service_url_do_corpo, service_url_assinada.as_str()) {
        warn!(
            canal = channel.name(),
            "teams: serviceUrl do corpo diverge da claim assinada do token; recusado"
        );
        return (StatusCode::UNAUTHORIZED, UNAUTHORIZED_BODY);
    }

    let Some((conversation_id, user_id, user_name, texto)) = mensagem_de_texto(&payload) else {
        info!(
            canal = channel.name(),
            "teams: atividade sem mensagem de texto, ignorada"
        );
        return (StatusCode::OK, "ok");
    };

    let (conversation_id, user_id, user_name, texto) = (
        conversation_id.to_string(),
        user_id.to_string(),
        user_name.to_string(),
        texto.to_string(),
    );
    // A URL usada na resposta e a **assinada**, nao a do corpo. As duas ja
    // conferem neste ponto; usar a do token deixa isso obvio para quem ler,
    // e sobrevive a alguem afrouxar a comparacao acima um dia.
    let service_url = service_url_assinada;
    let canal = Arc::clone(channel);

    tokio::spawn(async move {
        match canal
            .handle_incoming(&conversation_id, &user_id, &user_name, &texto)
            .await
        {
            Ok(resposta) => {
                if let Err(e) = canal
                    .send_activity(&service_url, &conversation_id, &resposta)
                    .await
                {
                    warn!(canal = canal.name(), "teams: falha ao responder: {e}");
                }
            }
            Err(motivo) if motivo == "__blocked__" => {}
            Err(motivo) => {
                warn!(canal = canal.name(), "teams: erro do agente: {motivo}");
            }
        }
    });

    info!(canal = channel.name(), "teams: webhook aceito");
    (StatusCode::OK, "ok")
}

/// Escolhe o canal dono do webhook **pelo token**, devolvendo junto o
/// `serviceurl` assinado dele.
///
/// Devolver os dois juntos e deliberado: separar "qual canal" de "qual
/// serviceUrl o token autoriza" convidaria alguem a usar um sem o outro.
async fn canal_do_token<'a>(
    channels: &'a [Arc<TeamsChannel>],
    token: &str,
) -> Option<(&'a Arc<TeamsChannel>, super::auth::ServiceUrlVerificada)> {
    for canal in channels {
        if let Ok(service_url) = canal.verificar_token(token).await {
            return Some((canal, service_url));
        }
    }
    None
}

/// Extrai `(conversation_id, user_id, user_name, texto)` de uma atividade
/// `message` com texto. Devolve `None` para qualquer outra coisa.
fn mensagem_de_texto(payload: &serde_json::Value) -> Option<(&str, &str, &str, &str)> {
    if payload.get("type")?.as_str()? != "message" {
        return None;
    }
    let texto = payload.get("text")?.as_str()?;
    if texto.trim().is_empty() {
        return None;
    }
    let conversation_id = payload.get("conversation")?.get("id")?.as_str()?;
    let from = payload.get("from")?;
    let user_id = from.get("id")?.as_str()?;
    let user_name = from.get("name").and_then(|v| v.as_str()).unwrap_or(user_id);
    Some((conversation_id, user_id, user_name, texto))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::teams::{TeamsConfig, TeamsOnMessageFn};
    use axum::Router;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::post;
    use tower::ServiceExt as _;

    fn canal(nome: &str, app_id: &str) -> Arc<TeamsChannel> {
        let on_msg: TeamsOnMessageFn =
            Arc::new(|_c, _u, _n, _t, _d| Box::pin(async { Ok(String::new()) }));
        Arc::new(
            TeamsChannel::new(
                TeamsConfig {
                    app_id: app_id.into(),
                    app_secret: "segredo".into(),
                    tenant_id: "tenant".into(),
                    name: nome.into(),
                },
                on_msg,
            )
            .expect("app_id nao vazio"),
        )
    }

    fn app(canais: Vec<Arc<TeamsChannel>>) -> Router {
        Router::new()
            .route("/webhooks/teams", post(teams_webhook))
            .with_state(Arc::new(canais) as TeamsState)
    }

    fn requisicao(corpo: &[u8], authorization: Option<&str>) -> Request<Body> {
        let mut req = Request::builder()
            .method("POST")
            .uri("/webhooks/teams")
            .header("content-type", "application/json");
        if let Some(a) = authorization {
            req = req.header("authorization", a);
        }
        req.body(Body::from(corpo.to_vec()))
            .expect("requisicao bem formada")
    }

    async fn resposta(canais: Vec<Arc<TeamsChannel>>, req: Request<Body>) -> (StatusCode, String) {
        let resp = app(canais).oneshot(req).await.expect("router responde");
        let status = resp.status();
        let corpo = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .expect("corpo cabe");
        (status, String::from_utf8_lossy(&corpo).into_owned())
    }

    const CORPO: &[u8] =
        br#"{"type":"message","serviceUrl":"https://smba.trafficmanager.net/amer"}"#;

    /// Antes deste PR a rota nao existia. Montada sem autenticacao — o
    /// "wiring ingenuo", que e o que o WhatsApp faz hoje (#1070) — um POST
    /// sem header nenhum chegaria ao agente **e** faria o gateway responder
    /// para o `serviceUrl` que o proprio POST escolheu.
    #[tokio::test]
    async fn post_sem_authorization_nao_passa() {
        let (status, corpo) = resposta(vec![canal("t", "app-id")], requisicao(CORPO, None)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(corpo, UNAUTHORIZED_BODY);
    }

    #[tokio::test]
    async fn toda_falha_de_token_da_o_mesmo_401() {
        let casos: [(&str, Option<&str>); 8] = [
            ("sem header", None),
            ("header vazio", Some("")),
            ("so o esquema", Some("Bearer")),
            ("esquema errado", Some("Basic dXNlcjpwYXNz")),
            ("token que nao e jwt", Some("Bearer nao-sou-um-jwt")),
            (
                "alg none",
                Some("Bearer eyJhbGciOiJub25lIiwidHlwIjoiSldUIiwia2lkIjoiayJ9.e30."),
            ),
            (
                "alg HS256",
                Some("Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCIsImtpZCI6ImsifQ.e30.x"),
            ),
            (
                "RS256 sem kid",
                Some("Bearer eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.e30.x"),
            ),
        ];
        for (caso, auth) in casos {
            let (status, corpo) =
                resposta(vec![canal("t", "app-id")], requisicao(CORPO, auth)).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{caso} deveria dar 401");
            assert_eq!(
                corpo, UNAUTHORIZED_BODY,
                "{caso} deveria dar o corpo constante"
            );
        }
    }

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

    /// O corpo nao e olhado antes do token: um payload que quebraria o parser
    /// sai no 401, nao no 400.
    #[tokio::test]
    async fn corpo_invalido_sem_token_sai_no_401_nao_no_400() {
        let (status, _) = resposta(
            vec![canal("t", "app-id")],
            requisicao(b"{ isto nao fecha", None),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn atividade_de_texto_completa_e_aceita() {
        let p = serde_json::json!({
            "type": "message",
            "text": "oi",
            "conversation": {"id": "conv-1"},
            "from": {"id": "user-1", "name": "Fulano"},
        });
        assert_eq!(
            mensagem_de_texto(&p),
            Some(("conv-1", "user-1", "Fulano", "oi"))
        );
    }

    #[test]
    fn sem_name_no_from_cai_no_id() {
        let p = serde_json::json!({
            "type": "message",
            "text": "oi",
            "conversation": {"id": "conv-1"},
            "from": {"id": "user-1"},
        });
        assert_eq!(
            mensagem_de_texto(&p),
            Some(("conv-1", "user-1", "user-1", "oi"))
        );
    }

    #[test]
    fn o_que_nao_e_mensagem_de_texto_e_ignorado() {
        let casos = [
            serde_json::json!({"type": "conversationUpdate"}),
            serde_json::json!({"type": "typing"}),
            serde_json::json!({"type": "invoke"}),
            // Sem texto nao ha o que perguntar ao agente.
            serde_json::json!({
                "type": "message",
                "conversation": {"id": "c"}, "from": {"id": "u"}
            }),
            // Texto so com espaco idem.
            serde_json::json!({
                "type": "message", "text": "   ",
                "conversation": {"id": "c"}, "from": {"id": "u"}
            }),
            // Sem conversation nao ha para onde responder.
            serde_json::json!({"type": "message", "text": "oi", "from": {"id": "u"}}),
            // Sem from nao ha identidade para a allowlist.
            serde_json::json!({"type": "message", "text": "oi", "conversation": {"id": "c"}}),
            serde_json::json!({}),
        ];
        for caso in casos {
            assert_eq!(mensagem_de_texto(&caso), None, "deveria ignorar: {caso}");
        }
    }
}
