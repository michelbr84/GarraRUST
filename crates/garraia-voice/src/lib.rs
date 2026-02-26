//! # garraia-voice
//!
//! Sistema de voz do GarraIA.
//!
//! Arquitetura:
//!   Whisper Large v3 (STT) → LLM (cérebro) → Hibiki-Zero-3B + MioCodec (TTS)
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
