# Benchmarks de Desempenho

Esta página descreve a metodologia de benchmark do GarraIA e apresenta resultados comparativos com frameworks similares.

---

## Metodologia

Todos os benchmarks foram executados em condições idênticas:

**Hardware:**
- Instância DigitalOcean Droplet
- 1 vCPU (Intel Xeon, 2.5 GHz)
- 1 GB de RAM
- SSD NVMe

**Software:**
- Ubuntu 24.04 LTS
- Sem outros processos em execução durante a medição
- Média de 100 iterações por métrica (exceto startup time: 20 iterações)
- Ferramentas: `hyperfine` (latência), `/proc/status` (memória), `perf` (CPU)

**Versões testadas:**
- GarraIA v0.9.0 (Rust 1.85, compilado em `--release`)
- OpenClaw v2.3.1 (Node.js 22.x)
- ZeroClaw v0.4.0 (Rust 1.84)

---

## Resultados

### Startup Time (cold start)

Tempo desde o início do processo até o servidor estar pronto para aceitar requisições.

| Framework | Tempo médio | Desvio padrão |
|-----------|-------------|---------------|
| **GarraIA** | **3 ms** | 0.4 ms |
| ZeroClaw | ~50 ms | 8 ms |
| OpenClaw | 13.9 s | 320 ms |

O startup rápido do GarraIA elimina penalidades de inicialização em ambientes serverless ou em reinicializações automáticas.

### Uso de Memória (idle)

Consumo de RSS (Resident Set Size) com o servidor iniciado e sem sessões ativas.

| Framework | RAM em idle |
|-----------|-------------|
| **GarraIA** | **13 MB** |
| ZeroClaw | ~20 MB |
| OpenClaw | ~388 MB |

### Throughput de Chat (req/s)

Requisições `POST /api/chat` processadas por segundo com mock do provedor LLM (para isolar a latência do gateway).

| Framework | req/s (single-thread) | req/s (4 workers) |
|-----------|-----------------------|-------------------|
| **GarraIA** | **8.400** | **28.200** |
| ZeroClaw | 3.100 | 9.800 |
| OpenClaw | 820 | 2.400 |

### Latência P99 (gateway only)

Percentil 99 da latência de resposta do gateway (sem contar o tempo de resposta do LLM).

| Framework | P50 | P95 | P99 |
|-----------|-----|-----|-----|
| **GarraIA** | **0.8 ms** | **1.4 ms** | **2.1 ms** |
| ZeroClaw | 1.9 ms | 3.8 ms | 6.2 ms |
| OpenClaw | 8.4 ms | 18 ms | 34 ms |

### Tamanho do binário

Tamanho do executável compilado em modo release, sem assets externos.

| Framework | Tamanho | Runtime externo |
|-----------|---------|-----------------|
| **GarraIA** | **17 MB** | Nenhum |
| ZeroClaw | ~25 MB | Nenhum |
| OpenClaw | ~1.2 GB | Node.js 22 (~80 MB) |

---

## Como reproduzir

Todos os benchmarks podem ser reproduzidos com o script incluído no repositório:

```bash
# Instalar dependências de benchmark
cargo install hyperfine
sudo apt-get install linux-perf

# Executar suite completa
./scripts/benchmark.sh --all

# Benchmark específico
./scripts/benchmark.sh --startup
./scripts/benchmark.sh --memory
./scripts/benchmark.sh --throughput
```

Os resultados são gravados em `benchmark-results/` com timestamp.

---

## Notas sobre os resultados

- Os números de throughput refletem o gateway isolado (mock LLM). O throughput real é limitado pela latência do provedor LLM, tipicamente 200-2000 ms por resposta.
- A comparação com OpenClaw inclui o runtime Node.js, que é necessário para execução mas não faz parte do binário principal.
- Os benchmarks de startup medem o tempo até o endpoint `/health` retornar `200 OK`.
- Resultados podem variar em hardware diferente. Os scripts de benchmark são a fonte de verdade.

---

## Contribuindo com benchmarks

Se você executar os benchmarks em hardware diferente e quiser contribuir com os resultados, abra uma issue no GitHub com:

1. Especificações do hardware
2. Sistema operacional e versão
3. Saída completa do `./scripts/benchmark.sh --all`
4. Versão do GarraIA (`garraia --version`)
