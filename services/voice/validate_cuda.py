"""
GarraIA — Validação CUDA FP16 para Voice System
=================================================
Verifica se o ambiente está pronto para rodar Whisper + Hibiki na GPU.
"""

import sys


def validate():
    print("=" * 60)
    print("GarraIA CUDA Validation")
    print("=" * 60)

    errors = []

    # 1. PyTorch
    try:
        import torch

        print(f"[OK] PyTorch {torch.__version__}")
    except ImportError:
        print("[FAIL] PyTorch não instalado")
        errors.append("PyTorch")

    # 2. CUDA
    if torch.cuda.is_available():
        gpu_name = torch.cuda.get_device_name(0)
        vram_gb = torch.cuda.get_device_properties(0).total_memory / 1e9
        print(f"[OK] CUDA disponível: {gpu_name} ({vram_gb:.1f} GB)")

        if vram_gb < 16:
            print(f"[WARN] VRAM ({vram_gb:.1f} GB) < 16 GB recomendado")

        # 3. FP16
        dummy = torch.randn(100, 100, dtype=torch.float16, device="cuda")
        result = torch.matmul(dummy, dummy.T)
        print(f"[OK] FP16 funcional (matmul test passed)")
        del dummy, result

        # 4. BF16
        if torch.cuda.is_bf16_supported():
            dummy = torch.randn(100, 100, dtype=torch.bfloat16, device="cuda")
            result = torch.matmul(dummy, dummy.T)
            print(f"[OK] BF16 suportado (recomendado para Hibiki)")
            del dummy, result
        else:
            print("[WARN] BF16 não suportado — usando FP16")

    else:
        print("[FAIL] CUDA não disponível")
        errors.append("CUDA")

    # 5. faster-whisper
    try:
        from faster_whisper import WhisperModel
        print("[OK] faster-whisper disponível")
    except ImportError:
        print("[WARN] faster-whisper não instalado (fallback para openai-whisper)")

    # 6. huggingface_hub
    try:
        from huggingface_hub import snapshot_download
        print("[OK] huggingface_hub disponível")
    except ImportError:
        print("[FAIL] huggingface_hub não instalado")
        errors.append("huggingface_hub")

    # 7. safetensors
    try:
        from safetensors.torch import load_file
        print("[OK] safetensors disponível")
    except ImportError:
        print("[FAIL] safetensors não instalado")
        errors.append("safetensors")

    # 8. soundfile
    try:
        import soundfile
        print("[OK] soundfile disponível")
    except ImportError:
        print("[FAIL] soundfile não instalado")
        errors.append("soundfile")

    # 9. FastAPI
    try:
        import fastapi
        print(f"[OK] FastAPI {fastapi.__version__}")
    except ImportError:
        print("[FAIL] FastAPI não instalado")
        errors.append("FastAPI")

    # Resultado
    print("=" * 60)
    if errors:
        print(f"[FAIL] {len(errors)} problemas encontrados: {', '.join(errors)}")
        print("Execute: pip install -r requirements.txt")
        return False
    else:
        print("[OK] Ambiente pronto para GarraIA Voice System!")
        if torch.cuda.is_available():
            vram = torch.cuda.get_device_properties(0).total_memory / 1e9
            print(f"     GPU: {torch.cuda.get_device_name(0)}")
            print(f"     VRAM: {vram:.1f} GB")
            print(f"     Whisper estimado: ~2.5 GB")
            print(f"     Hibiki estimado:  ~9 GB")
            print(f"     Total estimado:   ~11.5 GB")
            print(f"     VRAM livre:       ~{vram - 11.5:.1f} GB")
        return True


if __name__ == "__main__":
    ok = validate()
    sys.exit(0 if ok else 1)

