# GarraIA ─ ROADMAP & Next Steps (AAA Tier)

Este roadmap define as fundações e os próximos passos para elevar o ecossistema multidimensional do GarraIA (CLI, Gateway, Desktop, Mobile, e Agents) ao padrão **AAA**, mesclando workflows agenticos avançados, inferência local ultra-otimizada e uma arquitetura hiper-responsiva.

---

## 🚀 Fase 1: Integrações Core & Fundações

*Focada no fortalecimento da infraestrutura do motor de inferência, configuração e comportamento autonômo.*

- [ ] **TurboQuant+ (Inferência Otimizada Local)**
  - Implementar compressão do KV Cache para melhorar a eficiência da memória em sessões longas.
  - Testar e refinar PagedAttention/Continuous Batching para backends locais (ex: Llama.cpp ou Candle integrados).
  - Melhorar suporte à quantização de LLMs, rodando de forma otimizada usando os backends paralelos da máquina do usuário (MPS/CUDA).
- [ ] **Superpowers (Workflow Agentico & Auto-Dev)**
  - Institucionalizar o TDD com Subagentes: Agentes focados na criação assertiva de testes automatizados unitários nos crates e integrações no ecosistema RUST.
  - Implementar rotinas robustas de Git Worktrees para experimentações branch-by-branch que rodam sem afetar o repo principal em uso.
  - Orquestrador Mestre-Escravo: Permitir que o framework agentico delegue pequenas tarefas (`garraia-agents`) autonomamente para o LLM.
- [ ] **Config & Runtime Wiring (Unificação)**
  - Harmonização total de `.garraia/config.toml`, `mcp.json` e as instâncias de Providers LLM instanciadas.
  - Garantir que toda alteração via Web UI/CLI no Gateway (`garraia-gateway`) reflita em tempo real e de forma reativa nos processos via Server-Sent Events (SSE) ou gRPC.

---

## 🛠 Fase 2: Performance, Memória e MCP Ecosystem

*Focada em entregar escalabilidade e extensibilidade padrão Enterprise localmente.*

- [ ] **Memória de Longo Prazo e RAG Veloz**
  - Integração Local Embeddings: Rodar modelos nativos como o `mxbai-embed-large-v1` via onnxruntime ou em Rust nativo para vetorização.
  - Persistência e Indexação Vetorial Local usando uma engine ágil (ex: `lancedb` ou `qdrant` embutido) aliada à atual base SQLite.
- [ ] **Model Context Protocol (MCP) e Plugins WASM**
  - Expansão robusta dos servers MCP mantidos pelo Gateway para que aceitem sandboxing WebAssembly.
  - Ferramentas interativas autoescrevíveis (Sub-agentes Garra criam suas próprias ferramentas e as executam em um ambiente WASM restrito para segurança).
- [ ] **Zero-Latency Streaming & Telemetria**
  - Finalizar o backbone assíncrono Tokio com suporte avançado a processamento de stream via buffers enxutos em WebSockets.
  - Embarcar rastreamento via OpenTelemetry (traces simplificados) para identificar qualquer gargalo de microsegundos na arquitetura RUST para usuários exigentes.

---

## 🖥 Fase 3: Experiência Multi-Plataforma AAA

*Para consolidar o visual e a interação como a melhor plataforma IA Open Source.*

- [ ] **Garra Desktop (Tauri - Win/Mac/Linux)**
  - UI de estética extrema: Dark Mode imersivo, Micro-interações, Glassmorphism e Transições Fluidas (Performance Nativa de 120Hz).
  - Backend interligado ao processo principal `garraia-gateway` em bridge Rust ↔️ Typescript/WASM transparente.
- [ ] **Garra Mobile (Android / iOS)**
  - Correções no build do Android finalizadas para garantir perfeita compilação com os SDKs mais recentes (Java 17/11, Gradle 8.x).
  - App leve usando WebSocket Secure ou WebRTC para parear remotamente com a engine do Computador Central (sem latência extra para o aplicativo móvel).
  - Explorar uso de Tiny-LLMs locais ou NPU diretamente no smartphone (MLX Apple / ONNX Mobile).

---

## 🛡 Fase 4: Qualidade, Segurança e "Polishing"

*Estabilidade garantida antes dos lançamentos mundiais.*

- [ ] **Security e Vaults Criptografados**
  - Implementação final do Credentials Vault (`GAR-291`) para chaves e APIs, criptografia AES-256-GCM.
- [ ] **Cobertura Extensa e Continuous Fuzzing**
  - Rotinas CI/CA baseadas nos "Superpowers" do próprio GarraIA. (Sim, o Garra testa o próprio código).
- [ ] **UX Inicial Impecável ("Out of the Box")**
  - Wizard intuitivo na primeira execução, configurando o ambiente (Docker Local, Servidores Ollama ou chaves Nuvem) perfeitamente sem fricção para usuários casuais.
