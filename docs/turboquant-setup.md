# TurboQuant+ Setup — GarraRUST

Guia para usar [TurboQuant+](https://github.com/TheTom/turboquant_plus) com o GarraRUST para compressão de KV cache na inferência local.

## O que é TurboQuant+?

TurboQuant+ comprime o KV cache dos transformers durante inferência local usando PolarQuant + Walsh-Hadamard rotation. Isso reduz significativamente o uso de memória com mínima perda de qualidade.

| Tipo | Bits | Compressão | Recomendação |
|------|------|------------|--------------|
| `f16` | 16 | 1.0× | Baseline (sem compressão) |
| `q8_0` | 8 | 2.0× | Padrão llama.cpp |
| `q4_0` | 4 | 4.0× | Boa compressão, alguma perda |
| `turbo4` | 4 | 3.8× | TurboQuant — melhor que q4_0 em qualidade |
| `turbo3` | 3 | 5.1× | TurboQuant — excelente balanço custo/qualidade |
| `turbo2` | 2 | 6.4× | TurboQuant — máxima compressão |

### Descoberta-chave: V compression é "free"

A compressão do cache de valores (V) até 2 bits não tem efeito mensurável na qualidade da atenção quando a precisão das chaves (K) é mantida. Por isso, a configuração recomendada é **assimétrica**:

```
K = turbo3 (3 bits) + V = turbo2 (2 bits)
```

## Opção 1: Build manual

```bash
# Clone e compile
./scripts/build-turboquant-llama.sh --cuda   # NVIDIA GPU
./scripts/build-turboquant-llama.sh --cpu    # CPU only

# Inicie o servidor
./services/llama-turboquant/bin/llama-server \
    -m models/seu-modelo.gguf \
    --cache-type-k turbo3 --cache-type-v turbo2 \
    -ngl 99 -c 32768 -fa on \
    --host 0.0.0.0 --port 8080
```

## Opção 2: Docker

```bash
# Coloque seu modelo em ./models/
# Configure em .env:
TURBOQUANT_MODEL=seu-modelo.gguf
CACHE_TYPE_K=turbo3
CACHE_TYPE_V=turbo2

# Suba
docker compose -f docker-compose.turboquant.yml up
```

## Configuração no GarraRUST

No `garraia.toml`, adicione um provider `llama-cpp`:

```yaml
llm:
  turboquant:
    provider: llama-cpp
    model: seu-modelo
    base_url: "http://localhost:8080"
    cache_type_k: turbo3
    cache_type_v: turbo2
    context_size: 32768
    flash_attention: true

agent:
  default_provider: turboquant
```

## Recomendações por Hardware

| Hardware | K cache | V cache | Context | Notas |
|----------|---------|---------|---------|-------|
| RTX 3090 (24GB) | turbo3 | turbo2 | 32K | Modelos até 35B com quantização Q4 |
| RTX 4090 (24GB) | turbo3 | turbo2 | 64K | Mais rápido que 3090, mesma VRAM |
| Apple M5 Max (128GB) | turbo3 | turbo2 | 128K | Modelos até 104B |
| CPU (32GB RAM) | q8_0 | turbo2 | 8K | TurboQuant requer flash attention |

## Verificação

```bash
# Verificar que os tipos turbo estão disponíveis
./services/llama-turboquant/bin/llama-server --help | grep turbo

# Testar via curl
curl http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "default",
    "messages": [{"role": "user", "content": "Hello!"}],
    "max_tokens": 50
  }'

# Verificar health
curl http://localhost:8080/health
```

## Referências

- [TurboQuant+ repo](https://github.com/TheTom/turboquant_plus)
- [llama.cpp fork](https://github.com/TheTom/llama-cpp-turboquant)
- [Paper original (ICLR 2026)](https://research.google/blog/turboquant-redefining-ai-efficiency-with-extreme-compression/)
- [Config recommendations](https://github.com/TheTom/turboquant_plus/blob/main/docs/turboquant-recommendations.md)
