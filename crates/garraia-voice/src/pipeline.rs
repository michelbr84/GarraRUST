//! Pipeline de voz do GarraIA.
//!
//! Fluxo completo:
//!   .ogg (Telegram) → wav (ffmpeg) → texto (Whisper) → LLM → texto resposta
//!   → wav (Hibiki) → .ogg (ffmpeg) → Telegram
//!
//! Interface mínima:
//!   `pipeline.process_voice(&input_ogg).await -> Result<PathBuf>`

use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::audio::converter::AudioConverter;
use crate::stt::whisper_client::WhisperClient;
use crate::tts::hibiki_client::HibikiClient;

// ─── Errors ────────────────────────────────────────────────────────────────

/// Erros possíveis no pipeline de voz.
#[derive(Debug, thiserror::Error)]
pub enum VoiceError {
    #[error("STT error: {0}")]
    Stt(String),

    #[error("TTS error: {0}")]
    Tts(String),

    #[error("Audio conversion error: {0}")]
    AudioConversion(String),

    #[error("LLM callback error: {0}")]
    Llm(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

// ─── Metrics ───────────────────────────────────────────────────────────────

/// Métricas de uma execução do pipeline.
#[derive(Debug, Clone)]
pub struct VoiceMetrics {
    /// Tempo para converter ogg → wav.
    pub ogg_to_wav_ms: u128,
    /// Tempo para transcrever áudio → texto (Whisper).
    pub stt_ms: u128,
    /// Texto transcrito pelo Whisper.
    pub transcribed_text: String,
    /// Tempo para obter resposta do LLM.
    pub llm_ms: u128,
    /// Texto da resposta do LLM.
    pub llm_response: String,
    /// Tempo para sintetizar texto → áudio (Hibiki).
    pub tts_ms: u128,
    /// Tempo para converter wav → ogg.
    pub wav_to_ogg_ms: u128,
    /// Tempo total do pipeline.
    pub total_ms: u128,
}

// ─── Pipeline ──────────────────────────────────────────────────────────────

/// Pipeline de voz completo do GarraIA.
///
/// Orquestra: conversão de áudio, STT (Whisper), callback LLM, TTS (Hibiki).
pub struct VoicePipeline {
    whisper: WhisperClient,
    hibiki: HibikiClient,
    /// Diretório temporário para arquivos intermediários.
    work_dir: PathBuf,
}

impl VoicePipeline {
    /// Cria um novo pipeline de voz.
    ///
    /// # Arguments
    /// * `whisper` – cliente STT (Whisper)
    /// * `hibiki`  – cliente TTS (Hibiki-Zero-3B)
    /// * `work_dir` – diretório para arquivos temporários (criado se não existir)
    pub fn new(whisper: WhisperClient, hibiki: HibikiClient, work_dir: PathBuf) -> Self {
        Self {
            whisper,
            hibiki,
            work_dir,
        }
    }

    /// Processa uma mensagem de voz completa.
    ///
    /// # Fluxo
    /// 1. Converte `.ogg` → `.wav` (16 kHz mono) via ffmpeg
    /// 2. Transcreve `.wav` → texto via Whisper
    /// 3. Envia texto ao LLM (callback) e obtém resposta
    /// 4. Sintetiza resposta → `.wav` (44.1 kHz) via Hibiki-Zero
    /// 5. Converte `.wav` → `.ogg` (Opus) via ffmpeg
    ///
    /// # Returns
    /// Caminho do `.ogg` final pronto para enviar ao Telegram.
    pub async fn process_voice<F, Fut>(
        &self,
        input: &Path,
        llm_callback: F,
    ) -> Result<(PathBuf, VoiceMetrics), VoiceError>
    where
        F: FnOnce(String) -> Fut,
        Fut: std::future::Future<Output = Result<String, VoiceError>>,
    {
        let pipeline_start = Instant::now();

        // Garante que o diretório de trabalho existe
        tokio::fs::create_dir_all(&self.work_dir).await?;

        // Gera prefixo único para arquivos intermediários
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let base = self.work_dir.join(format!("voice_{ts}"));

        // Copia input para work_dir (preserva original)
        let input_ogg = base.with_extension("input.ogg");
        tokio::fs::copy(input, &input_ogg).await?;

        // ── 1. OGG → WAV (16 kHz mono) ──────────────────────────────────
        let t = Instant::now();
        let input_wav = AudioConverter::ogg_to_wav(&input_ogg).await?;
        let ogg_to_wav_ms = t.elapsed().as_millis();
        tracing::info!(ogg_to_wav_ms, "Step 1/5: ogg → wav");

        // ── 2. WAV → Texto (Whisper STT) ────────────────────────────────
        let t = Instant::now();
        let transcribed_text = self.whisper.transcribe(&input_wav).await?;
        let stt_ms = t.elapsed().as_millis();
        tracing::info!(
            stt_ms,
            text_len = transcribed_text.len(),
            "Step 2/5: wav → text (Whisper)"
        );

        if transcribed_text.trim().is_empty() {
            return Err(VoiceError::Stt(
                "Whisper returned empty transcription".to_string(),
            ));
        }

        // ── 3. Texto → LLM → Resposta ───────────────────────────────────
        let t = Instant::now();
        let llm_response = llm_callback(transcribed_text.clone()).await?;
        let llm_ms = t.elapsed().as_millis();
        tracing::info!(
            llm_ms,
            response_len = llm_response.len(),
            "Step 3/5: text → LLM → response"
        );

        if llm_response.trim().is_empty() {
            return Err(VoiceError::Llm("LLM returned empty response".to_string()));
        }

        // ── 4. Resposta → WAV (Hibiki TTS) ──────────────────────────────
        let t = Instant::now();
        let output_wav = base.with_extension("output.wav");
        self.hibiki.synthesize(&llm_response, &output_wav).await?;
        let tts_ms = t.elapsed().as_millis();
        tracing::info!(tts_ms, "Step 4/5: text → wav (Hibiki)");

        // ── 5. WAV → OGG (Opus) ─────────────────────────────────────────
        let t = Instant::now();
        let output_ogg = AudioConverter::wav_to_ogg(&output_wav).await?;
        let wav_to_ogg_ms = t.elapsed().as_millis();
        tracing::info!(wav_to_ogg_ms, "Step 5/5: wav → ogg");

        // ── Métricas ────────────────────────────────────────────────────
        let total_ms = pipeline_start.elapsed().as_millis();

        let metrics = VoiceMetrics {
            ogg_to_wav_ms,
            stt_ms,
            transcribed_text,
            llm_ms,
            llm_response,
            tts_ms,
            wav_to_ogg_ms,
            total_ms,
        };

        tracing::info!(total_ms, "Voice pipeline complete");

        // ── Limpeza de arquivos intermediários ──────────────────────────
        let _ = tokio::fs::remove_file(&input_ogg).await;
        let _ = tokio::fs::remove_file(&input_wav).await;
        let _ = tokio::fs::remove_file(&output_wav).await;

        Ok((output_ogg, metrics))
    }
}
