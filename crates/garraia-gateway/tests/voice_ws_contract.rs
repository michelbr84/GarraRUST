//! #1754: contrato do `/ws` para o Voice Chat, num gateway de verdade.
//!
//! Sobe o `build_router` inteiro numa porta efemera e conversa por WebSocket
//! como o navegador faz. Os providers sao de roteiro (nada de rede, nada pago)
//! e gravam o que receberam, para o teste afirmar o que o modelo viu:
//!
//! * a sessao e o historico sobrevivem a troca de provider e de modelo entre
//!   turnos de voz, sem reconectar;
//! * todo frame do turno leva o `turn_id`, e o `message` diz quem respondeu;
//! * interromper durante a geracao persiste a fala do usuario e separa o que
//!   foi falado do que foi gerado e nao ouvido;
//! * interromper a reproducao de um turno ja concluido vira uma nota de
//!   contexto para o proximo turno, montada pelo servidor;
//! * `turn_id` invalido nao abre turno;
//! * o fallback configurado pelo operador aparece no frame, nunca em silencio.

use std::pin::Pin;
use std::sync::{Arc, LazyLock, Mutex as StdMutex};
use std::time::Duration;

use async_trait::async_trait;
use futures::{SinkExt, Stream, StreamExt};
use garraia_agents::{
    AgentRuntime, ChatRole, ContentBlock, LlmProvider, LlmRequest, LlmResponse, MessagePart,
    StreamEvent,
};
use garraia_channels::ChannelRegistry;
use garraia_config::AppConfig;
use garraia_gateway::admin::store::AdminStore;
use garraia_gateway::push_channels::PushChannelStates;
use garraia_gateway::router::build_router;
use garraia_gateway::state::AppState;
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;

/// `AppState::new` le e grava no diretorio de config: o binario inteiro
/// aponta para um tempdir, e sem autoprovisionar MCP.
static TEST_ENV: LazyLock<tempfile::TempDir> = LazyLock::new(|| {
    let dir = tempfile::tempdir().expect("temp config dir");
    // SAFETY: o `LazyLock` serializa isto contra as outras threads de teste
    // deste binario; todo teste chama `subir()` primeiro.
    unsafe {
        std::env::set_var("GARRAIA_CONFIG_DIR", dir.path());
        std::env::set_var(
            garraia_gateway::mcp::McpPersistenceService::DISABLE_AUTOPROVISION_ENV,
            "1",
        );
    }
    dir
});

/// O que um provider recebeu num turno.
#[derive(Debug, Clone)]
struct Pedido {
    provider: String,
    model: String,
    max_tokens: Option<u32>,
    /// `(papel, texto)` de cada mensagem do historico enviado.
    mensagens: Vec<(String, String)>,
}

type Registro = Arc<StdMutex<Vec<Pedido>>>;

fn texto(part: &MessagePart) -> String {
    match part {
        MessagePart::Text(t) => t.clone(),
        MessagePart::Parts(blocks) => blocks
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(""),
    }
}

fn gravar(registro: &Registro, provider: &str, request: &LlmRequest) {
    registro.lock().unwrap().push(Pedido {
        provider: provider.to_string(),
        model: request.model.clone(),
        max_tokens: request.max_tokens,
        mensagens: request
            .messages
            .iter()
            .map(|m| {
                let papel = match m.role {
                    ChatRole::System => "system",
                    ChatRole::User => "user",
                    ChatRole::Assistant => "assistant",
                    ChatRole::Tool => "tool",
                };
                (papel.to_string(), texto(&m.content))
            })
            .collect(),
    });
}

type Fluxo = Pin<Box<dyn Stream<Item = garraia_common::Result<StreamEvent>> + Send>>;

/// Provider de roteiro: responde `pedacos` em streaming, com `pausa` antes do
/// ultimo (para o teste conseguir interromper no meio).
struct Roteiro {
    id: &'static str,
    pedacos: &'static [&'static str],
    pausa: Duration,
    registro: Registro,
}

#[async_trait]
impl LlmProvider for Roteiro {
    fn provider_id(&self) -> &str {
        self.id
    }

    async fn complete(&self, request: &LlmRequest) -> garraia_common::Result<LlmResponse> {
        gravar(&self.registro, self.id, request);
        Ok(LlmResponse {
            content: vec![ContentBlock::Text {
                text: self.pedacos.concat(),
            }],
            model: request.model.clone(),
            stop_reason: None,
            usage: None,
        })
    }

