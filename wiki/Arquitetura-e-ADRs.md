# Arquitetura e ADRs

## Visão geral

- [Architecture Overview](https://github.com/michelbr84/GarraRUST/blob/main/docs/architecture.md) — estrutura do workspace (22 crates), fluxo do runtime, pipeline de voz, multi-agente, memória, segurança, hot-reload
- [Referência da API REST do gateway](https://github.com/michelbr84/GarraRUST/blob/main/docs/src/api-reference.md) · [OpenAPI da API mobile](https://github.com/michelbr84/GarraRUST/blob/main/docs/mobile-api-v1.yaml)
- [Sistema de memória](https://github.com/michelbr84/GarraRUST/blob/main/docs/memory.md) · [Benchmarks](https://github.com/michelbr84/GarraRUST/blob/main/docs/src/benchmarks.md)

## ADRs — Architectural Decision Records

Decisões irreversíveis são registradas antes de implementar ([índice](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/README.md)). Todas **accepted**:

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
