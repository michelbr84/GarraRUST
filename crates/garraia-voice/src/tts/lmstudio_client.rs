use crate::pipeline::VoiceError;
use std::path::{Path, PathBuf};

/// Cliente TTS que usa modelos de voz via LM Studio.
///
/// LM Studio expõe modelos TTS (como `vieneu-tts-v2-turbo`) através de uma
/// API compatível com OpenAI:
///   - `POST /v1/audio/speech` (OpenAI-compatible) — preferido
///   - `POST /api/v1/chat` (LM Studio specific) — fallback
///
/// O cliente tenta `/v1/audio/speech` primeiro. Se não disponível,
/// usa `/api/v1/chat` com o modelo TTS.
#[derive(Clone)]
pub struct LmStudioTtsClient {
    endpoint: String,
    client: reqwest::Client,
    model: String,
    language: String,
}

impl LmStudioTtsClient {
    pub fn new(endpoint: &str, model: &str, language: &str) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .unwrap_or_default();

        Self {
            endpoint: endpoint.trim_end_matches('/').to_string(),
            client,
            model: model.to_string(),
            language: language.to_string(),
        }
    }

    /// Sintetiza texto em áudio WAV via LM Studio TTS.
    pub async fn synthesize(&self, text: &str, output_path: &Path) -> Result<PathBuf, VoiceError> {
        // Tenta OpenAI-compatible endpoint primeiro
        match self.synthesize_openai_compat(text, output_path).await {
            Ok(path) => return Ok(path),
            Err(e) => {
                tracing::debug!("OpenAI-compat TTS failed, trying chat endpoint: {e}");
            }
        }

        // Fallback: usa /api/v1/chat (LM Studio specific)
        self.synthesize_via_chat(text, output_path).await
    }

    /// Tenta via `POST /v1/audio/speech` (OpenAI-compatible).
    async fn synthesize_openai_compat(
        &self,
        text: &str,
        output_path: &Path,
    ) -> Result<PathBuf, VoiceError> {
        let url = format!("{}/v1/audio/speech", self.endpoint);

        let body = serde_json::json!({
            "model": self.model,
            "input": text,
            "voice": "alloy",
            "response_format": "wav",
        });

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| VoiceError::Tts(format!("LM Studio TTS request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            return Err(VoiceError::Tts(format!(
                "LM Studio TTS returned {status}: {body_text}"
            )));
        }

        let audio_bytes = resp
            .bytes()
            .await
            .map_err(|e| VoiceError::Tts(format!("Failed to read TTS response: {e}")))?;

        tokio::fs::write(output_path, &audio_bytes)
            .await
            .map_err(|e| VoiceError::Tts(format!("Failed to write WAV: {e}")))?;

        tracing::info!(
            bytes = audio_bytes.len(),
            model = %self.model,
            path = %output_path.display(),
            "LM Studio TTS synthesis complete (openai-compat)"
        );

        Ok(output_path.to_path_buf())
    }

    /// Fallback via `POST /api/v1/chat` (LM Studio specific TTS).
    ///
    /// Alguns modelos TTS no LM Studio usam o endpoint de chat com
    /// `system_prompt` e `input` em vez do endpoint de áudio padrão.
    async fn synthesize_via_chat(
        &self,
        text: &str,
        output_path: &Path,
    ) -> Result<PathBuf, VoiceError> {
        let url = format!("{}/api/v1/chat", self.endpoint);

        let body = serde_json::json!({
            "model": self.model,
            "system_prompt": format!(
                "You are a text-to-speech engine. Speak naturally in {}.",
                self.language
            ),
            "input": text,
        });

        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| VoiceError::Tts(format!("LM Studio chat TTS failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            return Err(VoiceError::Tts(format!(
                "LM Studio chat TTS returned {status}: {body_text}"
            )));
        }

        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if content_type.contains("audio") {
            // Resposta é áudio direto
            let audio_bytes = resp
                .bytes()
                .await
                .map_err(|e| VoiceError::Tts(format!("Failed to read audio: {e}")))?;

            tokio::fs::write(output_path, &audio_bytes)
                .await
                .map_err(|e| VoiceError::Tts(format!("Failed to write WAV: {e}")))?;

            tracing::info!(
                bytes = audio_bytes.len(),
                model = %self.model,
                path = %output_path.display(),
                "LM Studio TTS synthesis complete (chat endpoint)"
            );

            Ok(output_path.to_path_buf())
        } else {
            // Resposta é JSON — extrair áudio base64 se disponível
            let body_text = resp.text().await.unwrap_or_default();

            // Tentar parsear como JSON com campo de áudio
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body_text) {
                // Checar campos comuns de áudio em respostas TTS
                if let Some(audio_b64) = json
                    .get("audio")
                    .or_else(|| json.get("data"))
                    .and_then(|v| v.as_str())
                {
                    use base64::Engine;
                    let audio_bytes = base64::engine::general_purpose::STANDARD
                        .decode(audio_b64)
                        .map_err(|e| {
                            VoiceError::Tts(format!("Failed to decode base64 audio: {e}"))
                        })?;

                    tokio::fs::write(output_path, &audio_bytes)
                        .await
                        .map_err(|e| VoiceError::Tts(format!("Failed to write WAV: {e}")))?;

                    return Ok(output_path.to_path_buf());
                }
            }

            Err(VoiceError::Tts(format!(
                "LM Studio TTS returned non-audio response: {}",
                &body_text[..body_text.len().min(200)]
            )))
        }
    }

    /// Health check — verifica se o endpoint está acessível.
    pub async fn health_check(&self) -> Result<bool, VoiceError> {
        let url = format!("{}/v1/models", self.endpoint);
        match self.client.get(&url).send().await {
            Ok(resp) => Ok(resp.status().is_success()),
            Err(_) => Ok(false),
        }
    }
}
