use crate::pipeline::VoiceError;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Cliente HTTP assíncrono para o serviço Hibiki-Zero-3B (TTS).
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
            // #1754: sem timeout, um servidor travado prendia a chamada para
            // sempre. 120 s, como o Chatterbox e o LM Studio.
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            language: language.to_string(),
        }
    }

    /// Envia texto ao Hibiki-Zero e devolve o áudio WAV em memória.
    ///
    /// É o caminho do [`crate::TtsSynthesizer`]: sem arquivo intermediário,
    /// cada chamada fica com os próprios bytes (#1754).
    pub async fn synthesize_to_bytes(&self, text: &str) -> Result<Vec<u8>, VoiceError> {
        let body = serde_json::json!({
            "text": text,
            "language": self.language,
            "sample_rate": 44100,
        });

        let url = format!("{}/v1/audio/speech", self.endpoint);

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| VoiceError::Tts(format!("Hibiki request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(VoiceError::Tts(format!("Hibiki returned {status}: {body}")));
        }

        let audio_bytes = resp
            .bytes()
            .await
            .map_err(|e| VoiceError::Tts(format!("Failed to read Hibiki response: {e}")))?;

        tracing::info!(bytes = audio_bytes.len(), "Hibiki synthesis complete");

        Ok(audio_bytes.to_vec())
    }

    /// Envia texto ao Hibiki-Zero e salva o áudio WAV resultante em `output_path`.
    pub async fn synthesize(&self, text: &str, output_path: &Path) -> Result<PathBuf, VoiceError> {
        let audio_bytes = self.synthesize_to_bytes(text).await?;

        tokio::fs::write(output_path, &audio_bytes)
            .await
            .map_err(|e| VoiceError::Tts(format!("Failed to write WAV: {e}")))?;

        tracing::debug!(path = %output_path.display(), "Hibiki WAV written");

        Ok(output_path.to_path_buf())
    }
}
