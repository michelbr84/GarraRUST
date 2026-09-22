//! GAR-187 ponta a ponta no gateway: o "sim" do turno seguinte aprova (#1343).
//!
//! Sobe os handlers reais (`/ws`, `/v1/chat/completions`, `POST /chat` do
//! mobile) num servidor HTTP de verdade, com um provider de roteiro e uma
//! tool que pede confirmacao e conta quantas vezes rodou. Antes do #1343 o
//! historico destes canais e texto puro, o `ToolResult` pausado nunca
//! voltava, e o "sim" caia no vazio: a tool perguntava de novo para sempre.
//!
//! Cada teste monta o proprio `AppState` (e a propria tool), entao os
//! contadores nao se misturam.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock};

use async_trait::async_trait;
use axum::Router;
use axum::routing::{get, post};
use futures::{SinkExt, Stream, StreamExt};
use garraia_agents::tools::approval::ApprovalFingerprint;
use garraia_agents::{
    AgentRuntime, ChatMessage, ChatRole, ContentBlock, LlmProvider, LlmRequest, LlmResponse,
    MessagePart, StreamEvent, Tool, ToolContext, ToolOutput,
};
use garraia_channels::ChannelRegistry;
use garraia_config::AppConfig;
use garraia_gateway::state::AppState;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

/// `AppState::new` le/grava `allowlist.json` e `mcp.json` no diretorio de
/// config: aponta o binario inteiro para um tempdir antes de qualquer teste
/// montar estado (mesmo padrao de `admin_mcp_restart_allowlist.rs`).
static TEST_ENV: LazyLock<tempfile::TempDir> = LazyLock::new(|| {
    let dir = tempfile::tempdir().expect("temp config dir");
    // SAFETY: o `LazyLock` serializa isto contra as outras threads de teste
    // deste binario; todo teste chama `test_env()` primeiro.
    unsafe {
        std::env::set_var("GARRAIA_CONFIG_DIR", dir.path());
        std::env::set_var(
            garraia_gateway::mcp::McpPersistenceService::DISABLE_AUTOPROVISION_ENV,
            "1",
        );
    }
    dir
});

fn test_env() {
    LazyLock::force(&TEST_ENV);
}

const TOOL: &str = "apaga_arquivo";
const ALVO: &str = "/tmp/garraia-1343-alvo";

/// A tool perigosa do teste: sem aprovacao que cubra `(TOOL, ALVO)`, pede
/// confirmacao; com ela, "roda" (conta).
struct ApagaArquivo {
    rodou: Arc<AtomicUsize>,
}

#[async_trait]
impl Tool for ApagaArquivo {
    fn name(&self) -> &str {
        TOOL
    }
    fn description(&self) -> &str {
        "apaga um arquivo (stub do teste)"
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object"})
    }
    async fn execute(&self, c: &ToolContext, _i: Value) -> garraia_common::Result<ToolOutput> {
        if c.approval.covers(TOOL, ALVO) {
            self.rodou.fetch_add(1, Ordering::SeqCst);
            return Ok(ToolOutput::success(format!("apagado: {ALVO}")));
        }
        let marcador = ApprovalFingerprint::of(TOOL, ALVO).marker();
        Ok(ToolOutput::confirmation_request(format!(
            "Confirma apagar {ALVO}? Responda sim. {marcador}"
        )))
    }
}

/// O modelo do roteiro: a cada mensagem humana pede a tool (inclusive no
/// "sim" — e assim que o GAR-187 funciona, o modelo repete a chamada e a
/// aprovacao a cobre), menos no "nao", que responde em texto. Depois de um
/// resultado de tool, encerra em texto.
struct Roteiro;

impl Roteiro {
    fn pede_tool(request: &LlmRequest) -> bool {
        match request.messages.last() {
            Some(ChatMessage {
                role: ChatRole::User,
                content: MessagePart::Text(t),
            }) => t.trim().to_lowercase() != "nao",
            _ => false,
        }
    }
}

#[async_trait]
impl LlmProvider for Roteiro {
    fn provider_id(&self) -> &str {
        "roteiro"
    }

    async fn complete(&self, request: &LlmRequest) -> garraia_common::Result<LlmResponse> {
        let content = if Self::pede_tool(request) {
            vec![ContentBlock::ToolUse {
                id: "t-1343".to_string(),
                name: TOOL.to_string(),
                input: json!({ "alvo": ALVO }),
            }]
        } else {
            vec![ContentBlock::Text {
                text: "feito".to_string(),
            }]
        };
        Ok(LlmResponse {
            content,
            model: "m".to_string(),
            stop_reason: None,
            usage: None,
        })
    }

