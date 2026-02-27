use crate::pipeline::VoiceError;
use std::path::Path;

/// Cliente HTTP assíncrono para o serviço Whisper (STT).
#[derive(Clone)]
pub struct WhisperClient {
    endpoint: String,
    client: reqwest::Client,
    language: String,
}

impl WhisperClient {
    pub fn new(endpoint: &str, language: &str) -> Self {
        Self {
            endpoint: endpoint.trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
            language: language.to_string(),
        }
    }

    /// Envia um arquivo WAV ao Whisper e retorna o texto transcrito.
    pub async fn transcribe(&self, audio_path: &Path) -> Result<String, VoiceError> {
        let file_bytes = tokio::fs::read(audio_path)
            .await
            .map_err(|e| VoiceError::Stt(format!("Failed to read audio file: {e}")))?;

        let file_name = audio_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name(file_name)
            .mime_str("audio/wav")
            .map_err(|e| VoiceError::Stt(format!("Mime error: {e}")))?;

        let form = reqwest::multipart::Form::new()
            .part("file", part)
            .text("language", self.language.clone());

        let url = format!("{}/v1/audio/transcriptions", self.endpoint);

        let resp = self
            .client
            .post(&url)
            .multipart(form)
            .send()
            .await
            .map_err(|e| VoiceError::Stt(format!("Whisper request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(VoiceError::Stt(format!(
                "Whisper returned {status}: {body}"
            )));
        }

        #[derive(serde::Deserialize)]
        struct TranscribeResponse {
            text: String,
        }

        let result: TranscribeResponse = resp
            .json()
            .await
            .map_err(|e| VoiceError::Stt(format!("Failed to parse Whisper response: {e}")))?;

        tracing::info!(
            text_len = result.text.len(),
            path = %audio_path.display(),
            "Whisper transcription complete"
        );

        Ok(result.text)
    }
}
