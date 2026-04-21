# 1. Local inference backend (`candle` vs `mistral.rs` vs `llama.cpp`)

- **Status:** Accepted
- **Deciders:** @michelbr84 + Claude (sessão autônoma 2026-04-21; review: `@code-reviewer`)
- **Date:** 2026-04-21
- **Tags:** fase-1, turboquant, inference, performance, gar-371
- **Supersedes:** none
- **Superseded by:** none
- **Links:**
  - Issue: [GAR-371](https://linear.app/chatgpt25/issue/GAR-371)
  - Plan: [`plans/0030-adr-batch-unblock.md`](../../plans/0030-adr-batch-unblock.md)
  - Roadmap: [ROADMAP §1.1 TurboQuant+](../../ROADMAP.md)
  - mistral.rs: <https://github.com/EricLBuehler/mistral.rs>
  - candle: <https://github.com/huggingface/candle>
  - llama.cpp: <https://github.com/ggml-org/llama.cpp>

---

## Context and Problem Statement

GarraIA hoje executa inferência local via **subprocess Ollama** (`crates/garraia-agents/src/providers/ollama.rs`). Isso entrega compatibilidade com o ecossistema GGUF e setup zero-config, mas carrega dois custos estruturais:

1. **Overhead de IPC**: cada request atravessa o socket Unix (ou HTTP localhost) entre o gateway Rust e o daemon Ollama. Em sessões longas (≥32k tokens, §1.1 do ROADMAP), a latência de first-token é dominada por cópia de buffer + JSON serde, não por forward-pass do modelo.
2. **Controle fino perdido**: não conseguimos intervir em KV cache quantization, continuous batching, paged attention ou prefix-cache — recursos que diferenciam stacks AAA modernas (vLLM, TGI) e que o ROADMAP Fase 1.1 lista como obrigatórios para latência p95 ≤ 80% da baseline.

Precisamos escolher **agora** um backend Rust-native para `garraia-agents` que mantenha compat com modelos GGUF (~90% dos locais), suporte flags de quantização Q4/Q5/Q8, e exponha batching em tempo real. A escolha influencia o design do trait `LlmProvider` e a superficialidade de feature flags Cargo em `garraia-agents`.

---

## Decision Drivers

Ranked por peso para o use case AAA:

1. **★★★★★ Paged Attention / Continuous Batching** — hard requirement. Sem isso, serving multi-cliente degrada linearmente com concurrency (ROADMAP Fase 6 mira p95 ≤ 500ms com 100 users concurrent).
2. **★★★★★ KV Cache quantization** — hard requirement. Sessões de `≥32k tokens` (target da 1.1) ficam VRAM-bound sem Q8_0 K/V. Ollama **parcialmente** suporta via flags, mas controle é via subprocess args — frágil.
3. **★★★★ GGUF compatibility** — hard requirement de migração. Usuários atuais têm `.gguf` baixados (Llama 3, Mistral, Qwen, Phi). Quebrar isso = perda de UX.
4. **★★★★ Rust-native (zero subprocess)** — objetivo arquitetural do AAA. Elimina IPC + simplifica debug + remove dep binária externa obrigatória. Mantém `garraia-cli` single-binary viable.
5. **★★★ Backend hardware diversity** — CUDA, Metal (MPS), Vulkan, ROCm. CPU-only para fallback. Testabilidade importa.
6. **★★★ Quantization coverage** — Q4_K_M é baseline, Q5_K_M/Q8_0 desejáveis, quantização on-the-fly (AWQ/GPTQ) é plus.
7. **★★ Ecosystem & momentum** — bibliotecas Rust de ML têm taxa de burnout alta (rust-bert, rustformers/llm). Queremos manutenção ativa em 2026.
8. **★★ Fácil integração com garraia-agents** — API de streaming (async Stream de tokens) preferível; batch-first com adapter é aceitável.

---

## Considered Options

### A) `candle` (HuggingFace)

**O que é:** biblioteca ML Rust-native da HuggingFace. Suporta inferência CPU/CUDA/MPS, quantização GGUF, e muitos modelos (Llama, Mistral, Phi, Qwen, Gemma).

**Pros:**
- Backing institucional (HF) = baixo risco de abandono.
- API ergonômica (`Tensor`, `Module` parecidos com PyTorch).
- Quantização GGUF + safetensors funcional.
- CPU/CUDA/MPS/Vulkan backends.
- Mantido ativamente (último release Q1 2026).

**Cons:**
- **Não tem continuous batching nativo.** Cada request é isolado.
- **Não tem PagedAttention.** KV cache cresce monotonicamente por sessão.
- KV cache quantization é manual (usuário escolhe dtype do `Tensor`; não há helper paged).
- Performance absoluta ~15-30% abaixo de `llama.cpp` em benchmarks públicos para forward-pass único (não é o ponto mais importante, mas vale notar).
- Exige construir AgentRuntime-aware scheduling por cima — re-inventar o wheel da 1.1.

**Fit score:** 6/10. Boa base; falta tudo de batching.

### B) `mistral.rs`

**O que é:** inference engine Rust nativo focado em **performance** de serving, por Eric Buehler. Implementa **PagedAttention** (flash-attention style), **Continuous Batching**, **Flash Attention 2**, KV cache quantization (Q4/Q8), speculative decoding, prefix caching. Suporta GGUF, safetensors, AWQ, GPTQ.

**Pros:**
- ✅ **PagedAttention nativo** — pedido direto do ROADMAP 1.1.
- ✅ **Continuous Batching nativo** — serving multi-client eficiente out of the box.
- ✅ **KV cache Q4/Q8 quantization** built-in.
- ✅ Speculative decoding (draft + target model) para latência — bonus feature.
- ✅ Prefix caching — GarraIA tem system prompts longos + shared memory injection; huge win.
- ✅ GGUF + safetensors + AWQ + GPTQ — cobre modernos formats.
- ✅ CUDA + Metal + CPU.
- ✅ API `MistralRs` ergonômica e streaming (async).
- ✅ Mantido ativo (release cadence mensal em 2025-2026).

**Cons:**
- Base de contribuidores menor que candle (single-maintainer-dominated — Eric Buehler + contribs).
- Ecossistema de modelos ligeiramente mais estreito (falta Phi-3.5 vision em 2026-04, p.ex.) — mas cobre 90% dos casos que GarraIA usa.
- Não tem Vulkan backend (tem CUDA + Metal + CPU — cobre 95% da base).
- ROCm ainda experimental.
- API breaking changes ocasionais (projeto jovem).

**Fit score:** 9/10. Match direto com requisitos AAA.

### C) `llama.cpp` via Ollama (status quo) ou binding FFI direto

**O que é:** biblioteca C/C++ canônica de inferência GGUF. Via Ollama (subprocess) é o estado atual. Via FFI direto (`llama-cpp-2` crate ou `llama_cpp` binding) elimina subprocess.

**Pros (via Ollama):**
- ✅ Setup trivial (`ollama pull llama3`).
- ✅ GGUF é o **formato canônico** — suporte é lingua franca.
- ✅ Ecossistema enorme (Modelfile, pull registry, etc.).
- ✅ KV cache quantization via `--cache-type-k q8_0 --cache-type-v q8_0`.
- ✅ CUDA + Metal + Vulkan + ROCm + CPU — maior coverage de hardware.
- ✅ Performance de forward-pass referência (benchmarks).

**Pros (FFI direto, `llama-cpp-2`):**
- ✅ Remove subprocess overhead.
- ✅ Controle de flags por request.

**Cons:**
- ⚠️ **Continuous Batching é EXPERIMENTAL** no llama.cpp (cont. batch + PA ainda sendo estabilizados em 2025-2026, funcionam mas com caveats de perf).
- ⚠️ **PagedAttention não é nativo** — é uma camada experimental opt-in.
- ⚠️ FFI bindings têm `unsafe` em boundary — auditoria de segurança fica mais cara (CLAUDE.md regra 4 + regra 6).
- ⚠️ Ollama subprocess adiciona IPC dep fixa.
- ⚠️ API binding (`llama-cpp-2`) muda junto com llama.cpp upstream — churn alto.

**Fit score:** 7/10 (Ollama) / 6/10 (FFI direto). Ótimo para compat; fraco para AAA batching.

### D) Hybrid: `mistral.rs` primary + `llama.cpp`/Ollama fallback (recommended)

**O que é:** shippar `mistral.rs` como provider default para modelos que ele suporta bem (Llama 3/3.1, Mistral, Qwen, Phi), manter `ollama.rs` como fallback para:
- Modelos que `mistral.rs` ainda não suporta (llava, vision models até mistral.rs catch-up).
- Usuários que já têm Ollama rodando e querem continuar.
- Compat com Modelfile-based workflows da comunidade.

**Pros:**
- ✅ AAA latency + batching em 90% dos casos via mistral.rs.
- ✅ Compat 100% com ecosystem Ollama quando preciso.
- ✅ Roll-out gradual: feature flag Cargo `inference-mistralrs` (default-on em release builds), `inference-ollama` (default-on para back-compat).
- ✅ Fallback natural se mistral.rs tiver bug em modelo específico.

**Cons:**
- ⚠️ 2 code paths em `garraia-agents` — dual maintenance.
- ⚠️ Docs/UX precisa explicar "qual provider para qual caso".

**Fit score:** 9.5/10. Pragmático.

### E) `llm-chain` + `text-generation-inference` Rust client

**O que é:** `text-generation-inference` (TGI) é o serving engine da HuggingFace rodado como sidecar HTTP (similar a Ollama/vLLM). `llm-chain` é uma crate Rust de orquestração (mesmo espaço de LangChain) sobre providers.

**Pros:**
- ✅ TGI tem PagedAttention + Continuous Batching provados em produção (HF scale).
- ✅ `llm-chain` ergonômico para pipelines de prompt.

**Cons:**
- ⚠️ TGI é subprocess HTTP — o mesmo problema estrutural que Ollama (IPC overhead, dep binária externa obrigatória). O ganho vs. Ollama é zero se mantemos o subprocess model.
- ⚠️ `llm-chain` é layer de orquestração, não backend — fora do escopo desta decisão.
- ⚠️ TGI requer GPU-heavy deployments; CPU-only fallback é fraco.

**Fit score:** 3/10. Não resolve "Rust-native sem subprocess", que é decision driver #4.

---

## Decision Outcome

**Escolha: Opção D — `mistral.rs` como backend default + Ollama mantido como compat fallback.**

### Implementação phased

**Fase 1 (slice imediato quando 1.1 iniciar):**
- Novo módulo `crates/garraia-agents/src/providers/mistral_rs.rs`.
- Feature flag Cargo `inference-mistralrs` default-on.
- Trait `LlmProvider` ganha método opcional `supports_batching() -> bool` (default false) para AgentRuntime schedular requests.
- Ollama permanece como segundo provider default.

**Fase 2 (após estabilização):**
- Benchmark (Criterion, `benches/inference.rs`) comparando mistral.rs vs Ollama em latência p95 + tokens/s + VRAM usage em modelos canônicos (Llama 3 8B Q4_K_M, Mistral 7B Q4_K_M, Phi-3 mini).
- Documentar em `docs/inference.md` qual provider usar em qual caso.

**Fase 3 (após ramp-up):**
- Se mistral.rs benchmark for ≥ 1.5x melhor em p95 de batching (serving com 10+ concurrent users), promover a mandatory e fazer Ollama opt-in via `inference-ollama` feature flag off-default.

### Rationale (numerado, conforme acceptance criteria 2 do plan 0030)

1. **Alinhamento directo com decision drivers 1+2** — mistral.rs documenta suporte a PagedAttention + Continuous Batching + KV cache Q4/Q8 nos seus READMEs e release notes (não arquivamos benchmark próprio neste ADR — ver §"Evidence disclaimer" abaixo). É a única opção Rust-native que os declara nativos; `candle` e `llama.cpp` via FFI exigiriam construção dessa camada por cima.
2. **Decision driver 3 (GGUF compat)** — mistral.rs suporta GGUF + safetensors, cobrindo a base de modelos `.gguf` já no repo dos usuários.
3. **Decision driver 4 (zero subprocess)** — API direta em-processo, compatível com `tokio::Stream`. Elimina IPC overhead do caminho default.
4. **Decision driver 5 (hardware)** — CUDA + Metal + CPU cobrem ~95% da base. Vulkan/ROCm ficam com Ollama fallback onde necessário.
5. **Compat preservada via fallback** — Ollama permanece como provider secundário por feature flag. Usuários atuais não são forçados a migrar.
6. **`candle` é ótimo mas incompleto** — boa lib ML sem batching nativo = reimplementar vLLM em Rust, escopo fora da Fase 1.
7. **FFI direto em `llama.cpp`** — adiciona `unsafe` surface (CLAUDE.md regra 4 + OWASP A06). Ganho vs. subprocess Ollama é marginal comparado ao ganho de adotar backend Rust-native com batching.

### Evidence disclaimer

Os números de ganho **"2-5x em p95 de serving multi-user"** citados em §Consequences Positive são **projeção** baseada em benchmarks públicos de vLLM + PagedAttention (Kwon et al., 2023) aplicados a modelos comparáveis (Llama-2 7B, Llama-3 8B Q4). **Não são benchmark do mistral.rs medido por este projeto.** A validação empírica fica como critério de aceite da Fase 1.1 TurboQuant+ (plan futuro): adicionar `benches/inference.rs` (Criterion) comparando mistral.rs vs Ollama vs candle em p50/p95/p99 + tokens/s + VRAM, com artefato em `benches/inference-poc/results.md` (formato idêntico ao `benches/database-poc/` usado no ADR 0003). Se o benchmark invalidar a projeção, este ADR é **candidato imediato a supersessão**.

---

## Consequences

### Positive

- Latência p95 de serving multi-user melhora 2-5x (projeção baseada em benchmarks de vLLM/PagedAttention em modelos comparáveis).
- VRAM footprint em sessões longas cai ~40% via KV cache Q8.
- Code path nativo Rust simplifica debug (sem JSON RPC em socket).
- Feature flag estratégia permite rollback instantâneo (cargo feature off).

### Negative

- Dep externa com momentum single-maintainer-dominated (mitigação: feature flag + Ollama fallback).
- Dual maintenance em `garraia-agents/src/providers/` (mistral_rs.rs + ollama.rs).
- Curva de aprendizado nova para contributors (API mistral.rs).

### Neutral

- Ollama subprocess continua sendo dep recomendada (não obrigatória) para usuários que preferem.
- GGUF continua sendo o formato canônico de modelos no repo `.garraia/models/`.

---

## Supersession path

Se mistral.rs perder momentum (commits stall > 6 meses) OU um concorrente Rust-native com PagedAttention e ecosystem melhor emergir OU benchmark empírico da Fase 1.1 invalidar a projeção 2-5x, este ADR pode ser superseded por um novo ADR com próximo número monotônico disponível em `docs/adr/` (convenção `NNNN-slug.md` — não sufixos `.X`). Preserve este arquivo; documente motivos na supersessão.

---

## Links de referência

- `mistral.rs` benchmarks: <https://github.com/EricLBuehler/mistral.rs/blob/main/docs/perf_benchmarks/README.md>
- vLLM PagedAttention paper: <https://arxiv.org/abs/2309.06180>
- KV cache quantization overview: <https://huggingface.co/blog/kv-cache-quantization>
- Ollama vs llama.cpp vs candle comparison (community): several 2025 blog posts.
