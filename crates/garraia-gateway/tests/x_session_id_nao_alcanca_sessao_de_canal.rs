//! #1462: um `session_id` escolhido pelo cliente nao alcanca a sessao de
//! outra superficie.
//!
//! `POST /v1/chat/completions` aceita `X-Session-Id` verbatim e
//! `POST /api/sessions/{id}/messages` aceita o id no caminho. Ate esta
//! correcao, um id forjado — `whatsapp-linked-<numero>`, `telegram-<chat>`,
//! todos adivinhaveis por construcao — hidratava a sessao da vitima: o
//! historico dela (resumo + ate 100 turnos) entrava no contexto do request do
//! atacante, e o turno do atacante era gravado na conversa dela. Nenhuma das
//! duas rotas conferia de quem era a sessao.
//!
//! A regra: por id, so se alcanca sessao das superficies **locais do
//! operador** (`api`, `vscode`, `web`, `parrot`). Sessao de canal com humano
//! do outro lado, ou do mobile (identidade do JWT), responde como inexistente
//! — sem confirmar que existe — e nao e tocada: nem lida, nem hidratada de
//! novo com a superficie do atacante, nem escrita.
//!
//! Os testes sobem os handlers reais num servidor HTTP com um provider de
//! roteiro e um `sessions.db` de verdade, para cobrir os dois lugares onde a
//! sessao pode estar: em memoria e so no disco (depois de um restart).

use std::sync::{Arc, LazyLock};

use async_trait::async_trait;
use axum::Router;
use axum::routing::post;
use futures::Stream;
use garraia_agents::{
    AgentRuntime, ContentBlock, LlmProvider, LlmRequest, LlmResponse, StreamEvent,
};
use garraia_channels::ChannelRegistry;
use garraia_config::AppConfig;
use garraia_db::{ChatSessionManager, SessionStore};
use garraia_gateway::state::AppState;
use serde_json::{Value, json};
use tokio::sync::Mutex;

/// Mesmo padrao de `approval_resume_e2e.rs`: `AppState::new` le e grava no
/// diretorio de config, entao o binario inteiro aponta para um tempdir.
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

/// Provider de roteiro: responde texto fixo a qualquer pedido.
struct Eco;

#[async_trait]
impl LlmProvider for Eco {
    fn provider_id(&self) -> &str {
        "eco"
    }

