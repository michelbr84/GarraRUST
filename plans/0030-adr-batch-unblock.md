# Plan 0030 — ADR batch A: destravar cascatas Fase 1/2/3

**Status:** Em execução
**Autor:** Claude Opus 4.7 (sessão autônoma, ClaudeMaxPower + Superpowers)
**Data:** 2026-04-21 (America/New_York)
**Issues fechadas:** GAR-371, GAR-372, GAR-374, GAR-376
**Branch:** `feat/0030-adr-batch-unblock`

---

## 1. Goal

Escrever de uma só vez os 4 ADRs que hoje bloqueiam issues dependentes no backlog, transformando decisões já discutidas implicitamente no `ROADMAP.md` + `deep-research-report.md` em decisões **formalizadas e imutáveis** conforme regra 8 do `CLAUDE.md`.

O output é puramente documental: 4 arquivos em `docs/adr/` + atualização do índice, zero mudança de código ou dependência. A entrega fecha **4 issues Linear** (status Done) e **destrava outras 3** diretamente (GAR-394, GAR-395, GAR-387) além de habilitar planning de Fase 3.6 (search) e Fase 1.1 (TurboQuant+).

## 2. Non-goals

- **Não implementa** nenhum dos crates/módulos propostos (`garraia-storage`, `garraia-embeddings`, etc.).
- **Não roda PoCs empíricos** — os ADRs registram a recomendação inicial baseada no estado da arte + `deep-research-report.md`; benchmarks empíricos seguem em slices posteriores quando o código encostar no trade-off real.
- **Não escreve** ADRs 0007 (desktop frontend) e 0008 (doc collab) — Fase 4/3.8 são fronteiras mais longas e merecem plan próprio quando entrarem em ciclo ativo.
- **Não toca** em `ROADMAP.md` — apenas referencia.

## 3. Scope

Arquivos criados:
- `docs/adr/0001-local-inference-backend.md` (GAR-371, Fase 1.1)
- `docs/adr/0002-vector-store.md` (GAR-372, Fase 2.1)
- `docs/adr/0004-object-storage.md` (GAR-374, Fase 3.5)
- `docs/adr/0006-search-strategy.md` (GAR-376, Fase 3.6)

Arquivos atualizados:
- `docs/adr/README.md` — status de 4 ADRs passa de `📋 proposed` / `🔒 blocked` para `✅ accepted`, data `2026-04-21`.
- `plans/README.md` — adiciona entrada 0030.

## 4. Acceptance criteria

1. Cada um dos 4 ADRs tem todas as seções obrigatórias do formato MADR definido em `docs/adr/README.md`:
   - Status, Context, Decision Drivers, Considered Options, Decision Outcome, Consequences, Links.
2. Cada ADR tem no mínimo **3 opções consideradas** (prós/contras explícitos) e **rationale numerada** para a escolha.
3. `docs/adr/README.md` índice reflete status `✅ accepted` com data `2026-04-21` para cada um dos 4.
4. `plans/README.md` lista o plano 0030 com hyperlink funcional.
5. Links internos (`[GAR-371]`, etc.) seguem o padrão usado em 0003/0005.
6. Zero flake de Markdown lint (não temos CI de markdown; inspeção visual basta).
7. PR mergeado contém somente arquivos `docs/**.md` e `plans/*.md` (nada de código).

## 5. Design rationale (por ADR)

### 0001 — Local inference backend
- **Escolha recomendada:** `mistral.rs` como backend Rust nativo primário, com `llama.cpp` (via Ollama) mantido como fallback/compat layer.
- **Por quê:** `mistral.rs` entrega PagedAttention + Continuous Batching em Rust puro, sem subprocess overhead — exatamente o que a Fase 1.1 do ROADMAP pede. `candle` é mais genérico mas imaturo em batching. `llama.cpp` via Ollama é ótimo para compat mas perde o objetivo AAA de controle fino.

### 0002 — Vector store
- **Escolha recomendada:** `pgvector` (já instalado no `garraia-workspace`) como store primário até atingir trigger de escala, `lancedb` como opção embarcável para edge/offline scenarios (CLI, mobile).
- **Por quê:** ADR 0003 (accepted) já adota Postgres 16 + pgvector com benchmark B4 (5.53 ms p95 @ 100k × 768d). Adicionar outro store embutido multiplica surface de ops sem ganho mensurável. `lancedb` fica documentado como "ponto de expansão" para o dia que offline-first matter.

