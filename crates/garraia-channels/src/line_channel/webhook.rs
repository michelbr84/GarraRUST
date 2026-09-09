//! Handler HTTP do webhook do LINE (#1050, requisitos vindos do #1051).
//!
//! O molde de forma é o `whatsapp/webhook.rs`, **mas não o de corpo**. Aquele
//! extrai `Json<Value>`, o que consome o corpo já parseado — e para o LINE
//! isso seria fatal: a assinatura é um HMAC-SHA256 sobre os **bytes crus**,
//! e um corpo re-serializado a partir do `Value` não bate. Por isso aqui o
//! último extrator é `Bytes`, e nada é desserializado antes de a assinatura
//! conferir.
//!
//! ## Invariantes
//!
//! - **Verificar antes de parsear.** `serde_json::from_slice` só acontece
//!   depois de um `validate_signature` que passou. Parsear antes daria a um
//!   POST forjado a chance de exercitar o parser.
//! - **403 uniforme.** Header ausente, Base64 malformado, tamanho errado,
//!   HMAC diferente, segredo em branco: tudo responde o mesmo 403 com o mesmo
//!   corpo. Distinguir os casos diria a quem está sondando o quão perto
//!   chegou. O motivo vai só para o log.
//! - **A assinatura escolhe o canal.** Com mais de um canal LINE configurado,
//!   é o HMAC que diz de quem é o webhook — não um campo do corpo, que o
//!   atacante controla.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use tracing::{info, warn};

use super::LineChannel;

/// Estado compartilhado com o handler: os canais LINE configurados.
pub type LineState = Arc<Vec<Arc<LineChannel>>>;

/// O header que o LINE usa para a assinatura. Case-insensitive na leitura,
/// porque `HeaderMap` normaliza.
const SIGNATURE_HEADER: &str = "x-line-signature";

/// Corpo do 403. Constante: nada do pedido é ecoado, e todos os motivos de
/// recusa respondem exatamente isto.
const FORBIDDEN_BODY: &str = "line: invalid webhook signature";

/// POST do webhook do LINE.
///
/// `Bytes` é o **último** extrator de propósito: ele consome o corpo, e é o
/// corpo cru que a assinatura cobre.
pub async fn line_webhook(
    State(channels): State<LineState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let assinatura = headers
        .get(SIGNATURE_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let Some(channel) = canal_da_assinatura(&channels, &body, assinatura) else {
        // Um `warn!` so, e nao um por canal: com N canais configurados, um
        // POST forjado geraria N linhas, e quem forja controla a frequencia.
        warn!(
            canais = channels.len(),
            "line: webhook recusado — nenhuma assinatura confere"
        );
        return (StatusCode::FORBIDDEN, FORBIDDEN_BODY);
    };

    // Só agora o corpo vira JSON.
    let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&body) else {
        warn!(
            canal = channel.name(),
            "line: webhook com assinatura valida mas corpo que nao e JSON"
        );
        // Assinatura confere, então quem mandou tem o segredo: é um erro de
        // formato, não uma tentativa de forjar. 400, não 403.
        return (StatusCode::BAD_REQUEST, "line: malformed body");
    };

    let eventos = payload
        .get("events")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    // Responder rápido e trabalhar depois: o reply token do LINE tem janela
    // curta, e o webhook tem timeout. Awaitar o turno do agente aqui gastaria
    // os dois.
    let canal = Arc::clone(channel);
    tokio::spawn(async move {
        for evento in eventos {
            let Some((reply_token, user_id, texto)) = evento_de_texto(&evento) else {
                continue;
            };

            match canal.handle_incoming(reply_token, user_id, texto).await {
                Ok(resposta) => {
                    if let Err(e) = canal.reply_message(reply_token, &resposta).await {
                        warn!(canal = canal.name(), "line: falha ao responder: {e}");
                    }
                }
                // O canal usa `__blocked__` para "recusado em silencio", como
                // os outros: allowlist, pareamento, injecao de prompt.
                Err(motivo) if motivo == "__blocked__" => {}
                Err(motivo) => {
                    warn!(canal = canal.name(), "line: erro do agente: {motivo}");
                }
            }
        }
    });

    info!(canal = channel.name(), "line: webhook aceito");
    (StatusCode::OK, "ok")
}

