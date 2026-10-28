//! #1733: `/v1/chat/completions` diz qual provider/modelo respondeu.
//!
//! Complemento do que a #1745 fez para `POST /api/sessions/{id}/messages`. O
//! cenario e o mesmo dos dois testes: primario com a chave morta (401) e um
//! fallback que atende. O corpo, os headers por turno e — no streaming — o
//! chunk final tem de nomear quem serviu de fato, nao o rotulo pedido.

use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use axum::http::StatusCode;
use futures::Stream;
use garraia_agents::{
    AgentRuntime, ContentBlock, LlmProvider, LlmRequest, LlmResponse, StreamEvent,
};
use garraia_channels::ChannelRegistry;
use garraia_config::AppConfig;

use super::*;
use crate::state::AppState;

/// Primario cuja credencial e recusada (401): o runtime cai no fallback.
///
/// `stream_complete` fica no default do trait ("nao suporta streaming"), um
/// erro que `stream_complete_with_fallback` nao classifica como transitorio,
/// de transporte nem de credencial. Por isso o braco de streaming reroteia
/// para o caminho batch (`complete_reportando_provider`), que e quem reporta o
/// provider que de fato serviu — a propriedade que o chunk final le das stats.
struct ProviderComChaveMorta;

#[async_trait]
impl LlmProvider for ProviderComChaveMorta {
    fn provider_id(&self) -> &str {
        "primario"
    }
    fn configured_model(&self) -> Option<&str> {
        Some("primario-1")
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

/// Fallback que responde — no batch e no streaming. Provider "anota", modelo
/// "anota-1", como o fixture de `api.rs`.
struct FallbackQueResponde;

const RESPOSTA: &str = "resposta do fallback";

#[async_trait]
impl LlmProvider for FallbackQueResponde {
    fn provider_id(&self) -> &str {
        "anota"
    }
    fn configured_model(&self) -> Option<&str> {
        Some("anota-1")
    }
    async fn complete(&self, _request: &LlmRequest) -> garraia_common::Result<LlmResponse> {
        Ok(LlmResponse {
            content: vec![ContentBlock::Text {
                text: RESPOSTA.to_string(),
            }],
            model: "anota-1".to_string(),
            usage: None,
            stop_reason: Some("end_turn".to_string()),
        })
    }
    async fn stream_complete(
        &self,
        _request: &LlmRequest,
    ) -> garraia_common::Result<
        Pin<Box<dyn Stream<Item = garraia_common::Result<StreamEvent>> + Send>>,
    > {
        Ok(Box::pin(futures::stream::iter(vec![
            Ok(StreamEvent::TextDelta(RESPOSTA.to_string())),
            Ok(StreamEvent::MessageStop),
        ])))
    }
    async fn health_check(&self) -> garraia_common::Result<bool> {
        Ok(true)
    }
}

/// `SharedState` com o par primario-morto + fallback-que-responde, config num
/// tempdir (`with_config_dir`) para nao tocar a config real da maquina. Sem
/// session store: o handler OpenAI nao exige um (persist/hydrate saem cedo sem
/// store), e sem `X-Session-Id` a sessao e um UUID novo por request.
fn estado() -> (tempfile::TempDir, SharedState) {
    let dir = tempfile::tempdir().expect("tempdir");
    let runtime = AgentRuntime::new();
    runtime.register_provider(Arc::new(FallbackQueResponde));
    runtime.register_provider(Arc::new(ProviderComChaveMorta));
    assert!(
        runtime.set_default_provider_id("primario"),
        "primario registrado"
    );
    runtime.set_fallback_providers(vec!["anota".to_string()]);
    let st = AppState::with_config_dir(
        AppConfig::default(),
        Arc::new(runtime),
        ChannelRegistry::new(),
        &dir.path().join("config"),
    );
    (dir, Arc::new(st))
}

fn pedido(stream: bool) -> ChatCompletionRequest {
    // `model` ausente de proposito: o rotulo ecoado cai no default do provider
    // primario ("primario-1"), que diverge de quem responde ("anota-1").
    serde_json::from_value(serde_json::json!({
        "messages": [{"role": "user", "content": "oi"}],
        "stream": stream,
    }))
    .expect("request valido")
}

/// Nao-streaming: `model`/`provider` do corpo e os headers nomeiam o fallback.
#[tokio::test]
async fn nao_streaming_ecoa_quem_respondeu() {
    let (_dir, st) = estado();

    let resp = chat_completions(State(st), HeaderMap::new(), Json(pedido(false))).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let header = |n: &str| {
        resp.headers()
            .get(n)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    assert_eq!(header("X-AI-Provider").as_deref(), Some("anota"));
    assert_eq!(header("X-AI-Model").as_deref(), Some("anota-1"));

    let bytes = axum::body::to_bytes(resp.into_body(), 256 * 1024)
        .await
        .expect("corpo");
    let corpo: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(corpo["model"], "anota-1", "{corpo}");
    assert_eq!(corpo["provider"], "anota", "{corpo}");
}

/// Streaming: os chunks ja emitidos nao mudam (o provider so se sabe no fim),
/// mas o chunk final — o do `finishReason: "stop"` — carrega `model`/`provider`
/// de quem respondeu. O primeiro chunk mantem o rotulo pedido, de proposito.
#[tokio::test]
async fn streaming_chunk_final_nomeia_quem_respondeu() {
    let (_dir, st) = estado();

    let resp = chat_completions(State(st), HeaderMap::new(), Json(pedido(true))).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        axum::body::to_bytes(resp.into_body(), 1024 * 1024),
    )
    .await
    .expect("stream nao trava")
    .expect("corpo SSE");
    let texto = String::from_utf8(bytes.to_vec()).expect("utf8");

    // Payloads `data:` do SSE (com ou sem espaco apos os dois pontos).
    let payloads: Vec<String> = texto
        .lines()
        .filter(|l| l.starts_with("data:"))
        .map(|l| l.trim_start_matches("data:").trim().to_string())
        .collect();

    assert_eq!(
        payloads.last().map(String::as_str),
        Some("[DONE]"),
        "o stream termina em [DONE]: {texto}"
    );

    let chunks: Vec<serde_json::Value> = payloads
        .iter()
        .filter(|p| p.as_str() != "[DONE]")
        .map(|p| serde_json::from_str(p).expect("chunk json"))
        .collect();
    assert!(chunks.len() >= 2, "ao menos role + final: {texto}");

    // Primeiro chunk: rotulo pedido ("primario-1") e sem `provider`.
    let primeiro = &chunks[0];
    assert_eq!(primeiro["model"], "primario-1", "{texto}");
    assert!(
        primeiro.get("provider").is_none(),
        "chunk inicial nao afirma provider: {texto}"
    );

    // Ultimo chunk antes do [DONE]: desfecho + quem respondeu de fato.
    let ultimo = chunks.last().expect("chunk final");
    assert_eq!(ultimo["choices"][0]["finishReason"], "stop", "{texto}");
    assert_eq!(ultimo["model"], "anota-1", "{texto}");
    assert_eq!(ultimo["provider"], "anota", "{texto}");
}
