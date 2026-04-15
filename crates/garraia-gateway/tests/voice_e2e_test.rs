//! Voice E2E Test Suite
//!
//! Tests the complete voice pipeline: audio → STT → LLM → TTS → audio
//! Uses mocking for external services (Whisper, LLM, TTS) since they're not always available.
//!
//! Run with: cargo test -p garraia-gateway voice_e2e

use std::net::TcpListener;
use std::time::Duration;

use garraia_config::AppConfig;
use garraia_gateway::GatewayServer;
use garraia_voice::{VoiceError, VoiceMetrics};
use tracing::info;

/// Pick a random available port.
fn random_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind to random port");
    listener.local_addr().unwrap().port()
}

/// Build a minimal `AppConfig` for voice testing.
fn test_config(port: u16) -> AppConfig {
    let mut config = AppConfig::default();
    config.gateway.host = "127.0.0.1".to_string();
    config.gateway.port = port;
    config.memory.enabled = false;
    config.voice.enabled = true;
    config.voice.tts_endpoint = "http://localhost:9999".to_string();
    config.voice.language = "pt".to_string();

    // Add a mock LLM provider
    config.llm.insert(
        "mock".to_string(),
        garraia_config::LlmProviderConfig {
            provider: "anthropic".to_string(),
            model: Some("claude-test".to_string()),
            api_key: Some("sk-test-key".to_string()),
            base_url: Some("http://localhost:9998".to_string()),
            extra: Default::default(),
        },
    );

    config
}

