use axum::Json;
use axum::extract::{Multipart, State};
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

/// Resume `texto` ao limite de `max` CARACTERES (nao bytes) para log.
///
/// Cortar por byte (`&texto[..n]`) pode cair no meio de um code point
/// multibyte — um acento pt-BR ou um emoji na fronteira — e entrar em panico.
/// Como o gateway nao tem `CatchPanicLayer`, esse panico derrubava a conexao
/// do `POST /api/tts`. `chars().take()` respeita a fronteira de caractere.
fn resumo_do_texto(texto: &str, max: usize) -> String {
    texto.chars().take(max).collect()
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

    let text_preview = resumo_do_texto(&body.text, 80);
    info!(
        text_preview = %text_preview,
        "🔊 TTS synthesis started"
    );
    let start = std::time::Instant::now();

    match voice_client.synthesize_bytes(&body.text, lang).await {
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

            if allow_fallback {
                // Graceful degradation: return text instead of crashing. O
                // motivo vai para o log ANTES do fallback para que uma voz que
                // "falha em silencio" deixe rastro — warn porque a rota se
                // recupera, nao quebra.
                warn!(
                    error = %e,
                    duration_ms = elapsed.as_millis() as u64,
                    "⚠️ TTS synthesis failed — falling back to text"
                );
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
                error!(
                    error = %e,
                    duration_ms = elapsed.as_millis() as u64,
                    "❌ TTS synthesis failed (no fallback)"
                );
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
            (StatusCode::OK, Json(SttResponse { text, error: None }))
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

#[cfg(test)]
mod tests {
    use super::resumo_do_texto;

    #[test]
    fn resumo_nao_panica_no_byte_que_cairia_no_meio_de_char() {
        // Reproduz o panico antigo: 79 'a' + 'ç' = 81 bytes. O corte por byte
        // em `min(80)` caia dentro do 'ç' (bytes 79..81) e entrava em panico.
        // Por caractere, pega os 80 primeiros inteiros sem quebrar.
        let texto = format!("{}ç", "a".repeat(79));
        let resumo = resumo_do_texto(&texto, 80);
        assert_eq!(resumo.chars().count(), 80);
        assert!(resumo.ends_with('ç'), "resumo: {resumo}");
    }

    #[test]
    fn resumo_com_acento_dentro_do_limite_e_identico() {
        let texto = "ação da garra";
        assert_eq!(resumo_do_texto(texto, 80), texto);
    }

    #[test]
    fn resumo_com_emoji_na_fronteira_nao_panica() {
        // Emoji sao 4 bytes cada; 40 emojis = 160 bytes, 40 caracteres. Pedir
        // 80 caracteres devolve todos; cortar por byte cairia no meio de um.
        let texto = "🔊".repeat(40);
        let resumo = resumo_do_texto(&texto, 80);
        assert_eq!(resumo.chars().count(), 40);
        assert_eq!(resumo, texto);
    }

    #[test]
    fn resumo_trunca_em_caracteres_quando_excede() {
        // 100 'á' = 200 bytes, 100 caracteres; o corte em 80 devolve 80
        // caracteres inteiros (160 bytes), nunca um byte solto.
        let texto = "á".repeat(100);
        let resumo = resumo_do_texto(&texto, 80);
        assert_eq!(resumo.chars().count(), 80);
        assert_eq!(resumo, "á".repeat(80));
    }
}
