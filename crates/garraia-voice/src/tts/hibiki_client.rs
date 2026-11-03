use crate::http::{MAX_AUDIO_BYTES, MAX_TEXT_BYTES, api_url, bounded_body};
use crate::pipeline::VoiceError;
use std::path::{Path, PathBuf};

/// Cliente HTTP assincrono para o servico Hibiki (diagnostico legado).
///
/// O `services/voice/hibiki_server.py` do repositorio ainda devolve um tom
/// de teste: um HTTP 200 dele nao prova sintese de fala (ADR 0027).
#[derive(Clone)]
pub struct HibikiClient {
    endpoint: String,
    client: reqwest::Client,
    language: String,
}

impl HibikiClient {
    pub fn new(endpoint: &str, language: &str) -> Self {
        Self {
            endpoint: endpoint.trim_end_matches('/').to_string(),
            client: crate::http::client(),
            language: language.to_string(),
        }
    }

    /// Envia texto ao Hibiki e devolve o audio em memoria, no idioma
    /// configurado. Sem arquivo intermediario: cada chamada fica com os
    /// proprios bytes (#1754).
    pub async fn synthesize_to_bytes(&self, text: &str) -> Result<Vec<u8>, VoiceError> {
        self.synthesize_bytes(text, None).await
    }

    pub async fn synthesize(&self, text: &str, output_path: &Path) -> Result<PathBuf, VoiceError> {
        let bytes = self.synthesize_bytes(text, None).await?;
        tokio::fs::write(output_path, bytes).await?;
        Ok(output_path.to_path_buf())
    }

    /// In-memory synthesis, including a per-request language override.
    pub async fn synthesize_bytes(
        &self,
        text: &str,
        language: Option<&str>,
    ) -> Result<Vec<u8>, VoiceError> {
        if text.trim().is_empty() || text.len() > MAX_TEXT_BYTES {
            return Err(VoiceError::Tts(
                "text is empty or exceeds the size limit".into(),
            ));
        }
        if language.is_some_and(|s| s.len() > 64) {
            return Err(VoiceError::Tts("invalid language".into()));
        }
        let body = serde_json::json!({
            "text": text,
            "language": language.unwrap_or(&self.language),
            "sample_rate": 44100,
        });
        let response = self
            .client
            .post(api_url(&self.endpoint, "/v1/audio/speech"))
            .json(&body)
            .send()
            .await
            .map_err(|_| VoiceError::Tts("Hibiki request failed".into()))?;
        if !response.status().is_success() {
            return Err(VoiceError::Tts(format!(
                "Hibiki returned HTTP {}",
                response.status().as_u16()
            )));
        }
        bounded_body(response, MAX_AUDIO_BYTES)
            .await
            .map_err(VoiceError::Tts)
    }
}
