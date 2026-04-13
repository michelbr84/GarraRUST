# Plan 0002: GAR-373 — ADR 0003 Database para Group Workspace

> **Status:** 📋 Awaiting approval
> **Issue:** [GAR-373](https://linear.app/chatgpt25/issue/GAR-373)
> **Project:** Fase 3 — Group Workspace
> **Labels:** `epic:ws-schema`, `adr-needed`, `security`
> **Priority:** Urgent
> **Estimated session size:** 1 dia de trabalho focado (~4-6 horas)
> **Author:** Claude Opus 4.6 + @michelbr84
> **Date:** 2026-04-13
> **Depends on:** nada. **Unblocks:** GAR-386 a 390, GAR-407, GAR-408, GAR-391, GAR-393 e boa parte da Fase 3.

---

## 1. Goal (one sentence)

Produzir e mergear o ADR `docs/adr/0003-database-for-workspace.md` que decide qual banco de dados o `garraia-workspace` (multi-tenant Fase 3) vai usar — com benchmark reproduzível, plano de migração e critérios de reversibilidade — de modo que todos os issues bloqueados de schema/RLS/auth possam começar na sessão seguinte sem ambiguidade.

---

## 2. Rationale — por que esse agora

1. **Caminho crítico destravado.** Sete issues urgentes de Fase 3 (migrations 001-007, `garraia-auth`, API `/v1/groups`) estão literalmente travados aguardando essa decisão. Enquanto o ADR não existir, ninguém pode abrir `Cargo.toml` de `garraia-workspace` sem arriscar refactor total depois.
2. **Baixíssimo risco.** O output é um documento + um script de benchmark + um feature branch com PoC efêmero. Não toca código de produção. Reversível via `git rm`.
3. **Alavancagem máxima por hora.** 4-6h produzem uma decisão que destrava ~40 horas-homem de trabalho subsequente.
4. **Timing ótimo.** Com o commit `84c4753` (GAR-384) já temos OpenTelemetry rodando — dá para instrumentar o benchmark e medir latência p95 com métricas reais, não palpites.
5. **Dívida existente.** O CLAUDE.md e o ROADMAP já afirmam "Postgres recomendado" sem ADR. Formalizar remove ambiguidade e força o confronto de edge cases (migração SQLite → Postgres, dual-backend dev/prod, teste de RLS).

---

## 3. Scope & Non-Scope

### In scope

- **ADR escrito** em `docs/adr/0003-database-for-workspace.md` seguindo template [MADR 3.0](https://adr.github.io/madr/) (ou um subset minimalista equivalente).
- **PoC de benchmark reproduzível** em `benches/database-poc/` com:
  - Script Rust usando `sqlx` contra Postgres 16 + pgvector via testcontainers.
  - Script Rust paralelo usando `rusqlite` contra SQLite em tmpfs.
  - Cenários: insert 100k mensagens, query full-text (tsvector vs FTS5), query ANN (pgvector HNSW vs sqlite-vec), cross-group isolation via RLS (Postgres apenas).
  - Saída: `benches/database-poc/results.md` com p50/p95/p99 por cenário.
- **Decisão registrada** no ADR com contexto, opções, decisão, consequências, status.
- **Migration strategy** documentada: como usuários atuais com SQLite fazem upgrade para Postgres (one-shot import via `garraia-cli migrate`, ou dual-write temporário, ou backup/restore).
- **Compatibilidade de ferramentas** mapeada: `sqlx-cli` vs o ecossistema atual `rusqlite`, `tokio::sync::Mutex<SessionStore>` vs pool async nativo.
- **Update do ROADMAP.md** Fase 3.1 marcando GAR-373 como `[x]` e linkando o ADR.
- **Linear**: mover GAR-373 para Done, remover label `adr-needed` dos issues filhos uma vez que ADR esteja merged.
- **Atualização de `docs/adr/README.md`** (criar se não existir) com índice dos ADRs.

### Out of scope

- ❌ **Escrever código de produção em `garraia-workspace`.** Esse é o próximo plan (0003).
- ❌ **Decidir vector store** (lancedb vs qdrant vs pgvector como tudo-em-um). Esse é ADR 0002 (GAR-372).
- ❌ **Decidir identity provider** (interno vs OIDC). Esse é ADR 0005 (GAR-375).
- ❌ **Migrar o SQLite atual** (`sessions`, `mobile_users`, etc.) para Postgres. O ADR define a estratégia, a execução vem em issues separadas.
- ❌ **Setup de produção Postgres** (HA, PITR, read replicas). Fase 6.
- ❌ **Adoção de CockroachDB/MySQL/MongoDB** como default. Pode ser discutida no ADR como "alternativas consideradas", mas decisão recomendada é Postgres pela literatura + deep-research-report.md.

---

## 4. Acceptance criteria (verificáveis)

- [ ] `docs/adr/0003-database-for-workspace.md` existe, > 200 linhas, segue template MADR.
- [ ] O ADR tem seções: `Context`, `Decision Drivers`, `Considered Options`, `Decision Outcome`, `Pros and Cons`, `Consequences`, `Links`.
- [ ] Pelo menos **4 opções** comparadas: Postgres 16 + pgvector, SQLite 3 + sqlite-vec, CockroachDB, PlanetScale/MySQL (rejected — mas comparado).
- [ ] **Benchmark rodado** com testcontainers e números registrados em `benches/database-poc/results.md`.
- [ ] O benchmark é reproduzível em uma máquina limpa via `cargo run --manifest-path benches/database-poc/Cargo.toml --release` (e esse comando está documentado no ADR e no README do benches).
- [ ] **RLS cross-group** é testado no benchmark (tentativa de SELECT do grupo B a partir de sessão do grupo A retorna 0 rows — prova que a defesa em profundidade funciona no PG).
- [ ] **Estratégia de migração SQLite → Postgres** documentada em pelo menos 3 parágrafos, incluindo: ferramenta (`garraia-cli migrate`), handling de dados existentes, rollback.
- [ ] **Decisão explícita** (não "a ser definido"): qual banco, qual versão mínima, quais extensões (pgvector, pg_trgm, uuid-ossp).
- [ ] ROADMAP.md Fase 3 atualizado marcando GAR-373 como `[x]` com link.
- [ ] `docs/adr/README.md` criado (ou atualizado se já existir) com índice.
- [ ] PR aberto no GitHub linkando issue GAR-373.
- [ ] Review feito por `@code-reviewer` E `@security-auditor` (security porque RLS é decisão de segurança).
- [ ] Linear GAR-373 movida para Done após merge.

---

## 5. File-level changes

### 5.1 Novos arquivos

```
docs/
  adr/
    README.md                             # índice dos ADRs (novo ou atualizado)
    0001-local-inference-backend.md       # stub placeholder (GAR-371, issue futura)
    0002-vector-store.md                  # stub placeholder (GAR-372, issue futura)
    0003-database-for-workspace.md        # ★ o ADR principal deste plano
benches/
  database-poc/
    Cargo.toml                            # crate isolado, NÃO membro do workspace
    README.md                             # como rodar
    src/
      main.rs                             # CLI que dispara todos os cenários
      postgres_scenarios.rs               # sqlx + testcontainers
      sqlite_scenarios.rs                 # rusqlite + tempfile
      shared.rs                           # fixtures: 100k mensagens, 1k grupos
    results.md                            # saída versionada do benchmark
```

**Decisão intencional:** `benches/database-poc/` é um **crate isolado**, NÃO um membro do workspace `garraia`. Motivo: dependências de benchmark (sqlx + testcontainers + criterion) não devem bleed no build normal do gateway. É código efêmero — quando o PoC sair do lugar, deletamos o diretório inteiro.

### 5.2 Edits em arquivos existentes

- `ROADMAP.md` §3.1: marcar GAR-373 como `[x]` e apontar para o ADR.
- `CLAUDE.md` Regras absolutas: não precisa mudar (regra 8 já exige ADR antes de decisão arquitetural).
- `.gitignore`: nenhuma mudança (benches/ não está ignorado — é código de projeto).

### 5.3 Nada em production code

- **Zero** linhas mexidas em `crates/`, `apps/`, `src/`.
- Todas as mudanças são docs + PoC isolado.

---

## 6. ADR outline (o que vai no arquivo)

Abaixo o **esboço estrutural** do `0003-database-for-workspace.md` — NÃO é o conteúdo final, é o roteiro para a sessão de execução.

```markdown
# 3. Database para Group Workspace (garraia-workspace)

- Status: proposed → accepted após aprovação deste plano
- Deciders: @michelbr84, review @code-reviewer + @security-auditor
- Date: 2026-04-XX
- Tags: fase-3, multi-tenant, security, ws-schema

## Context and Problem Statement

Garra é hoje mono-usuário: `garraia-db` wraps rusqlite e atende 1 instalação local.
A Fase 3 do ROADMAP AAA (Group Workspace) transforma Garra em multi-tenant com:
- Grupos/membros com RBAC
- Arquivos compartilhados escopados por group_id
- Chats compartilhados + FTS
- Memória IA de 3 níveis (user / group / chat)
- Tasks + docs colaborativos (Notion-like)
- Audit trail LGPD/GDPR-compliant

Precisamos escolher o database backend antes de escrever qualquer migration
ou crate novo. A escolha impacta: schema design (RLS nativo vs app-level
isolation), performance de FTS e ANN, operação (backup, replicas, PITR),
custo de ferramentas (sqlx vs rusqlite async), e migration path para usuários
atuais do SQLite.

## Decision Drivers

1. **Isolamento multi-tenant** com defense-in-depth (RLS nativo ideal)
2. **Full-text search** nativo, performante até 10M+ mensagens
3. **Vector search** para memória IA (pgvector, sqlite-vec, ou sidecar)
4. **Operação** (backup, PITR, replicas, observabilidade)
5. **Ecossistema Rust** (sqlx async, migrations, type safety)
6. **Path de migração** para usuários atuais em SQLite
7. **Self-host friendly** (família rodando Garra em Raspberry Pi)
8. **Compliance** (LGPD art. 46, GDPR art. 32 — segregação demonstrável)

## Considered Options

### A) PostgreSQL 16 + pgvector + pg_trgm  (RECOMENDADO)
### B) SQLite 3 + sqlite-vec + FTS5
### C) CockroachDB (SQL distribuído)
### D) MySQL 8 / MariaDB

(Cada uma com: how it works, pros, cons, benchmark score, fit score.)

## Decision Outcome

Chosen option: "PostgreSQL 16 + pgvector + pg_trgm", because ...

### Rationale (3-5 parágrafos)

### Consequences

#### Positive
- RLS nativo via `CREATE POLICY` → defense-in-depth trivial
- FTS via tsvector/GIN sólido até dezenas de milhões de rows
- pgvector com HNSW resolve memória IA sem sidecar separado
- sqlx async com type-checked queries via macro `sqlx::query!`
- PITR via WAL archiving documentado e testado na comunidade
- Grande mercado de hosted managed (Supabase, Neon, RDS)

#### Negative
- Operação mais pesada que SQLite single-file
- Self-host em Raspberry Pi funciona mas exige tuning
- Migração do SQLite atual precisa ferramenta dedicada

#### Neutral / Mitigado
- Dev single-user continua usando SQLite via feature flag `workspace-backend-sqlite` (experimental) — **decidir neste ADR se isso fica ou não**

### Migration Strategy (SQLite → Postgres)

(3+ parágrafos: garraia-cli migrate, one-shot vs dual-write, rollback,
retenção dos dados existentes, feature flag durante transição.)

## Validation

Benchmark executado em `benches/database-poc/` em 2026-04-XX:

(Tabela: cenário × backend × p50 × p95 × p99 × throughput)

RLS cross-group test: PASS (PG bloqueia SELECT direto, SQLite não tem equivalente).

## Links

- [GAR-373](https://linear.app/chatgpt25/issue/GAR-373) — issue origem
- [GAR-407](https://linear.app/chatgpt25/issue/GAR-407) — schema bootstrap (unblocked)
- [GAR-408](https://linear.app/chatgpt25/issue/GAR-408) — RLS migration (unblocked)
- [deep-research-report.md](../../deep-research-report.md) §"Comparativo de bancos"
- [PostgreSQL 16 docs](https://www.postgresql.org/docs/16/)
- [pgvector](https://github.com/pgvector/pgvector)
- [sqlx](https://docs.rs/sqlx/)
```

---

## 7. Benchmark plan

### 7.1 Cenários obrigatórios

| # | Cenário | Métrica | Backend |
|---|---|---|---|
| B1 | Insert 100k messages (batched, 500 por commit) | throughput msg/s, p95 latency | PG, SQLite |
| B2 | FTS query "palavra comum" em 1M rows | p50/p95/p99 latency | PG (tsvector+GIN), SQLite (FTS5) |
| B3 | FTS query "termo raro" em 1M rows | p50/p95/p99 latency | PG, SQLite |
| B4 | ANN search top-5 sobre 100k embeddings 768d | p95 latency | PG (pgvector HNSW), SQLite (sqlite-vec) |
| B5 | Hybrid query: FTS + ANN + filter by group_id | p95 latency | PG apenas (referência) |
| B6 | Cross-group RLS test: SELECT direto por ID de outro grupo | must return 0 rows | PG apenas |
| B7 | Connection pool stress: 100 conns concorrentes, mix read/write | p95 latency, error rate | PG apenas (SQLite WAL single-writer não escala) |

### 7.2 Hardware target

- Baseline: a máquina de dev do @michelbr84 (documentar specs no `results.md`).
- Rodar ambos backends em containers no MESMO host para ser comparação justa.
- PG via `testcontainers-rs` image `pgvector/pgvector:pg16`.
- SQLite em `tempfile::tempdir()` em tmpfs se Linux, fallback para disco local em Windows.

### 7.3 Sample data generation

- 1M mensagens sintéticas via `fake-rs` crate — texto em português com distribuição Zipfian.
- 1k grupos × 10 membros cada.
- 100k embeddings aleatórios dimensão 768 normalizados.
- Seed fixo para reproduzibilidade.

### 7.4 Reporting

- `benches/database-poc/results.md` com:
  - Specs da máquina
  - Timestamp
  - Tabela Markdown dos 7 cenários
  - Interpretação (parágrafo por cenário)
  - Conclusão agregada (1 frase por backend)

---

## 8. Rollback plan

ADRs são docs, não código. Rollback tem 3 níveis:

1. **Antes de merge:** fechar o PR sem ações.
2. **Depois de merge, antes de qualquer código downstream:** `git revert` do commit do ADR + `plans/0002` marked superseded.
3. **Depois de `garraia-workspace` existir:** o ADR vira histórico — se a decisão se provar errada, escreve-se um ADR 0003.1 ou 0003-superseded que referencia o 0003 e inverte com rationale.

O benchmark `benches/database-poc/` pode ser deletado a qualquer momento sem impacto — ele é código efêmero de research.

---

## 9. Risks & mitigations

| Risco | Prob. | Impacto | Mitigação |
|---|---|---|---|
| Benchmark roda mas resultados são inconclusivos | Média | Médio | Usar testcontainers com image fixa + seed fixo + rodar 3 vezes e usar mediana. Se ainda inconclusivo, documentar que ambos backends servem e escolher PG por outros drivers (ecosystem, RLS, pgvector). |
| Postgres perder feio em latência pura vs SQLite em cenário B1/B2 | Baixa | Alto (abalaria a recomendação) | Ajustar tuning default (`shared_buffers`, `work_mem`, `effective_cache_size`) e documentar. Ainda assim, RLS + FTS + pgvector são decisivos mesmo com 2x overhead. |
| testcontainers-rs falhar no Windows do @michelbr84 | Média | Médio | Fallback: rodar `docker compose up` manual com o mesmo image e apontar o bench via env var `DATABASE_URL`. |
| ADR virar fanfic sem decisão clara ("depende") | Baixa | **Alto** | Critério de aceite §4 exige "decisão explícita, não a ser definido". Review gate. |
| Migration path SQLite → PG ser tão custoso que invalida escolha | Baixa | Alto | Desenhar path no ADR com PoC textual (pseudo-código de `garraia-cli migrate`); validar que o path existe antes de mergear. |
| pgvector 0.7+ ter quirks de compat com sqlx 0.8 | Média | Médio | Validar sqlx ↔ pgvector ↔ HNSW no PoC antes de escrever o ADR. |

---

## 10. Sequence of work (ordem proposta quando aprovado)

1. **Criar diretório `benches/database-poc/`** (~15 min)
   - `Cargo.toml` isolado (NÃO membro do workspace)
   - `README.md` com instruções de execução
   - `src/main.rs` stub com clap para selecionar cenário

2. **Fixtures e data generation** (~45 min)
   - `shared.rs` com geradores determinísticos (1M msgs, 100k embeddings, 1k groups)
   - Seed fixo via `StdRng::seed_from_u64(42)`

3. **Postgres scenarios** (~1.5 h)
   - `postgres_scenarios.rs` com testcontainers `pgvector/pgvector:pg16`
   - Schema DDL inline
   - Cenários B1-B7
   - Medições via `std::time::Instant` — OTel integration opcional

4. **SQLite scenarios** (~1 h)
   - `sqlite_scenarios.rs` com `tempfile::tempdir()`
   - Schema DDL inline (FTS5 virtual table + sqlite-vec)
   - Cenários B1-B4 (B5-B7 são N/A)

5. **Executar benchmarks** (~30 min)
   - `cargo run --release` no bench
   - 3 runs, guardar mediana
   - Escrever `results.md`

6. **Escrever o ADR** (~1.5 h)
   - Preencher o template do §6 com dados reais do benchmark
   - Decisão explícita
   - Migration strategy completa

7. **Atualizar ROADMAP.md e criar docs/adr/README.md** (~20 min)
   - Marcar GAR-373 como `[x]` em Fase 3.1
   - Índice dos ADRs

8. **Review pass** (~20 min)
   - Spawn `@code-reviewer` + `@security-auditor` em paralelo
   - Endereçar findings

9. **Commit + PR + Linear Done** (~15 min)

**Total estimado: 5-6 horas de trabalho focado.**

---

## 11. Definition of Done

- [ ] `docs/adr/0003-database-for-workspace.md` merged em `main`.
- [ ] `docs/adr/README.md` com índice atualizado.
- [ ] `benches/database-poc/` committed com `results.md` versionado.
- [ ] `ROADMAP.md` §3.1 atualizado marcando GAR-373 como `[x]`.
- [ ] PR mergeado com review verde de `@code-reviewer` + `@security-auditor`.
- [ ] Linear GAR-373 → **Done**.
- [ ] Follow-up issue criada se o benchmark revelou surpresas (ex.: "pgvector + sqlx 0.8 tuning necessário").
- [ ] Os issues bloqueados (GAR-386 a 390, 407, 408, 391, 393) tiveram o label `blocked-by-adr` (se existir) removido.

---

## 12. Open questions (preciso da sua resposta antes de começar)

1. **Opção de dev backend dual:** devo manter um feature flag `workspace-backend-sqlite` permitindo rodar `garraia-workspace` contra SQLite em dev (single-user, sem RLS), ou a decisão é "Postgres only, inclusive em dev"? Recomendo **Postgres only** com docker-compose local — dual-backend dobra testes e convida drift. Família/self-host com Raspberry Pi 4+ já roda PG16 sem dor.

2. **Versão mínima de Postgres:** pinning em **16** ou aceito 14+? Recomendo **16 only** — ganhos de performance do incremental sort + scalar MERGE + pgvector HNSW maduro. Usuários em 14 fazem upgrade (documentado no migration strategy).

3. **Escopo do benchmark:** fazer os 7 cenários (B1-B7, ~3h de execução + análise) ou reduzir para B1+B2+B4+B6 (core + RLS, ~1.5h)? Recomendo **os 7** — a sobra de tempo vem da Rationale mais robusta, e B5/B7 são exatamente onde SQLite desmorona. Se você quiser cortar para encaixar na sessão, B3 (termo raro) e B7 (conn stress) são os corta-primeiro.

4. **PoC crate dentro ou fora do workspace?** Recomendo **fora** (`benches/database-poc/Cargo.toml` não é `members.*`) para não poluir o build do gateway com deps pesadas (testcontainers + sqlx + criterion ~300 MB de dependências baixadas). Trade-off: perde `cargo check --workspace` automático para o bench. Aceito — ele é efêmero.

5. **Vale envolver algum agente especializado no benchmark?** O code-reviewer é útil na revisão do ADR, mas o benchmark é research puro — sugiro que eu execute direto sem delegar para agente. Se você preferir delegar, uso um `general-purpose` agent para o cenário Postgres e outro para SQLite em paralelo.

---

## 13. Next recommended issue (depois de GAR-373 merged)

Com o ADR no ar, o caminho crítico destrava e as opções imediatas são:

- **GAR-407 — `garraia-workspace` + migration 001** (3-5 dias) — transforma o ADR em schema real. **Caminho crítico da Fase 3.**
- **GAR-379 — `garraia-config` reactive hot-reload** (2-3 dias) — Fase 1.3, não bloqueia Fase 3 mas facilita tudo (OTel config reloaded sem restart seria imediato benefício).
- **GAR-410 — CredentialVault final** (2-3 dias) — Fase 5.1, security dívida histórica.
- **GAR-411 — Telemetry hardening** (1-2 dias) — follow-ups do GAR-384.

Recomendação: **GAR-407 imediatamente** — o ADR foi escrito para viabilizar isso.

---

**Aguardando sua aprovação.** Se aprovar como está, respondo as 5 open questions do §12 com meus defaults (a menos que você ajuste) e começo pelo passo 1 do §10. Se quiser cortar escopo (ex.: "pula o benchmark, aceita literatura"), me diga — mas aviso que um ADR sem dados empíricos envelhece mal.