/// Escolhe o canal dono do webhook **pela assinatura**.
///
/// Com mais de um canal LINE configurado alguem tem de decidir de quem e a
/// requisicao. Fazer isso por um campo do corpo (`destination`, por exemplo)
/// entregaria a escolha a quem manda o POST: bastaria apontar para o canal de
/// segredo mais fraco. O HMAC nao tem esse problema — so acerta quem tem o
/// segredo daquele canal.
///
/// Devolve o primeiro que confere. Dois canais com o mesmo `channel_secret`
/// sao o mesmo canal do ponto de vista do LINE, entao a ordem nao importa.
fn canal_da_assinatura<'a>(
    channels: &'a [Arc<LineChannel>],
    body: &[u8],
    assinatura: &str,
) -> Option<&'a Arc<LineChannel>> {
    channels
        .iter()
        .find(|ch| ch.verify_webhook_signature(body, assinatura).is_ok())
}

/// Extrai `(reply_token, user_id, texto)` de um evento de mensagem de texto.
/// Devolve `None` para qualquer outra coisa — follow, sticker, imagem, evento
/// sem os campos.
///
/// O tipo do evento fica **fora** do retorno de proposito: quem recebe um
/// `Some` ja sabe que e `"message"`/`"text"`, porque nenhum outro caminho
/// chega ate aqui. Devolve-lo faria o call-site parecer estar conferindo algo
/// que ja e invariante — um match que nunca falha e pior que nenhum, porque
/// sugere uma checagem que nao existe.
fn evento_de_texto(evento: &serde_json::Value) -> Option<(&str, &str, &str)> {
    if evento.get("type")?.as_str()? != "message" {
        return None;
    }
    let mensagem = evento.get("message")?;
    if mensagem.get("type")?.as_str()? != "text" {
        return None;
    }
    Some((
        evento.get("replyToken")?.as_str()?,
        evento.get("source")?.get("userId")?.as_str()?,
        mensagem.get("text")?.as_str()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::line_channel::signature::compute_signature;
    use crate::line_channel::{LineConfig, LineOnMessageFn};
    use axum::Router;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::post;
    use tower::ServiceExt as _;

    const SEGREDO_A: &str = "segredo-do-canal-a";
    const SEGREDO_B: &str = "segredo-do-canal-b";

    /// Corpo com `events` vazio: o handler responde 200 sem nada para
    /// despachar, entao nenhum teste de rota toca a rede da LINE API.
    const CORPO: &[u8] = br#"{"destination":"Uxxxx","events":[]}"#;

    fn canal(nome: &str, segredo: &str) -> Arc<LineChannel> {
        let on_msg: LineOnMessageFn =
            Arc::new(|_r, _u, _n, _t, _d| Box::pin(async { Ok(String::new()) }));
        Arc::new(
            LineChannel::new(
                LineConfig {
                    channel_access_token: "token".into(),
                    channel_secret: segredo.into(),
                    name: nome.into(),
                },
                on_msg,
            )
            .expect("segredo nao vazio"),
        )
    }

    fn app(canais: Vec<Arc<LineChannel>>) -> Router {
        Router::new()
            .route("/webhooks/line", post(line_webhook))
            .with_state(Arc::new(canais) as LineState)
    }

    /// Monta o POST. `assinatura: None` = sem o header, que e como chega o
    /// primeiro POST de quem so descobriu a URL.
    fn requisicao(corpo: &[u8], assinatura: Option<&str>) -> Request<Body> {
        let mut req = Request::builder()
            .method("POST")
            .uri("/webhooks/line")
            .header("content-type", "application/json");
        if let Some(sig) = assinatura {
            req = req.header(SIGNATURE_HEADER, sig);
        }
        req.body(Body::from(corpo.to_vec()))
            .expect("requisicao bem formada")
    }

    async fn resposta(canais: Vec<Arc<LineChannel>>, req: Request<Body>) -> (StatusCode, String) {
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
        let (status, _) =
            resposta(vec![canal("a", SEGREDO_A)], requisicao(CORPO, Some(&sig))).await;
        assert_eq!(status, StatusCode::OK);
    }

    /// Os quatro modos de falha respondem o **mesmo** 403 com o **mesmo**
    /// corpo. Uma mensagem diferente por motivo viraria um oraculo: diria a
    /// quem esta sondando se errou o formato, o tamanho ou so o segredo.
    #[tokio::test]
    async fn toda_falha_de_assinatura_da_o_mesmo_403() {
        let sig_de_outro_segredo = compute_signature(SEGREDO_B, CORPO);
        let casos: [(&str, Option<String>); 5] = [
            ("sem header", None),
            ("header vazio", Some(String::new())),
            ("nao e base64", Some("!!! nao e base64 !!!".into())),
            ("base64 curto demais", Some("AAAA".into())),
            ("segredo errado", Some(sig_de_outro_segredo)),
        ];
        for (caso, sig) in casos {
            let (status, corpo) = resposta(
                vec![canal("a", SEGREDO_A)],
                requisicao(CORPO, sig.as_deref()),
            )
            .await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{caso} deveria dar 403");
            assert_eq!(
                corpo, FORBIDDEN_BODY,
                "{caso} deveria dar o corpo constante"
            );
        }
    }

    /// **A prova por reversao.** Trocar `Bytes` por `Json<Value>` no handler
    /// faria a assinatura ser conferida contra o JSON re-serializado, e este
    /// teste e o unico que percebe: os bytes crus tem espaco depois dos `:` e
    /// as chaves fora de ordem alfabetica, e o `serde_json::to_vec` de um
    /// `Value` produz nem um nem outro. O corpo abaixo assina como veio.
    #[tokio::test]
    async fn a_assinatura_cobre_os_bytes_crus_nao_o_json_reserializado() {
        let cru = br#"{ "events" : [] , "destination" : "Uxxxx" }"#;
        let reserializado = serde_json::to_vec(
            &serde_json::from_slice::<serde_json::Value>(cru).expect("json valido"),
        )
        .expect("serializa");
        assert_ne!(
            cru.as_slice(),
            reserializado.as_slice(),
            "o teste so tem valor se os dois corpos diferirem em bytes"
        );

        // Assinado como o LINE assina: sobre o que foi enviado.
        let sig = compute_signature(SEGREDO_A, cru);
        let (status, _) = resposta(vec![canal("a", SEGREDO_A)], requisicao(cru, Some(&sig))).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "o corpo cru assinado tem de passar — se falhar, o handler nao ve os bytes originais"
        );

        // E a assinatura do corpo re-serializado nao vale para o corpo cru.
        let sig_do_reserializado = compute_signature(SEGREDO_A, &reserializado);
        let (status, _) = resposta(
            vec![canal("a", SEGREDO_A)],
            requisicao(cru, Some(&sig_do_reserializado)),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    /// Corpo alterado depois de assinado — o caso que a assinatura existe
    /// para pegar.
    #[tokio::test]
    async fn corpo_adulterado_e_recusado() {
        let sig = compute_signature(SEGREDO_A, CORPO);
        let adulterado = br#"{"destination":"Uyyyy","events":[]}"#;
        let (status, corpo) = resposta(
            vec![canal("a", SEGREDO_A)],
            requisicao(adulterado, Some(&sig)),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(corpo, FORBIDDEN_BODY);
    }

    /// Assinatura valida com corpo que nao e JSON: 400, nao 403. Quem chegou
    /// aqui tem o segredo, entao e erro de formato, nao forja — e distinguir
    /// os dois nao vaza nada para quem nao tem o segredo, que nunca chega.
    #[tokio::test]
    async fn assinatura_valida_com_corpo_invalido_da_400() {
        let lixo = b"nao sou json";
        let sig = compute_signature(SEGREDO_A, lixo);
        let (status, _) = resposta(vec![canal("a", SEGREDO_A)], requisicao(lixo, Some(&sig))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    /// Sem nenhum canal configurado a rota nao pode virar um buraco aberto:
    /// o `find` de uma lista vazia nao acha nada, e nada e 403.
    #[tokio::test]
    async fn sem_canais_configurados_tudo_e_recusado() {
        let sig = compute_signature(SEGREDO_A, CORPO);
        let (status, corpo) = resposta(Vec::new(), requisicao(CORPO, Some(&sig))).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(corpo, FORBIDDEN_BODY);
    }

    /// E a assinatura, nao um campo do corpo, que diz de qual canal e o
    /// webhook. O `destination` abaixo aponta para o canal errado de
    /// proposito: se a escolha olhasse para ele, quem manda o POST escolheria
    /// contra qual segredo ser conferido.
    #[test]
    fn a_assinatura_escolhe_o_canal_nao_o_corpo() {
        let canais = vec![canal("a", SEGREDO_A), canal("b", SEGREDO_B)];
        let corpo = br#"{"destination":"canal-a","events":[]}"#;

        let escolhido = canal_da_assinatura(&canais, corpo, &compute_signature(SEGREDO_B, corpo));
        assert_eq!(
            escolhido.map(|c| c.name()),
            Some("b"),
            "quem assinou foi o canal b, mesmo o corpo dizendo canal-a"
        );

        let escolhido = canal_da_assinatura(&canais, corpo, &compute_signature(SEGREDO_A, corpo));
        assert_eq!(escolhido.map(|c| c.name()), Some("a"));

        assert!(
            canal_da_assinatura(&canais, corpo, &compute_signature("terceiro", corpo)).is_none(),
            "segredo de nenhum canal configurado nao escolhe canal nenhum"
        );
    }

    fn evento(json: serde_json::Value) -> Option<(String, String, String)> {
        evento_de_texto(&json).map(|(r, u, t)| (r.to_string(), u.to_string(), t.to_string()))
    }

    #[test]
    fn evento_de_texto_completo_e_aceito() {
        let e = evento(serde_json::json!({
            "type": "message",
            "replyToken": "rt1",
            "source": {"userId": "U1"},
            "message": {"type": "text", "text": "oi"},
        }));
        assert_eq!(
            e,
            Some(("rt1".into(), "U1".into(), "oi".into())),
            "evento bem formado deveria passar"
        );
    }

    /// Tudo que nao e mensagem de texto e ignorado sem tocar no agente: um
    /// `follow` ou um sticker nao tem `text`, e adivinhar seria pior.
    #[test]
    fn o_que_nao_e_mensagem_de_texto_e_ignorado() {
        let casos = [
            serde_json::json!({"type": "follow", "replyToken": "rt", "source": {"userId": "U"}}),
            serde_json::json!({
                "type": "message",
                "replyToken": "rt",
                "source": {"userId": "U"},
                "message": {"type": "sticker", "packageId": "1"},
            }),
            // Sem replyToken nao ha como responder.
            serde_json::json!({
                "type": "message",
                "source": {"userId": "U"},
                "message": {"type": "text", "text": "oi"},
            }),
            // Sem userId nao ha identidade para a allowlist.
            serde_json::json!({
                "type": "message",
                "replyToken": "rt",
                "source": {},
                "message": {"type": "text", "text": "oi"},
            }),
            serde_json::json!({}),
            serde_json::json!({"type": "message"}),
        ];
        for caso in casos {
            assert_eq!(evento(caso.clone()), None, "deveria ignorar: {caso}");
        }
    }
}
