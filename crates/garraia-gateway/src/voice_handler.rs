use axum::Json;
use axum::extract::{Multipart, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use garraia_voice::{MAX_AUDIO_BYTES, MAX_TEXT_BYTES, VOICE_REQUEST_TIMEOUT, VoiceError};
use serde::Deserialize;
use tracing::{debug, info, warn};

use crate::state::SharedState;

#[derive(serde::Serialize)]
pub struct TtsResponse {
    pub audio: Option<String>,
    pub text: Option<String>,
    pub error: Option<String>,
    pub fallback: bool,
}

#[derive(Deserialize)]
pub struct TtsRequest {
    pub text: String,
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
/// do `POST /api/tts` (#1768). `chars().take()` respeita a fronteira de
/// caractere. Fica em `debug!`: o texto e do usuario e nao vai ao log `info`.
fn resumo_do_texto(texto: &str, max: usize) -> String {
    texto.chars().take(max).collect()
}

/// POST /api/tts. Preserve the legacy fallback contract, while bounding work
/// and never reflecting provider response bodies (which may contain secrets).
#[tracing::instrument(skip(state, body), fields(text_len = body.text.len()))]
pub async fn synthesize(
    State(state): State<SharedState>,
    axum::extract::Query(query): axum::extract::Query<SynthesizeQuery>,
    Json(body): Json<TtsRequest>,
) -> axum::response::Response {
    let allow_fallback = !query
        .fallback
        .as_deref()
        .is_some_and(|s| s.eq_ignore_ascii_case("false"));
    let failure = |status, message: &str| {
        if allow_fallback {
            (
                status,
                Json(TtsResponse {
                    audio: None,
                    text: Some(message.to_string()),
                    error: Some(message.to_string()),
                    fallback: true,
                }),
            )
                .into_response()
        } else {
            (status, Json(serde_json::json!({"error": message}))).into_response()
        }
    };
    let Some(voice_client) = &state.voice_client else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "Voice mode is not enabled. Start the server with --with-voice.",
        );
    };
    if body.text.trim().is_empty() {
        return failure(StatusCode::BAD_REQUEST, "text field cannot be empty");
    }
    if body.text.len() > MAX_TEXT_BYTES {
        return failure(
            StatusCode::PAYLOAD_TOO_LARGE,
            "text exceeds the synthesis limit",
        );
    }
    if body.language.as_ref().is_some_and(|s| s.len() > 64) {
        return failure(StatusCode::BAD_REQUEST, "invalid language");
    }
    let config = state.current_config();
    let language = body
        .language
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&config.voice.language);
    debug!(text_preview = %resumo_do_texto(&body.text, 80), "voice synthesis started");
    let start = std::time::Instant::now();
    let result = tokio::time::timeout(
        VOICE_REQUEST_TIMEOUT,
        voice_client.synthesize_bytes(&body.text, language),
    )
    .await
    .unwrap_or_else(|_| Err(VoiceError::Tts("synthesis timed out".into())))
    .and_then(|audio| {
        if audio.is_empty() || audio.len() > MAX_AUDIO_BYTES {
            Err(VoiceError::Tts("invalid audio response size".into()))
        } else {
            Ok(audio)
        }
    });
    match result {
        Ok(bytes) => {
            info!(
                audio_bytes = bytes.len(),
                duration_ms = start.elapsed().as_millis() as u64,
                "voice synthesis complete"
            );
            ([(axum::http::header::CONTENT_TYPE, "audio/wav")], bytes).into_response()
        }
        Err(error) => {
            warn!(error = %error, duration_ms = start.elapsed().as_millis() as u64, "voice synthesis failed");
            if allow_fallback {
                (
                    StatusCode::OK,
                    Json(TtsResponse {
                        audio: None,
                        text: Some(body.text),
                        error: Some("TTS failed: speech provider unavailable".into()),
                        fallback: true,
                    }),
                )
                    .into_response()
            } else {
                failure(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "TTS synthesis failed: speech provider unavailable",
                )
            }
        }
    }
}

#[derive(serde::Serialize)]
pub struct SttResponse {
    pub text: String,
    pub error: Option<String>,
}

fn stt_failure(status: StatusCode, message: &str) -> (StatusCode, Json<SttResponse>) {
    (
        status,
        Json(SttResponse {
            text: String::new(),
            error: Some(message.to_string()),
        }),
    )
}

/// POST /api/stt. Audio remains in memory only and is released on success,
/// error, disconnect/cancellation or timeout; no recordings are persisted.
#[tracing::instrument(skip(state, multipart))]
pub async fn transcribe(
    State(state): State<SharedState>,
    mut multipart: Multipart,
) -> (StatusCode, Json<SttResponse>) {
    let Some(stt_client) = &state.stt_client else {
        return stt_failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "Voice mode is not enabled. Start the server with --with-voice.",
        );
    };
    let mut audio = None;
    let mut field_count = 0;
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(_) => {
                return stt_failure(StatusCode::BAD_REQUEST, "Invalid audio multipart upload");
            }
        };
        field_count += 1;
        if field_count > 8 {
            return stt_failure(StatusCode::BAD_REQUEST, "Too many multipart fields");
        }
        if field.name() != Some("file") {
            continue;
        }
        if audio.is_some() {
            return stt_failure(StatusCode::BAD_REQUEST, "Provide exactly one audio file");
        }
        let filename: String = field
            .file_name()
            .unwrap_or("audio.wav")
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("audio.wav")
            .chars()
            .take(120)
            .collect();
        let content_type = field.content_type().unwrap_or("audio/wav").to_string();
        if !content_type.starts_with("audio/") && content_type != "application/octet-stream" {
            return stt_failure(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Expected an audio content type",
            );
        }
        let mut field = field;
        let mut bytes = Vec::new();
        loop {
            match field.chunk().await {
                Ok(Some(chunk)) => {
                    if chunk.len() > MAX_AUDIO_BYTES.saturating_sub(bytes.len()) {
                        return stt_failure(
                            StatusCode::PAYLOAD_TOO_LARGE,
                            "Audio exceeds the upload limit",
                        );
                    }
                    bytes.extend_from_slice(&chunk);
                }
                Ok(None) => break,
                Err(_) => return stt_failure(StatusCode::BAD_REQUEST, "Invalid audio upload"),
            }
        }
        audio = Some((filename, content_type, bytes));
    }
    let Some((filename, content_type, bytes)) = audio else {
        return stt_failure(StatusCode::BAD_REQUEST, "No audio file provided");
    };
    if bytes.is_empty() {
        return stt_failure(StatusCode::BAD_REQUEST, "Audio file is empty");
    }
    let start = std::time::Instant::now();
    let result = tokio::time::timeout(
        VOICE_REQUEST_TIMEOUT,
        stt_client.transcribe_bytes(&bytes, &filename, &content_type, None, None),
    )
    .await
    .unwrap_or_else(|_| Err(VoiceError::Stt("transcription timed out".into())));
    match result {
        Ok(text) => {
            info!(
                text_len = text.len(),
                duration_ms = start.elapsed().as_millis() as u64,
                "voice transcription complete"
            );
            (StatusCode::OK, Json(SttResponse { text, error: None }))
        }
        Err(error) => {
            warn!(error = %error, duration_ms = start.elapsed().as_millis() as u64, "voice transcription failed");
            stt_failure(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Transcription failed: speech provider unavailable",
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