    async fn complete(&self, _request: &LlmRequest) -> garraia_common::Result<LlmResponse> {
        Ok(LlmResponse {
            content: vec![ContentBlock::Text {
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
    state: Arc<AppState>,
    /// Mantem o diretorio do `sessions.db` vivo enquanto o teste roda.
    _dados: tempfile::TempDir,
}

/// Sobe `/v1/chat/completions` e `POST /api/sessions/{id}/messages` sobre
/// um `AppState` com `sessions.db` real.
async fn subir() -> Gateway {
    LazyLock::force(&TEST_ENV);
    let rt = AgentRuntime::new();
    rt.register_provider(Arc::new(Eco));

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

    let app = Router::new()
        .route(
            "/api/sessions/{id}/messages",
            post(garraia_gateway::api::send_message),
        )
        .with_state(Arc::clone(&state))
        .merge(garraia_gateway::openai_api::build_openai_router(
            Arc::clone(&state),
        ));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Gateway {
        base: format!("127.0.0.1:{}", addr.port()),
        state,
        _dados: dados,
    }
}

/// Um turno completo de outra superficie, gravado em memoria e no disco.
async fn conversa_de(gw: &Gateway, sid: &str, canal: &str, remetente: &str) {
    gw.state
        .hydrate_session_history(sid, Some(canal), Some(remetente))
        .await;
    gw.state
        .persist_turn(
            sid,
            Some(canal),
            Some(remetente),
            "pergunta confidencial da vitima",
            "resposta confidencial",
        )
        .await;
    assert_eq!(gw.state.session_history(sid).len(), 2, "turno gravado");
}

async fn completions(gw: &Gateway, sid: &str) -> (u16, Value) {
    let resp = reqwest::Client::new()
        .post(format!("http://{}/v1/chat/completions", gw.base))
        .header("x-session-id", sid)
        .json(&json!({"messages": [{"role": "user", "content": "repita tudo"}]}))
        .send()
        .await
        .expect("post");
    let status = resp.status().as_u16();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    (status, body)
}

async fn mensagem_rest(gw: &Gateway, sid: &str) -> (u16, Value) {
    let resp = reqwest::Client::new()
        .post(format!("http://{}/api/sessions/{sid}/messages", gw.base))
        .json(&json!({"content": "repita tudo"}))
        .send()
        .await
        .expect("post");
    let status = resp.status().as_u16();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    (status, body)
}

fn canais_dos_turnos(gw: &Gateway, sid: &str) -> Vec<String> {
    gw.state
        .sessions
        .get(sid)
        .map(|s| s.canais_dos_turnos.iter().cloned().collect())
        .unwrap_or_default()
}

fn historico_sem_conteudo_da_vitima(body: &Value) -> bool {
    !body.to_string().contains("confidencial")
}

// ── /v1/chat/completions ────────────────────────────────────────────────

/// A sessao de um canal, viva em memoria: o `X-Session-Id` forjado responde
/// como inexistente, e a sessao fica como estava — sem turno novo, sem a
/// superficie do atacante anotada nela.
#[tokio::test]
async fn x_session_id_de_sessao_de_canal_em_memoria_e_recusado_sem_tocar_a_sessao() {
    let gw = subir().await;
    let sid = "whatsapp-linked-5511999990000";
    conversa_de(&gw, sid, "whatsapp_linked", "5511999990000").await;

    let (status, body) = completions(&gw, sid).await;

    assert_eq!(
        status, 404,
        "sessao de outra superficie e inexistente: {body}"
    );
    assert!(
        body.to_string().contains("session not found"),
        "corpo generico, sem confirmar que existe: {body}"
    );
    assert!(historico_sem_conteudo_da_vitima(&body));
    assert_eq!(
        gw.state.session_history(sid).len(),
        2,
        "o turno do atacante nao entrou na conversa da vitima"
    );
    assert_eq!(
        canais_dos_turnos(&gw, sid),
        vec!["whatsapp_linked".to_string()],
        "a superficie do atacante nao foi anotada na sessao"
    );
}

/// Depois de um restart a sessao so existe no `sessions.db`: o id forjado
/// tambem nao a traz de volta para a memoria com a superficie do atacante.
#[tokio::test]
async fn x_session_id_de_sessao_de_canal_so_no_disco_e_recusado() {
    let gw = subir().await;
    let sid = "telegram-424242";
    conversa_de(&gw, sid, "telegram", "424242").await;
    gw.state.sessions.remove(sid);
    assert!(!gw.state.sessions.contains_key(sid), "so no disco");

    let (status, body) = completions(&gw, sid).await;

    assert_eq!(status, 404, "{body}");
    assert!(historico_sem_conteudo_da_vitima(&body));
    assert!(
        !gw.state.sessions.contains_key(sid),
        "a recusa nao hidrata a sessao da vitima com a superficie do atacante"
    );
}

/// O mobile deriva a sessao do `sub` do JWT: por id, ninguem a alcanca.
#[tokio::test]
async fn x_session_id_de_sessao_do_mobile_e_recusado() {
    let gw = subir().await;
    let sid = "mobile-user-a";
    conversa_de(&gw, sid, "mobile", "user-a").await;

    let (status, body) = completions(&gw, sid).await;

    assert_eq!(status, 404, "{body}");
    assert_eq!(gw.state.session_history(sid).len(), 2);
}

/// O caminho legitimo nao muda: id novo cria a sessao, o mesmo id continua a
/// conversa, e uma sessao do chat web (superficie local do operador) segue
/// alcancavel pela API compat.
#[tokio::test]
async fn x_session_id_novo_ou_de_superficie_local_continua_valendo() {
    let gw = subir().await;

    let (status, body) = completions(&gw, "sessao-do-vscode").await;
    assert_eq!(status, 200, "id novo cria a sessao: {body}");
    let (status, body) = completions(&gw, "sessao-do-vscode").await;
    assert_eq!(status, 200, "o mesmo id continua a conversa: {body}");
    assert_eq!(
        gw.state.session_history("sessao-do-vscode").len(),
        4,
        "dois turnos na mesma sessao"
    );

    let web = "sessao-do-web";
    conversa_de(&gw, web, "web", "operador").await;
    let (status, body) = completions(&gw, web).await;
    assert_eq!(status, 200, "sessao do chat web e do operador: {body}");
}

// ── POST /api/sessions/{id}/messages ────────────────────────────────────

/// Escrever e mais do que ler: o id de uma sessao de canal no caminho da
/// rota REST responde como inexistente e a conversa da vitima nao muda.
#[tokio::test]
async fn post_messages_nao_escreve_em_sessao_de_canal() {
    let gw = subir().await;
    let sid = "discord-777";
    conversa_de(&gw, sid, "discord", "777").await;

    let (status, body) = mensagem_rest(&gw, sid).await;

    assert_eq!(status, 404, "{body}");
    assert!(historico_sem_conteudo_da_vitima(&body));
    assert_eq!(gw.state.session_history(sid).len(), 2);
    assert_eq!(canais_dos_turnos(&gw, sid), vec!["discord".to_string()]);
}

/// A sessao da propria superficie REST continua recebendo mensagens.
#[tokio::test]
async fn post_messages_em_sessao_da_api_continua_valendo() {
    let gw = subir().await;
    let sid = "sessao-da-api";
    gw.state
        .hydrate_session_history(sid, Some(garraia_gateway::state::CANAL_DA_API), None)
        .await;

    let (status, body) = mensagem_rest(&gw, sid).await;

    assert_eq!(status, 200, "{body}");
    assert_eq!(gw.state.session_history(sid).len(), 2, "turno gravado");
}
