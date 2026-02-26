use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;

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
pub async fn synthesize(
    State(state): State<SharedState>,
    Json(body): Json<TtsRequest>,
) -> impl IntoResponse {
    let Some(voice_client) = &state.voice_client else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "Voice mode is not enabled. Start the server with --with-voice."
            })),
        )
            .into_response();
    };

    if body.text.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "text field cannot be empty"
            })),
        )
            .into_response();
    }

    match voice_client
        .synthesize(&body.text, body.language.as_deref())
        .await
    {
        Ok(audio_bytes) => {
            let headers = [
                (axum::http::header::CONTENT_TYPE, "audio/wav"),
            ];
            (headers, audio_bytes).into_response()
        }
        Err(e) => {
            tracing::error!("TTS synthesis failed: {e}");
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
