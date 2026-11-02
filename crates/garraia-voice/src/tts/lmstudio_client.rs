use crate::http::{MAX_AUDIO_BYTES, MAX_TEXT_BYTES, api_url, bounded_body};
use crate::pipeline::VoiceError;
use std::path::{Path, PathBuf};

/// Adaptador para um servidor de fala local compativel com OpenAI (LM Studio).
///
/// Tenta `POST /v1/audio/speech`; so cai no `POST /api/v1/chat` quando a rota
/// nao existe (404/405/501). Timeout, limite de taxa ou erro do provedor nao
/// disparam uma segunda geracao (sem custo duplicado implicito).
#[derive(Clone)]
pub struct LmStudioTtsClient {
    endpoint: String,
    client: reqwest::Client,
    model: String,
    language: String,
}

impl LmStudioTtsClient {
    pub fn new(endpoint: &str, model: &str, language: &str) -> Self {
        Self {
            endpoint: endpoint.trim_end_matches('/').to_string(),
            client: crate::http::client(),
            model: model.to_string(),
            language: language.to_string(),
        }
    }

    /// Sintetiza no idioma e voz configurados e devolve os bytes. Sem
    /// arquivo intermediario: cada chamada fica com os proprios bytes (#1754).
    pub async fn synthesize_to_bytes(&self, text: &str) -> Result<Vec<u8>, VoiceError> {
        self.synthesize_bytes_with_options(text, None, None).await
    }

    pub async fn synthesize(&self, text: &str, output_path: &Path) -> Result<PathBuf, VoiceError> {
        let bytes = self.synthesize_bytes_with_options(text, None, None).await?;
        tokio::fs::write(output_path, bytes).await?;
        Ok(output_path.to_path_buf())
    }

    /// Calls own their bytes, including concurrent segments and cancellation.
    /// The OpenAI speech shape infers language from input text; language is
    /// sent to the legacy local chat fallback only.
    pub async fn synthesize_bytes_with_options(
        &self,
        text: &str,
        language: Option<&str>,
        voice: Option<&str>,
    ) -> Result<Vec<u8>, VoiceError> {
        if text.trim().is_empty() || text.len() > MAX_TEXT_BYTES {
            return Err(VoiceError::Tts(
                "text is empty or exceeds the size limit".into(),
            ));
        }
        let explicit_voice = voice.is_some_and(|s| !s.trim().is_empty());
        let voice = voice.filter(|s| !s.trim().is_empty()).unwrap_or("alloy");
        if voice.len() > 128 || language.is_some_and(|s| s.len() > 64) {
            return Err(VoiceError::Tts("invalid voice or language".into()));
        }
        let response = self.client.post(api_url(&self.endpoint, "/v1/audio/speech"))
            .json(&serde_json::json!({"model": self.model, "input": text, "voice": voice, "response_format": "wav"}))
            .send().await.map_err(|_| VoiceError::Tts("speech request failed".into()))?;
        // Retry only a missing route, never timeout/rate limit/provider error.
        if matches!(response.status().as_u16(), 404 | 405 | 501) {
            if explicit_voice {
                return Err(VoiceError::Tts(
                    "this server does not expose voice selection through /v1/audio/speech".into(),
                ));
            }
            return self
                .synthesize_via_chat(text, language.unwrap_or(&self.language))
                .await;
        }
        Self::check_status(&response)?;
        bounded_body(response, MAX_AUDIO_BYTES)
            .await
            .map_err(VoiceError::Tts)
    }

    fn check_status(response: &reqwest::Response) -> Result<(), VoiceError> {
        if response.status().is_success() {
            Ok(())
        } else {
            Err(VoiceError::Tts(format!(
                "speech server returned HTTP {}",
                response.status().as_u16()
            )))
        }
    }

    async fn synthesize_via_chat(&self, text: &str, language: &str) -> Result<Vec<u8>, VoiceError> {
        let response = self.client.post(format!("{}/api/v1/chat", self.endpoint.trim_end_matches("/v1")))
            .json(&serde_json::json!({"model": self.model, "system_prompt": format!("You are a text-to-speech engine. Speak naturally in {language}."), "input": text}))
            .send().await.map_err(|_| VoiceError::Tts("local speech fallback request failed".into()))?;
        Self::check_status(&response)?;
        let audio = response
            .headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok())
            .is_some_and(|s| s.starts_with("audio/"));
        let bytes = bounded_body(
            response,
            if audio {
                MAX_AUDIO_BYTES
            } else {
                MAX_AUDIO_BYTES * 4 / 3 + 1024
            },
        )
        .await
        .map_err(VoiceError::Tts)?;
        if audio {
            return Ok(bytes);
        }
        let json: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|_| VoiceError::Tts("LM Studio TTS returned non-audio response".into()))?;
        let encoded = json
            .get("audio")
            .or_else(|| json.get("data"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                // Sem eco do corpo: o resumo antigo cortava por byte e dava
                // panic em caractere multibyte, e o corpo nao e confiavel.
                VoiceError::Tts("LM Studio TTS returned non-audio response".into())
            })?;
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| VoiceError::Tts("invalid local speech audio".into()))?;
        if bytes.len() > MAX_AUDIO_BYTES {
            return Err(VoiceError::Tts(
                "speech audio exceeds the size limit".into(),
            ));
        }
        Ok(bytes)
    }

    pub async fn health_check(&self) -> Result<bool, VoiceError> {
        Ok(self
            .client
            .get(api_url(&self.endpoint, "/v1/models"))
            .send()
            .await
            .is_ok_and(|r| r.status().is_success()))
    }
}
