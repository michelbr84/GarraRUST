use std::path::{Path, PathBuf};
use crate::pipeline::VoiceError;

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
            client: reqwest::Client::new(),
            language: language.to_string(),
        }
    }

    /// Envia texto ao Hibiki-Zero e salva o áudio WAV resultante em `output_path`.
    pub async fn synthesize(&self, text: &str, output_path: &Path) -> Result<PathBuf, VoiceError> {
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
            return Err(VoiceError::Tts(format!(
                "Hibiki returned {status}: {body}"
            )));
        }

        let audio_bytes = resp
            .bytes()
            .await
            .map_err(|e| VoiceError::Tts(format!("Failed to read Hibiki response: {e}")))?;

        tokio::fs::write(output_path, &audio_bytes)
            .await
            .map_err(|e| VoiceError::Tts(format!("Failed to write WAV: {e}")))?;

        tracing::info!(
            bytes = audio_bytes.len(),
            path = %output_path.display(),
            "Hibiki synthesis complete"
        );

        Ok(output_path.to_path_buf())
    }
}
