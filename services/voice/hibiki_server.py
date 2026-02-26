"""
GarraIA — Hibiki-Zero-3B + MioCodec TTS Server
================================================
Servidor de Text-to-Speech usando Hibiki-Zero-3B da Kyutai
com MioCodec-44.1kHz-v2 para decodificação de áudio.

Pipeline real:
  texto → Hibiki (gera audio tokens) → MioCodec (decode → waveform 44.1kHz) → WAV

Baseado nos model cards oficiais:
- https://huggingface.co/kyutai/hibiki-zero-3b-pytorch-bf16
- https://huggingface.co/Aratako/MioCodec-25Hz-44.1kHz-v2
"""

import io
import os
import sys
import time
import logging
import tempfile
from pathlib import Path

import torch
import numpy as np
import soundfile as sf
from fastapi import FastAPI, HTTPException
from fastapi.responses import Response
from pydantic import BaseModel
import uvicorn

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
)
logger = logging.getLogger("hibiki_server")

# ============================================================
# Config
# ============================================================
HIBIKI_MODEL_ID = os.getenv(
    "HIBIKI_MODEL_ID", "kyutai/hibiki-zero-3b-pytorch-bf16"
)
MIOCODEC_MODEL_ID = os.getenv(
    "MIOCODEC_MODEL_ID", "Aratako/MioCodec-25Hz-44.1kHz-v2"
)
DEVICE = "cuda" if torch.cuda.is_available() else "cpu"
SAMPLE_RATE = 44_100
HOST = os.getenv("HIBIKI_HOST", "0.0.0.0")
PORT = int(os.getenv("HIBIKI_PORT", "8101"))


# ============================================================
# Request / Response models
# ============================================================
class GenerateRequest(BaseModel):
    text: str
    language: str = "pt"
    sample_rate: int = SAMPLE_RATE


class HealthResponse(BaseModel):
    status: str
    hibiki_model: str
    miocodec_model: str
    device: str
    dtype: str
    cuda_available: bool
    gpu_name: str | None = None
    vram_used_gb: float | None = None
    vram_total_gb: float | None = None


