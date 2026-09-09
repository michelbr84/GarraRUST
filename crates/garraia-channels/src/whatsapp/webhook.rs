use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use serde::Deserialize;
use sha2::{Digest, Sha256};
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
    //
    // Compara os digests SHA-256 e nao os bytes crus: o `ct_eq` do `subtle`
    // sai imediatamente quando os slices tem tamanhos diferentes, e isso
    // vazava o **comprimento** do token configurado. Passando pelo digest,
    // os dois lados tem sempre 32 bytes e o XOR roda inteiro. O digest nao
    // precisa ser secreto — o atacante so ve igual/diferente, como antes;
    // o que muda e que o tempo para de responder "errou o tamanho".
    let token_digest = Sha256::digest(token.as_bytes());
    let valid = channels.iter().fold(false, |acc, ch| {
        let esperado = Sha256::digest(ch.verify_token().as_bytes());
        let hit: bool = esperado.ct_eq(&token_digest).into();
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
/// escolhe qual deles atende — nunca um campo do corpo. O canal escolhido e
/// o que respondeu ao HMAC, e e com o `access_token` dele que a resposta
/// sai.
///
/// Isso importa mais do que parece. Se o roteamento fosse por
/// `metadata.phone_number_id` do corpo, quem conhecesse o `app_secret` do
/// canal mais fraco assinaria um corpo apontando para outro canal e a
/// resposta sairia com o **token do outro** — escalada entre canais com uma
/// credencial de baixo valor. O corpo estar assinado nao ajuda: ele esta
/// assinado pela chave errada.
///
/// Um `phone_number_id` que nao seja o do canal autenticado e aceito apenas
/// quando **nenhum** canal configurado o reivindica: uma WABA pode ter varios
/// numeros sob a mesma app, todos assinados pelo mesmo `app_secret`, e nesse
/// caso o canal autenticado e mesmo o certo. Se o numero pertence a **outro**
/// canal configurado, o evento e descartado: trafego legitimo daquele numero
/// viria assinado pelo segredo dele.
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
    // `NoChannels` e o estado inicial de proposito: com a lista vazia o loop
    // nao roda e nenhum `Err` sobrescreve isto, entao o log diz "nenhum canal
    // configurado" em vez de culpar um `app_secret` que nunca foi consultado.
    let mut last_err = SignatureError::NoChannels;
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

    let Some(canal_autenticado) = matched else {
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

                // O canal e o que a assinatura escolheu. O
                // `metadata.phone_number_id` do corpo NAO decide nada: ver
                // "# Qual canal" na doc do handler.
                let channel = canal_autenticado;

                // Unica pergunta que o corpo pode fazer: este numero e de
                // OUTRO canal configurado? Se for, o evento nao devia estar
                // assinado por este segredo, e responder com o token deste
                // canal seria falar por um numero que nao e o dele.
                if !metadata_phone_id.is_empty()
                    && metadata_phone_id != channel.phone_number_id()
                    && channels
                        .iter()
                        .any(|ch| ch.phone_number_id() == metadata_phone_id)
                {
                    warn!(
                        "whatsapp: corpo assinado por um canal reivindica o phone_number_id de \
                         outro canal configurado; evento descartado"
                    );
                    continue;
                }

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

    /// Canal com `phone_number_id` proprio e um callback que grava quem
    /// atendeu. Devolve `Err("__blocked__")`, que o handler trata como "user
    /// nao autorizado" e descarta em silencio — assim o teste observa a
    /// escolha do canal sem que nada saia para a rede.
    fn canal_que_grava(
        secret: &str,
        phone_id: &str,
        marca: &'static str,
        diario: Arc<std::sync::Mutex<Vec<&'static str>>>,
    ) -> Arc<WhatsAppChannel> {
        let on_msg: WhatsAppOnMessageFn = Arc::new(move |_f, _u, _t, _d| {
            let diario = Arc::clone(&diario);
            Box::pin(async move {
                diario.lock().expect("mutex do teste").push(marca);
                Err("__blocked__".to_string())
            })
        });
        Arc::new(
            WhatsAppChannel::new(
                format!("token-de-{marca}"),
                phone_id.into(),
                "verify".into(),
                secret.into(),
                on_msg,
            )
            .expect("app_secret nao vazio"),
        )
    }

    /// Corpo com uma mensagem de texto de verdade, endereçada a `phone_id`.
    fn corpo_para(phone_id: &str) -> Vec<u8> {
        serde_json::json!({
            "entry": [{
                "changes": [{
                    "value": {
                        "metadata": { "phone_number_id": phone_id },
                        "contacts": [{ "wa_id": "5511999", "profile": { "name": "Fulano" } }],
                        "messages": [{
                            "type": "text",
                            "id": "wamid.1",
                            "from": "5511999",
                            "text": { "body": "oi" }
                        }]
                    }
                }]
            }]
        })
        .to_string()
        .into_bytes()
    }

    /// As tasks do handler sao `tokio::spawn`; da tempo a elas de gravar.
    async fn quem_atendeu(diario: &Arc<std::sync::Mutex<Vec<&'static str>>>) -> Vec<&'static str> {
        for _ in 0..50 {
            if !diario.lock().expect("mutex do teste").is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        // Mais uma volta para pegar um segundo canal que tenha atendido
        // errado — senao o teste passaria por chegar cedo demais.
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        diario.lock().expect("mutex do teste").clone()
    }

    fn app(canais: Vec<Arc<WhatsAppChannel>>) -> Router {
        Router::new()
            .route("/webhooks/whatsapp", post(whatsapp_webhook))
            .with_state(Arc::new(canais) as WhatsAppState)
    }

    /// Como o `app`, mas com o `GET` do handshake tambem — a rota de
    /// producao monta os dois no mesmo path (`router.rs`).
    fn app_com_get(canais: Vec<Arc<WhatsAppChannel>>) -> Router {
        Router::new()
            .route(
                "/webhooks/whatsapp",
                axum::routing::get(whatsapp_verify).post(whatsapp_webhook),
            )
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

    /// Com dois canais configurados, qualquer um dos dois segredos autentica
    /// — e nenhum outro.
    #[tokio::test]
    async fn so_os_segredos_dos_canais_configurados_autenticam() {
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

    /// O canal que atende e o que **assinou**, e nao o que o corpo aponta.
    ///
    /// Este teste afirma a escolha, nao so o status: gravar 200 nao distingue
    /// "canal A atendeu" de "canal B atendeu", que e exatamente a diferenca
    /// que importa aqui.
    #[tokio::test]
    async fn quem_atende_e_o_canal_que_assinou() {
        let diario = Arc::new(std::sync::Mutex::new(Vec::new()));
        let canais = vec![
            canal_que_grava(SEGREDO_A, "111", "A", Arc::clone(&diario)),
            canal_que_grava(SEGREDO_B, "222", "B", Arc::clone(&diario)),
        ];
        let corpo = corpo_para("111");
        let sig = compute_signature(SEGREDO_A, &corpo);

        let (status, _) = resposta(canais, requisicao(&corpo, Some(&sig))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(quem_atendeu(&diario).await, vec!["A"]);
    }

    /// A escalada entre canais que o roteamento por corpo permitia.
    ///
    /// Quem conhece o `app_secret` do canal A assina um corpo cujo
    /// `metadata.phone_number_id` e o do canal B. Antes, o handler procurava
    /// o canal por esse campo e respondia com o **`access_token` do B** —
    /// uma credencial de baixo valor dirigindo a conta de outro canal. O
    /// corpo estar assinado nao ajudava: estava assinado pela chave errada.
    ///
    /// Agora o evento e descartado, porque trafego legitimo do numero do B
    /// viria assinado pelo segredo do B.
    #[tokio::test]
    async fn segredo_de_um_canal_nao_dirige_o_outro() {
        let diario = Arc::new(std::sync::Mutex::new(Vec::new()));
        let canais = vec![
            canal_que_grava(SEGREDO_A, "111", "A", Arc::clone(&diario)),
            canal_que_grava(SEGREDO_B, "222", "B", Arc::clone(&diario)),
        ];
        // Assinado por A, apontando para o numero do B.
        let corpo = corpo_para("222");
        let sig = compute_signature(SEGREDO_A, &corpo);

        let (status, _) = resposta(canais, requisicao(&corpo, Some(&sig))).await;
        // 200: a assinatura e valida, so o roteamento e que nao. Devolver
        // 403 aqui daria a quem tem o segredo do A um jeito de descobrir
        // quais numeros estao configurados no gateway.
        assert_eq!(status, StatusCode::OK);
        assert!(
            quem_atendeu(&diario).await.is_empty(),
            "nenhum canal devia atender: nem o B (nao assinou) nem o A (nao e o numero dele)"
        );
    }

    /// Numero que nenhum canal reivindica continua atendido pelo canal que
    /// assinou. Uma WABA pode ter varios numeros sob a mesma app da Meta,
    /// todos assinados pelo mesmo `app_secret`, e o operador so configurou
    /// um deles — descartar aqui deixaria o canal mudo.
    #[tokio::test]
    async fn numero_de_ninguem_ainda_vai_para_quem_assinou() {
        let diario = Arc::new(std::sync::Mutex::new(Vec::new()));
        let canais = vec![canal_que_grava(SEGREDO_A, "111", "A", Arc::clone(&diario))];
        let corpo = corpo_para("999");
        let sig = compute_signature(SEGREDO_A, &corpo);

        let (status, _) = resposta(canais, requisicao(&corpo, Some(&sig))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(quem_atendeu(&diario).await, vec!["A"]);
    }

    /// O header em minusculas tambem casa. O hyper normaliza `HeaderName`
    /// para lowercase, entao `headers.get("X-Hub-Signature-256")` acha o
    /// header qualquer que seja a capitalizacao na rede. Documentado num
    /// teste porque a alternativa e alguem "consertar" isso um dia por
    /// medo de um proxy que mude o case.
    #[tokio::test]
    async fn header_em_minusculas_tambem_casa() {
        let sig = compute_signature(SEGREDO_A, CORPO);
        let req = Request::builder()
            .method("POST")
            .uri("/webhooks/whatsapp")
            .header("content-type", "application/json")
            .header("x-hub-signature-256", &sig)
            .body(Body::from(CORPO.to_vec()))
            .expect("request valida");
        let (status, _) = resposta(vec![canal(SEGREDO_A)], req).await;
        assert_eq!(status, StatusCode::OK);
    }

    /// O `hub.verify_token` do GET nao pode vazar o **comprimento** do token
    /// configurado. Comparar os digests SHA-256 iguala os tamanhos, entao um
    /// palpite de qualquer comprimento e recusado do mesmo jeito.
    #[tokio::test]
    async fn verify_token_de_qualquer_tamanho_e_recusado_igual() {
        let app = app_com_get(vec![canal(SEGREDO_A)]);
        for palpite in ["", "v", "verif", "verifyyyyyyyyyyyyyyyyyyyyyyyyyyyy"] {
            let req = Request::builder()
                .method("GET")
                .uri(format!(
                    "/webhooks/whatsapp?hub.mode=subscribe&hub.verify_token={palpite}&hub.challenge=abc"
                ))
                .body(Body::empty())
                .expect("request valida");
            let r = app.clone().oneshot(req).await.expect("resposta");
            assert_eq!(
                r.status(),
                StatusCode::FORBIDDEN,
                "palpite de {} chars devia ser recusado",
                palpite.len()
            );
        }
    }

    /// E o token certo passa, devolvendo o `hub.challenge` — senao o teste
    /// acima passaria tambem com a verificacao quebrada em "recusa tudo".
    #[tokio::test]
    async fn verify_token_correto_devolve_o_challenge() {
        let app = app_com_get(vec![canal(SEGREDO_A)]);
        let req = Request::builder()
            .method("GET")
            .uri("/webhooks/whatsapp?hub.mode=subscribe&hub.verify_token=verify&hub.challenge=abc")
            .body(Body::empty())
            .expect("request valida");
        let r = app.oneshot(req).await.expect("resposta");
        assert_eq!(r.status(), StatusCode::OK);
        let corpo = axum::body::to_bytes(r.into_body(), 4096)
            .await
            .expect("corpo");
        assert_eq!(&corpo[..], b"abc");
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