    async fn stream_complete(
        &self,
        request: &LlmRequest,
    ) -> garraia_common::Result<
        std::pin::Pin<Box<dyn Stream<Item = garraia_common::Result<StreamEvent>> + Send>>,
    > {
        let eventos = if Self::pede_tool(request) {
            vec![
                Ok(StreamEvent::ToolUseStart {
                    index: 0,
                    id: "t-1343".to_string(),
                    name: TOOL.to_string(),
                }),
                Ok(StreamEvent::InputJsonDelta(
                    json!({ "alvo": ALVO }).to_string(),
                )),
                Ok(StreamEvent::ContentBlockStop { index: 0 }),
                Ok(StreamEvent::MessageStop),
            ]
        } else {
            vec![
                Ok(StreamEvent::TextDelta("feito".to_string())),
                Ok(StreamEvent::MessageStop),
            ]
        };
        Ok(Box::pin(futures::stream::iter(eventos)))
    }

    async fn health_check(&self) -> garraia_common::Result<bool> {
        Ok(true)
    }
}

/// O pedido de confirmacao pela frase da ferramenta de teste. Desde a #1373 o
/// marcador interno nao chega ao texto do humano (e digita-lo nunca aprovou
/// nada), entao toda resposta que passa por aqui tambem prova que ele sumiu.
fn e_pedido(resposta: &str) -> bool {
    assert!(
        !resposta.contains(garraia_agents::tools::approval::MARKER_PREFIX),
        "o marcador interno chegou ao texto do humano: {resposta}"
    );
    resposta.contains("Confirma apagar")
}

struct Gateway {
    base: String,
    rodou: Arc<AtomicUsize>,
}

impl Gateway {
    fn rodou(&self) -> usize {
        self.rodou.load(Ordering::SeqCst)
    }
}

