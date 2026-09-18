//! Regression for issue #1248: a provider registered via `POST /api/providers`
//! must connect with an SSRF-pinned HTTP client — redirects off, IPs pinned
//! to the addresses resolved at validation time. The route already ran
//! `vet_url` on the caller-supplied `base_url`, but it discarded the
//! `VettedUrl` and let the provider build its own `reqwest::Client` (default
//! redirect policy: up to 10 hops). That opened two windows the guard exists
//! to close:
//!
//!  1. **Redirect laundering** — a public host that passes validation responds
//!     `302` to `169.254.169.254` (cloud metadata) or any internal address.
//!  2. **DNS rebinding** — the host is resolved once at validation and again
//!     at connect; between the two the record can change.
//!
//! This test exercises the real `add_provider` handler — the same one
//! `POST /api/providers` routes to — and then drives the registered provider's
//! `complete` against a wiremock that 302-redirects to a second mock. With the
//! fix the redirect is not followed (the provider sees the 302 and errors);
//! without the fix (`redirect::Policy::none()` removed or the pinned client
//! not attached) the client follows the 302, reaches the second mock, and
//! returns its `200` as a successful completion — so the `is_err()` assertion
//! fails.

use std::sync::{Arc, LazyLock};

use axum::Json;
use axum::extract::State;
use garraia_agents::{AgentRuntime, ChatMessage, ChatRole, LlmRequest, MessagePart};
use garraia_channels::ChannelRegistry;
use garraia_config::AppConfig;
use garraia_gateway::router::{AddProviderRequest, add_provider};
use garraia_gateway::state::AppState;
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Point this test binary at a throwaway config dir before any `AppState` is
/// built — `AppState::new` provisions `mcp.json` and loads `allowlist.json`
/// from the default config dir, and without this the test would write the
/// developer's real `~/.config/garraia`. Same pattern as
/// `admin_mcp_restart_allowlist.rs`.
static TEST_ENV: LazyLock<tempfile::TempDir> = LazyLock::new(|| {
    let dir = tempdir().expect("temp config dir");
    // SAFETY: the `LazyLock` serialises this against the other test threads
    // in this binary; every test calls `test_env()` first.
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

fn minimal_request() -> LlmRequest {
    LlmRequest {
        model: "gpt-test".to_string(),
        messages: vec![ChatMessage {
            role: ChatRole::User,
            content: MessagePart::Text("hi".to_string()),
        }],
        system: None,
        max_tokens: Some(16),
        temperature: None,
        tools: Vec::new(),
    }
}

/// A valid OpenAI chat-completion body — what the "evil" redirect target
/// returns. If the client follows the `302`, the provider parses this and
/// returns `Ok`, which is exactly the outcome the fix must prevent.
fn evil_completion() -> serde_json::Value {
    serde_json::json!({
        "choices": [{
            "message": {"role": "assistant", "content": "REDIRECT_FOLLOWED"},
            "finish_reason": "stop"
        }],
        "model": "gpt-test",
        "usage": {"prompt_tokens": 1, "completion_tokens": 1}
    })
}

fn build_state(agents: Arc<AgentRuntime>) -> Arc<AppState> {
    test_env();
    Arc::new(AppState::new(
        AppConfig::default(),
        agents,
        ChannelRegistry::new(),
    ))
}

/// A provider registered via `add_provider` with a caller-supplied `base_url`
/// must use a pinned client that does NOT follow redirects.
///
/// Mutation proof: revert `OpenAiProvider::new` to the default redirect policy
/// (or drop the `with_client(pinned)` call in `add_provider`) and the client
/// follows the `302` to `evil`, parses its `200` as a valid completion, and
/// returns `Ok` — the `is_err()` assertion below turns red.
#[tokio::test]
async fn add_provider_pins_client_and_refuses_redirect() {
    // "evil" — the redirect target. Mounted on GET because reqwest follows a
    // 302 by re-issuing the request as GET (RFC 7231 §6.4.3).
    let evil = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(evil_completion()))
        .mount(&evil)
        .await;

    // "front" — the vetted provider endpoint. Returns 302 → evil.
    let front = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", evil.uri()))
        .mount(&front)
        .await;

    let agents = Arc::new(AgentRuntime::new());
    let state = build_state(agents.clone());

    // Register the provider exactly as `POST /api/providers` does.
    let body = AddProviderRequest {
        provider_type: "openai".to_string(),
        api_key: Some("sk-test".to_string()),
        model: Some("gpt-test".to_string()),
        base_url: Some(front.uri()),
        set_default: None,
    };
    let (status, Json(json)) = add_provider(State(state), Json(body)).await;
    assert_eq!(
        status,
        axum::http::StatusCode::CREATED,
        "provider registration: {json}"
    );

    let provider = agents
        .get_provider("openai")
        .expect("openai provider was registered");

    let result = provider.complete(&minimal_request()).await;

    // FIXED: the 302 is returned as-is (redirects off) → the provider errors
    // with `status=302`.
    // BROKEN: the client follows the 302 to `evil`, gets a 200 with a valid
    // OpenAI completion → `Ok`. The assertion below fails on broken code.
    assert!(
        result.is_err(),
        "pinned provider must NOT follow the 302 redirect to the evil target"
    );
    let err = format!("{result:?}");
    assert!(
        err.contains("302"),
        "error should report the un-followed 302 status, got: {err}"
    );
}

/// The legitimate local-first path: an Ollama on `127.0.0.1` must still pass
/// the SSRF gate under `IpScope::AllowPrivate` and register. This is the
/// "caminho legitimo intocado" acceptance criterion — the fix must not break
/// local providers.
#[tokio::test]
async fn add_provider_allows_local_ollama_under_allow_private() {
    let local = MockServer::start().await; // binds 127.0.0.1
    let agents = Arc::new(AgentRuntime::new());
    let state = build_state(agents.clone());

    let body = AddProviderRequest {
        provider_type: "ollama".to_string(),
        api_key: None,
        model: Some("llama-test".to_string()),
        base_url: Some(local.uri()),
        set_default: None,
    };
    let (status, Json(json)) = add_provider(State(state), Json(body)).await;
    assert_eq!(
        status,
        axum::http::StatusCode::CREATED,
        "local ollama should register: {json}"
    );
    assert!(
        agents.get_provider("ollama").is_some(),
        "ollama provider should be registered"
    );
}
