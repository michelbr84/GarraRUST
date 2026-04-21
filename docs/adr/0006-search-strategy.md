# 6. Search strategy (Postgres FTS → Tantivy → Meilisearch)

- **Status:** Accepted
- **Deciders:** @michelbr84 + Claude (sessão autônoma 2026-04-21; review: `@code-reviewer`)
- **Date:** 2026-04-21
- **Tags:** fase-3, ws-search, fts, ann, gar-376
- **Supersedes:** none
- **Superseded by:** none
- **Links:**
  - Issue: [GAR-376](https://linear.app/chatgpt25/issue/GAR-376)
  - Plan: [`plans/0030-adr-batch-unblock.md`](../../plans/0030-adr-batch-unblock.md)
  - Related: [ADR 0003 Postgres + FTS](0003-database-for-workspace.md), [ADR 0002 Vector store](0002-vector-store.md)
  - Roadmap: [ROADMAP §3.6 Search](../../ROADMAP.md)

---

## Context and Problem Statement

A Fase 3.6 do ROADMAP pede **busca unificada** cobrindo mensagens, arquivos (metadata + conteúdo) e memórias — com filtros por grupo/escopo, suporte a operadores booleanos, e opcionalmente busca cross-group em workspaces que o usuário participa.

Já temos:

- **Postgres FTS** habilitado em migration 004 via `tsvector + GIN` (`messages_body_fts`).
- **pgvector HNSW** habilitado em migration 005 para ANN semantic search.
- **pg_trgm** habilitado para `LIKE` trigram performante.

O que **falta decidir**:

1. Até quando Postgres FTS sozinho atende? Qual o trigger para evoluir?
2. Quando sair do Postgres FTS, vamos para **Tantivy embedded** (Rust-native, bible-text-like ranking) ou diretamente para **Meilisearch sidecar** (typo-tolerant, UX fancier)?
3. Essa é uma decisão **evolutiva** (sobe degraus) ou **big-bang** (escolhe um destino desde o início)?

Decisão impacta planning de Fase 3.6 (issue GAR-376 mesmo), design de `garraia-workspace::search` module, e schema de migration futura (se Tantivy entrar, precisamos de ingest pipeline incremental).

---

## Decision Drivers

1. **★★★★★ Zero premature complexity** — CLAUDE.md regra "não adicionar abstração antes da dor real". Começar com Postgres FTS já feito > adotar sidecar preemptivamente.
2. **★★★★★ Data-driven triggers** — quando sair do Postgres tem que ser decisão métrica, não subjetiva.
3. **★★★★ Rust-native preferred** — reduz ops burden + evita rede/IPC. Tantivy > Meilisearch nesse eixo.
4. **★★★★ Hybrid search (FTS + ANN + filter)** — já funciona em Postgres via CTE (ADR 0003 benchmark B5 → 8.43 ms). Migrar para fora do Postgres quebra isso.
5. **★★★ UX features** — typo tolerance, prefix search, facets, highlights.
6. **★★★ Cross-group / workspace-global search** — requer index único ou federate. Feature de "grande" — não v1.
7. **★★ Multilingual** — stemming PT-BR + EN + ES (Postgres `simple` + `portuguese` dicts cobre; Tantivy tem tokenizers por idioma; Meilisearch auto-detecta).
8. **★★ Deploy simplicity** — sidecar adiciona operational surface.

---

## Considered Options

### A) **Big-bang: Meilisearch sidecar desde v1**

**Pros:**
- ✅ Typo tolerance, facets, highlights out-of-the-box.
- ✅ REST API simples.
- ✅ Ranking bem curado para UX.

**Cons:**
- ⚠️ Sidecar adiciona container, porta, backup separado.
- ⚠️ Ingest pipeline precisa ser built + mantido (CDC from Postgres).
- ⚠️ Hybrid (FTS + ANN) vira federate query → 2 round-trips.
- ⚠️ Postgres FTS já está funcional — desperdiçar o trabalho já feito.

**Fit score:** 5/10 para v1. Overhead sem dor provada.

### B) **Big-bang: Tantivy embedded desde v1**

**Pros:**
- ✅ Rust-native, embarcável.
- ✅ BM25 tuning granular.
- ✅ Ingest in-process (zero IPC).

**Cons:**
- ⚠️ Desperdiça Postgres FTS já shipado.
- ⚠️ Sem hybrid ANN nativo (precisa coordinate com pgvector).
- ⚠️ API é mais low-level que Meili — mais código de boilerplate.
- ⚠️ Sem typo tolerance out-of-the-box.

**Fit score:** 4/10 para v1. Overhead sem dor provada.

### C) **Stick with Postgres FTS forever**

**Pros:**
- ✅ Zero ops change.
- ✅ Hybrid trivial.

**Cons:**
- ⚠️ Em > 10M messages OU queries complexas com OR/proximity, FTS degrada para 100-500 ms p95.
- ⚠️ Sem typo tolerance.
- ⚠️ Sem facets eficientes sem agregar via SQL (caro).

**Fit score:** 7/10. Boa base, mas teto conhecido.

### D) **Evolutionary: Postgres FTS → Tantivy → Meilisearch** *(recommended)*

**O que é:** estratégia por fases, com triggers empíricos claros:

| Fase | Backend | Trigger de promoção |
|---|---|---|
| **S0 (hoje)** | Postgres FTS + pg_trgm + pgvector | — |
| **S1** | +Tantivy embedded (msg body + file text extraction) | FTS p95 > 300 ms OU > 10M messages/grupo |
| **S2** | +Meilisearch sidecar (cross-group, typo-tolerant UI search) | Feature "global search" aprovada como produto |

Estratégia coexistence:
- **S1 → S2**: Tantivy fica para queries hybrid + per-group FTS. Meilisearch só para cross-group / global.
- **Hybrid**: Postgres CTE fica o padrão até entrar em S1. Em S1, `garraia-workspace::search::hybrid` combina Tantivy FTS + pgvector ANN em Rust.

**Pros:**
- ✅ Zero operação nova em S0 (estado atual).
- ✅ Triggers métricos — não-subjetivos.
- ✅ Cada degrau tem valor incremental testável.
- ✅ Cada degrau é revertible (feature flag).

**Cons:**
- ⚠️ Docs precisa explicar "qual backend está ativo" — mitigação: `garraia-cli status search` reporta.
- ⚠️ Risk de ficar permanentemente em S0 e adiar decisão — mitigação: triggers automáticos (alerta Grafana quando bater).

**Fit score:** 9/10.

---

## Decision Outcome

**Escolha: Opção D — estratégia evolutiva em 3 fases com triggers empíricos.**

### S0 (current state, effective 2026-04-21)

- **Backend ativo:** Postgres FTS (tsvector + GIN) + pg_trgm para LIKE trigram + pgvector HNSW para ANN.
- **Módulo `garraia-workspace::search`** (a criar quando Fase 3.6 entrar) expõe um `Searcher` async com duas operações: `search_messages` (FTS-only) e `search_hybrid` (FTS + ANN + filter). Design de trait concreto fica no plano da Fase 3.6 — este ADR não dita assinatura de API.
- Impl: `PgSearcher` wraps um SQL CTE único que combina FTS + ANN + filter por `group_id`.

### S1 trigger → adota Tantivy

Trigger empírico **consolidado em escopo de "grupo"** (não de instância), para permitir alerta Prometheus acionável por grupo sem misturar escala total e escala crítica. Qualquer uma das condições basta:

1. **Latência per-grupo**: p95 de `search_messages` > **300 ms** sustentado por **7 dias** em **qualquer grupo com > 1M messages** (medido via Prometheus `search_query_duration_seconds{backend="pg_fts",group_id="..."}` quando `/metrics` já exporta — track GAR-411).
2. **Volume per-grupo**: qualquer grupo individual ultrapassa **10M messages** no tsvector index e FTS p95 sobe > 500 ms mesmo com cache quente.
3. **Janela de backup**: pg_dump da instância inteira passa a estourar janela de 45 min (indicador secundário de saturação global; se este disparar isolado, priorizar housekeeping de `messages_body_fts` + particionamento antes de assumir migração Tantivy).
4. **Feature `file_body_fts`** (extract de texto de PDF/DOCX para FTS) entra em roadmap — Tantivy lida melhor com documentos longos que tsvector single-row.

Em S1, adicionar crate `garraia-search-tantivy` com ingest:

- **CDC via LISTEN/NOTIFY** em Postgres triggers de `messages`/`files` → worker async popula Tantivy index em batches.
- **Durability**: Tantivy index é mmap segment files; backup incluir `/var/lib/garraia/search/tantivy/`.
- **Rollback**: feature flag `search-backend = "pg_fts" | "tantivy"` em `garraia-config`. Swap é zero-downtime (mantém ambos rodando 1 semana em shadow mode).

### S2 trigger → adota Meilisearch

Condições:

1. Feature produto "global search" (cross-group/workspace) aprovada formalmente no roadmap.
2. Demanda de UX typo-tolerant + instant search (< 50 ms p95 com 2 chars typed).

Em S2, sidecar Meilisearch com ingest similar (CDC from Postgres/Tantivy). Postgres FTS + Tantivy mantém para per-group queries; Meilisearch **apenas** para global/cross-group.

### Dashboards obrigatórios

Quando S0 estiver rodando em produção (Fase 6 soft launch):

- `search_query_duration_seconds{backend,quantile}` — p50, p95, p99 por backend.
- `search_index_size_bytes{backend,group_id}` — tamanho de index.
- `search_queries_total{backend,type}` — contagem por tipo (fts/ann/hybrid).

Alerta Prometheus: `search_query_duration_seconds{quantile="0.95",backend="pg_fts"} > 0.3` por 7d → pager para owner avaliar trigger S1.

### Multilingual strategy

Política de alto nível (detalhes de tokenizer/dicionário ficam em plano de implementação da Fase 3.6):

- **S0** usa Postgres text search configs (default: `portuguese` + `simple`) exposto em `groups.settings_jsonb.lang`.
- **S1** herda language awareness via tokenizers nativos Tantivy por idioma.
- **S2** delega ao auto-detect do Meilisearch.

PT-BR + EN são a base v1. Expansão para ES/FR/DE é incremento de config, não decisão arquitetural.

---

## Consequences

### Positive

- Zero ops burden hoje.
- Cada degrau tem ROI mensurável.
- Hybrid queries permanecem simples até S1.
- Multilingual cobre casos atuais (PT-BR + EN) sem trabalho extra.

### Negative

- Equipe precisa operar potencialmente 3 backends em S2 (mitigação: Postgres FTS vira fallback-only em S2).
- CDC ingest em S1 requer engenharia cuidadosa (mitigação: `testcontainers` garante parity antes de swap).

### Neutral

- Strategy é visível em `garraia-config` (`search.backend`, `search.tantivy.*`, `search.meili.*`).
- Supersession é explícita via um novo ADR (próximo número monotônico, convenção `NNNN-slug.md`) se trigger empírico invalidar a ordem.

---

## Supersession path

Re-visitar se:

- Postgres FTS performance drástica melhorar (pg17+ traz algo que mude o game).
- Tantivy project stall (unlikely — parte do Quickwit, ativo).
- Meilisearch API enshittification / licensing change.

---

## Links de referência

- Postgres FTS docs: <https://www.postgresql.org/docs/16/textsearch.html>
- pg_trgm: <https://www.postgresql.org/docs/16/pgtrgm.html>
- Tantivy: <https://github.com/quickwit-oss/tantivy>
- Meilisearch: <https://github.com/meilisearch/meilisearch>
- ADR 0002 vector store: [`0002-vector-store.md`](0002-vector-store.md)
- ADR 0003 Postgres benchmark B5 (hybrid CTE): [`0003-database-for-workspace.md`](0003-database-for-workspace.md)
