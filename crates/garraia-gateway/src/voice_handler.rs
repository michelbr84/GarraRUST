use axum::Json;
use axum::extract::{State, Multipart};
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
        .synthesize_bytes(&body.text, lang)
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

/// Response for STT transcription
#[derive(serde::Serialize)]
pub struct SttResponse {
    pub text: String,
    pub error: Option<String>,
}

/// POST /api/stt — transcribe audio using Whisper STT.
/// 
/// Accepts multipart form data with an audio file (WAV, OGG, MP3, etc.).
/// Returns JSON with transcribed text on success.
/// Only available when the server is started with `--with-voice`.
#[tracing::instrument(skip(state), fields(filename))]
pub async fn transcribe(
    State(state): State<SharedState>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let Some(stt_client) = &state.stt_client else {
        warn!("STT request rejected: voice mode not enabled");
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(SttResponse {
                text: String::new(),
                error: Some(
                    "Voice mode is not enabled. Start the server with --with-voice.".to_string(),
                ),
            }),
        );
    };

    // Extract audio file from multipart
    let mut audio_data: Option<(String, Vec<u8>)> = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        let field_name = field.name().unwrap_or("").to_string();
        
        if field_name == "file" {
            let filename = field
                .file_name()
                .map(|s: &str| s.to_string())
                .unwrap_or_else(|| "audio.wav".to_string());
            
            // Record filename for tracing before moving
            tracing::Span::current().record("filename", &filename);
            
            let data: Vec<u8> = match field.bytes().await {
                Ok(bytes) => bytes.to_vec(),
                Err(e) => {
                    error!("Failed to read file data: {}", e);
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(SttResponse {
                            text: String::new(),
                            error: Some(format!("Failed to read file: {}", e)),
                        }),
                    );
                }
            };
            audio_data = Some((filename, data));
        }
    }

    let (filename, audio_bytes) = match audio_data {
        Some(data) => data,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(SttResponse {
                    text: String::new(),
                    error: Some("No audio file provided".to_string()),
                }),
            );
        }
    };

    if audio_bytes.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(SttResponse {
                text: String::new(),
                error: Some("Audio file is empty".to_string()),
            }),
        );
    }

    info!(
        filename = %filename,
        audio_size = audio_bytes.len(),
        "🎙️ STT transcription started"
    );
    let start = std::time::Instant::now();

    // Create temp file for Whisper (it expects a file path)
    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join(format!("garraia_stt_{}.wav", uuid::Uuid::new_v4()));
    
    // Write audio to temp file
    if let Err(e) = tokio::fs::write(&temp_path, &audio_bytes).await {
        error!("Failed to write temp audio file: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(SttResponse {
                text: String::new(),
                error: Some(format!("Failed to process audio: {}", e)),
            }),
        );
    }

    // Transcribe using Whisper
    let result = stt_client.transcribe(&temp_path).await;

    // Clean up temp file
    let _ = tokio::fs::remove_file(&temp_path).await;

    match result {
        Ok(text) => {
            let elapsed = start.elapsed();
            info!(
                text_len = text.len(),
                duration_ms = elapsed.as_millis() as u64,
                "✅ STT transcription complete"
            );
            (
                StatusCode::OK,
                Json(SttResponse {
                    text,
                    error: None,
                }),
            )
        }
        Err(e) => {
            let elapsed = start.elapsed();
            error!(
                error = %e,
                duration_ms = elapsed.as_millis() as u64,
                "❌ STT transcription failed"
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(SttResponse {
                    text: String::new(),
                    error: Some(format!("Transcription failed: {}", e)),
                }),
            )
        }
    }
}
