# Arquitetura e ADRs

## Visão geral

- [Architecture Overview](https://github.com/michelbr84/GarraRUST/blob/main/docs/architecture.md) — estrutura do workspace (24 crates), fluxo do runtime, pipeline de voz, multi-agente, memória, segurança, hot-reload
- [A plataforma de hardware](https://github.com/michelbr84/GarraRUST/blob/main/docs/hardware.md) — camadas core → `garraia-hardware` → adapter/skill → device, modelo de risco R0-R5, motor de automações · [Hardware skills](https://github.com/michelbr84/GarraRUST/blob/main/docs/hardware-skills.md)
- [Referência da API REST do gateway](https://github.com/michelbr84/GarraRUST/blob/main/docs/src/api-reference.md) · [OpenAPI da API mobile](https://github.com/michelbr84/GarraRUST/blob/main/docs/mobile-api-v1.yaml)
- [Sistema de memória](https://github.com/michelbr84/GarraRUST/blob/main/docs/src/memory.md) · [Benchmarks](https://github.com/michelbr84/GarraRUST/blob/main/docs/src/benchmarks.md)

## ADRs — Architectural Decision Records

Decisões irreversíveis são registradas antes de implementar ([índice](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/README.md)). São 23 ADRs; todas **accepted**, exceto a 0018, ainda **proposed**:

| # | Decisão | Data |
|---|---|---|
| [0001](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0001-local-inference-backend.md) | Backend de inferência local (candle vs mistral.rs vs llama.cpp) | 2026-04-21 |
| [0002](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0002-vector-store.md) | Vector store (pgvector vs lancedb vs qdrant) | 2026-04-21 |
| [0003](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0003-database-for-workspace.md) | Postgres para o Group Workspace | 2026-04-13 |
| [0004](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0004-object-storage.md) | Object storage S3-compatible (MinIO default) | 2026-04-21 |
| [0005](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0005-identity-provider.md) | Identity Provider (BYPASSRLS + Argon2id + JWT HS256) | 2026-04-13 |
| [0006](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0006-search-strategy.md) | Estratégia de busca (Postgres FTS → Tantivy → Meilisearch) | 2026-04-21 |
| [0007](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0007-desktop-frontend.md) | Frontend desktop (HTML vanilla → SolidJS) | 2026-04-21 |
| [0008](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0008-doc-collaboration.md) | Colaboração em docs (single-editor → y-crdt) | 2026-04-21 |
| [0009](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0009-web-console-design-system.md) | Design system "Garra Glass" do Web Console (zero CDN) | 2026-05-13 |
| [0010](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0010-garra-learning-agent.md) | Garra Learning Agent (manual de operações auto-evolutivo) | 2026-05-17 |
| [0011](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0011-garra-max-power.md) | GarraMaxPower — modo agent-advanced nativo | 2026-05-24 |
| [0012](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0012-garra-persona.md) | Persona amistosa do Garra (tom de voz padrão) | 2026-06-01 |
| [0013](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0013-scheduling-two-tier-recurrence.md) | Recorrência de agendamento em duas camadas (cron no SQLite + RRULE no workspace) | 2026-08-29 |
| [0014](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0014-anthropic-messages-shim.md) | Superfície Anthropic-compatible no gateway (`POST /v1/messages`) | 2026-08-30 |
| [0015](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0015-linux-packaging-toolchain.md) | Toolchain de empacotamento Linux (nfpm `.deb`/`.rpm` + AppImage) | 2026-08-31 |
| [0016](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0016-mobile-termux-local-first.md) | Garra Mobile — Termux como camada de execução (local-first no Android) | 2026-09-02 |
| [0017](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0017-ui-event-terminal-renderer.md) | Camada de apresentação do CLI (`UiEvent` + `TerminalRenderer`) | 2026-09-05 |
| [0018](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0018-crate-garraia-embeddings.md) | O que fazer com o crate `garraia-embeddings` — **proposed** | 2026-09-07 |
| [0019](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0019-process-hardening-and-sandbox.md) | Confinamento das tools — `PR_SET_DUMPABLE` agora, Landlock depois | 2026-09-09 |
| [0020](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0020-crate-garraia-hardware.md) | Crate `garraia-hardware` — abstração de dispositivos físicos | 2026-09-11 |
| [0021](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0021-garraia-desktop-control-center.md) | GarraIA Desktop — Control Center (`garraia desktop`) | 2026-09-14 |
| [0022](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0022-default-llm-identity.md) | `z-ai/glm-5.3-flash` via OpenRouter como LLM padrão; local como segunda opção | 2026-09-13 |
| [0023](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0023-whatsapp-dispositivo-vinculado.md) | WhatsApp pessoal por dispositivo vinculado (`garra whatsapp`) — bridge Node/Baileys por stdio + sessão cifrada | 2026-09-16 |
