//! #1541 (defeito 1): `POST /v1/chat/completions` sem `model` no corpo nao
//! pode mandar o literal `"gpt-4"` ao backend.
//!
//! `openai_api.rs` fazia `body.model.unwrap_or_else(|| "gpt-4")` e entregava
//! isso ao runtime como se o cliente tivesse pedido. Contra um llama-server
//! ou um Ollama local a resposta e 404 `model 'gpt-4' not found` — ou seja, a
//! API OpenAI-compat era inutilizavel exatamente para o cliente que nao manda
//! `model` porque quer o default do servidor. O A2A ja tinha tomado a decisao
//! oposta (`a2a.rs:254`).
//!
//! O que se prova aqui e o que o **provider recebe**, nao o que o handler
//! escreve num log: um provider de teste grava o `request.model` que chegou.
//!
//! O contrato do runtime e que `LlmRequest.model` VAZIO significa "provider,
//! use o seu": `OpenAiProvider::build_request` (`openai.rs:181`) e
//! `OllamaProvider::build_request_body` substituem a string vazia pelo
//! `configured_model` deles. Era exatamente esse sinal que o
//! `unwrap_or_else(|| "gpt-4")` destruia — o handler respondia pelo cliente
//! uma pergunta que so o provider sabe responder.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::body::Body;
use axum::http::Request;
use garraia_agents::{AgentRuntime, LlmProvider, LlmRequest, LlmResponse, providers::ContentBlock};
use garraia_channels::ChannelRegistry;
use garraia_config::AppConfig;
use garraia_db::SessionStore;
use garraia_gateway::openai_api::build_openai_router;
use garraia_gateway::state::AppState;
use serde_json::json;
use tower::ServiceExt; // `oneshot`

/// Serializa os testes deste binario: `config_dir_de_teste` escreve no
/// ambiente do processo, que e global. Mesmo contrato de ordem do
/// `auto_router_registra_o_modo.rs` — guard antes do runtime tokio, runtime
/// inteiro dentro da janela do guard.
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Provider que nao fala com ninguem e so guarda o `model` que recebeu.
#[derive(Debug)]
struct ProviderQueAnota {
    modelo_configurado: String,
    visto: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl LlmProvider for ProviderQueAnota {
    fn provider_id(&self) -> &str {
        "anota"
    }

    fn configured_model(&self) -> Option<&str> {
        Some(&self.modelo_configurado)
    }

    async fn complete(&self, request: &LlmRequest) -> garraia_common::Result<LlmResponse> {
        self.visto.lock().unwrap().push(request.model.clone());
        Ok(LlmResponse {
            content: vec![ContentBlock::Text {
                text: "ok".to_string(),
            }],
            model: request.model.clone(),
            usage: None,
            stop_reason: Some("end_turn".to_string()),
        })
    }

    async fn health_check(&self) -> garraia_common::Result<bool> {
        Ok(true)
    }
}

/// SAFETY (chamador): o chamador segura `ENV_LOCK` durante todo o teste.
fn config_dir_de_teste() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp config dir");
    unsafe {
        std::env::set_var("GARRAIA_CONFIG_DIR", dir.path());
        std::env::set_var(
            garraia_gateway::mcp::McpPersistenceService::DISABLE_AUTOPROVISION_ENV,
            "1",
        );
    }
    dir
}

fn estado_com_provider(modelo_configurado: &str) -> (Arc<AppState>, Arc<Mutex<Vec<String>>>) {
    let mut config = AppConfig::default();
    config.memory.enabled = false;

    let visto = Arc::new(Mutex::new(Vec::new()));
    let runtime = AgentRuntime::new();
    runtime.register_provider(Arc::new(ProviderQueAnota {
        modelo_configurado: modelo_configurado.to_string(),
        visto: Arc::clone(&visto),
    }));

    let mut state = AppState::new(config, Arc::new(runtime), ChannelRegistry::new());
    state.set_session_store(Arc::new(tokio::sync::Mutex::new(
        SessionStore::in_memory().expect("store em memoria"),
    )));
    (Arc::new(state), visto)
}

async fn post_chat(state: Arc<AppState>, sessao: &str, corpo: serde_json::Value) -> String {
    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .header("x-session-id", sessao)
        .body(Body::from(corpo.to_string()))
        .expect("request builder");
    let resp = build_openai_router(state)
        .oneshot(req)
        .await
        .expect("o router responde sem fazer bind");
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("corpo legivel");
    String::from_utf8_lossy(&bytes).into_owned()
}

/// O caso da issue: corpo sem `model`. O provider tem de ver o model DELE.
#[test]
fn sem_model_no_corpo_o_provider_recebe_o_proprio_default() {
    let _serializa = ENV_LOCK
        .lock()
        .unwrap_or_else(|envenenado| envenenado.into_inner());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let _dir = config_dir_de_teste();

    let (corpo, visto) = runtime.block_on(async {
        let (state, visto) = estado_com_provider("glm53-flash");
        let corpo = post_chat(
            state,
            "sessao-sem-model",
            json!({ "messages": [{ "role": "user", "content": "oi" }] }),
        )
        .await;
        (corpo, visto)
    });

    let vistos = visto.lock().unwrap().clone();
    assert!(!vistos.is_empty(), "o provider nao foi chamado: {corpo}");
    assert!(
        !vistos.iter().any(|m| m == "gpt-4"),
        "o gateway mandou o literal `gpt-4` ao backend (#1541): {vistos:?}"
    );
    assert!(
        vistos.iter().all(|m| m.is_empty()),
        "sem pedido do cliente o provider tem de receber o sentinel vazio \
         (`build_request` troca pelo `configured_model` dele): {vistos:?}"
    );

    // O campo `model` do response e obrigatorio no protocolo: ecoa o model
    // que de fato atendeu, e nao um nome inventado.
    let json: serde_json::Value = serde_json::from_str(&corpo).expect("response e JSON");
    assert_eq!(json["model"], "glm53-flash", "{corpo}");
}

/// O caminho que ja funcionava nao pode mudar: `model` pedido vai inteiro.
#[test]
fn com_model_no_corpo_o_pedido_do_cliente_chega_intacto() {
    let _serializa = ENV_LOCK
        .lock()
        .unwrap_or_else(|envenenado| envenenado.into_inner());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let _dir = config_dir_de_teste();

    let (corpo, visto) = runtime.block_on(async {
        let (state, visto) = estado_com_provider("glm53-flash");
        let corpo = post_chat(
            state,
            "sessao-com-model",
            json!({
                "model": "qwen3-coder",
                "messages": [{ "role": "user", "content": "oi" }],
            }),
        )
        .await;
        (corpo, visto)
    });

    let vistos = visto.lock().unwrap().clone();
    assert!(!vistos.is_empty(), "o provider nao foi chamado: {corpo}");
    assert!(
        vistos.iter().all(|m| m == "qwen3-coder"),
        "o model pedido pelo cliente nao chegou ao provider: {vistos:?}"
    );
}
