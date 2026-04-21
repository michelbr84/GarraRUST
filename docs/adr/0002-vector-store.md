# 2. Vector store (`pgvector` vs `lancedb` vs `qdrant embedded`)

- **Status:** Accepted
- **Deciders:** @michelbr84 + Claude (sessão autônoma 2026-04-21; review: `@code-reviewer`)
- **Date:** 2026-04-21
- **Tags:** fase-2, rag, memory, vector-search, gar-372
- **Supersedes:** none
- **Superseded by:** none
- **Links:**
  - Issue: [GAR-372](https://linear.app/chatgpt25/issue/GAR-372)
  - Plan: [`plans/0030-adr-batch-unblock.md`](../../plans/0030-adr-batch-unblock.md)
  - Related ADR: [ADR 0003 (Postgres + pgvector accepted)](0003-database-for-workspace.md)
  - Roadmap: [ROADMAP §2.1 RAG & Embeddings](../../ROADMAP.md)
  - pgvector: <https://github.com/pgvector/pgvector>
  - LanceDB: <https://github.com/lancedb/lancedb>
  - Qdrant: <https://github.com/qdrant/qdrant>

---

## Context and Problem Statement

A Fase 2.1 do `ROADMAP.md` promete RAG e memória de longo prazo com embeddings locais (mxbai 768d recomendado). Precisamos de um **vector store** que:

1. Faça ANN top-5 em ≥ 100k embeddings com p95 ≤ 20 ms (§2.1).
2. Suporte filtragem por `group_id` + `scope` (user/group/chat) — multi-tenant real.
3. Seja operacionalmente coerente com a stack já aceita (ADR 0003 → Postgres 16 + pgvector).
4. Não exploda surface de backup/PITR/segurança adicionando sidecars não auditados.

Não é obvio se **mantemos pgvector puro** (já habilitado em migration 005 com HNSW cosine) ou se **adotamos um store dedicado** (`lancedb`, `qdrant embedded`) para latência menor e features avançadas (hybrid search, reranking nativo).

Esta decisão precisa fechar **antes** de criar o crate `garraia-embeddings` (planejado em Fase 2.1, issue GAR-372 mesmo).

---

## Decision Drivers

1. **★★★★★ Consistência com ADR 0003** — Postgres já é o sistema de registro. Adicionar outra DB de vetores duplica trilha de auditoria, backup e migração.
2. **★★★★★ Multi-tenant filtering** — filter-then-ANN ou ANN-then-filter tem que rodar DENTRO do mesmo sistema que aplica RLS por `group_id`. Se o vector store não conhece `group_id`, app-layer filtering pós-ANN vaza top-K items de outros tenants em latência alta.
3. **★★★★ Performance target** — 20 ms p95 top-5 @ 100k × 768d. Benchmark B4 do ADR 0003 mede **5.53 ms p95** com pgvector HNSW. Já estamos dentro do target, com folga.
4. **★★★ Hybrid search (ANN + FTS + filter)** — benchmark B5 do ADR 0003 mede **8.43 ms p95** para query combinada em uma CTE Postgres. Qualquer store dedicado ou duplica esse trabalho ou força federate-query.
5. **★★★ Embedabilidade (deployment)** — para casos edge (CLI offline, mobile) queremos opção embarcável que funcione sem Postgres.
6. **★★ Advanced features** — rerank nativo, sparse+dense, index tuning (M, ef), payload filtering eficiente.
7. **★★ Ecosystem fit** — SDKs Rust, testcontainers, docs.

---

## Considered Options

### A) `pgvector` apenas (status quo, accepted em ADR 0003)

**Pros:**
- ✅ Já habilitado (migration 005): HNSW cosine, 768d columns, 100k benchmark B4 → 5.53 ms p95.
- ✅ Filtering multi-tenant trivial: `WHERE group_id = $1 AND scope = $2 ORDER BY embedding <=> $q LIMIT 5` — usa RLS nativo + index composto.
- ✅ Hybrid queries (FTS + ANN + filter): uma CTE, zero federate. Benchmark B5 → 8.43 ms.
- ✅ Ops: pg_dump/pg_restore cobre TUDO (sessões + memórias + embeddings) no mesmo backup.
- ✅ `sqlx::query!` compile-time check estende a queries vetoriais.
- ✅ Zero nova dep runtime.

**Cons:**
- ⚠️ HNSW tuning (`m`, `ef_construction`) via `CREATE INDEX ... WITH (m=16, ef_construction=64)` — menos ergonômico que config programática de Qdrant.
- ⚠️ Embedabilidade: requer Postgres rodando, sem opção single-file para edge (mobile/CLI offline).
- ⚠️ Updates de embeddings (re-embedding após mudança de modelo) são `UPDATE ... SET embedding = $1` — locks de row padrão.

**Fit score:** 9/10. É a escolha óbvia dado ADR 0003.

### B) `LanceDB`

**O que é:** vector store embarcável, colunar, mmap, single-file (arquivos `.lance`). Rust nativo, feito por team ex-Google/HF. Suporta ANN HNSW/IVF, filter push-down, versionamento de dataset, zero-copy reads.

**Pros:**
- ✅ Embarcável: zero ops, roda dentro do processo.
- ✅ Colunar + mmap → excelente ler-muito-pouco-escrito (perfil RAG).
- ✅ Versionamento nativo (cada commit é imutável; rollback trivial).
- ✅ Rust-native API via `lance` + `lancedb` crates.
- ✅ SIMD paths (arrow2-based) performática.
- ✅ Filter push-down (executa filtro ANTES do ANN scan — correto para multi-tenant).

**Cons:**
- ⚠️ **Duplica o sistema de registro**: memórias vão para Postgres (text + metadata) E LanceDB (embedding). Dual-write + dual-backup + dual-consistency issues.
- ⚠️ Hybrid search com FTS exige dois round-trips (FTS em Postgres + ANN em Lance) ou duplicar texto também no Lance.
- ⚠️ Menos maturidade operacional (primeiro release estável 2024, PITR não-trivial).
- ⚠️ RLS app-layer only — proteção multi-tenant é convenção, não enforce de DB.
- ⚠️ Writes concorrentes cross-processes: locks de arquivo explícitos, não MVCC.

**Fit score:** 6/10. Bom para edge/offline; problemático como primary store.

### C) `Qdrant embedded`

**O que é:** Qdrant como lib Rust embarcável (modo `qdrant_lib` ou REST server local). Focado em vector search com payload filtering (boolean AND/OR sobre metadata), rerank, sparse+dense hybrid.

**Pros:**
- ✅ Payload filtering indexado e performático — ótimo para multi-tenant filter-then-search.
- ✅ Hybrid dense + sparse (BM25-like) nativo.
- ✅ ANN HNSW com tuning exposto via API.
- ✅ Momentum forte, comunidade ativa 2025-2026.

**Cons:**
- ⚠️ Duplica registro (mesmo problema de B).
- ⚠️ Modo embedded ainda é **less documented** — maioria dos usuários roda como server.
- ⚠️ Tamanho de binário: `qdrant_lib` adiciona ~20MB ao single-binary (matters for CLI distribution).
- ⚠️ Federate queries (FTS + ANN) requer app-layer join.

**Fit score:** 6/10. Bom em payload filtering; caro em ops.

### D) `pgvector` primary + `LanceDB` optional para edge (recommended)

**O que é:** pgvector continua sendo primary vector store (ADR 0003 já locked). `lancedb` documentado como opção de expansão quando surgir use case edge/offline real (e.g., `garraia-cli` em modo `--offline` ou mobile com RAG local sem backend).

**Pros:**
- ✅ Zero ops change hoje.
- ✅ Path de expansão documentado e consciente.
- ✅ Trigger de migração não-ambíguo: "se pgvector p95 > 50 ms @ 1M vectors OR surgir feature de edge RAG".

**Cons:**
- ⚠️ Complexidade futura se adotarmos dual store — mas isso é decisão posterior a este ADR.

**Fit score:** 9/10. Pragmático + future-proof.

---

## Decision Outcome

**Escolha: Opção D — `pgvector` como vector store primary, `LanceDB` documentado como opção para edge/offline quando aquele use case emergir.**

### Decisões específicas

1. **Crate `garraia-embeddings` (GAR-372 dependent)**: wraps pgvector via `sqlx`. Expõe um trait abstrato (`VectorStore`) com três operações assíncronas — `insert`, `search`, `delete` — escopadas por `Scope` (User/Group/Chat) + `group_id`. Design detalhado do trait fica no plano de implementação da Fase 2.1, não neste ADR. Implementação default: `PgVectorStore` (feature Cargo `store-pgvector`). `LanceDbStore` **não** ship em v1 — documentado como feature futura (`store-lancedb`) quando trigger bater.

2. **Embedding dimension**: 768 (mxbai-embed-large v1 quantizado Q8 → 768d). Column já exists em migration 005.

3. **Index tuning**: `m=16, ef_construction=64` (defaults pgvector 0.7). Expor tuning via `garraia-config` schema quando surgir necessidade.

4. **Hybrid queries (FTS + ANN + filter)**: CTE Postgres, zero federate. Padronizado em `garraia-embeddings::HybridQuery` builder.

5. **Re-embedding path**: quando mudar modelo de embedding, script migration SQL com `UPDATE memory_embeddings SET embedding = $1, embed_version = $2 WHERE embed_version < $2 LIMIT 1000` em batches. Documentado em plan futuro de Fase 2.1.

6. **Trigger de revisão (supersessão candidate)**:
   - pgvector p95 > 50 ms @ 1M vectors por grupo, OU
   - memória total cross-grupo > 100M vectors e backup pg_dump deixa de caber em janela de noite, OU
   - feature `garraia-cli --offline-rag` entrar no roadmap (requer store edge).

### Rationale resumido

1. **ADR 0003 já locked Postgres+pgvector**. Mudar agora é desalinhamento.
2. **Benchmark existente (B4: 5.53 ms p95)** cumpre target (20 ms) com 3.6x folga. Não há problema a resolver.
3. **RLS + filter-then-ANN** nativo Postgres > app-layer em qualquer store externo.
4. **Hybrid queries** em 8.43 ms p95 (B5) é extraordinário para 100k × 768d + FTS.
5. **Single source of truth** para backup/PITR/audit.
6. **LanceDB como opção futura** permite flexibilidade sem commit prematuro.

### Benchmark methodology caveat (citado do ADR 0003)

Os números B4 e B5 mencionados são do benchmark `benches/database-poc/` do ADR 0003, com as seguintes condições documentadas:

- **Plataforma**: Windows x86_64 com Docker Desktop rodando Postgres 16 + pgvector.
- **Dataset**: 100k linhas × 768d, embeddings sintéticos uniformes.
- **Metodologia**: medição wall-time via Criterion; para B4 o baseline SQLite roda um full scan por iteração, levando p50/p95/p99 a colapsar em um único tempo de parede (detalhe discutido em `benches/database-poc/results.md`).
- **Generalização**: estes números representam a **ordem de grandeza** em hardware moderno; variam com cache warming, HNSW tuning, dimensão do embedding, e distribuição real das queries em produção.

Reprodução empírica em produção real com dataset de usuários (quando Fase 2.1 estabilizar) é recomendada para revalidar com dados de domínio. Se a reprodução invalidar a projeção, ver §Supersession path.

---

## Consequences

### Positive

- Zero trabalho operacional novo (ADR 0003 já resolveu).
- Crate `garraia-embeddings` fica muito simples (wrap de sqlx queries).
- Hybrid search é um único SQL statement.
- Backup strategy unificada (pg_dump cobre tudo).

### Negative

- Se pgvector regressar em performance em release futuro, não temos Plan B já validado (mitigação: plan de migração para LanceDB documentado como trigger-driven).
- Deploy exige Postgres rodando, sem opção CLI-only (aceitável: `garraia-cli` local já usa SQLite para sessões via `garraia-db`; RAG offline fica em `pnpm roadmap` para quando virar feature).

### Neutral

- Trait `VectorStore` abstrato permite swap sem mudar callers.
- Embedding dimension fixo em 768 (por modelo atual); mudança futura exige migration.

---

## Supersession path

Este ADR **pode** ser superseded por um novo ADR (próximo número monotônico disponível em `docs/adr/`, convenção `NNNN-slug.md`) se qualquer trigger acima bater. A supersessão vai documentar benchmark comparativo e plano de migração.

---

## Links de referência

- pgvector HNSW tuning: <https://github.com/pgvector/pgvector#hnsw>
- ADR 0003 §Benchmark B4 (ANN p95): [0003-database-for-workspace.md](0003-database-for-workspace.md)
- Embedding model mxbai-embed-large-v1: <https://huggingface.co/mixedbread-ai/mxbai-embed-large-v1>
- LanceDB benchmarks: <https://lancedb.com/docs/concepts/vector-search>
