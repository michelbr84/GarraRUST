use crate::pipeline::VoiceError;
use std::path::Path;

/// Cliente HTTP assíncrono para serviços STT compatíveis com Whisper.
///
/// Suporta dois backends:
///   - **whisper.cpp server** (`POST /inference`) — servidor local via whisper.cpp
///   - **OpenAI-compatible** (`POST /v1/audio/transcriptions`) — OpenAI, LM Studio, etc.
///
/// O cliente tenta `/inference` primeiro (whisper.cpp), e se falhar
/// tenta `/v1/audio/transcriptions` (OpenAI-compatible).
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

    /// Envia um arquivo de áudio ao servidor Whisper e retorna o texto transcrito.
    ///
    /// Tenta whisper.cpp (`/inference`) primeiro, depois OpenAI-compat (`/v1/audio/transcriptions`).
    pub async fn transcribe(&self, audio_path: &Path) -> Result<String, VoiceError> {
        // Tenta whisper.cpp server primeiro
        match self.transcribe_whispercpp(audio_path).await {
            Ok(text) => return Ok(text),
            Err(e) => {
                tracing::debug!("whisper.cpp endpoint failed, trying OpenAI-compat: {e}");
            }
        }

        // Fallback: OpenAI-compatible endpoint
        self.transcribe_openai_compat(audio_path).await
    }

    /// whisper.cpp server: `POST /inference` com multipart form (campo `file`).
    ///
    /// Resposta: JSON `{ "text": "..." }` ou texto puro.
    async fn transcribe_whispercpp(&self, audio_path: &Path) -> Result<String, VoiceError> {
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
            .text("language", self.language.clone())
            .text("response_format", "json".to_string());

        let url = format!("{}/inference", self.endpoint);

        let resp = self
            .client
            .post(&url)
            .multipart(form)
            .send()
            .await
            .map_err(|e| VoiceError::Stt(format!("whisper.cpp request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(VoiceError::Stt(format!(
                "whisper.cpp returned {status}: {body}"
            )));
        }

        let body = resp.text().await
            .map_err(|e| VoiceError::Stt(format!("Failed to read response: {e}")))?;

        // whisper.cpp pode retornar JSON {"text": "..."} ou texto puro
        let text = if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
            json.get("text")
                .and_then(|v| v.as_str())
                .unwrap_or(&body)
                .trim()
                .to_string()
        } else {
            body.trim().to_string()
        };

        tracing::info!(
            text_len = text.len(),
            path = %audio_path.display(),
            "whisper.cpp transcription complete"
        );

        Ok(text)
    }

    /// OpenAI-compatible: `POST /v1/audio/transcriptions` (OpenAI, LM Studio, faster-whisper).
    async fn transcribe_openai_compat(&self, audio_path: &Path) -> Result<String, VoiceError> {
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
            .text("language", self.language.clone())
            .text("model", "whisper-1".to_string());

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
            "OpenAI-compat transcription complete"
        );

        Ok(result.text)
    }

    /// Health check — verifica se o endpoint de transcrição está acessível.
    pub async fn health_check(&self) -> Result<bool, VoiceError> {
        let url = format!("{}/", self.endpoint);
        match self.client.get(&url).send().await {
            Ok(resp) => Ok(resp.status().is_success()),
            Err(_) => Ok(false),
        }
    }
}
