//! #1541 (defeito 4): `stream=true` nao pode transformar erro do provider em
//! resposta em branco.
//!
//! Observado em producao (0.4.6): com o backend recusando o pedido, o
//! `POST /v1/chat/completions` com `stream: true` devolvia 200, o chunk
//! inicial de role, **nenhum delta**, um `finish_reason: "stop"` e `[DONE]`.
//! Pelo SSE, isso e indistinguivel de um turno em que o modelo escolheu nao
//! dizer nada — e era assim que o cliente (SDK da OpenAI, Continue, Cline)
//! lia: sucesso com texto vazio. A causa era o `if let Ok(..)` na task do
//! turno, que descartava o `Err` em silencio.
//!
//! O teste sobe o handler real com um provider que falha nos dois caminhos
//! (streaming e batch) e le o corpo SSE inteiro.

use std::sync::{Arc, LazyLock};

use async_trait::async_trait;
use futures::Stream;
use garraia_agents::{AgentRuntime, LlmProvider, LlmRequest, LlmResponse, StreamEvent};
use garraia_channels::ChannelRegistry;
use garraia_config::AppConfig;
use garraia_db::{ChatSessionManager, SessionStore};
use garraia_gateway::state::AppState;
use serde_json::json;
use tokio::sync::Mutex;

/// Mesmo padrao dos outros testes de handler real: `AppState::new` le e grava
/// no diretorio de config, entao o binario inteiro aponta para um tempdir.
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

/// A mensagem exata que o backend local devolve quando o `model` nao existe
/// — o caso concreto da #1541.
const MOTIVO: &str = "model 'gpt-4' not found";

/// Provider que recusa tudo. Erro de `Agent` e nao-retryable de proposito: o
/// runtime nao gasta retry nem procura fallback, e o turno termina em `Err`,
/// que e exatamente o desfecho que o SSE precisa saber contar.
struct SempreRecusa;

#[async_trait]
impl LlmProvider for SempreRecusa {
    fn provider_id(&self) -> &str {
        "sempre-recusa"
    }

    async fn complete(&self, _request: &LlmRequest) -> garraia_common::Result<LlmResponse> {
        Err(garraia_common::Error::Agent(MOTIVO.to_string()))
    }

    async fn stream_complete(
        &self,
        _request: &LlmRequest,
    ) -> garraia_common::Result<
        std::pin::Pin<Box<dyn Stream<Item = garraia_common::Result<StreamEvent>> + Send>>,
    > {
        Err(garraia_common::Error::Agent(MOTIVO.to_string()))
    }

    async fn health_check(&self) -> garraia_common::Result<bool> {
        Ok(false)
    }
}

/// Provider que responde normalmente, para provar que o caminho feliz
/// continua fechando com `finish_reason: "stop"` e sem envelope de erro.
struct Eco;

#[async_trait]
impl LlmProvider for Eco {
    fn provider_id(&self) -> &str {
        "eco"
    }

    async fn complete(&self, _request: &LlmRequest) -> garraia_common::Result<LlmResponse> {
        Ok(LlmResponse {
            content: vec![garraia_agents::ContentBlock::Text {
                text: "eco".to_string(),
            }],
            model: "m".to_string(),
            stop_reason: None,
            usage: None,
        })
    }

    async fn stream_complete(
        &self,
        _request: &LlmRequest,
    ) -> garraia_common::Result<
        std::pin::Pin<Box<dyn Stream<Item = garraia_common::Result<StreamEvent>> + Send>>,
    > {
        Ok(Box::pin(futures::stream::iter(vec![
            Ok(StreamEvent::TextDelta("eco".to_string())),
            Ok(StreamEvent::MessageStop),
        ])))
    }

    async fn health_check(&self) -> garraia_common::Result<bool> {
        Ok(true)
    }
}

struct Gateway {
    base: String,
    /// Mantem o diretorio do `sessions.db` vivo enquanto o teste roda.
    _dados: tempfile::TempDir,
}

async fn subir(provider: Arc<dyn LlmProvider>) -> Gateway {
    LazyLock::force(&TEST_ENV);
    let rt = AgentRuntime::new();
    rt.register_provider(provider);

    let mut config = AppConfig::default();
    config.memory.enabled = false;
    let mut state = AppState::new(config, Arc::new(rt), ChannelRegistry::new());
    state.allowlist = Arc::new(std::sync::Mutex::new(
        garraia_security::Allowlist::restricted(Vec::<String>::new()),
    ));
    let dados = tempfile::tempdir().expect("tempdir dos dados");
    let store = SessionStore::open(&dados.path().join("sessions.db")).expect("abrir store");
    let store = Arc::new(Mutex::new(store));
    state.set_session_store(Arc::clone(&store));
    state.set_chat_session_manager(Arc::new(ChatSessionManager::new(store)));
    let state = Arc::new(state);

    let app = garraia_gateway::openai_api::build_openai_router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Gateway {
        base: format!("127.0.0.1:{}", addr.port()),
        _dados: dados,
    }
}

/// Le o corpo SSE inteiro do `stream: true`.
async fn corpo_sse(gw: &Gateway) -> (u16, String) {
    let resp = reqwest::Client::new()
        .post(format!("http://{}/v1/chat/completions", gw.base))
        .json(&json!({
            "messages": [{"role": "user", "content": "oi"}],
            "stream": true
        }))
        .send()
        .await
        .expect("post");
    let status = resp.status().as_u16();
    let corpo = resp.text().await.expect("corpo");
    (status, corpo)
}

#[tokio::test]
async fn erro_do_provider_vira_evento_de_erro_no_sse() {
    let gw = subir(Arc::new(SempreRecusa)).await;
    let (status, corpo) = corpo_sse(&gw).await;

    // O SSE ja comecou quando o erro aparece, entao o status **tem** de ser
    // 200: a correcao esta no corpo, nao no codigo HTTP.
    assert_eq!(status, 200, "SSE ja iniciado responde 200");

    assert!(
        corpo.contains("\"error\""),
        "o stream precisa carregar o envelope de erro; corpo:\n{corpo}"
    );
    assert!(
        corpo.contains(MOTIVO),
        "o motivo do provider precisa chegar ao cliente; corpo:\n{corpo}"
    );
    assert!(
        !corpo.contains("\"stop\""),
        "turno que falhou nao pode fechar como `finish_reason: stop`; corpo:\n{corpo}"
    );
    assert!(
        corpo.contains("[DONE]"),
        "o sentinela continua fechando o stream; corpo:\n{corpo}"
    );
}

#[tokio::test]
async fn turno_bem_sucedido_continua_fechando_em_stop() {
    let gw = subir(Arc::new(Eco)).await;
    let (status, corpo) = corpo_sse(&gw).await;

    assert_eq!(status, 200);
    assert!(corpo.contains("eco"), "delta de conteudo; corpo:\n{corpo}");
    assert!(
        corpo.contains("\"stop\""),
        "caminho feliz fecha em stop; corpo:\n{corpo}"
    );
    assert!(
        !corpo.contains("\"error\""),
        "sucesso nao carrega envelope de erro; corpo:\n{corpo}"
    );
    assert!(corpo.contains("[DONE]"));
}
