# Architectural Decision Records (ADRs)

Decisões arquiteturais do GarraIA seguem o formato [MADR](https://adr.github.io/madr/) simplificado. Cada ADR é imutável após merge — se a decisão mudar, escreve-se um novo ADR que superseda o antigo.

## Quando escrever um ADR

Per `CLAUDE.md` regra 8: **sempre** antes de uma decisão arquitetural irreversível. Exemplos:

- Escolha de database backend
- Escolha de vector store
- Protocolo de autenticação
- Framework de UI
- Runtime de sandbox
- Estratégia de deployment

Se a decisão é fácil de reverter (ex.: qual biblioteca de logging usar), ADR é overkill.

## Convenção de nome

`NNNN-slug-em-kebab-case.md`

- `NNNN` — sequencial monotônico. Primeira decisão é `0001`.
- `slug` — identificador humano curto.

## Formato mínimo

Cada ADR tem essas seções:

1. **Status** — proposed | accepted | superseded | deprecated
2. **Context and Problem Statement** — o que estamos decidindo e por quê
3. **Decision Drivers** — critérios ponderados
4. **Considered Options** — alternativas com prós/contras
5. **Decision Outcome** — a escolha + rationale
6. **Consequences** — positive / negative / neutral
7. **Links** — issues, PRs, benchmarks, docs externos

Rationale curta ("porque sim") é sinal de que a decisão não deveria ser ADR — ou que falta pensamento.

## Index

| # | Title | Status | Date | Issue |
|---|---|---|---|---|
| [0001](0001-local-inference-backend.md) | Local inference backend (candle vs mistral.rs vs llama.cpp) | ✅ accepted | 2026-04-21 | [GAR-371](https://linear.app/chatgpt25/issue/GAR-371) |
| [0002](0002-vector-store.md) | Vector store (pgvector vs lancedb vs qdrant embedded) | ✅ accepted | 2026-04-21 | [GAR-372](https://linear.app/chatgpt25/issue/GAR-372) |
| [0003](0003-database-for-workspace.md) | Database para Group Workspace (Postgres vs SQLite) | ✅ accepted | 2026-04-13 | [GAR-373](https://linear.app/chatgpt25/issue/GAR-373) |
| [0004](0004-object-storage.md) | Object storage (S3 compatible, MinIO default) | ✅ accepted | 2026-04-21 | [GAR-374](https://linear.app/chatgpt25/issue/GAR-374) |
| [0005](0005-identity-provider.md) | Identity Provider (BYPASSRLS role + Argon2id + HS256 + lazy upgrade) | ✅ accepted | 2026-04-13 | [GAR-375](https://linear.app/chatgpt25/issue/GAR-375) |
| [0006](0006-search-strategy.md) | Search strategy (Postgres FTS → Tantivy → Meilisearch) | ✅ accepted | 2026-04-21 | [GAR-376](https://linear.app/chatgpt25/issue/GAR-376) |
| [0007](0007-desktop-frontend.md) | Desktop frontend (HTML+Vanilla baseline → SolidJS trigger) | ✅ accepted | 2026-04-21 | [GAR-377](https://linear.app/chatgpt25/issue/GAR-377) |
| [0008](0008-doc-collaboration.md) | Doc collaboration (Tier 1 single-editor → y-crdt Tier 2) | ✅ accepted | 2026-04-21 | [GAR-378](https://linear.app/chatgpt25/issue/GAR-378) |

Legenda: ✅ accepted · 📋 proposed (aguardando execução) · 🔒 blocked (issue Linear aguardando este ADR ser escrito).

## Histórico de supersessões

Nenhuma até o momento.
