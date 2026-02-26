use std::path::{Path, PathBuf};
use crate::pipeline::VoiceError;

/// Converte áudio entre formatos usando ffmpeg.
pub struct AudioConverter;

impl AudioConverter {
    /// Converte .ogg (Opus) → .wav (PCM 16kHz mono) para o Whisper.
    pub async fn ogg_to_wav(input: &Path) -> Result<PathBuf, VoiceError> {
        let output = input.with_extension("wav");

        let status = tokio::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-i", &input.to_string_lossy(),
                "-ar", "16000",
                "-ac", "1",
                "-f", "wav",
                &output.to_string_lossy(),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .map_err(|e| VoiceError::AudioConversion(format!(
                "Failed to run ffmpeg (ogg→wav): {e}. Is ffmpeg installed?"
            )))?;

        if !status.success() {
            return Err(VoiceError::AudioConversion(format!(
                "ffmpeg ogg→wav exited with status {status}"
            )));
        }

        tracing::debug!(
            input = %input.display(),
            output = %output.display(),
            "Converted ogg → wav (16kHz mono)"
        );

        Ok(output)
    }

    /// Converte .wav (44.1kHz) → .ogg (Opus) para enviar via Telegram.
    pub async fn wav_to_ogg(input: &Path) -> Result<PathBuf, VoiceError> {
        let output = input.with_extension("ogg");

        let status = tokio::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-i", &input.to_string_lossy(),
                "-c:a", "libopus",
                "-b:a", "128k",
                "-ar", "48000",
                "-ac", "1",
                "-f", "ogg",
                &output.to_string_lossy(),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .map_err(|e| VoiceError::AudioConversion(format!(
                "Failed to run ffmpeg (wav→ogg): {e}. Is ffmpeg installed?"
            )))?;

        if !status.success() {
            return Err(VoiceError::AudioConversion(format!(
                "ffmpeg wav→ogg exited with status {status}"
            )));
        }

        tracing::debug!(
            input = %input.display(),
            output = %output.display(),
            "Converted wav → ogg (Opus 128k)"
        );

        Ok(output)
    }
}
