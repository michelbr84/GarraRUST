use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;
use tracing::{info, warn, error};

use crate::state::SharedState;

#[derive(Deserialize)]
pub struct TtsRequest {
    /// Text to synthesize into speech.
    pub text: String,
    /// Language code (e.g. "pt", "en", "es", "fr", "de", "it", "hi").
    /// Falls back to the configured default if omitted.
    pub language: Option<String>,
}

/// POST /api/tts — synthesize speech from text using Chatterbox TTS.
///
/// Returns WAV audio bytes with `Content-Type: audio/wav`.
/// Only available when the server is started with `--with-voice`.
#[tracing::instrument(skip(state, body), fields(text_len, lang))]
pub async fn synthesize(
    State(state): State<SharedState>,
    Json(body): Json<TtsRequest>,
) -> impl IntoResponse {
    let lang = body.language.as_deref().unwrap_or("default");
    tracing::Span::current().record("text_len", body.text.len());
    tracing::Span::current().record("lang", lang);

    let Some(voice_client) = &state.voice_client else {
        warn!("TTS request rejected: voice mode not enabled");
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "Voice mode is not enabled. Start the server with --with-voice."
            })),
        )
            .into_response();
    };

    if body.text.trim().is_empty() {
        warn!("TTS request rejected: empty text");
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "text field cannot be empty"
            })),
        )
            .into_response();
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
            let headers = [
                (axum::http::header::CONTENT_TYPE, "audio/wav"),
            ];
            (headers, audio_bytes).into_response()
        }
        Err(e) => {
            let elapsed = start.elapsed();
            error!(
                error = %e,
                duration_ms = elapsed.as_millis() as u64,
                "❌ TTS synthesis failed"
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("TTS synthesis failed: {e}")
                })),
            )
                .into_response()
        }
    }
}