    async fn stream_complete(&self, request: &LlmRequest) -> garraia_common::Result<Fluxo> {
        gravar(&self.registro, self.id, request);
        let pausa = self.pausa;
        let total = self.pedacos.len();
        let pedacos = self.pedacos.iter().map(|p| p.to_string()).enumerate();
        Ok(Box::pin(
            futures::stream::iter(pedacos)
                .then(move |(i, p)| async move {
                    if i + 1 == total && !pausa.is_zero() {
                        tokio::time::sleep(pausa).await;
                    }
                    Ok(StreamEvent::TextDelta(p))
                })
                .chain(futures::stream::iter(vec![Ok(StreamEvent::MessageStop)])),
        ))
    }

    async fn health_check(&self) -> garraia_common::Result<bool> {
        Ok(true)
    }
}

/// Primario com a credencial recusada (401): o runtime cai no fallback que o
/// operador configurou.
struct ChaveMorta;

#[async_trait]
impl LlmProvider for ChaveMorta {
    fn provider_id(&self) -> &str {
        "primario"
    }
    async fn complete(&self, _request: &LlmRequest) -> garraia_common::Result<LlmResponse> {
        Err(garraia_common::Error::Agent(
            "openai API error: status=401 Unauthorized, body=User not found.".to_string(),
        ))
    }
    async fn health_check(&self) -> garraia_common::Result<bool> {
        Ok(true)
    }
}

struct Gateway {
    base: String,
    registro: Registro,
}

const UMA: &[&str] = &["Primeira frase. ", "Segunda frase."];
const OUTRA: &[&str] = &["Resposta do segundo."];

async fn subir_com(configurar: impl FnOnce(&AgentRuntime, &Registro)) -> Gateway {
    LazyLock::force(&TEST_ENV);
    let registro: Registro = Arc::default();
    let runtime = AgentRuntime::new();
    configurar(&runtime, &registro);
    let mut config = AppConfig::default();
    config.memory.enabled = false;
    config.mcp.clear();
    let state = AppState::new(config, Arc::new(runtime), ChannelRegistry::new());
    let router = build_router(
        Arc::new(state),
        PushChannelStates::empty(),
        Arc::new(Mutex::new(AdminStore::in_memory().expect("admin store"))),
        Arc::new(vec![0u8; 32]),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await;
    });
    Gateway {
        base: format!("127.0.0.1:{}", addr.port()),
        registro,
    }
}

async fn subir() -> Gateway {
    subir_com(|runtime, registro| {
        runtime.register_provider(Arc::new(Roteiro {
            id: "primeiro",
            pedacos: UMA,
            pausa: Duration::ZERO,
            registro: Arc::clone(registro),
        }));
        runtime.register_provider(Arc::new(Roteiro {
            id: "segundo",
            pedacos: OUTRA,
            pausa: Duration::ZERO,
            registro: Arc::clone(registro),
        }));
        runtime.register_provider(Arc::new(Roteiro {
            id: "lento",
            pedacos: &["Olá, ", "isto nunca chega."],
            pausa: Duration::from_secs(30),
            registro: Arc::clone(registro),
        }));
    })
    .await
}

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Abre o socket com `init` e devolve o `session_id` do `connected`.
async fn conectar(gw: &Gateway) -> (Ws, String) {
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{}/ws", gw.base))
        .await
        .expect("ws connect");
    ws.send(Message::Text(json!({"type":"init"}).to_string().into()))
        .await
        .expect("init");
    let conectado = proximo(&mut ws, |f| f["type"] == "connected").await;
    let sid = conectado["session_id"]
        .as_str()
        .expect("session_id")
        .to_string();
    (ws, sid)
}

async fn enviar(ws: &mut Ws, frame: Value) {
    ws.send(Message::Text(frame.to_string().into()))
        .await
        .expect("enviar");
}

/// Proximo frame JSON que satisfaz `quer`, ignorando os demais (logs etc.).
async fn proximo(ws: &mut Ws, quer: impl Fn(&Value) -> bool) -> Value {
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(20), ws.next())
            .await
            .expect("frame a tempo")
            .expect("socket aberto")
            .expect("frame valido");
        if let Message::Text(t) = frame
            && let Ok(v) = serde_json::from_str::<Value>(&t)
            && quer(&v)
        {
            return v;
        }
    }
}

