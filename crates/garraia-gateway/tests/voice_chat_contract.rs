//! Contracts for the Voice Chat routes mounted inside the real gateway.
//! No model inference, paid service, microphone, environment edits or external
//! network are needed: only ephemeral loopback HTTP servers are used.

use std::sync::{Arc, Mutex as StdMutex};

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
    let runtime = AgentRuntime::new();
    runtime.register_provider(Arc::new(RegisteredProvider));
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

fn wav() -> Vec<u8> {
    let pcm = vec![0_u8; 640]; // 20 ms of mono PCM16, 16 kHz.
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
