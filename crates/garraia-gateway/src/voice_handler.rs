use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;
use tracing::{error, info, warn};

use crate::state::SharedState;

/// Response for TTS with optional fallback text
#[derive(serde::Serialize)]
pub struct TtsResponse {
    pub audio: Option<String>, // Base64 encoded audio if successful
    pub text: Option<String>,  // Text fallback if TTS fails
    pub error: Option<String>,
    pub fallback: bool, // true if we fell back to text
}

#[derive(Deserialize)]
pub struct TtsRequest {
    /// Text to synthesize into speech.
    pub text: String,
    /// Language code (e.g. "pt", "en", "es", "fr", "de", "it", "hi").
    /// Falls back to the configured default if omitted.
    pub language: Option<String>,
}

#[derive(Deserialize, Default, Debug)]
pub struct SynthesizeQuery {
    pub fallback: Option<String>,
}

/// POST /api/tts — synthesize speech from text using Chatterbox TTS.
///
/// Returns WAV audio bytes with `Content-Type: audio/wav` on success.
/// On failure, returns a JSON response with text fallback.
/// Only available when the server is started with `--with-voice`.
///
/// Query parameter `fallback` (default: true) controls whether to return
/// text fallback when TTS fails. Set to "false" to get error only.
#[tracing::instrument(skip(state, body), fields(text_len, lang))]
pub async fn synthesize(
    State(state): State<SharedState>,
    axum::extract::Query(query): axum::extract::Query<SynthesizeQuery>,
    Json(body): Json<TtsRequest>,
) -> impl IntoResponse {
    let allow_fallback = !query
        .fallback
        .as_deref()
        .map(|s| s.eq_ignore_ascii_case("false"))
        .unwrap_or(false);

    let lang = body.language.as_deref().unwrap_or("default");
    tracing::Span::current().record("text_len", body.text.len());
    tracing::Span::current().record("lang", lang);

    let Some(voice_client) = &state.voice_client else {
        warn!("TTS request rejected: voice mode not enabled");
        return if allow_fallback {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(TtsResponse {
                    audio: None,
                    text: Some(
                        "Voice mode is not enabled. Start the server with --with-voice."
                            .to_string(),
                    ),
                    error: Some("Voice mode not enabled".to_string()),
                    fallback: true,
                }),
            )
                .into_response()
        } else {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": "Voice mode is not enabled. Start the server with --with-voice."
                })),
            )
                .into_response()
        };
    };

    if body.text.trim().is_empty() {
        warn!("TTS request rejected: empty text");
        return if allow_fallback {
            (
                StatusCode::BAD_REQUEST,
                Json(TtsResponse {
                    audio: None,
                    text: Some("Cannot synthesize empty text".to_string()),
                    error: Some("text field cannot be empty".to_string()),
                    fallback: true,
                }),
            )
                .into_response()
        } else {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "text field cannot be empty"
                })),
            )
                .into_response()
        };
    }

    info!(
        text_preview = %&body.text[..body.text.len().min(80)],
        "🔊 TTS synthesis started"
    );
    let start = std::time::Instant::now();

    match voice_client
        .synthesize(&body.text, body.language.as_deref())
        .await
    {
        Ok(audio_bytes) => {
            let elapsed = start.elapsed();
            info!(
                audio_bytes = audio_bytes.len(),
                duration_ms = elapsed.as_millis() as u64,
                "✅ TTS synthesis complete — WAV generated"
            );
            let headers = [(axum::http::header::CONTENT_TYPE, "audio/wav")];
            (headers, audio_bytes).into_response()
        }
        Err(e) => {
            let elapsed = start.elapsed();
            error!(
                error = %e,
                duration_ms = elapsed.as_millis() as u64,
                "❌ TTS synthesis failed"
            );

            if allow_fallback {
                // Graceful degradation: return text instead of crashing
                (
                    StatusCode::OK,
                    Json(TtsResponse {
                        audio: None,
                        text: Some(body.text.clone()),
                        error: Some(format!("TTS failed: {}", e)),
                        fallback: true,
                    }),
                )
                    .into_response()
            } else {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "error": format!("TTS synthesis failed: {}", e)
                    })),
                )
                    .into_response()
            }
        }
    }
}
