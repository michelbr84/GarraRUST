use crate::http::{MAX_AUDIO_BYTES, MAX_METADATA_BYTES, MAX_TEXT_BYTES, api_url, bounded_body};
use crate::pipeline::VoiceError;
use std::path::Path;

/// Whisper-compatible STT. Unsupported whisper.cpp routes fall back to the
/// OpenAI-compatible route on the same configured server only.
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
            client: crate::http::client(),
            language: language.to_string(),
        }
    }

    pub async fn transcribe(&self, audio_path: &Path) -> Result<String, VoiceError> {
        let metadata = tokio::fs::metadata(audio_path).await?;
        if metadata.len() > MAX_AUDIO_BYTES as u64 {
            return Err(VoiceError::Stt("audio exceeds the size limit".into()));
        }
        let bytes = tokio::fs::read(audio_path).await?;
        let filename = audio_path.file_name().unwrap_or_default().to_string_lossy();
        self.transcribe_bytes(&bytes, &filename, "audio/wav", None, None)
            .await
    }

    /// No temporary files or shared mutable settings. A model override selects
    /// the OpenAI-compatible route; whisper.cpp loads models outside requests.
    pub async fn transcribe_bytes(
        &self,
        audio: &[u8],
        filename: &str,
        content_type: &str,
        language: Option<&str>,
        model: Option<&str>,
    ) -> Result<String, VoiceError> {
        if audio.is_empty() || audio.len() > MAX_AUDIO_BYTES {
            return Err(VoiceError::Stt(
                "audio is empty or exceeds the size limit".into(),
            ));
        }
        let language = language
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&self.language);
        if language.len() > 64 || model.is_some_and(|s| s.len() > 256) {
            return Err(VoiceError::Stt("invalid language or model".into()));
        }
        let model = model.filter(|s| !s.trim().is_empty());
        if model.is_none() {
            let response = self
                .request(audio, filename, content_type, language, None)
                .await?;
            if !matches!(response.status().as_u16(), 404 | 405 | 501) {
                return Self::transcription(response, true).await;
            }
        }
        let response = self
            .request(
                audio,
                filename,
                content_type,
                language,
                Some(model.unwrap_or("whisper-1")),
            )
            .await?;
        Self::transcription(response, false).await
    }

    async fn request(
        &self,
        audio: &[u8],
        filename: &str,
        content_type: &str,
        language: &str,
        model: Option<&str>,
    ) -> Result<reqwest::Response, VoiceError> {
        let part = reqwest::multipart::Part::bytes(audio.to_vec())
            .file_name(filename.to_string())
            .mime_str(content_type)
            .map_err(|_| VoiceError::Stt("invalid audio content type".into()))?;
        let mut form = reqwest::multipart::Form::new()
            .part("file", part)
            .text("language", language.to_string())
            .text("response_format", "json");
        let path = if let Some(model) = model {
            form = form.text("model", model.to_string());
            "/v1/audio/transcriptions"
        } else {
            "/inference"
        };
        self.client
            .post(api_url(&self.endpoint, path))
            .multipart(form)
            .send()
            .await
            .map_err(|e| {
                VoiceError::Stt(
                    if e.is_timeout() {
                        "transcription timed out"
                    } else {
                        "transcription request failed"
                    }
                    .into(),
                )
            })
    }

    async fn transcription(
        response: reqwest::Response,
        allow_plain: bool,
    ) -> Result<String, VoiceError> {
        if !response.status().is_success() {
            return Err(VoiceError::Stt(format!(
                "transcription server returned HTTP {}",
                response.status().as_u16()
            )));
        }
        let body = bounded_body(response, MAX_METADATA_BYTES)
            .await
            .map_err(VoiceError::Stt)?;
        let text = match serde_json::from_slice::<serde_json::Value>(&body) {
            Ok(json) => json
                .get("text")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .ok_or_else(|| VoiceError::Stt("transcription response has no text".into()))?,
            Err(_) if allow_plain => String::from_utf8(body)
                .map_err(|_| VoiceError::Stt("invalid transcription encoding".into()))?,
            Err(_) => return Err(VoiceError::Stt("invalid transcription response".into())),
        };
        if text.len() > MAX_TEXT_BYTES {
            return Err(VoiceError::Stt(
                "transcription exceeds the text limit".into(),
            ));
        }
        tracing::info!(text_len = text.len(), "voice transcription complete");
        Ok(text.trim().to_string())
    }

    pub async fn health_check(&self) -> Result<bool, VoiceError> {
        Ok(self
            .client
            .get(format!("{}/", self.endpoint))
            .send()
            .await
            .is_ok_and(|r| r.status().is_success()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn timeout_does_not_start_a_second_transcription() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 4096];
            // On a slow runner the client may time out before sending the
            // request (read returns 0), so the byte count is not asserted:
            // the invariant is only that no second connection follows (#1829).
            let _ = socket.read(&mut request).await;
            // A fallback would open another connection and hide the original
            // timeout.
            assert!(
                tokio::time::timeout(Duration::from_millis(500), listener.accept())
                    .await
                    .is_err()
            );
            drop(socket);
        });
        let mut client = WhisperClient::new(&endpoint, "pt");
        client.client = reqwest::Client::builder()
            .timeout(Duration::from_millis(100))
            .build()
            .unwrap();
        let error = client
            .transcribe_bytes(b"audio", "audio.wav", "audio/wav", None, None)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("timed out"));
        server.await.unwrap();
    }
}