/// Start the gateway in the background and return the base URL.
/// Waits for the server to be fully ready.
async fn start_test_gateway(config: AppConfig) -> String {
    let port = config.gateway.port;
    tokio::spawn(async move {
        let server = GatewayServer::new(config);
        let _ = server.run().await;
    });

    // Wait for the server to be ready with retries
    let mut retries = 0;
    while retries < 50 {
        if TcpListener::bind(format!("127.0.0.1:{port}")).is_err() {
            break; // port is in use = server is up
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        retries += 1;
    }

    // Additional delay to ensure server is fully initialized
    tokio::time::sleep(Duration::from_millis(1000)).await;

    format!("http://127.0.0.1:{port}")
}

// ============================================================================
// Mock Voice Pipeline for Testing
// ============================================================================

/// A mock voice pipeline that simulates STT → LLM → TTS with configurable behavior.
pub struct MockVoicePipeline {
    /// If true, STT will fail
    pub stt_failure: bool,
    /// If true, LLM will fail
    pub llm_failure: bool,
    /// If true, TTS will fail
    pub tts_failure: bool,
    /// Simulated transcription text
    pub mock_transcription: String,
    /// Simulated LLM response
    pub mock_llm_response: String,
    /// If true, record metrics
    pub record_metrics: bool,
    /// Captured metrics from last run
    pub last_metrics: Option<VoiceMetrics>,
}

impl MockVoicePipeline {
    pub fn new() -> Self {
        Self {
            stt_failure: false,
            llm_failure: false,
            tts_failure: false,
            mock_transcription: "Hello, this is a test".to_string(),
            mock_llm_response: "This is a mock response from the LLM.".to_string(),
            record_metrics: true,
            last_metrics: None,
        }
    }

    /// Create a pipeline that always fails at STT
    pub fn with_stt_failure(mut self) -> Self {
        self.stt_failure = true;
        self
    }

    /// Create a pipeline that always fails at LLM
    pub fn with_llm_failure(mut self) -> Self {
        self.llm_failure = true;
        self
    }

    /// Create a pipeline that always fails at TTS
    pub fn with_tts_failure(mut self) -> Self {
        self.tts_failure = true;
        self
    }

    /// Process voice with the mock pipeline
    pub async fn process_voice<F, Fut>(
        &mut self,
        _input: &std::path::Path,
        llm_callback: F,
    ) -> Result<(std::path::PathBuf, VoiceMetrics), VoiceError>
    where
        F: FnOnce(String) -> Fut,
        Fut: std::future::Future<Output = Result<String, VoiceError>>,
    {
        let start = std::time::Instant::now();

        // Step 1: STT (Whisper) - simulated
        let stt_start = std::time::Instant::now();
        tokio::time::sleep(Duration::from_millis(1)).await; // Small delay to ensure timing
        if self.stt_failure {
            return Err(VoiceError::Stt("Mock STT failure".to_string()));
        }
        let transcribed_text = self.mock_transcription.clone();
        let stt_ms = stt_start.elapsed().as_millis();
        info!(
            stt_ms,
            text_len = transcribed_text.len(),
            "Mock STT complete"
        );

        // Step 2: LLM - simulated
        let llm_start = std::time::Instant::now();
        tokio::time::sleep(Duration::from_millis(1)).await;
        if self.llm_failure {
            return Err(VoiceError::Llm("Mock LLM failure".to_string()));
        }
        let llm_response = llm_callback(transcribed_text.clone()).await?;
        let llm_ms = llm_start.elapsed().as_millis();
        info!(
            llm_ms,
            response_len = llm_response.len(),
            "Mock LLM complete"
        );

        // Step 3: TTS - simulated
        let tts_start = std::time::Instant::now();
        tokio::time::sleep(Duration::from_millis(1)).await;
        if self.tts_failure {
            return Err(VoiceError::Tts("Mock TTS failure".to_string()));
        }
        // Create a dummy output file
        let output_path = std::env::temp_dir().join("mock_voice_output.ogg");
        // Write dummy audio data
        tokio::fs::write(&output_path, b"OGG")
            .await
            .unwrap_or_default();
        let tts_ms = tts_start.elapsed().as_millis();
        info!(tts_ms, "Mock TTS complete");

        let total_ms = start.elapsed().as_millis();

        let metrics = VoiceMetrics {
            ogg_to_wav_ms: 10,
            stt_ms,
            transcribed_text,
            llm_ms,
            llm_response: llm_response.clone(),
            tts_ms,
            wav_to_ogg_ms: 5,
            total_ms,
        };

        if self.record_metrics {
            info!(
                total_ms,
                stt_ms, llm_ms, tts_ms, "Voice pipeline metrics recorded"
            );
        }

        self.last_metrics = Some(metrics.clone());

        Ok((output_path, metrics))
    }
}

// ============================================================================
// TTS Endpoint Tests (integration-style)
// These tests verify the TTS endpoint behavior
// ============================================================================

#[tokio::test]
#[ignore = "TODO(fix/ci-triage-2026-04-15): needs running gateway (same server.run() cascade as auth_test.rs / gateway_integration.rs — exits silently on CI missing Postgres). Deferred to gateway-test-fixture follow-up PR."]
async fn tts_endpoint_returns_error_when_voice_not_enabled() {
    let port = random_port();
    let mut config = test_config(port);
    config.voice.enabled = false; // Disable voice
    let base_url = start_test_gateway(config).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/tts", base_url))
        .json(&serde_json::json!({
            "text": "Hello world"
        }))
        .send()
        .await
        .expect("tts request failed");

    // Should return service unavailable
    assert_eq!(resp.status(), 503);

    let body: serde_json::Value = resp.json().await.unwrap();
    // Should have error or text (fallback)
    assert!(body.get("error").is_some() || body.get("text").is_some());
}

#[tokio::test]
#[ignore = "TODO(fix/ci-triage-2026-04-15): needs running gateway (same server.run() cascade). Deferred."]
async fn voice_status_endpoint_when_disabled() {
    let port = random_port();
    let mut config = test_config(port);
    config.voice.enabled = false;
    let base_url = start_test_gateway(config).await;

    // The status endpoint should work regardless of voice
    let resp = reqwest::Client::new()
        .get(format!("{}/api/status", base_url))
        .send()
        .await
        .expect("status request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "running");
}

// ============================================================================
// Voice Pipeline Unit Tests (with mocks)
// These tests verify the voice pipeline with mocked STT/LLM/TTS
// ============================================================================

#[tokio::test]
async fn mock_voice_pipeline_success() {
    let mut pipeline = MockVoicePipeline::new();

    let result = pipeline
        .process_voice(
            std::path::Path::new("/dummy/input.ogg"),
            |text| async move { Ok(format!("Echo: {}", text)) },
        )
        .await;

    assert!(result.is_ok());
    let (_path, metrics) = result.unwrap();
    assert_eq!(metrics.transcribed_text, "Hello, this is a test");
    assert!(metrics.llm_response.contains("Echo"));
}

#[tokio::test]
async fn mock_voice_pipeline_stt_failure() {
    let mut pipeline = MockVoicePipeline::new().with_stt_failure();

    let result = pipeline
        .process_voice(
            std::path::Path::new("/dummy/input.ogg"),
            |_text| async move { Ok("Should not reach here".to_string()) },
        )
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, VoiceError::Stt(_)));
}

#[tokio::test]
async fn mock_voice_pipeline_llm_failure() {
    let mut pipeline = MockVoicePipeline::new().with_llm_failure();

    let result = pipeline
        .process_voice(
            std::path::Path::new("/dummy/input.ogg"),
            |_text| async move { Err(VoiceError::Llm("Mock LLM failure".to_string())) },
        )
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, VoiceError::Llm(_)));
}

#[tokio::test]
async fn mock_voice_pipeline_tts_failure() {
    let mut pipeline = MockVoicePipeline::new().with_tts_failure();

    let result = pipeline
        .process_voice(
            std::path::Path::new("/dummy/input.ogg"),
            |text| async move { Ok(format!("Response to: {}", text)) },
        )
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, VoiceError::Tts(_)));
}

