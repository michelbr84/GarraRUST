//! # garraia-voice
//!
//! Sistema de voz do GarraIA.
//!
//! Arquitetura:
//!   Whisper (STT) → LLM (cérebro) → TTS (Chatterbox/Hibiki/LMStudio)
//!
//! Interface principal:
//! ```ignore
//! let pipeline = VoicePipeline::new(whisper, hibiki, work_dir);
//! let (output_ogg, metrics) = pipeline.process_voice(&input_ogg, |text| async {
//!     // envia texto ao LLM e retorna resposta
//!     Ok(llm_response)
//! }).await?;
//! ```

pub mod audio;
pub mod pipeline;
pub mod stt;
pub mod tts;

// Re-exports para conveniência
pub use pipeline::{VoiceError, VoiceMetrics, VoicePipeline};
pub use stt::whisper_client::WhisperClient;
pub use tts::chatterbox_client::ChatterboxClient;
pub use tts::hibiki_client::HibikiClient;
pub use tts::lmstudio_client::LmStudioTtsClient;

/// Trait unificado para provedores TTS.
///
/// Retorna bytes de áudio WAV cru.
#[async_trait::async_trait]
pub trait TtsSynthesizer: Send + Sync {
    async fn synthesize_bytes(&self, text: &str, language: &str) -> Result<Vec<u8>, VoiceError>;
}

#[async_trait::async_trait]
impl TtsSynthesizer for ChatterboxClient {
    async fn synthesize_bytes(&self, text: &str, language: &str) -> Result<Vec<u8>, VoiceError> {
        self.synthesize(text, Some(language)).await
    }
}

#[async_trait::async_trait]
impl TtsSynthesizer for HibikiClient {
    async fn synthesize_bytes(&self, text: &str, _language: &str) -> Result<Vec<u8>, VoiceError> {
        let tmp = std::env::temp_dir().join(format!("garraia_tts_{}.wav", std::process::id()));
        self.synthesize(text, &tmp).await?;
        let bytes = tokio::fs::read(&tmp)
            .await
            .map_err(|e| VoiceError::Tts(e.to_string()))?;
        let _ = tokio::fs::remove_file(&tmp).await;
        Ok(bytes)
    }
}

#[async_trait::async_trait]
impl TtsSynthesizer for LmStudioTtsClient {
    async fn synthesize_bytes(&self, text: &str, _language: &str) -> Result<Vec<u8>, VoiceError> {
        let tmp = std::env::temp_dir().join(format!("garraia_tts_{}.wav", std::process::id()));
        self.synthesize(text, &tmp).await?;
        let bytes = tokio::fs::read(&tmp)
            .await
            .map_err(|e| VoiceError::Tts(e.to_string()))?;
        let _ = tokio::fs::remove_file(&tmp).await;
        Ok(bytes)
    }
}