### 0004 — Object storage
- **Escolha recomendada:** trait `ObjectStore` com 3 impls shipped em v1: `LocalFs` (dev/single-machine), `S3Compatible` (AWS/Cloudflare R2/Backblaze B2), `MinIO` (self-host). Versionamento obrigatório, presigned URLs ≤ 15 min, criptografia em repouso (server-side para S3, filesystem-level para LocalFs).
- **Por quê:** `deep-research-report.md` §Files recomenda object storage com versionamento como padrão multi-tenant. Trait abstraction permite swap entre `LocalFs` → `MinIO` → cloud sem mudança de schema. Presigned URL é o único padrão que permite browser/mobile upload direto sem proxy de bytes pelo gateway (Fase 3.5 quer isso).

### 0006 — Search strategy
- **Escolha recomendada:** estratégia evolutiva de 3 fases — (1) Postgres FTS + pg_trgm até trigger, (2) Tantivy embedded quando FTS p95 > 300 ms OU > 10M messages/grupo, (3) Meilisearch sidecar quando busca cross-grupo / cross-workspace virar requisito explícito (não MVP).
- **Por quê:** inicia com zero dep nova (Postgres FTS já está funcional em migration 004). Triggers empíricos, não temporais. Tantivy é Rust-native e embarca, evita sidecar prematuro. Meilisearch fica para quando global search for feature vendida, não antes.

## 6. Work breakdown

| Task | Arquivo | Estimativa | Reviewer |
|---|---|---|---|
| T1 | `plans/0030-adr-batch-unblock.md` | 5 min | — |
| T2 | `docs/adr/0001-local-inference-backend.md` | 15 min | code-reviewer |
| T3 | `docs/adr/0002-vector-store.md` | 15 min | code-reviewer |
| T4 | `docs/adr/0004-object-storage.md` | 20 min | security-auditor + code-reviewer |
| T5 | `docs/adr/0006-search-strategy.md` | 15 min | code-reviewer |
| T6 | `docs/adr/README.md` update | 5 min | — |
| T7 | `plans/README.md` update | 3 min | — |
| T8 | Review pass + commit + PR | 10 min | Claude + agents |

Total: ~90 min linear. Pode-se paralelizar T2-T5.

## 7. Verification

- Leitura cruzada: cada ADR referencia outros quando há dependência (ex.: 0004 referencia 0003 para backend de file metadata).
- Links GAR-XXX válidos.
- `docs/adr/README.md` tabela sincroniza com arquivos existentes.

## 8. Rollback plan

100 % reversível: revert do commit. Como são doc-only, não há estado em DB/config/secrets a reverter. Se alguma decisão for revisada depois, o padrão MADR permite supersessão via ADR novo — o arquivo antigo fica como histórico.

## 9. Risk assessment

| Risco | Likelihood | Impact | Mitigação |
|---|---|---|---|
| ADR recommendation discordante do que emergir em PoC empírico | Médio | Baixo | ADR é imutável mas supersedable; convenção MADR cobre isso. |
| Decisão em 0001 (mistral.rs) ser invalidada por benchmark real | Baixo | Baixo | ADR marca a recomendação como "current evidence" e cita path de supersessão. |
| Scope creep no ADR 0004 puxando código de storage | Alto | Médio | Regra: zero código nesta PR. Enforcer via pre-PR check de arquivos tocados. |

## 10. Security review trigger

**security-auditor APPROVE** obrigatório antes do merge para ADR 0004 (storage, presigned URLs, crypto at rest, PII risk). Outros 3 ADRs não tocam surface de segurança — dispensam auditor específico, code-reviewer é suficiente.

## 11. Changelog notes

Next `.garra-estado.md` entry deve registrar:
- ADRs 0001, 0002, 0004, 0006 passaram de blocked/proposed para accepted.
- GAR-371, 372, 374, 376 fechadas.
- Desbloqueia GAR-394 + GAR-395 + GAR-387.

## 12. Open questions

Nenhuma — premissas resolvidas dentro dos próprios ADRs.
