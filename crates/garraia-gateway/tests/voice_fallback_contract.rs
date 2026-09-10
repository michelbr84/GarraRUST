// crates/garraia-gateway/tests/voice_fallback_contract.rs
//! Contrato do fallback de TTS em POST /api/tts.
//! Pinnamos exatamente a forma da resposta: sem fallback (default) um erro de
//! voz NAO pode quebrar a rota — devolve 200 com audio nulo e flag fallback;
//! com ?fallback=false o erro propaga como 500. Isso trava o contrato entre
//! gateway e clientes de voz contra regressoes acidentais.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use garraia_agents::AgentRuntime;
use garraia_channels::ChannelRegistry;
use garraia_config::AppConfig;
use garraia_gateway::admin::store::AdminStore;
use garraia_gateway::push_channels::PushChannelStates;
use garraia_gateway::router::build_router;
use garraia_gateway::state::AppState;
use tokio::sync::Mutex;
use tower::ServiceExt;

/// Mock de sintetizador que SEMPRE falha: força o caminho de erro do handler.
struct TtsQueFalha;

#[async_trait::async_trait]
impl garraia_voice::TtsSynthesizer for TtsQueFalha {
    async fn synthesize_bytes(
        &self,
        _text: &str,
        _language: &str,
    ) -> Result<Vec<u8>, garraia_voice::VoiceError> {
        Err(garraia_voice::VoiceError::Tts(
            "servidor de teste fora".to_string(),
        ))
    }
}

/// Monta o router com o gate global desligado (AppConfig::default nao tem
/// api_key) e o voice_client injetado ANTES de envolver o estado em Arc.
fn router_com_tts_que_falha() -> axum::Router {
    let mut st = AppState::new(
        AppConfig::default(),
        Arc::new(AgentRuntime::new()),
        ChannelRegistry::new(),
    );
    // injetamos antes do Arc porque o campo so e mutavel no estado possuido
    st.voice_client = Some(Arc::new(TtsQueFalha));
    let admin_store = Arc::new(Mutex::new(
        AdminStore::in_memory().expect("in-memory admin store"),
    ));
    build_router(
        Arc::new(st),
        PushChannelStates::empty(),
        admin_store,
        Arc::new(vec![0u8; 32]),
    )
}

/// Sem ConnectInfo o rate limiter estoura 500 antes de chegar ao handler;
/// toda request pinnada precisa da extensao de endereco.
fn request_base(uri: &str) -> Request<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"text": "ola garra", "language": "pt"}"#))
        .expect("request valida");
    req.extensions_mut().insert(axum::extract::ConnectInfo(
        std::net::SocketAddr::from(([127, 0, 0, 1], 40404])),
    ));
    req
}

#[tokio::test]
async fn tts_falha_sem_query_usa_fallback_com_200() {
    let app = router_com_tts_que_falha();
    let resp = app
        .oneshot(request_base("/api/tts"))
        .await
        .expect("resposta do router");

    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("corpo legivel");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("json valido");

    assert_eq!(json["audio"], serde_json::Value::Null);
    assert_eq!(json["text"], "ola garra");
    // O sufixo do erro vem do Display do VoiceError (thiserror); o contrato
    // que importa aqui e o prefixo e a flag.
    let erro = json["error"].as_str().expect("erro presente");
    assert!(erro.starts_with("TTS failed: "), "erro: {erro}");
    assert_eq!(json["fallback"], true);
}

#[tokio::test]
async fn tts_falha_com_fallback_false_propaga_500() {
    let app = router_com_tts_que_falha();
    let resp = app
        .oneshot(request_base("/api/tts?fallback=false"))
        .await
        .expect("resposta do router");

    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("corpo legivel");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("json valido");

    let erro = json["error"].as_str().expect("erro presente");
    assert!(erro.starts_with("TTS synthesis failed"), "erro: {erro}");
}