/// Sobe `/ws`, `/v1/chat/completions` e `/chat` num `AppState` novo.
/// `preparo` mexe no estado antes do `Arc` (dono, JWT).
async fn subir(preparo: impl FnOnce(&mut AppState)) -> Gateway {
    test_env();
    let rt = AgentRuntime::new();
    let rodou = Arc::new(AtomicUsize::new(0));
    rt.register_tool(Box::new(ApagaArquivo {
        rodou: Arc::clone(&rodou),
    }));
    rt.register_provider(Arc::new(Roteiro));

    let mut config = AppConfig::default();
    config.memory.enabled = false;
    let mut state = AppState::new(config, Arc::new(rt), ChannelRegistry::new());
    preparo(&mut state);
    let state = Arc::new(state);

    let app = Router::new()
        .route("/ws", get(garraia_gateway::ws::ws_handler))
        .route(
            "/ws/parrot",
            get(garraia_gateway::parrot_ws::parrot_ws_handler),
        )
        .route("/chat", post(garraia_gateway::mobile_chat::chat))
        .with_state(Arc::clone(&state))
        .merge(garraia_gateway::openai_api::build_openai_router(state));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Gateway {
        base: format!("127.0.0.1:{}", addr.port()),
        rodou,
    }
}

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Le frames ate o primeiro cujo `type` e um dos pedidos (os logs do
/// broadcast e frames de progresso ficam pelo caminho).
async fn ate(ws: &mut Ws, tipos: &[&str]) -> Value {
    loop {
        let frame = tokio::time::timeout(std::time::Duration::from_secs(20), ws.next())
            .await
            .expect("frame a tempo")
            .expect("socket aberto")
            .expect("frame valido");
        if let Message::Text(t) = frame
            && let Ok(v) = serde_json::from_str::<Value>(&t)
            && v["type"].as_str().is_some_and(|x| tipos.contains(&x))
        {
            return v;
        }
    }
}

/// Conecta e abre uma sessao nova; devolve o socket e o `session_id`.
async fn conectar(gw: &Gateway) -> (Ws, String) {
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{}/ws", gw.base))
        .await
        .expect("ws connect");
    ws.send(Message::Text(json!({"type": "init"}).to_string().into()))
        .await
        .expect("init");
    let boas_vindas = ate(&mut ws, &["connected"]).await;
    let sid = boas_vindas["session_id"]
        .as_str()
        .expect("session_id")
        .to_string();
    (ws, sid)
}

/// Retoma `sid` num socket novo, SEM token — o caminho permissivo do
/// `ws.rs` enquanto a sessao esta em memoria.
async fn retomar(gw: &Gateway, sid: &str) -> Ws {
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{}/ws", gw.base))
        .await
        .expect("ws connect");
    ws.send(Message::Text(
        json!({"type": "resume", "session_id": sid})
            .to_string()
            .into(),
    ))
    .await
    .expect("resume");
    let ack = ate(&mut ws, &["resumed", "connected"]).await;
    assert_eq!(ack["type"], "resumed", "a sessao estava em memoria: {ack}");
    ws
}

async fn falar(ws: &mut Ws, texto: &str) -> String {
    ws.send(Message::Text(json!({"content": texto}).to_string().into()))
        .await
        .expect("enviar");
    let r = ate(ws, &["message", "error"]).await;
    assert_eq!(r["type"], "message", "turno falhou: {r}");
    r["content"].as_str().expect("content").to_string()
}

// ── WebSocket (chat web) ────────────────────────────────────────────────

/// O bug do #1343 no canal web: pausa, "sim" no turno seguinte roda UMA
/// vez, e um segundo "sim" (replay) pausa de novo sem rodar outra vez.
#[tokio::test]
async fn ws_sim_no_turno_seguinte_roda_uma_vez_e_replay_pausa() {
    let gw = subir(|_| {}).await;
    let (mut ws, _) = conectar(&gw).await;

    let r1 = falar(&mut ws, "apaga o arquivo").await;
    assert!(e_pedido(&r1), "turno 1 pausa: {r1}");
    assert_eq!(gw.rodou(), 0);

    let r2 = falar(&mut ws, "sim").await;
    assert!(!e_pedido(&r2), "o sim aprovou: {r2}");
    assert_eq!(gw.rodou(), 1, "roda exatamente uma vez");

    let r3 = falar(&mut ws, "sim").await;
    assert!(e_pedido(&r3), "replay pausa de novo: {r3}");
    assert_eq!(gw.rodou(), 1, "replay nao roda de novo");
}

/// A mesma sessao retomada em OUTRA conexao (sem token) nao aprova o pedido
/// da primeira — e o "sim" dela encerra o pedido, entao o "sim" tardio da
/// primeira tambem nao roda.
#[tokio::test]
async fn ws_outra_conexao_na_mesma_sessao_nao_aprova() {
    let gw = subir(|_| {}).await;
    let (mut a, sid) = conectar(&gw).await;

    let r1 = falar(&mut a, "apaga o arquivo").await;
    assert!(e_pedido(&r1), "{r1}");

    let mut b = retomar(&gw, &sid).await;
    let rb = falar(&mut b, "sim").await;
    assert!(e_pedido(&rb), "outra conexao nao aprova: {rb}");
    assert_eq!(gw.rodou(), 0);

    let ra = falar(&mut a, "sim").await;
    assert!(e_pedido(&ra), "o pedido ja tinha sido encerrado: {ra}");
    assert_eq!(gw.rodou(), 0);
}

/// Outra sessao, mesmo processo: nao alcanca o pedido.
#[tokio::test]
async fn ws_outra_sessao_nao_aprova() {
    let gw = subir(|_| {}).await;
    let (mut a, _) = conectar(&gw).await;
    let (mut b, _) = conectar(&gw).await;

    assert!(e_pedido(&falar(&mut a, "apaga o arquivo").await));
    assert!(e_pedido(&falar(&mut b, "sim").await));
    assert_eq!(gw.rodou(), 0);

    // O pedido de A continua de pe: B nao tocou nele.
    assert!(!e_pedido(&falar(&mut a, "sim").await));
    assert_eq!(gw.rodou(), 1);
}

/// "nao" encerra o pedido; o "sim" depois nao ressuscita.
#[tokio::test]
async fn ws_nao_depois_sim_nao_aprova() {
    let gw = subir(|_| {}).await;
    let (mut ws, _) = conectar(&gw).await;

    assert!(e_pedido(&falar(&mut ws, "apaga o arquivo").await));
    let r = falar(&mut ws, "nao").await;
    assert!(!e_pedido(&r), "{r}");
    assert!(e_pedido(&falar(&mut ws, "sim").await));
    assert_eq!(gw.rodou(), 0);
}

// ── Desktop (parrot) ──────────────────────────────────────────────────────

async fn parrot(gw: &Gateway) -> Ws {
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{}/ws/parrot", gw.base))
        .await
        .expect("parrot connect");
    ate(&mut ws, &["connected"]).await;
    ws
}

async fn falar_parrot(ws: &mut Ws, texto: &str) -> String {
    ws.send(Message::Text(
        json!({"type": "message", "text": texto}).to_string().into(),
    ))
    .await
    .expect("enviar");
    let r = ate(ws, &["response", "error"]).await;
    assert_eq!(r["type"], "response", "turno falhou: {r}");
    r["text"].as_str().expect("text").to_string()
}

/// A sessao do desktop e fixa (`parrot-desktop`): quem aprova e a conexao.
/// A mesma conexao aprova uma vez; outra conexao na mesma sessao nao.
#[tokio::test]
async fn parrot_mesma_conexao_aprova_e_outra_conexao_nao() {
    let gw = subir(|_| {}).await;
    let mut a = parrot(&gw).await;

    assert!(e_pedido(&falar_parrot(&mut a, "apaga o arquivo").await));
    assert!(!e_pedido(&falar_parrot(&mut a, "sim").await));
    assert_eq!(gw.rodou(), 1);

    assert!(e_pedido(&falar_parrot(&mut a, "apaga o arquivo").await));
    let mut b = parrot(&gw).await;
    assert!(e_pedido(&falar_parrot(&mut b, "sim").await));
    assert_eq!(gw.rodou(), 1, "outra conexao nao aprova");
}

// ── API compativel com OpenAI ───────────────────────────────────────────

/// Um turno em `/v1/chat/completions`. `sessao: None` nao manda
/// `X-Session-Id`; `authorization` vai como header cru (bytes), para o teste
/// do header que nao e UTF-8. Com `stream`, remonta os deltas do SSE ate o
/// `[DONE]`.
async fn openai_turno(
    gw: &Gateway,
    sessao: Option<&str>,
    authorization: Option<&[u8]>,
    texto: &str,
    stream: bool,
) -> String {
    let mut req = reqwest::Client::new()
        .post(format!("http://{}/v1/chat/completions", gw.base))
        .json(&json!({
            "messages": [{"role": "user", "content": texto}],
            "stream": stream,
        }));
    if let Some(s) = sessao {
        req = req.header("x-session-id", s);
    }
    if let Some(a) = authorization {
        req = req.header(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_bytes(a).expect("header"),
        );
    }
    let resp = tokio::time::timeout(std::time::Duration::from_secs(20), req.send())
        .await
        .expect("resposta a tempo")
        .expect("post");
    assert!(resp.status().is_success(), "status {}", resp.status());
    if !stream {
        let v: Value = resp.json().await.expect("json");
        return v["choices"][0]["message"]["content"]
            .as_str()
            .expect("content")
            .to_string();
    }
    let corpo = tokio::time::timeout(std::time::Duration::from_secs(20), resp.text())
        .await
        .expect("stream a tempo")
        .expect("corpo");
    let mut saida = String::new();
    let mut fechou = false;
    for linha in corpo.lines() {
        let Some(dado) = linha.strip_prefix("data:") else {
            continue;
        };
        let dado = dado.trim();
        if dado == "[DONE]" {
            fechou = true;
            break;
        }
        let v: Value = serde_json::from_str(dado).expect("chunk json");
        if let Some(t) = v["choices"][0]["delta"]["content"].as_str() {
            saida.push_str(t);
        }
    }
    assert!(fechou, "o SSE terminou sem [DONE]: {corpo}");
    saida
}

fn bearer(b: &str) -> Vec<u8> {
    format!("Bearer {b}").into_bytes()
}

async fn openai(gw: &Gateway, sessao: &str, cred: Option<&str>, texto: &str) -> String {
    let auth = cred.map(bearer);
    openai_turno(gw, Some(sessao), auth.as_deref(), texto, false).await
}

/// O mesmo turno pelo ramo `"stream": true` (`handle_streaming`), que monta
/// o proprio escopo dentro do `tokio::spawn`.
async fn openai_stream(gw: &Gateway, sessao: &str, cred: Option<&str>, texto: &str) -> String {
    let auth = cred.map(bearer);
    openai_turno(gw, Some(sessao), auth.as_deref(), texto, true).await
}

fn com_dono(state: &mut AppState) {
    state
        .allowlist
        .lock()
        .expect("allowlist")
        .claim_owner("dono-1343");
}

#[tokio::test]
async fn openai_sim_com_a_mesma_credencial_roda_uma_vez() {
    let gw = subir(com_dono).await;
    let s = "sess-openai-a";
    assert!(e_pedido(
        &openai(&gw, s, Some("cred-a"), "apaga o arquivo").await
    ));
    assert!(!e_pedido(&openai(&gw, s, Some("cred-a"), "sim").await));
    assert_eq!(gw.rodou(), 1);
    assert!(e_pedido(&openai(&gw, s, Some("cred-a"), "sim").await));
    assert_eq!(gw.rodou(), 1, "replay nao roda");
}

/// Mesma `X-Session-Id`, outra credencial: nao aprova — e encerra o pedido.
#[tokio::test]
async fn openai_outra_credencial_na_mesma_sessao_nao_aprova() {
    let gw = subir(com_dono).await;
    let s = "sess-openai-b";
    assert!(e_pedido(
        &openai(&gw, s, Some("cred-a"), "apaga o arquivo").await
    ));
    assert!(e_pedido(&openai(&gw, s, Some("cred-b"), "sim").await));
    assert_eq!(gw.rodou(), 0);
    assert!(e_pedido(&openai(&gw, s, Some("cred-a"), "sim").await));
    assert_eq!(gw.rodou(), 0);
}

/// #1343 A7: o ramo streaming tem o mesmo contrato — "sim" com a mesma
/// credencial roda uma vez, replay pausa.
#[tokio::test]
async fn openai_stream_sim_com_a_mesma_credencial_roda_uma_vez() {
    let gw = subir(com_dono).await;
    let s = "sess-openai-stream-a";
    let r1 = openai_stream(&gw, s, Some("cred-a"), "apaga o arquivo").await;
    assert!(e_pedido(&r1), "o pedido chega pelo SSE: {r1}");
    let r2 = openai_stream(&gw, s, Some("cred-a"), "sim").await;
    assert!(!e_pedido(&r2), "o sim aprovou: {r2}");
    assert_eq!(gw.rodou(), 1);
    assert!(e_pedido(
        &openai_stream(&gw, s, Some("cred-a"), "sim").await
    ));
    assert_eq!(gw.rodou(), 1, "replay nao roda");
}

/// #1343 A7: no streaming, outra credencial na mesma sessao nao aprova — e
/// encerra o pedido.
#[tokio::test]
async fn openai_stream_outra_credencial_na_mesma_sessao_nao_aprova() {
    let gw = subir(com_dono).await;
    let s = "sess-openai-stream-b";
    assert!(e_pedido(
        &openai_stream(&gw, s, Some("cred-a"), "apaga o arquivo").await
    ));
    assert!(e_pedido(
        &openai_stream(&gw, s, Some("cred-b"), "sim").await
    ));
    assert_eq!(gw.rodou(), 0);
    assert!(e_pedido(
        &openai_stream(&gw, s, Some("cred-a"), "sim").await
    ));
    assert_eq!(gw.rodou(), 0);
}

/// #1343 A7: os dois ramos montam o MESMO escopo — pausa pelo streaming e
/// "sim" sem streaming, com a mesma credencial, roda uma vez (e ao
/// contrario tambem).
#[tokio::test]
async fn openai_pausa_num_ramo_e_o_sim_no_outro_aprova() {
    let gw = subir(com_dono).await;
    let s = "sess-openai-misto";
    assert!(e_pedido(
        &openai_stream(&gw, s, Some("cred-a"), "apaga o arquivo").await
    ));
    assert!(!e_pedido(&openai(&gw, s, Some("cred-a"), "sim").await));
    assert_eq!(gw.rodou(), 1);

    assert!(e_pedido(
        &openai(&gw, s, Some("cred-a"), "apaga o arquivo").await
    ));
    assert!(!e_pedido(
        &openai_stream(&gw, s, Some("cred-a"), "sim").await
    ));
    assert_eq!(gw.rodou(), 2);
}

/// #1343 A8: sem `X-Session-Id` cada request e uma sessao nova (UUID do
/// servidor), entao o "sim" nao tem o que retomar — a pausa e terminal.
/// Quem quer aprovar manda a mesma `X-Session-Id` nos dois requests.
#[tokio::test]
async fn openai_sem_x_session_id_o_sim_nao_aprova() {
    let gw = subir(com_dono).await;
    let auth = bearer("cred-a");
    for stream in [false, true] {
        let r1 = openai_turno(&gw, None, Some(&auth), "apaga o arquivo", stream).await;
        assert!(e_pedido(&r1), "{r1}");
        let r2 = openai_turno(&gw, None, Some(&auth), "sim", stream).await;
        assert!(e_pedido(&r2), "sem sessao estavel o sim nao aprova: {r2}");
    }
    assert_eq!(gw.rodou(), 0);
}

/// #1343 A9: um `Authorization` que nao e UTF-8 (obs-text) continua sendo a
/// credencial dele. Antes ele caia na identidade de quem nao manda header
/// nenhum, e um cliente anonimo com a mesma `X-Session-Id` aprovava.
#[tokio::test]
async fn openai_authorization_nao_utf8_nao_vira_o_anonimo() {
    let gw = subir(com_dono).await;
    let s = "sess-openai-obs-text";
    let obs = b"Bearer \xff\xfe-cred".to_vec();
    assert!(e_pedido(
        &openai_turno(&gw, Some(s), Some(&obs), "apaga o arquivo", false).await
    ));
    assert!(e_pedido(
        &openai_turno(&gw, Some(s), None, "sim", false).await
    ));
    assert_eq!(gw.rodou(), 0, "o anonimo nao aprova o pedido do obs-text");

    // E o proprio dono do header obs-text aprova o dele.
    assert!(e_pedido(
        &openai_turno(&gw, Some(s), Some(&obs), "apaga o arquivo", false).await
    ));
    assert!(!e_pedido(
        &openai_turno(&gw, Some(s), Some(&obs), "sim", false).await
    ));
    assert_eq!(gw.rodou(), 1);
}

/// Sem dono na allowlist nao ha remetente do servidor: a pausa e terminal.
#[tokio::test]
async fn openai_sem_dono_a_pausa_e_terminal() {
    let gw = subir(|_| {}).await;
    let s = "sess-openai-c";
    assert!(e_pedido(
        &openai(&gw, s, Some("cred-a"), "apaga o arquivo").await
    ));
    assert!(e_pedido(&openai(&gw, s, Some("cred-a"), "sim").await));
    assert_eq!(gw.rodou(), 0);
}

/// Uma `X-Session-Id` escolhida pelo cliente apontando para uma sessao do
/// chat web nao consome o pedido pendente do web: o canal do escopo e fixo
/// por quem chama, nunca lido da sessao.
#[tokio::test]
async fn openai_com_session_id_de_outro_canal_nao_consome_o_pedido() {
    let gw = subir(com_dono).await;
    let (mut ws, sid) = conectar(&gw).await;
    assert!(e_pedido(&falar(&mut ws, "apaga o arquivo").await));

    assert!(e_pedido(&openai(&gw, &sid, Some("cred-a"), "sim").await));
    assert_eq!(gw.rodou(), 0);

    // O pedido do web ficou intacto: o "sim" na conexao certa roda.
    assert!(!e_pedido(&falar(&mut ws, "sim").await));
    assert_eq!(gw.rodou(), 1);
}

// ── Mobile ──────────────────────────────────────────────────────────────

fn com_jwt(state: &mut AppState) {
    use secrecy::SecretString;
    state.set_auth_config(Arc::new(garraia_config::AuthConfig {
        jwt_secret: SecretString::from("j".repeat(32)),
        refresh_hmac_secret: SecretString::from("r".repeat(32)),
        login_database_url: SecretString::from("postgres://x@localhost/x".to_string()),
        signup_database_url: SecretString::from("postgres://y@localhost/y".to_string()),
        app_database_url: None,
    }));
}

async fn mobile(gw: &Gateway, token: &str, texto: &str) -> String {
    let resp = reqwest::Client::new()
        .post(format!("http://{}/chat", gw.base))
        .bearer_auth(token)
        .json(&json!({"message": texto}))
        .send()
        .await
        .expect("post");
    assert!(resp.status().is_success(), "status {}", resp.status());
    let v: Value = resp.json().await.expect("json");
    v["reply"].as_str().expect("reply").to_string()
}

/// No mobile o remetente e o `sub` do JWT (que tambem deriva a sessao): o
/// "sim" do mesmo usuario roda uma vez; o de outro usuario cai em outra
/// sessao e nao toca no pedido.
#[tokio::test]
async fn mobile_sim_do_mesmo_sub_roda_e_outro_sub_nao_alcanca() {
    let mut tokens = (String::new(), String::new());
    let gw = subir(|st| {
        com_jwt(st);
        tokens = (
            garraia_gateway::mobile_auth::issue_jwt_pub(st, "user-a", "a@example.test")
                .expect("jwt a"),
            garraia_gateway::mobile_auth::issue_jwt_pub(st, "user-b", "b@example.test")
                .expect("jwt b"),
        );
    })
    .await;
    let (a, b) = tokens;

    assert!(e_pedido(&mobile(&gw, &a, "apaga o arquivo").await));
    assert!(e_pedido(&mobile(&gw, &b, "sim").await));
    assert_eq!(gw.rodou(), 0);
    assert!(!e_pedido(&mobile(&gw, &a, "sim").await));
    assert_eq!(gw.rodou(), 1);
}

// ── Canais de chat (bootstrap) ──────────────────────────────────────────

/// Um turno de canal pelo MESMO caminho dos 11 `bootstrap/<canal>.rs`:
/// `crate::approval_scope::com_escopo(exec, canal, sessao, user_id)` e o
/// runtime. A sessao e da conversa (num grupo, a mesma para todo membro);
/// o remetente e quem falou. O historico vai vazio: com escopo ele nao
/// decide aprovacao nenhuma.
async fn turno_de_canal(rt: &AgentRuntime, sessao: &str, usuario: &str, texto: &str) -> String {
    let exec = garraia_gateway::approval_scope::com_escopo(
        garraia_agents::exec_context::ExecContext::default(),
        "discord",
        sessao,
        usuario,
    );
    rt.process_message_with_agent_config(
        sessao,
        texto,
        &[],
        None,
        Some(usuario),
        None,
        None,
        None,
        None,
        &exec,
    )
    .await
    .expect("turno")
}

fn runtime_de_roteiro() -> (AgentRuntime, Arc<AtomicUsize>) {
    let rt = AgentRuntime::new();
    let rodou = Arc::new(AtomicUsize::new(0));
    rt.register_tool(Box::new(ApagaArquivo {
        rodou: Arc::clone(&rodou),
    }));
    rt.register_provider(Arc::new(Roteiro));
    (rt, rodou)
}

/// O gemeo positivo: na conversa do grupo, quem pediu aprova o proprio
/// pedido, uma vez.
#[tokio::test]
async fn canal_o_mesmo_usuario_aprova_o_proprio_pedido_uma_vez() {
    let (rt, rodou) = runtime_de_roteiro();
    let g = "discord-grupo-1343";
    assert!(e_pedido(
        &turno_de_canal(&rt, g, "user-a", "apaga o arquivo").await
    ));
    assert!(!e_pedido(&turno_de_canal(&rt, g, "user-a", "sim").await));
    assert_eq!(rodou.load(Ordering::SeqCst), 1);
    assert!(e_pedido(&turno_de_canal(&rt, g, "user-a", "sim").await));
    assert_eq!(rodou.load(Ordering::SeqCst), 1, "replay nao roda");
}

/// #1343 A6, o gemeo negativo: dois usuarios na MESMA sessao de canal nao
/// aprovam o pedido um do outro — e o "sim" do outro encerra o pedido.
#[tokio::test]
async fn canal_dois_usuarios_na_mesma_sessao_nao_aprovam_o_pedido_um_do_outro() {
    let (rt, rodou) = runtime_de_roteiro();
    let g = "discord-grupo-1343";

    assert!(e_pedido(
        &turno_de_canal(&rt, g, "user-a", "apaga o arquivo").await
    ));
    assert!(e_pedido(&turno_de_canal(&rt, g, "user-b", "sim").await));
    assert_eq!(
        rodou.load(Ordering::SeqCst),
        0,
        "B nao aprova o pedido de A"
    );
    assert!(e_pedido(&turno_de_canal(&rt, g, "user-a", "sim").await));
    assert_eq!(
        rodou.load(Ordering::SeqCst),
        0,
        "o sim de B encerrou o pedido"
    );

    assert!(e_pedido(
        &turno_de_canal(&rt, g, "user-b", "apaga o arquivo").await
    ));
    assert!(e_pedido(&turno_de_canal(&rt, g, "user-a", "sim").await));
    assert_eq!(
        rodou.load(Ordering::SeqCst),
        0,
        "A nao aprova o pedido de B"
    );
}
