//! Contracts for the Voice Chat routes mounted inside the real gateway.
//! No model inference, paid service, microphone, environment edits or external
//! network are needed: only ephemeral loopback HTTP servers are used.

use std::sync::{Arc, LazyLock, Mutex as StdMutex};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use garraia_agents::{AgentRuntime, LlmProvider, LlmRequest, LlmResponse};
use garraia_channels::ChannelRegistry;
use garraia_config::{AppConfig, LlmProviderConfig};
use garraia_gateway::{
    admin::store::AdminStore, push_channels::PushChannelStates, router::build_router,
    state::AppState,
};
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tower::ServiceExt;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, method, path, query_param},
};

/// `AppState::new` le e grava no diretorio de config: o binario de teste
/// inteiro aponta para um tempdir, e sem autoprovisionar MCP.
static TEST_ENV: LazyLock<tempfile::TempDir> = LazyLock::new(|| {
    let dir = tempfile::tempdir().expect("temp config dir");
    // SAFETY: o `LazyLock` serializa isto contra as outras threads de teste
    // deste binario; todo teste passa por `router_with` primeiro.
    unsafe {
        std::env::set_var("GARRAIA_CONFIG_DIR", dir.path());
        std::env::set_var(
            garraia_gateway::mcp::McpPersistenceService::DISABLE_AUTOPROVISION_ENV,
            "1",
        );
    }
    dir
});

const API_KEY: &str = "voice-contract-gateway-key";
const PROVIDER_KEY: &str = "voice-contract-upstream-secret";

struct RegisteredProvider;

#[async_trait::async_trait]
impl LlmProvider for RegisteredProvider {
    fn provider_id(&self) -> &str {
        "voice-router"
    }

    async fn complete(&self, _: &LlmRequest) -> garraia_common::Result<LlmResponse> {
        panic!("audio routes must not invoke a text model as fallback")
    }

    async fn health_check(&self) -> garraia_common::Result<bool> {
        Ok(true)
    }
}

#[derive(Default)]
struct LocalSpeech {
    seen: StdMutex<Vec<(String, String)>>,
}

#[async_trait::async_trait]
impl garraia_voice::TtsSynthesizer for LocalSpeech {
    async fn synthesize_bytes(
        &self,
        text: &str,
        language: &str,
    ) -> Result<Vec<u8>, garraia_voice::VoiceError> {
        self.seen
            .lock()
            .unwrap()
            .push((text.into(), language.into()));
        Ok(wav())
    }
}

fn config() -> AppConfig {
    let mut config = AppConfig::default();
    config.gateway.api_key = Some(API_KEY.into());
    config.memory.enabled = false;
    config.voice.tts_provider = "chatterbox".into();
    config.voice.language = "pt".into();
    config
}

fn remote_config(server: &MockServer) -> AppConfig {
    let mut config = config();
    config.llm.insert(
        "voice-router".into(),
        LlmProviderConfig {
            provider: "openrouter".into(),
            model: Some("example/text".into()),
            api_key: Some(PROVIDER_KEY.into()),
            base_url: Some(format!("{}/v1", server.uri())),
            extra: Default::default(),
        },
    );
    config
}

fn router(
    config: AppConfig,
    local: Option<Arc<LocalSpeech>>,
    stt: Option<garraia_voice::WhisperClient>,
) -> Router {
    router_with(
        config,
        local.map(|l| l as Arc<dyn garraia_voice::TtsSynthesizer>),
        stt,
        |_| {},
    )
}

fn router_with(
    config: AppConfig,
    local: Option<Arc<dyn garraia_voice::TtsSynthesizer>>,
    stt: Option<garraia_voice::WhisperClient>,
    setup: impl FnOnce(&AgentRuntime),
) -> Router {
    LazyLock::force(&TEST_ENV);
    let runtime = AgentRuntime::new();
    runtime.register_provider(Arc::new(RegisteredProvider));
    setup(&runtime);
    let mut state = AppState::new(config, Arc::new(runtime), ChannelRegistry::new());
    if let Some(local) = local {
        state.voice_client = Some(local);
    }
    state.stt_client = stt.map(Arc::new);
    build_router(
        Arc::new(state),
        PushChannelStates::empty(),
        Arc::new(Mutex::new(AdminStore::in_memory().unwrap())),
        Arc::new(vec![0; 32]),
    )
}