/// Todos os frames do turno `turn_id` ate o `message`/`error`/`stopped` dele.
async fn turno(ws: &mut Ws, turn_id: &str) -> Vec<Value> {
    let mut frames = Vec::new();
    loop {
        let f = proximo(ws, |f| {
            matches!(
                f["type"].as_str(),
                Some("delta" | "message" | "error" | "stopped" | "tool_started" | "tool_finished")
            )
        })
        .await;
        let fim = matches!(f["type"].as_str(), Some("message" | "error" | "stopped"));
        assert_eq!(f["turn_id"], turn_id, "frame de outro turno: {f}");
        frames.push(f);
        if fim {
            return frames;
        }
    }
}

fn voz(turn_id: &str, provider: &str, model: &str, content: &str) -> Value {
    json!({"content":content, "provider":provider, "model":model, "stream":true, "voice":true, "turn_id":turn_id})
}

fn pedidos(gw: &Gateway) -> Vec<Pedido> {
    gw.registro.lock().unwrap().clone()
}

#[tokio::test]
async fn sessao_e_historico_sobrevivem_a_troca_de_provider_e_modelo() {
    let gw = subir().await;
    let (mut ws, sid) = conectar(&gw).await;

    enviar(
        &mut ws,
        voz("v-1", "primeiro", "modelo-a", "Qual é a capital?"),
    )
    .await;
    let primeiro = turno(&mut ws, "v-1").await;
    let fim = primeiro.last().unwrap();
    assert_eq!(fim["type"], "message", "{primeiro:?}");
    assert_eq!(fim["content"], "Primeira frase. Segunda frase.");
    assert_eq!(fim["provider"], "primeiro");
    assert_eq!(fim["fallback"], false);
    assert!(
        primeiro.iter().filter(|f| f["type"] == "delta").count() >= 2,
        "deltas em streaming: {primeiro:?}"
    );

    // Troca provider e modelo no meio da chamada, no mesmo socket.
    enviar(&mut ws, voz("v-2", "segundo", "modelo-b", "E a população?")).await;
    let segundo = turno(&mut ws, "v-2").await;
    let fim = segundo.last().unwrap();
    assert_eq!(fim["provider"], "segundo");
    assert_eq!(fim["content"], "Resposta do segundo.");

    for f in primeiro.iter().chain(&segundo) {
        assert_eq!(f["session_id"], sid.as_str(), "mesma sessao: {f}");
    }
    let p = pedidos(&gw);
    assert_eq!(p.len(), 2);
    assert_eq!(
        (p[0].provider.as_str(), p[0].model.as_str()),
        ("primeiro", "modelo-a")
    );
    assert_eq!(
        (p[1].provider.as_str(), p[1].model.as_str()),
        ("segundo", "modelo-b")
    );
    assert_eq!(
        p[1].max_tokens,
        Some(1024),
        "turno de voz tem teto de tokens"
    );
    // O segundo modelo viu a conversa inteira do primeiro.
    let historico = &p[1].mensagens;
    assert!(
        historico
            .iter()
            .any(|(r, t)| r == "user" && t == "Qual é a capital?")
    );
    assert!(
        historico
            .iter()
            .any(|(r, t)| r == "assistant" && t == "Primeira frase. Segunda frase.")
    );
    assert!(
        historico
            .iter()
            .any(|(r, t)| r == "system" && t.contains("live voice conversation")),
        "dica de voz no historico: {historico:?}"
    );
}

#[tokio::test]
async fn interromper_durante_a_geracao_separa_o_falado_do_nao_ouvido() {
    let gw = subir().await;
    let (mut ws, _sid) = conectar(&gw).await;

    enviar(&mut ws, voz("v-1", "lento", "m", "Me conte uma história")).await;
    let primeiro_delta = proximo(&mut ws, |f| f["type"] == "delta").await;
    assert_eq!(primeiro_delta["turn_id"], "v-1");
    // O cliente tocou 3 caracteres ("Olá") quando o usuario falou por cima.
    enviar(
        &mut ws,
        json!({"type":"stop","turn_id":"v-1","heard_chars":3}),
    )
    .await;
    let parou = proximo(&mut ws, |f| f["type"] == "stopped").await;
    assert_eq!(parou["turn_id"], "v-1");

    enviar(&mut ws, voz("v-2", "segundo", "m", "Mais curto, por favor")).await;
    let fim = turno(&mut ws, "v-2").await;
    assert_eq!(fim.last().unwrap()["type"], "message");

    let p = pedidos(&gw);
    let historico = &p.last().unwrap().mensagens;
    assert!(
        historico
            .iter()
            .any(|(r, t)| r == "user" && t == "Me conte uma história"),
        "a fala interrompida do usuario fica no contexto: {historico:?}"
    );
    let interrompida = historico
        .iter()
        .find(|(r, _)| r == "assistant")
        .map(|(_, t)| t.clone())
        .expect("resposta interrompida no historico");
    assert!(
        interrompida.contains("Spoken aloud:]\nOlá\n[Generated but not heard:]\n, "),
        "{interrompida}"
    );
    assert!(!interrompida.contains("isto nunca chega"));
}

