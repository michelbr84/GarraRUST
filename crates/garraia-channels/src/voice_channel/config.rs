//! Configuration for Voice channel.

use serde::{Deserialize, Serialize};

/// Voice channel configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceConfig {
    /// Speech-to-Text provider configuration.
    pub stt_provider: SttProvider,

    /// Text-to-Speech provider configuration.
    pub tts_provider: TtsProvider,
}

/// Supported Speech-to-Text providers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SttProvider {
    /// OpenAI Whisper API (cloud).
    WhisperApi {
        /// OpenAI API key.
        api_key: String,
        /// Model name (e.g., "whisper-1").
        #[serde(default = "default_whisper_model")]
        model: String,
    },
    /// Local Whisper server (self-hosted, e.g., faster-whisper).
    WhisperLocal {
        /// Endpoint URL for the local Whisper server.
        endpoint: String,
    },
}

/// Supported Text-to-Speech providers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TtsProvider {
    /// ElevenLabs TTS API.
    ElevenLabs {
        /// ElevenLabs API key.
        api_key: String,
        /// Voice ID to use.
        voice_id: String,
        /// Model ID (optional, e.g., "eleven_multilingual_v2").
        model_id: Option<String>,
    },
    /// Kokoro TTS (local/self-hosted).
    Kokoro {
        /// Endpoint URL for the Kokoro TTS server.
        endpoint: String,
    },
    /// Chatterbox TTS (local/self-hosted).
    Chatterbox {
        /// Endpoint URL for the Chatterbox TTS server.
        endpoint: String,
    },
}

/// Supported audio formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioFormat {
    /// Opus codec (WebM container, used by Telegram voice messages).
    Opus,
    /// WAV (uncompressed PCM).
    Wav,
    /// MP3.
    Mp3,
}

impl AudioFormat {
    /// File extension for this format.
    pub fn extension(&self) -> &str {
        match self {
            AudioFormat::Opus => "opus",
            AudioFormat::Wav => "wav",
            AudioFormat::Mp3 => "mp3",
        }
    }

    /// MIME type for this format.
    pub fn mime_type(&self) -> &str {
        match self {
            AudioFormat::Opus => "audio/opus",
            AudioFormat::Wav => "audio/wav",
            AudioFormat::Mp3 => "audio/mpeg",
        }
    }
}

fn default_whisper_model() -> String {
    "whisper-1".to_string()
}