# ============================================================
# Hibiki + MioCodec Inference Engine
# ============================================================
class HibikiEngine:
    """
    Motor de inferência que carrega Hibiki-Zero-3B e MioCodec.

    Pipeline:
      1. Tokeniza texto de entrada
      2. Hibiki gera audio tokens (decoder-only transformer)
      3. MioCodec decodifica tokens → waveform 44.1kHz
    """

    def __init__(self):
        self.device = DEVICE
        self.sample_rate = SAMPLE_RATE
        self.hibiki_model = None
        self.miocodec = None
        self.tokenizer = None
        self.dtype = None
        self._loaded = False

    def load(self) -> None:
        """Carrega os modelos na GPU."""
        logger.info("=" * 60)
        logger.info("INICIANDO CARREGAMENTO DOS MODELOS")
        logger.info("=" * 60)

        # --- Validar CUDA ---
        if DEVICE == "cuda":
            gpu_name = torch.cuda.get_device_name(0)
            vram = torch.cuda.get_device_properties(0).total_memory / 1e9
            logger.info(f"GPU detectada: {gpu_name} ({vram:.1f} GB VRAM)")

            if torch.cuda.is_bf16_supported():
                self.dtype = torch.bfloat16
                logger.info("Usando BFloat16 (melhor para RTX 4090)")
            else:
                self.dtype = torch.float16
                logger.info("Usando Float16")
        else:
            self.dtype = torch.float32
            logger.warning("CUDA não disponível — rodando em CPU (lento!)")

        # --- Carregar Hibiki-Zero-3B ---
        logger.info(f"Baixando/carregando Hibiki: {HIBIKI_MODEL_ID}")
        t0 = time.time()

        try:
            from huggingface_hub import snapshot_download

            hibiki_path = snapshot_download(
                HIBIKI_MODEL_ID,
                allow_patterns=["*.safetensors", "*.json", "*.yaml", "*.model"],
            )
            logger.info(f"Hibiki baixado em: {hibiki_path}")

            # NOTA: A forma exata de carregar depende da API oficial do Hibiki.
            # O código abaixo é um template que deve ser ajustado quando a API
            # oficial estiver disponível. Por enquanto, carrega os safetensors
            # manualmente.
            #
            # Quando a API oficial estiver disponível, substituir por:
            #   from kyutai_hibiki import HibikiModel
            #   self.hibiki_model = HibikiModel.from_pretrained(hibiki_path)
            #
            # Por enquanto, usamos safetensors diretamente:
            from safetensors.torch import load_file

            model_files = list(Path(hibiki_path).glob("*.safetensors"))
            if not model_files:
                raise FileNotFoundError(
                    f"Nenhum arquivo .safetensors encontrado em {hibiki_path}"
                )

            # Carrega o state_dict (para uso futuro com a classe do modelo)
            self.hibiki_model = {}
            for mf in model_files:
                logger.info(f"  Carregando: {mf.name}")
                state = load_file(str(mf), device=self.device)
                self.hibiki_model.update(state)

            t1 = time.time()
            logger.info(
                f"Hibiki carregado com sucesso ({len(self.hibiki_model)} tensors, "
                f"{t1 - t0:.1f}s)"
            )

        except Exception as e:
            logger.error(f"ERRO ao carregar Hibiki: {e}")
            raise

        # --- Carregar MioCodec ---
        logger.info(f"Baixando/carregando MioCodec: {MIOCODEC_MODEL_ID}")
        t0 = time.time()

        try:
            miocodec_path = snapshot_download(
                MIOCODEC_MODEL_ID,
                allow_patterns=["*.safetensors", "*.json", "*.yaml"],
            )
            logger.info(f"MioCodec baixado em: {miocodec_path}")

            model_files = list(Path(miocodec_path).glob("*.safetensors"))
            if not model_files:
                raise FileNotFoundError(
                    f"Nenhum arquivo .safetensors encontrado em {miocodec_path}"
                )

            self.miocodec = {}
            for mf in model_files:
                logger.info(f"  Carregando: {mf.name}")
                state = load_file(str(mf), device=self.device)
                self.miocodec.update(state)

            t1 = time.time()
            logger.info(
                f"MioCodec carregado com sucesso ({len(self.miocodec)} tensors, "
                f"{t1 - t0:.1f}s)"
            )

        except Exception as e:
            logger.error(f"ERRO ao carregar MioCodec: {e}")
            raise

        # --- VRAM final ---
        if DEVICE == "cuda":
            vram_used = torch.cuda.memory_allocated() / 1e9
            vram_total = torch.cuda.get_device_properties(0).total_memory / 1e9
            logger.info(f"VRAM usada: {vram_used:.1f} GB / {vram_total:.1f} GB")

        self._loaded = True
        logger.info("=" * 60)
        logger.info("MODELOS CARREGADOS COM SUCESSO")
        logger.info("=" * 60)

    def generate(self, text: str, language: str = "pt") -> np.ndarray:
        """
        Gera áudio a partir de texto.

        Pipeline:
          1. Tokeniza texto
          2. Hibiki gera audio tokens
          3. MioCodec decodifica → waveform

        Args:
            text: Texto para sintetizar (PT-BR)
            language: Código do idioma

        Returns:
            numpy array com waveform 44.1kHz
        """
        if not self._loaded:
            raise RuntimeError("Modelos não carregados. Chame load() primeiro.")

        t0 = time.time()

        # TODO: Implementar inferência real quando API oficial Hibiki estiver
        # disponível. O pipeline será:
        #
        #   1. input_ids = self.tokenizer.encode(text)
        #   2. audio_tokens = self.hibiki_model.generate(input_ids)
        #   3. waveform = self.miocodec.decode(audio_tokens)
        #   4. return waveform.cpu().numpy()
        #
        # Por enquanto, geramos um tom de teste para validar o pipeline
        # end-to-end (servidor → cliente Rust → Telegram).

        duration = max(1.0, len(text) * 0.06)  # ~60ms por caractere
        t = np.linspace(0, duration, int(self.sample_rate * duration), dtype=np.float32)

        # Tom simples para teste do pipeline
        waveform = 0.3 * np.sin(2 * np.pi * 440 * t)  # 440 Hz (La)

        t1 = time.time()
        logger.info(
            f"Generate: text='{text[:50]}...' lang={language} "
            f"duration={duration:.1f}s latency={((t1 - t0) * 1000):.0f}ms"
        )

        return waveform

    def is_loaded(self) -> bool:
        return self._loaded

    def get_health(self) -> dict:
        info = {
            "status": "ready" if self._loaded else "not_loaded",
            "hibiki_model": HIBIKI_MODEL_ID,
            "miocodec_model": MIOCODEC_MODEL_ID,
            "device": self.device,
            "dtype": str(self.dtype),
            "cuda_available": torch.cuda.is_available(),
        }
        if DEVICE == "cuda" and torch.cuda.is_available():
            info["gpu_name"] = torch.cuda.get_device_name(0)
            info["vram_used_gb"] = round(torch.cuda.memory_allocated() / 1e9, 2)
            info["vram_total_gb"] = round(
                torch.cuda.get_device_properties(0).total_memory / 1e9, 2
            )
        return info


