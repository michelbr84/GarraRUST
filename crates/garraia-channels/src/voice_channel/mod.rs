//! Voice integration channel for GarraIA.
//!
//! Provides a `VoiceChannel` struct that implements the `Channel` trait,
//! with a pipeline: receive audio -> STT -> process -> TTS -> send audio.

pub mod config;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use reqwest::Client;
use tokio::sync::mpsc;
use tracing::info;

use crate::traits::{Channel, ChannelStatus};
use garraia_common::{Error, Message, MessageContent, Result};

pub use config::{AudioFormat, SttProvider, TtsProvider, VoiceConfig};

/// Callback invoked when a transcribed text message is ready for processing.
///
/// Arguments: `(session_id, user_id, transcribed_text, delta_tx)`.
/// Return the response text which will be converted to audio via TTS.
pub type VoiceOnMessageFn = Arc<
    dyn Fn(
            String,
            String,
            String,
            Option<mpsc::Sender<String>>,
        ) -> Pin<Box<dyn Future<Output = std::result::Result<String, String>> + Send>>
        + Send
        + Sync,
>;

/// Voice integration channel.
///
/// Orchestrates the audio pipeline:
/// 1. Receive audio data
/// 2. Transcribe via STT (Speech-to-Text)
/// 3. Process transcribed text through the AI
/// 4. Convert response to audio via TTS (Text-to-Speech)
/// 5. Return audio data
pub struct VoiceChannel {
    config: VoiceConfig,
    client: Client,
    status: ChannelStatus,
    on_message: VoiceOnMessageFn,
}

impl VoiceChannel {
    /// Create a new `VoiceChannel` from config and callback.
    pub fn new(config: VoiceConfig, on_message: VoiceOnMessageFn) -> Self {
        Self {
            config,
            client: Client::new(),
            status: ChannelStatus::Disconnected,
            on_message,
        }
    }

    /// Access the current config.
    pub fn config(&self) -> &VoiceConfig {
        &self.config
    }

    /// Transcribe audio data to text using the configured STT provider.
    pub async fn transcribe(&self, audio_data: &[u8], format: AudioFormat) -> Result<String> {
        match &self.config.stt_provider {
            SttProvider::WhisperApi { api_key, model } => {
                self.transcribe_whisper_api(audio_data, format, api_key, model)
                    .await
            }
            SttProvider::WhisperLocal { endpoint } => {
                self.transcribe_whisper_local(audio_data, format, endpoint)
                    .await
            }
        }
    }

    /// Synthesize text to audio using the configured TTS provider.
    pub async fn synthesize(&self, text: &str) -> Result<Vec<u8>> {
        match &self.config.tts_provider {
            TtsProvider::ElevenLabs {
                api_key,
                voice_id,
                model_id,
            } => {
                self.tts_elevenlabs(text, api_key, voice_id, model_id.as_deref())
                    .await
            }
            TtsProvider::Kokoro { endpoint } => self.tts_kokoro(text, endpoint).await,
            TtsProvider::Chatterbox { endpoint } => self.tts_chatterbox(text, endpoint).await,
        }
    }

    /// Process the full voice pipeline: audio_in -> STT -> AI -> TTS -> audio_out.
    pub async fn process_voice(
        &self,
        session_id: &str,
        user_id: &str,
        audio_data: &[u8],
        input_format: AudioFormat,
    ) -> Result<Vec<u8>> {
        // Step 1: Transcribe audio to text
        let transcription = self.transcribe(audio_data, input_format).await?;
        info!(
            "voice: transcribed {} chars from audio",
            transcription.len()
        );

        if transcription.trim().is_empty() {
            return Err(Error::Channel("voice: empty transcription".into()));
        }

        // Step 2: Process through AI callback
        let response = (self.on_message)(
            session_id.to_string(),
            user_id.to_string(),
            transcription,
            None,
        )
        .await
        .map_err(|e| Error::Channel(format!("voice: AI processing failed: {e}")))?;

        // Step 3: Synthesize response to audio
        let audio_out = self.synthesize(&response).await?;
        info!("voice: synthesized {} bytes of audio", audio_out.len());

        Ok(audio_out)
    }

    // ─── STT Implementations ───────────────────────────────────────────

    async fn transcribe_whisper_api(
        &self,
        audio_data: &[u8],
        format: AudioFormat,
        api_key: &str,
        model: &str,
    ) -> Result<String> {
        let extension = format.extension();
        let filename = format!("audio.{}", extension);

        let part = reqwest::multipart::Part::bytes(audio_data.to_vec())
            .file_name(filename)
            .mime_str(format.mime_type())
            .map_err(|e| Error::Channel(format!("voice: invalid mime type: {e}")))?;

        let form = reqwest::multipart::Form::new()
            .text("model", model.to_string())
            .part("file", part);

        let resp = self
            .client
            .post("https://api.openai.com/v1/audio/transcriptions")
            .bearer_auth(api_key)
            .multipart(form)
            .send()
            .await
            .map_err(|e| Error::Channel(format!("whisper API failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Channel(format!(
                "whisper API error {status}: {body}"
            )));
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| Error::Channel(format!("whisper API parse failed: {e}")))?;

