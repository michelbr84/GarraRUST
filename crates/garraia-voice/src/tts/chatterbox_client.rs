use crate::pipeline::VoiceError;

/// Async HTTP client for the Chatterbox Multilingual TTS (Gradio) service.
///
/// Calls the Gradio `/gradio_api/call/generate_tts_audio` endpoint to synthesize
/// speech from text. The Chatterbox server must be running (e.g. via
/// `run_multilingual.bat`) before using this client.
#[derive(Clone)]
pub struct ChatterboxClient {
    endpoint: String,
    client: reqwest::Client,
    language: String,
}

impl ChatterboxClient {
    /// Create a new Chatterbox client.
    ///
    /// # Arguments
    /// * `endpoint` – base URL of the Gradio app, e.g. `http://127.0.0.1:7860`
    /// * `language` – default language code (e.g. `"pt"` for Portuguese)
    pub fn new(endpoint: &str, language: &str) -> Self {
        Self {
            endpoint: endpoint.trim_end_matches('/').to_string(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            language: language.to_string(),
        }
    }

    /// Check if the Chatterbox server is reachable.
    ///
    /// Tries root endpoint first (works on custom Chatterbox builds like multilingual),
    /// then falls back to `/gradio_api/config` (standard Gradio app).
    pub async fn health_check(&self) -> Result<bool, VoiceError> {
        // Try root endpoint first (custom Chatterbox multilingual returns 200 here)
        let root_url = format!("{}/", self.endpoint);
        match self.client.get(&root_url).send().await {
            Ok(resp) if resp.status().is_success() => return Ok(true),
            _ => {}
        }

        // Fallback: standard Gradio config endpoint
        let config_url = format!("{}/gradio_api/config", self.endpoint);
        match self.client.get(&config_url).send().await {
            Ok(resp) => Ok(resp.status().is_success()),
            Err(e) => {
                tracing::warn!("Chatterbox health check failed: {e}");
                Ok(false)
            }
        }
    }

    /// Synthesize speech from text using the Chatterbox Multilingual model.
    ///
    /// Returns the raw audio bytes (WAV format).
    ///
    /// # Arguments
    /// * `text` – the text to speak
    /// * `language` – optional language override (uses default if `None`)
    pub async fn synthesize(
        &self,
        text: &str,
        language: Option<&str>,
    ) -> Result<Vec<u8>, VoiceError> {
        let lang = language.unwrap_or(&self.language);

        // Step 1: Submit the generation job via Gradio's call API
        let submit_url = format!("{}/gradio_api/call/generate_tts_audio", self.endpoint);
        let body = serde_json::json!({
            "data": [text, lang, null, 1.0]
        });

        let resp = self
            .client
            .post(&submit_url)
            .json(&body)
            .send()
            .await
            .map_err(|e| VoiceError::Tts(format!("Chatterbox submit failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(VoiceError::Tts(format!(
                "Chatterbox submit returned {status}: {body}"
            )));
        }

        #[derive(serde::Deserialize)]
        struct SubmitResponse {
            event_id: String,
        }

        let submit_result: SubmitResponse = resp
            .json()
            .await
            .map_err(|e| VoiceError::Tts(format!("Failed to parse Chatterbox submit response: {e}")))?;

        // Step 2: Poll for the result using the event_id
        let result_url = format!(
            "{}/gradio_api/call/generate_tts_audio/{}",
            self.endpoint, submit_result.event_id
        );

        let event_resp = self
            .client
            .get(&result_url)
            .send()
            .await
            .map_err(|e| VoiceError::Tts(format!("Chatterbox result poll failed: {e}")))?;

        if !event_resp.status().is_success() {
            let status = event_resp.status();
            let body = event_resp.text().await.unwrap_or_default();
            return Err(VoiceError::Tts(format!(
                "Chatterbox result returned {status}: {body}"
            )));
        }

        // The event stream returns SSE lines. Parse for the "complete" event
        // containing the audio file path.
        let event_text = event_resp
            .text()
            .await
            .map_err(|e| VoiceError::Tts(format!("Failed to read Chatterbox event stream: {e}")))?;

        // Parse SSE: look for "data:" line after "event: complete"
        let audio_url = Self::parse_audio_url_from_sse(&event_text, &self.endpoint)?;

        // Step 3: Download the audio file
        let audio_resp = self
            .client
            .get(&audio_url)
            .send()
            .await
            .map_err(|e| VoiceError::Tts(format!("Failed to download audio: {e}")))?;

        if !audio_resp.status().is_success() {
            let status = audio_resp.status();
            return Err(VoiceError::Tts(format!(
                "Audio download returned {status}"
            )));
        }

        let audio_bytes = audio_resp
            .bytes()
            .await
            .map_err(|e| VoiceError::Tts(format!("Failed to read audio bytes: {e}")))?;

        tracing::info!(
            bytes = audio_bytes.len(),
            lang,
            text_len = text.len(),
            "Chatterbox TTS synthesis complete"
        );

        Ok(audio_bytes.to_vec())
    }

    /// Parse the audio file URL from the Gradio SSE event stream.
    fn parse_audio_url_from_sse(sse_text: &str, endpoint: &str) -> Result<String, VoiceError> {
        // SSE format:
        //   event: complete
        //   data: [{"path": "/tmp/gradio/.../audio.wav", "url": "...", ...}]
        let mut found_complete = false;
        for line in sse_text.lines() {
            if line.starts_with("event: complete") {
                found_complete = true;
                continue;
            }
            if found_complete && line.starts_with("data: ") {
                let data = &line["data: ".len()..];
                // Try parsing as JSON array
                if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(data) {
                    // The audio output is typically the first element
                    if let Some(obj) = arr.first() {
                        // Check for "url" field first
                        if let Some(url) = obj.get("url").and_then(|v| v.as_str()) {
                            return Ok(url.to_string());
                        }
                        // Fall back to constructing from "path"
                        if let Some(path) = obj.get("path").and_then(|v| v.as_str()) {
                            return Ok(format!("{}/file={}", endpoint, path));
                        }
                    }
                }
                // Maybe data is a single object instead of array
                if let Ok(obj) = serde_json::from_str::<serde_json::Value>(data) {
                    if let Some(url) = obj.get("url").and_then(|v| v.as_str()) {
                        return Ok(url.to_string());
                    }
                    if let Some(path) = obj.get("path").and_then(|v| v.as_str()) {
                        return Ok(format!("{}/file={}", endpoint, path));
                    }
                }
            }
        }

        Err(VoiceError::Tts(format!(
            "Could not parse audio URL from Chatterbox SSE response: {}",
            &sse_text[..sse_text.len().min(500)]
        )))
    }
}