fn request(method: &str, uri: &str, content_type: &str, bytes: Vec<u8>) -> Request<Body> {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "127.0.0.1:3888")
        .header("authorization", format!("Bearer {API_KEY}"))
        .header("content-type", content_type)
        .body(Body::from(bytes))
        .unwrap();
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
            [127, 0, 0, 1],
            40404,
        ))));
    req
}

fn synth(body: Value) -> Request<Body> {
    request(
        "POST",
        "/api/voice/synthesize",
        "application/json",
        serde_json::to_vec(&body).unwrap(),
    )
}

fn remote_speech() -> Value {
    json!({"provider":"voice-router","model":"example/speech","voice":"speaker-a","text":"Olá, mundo."})
}

/// 200 ms de tom (energia de fala) em PCM16 mono 16 kHz. Silencio puro nao
/// serve mais: a porta de energia do servidor nao chama STT para ele.
fn wav() -> Vec<u8> {
    wav_from((0..3200).map(|i| ((i as f64 * 0.0864).sin() * 8000.0) as i16))
}

fn silent_wav() -> Vec<u8> {
    wav_from(std::iter::repeat_n(0_i16, 16_000))
}

fn wav_from(samples: impl Iterator<Item = i16>) -> Vec<u8> {
    let pcm: Vec<u8> = samples.flat_map(i16::to_le_bytes).collect();
    let mut bytes = Vec::with_capacity(44 + pcm.len());
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&16000_u32.to_le_bytes());
    bytes.extend_from_slice(&32000_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&pcm);
    bytes
}

fn transcription(fields: &[(&str, &str)], audio: &[u8]) -> Request<Body> {
    let mut bytes = Vec::new();
    for (key, value) in fields {
        bytes.extend_from_slice(format!("--voice-contract\r\nContent-Disposition: form-data; name=\"{key}\"\r\n\r\n{value}\r\n").as_bytes());
    }
    bytes.extend_from_slice(b"--voice-contract\r\nContent-Disposition: form-data; name=\"file\"; filename=\"private.wav\"\r\nContent-Type: audio/wav\r\n\r\n");
    bytes.extend_from_slice(audio);
    bytes.extend_from_slice(b"\r\n--voice-contract--\r\n");
    request(
        "POST",
        "/api/voice/transcribe",
        "multipart/form-data; boundary=voice-contract",
        bytes,
    )
}

async fn call(router: Router, request: Request<Body>) -> (StatusCode, HeaderMap, Vec<u8>) {
    let response = router.oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 20 * 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    (status, headers, bytes)
}

fn model(id: &str, input: &str, output: &str) -> Value {
    json!({"id":id,"name":id,"architecture":{"input_modalities":[input],"output_modalities":[output]},"pricing":{"prompt":"0.000004","completion":"0"},"supported_voices":["speaker-a","speaker-b"]})
}

async fn mock_catalog(server: &MockServer, models: Vec<Value>, count: u64) {
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(query_param("output_modalities", "all"))
        .and(header("authorization", format!("Bearer {PROVIDER_KEY}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":models})))
        .expect(count)
        .mount(server)
        .await;
}