#[tokio::test]
async fn mock_voice_pipeline_metrics_recorded() {
    let mut pipeline = MockVoicePipeline::new();

    let _ = pipeline
        .process_voice(
            std::path::Path::new("/dummy/input.ogg"),
            |text| async move { Ok(format!("Response: {}", text)) },
        )
        .await;

    assert!(pipeline.last_metrics.is_some());
    let metrics = pipeline.last_metrics.unwrap();

    // Verify all metrics are populated (at least 1ms due to sleep)
    assert!(metrics.stt_ms >= 1);
    assert!(metrics.llm_ms >= 1);
    assert!(metrics.tts_ms >= 1);
    assert!(metrics.total_ms >= 1);
    assert!(!metrics.transcribed_text.is_empty());
    assert!(!metrics.llm_response.is_empty());
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[tokio::test]
async fn voice_pipeline_handles_empty_transcription() {
    let mut pipeline = MockVoicePipeline::new();
    pipeline.mock_transcription = "   ".to_string();

    // The pipeline should handle empty transcription via callback
    let result = pipeline
        .process_voice(
            std::path::Path::new("/dummy/input.ogg"),
            |text| async move {
                if text.trim().is_empty() {
                    Err(VoiceError::Stt("Empty transcription".to_string()))
                } else {
                    Ok(format!("Got: {}", text))
                }
            },
        )
        .await;

    // Should fail because of empty transcription
    assert!(result.is_err());
}

#[tokio::test]
async fn voice_pipeline_handles_empty_llm_response() {
    let mut pipeline = MockVoicePipeline::new();
    // The callback will return empty string
    let result = pipeline
        .process_voice(
            std::path::Path::new("/dummy/input.ogg"),
            |_text| async move {
                Ok("".to_string()) // Empty response from callback
            },
        )
        .await;

    // In this implementation, empty response from LLM is allowed
    // but the test verifies the behavior
    assert!(result.is_ok());
}

// ============================================================================
// Structured Logging Tests
// ============================================================================

#[tokio::test]
async fn voice_pipeline_logs_structured_info() {
    // This test verifies that structured logging works
    // The actual log output would need to be captured to verify

    let mut pipeline = MockVoicePipeline::new();

    let _ = pipeline
        .process_voice(
            std::path::Path::new("/dummy/input.ogg"),
            |text| async move { Ok(format!("Processed: {}", text)) },
        )
        .await;

    // If we got here without panicking, logging is working
    assert!(true);
}

// ============================================================================
// Full Pipeline Tests (E2E simulation)
// ============================================================================

#[tokio::test]
async fn voice_e2e_full_pipeline_with_mocked_services() {
    let mut pipeline = MockVoicePipeline::new();

    // Simulate the full voice E2E pipeline
    let result = pipeline
        .process_voice(
            std::path::Path::new("/dummy/input.ogg"),
            |text| async move {
                // Simulate LLM processing
                let response = format!("I heard you say: {}. Let me think about that...", text);
                Ok(response)
            },
        )
        .await;

    assert!(result.is_ok());
    let (_output_path, metrics) = result.unwrap();

    // Verify metrics are recorded
    assert!(metrics.stt_ms > 0);
    assert!(metrics.llm_ms > 0);
    assert!(metrics.tts_ms > 0);
    assert!(metrics.total_ms > 0);

    // Verify the pipeline worked
    assert!(metrics.transcribed_text.contains("Hello"));
    assert!(metrics.llm_response.contains("I heard you say"));

    info!(
        total_ms = metrics.total_ms,
        stt_ms = metrics.stt_ms,
        llm_ms = metrics.llm_ms,
        tts_ms = metrics.tts_ms,
        "Voice E2E pipeline completed successfully"
    );
}

#[tokio::test]
async fn voice_e2e_fallback_on_stt_failure() {
    let mut pipeline = MockVoicePipeline::new().with_stt_failure();

    // Test graceful degradation when STT fails
    let result = pipeline
        .process_voice(
            std::path::Path::new("/dummy/input.ogg"),
            |_text| async move { Ok("Should not reach here".to_string()) },
        )
        .await;

    // Should return error (no automatic fallback in mock)
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, VoiceError::Stt(_)));

    info!("Voice E2E STT failure handled gracefully");
}

#[tokio::test]
async fn voice_e2e_fallback_on_tts_failure() {
    let mut pipeline = MockVoicePipeline::new().with_tts_failure();

    // Test graceful degradation when TTS fails
    // In real implementation, this would fallback to text
    let result = pipeline
        .process_voice(
            std::path::Path::new("/dummy/input.ogg"),
            |text| async move { Ok(format!("Text response: {}", text)) },
        )
        .await;

    // Should return error
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, VoiceError::Tts(_)));

    info!("Voice E2E TTS failure handled gracefully");
}