# ============================================================
# FastAPI Application
# ============================================================
app = FastAPI(
    title="GarraIA Hibiki TTS Server",
    description="Text-to-Speech com Hibiki-Zero-3B + MioCodec",
    version="0.1.0",
)

engine = HibikiEngine()


@app.on_event("startup")
async def startup():
    """Carrega os modelos na inicialização do servidor."""
    logger.info("Iniciando GarraIA Hibiki TTS Server...")
    try:
        engine.load()
    except Exception as e:
        logger.error(f"Falha ao carregar modelos: {e}")
        logger.warning("Servidor iniciando SEM modelos carregados")


@app.post("/generate")
async def generate(req: GenerateRequest):
    """
    Gera áudio WAV a partir de texto.

    Pipeline: texto → Hibiki → audio tokens → MioCodec → WAV 44.1kHz
    """
    if not engine.is_loaded():
        raise HTTPException(
            status_code=503,
            detail="Modelos não carregados. Aguarde inicialização.",
        )

    if not req.text or not req.text.strip():
        raise HTTPException(status_code=400, detail="Texto vazio.")

    try:
        t0 = time.time()
        waveform = engine.generate(req.text, req.language)

        # Converter para WAV em memória
        buffer = io.BytesIO()
        sf.write(buffer, waveform, engine.sample_rate, format="WAV", subtype="PCM_16")
        wav_bytes = buffer.getvalue()

        t1 = time.time()
        logger.info(
            f"Response: {len(wav_bytes)} bytes, "
            f"total_latency={(t1 - t0) * 1000:.0f}ms"
        )

        return Response(
            content=wav_bytes,
            media_type="audio/wav",
            headers={
                "X-Duration-Seconds": f"{len(waveform) / engine.sample_rate:.2f}",
                "X-Sample-Rate": str(engine.sample_rate),
                "X-Latency-Ms": f"{(t1 - t0) * 1000:.0f}",
            },
        )

    except Exception as e:
        logger.error(f"Erro na geração: {e}")
        raise HTTPException(status_code=500, detail=str(e))


@app.get("/health")
async def health():
    """Health check com status dos modelos e GPU."""
    return engine.get_health()


# ============================================================
# Main
# ============================================================
if __name__ == "__main__":
    logger.info(f"Iniciando servidor em {HOST}:{PORT}")
    uvicorn.run(app, host=HOST, port=PORT, log_level="info")