#[tokio::test]
async fn every_voice_route_requires_gateway_credentials_before_body_or_origin() {
    let app = router(config(), None, None);
    let requests = [
        request("GET", "/api/voice/models", "application/json", vec![]),
        synth(remote_speech()),
        transcription(&[("provider", "local"), ("model", "configured")], &wav()),
    ];
    for mut req in requests {
        req.headers_mut().remove("authorization");
        req.headers_mut()
            .insert("origin", "https://untrusted.example".parse().unwrap());
        let (status, _, _) = call(app.clone(), req).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn authenticated_browser_cannot_trigger_cross_origin_inference() {
    let local = Arc::new(LocalSpeech::default());
    let app = router(config(), Some(local.clone()), None);
    for mut req in [
        synth(json!({"provider":"local","model":"configured","text":"Olá"})),
        transcription(&[("provider", "local"), ("model", "configured")], &wav()),
    ] {
        req.headers_mut()
            .insert("origin", "https://untrusted.example".parse().unwrap());
        let (status, _, _) = call(app.clone(), req).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    assert!(local.seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn default_local_installation_rejects_dns_rebinding_but_accepts_its_console() {
    let local = Arc::new(LocalSpeech::default());
    let mut config = config();
    config.gateway.api_key = None;
    let app = router(config, Some(local.clone()), None);
    for (origin, host, expected) in [
        (
            "http://attacker.example:3888",
            "attacker.example:3888",
            StatusCode::FORBIDDEN,
        ),
        ("http://127.0.0.1:3888", "127.0.0.1:3888", StatusCode::OK),
    ] {
        let mut req = synth(json!({"provider":"local","model":"configured","text":"Olá"}));
        req.headers_mut().remove("authorization");
        req.headers_mut().insert("origin", origin.parse().unwrap());
        req.headers_mut().insert("host", host.parse().unwrap());
        let (status, _, _) = call(app.clone(), req).await;
        assert_eq!(status, expected);
    }
    assert_eq!(local.seen.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn browser_cannot_supply_destinations_credentials_or_routing_policy() {
    let server = MockServer::start().await;
    let app = router(remote_config(&server), None, None);
    for key in ["endpoint", "base_url", "api_key", "provider_policy"] {
        let mut body = remote_speech();
        body[key] = json!("https://untrusted.example/steal");
        assert_eq!(
            call(app.clone(), synth(body)).await.0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    let mut body = remote_speech();
    body["provider"] = json!("https://untrusted.example/steal");
    assert_eq!(call(app, synth(body)).await.0, StatusCode::BAD_REQUEST);
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn discovery_uses_full_catalog_caches_it_and_does_not_claim_native_s2s() {
    let server = MockServer::start().await;
    mock_catalog(
        &server,
        vec![
            model("example/speech", "text", "speech"),
            model("example/transcribe", "audio", "transcription"),
            model("example/converse", "audio", "audio"),
            model("example/music", "text", "audio"),
        ],
        1,
    )
    .await;
    let app = router(remote_config(&server), None, None);
    for _ in 0..2 {
        let (status, _, bytes) = call(
            app.clone(),
            request("GET", "/api/voice/models", "application/json", vec![]),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["tts"][1]["id"], "example/speech");
        assert_eq!(body["tts"][1]["voices"], json!(["speaker-a", "speaker-b"]));
        assert_eq!(body["stt"][1]["id"], "example/transcribe");
        assert_eq!(body["speech_to_speech"].as_array().unwrap().len(), 1);
        assert_eq!(body["speech_to_speech"][0]["available"], false);
        assert!(!String::from_utf8_lossy(&bytes).contains(PROVIDER_KEY));
    }
}

#[tokio::test]
async fn incompatible_model_or_unadvertised_voice_never_triggers_synthesis() {
    let server = MockServer::start().await;
    mock_catalog(
        &server,
        vec![
            model("example/speech", "text", "speech"),
            model("example/transcribe", "audio", "transcription"),
        ],
        1,
    )
    .await;
    let app = router(remote_config(&server), None, None);
    for (field, value) in [
        ("model", "example/transcribe"),
        ("voice", "unadvertised"),
        ("model", "missing"),
    ] {
        let mut body = remote_speech();
        body[field] = json!(value);
        assert_eq!(
            call(app.clone(), synth(body)).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn capability_check_requires_the_input_modality_as_well_as_output() {
    let server = MockServer::start().await;
    mock_catalog(
        &server,
        vec![
            model("example/speech", "image", "speech"),
            model("example/transcribe", "text", "transcription"),
        ],
        1,
    )
    .await;
    let app = router(remote_config(&server), None, None);
    assert_eq!(
        call(app.clone(), synth(remote_speech())).await.0,
        StatusCode::BAD_REQUEST
    );
    let req = transcription(
        &[
            ("provider", "voice-router"),
            ("model", "example/transcribe"),
        ],
        &wav(),
    );
    assert_eq!(call(app, req).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn provider_errors_are_sanitized_and_never_retried() {
    let server = MockServer::start().await;
    mock_catalog(&server, vec![model("example/speech", "text", "speech")], 1).await;
    Mock::given(method("POST"))
        .and(path("/v1/audio/speech"))
        .respond_with(ResponseTemplate::new(500).set_body_string(format!(
            "{PROVIDER_KEY}: private transcript and upstream URL"
        )))
        .expect(1)
        .mount(&server)
        .await;
    let (status, _, body) = call(
        router(remote_config(&server), None, None),
        synth(remote_speech()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    let body = String::from_utf8(body).unwrap();
    assert!(!body.contains(PROVIDER_KEY));
    assert!(!body.contains("private transcript"));
    assert!(!body.contains(&server.uri()));
}

#[tokio::test]
async fn paid_provider_redirect_is_not_followed_with_credentials() {
    let server = MockServer::start().await;
    let destination = MockServer::start().await;
    mock_catalog(&server, vec![model("example/speech", "text", "speech")], 1).await;
    Mock::given(method("POST"))
        .and(path("/v1/audio/speech"))
        .respond_with(ResponseTemplate::new(307).insert_header("location", destination.uri()))
        .expect(1)
        .mount(&server)
        .await;
    assert_eq!(
        call(
            router(remote_config(&server), None, None),
            synth(remote_speech())
        )
        .await
        .0,
        StatusCode::BAD_GATEWAY
    );
    assert!(destination.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn unsupported_provider_restriction_fails_before_network() {
    let server = MockServer::start().await;
    let mut config = remote_config(&server);
    config.llm.get_mut("voice-router").unwrap().extra.insert(
        "voice_provider_policy".into(),
        json!({"only":["allowed-provider"]}),
    );
    assert_eq!(
        call(router(config, None, None), synth(remote_speech()))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn oversize_or_invalid_uploads_fail_without_contacting_provider() {
    let server = MockServer::start().await;
    let app = router(remote_config(&server), None, None);
    let mut body = remote_speech();
    body["text"] = json!("a".repeat(4097));
    assert_eq!(
        call(app.clone(), synth(body)).await.0,
        StatusCode::BAD_REQUEST
    );
    let fields = [
        ("provider", "voice-router"),
        ("model", "example/transcribe"),
    ];
    assert_eq!(
        call(app.clone(), transcription(&fields, b"not audio"))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let mut oversized = wav();
    oversized.resize(1024 * 1024 + 1, 0);
    let status = call(app, transcription(&fields, &oversized)).await.0;
    assert!(matches!(
        status,
        StatusCode::BAD_REQUEST | StatusCode::PAYLOAD_TOO_LARGE
    ));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn local_language_selection_does_not_mutate_the_next_turn_default() {
    let local = Arc::new(LocalSpeech::default());
    let app = router(config(), Some(local.clone()), None);
    let (status, headers, _) = call(
        app.clone(),
        synth(json!({"provider":"local","model":"configured","text":"Hello","language":"en"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "no-store");
    assert_eq!(
        call(
            app,
            synth(json!({"provider":"local","model":"configured","text":"Olá"}))
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        *local.seen.lock().unwrap(),
        vec![("Hello".into(), "en".into()), ("Olá".into(), "pt".into())]
    );
}

#[tokio::test]
async fn local_stt_receives_selected_language_and_real_wav_without_remote_fallback() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/inference"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"text":"hello"})))
        .expect(1)
        .mount(&server)
        .await;
    let client = garraia_voice::WhisperClient::new(&server.uri(), "pt");
    let (status, _, bytes) = call(
        router(config(), None, Some(client)),
        transcription(
            &[
                ("provider", "local"),
                ("model", "configured"),
                ("language", "en"),
            ],
            &wav(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap()["text"],
        "hello"
    );
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let body = String::from_utf8_lossy(&requests[0].body);
    assert!(body.contains("name=\"language\"\r\n\r\nen\r\n"));
    assert!(requests[0].body.windows(4).any(|chunk| chunk == b"RIFF"));
}

// ─── #1754: catalogo por fixture local, formatos, uso, silencio e capacidade ──

const FIXTURE: &str = include_str!("fixtures/openrouter_voice_models_2026-11-02.json");

async fn mock_fixture_catalog(server: &MockServer) {
    let body: Value = serde_json::from_str(FIXTURE).unwrap();
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(query_param("output_modalities", "all"))
        .and(header("authorization", format!("Bearer {PROVIDER_KEY}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

fn ids(list: &Value) -> Vec<String> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|m| {
            format!(
                "{}:{}",
                m["provider"].as_str().unwrap(),
                m["id"].as_str().unwrap()
            )
        })
        .collect()
}

/// Catalogo real (recortado) filtrado so por `architecture.*_modalities`:
/// musica (texto+imagem -> texto+audio) e texto puro ficam de fora; audio ->
/// audio aparece como speech-to-speech NAO executavel. Preco vem como o
/// provedor informa, sem unidade inventada; o horario da consulta vai junto, e
/// o fallback de conversa configurado pelo operador e anunciado.
#[tokio::test]
async fn catalog_from_local_fixture_reports_capabilities_prices_time_and_fallback() {
    let server = MockServer::start().await;
    mock_fixture_catalog(&server).await;
    let mut config = remote_config(&server);
    config.voice.tts_provider = "hibiki".into();
    let app = router_with(config, None, None, |runtime| {
        runtime.set_fallback_providers(vec!["voice-router".into()]);
    });
    let (status, _, bytes) = call(
        app,
        request("GET", "/api/voice/models", "application/json", vec![]),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        ids(&body["tts"]),
        [
            "local:configured",
            "voice-router:heygen/heygen-voice-1-turbo",
            "voice-router:heygen/heygen-voice-1-turbo:free",
            "voice-router:mistralai/voxtral-mini-tts-3-0",
            "voice-router:google/gemini-3.8-flash-lite-tts",
        ]
    );
    assert_eq!(
        ids(&body["stt"]),
        [
            "local:configured",
            "voice-router:mistralai/voxtral-mini-transcribe-3-0"
        ]
    );
    assert_eq!(
        ids(&body["speech_to_speech"]),
        ["voice-router:openai/gpt-audio-mini"]
    );
    assert_eq!(body["speech_to_speech"][0]["available"], false);
    // Locais: indisponibilidade explicita, com motivo.
    assert_eq!(body["stt"][0]["available"], false);
    assert!(
        body["stt"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("--with-voice")
    );
    assert_eq!(body["tts"][0]["available"], false);
    assert!(
        body["tts"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("Hibiki")
    );
    // Preco: o valor do catalogo, unidade desconhecida (nunca "por token").
    let voxtral = &body["tts"][3];
    assert!(
        voxtral["prices"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["field"] == "prompt" && p["usd"] == "0.000013" && p["unit"].is_null())
    );
    assert_eq!(voxtral["voices"].as_array().unwrap().len(), 6);
    // Horario da consulta (UTC, RFC 3339) e validade.
    let catalog = &body["catalogs"][0];
    assert_eq!(catalog["provider"], "voice-router");
    assert!(catalog["fetched_at"].as_str().unwrap().ends_with('Z'));
    assert_eq!(catalog["ttl_seconds"], 300);
    assert_eq!(body["llm_fallback_providers"], json!(["voice-router"]));
    assert!(
        body["limitations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l.as_str().unwrap().contains("Fallback de conversa"))
    );
    assert!(!String::from_utf8_lossy(&bytes).contains(PROVIDER_KEY));
}

/// Um enunciado sem energia de fala nao chega a nenhum STT — nem local, nem
/// pago. O cliente volta a escutar.
#[tokio::test]
async fn silent_utterance_never_reaches_any_stt() {
    let server = MockServer::start().await;
    let client = garraia_voice::WhisperClient::new(&server.uri(), "pt");
    let app = router(config(), None, Some(client));
    let (status, _, bytes) = call(
        app,
        transcription(
            &[("provider", "local"), ("model", "configured")],
            &silent_wav(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["text"], "");
    assert_eq!(body["skipped"], "silence");
    assert!(server.received_requests().await.unwrap().is_empty());
}

/// STT remoto: o audio vai em base64 com o idioma base; do `usage` do
/// provedor so voltam numeros (custo efetivo, segundos), nunca texto.
#[tokio::test]
async fn remote_transcription_returns_text_and_only_numeric_usage() {
    let server = MockServer::start().await;
    mock_fixture_catalog(&server).await;
    Mock::given(method("POST"))
        .and(path("/v1/audio/transcriptions"))
        .and(header("authorization", format!("Bearer {PROVIDER_KEY}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "text": " olá garra ",
            "usage": {"cost": 0.00012, "seconds": 0.2, "note": "internal routing detail"}
        })))
        .expect(1)
        .mount(&server)
        .await;
    let app = router(remote_config(&server), None, None);
    let (status, _, bytes) = call(
        app,
        transcription(
            &[
                ("provider", "voice-router"),
                ("model", "mistralai/voxtral-mini-transcribe-3-0"),
                ("language", "pt-BR"),
            ],
            &wav(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["text"], "olá garra");
    assert_eq!(body["usage"], json!({"cost": 0.00012, "seconds": 0.2}));
    let sent = server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.url.path() == "/v1/audio/transcriptions")
        .unwrap();
    let sent: Value = serde_json::from_slice(&sent.body).unwrap();
    assert_eq!(sent["model"], "mistralai/voxtral-mini-transcribe-3-0");
    assert_eq!(sent["language"], "pt");
    assert_eq!(sent["input_audio"]["format"], "wav");
}

/// Formato por familia documentada: Gemini pede PCM e, sem `rate`/`channels`
/// no MIME, usa o layout documentado (24 kHz mono) para virar WAV; Voxtral pede
/// MP3 e o MP3 passa direto.
#[tokio::test]
async fn documented_pcm_becomes_wav_and_mp3_passes_through() {
    let server = MockServer::start().await;
    mock_fixture_catalog(&server).await;
    Mock::given(method("POST"))
        .and(path("/v1/audio/speech"))
        .and(wiremock::matchers::body_partial_json(
            json!({"model":"google/gemini-3.8-flash-lite-tts","response_format":"pcm","voice":"Kore"}),
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "audio/pcm")
                .set_body_bytes(vec![0_u8; 4800]),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/audio/speech"))
        .and(wiremock::matchers::body_partial_json(
            json!({"model":"mistralai/voxtral-mini-tts-3-0","response_format":"mp3","voice":"voice_3150"}),
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "audio/mpeg")
                .set_body_bytes(b"ID3fake-mp3".to_vec()),
        )
        .expect(1)
        .mount(&server)
        .await;
    let app = router(remote_config(&server), None, None);
    let (status, headers, wav) = call(
        app.clone(),
        synth(json!({"provider":"voice-router","model":"google/gemini-3.8-flash-lite-tts","voice":"Kore","text":"Olá."})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["content-type"], "audio/wav");
    assert_eq!(&wav[..4], b"RIFF");
    assert_eq!(
        u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]),
        24_000
    );
    let (status, headers, mp3) = call(
        app,
        synth(json!({"provider":"voice-router","model":"mistralai/voxtral-mini-tts-3-0","voice":"voice_3150","text":"Olá."})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["content-type"], "audio/mpeg");
    assert_eq!(headers["cache-control"], "no-store");
    assert_eq!(mp3, b"ID3fake-mp3");
}

/// PCM sem layout de uma familia que nao documenta um padrao (HeyGen) e erro
/// visivel — nada de gerar de novo em outro formato (custo duplicado).
#[tokio::test]
async fn undocumented_pcm_layout_is_an_error_not_a_second_generation() {
    let server = MockServer::start().await;
    mock_fixture_catalog(&server).await;
    Mock::given(method("POST"))
        .and(path("/v1/audio/speech"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "audio/pcm")
                .set_body_bytes(vec![0_u8; 4800]),
        )
        .expect(1)
        .mount(&server)
        .await;
    let (status, _, _) = call(
        router(remote_config(&server), None, None),
        synth(json!({"provider":"voice-router","model":"heygen/heygen-voice-1-turbo","voice":"nadine","text":"Olá."})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
}

/// TTS local lento: segura a vaga enquanto sintetiza.
struct SlowSpeech;

#[async_trait::async_trait]
impl garraia_voice::TtsSynthesizer for SlowSpeech {
    async fn synthesize_bytes(
        &self,
        _text: &str,
        _language: &str,
    ) -> Result<Vec<u8>, garraia_voice::VoiceError> {
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        Ok(wav())
    }
}

/// Com as 4 vagas ocupadas, o pedido seguinte recebe 429 na hora, em vez de
/// entrar numa fila invisivel que atrasaria a fala de todo mundo.
#[tokio::test]
async fn concurrency_limit_answers_429_instead_of_queueing() {
    let app = router_with(config(), Some(Arc::new(SlowSpeech)), None, |_| {});
    let calls = (0..5).map(|i| {
        let app = app.clone();
        async move {
            call(
                app,
                synth(json!({"provider":"local","model":"configured","text":format!("frase {i}")})),
            )
            .await
            .0
        }
    });
    let statuses = futures::future::join_all(calls).await;
    assert_eq!(
        statuses.iter().filter(|s| **s == StatusCode::OK).count(),
        4,
        "{statuses:?}"
    );
    assert_eq!(
        statuses
            .iter()
            .filter(|s| **s == StatusCode::TOO_MANY_REQUESTS)
            .count(),
        1,
        "{statuses:?}"
    );
    // Vagas devolvidas: o proximo pedido passa.
    assert_eq!(
        call(
            app,
            synth(json!({"provider":"local","model":"configured","text":"depois"}))
        )
        .await
        .0,
        StatusCode::OK
    );
}

/// A chave do OpenRouter so sai por HTTPS ou por HTTP para um IP literal de
/// loopback. Um nome em `http` — mesmo `localhost`, que aqui resolveria para o
/// proprio mock — e recusado antes de qualquer pedido: o mock nao recebe nada.
#[tokio::test]
async fn credential_never_goes_to_a_cleartext_host_name() {
    let server = MockServer::start().await;
    mock_fixture_catalog(&server).await;
    let mut config = remote_config(&server);
    config.llm.get_mut("voice-router").unwrap().base_url =
        Some(format!("http://localhost:{}/v1", server.address().port()));
    let app = router(config, None, None);
    let (status, _, bytes) = call(
        app.clone(),
        request("GET", "/api/voice/models", "application/json", vec![]),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["catalogs"][0]["available"], false);
    assert_eq!(ids(&body["tts"]), ["local:configured"]);
    assert_eq!(
        call(app, synth(remote_speech())).await.0,
        StatusCode::BAD_REQUEST
    );
    assert!(server.received_requests().await.unwrap().is_empty());
}
