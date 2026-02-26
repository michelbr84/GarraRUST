"""
GarraIA — Whisper Large v3 STT Server
======================================
Servidor de Speech-to-Text usando Whisper Large v3.

Usa faster-whisper (CTranslate2) para inferência otimizada em CUDA FP16.

Baseado no model card oficial:
- https://huggingface.co/openai/whisper-large-v3
"""

import io
import os
import sys
import time
import logging
import tempfile
from pathlib import Path

import numpy as np
import soundfile as sf
from fastapi import FastAPI, HTTPException, UploadFile, File, Form
from fastapi.responses import JSONResponse
from pydantic import BaseModel
import uvicorn

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
)
logger = logging.getLogger("whisper_server")

# ============================================================
# Config
# ============================================================
WHISPER_MODEL = os.getenv("WHISPER_MODEL", "large-v3")
DEVICE = "cuda" if __import__("torch").cuda.is_available() else "cpu"
COMPUTE_TYPE = "float16" if DEVICE == "cuda" else "float32"
HOST = os.getenv("WHISPER_HOST", "0.0.0.0")
PORT = int(os.getenv("WHISPER_PORT", "8100"))


# ============================================================
# Response models
# ============================================================
class TranscriptionResponse(BaseModel):
    text: str
    language: str
    confidence: float
    duration_seconds: float
    processing_time_ms: float


class HealthResponse(BaseModel):
    status: str
    model: str
    device: str
    compute_type: str
    cuda_available: bool
    gpu_name: str | None = None


# ============================================================
# Whisper Engine (faster-whisper / CTranslate2)
# ============================================================
class WhisperEngine:
    """Motor STT baseado em faster-whisper (CTranslate2) para máxima performance."""

    def __init__(self):
        self.model = None
        self.model_size = WHISPER_MODEL
        self.device = DEVICE
        self.compute_type = COMPUTE_TYPE
        self._loaded = False

    def load(self) -> None:
        """Carrega o modelo Whisper na GPU com FP16."""
        logger.info(f"Carregando Whisper {self.model_size} em {self.device} ({self.compute_type})")
        t0 = time.time()

        try:
            from faster_whisper import WhisperModel

            self.model = WhisperModel(
                self.model_size,
                device=self.device,
                compute_type=self.compute_type,
            )

            t1 = time.time()
            self._loaded = True
            logger.info(f"Whisper carregado com sucesso em {t1 - t0:.1f}s")

            if self.device == "cuda":
                import torch
                vram_used = torch.cuda.memory_allocated() / 1e9
                logger.info(f"VRAM usada pelo Whisper: {vram_used:.1f} GB")

        except ImportError:
            logger.warning("faster-whisper não disponível, tentando openai-whisper...")
            import whisper

            self.model = whisper.load_model(self.model_size, device=self.device)
            self._loaded = True
            logger.info("Whisper (openai) carregado com sucesso")

    def transcribe(self, audio_path: str, language: str = "pt") -> dict:
        """
        Transcreve áudio para texto.

        Args:
            audio_path: Caminho para arquivo WAV (16kHz mono)
            language: Código do idioma ("pt" para português)

        Returns:
            dict com text, language, confidence, duration_seconds
        """
        if not self._loaded:
            raise RuntimeError("Modelo não carregado.")

        t0 = time.time()

        try:
            # faster-whisper
            from faster_whisper import WhisperModel

            segments, info = self.model.transcribe(
                audio_path,
                language=language,
                beam_size=5,
                vad_filter=True,
                vad_parameters=dict(min_silence_duration_ms=500),
            )

            full_text = ""
            total_prob = 0.0
            seg_count = 0

            for segment in segments:
                full_text += segment.text
                total_prob += segment.avg_log_prob
                seg_count += 1

            avg_confidence = min(1.0, max(0.0, 1.0 + (total_prob / max(seg_count, 1))))

            t1 = time.time()
            return {
                "text": full_text.strip(),
                "language": info.language,
                "confidence": avg_confidence,
                "duration_seconds": info.duration,
                "processing_time_ms": (t1 - t0) * 1000,
            }

        except ImportError:
            # Fallback: openai-whisper
            result = self.model.transcribe(audio_path, language=language)
            t1 = time.time()
            return {
                "text": result["text"].strip(),
                "language": result.get("language", language),
                "confidence": 0.95,
                "duration_seconds": 0.0,
                "processing_time_ms": (t1 - t0) * 1000,
            }

    def is_loaded(self) -> bool:
        return self._loaded


# ============================================================
# FastAPI Application
# ============================================================
app = FastAPI(
    title="GarraIA Whisper STT Server",
    description="Speech-to-Text com Whisper Large v3 (CUDA FP16)",
    version="0.1.0",
)

engine = WhisperEngine()


@app.on_event("startup")
async def startup():
    logger.info("Iniciando GarraIA Whisper STT Server...")
    try:
        engine.load()
    except Exception as e:
        logger.error(f"Falha ao carregar Whisper: {e}")
        logger.warning("Servidor iniciando SEM modelo carregado")


@app.post("/transcribe")
async def transcribe(
    audio: UploadFile = File(...),
    language: str = Form("pt"),
):
    """
    Transcreve áudio WAV para texto.

    Aceita: WAV 16kHz mono
    Retorna: texto transcrito + métricas
    """
    if not engine.is_loaded():
        raise HTTPException(status_code=503, detail="Modelo não carregado.")

    # Salvar upload em arquivo temporário
    with tempfile.NamedTemporaryFile(suffix=".wav", delete=False) as tmp:
        content = await audio.read()
        tmp.write(content)
        tmp_path = tmp.name

    try:
        result = engine.transcribe(tmp_path, language=language)
        logger.info(
            f"Transcribed: '{result['text'][:50]}...' "
            f"lang={result['language']} conf={result['confidence']:.2f} "
            f"latency={result['processing_time_ms']:.0f}ms"
        )
        return TranscriptionResponse(**result)

    except Exception as e:
        logger.error(f"Erro na transcrição: {e}")
        raise HTTPException(status_code=500, detail=str(e))

    finally:
        Path(tmp_path).unlink(missing_ok=True)


@app.get("/health")
async def health():
    info = {
        "status": "ready" if engine.is_loaded() else "not_loaded",
        "model": WHISPER_MODEL,
        "device": DEVICE,
        "compute_type": COMPUTE_TYPE,
        "cuda_available": __import__("torch").cuda.is_available(),
    }
    if DEVICE == "cuda":
        import torch
        info["gpu_name"] = torch.cuda.get_device_name(0)
    return info


# ============================================================
# Main
# ============================================================
if __name__ == "__main__":
    logger.info(f"Iniciando servidor em {HOST}:{PORT}")
    uvicorn.run(app, host=HOST, port=PORT, log_level="info")