        body.get("text")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| Error::Channel("whisper API: missing text in response".into()))
    }

    async fn transcribe_whisper_local(
        &self,
        audio_data: &[u8],
        format: AudioFormat,
        endpoint: &str,
    ) -> Result<String> {
        let extension = format.extension();
        let filename = format!("audio.{}", extension);

        let part = reqwest::multipart::Part::bytes(audio_data.to_vec())
            .file_name(filename)
            .mime_str(format.mime_type())
            .map_err(|e| Error::Channel(format!("voice: invalid mime type: {e}")))?;

        let form = reqwest::multipart::Form::new().part("file", part);

        let resp = self
            .client
            .post(endpoint)
            .multipart(form)
            .send()
            .await
            .map_err(|e| Error::Channel(format!("whisper local failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Channel(format!(
                "whisper local error {status}: {body}"
            )));
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| Error::Channel(format!("whisper local parse failed: {e}")))?;

        body.get("text")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| Error::Channel("whisper local: missing text in response".into()))
    }

    // ─── TTS Implementations ───────────────────────────────────────────

    async fn tts_elevenlabs(
        &self,
        text: &str,
        api_key: &str,
        voice_id: &str,
        model_id: Option<&str>,
    ) -> Result<Vec<u8>> {
        let url = format!("https://api.elevenlabs.io/v1/text-to-speech/{}", voice_id);

        let mut body = serde_json::json!({
            "text": text,
            "voice_settings": {
                "stability": 0.5,
                "similarity_boost": 0.75
            }
        });

        if let Some(model) = model_id {
            body.as_object_mut()
                .expect("body is object")
                .insert("model_id".into(), serde_json::json!(model));
        }

        let resp = self
            .client
            .post(&url)
            .header("xi-api-key", api_key)
            .header("Accept", "audio/mpeg")
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Channel(format!("elevenlabs TTS failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Channel(format!(
                "elevenlabs TTS error {status}: {body}"
            )));
        }

        resp.bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| Error::Channel(format!("elevenlabs TTS read failed: {e}")))
    }

    async fn tts_kokoro(&self, text: &str, endpoint: &str) -> Result<Vec<u8>> {
        let body = serde_json::json!({
            "text": text,
            "format": "mp3",
        });

        let resp = self
            .client
            .post(endpoint)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Channel(format!("kokoro TTS failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Channel(format!("kokoro TTS error {status}: {body}")));
        }

        resp.bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| Error::Channel(format!("kokoro TTS read failed: {e}")))
    }

    async fn tts_chatterbox(&self, text: &str, endpoint: &str) -> Result<Vec<u8>> {
        let body = serde_json::json!({
            "text": text,
            "format": "wav",
        });

        let resp = self
            .client
            .post(endpoint)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Channel(format!("chatterbox TTS failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Channel(format!(
                "chatterbox TTS error {status}: {body}"
            )));
        }

        resp.bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| Error::Channel(format!("chatterbox TTS read failed: {e}")))
    }
}

#[async_trait]
impl Channel for VoiceChannel {
    fn channel_type(&self) -> &str {
        "voice"
    }

    fn display_name(&self) -> &str {
        "Voice"
    }

    async fn connect(&mut self) -> Result<()> {
        // Voice channel is request-driven, no persistent connection needed.
        self.status = ChannelStatus::Connected;
        info!("voice channel connected");
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        self.status = ChannelStatus::Disconnected;
        info!("voice channel disconnected");
        Ok(())
    }

    async fn send_message(&self, message: &Message) -> Result<()> {
        // Voice channel sends audio, but the Channel trait sends Messages.
        // For text messages, we synthesize to audio and return it via metadata
        // or a separate mechanism. Here we handle the text fallback.
        let text = match &message.content {
            MessageContent::Text(t) => t.clone(),
            _ => {
                return Err(Error::Channel(
                    "voice channel: unsupported message content type".into(),
                ));
            }
        };

        // Synthesize and discard — in practice, the caller uses process_voice()
        // or synthesize() directly for the full pipeline.
        let _audio = self.synthesize(&text).await?;
        Ok(())
    }

    fn status(&self) -> ChannelStatus {
        self.status.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_type_is_voice() {
        let on_msg: VoiceOnMessageFn =
            Arc::new(|_session, _uid, _text, _delta_tx| Box::pin(async { Ok("test".to_string()) }));
        let config = VoiceConfig {
            stt_provider: SttProvider::WhisperLocal {
                endpoint: "http://localhost:9000/v1/audio/transcriptions".into(),
            },
            tts_provider: TtsProvider::Kokoro {
                endpoint: "http://localhost:9001/v1/tts".into(),
            },
        };
        let channel = VoiceChannel::new(config, on_msg);
        assert_eq!(channel.channel_type(), "voice");
        assert_eq!(channel.display_name(), "Voice");
        assert_eq!(channel.status(), ChannelStatus::Disconnected);
    }

    #[test]
    fn audio_format_extensions() {
        assert_eq!(AudioFormat::Opus.extension(), "opus");
        assert_eq!(AudioFormat::Wav.extension(), "wav");
        assert_eq!(AudioFormat::Mp3.extension(), "mp3");
    }

    #[test]
    fn audio_format_mime_types() {
        assert_eq!(AudioFormat::Opus.mime_type(), "audio/opus");
        assert_eq!(AudioFormat::Wav.mime_type(), "audio/wav");
        assert_eq!(AudioFormat::Mp3.mime_type(), "audio/mpeg");
    }
}