#[tokio::test]
async fn interromper_a_reproducao_vira_nota_no_proximo_turno() {
    let gw = subir().await;
    let (mut ws, _sid) = conectar(&gw).await;

    enviar(&mut ws, voz("v-1", "primeiro", "m", "Explique")).await;
    assert_eq!(
        turno(&mut ws, "v-1").await.last().unwrap()["type"],
        "message"
    );
    // O turno ja terminou; o usuario falou por cima da reproducao depois de
    // ouvir 16 de 30 caracteres.
    enviar(
        &mut ws,
        json!({"type":"stop","turn_id":"v-1","heard_chars":16}),
    )
    .await;
    let parou = proximo(&mut ws, |f| f["type"] == "stopped").await;
    assert_eq!(parou["turn_id"], "v-1");

    enviar(&mut ws, voz("v-2", "segundo", "m", "Espera")).await;
    turno(&mut ws, "v-2").await;
    let p = pedidos(&gw);
    let nota = p
        .last()
        .unwrap()
        .mensagens
        .iter()
        .find(|(r, t)| r == "system" && t.starts_with("[Voice note"))
        .map(|(_, t)| t.clone())
        .expect("nota de interrupcao no turno seguinte");
    assert!(nota.contains("first 16 of 30 characters"), "{nota}");
    assert!(nota.ends_with("Primeira frase. "), "{nota}");
    // A nota vale uma vez.
    enviar(&mut ws, voz("v-3", "segundo", "m", "Ok")).await;
    turno(&mut ws, "v-3").await;
    let p = pedidos(&gw);
    assert!(
        !p.last()
            .unwrap()
            .mensagens
            .iter()
            .any(|(r, t)| r == "system" && t.starts_with("[Voice note"))
    );
}

#[tokio::test]
async fn turn_id_invalido_nao_abre_turno() {
    let gw = subir().await;
    let (mut ws, _sid) = conectar(&gw).await;
    enviar(&mut ws, voz("tem espaço", "primeiro", "m", "oi")).await;
    let erro = proximo(&mut ws, |f| f["type"] == "error").await;
    assert_eq!(erro["code"], "invalid_turn_id");
    // Um turno valido depois prova que o socket segue vivo.
    enviar(&mut ws, voz("v-ok", "primeiro", "m", "oi")).await;
    turno(&mut ws, "v-ok").await;
    assert_eq!(
        pedidos(&gw).len(),
        1,
        "o turno invalido nao chamou provider"
    );
}

#[tokio::test]
async fn fallback_do_operador_e_nomeado_no_frame_de_voz() {
    let gw = subir_com(|runtime, registro| {
        runtime.register_provider(Arc::new(ChaveMorta));
        runtime.register_provider(Arc::new(Roteiro {
            id: "reserva",
            pedacos: OUTRA,
            pausa: Duration::ZERO,
            registro: Arc::clone(registro),
        }));
        runtime.set_fallback_providers(vec!["reserva".to_string()]);
    })
    .await;
    let (mut ws, _sid) = conectar(&gw).await;
    enviar(&mut ws, voz("v-1", "primario", "m", "oi")).await;
    let fim = turno(&mut ws, "v-1").await;
    let fim = fim.last().unwrap();
    assert_eq!(fim["type"], "message", "{fim}");
    assert_eq!(fim["provider"], "reserva");
    assert_eq!(fim["fallback"], true);
}

/// O cliente de texto antigo (sem `turn_id`, sem `voice`) recebe os frames de
/// sempre: nada de `turn_id`, `provider` ou `fallback` no `message`.
#[tokio::test]
async fn cliente_de_texto_antigo_nao_ve_campos_novos() {
    let gw = subir().await;
    let (mut ws, _sid) = conectar(&gw).await;
    enviar(&mut ws, json!({"content":"oi","provider":"primeiro"})).await;
    let msg = proximo(&mut ws, |f| f["type"] == "message").await;
    assert!(msg.get("turn_id").is_none(), "{msg}");
    assert!(msg.get("provider").is_none(), "{msg}");
    assert!(msg.get("fallback").is_none(), "{msg}");
    let p = pedidos(&gw);
    assert!(
        !p[0].mensagens.iter().any(|(r, _)| r == "system"),
        "sem dica de voz fora do Voice Chat"
    );
}
