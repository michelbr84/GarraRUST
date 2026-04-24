# Plano — Modo de Execução Máxima GarraRUST (foamy-origami)

> Objetivo: elevar a barra de qualidade técnica do monorepo GarraRUST
> (Rust workspace 19 crates + Flutter mobile) usando capacidades reais
> de ClaudeMaxPower + Superpowers + awesome-claude-token-stack,
> com agentes de papéis distintos, métricas objetivas e rastreabilidade
> Linear/GitHub Actions.
>
> **Estado:** **APROVADO em 2026-04-23** em modo conservador (executar
> Lote 0 → Lote 1 → reavaliar). Aplicados 4 ajustes pós-aprovação:
> (1) Lote 0 all-or-nothing sobre wiring acts MCP — se falhar vira
> investigação isolada; (2) Lote 3 sem serviço externo de coverage —
> apenas artifact + `gh pr comment`; (3) Lote 4 partido em 4a (fácil)
> + 4b (wasmtime em sub-plan próprio `0050.4`); (4) Lote 6 pulverizado
> em PRs pequenos (≤ 500 LOC movidos por PR).

---

## Context

O usuário pediu "modo de execução máxima" com disciplina de engenharia sênior.
Um `/max-power` já havia ativado ClaudeMaxPower (hooks, agent teams,
permissões, Auto Dream). O repositório GarraRUST é um gateway de IA Rust
maduro, com 51 plans no `plans/`, Fase 3 (Postgres + pgvector multi-tenant)
recém-encerrada no épico GAR-395 (plan 0047 TUS slice 3 mergeado em
96f5c03), mas traz dívida técnica material:

* clippy strict só foi ligado agora (plan 0049 / GAR-429);
* CI tem 3 jobs não-bloqueantes (`continue-on-error: true`) — e2e, playwright
  e security audit — mascarados como verdes;
* 13 advisories RUSTSEC vivos (ignore expira 2026-05-20);
* zero instrumentação de cobertura, zero mutation testing, zero fuzz;
* hotspots de tamanho não-triviais (`admin/handlers.rs` 3.300 LOC,
  `bootstrap.rs` 2.405 LOC, `migrate_workspace.rs` 1.590 LOC);
* crates sem testes de integração: `garraia-voice`, `garraia-agents`,
  `garraia-channels`, `garraia-security`, `garraia-plugins`;
* `awesome-claude-token-stack` está instalado (DB `.acts/store.db` de 176 KB
  com 1 observação-seed), mas as tools MCP `acts__*` não aparecem na lista
  de ferramentas deferidas desta sessão — wiring parcial.

O outcome desejado é um **plano incremental e verificável** que endereça:
(1) quality gates de CI, (2) cobertura + mutação, (3) refactor dos
hotspots, (4) wiring real do token-stack como observabilidade do próprio
workflow Claude, (5) triagem dos RUSTSEC antes da deadline, (6) rastreio
no Linear sem criar churn.

---

## A. Capability Gate — status + evidências

Matriz honesta do que está **ativo**, **configurado mas ocioso** e
**ausente**. Evidência concreta em cada linha.

### A.1 ClaudeMaxPower — ATIVO

| Item | Status | Evidência |
|---|---|---|
| `/max-power` skill | Executado | `C:\Users\miche\.claude\cmp-skills\max-power.md` (377 linhas), rodado neste turno |
| Hooks | 4/4 ativos | `.claude/hooks/{session-start,pre-tool-use,post-tool-use,stop}.sh` chmod +x, `setup.sh` verificou ✓ |
| Agent Teams | Ativo | `.claude/settings.json`: `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` |
| Permissões preset | Aplicadas | `allow`: git/cargo/python3/gh/jq/flutter/dart/npx/npm/openclaw; `deny`: `rm -rf /*` e `rm -rf ~*` |
| Layered CLAUDE.md | Ativo | `CLAUDE.md` (raiz, 243 linhas) + `.claude/superpowers-config.md` presentes |
| Auto Dream | Configurado | `~/.claude/settings.json.autoDreamEnabled=true`; gatilho atual 1/5 sessões |
| Agents locais | 4/4 carregados | `.claude/agents/{code-reviewer,security-auditor,doc-writer,team-coordinator}.md` |
| Skills locais | 14/14 listadas | `skills/*.md` + banner session-start enumera 14 |
| Worktrees | Disponível | `using-worktrees` skill carregada + `.claude/worktrees/` existe |

**Uso real nesta sessão:** `/max-power` rodou `scripts/setup.sh` (preflight
não-destrutivo), leu `CLAUDE.md`, enumerou hooks/agents/skills,
disparou 3 subagentes Explore em paralelo (Fase 1 deste plano).

### A.2 Superpowers plugin oficial — INSTALADO v5.0.7

| Item | Status | Evidência |
|---|---|---|
| Plugin oficial | Instalado | `~/.claude/plugins/installed_plugins.json` → `superpowers@claude-plugins-official` v5.0.7, commit `917e5f5`, installed 2026-04-14 |
| Instância em disco | Presente | `~/.claude/plugins/data/superpowers-claude-plugins-official/` |
| Marketplace | Configurado | `~/.claude/settings.json.enabledPlugins.superpowers@claude-plugins-official=true` + `extraKnownMarketplaces.superpowers-marketplace → obra/superpowers-marketplace` |
| rust-analyzer-lsp | Instalado | mesma origem, v1.0.0 |
| Skills obrigatórias carregadas | 15+ | `superpowers:{using-superpowers, brainstorming, writing-plans, executing-plans, verification-before-completion, systematic-debugging, test-driven-development, requesting-code-review, receiving-code-review, using-git-worktrees, subagent-driven-development, finishing-a-development-branch, writing-skills, dispatching-parallel-agents}` listadas na seção `available-skills` deste turno |

**Uso real já neste turno:** `using-superpowers` (meta-skill — skill gate
executado pelo system reminder). Fui orientado a usar skills antes de
responder e fiz isso.

**Skills que serão obrigatórias na execução** (bind 1-para-1 com fases
do plano abaixo):

| Skill | Quando será usada |
|---|---|
| `using-worktrees` | Antes de cada lote (isola alterações; ADR-friendly) |
| `writing-plans` | Quebrar cada fase em tasks executáveis por subagent |
| `subagent-dev` | Executar as tasks do plan com fresh subagent por task |
| `test-driven-development` / `tdd-loop-lite` | Todo código novo de produção com RED-GREEN-REFACTOR |
| `systematic-debugging` | Root-cause de bugs encontrados (ex. e2e startup) |
| `verification-before-completion` | Gate obrigatório antes de marcar task como done |
| `requesting-code-review` + `receiving-code-review` | Antes de cada PR |
| `pre-commit` | Antes de qualquer commit |
| `finishing-a-development-branch` | Após todos os testes verdes |
| `generate-docs` | Documentar novos quality gates e wiring |
| `refactor-module` | Para os 3 hotspots (`admin/handlers.rs`, `bootstrap.rs`, `migrate_workspace.rs`) |

### A.3 awesome-claude-token-stack — PARCIALMENTE ATIVO

| Item | Status | Evidência |
|---|---|---|
| Repo checkout | Presente | `G:/Projetos/awesome-claude-token-stack/` (bun lockfile, `packages/{cli,compress,core,mcp,memory,observe,sandbox}`) |
| DB `.acts/store.db` | Presente (176 KB, 23 tabelas) | `G:/Projetos/GarraRUST/.acts/store.db`; `_acts_meta.schema_version=1`; FTS5 ativo (`content_fts*`, `observations_fts*`) |
| MCP wiring | Configurado | `~/.claude/settings.json.mcpServers.acts` → `node C:/Users/miche/tools/acts/packages/mcp/dist/bin/acts-mcp.js`; `ACTS_DB_PATH=G:/Projetos/GarraRUST/.acts/store.db` |
| Runtime ativo nesta sessão | **NÃO** — tools `mcp__acts__*` **não aparecem** no deferred tool list | Evidência: a lista deferida inclui Canva/ClickUp/Gmail/HubSpot/Linear/Supabase/Vercel/n8n/Zapier mas **zero** `mcp__acts__*` |
| Dados reais já gravados | Mínimos | `observations=1` (seed "Lote 0B bootstrap" de plan 0B); `checkpoints=0`, `turn_metrics=0`, `tool_results=0`, `compression_events=0`, `sessions=0`, `quality_scores=0` |
| Compressão filtros | Instalado | `packages/compress/` com filtros git/npm/pytest/docker |
| Sandbox | Instalado | `packages/sandbox/` com content store FTS5 |
| Observability 7-signal | Instalado | `packages/observe/` |
| Memória progressiva | Instalado | `packages/memory/` (3-camadas 15/60/200 tokens) |

**Gap concreto a fechar no Lote 1:**
1. Validar se o binário `C:/Users/miche/tools/acts/packages/mcp/dist/bin/acts-mcp.js`
   existe e inicia sem erros (teste rápido via `node <path> --help`).
2. Se faltar, rebuildar a partir do workspace `G:/Projetos/awesome-claude-token-stack/packages/mcp/`.
3. Garantir que as tools `mcp__acts__*` apareçam no próximo `/max-power`.
4. Instrumentar hook `stop.sh` para gravar `turn_metrics` + `checkpoint`
   via CLI `acts` ao final de cada turn — evidência concreta = linhas
   novas em `turn_metrics` ao fim de cada sessão.

### A.4 GitHub + GitHub Actions — ACESSO ATIVO, GATES PARCIAIS

| Item | Status | Evidência |
|---|---|---|
| CLI `gh` | Autenticado | `gh auth status` → "Logged in to github.com account michelbr84 (keyring)"; ativo |
| Repo | Acessível | `michelbr84/GarraRUST`; branch `main`; git tree clean; último commit `ec15d1e` |
| PRs abertas | 0 | `gh pr list --limit 5` → vazio |
| Workflows | 4 | `.github/workflows/{ci.yml (280),cargo-audit.yml (151),deploy.yml (109),release.yml (219)}` |
| Jobs CI hoje | fmt ✓, clippy ✓ (strict -D warnings), test ✓ (Linux full, Windows/macOS `--no-run`), build ✓, **e2e ✗**, **playwright ✗**, **security ✗** | 3 jobs marcados `continue-on-error: true` — mascarados como verdes. TODO in-line `fix/ci-triage-2026-04-15` reconhece. |
| Gates ausentes | 8 | dependency-review-action, CodeQL, semgrep, SBOM, coverage, mutation, gitleaks, cargo-deny-in-CI |
| Cargo-deny configurado mas não invocado | `deny.toml` existe (licenças MIT/Apache/BSD/ISC, 5 clarifications do `ring`), mas ci.yml **não chama** `cargo deny check` |

### A.5 Linear — ACESSO ATIVO

| Item | Status | Evidência |
|---|---|---|
| Team | Identificado | `GarraIA-RUST` (`cf3ca822-b504-4638-a89c-789e3c8a7592`), atualizado 2026-04-23T12:10Z |
| In Progress | 1 issue | **GAR-413** (migrate workspace CLI SQLite→Postgres), startedAt 2026-04-21 |
| Backlog | 50+ issues | GAR-410 (Urgent, CredentialVault final), GAR-331 (Urgent, EPIC Mobile Alpha), GAR-383 (High, TLS 1.3), GAR-400 (High, LGPD/GDPR export/delete), GAR-401 (High, testcontainers CI), GAR-402 (Medium, cargo-fuzz), GAR-380/381 (High, hot-reload config/provider) |
| Projects | 6 | Fase 1 Core; Fase 3 Group Workspace; Fase 4 UX; Fase 5 Qualidade/Segurança; Fase 6 Lançamento; epic mobile-build |

**Itens que este plano rastreará:**

* **GAR-401** (testcontainers CI) → Lote 3 deste plano
* **GAR-402** (cargo-fuzz smoke) → Lote 4 deste plano
* **GAR-429** / plan 0049 (clippy strict) → já fechado; este plano consolida
* **Novo issue a criar** — Epic "Quality Gates Phase 3.6" contendo:
  * GAR-NEW-Q1: cargo-deny no CI pipeline
  * GAR-NEW-Q2: dependency-review-action em PR
  * GAR-NEW-Q3: gitleaks em PR
  * GAR-NEW-Q4: CodeQL semanal
  * GAR-NEW-Q5: cargo-llvm-cov em CI + publish
  * GAR-NEW-Q6: cargo-mutants piloto em `garraia-auth`
  * GAR-NEW-Q7: RUSTSEC triage antes de 2026-05-20
  * GAR-NEW-Q8: e2e + playwright fix (Postgres service container)
  * GAR-NEW-Q9: refactor `admin/handlers.rs` em ≥ 3 módulos
  * GAR-NEW-Q10: refactor `bootstrap.rs` em pipelines de `AppState`

---

## B. Diagnóstico Técnico

### B.1 Estrutura do repositório

* **19 crates ativos** no workspace + 1 PoC efêmero (`benches/database-poc`).
* **~98.400 LOC em Rust** distribuídas com forte desbalanceamento —
  `garraia-gateway` isolado detém **30.191 LOC (31%)** do total.
* **Sem ciclos** no grafo de dependências (validado por leitura dos
  `Cargo.toml`).
* Hub de fan-in: `garraia-common` (325 LOC, 10 dependentes).
* Hub de fan-out: `garraia-cli` (11 deps diretas) e `garraia-gateway` (10).

**Top 10 arquivos por LOC (candidatos a refactor):**

| Arquivo | LOC | Gravidade |
|---|---|---|
| `crates/garraia-gateway/src/admin/handlers.rs` | 3.300 | **Crítico** |
| `crates/garraia-gateway/src/bootstrap.rs` | 2.405 | **Crítico** |
| `crates/garraia-cli/src/migrate_workspace.rs` | 1.590 | Alto |
| `crates/garraia-gateway/src/rest_v1/uploads.rs` | 1.572 | Alto |
| `crates/garraia-gateway/src/rest_v1/groups.rs` | 1.531 | Alto |
| `crates/garraia-db/src/session_store.rs` | 1.493 | Alto |
| `crates/garraia-config/src/check.rs` | 1.223 | Médio |
| `crates/garraia-gateway/src/server.rs` | 1.138 | Médio |
| `crates/garraia-gateway/src/rate_limiter.rs` | 1.040 | Médio |
| `crates/garraia-gateway/src/router.rs` | 996 | Médio |

### B.2 Dependências

* **tokio v1** com feature `full` replicada em 21 crates — oportunidade de
  feature-gate por crate (reduzir build time ~5-10%).
* **sqlx v0.8** declarado em 6 crates com features `uuid,chrono` duplicadas
  — deveria subir ao workspace root (`workspace.dependencies`).
* **serde_yaml** declarado workspace-wide, usado só em `garraia-config` —
  mover para crate-level.
* **wasmtime v28** isolado em `garraia-plugins` — **5 advisories RUSTSEC
  ativas**, mais críticos: `RUSTSEC-2025-0046` (fd_renumber panic),
  `RUSTSEC-2025-0118` (shared linear memory unsound). Ignore expira
  2026-05-20.
* **reqwest v0.12** em 9 crates com feature-sets heterogêneos — auditoria
  recomendada.
* Nenhuma dep duplicada no `Cargo.lock` em versões conflitantes.

### B.3 Testes

* **~520 unit tests** (contagem via grep `#[test]`/`#[tokio::test]`).
* **44 arquivos de integration tests** (~13.800 LOC).
* **2 scripts E2E** (`tests/e2e_telegram_api.sh` 272 LOC, `tests/e2e_mobile_api.sh` 206 LOC) — decorativos no CI (mascarados).
* **Playwright** (MCP UI) — 1 spec, decorativo no CI.
* **Doctests:** estimativa ~28 blocos de código em `///` — esparso.
* **Property/Fuzz:** **ZERO**. Sem `proptest!`, `quickcheck`, `fuzz/`.
* **Mutation:** **ZERO**. Sem `cargo-mutants` ou `mutagen`.
* **Coverage tooling:** **ZERO**. Nem tarpaulin, nem llvm-cov, nem grcov.
* **Flutter:** 3.962 LOC em `lib/`, 164 LOC em `test/` → **cobertura estimada 4%**.

**Mapa de gaps** (evidência por crate):

| Crate | src LOC | Integration LOC | Nível | Risco |
|---|---|---|---|---|
| `garraia-auth` | 3.504 | 3.130 | Alto (89%) | Bem coberto |
| `garraia-gateway` | 30.191 | 6.722 | Médio (22%) | Middleware/WS auth gaps |
| `garraia-workspace` | 312 | 2.099 | Alto (harness) | Testes em volume |
| `garraia-cli` | 5.045 | 1.225 | Médio (24%) | Migrate OK |
| `garraia-storage` | 2.216 | 278 | Médio (13%) | S3 integration em 1 arquivo |
| `garraia-agents` | 14.476 | 0 | **Baixo (0% integration)** | LLM routing untested end-to-end |
| `garraia-channels` | 8.668 | 0 | **Baixo** | Telegram/Discord/Slack sem E2E |
| `garraia-security` | 1.057 | 0 | **Baixo** | AES-256-GCM vault sem adversarial |
| `garraia-plugins` | 1.333 | 0 | **Baixo** | WASM sandbox — só unit |
| `garraia-voice` | 1.011 | 0 | **Nenhum** | Zero testes |

### B.4 Complexidade e tamanho

* `clippy.toml` define `too-many-arguments=10`, `cognitive-complexity=25`,
  `too-many-lines=150` — thresholds pragmáticos, sem *denial rules* extras.
* Sem AST tooling ativo (`rust-code-analysis`, `tokei --complexity`).
  **Métrica de complexidade ciclomática** hoje é estimativa — gap.
* 15 arquivos > 700 LOC — todos candidatos a split. Os 3 piores
  (`admin/handlers.rs`, `bootstrap.rs`, `migrate_workspace.rs`) somam
  **7.295 LOC** — 7,4% do workspace em 3 arquivos.

### B.5 Mutation testing

Ausente. Prioridades quando introduzir (ordem de valor/custo):

1. **`garraia-auth`** (3.504 LOC, 89% test ratio) — crypto + RBAC;
   mutation vai testar se os testes realmente cobrem *branches*.
2. **`garraia-config::check`** (1.223 LOC) — lógica de validação densa,
   alta densidade de `if/match` — alvo ideal.
3. **`garraia-storage`** — commit two-phase e HMAC integrity.
4. **`garraia-gateway::rest_v1::uploads`** — TUS state machine.

Escolha do tool: `cargo-mutants` (mais maduro, CI-friendly,
`--timeout-multiplier` útil em crates Docker-bound).

### B.6 GitHub Actions — gates presentes/ausentes

Presentes e **bloqueantes**:
* `fmt` (`cargo fmt --check --all`)
* `clippy --workspace --exclude garraia-desktop --all-targets -- -D warnings`
* `test` (Linux full; Windows/macOS `--no-run`)
* `build` (release)

Presentes mas **mascarados** (`continue-on-error: true`):
* `e2e` (gateway falha ao subir sem Postgres)
* `playwright` (mesmo root-cause)
* `security` (`cargo audit`, 6 advisories vivas)

**Ausentes:** `cargo-deny` no pipeline (existe `deny.toml` mas não é
invocado), `dependency-review-action`, CodeQL, semgrep, SBOM, gitleaks,
coverage (codecov/coveralls), mutation, MSRV check explícito,
sqlx-cli migration lint.

**Sem pinning de SHA** nas actions (usa tags `@v4`). Recomendação:
pinear por SHA de versões minor-stable em workflow crítico.

### B.7 Linear — integração com diagnóstico

Cada finding deste diagnóstico mapeia diretamente para issues existentes
ou novas:

| Finding | Linear |
|---|---|
| E2E mascarado | **NOVO** GAR-NEW-Q8 |
| Coverage zero | Relacionado a **GAR-401** (testcontainers CI) |
| Mutation zero | **NOVO** GAR-NEW-Q6 |
| cargo-deny não invocado | **NOVO** GAR-NEW-Q1 |
| RUSTSEC debt | **NOVO** GAR-NEW-Q7 (deadline 2026-05-20) |
| Fuzz zero | **GAR-402** já existe (backlog) |
| admin/handlers.rs 3.300 LOC | **NOVO** GAR-NEW-Q9 |
| bootstrap.rs 2.405 LOC | **NOVO** GAR-NEW-Q10 |
| voice 0 tests | Novo sub-issue de **GAR-401** |
| Secret scan ausente | **NOVO** GAR-NEW-Q3 (gitleaks) |

---

## C. Equipe de Agentes — papéis e uso estratégico

Sem redundância. Cada agente tem escopo ortogonal e entregável verificável.
Agentes marcados como "subagent fresh" significam que serão instanciados
via Explore ou general-purpose *por task*, descartáveis, para proteger o
contexto principal.

| Papel | Agente | Missão | Entregável | Quando aciona | Custo contexto |
|---|---|---|---|---|---|
| Orchestrator | **main (este)** | Orquestra lotes, aprova transições, mantém rastreabilidade Linear | Plano + PRs merged | Cada lote | — |
| Repo Analyst | Explore (subagent fresh por lote) | Mapear mudanças, dependências, LOC deltas | Relatório ≤ 400 palavras | Antes e depois de cada refactor | baixo |
| Test & Coverage Engineer | Explore + `tdd-loop` skill | Cobertura por crate, identificar branches fracas | Relatório + teste novo | Lote 3, 5, 7 | médio |
| Dependency & Architecture Auditor | Explore + `@code-reviewer` | Audit de `Cargo.toml`, features, duplicação, ciclos | Diff + comentário PR | Lote 2, 6 | baixo |
| Complexity / Refactor Analyst | Explore + `refactor-module` skill | AST-lite grep, identificar split points | Plano de split arquivo-a-arquivo | Lote 6 (hotspots) | médio |
| CI/CD & GH Actions Auditor | general-purpose + `@security-auditor` | Revisar `.github/workflows/*` a cada PR de CI | Patch workflow + justificativa | Lote 1, 2, 4 | baixo |
| Linear Workflow Coordinator | main + `mcp__linear-server__*` tools | Criar/fechar issues, atualizar status, linkar PRs | Issues/comentários | Cada lote | baixo |
| Security / Reliability Reviewer | **`@security-auditor`** | Revisar crypto/auth/RLS/TUS/JWT surface | Aprovação/bloqueio de PR | Lote 4 (RUSTSEC), Lote 5 (mutation), Lote 6 (refactor) | médio |
| Documentation / Handoff Writer | **`@doc-writer`** + `generate-docs` | Atualizar CLAUDE.md, docs/adr/, plans/README.md | Docs + ADR novo | Final de cada lote | baixo |
| Code Reviewer / QA Gate | **`@code-reviewer`** | Revisão de Rust/Flutter antes de merge | Comentário de PR | Cada PR | médio |
| Team Coordinator | **`@team-coordinator`** | Delegar tarefas paralelas com worktree isolation | Status de paralelismo | Lotes 3, 5, 6 | baixo |

**Regra anti-teatro:** nenhum agente é acionado sem missão escrita.
Todos os disparos passam por `verification-before-completion` antes do
merge — evidência concreta (cargo test verde, cargo clippy verde, cargo
deny verde) é obrigatória antes de fechar o task no Linear.

---

## D. Métricas e Baseline

Valores **reais** medidos agora (2026-04-23) vs. alvos pós-execução.
Quando não há medida direta, está rotulado *estimativa*.

| Métrica | Baseline hoje | Alvo pós-plano | Medida por |
|---|---|---|---|
| Total LOC Rust (src) | ~98.400 | -3% (refactor desliza para testes) | `tokei crates/ --exclude tests` |
| Cobertura de linha por crate | Não medida | `auth, config, workspace ≥ 85%`; `gateway ≥ 70%`; `agents, channels ≥ 50%` | `cargo-llvm-cov --workspace --lcov` + codecov |
| Testes unit | 520 | +60 (crates `voice`, `security` integration) | contador de `#[test]` |
| Testes integration | 44 arquivos | +10 arquivos | `ls crates/*/tests/*.rs` |
| Doctests | ~28 | +25 nas APIs públicas | `cargo test --doc` |
| Mutation score (auth piloto) | N/A | ≥ 65% killed | `cargo mutants --package garraia-auth` |
| Complexidade ciclomática (top 10 funções) | Não medida | relatório publicado + threshold 30 | `rust-code-analysis-cli` |
| Arquivos > 700 LOC | 15 | ≤ 10 | find + wc -l |
| Arquivos > 1.500 LOC | 8 | ≤ 3 | find + wc -l |
| RUSTSEC advisories ignoradas | 13 (deadline 2026-05-20) | ≤ 3 (todas com PR aberto com fix) | `cargo audit --json` |
| Jobs CI bloqueantes | 4/7 (57%) | 7/7 (100%) | `.github/workflows/ci.yml` |
| Quality gates ausentes | 8 | 1 (CodeQL opcional) | checklist seção B.6 |
| Tempo médio CI por PR | ~6 min | ≤ 10 min | GH API actions runs |
| Dívida em `continue-on-error: true` | 3 jobs | 0 | grep no `.github/workflows/` |
| `.acts/store.db` turn_metrics | 0 | > 0 em cada sessão | SQL `SELECT COUNT(*) FROM turn_metrics` |
| Linear issues novas criadas | 0 | 10 (sub-issues do epic Quality Gates) | `list_issues` filtrando epic |
| Linear issues fechadas | 0 | ≥ 6 do novo epic | status-filter `done` |
| Risco técnico agregado | Alto (RUSTSEC + E2E oculto + 0 mutation) | Médio-baixo | qualitativa review final |

**Estimativas de custo:** lote 1-2 (quality gates CI) ≈ 4-6h de wall-clock
com revisão; lote 3 (cobertura) ≈ 6-10h; lote 4 (RUSTSEC) ≈ 4-8h (depende
de wasmtime upgrade); lote 5 (mutation) ≈ 4h piloto; lote 6 (refactor
hotspots) ≈ 16-24h pelos 3 arquivos maiores.

---

## E. Plano Proposto — fases, tasks, ordem

Cada lote tem entrada (pré-condição), saída (critério verificável) e
estimativa. Execução em branches separadas via `using-worktrees`.
Um PR por lote. Rastreio Linear por issue.

### Lote 0 — Wiring token-stack + bootstrap Linear (60-90 min)
*Dependência: nenhuma. Sem impacto em código de produção.*

**Ajuste pós-aprovação (ajuste #1 do usuário):** este lote é
**all-or-nothing** quanto ao wiring do token-stack. Se o binário
`acts-mcp.js` não puder ser validado e as tools `mcp__acts__*` não
puderem aparecer no próximo `/max-power`, o lote é interrompido e o
problema vira uma **task de investigação isolada** (provável sub-plan
`0050.0-investigar-acts-mcp.md`). Não seguimos parcialmente com
"hook grava stop.sh sem MCP ativo" — isso já existe há tempo e não
produziu observabilidade, exatamente o que este plano quer consertar.

1. **Validar binário** — rodar `node C:/Users/miche/tools/acts/packages/mcp/dist/bin/acts-mcp.js --help`.
   * Se existir e aceitar `--help`: seguir para passo 2.
   * Se falhar: tentar rebuild em `G:/Projetos/awesome-claude-token-stack/packages/mcp/` (`bun install && bun run build`) e copiar o resultado para `C:/Users/miche/tools/acts/packages/mcp/dist/bin/`.
   * Se ainda falhar: **abortar o Lote 0**, criar task "Investigar wiring acts-mcp" e reportar ao usuário. Os passos 2–4 abaixo ficam pendentes até o MCP estar funcional.
2. Ajustar `.claude/hooks/stop.sh` para gravar `turn_metrics` via
   `acts` CLI em cada fim de turno (uma linha de evidência por sessão).
   **Só executar esta etapa depois do passo 1 ter passado.**
3. Criar o Epic Linear `Quality Gates Phase 3.6` + 10 sub-issues
   (GAR-NEW-Q1..Q10) usando `mcp__linear-server__save_issue`, todas com
   label `epic:quality-gates`. **Esta etapa NÃO depende do MCP acts
   funcionar** — pode ser feita mesmo que o passo 1 tenha falhado,
   porque o Linear é independente e os issues são necessários de
   qualquer forma para rastrear o restante do plano.
4. Atualizar `plans/README.md` listando este plan file.

**Saída (happy path):** `acts__*` tools visíveis no próximo `/max-power`;
10 issues novas no Linear; `.acts/store.db.turn_metrics` com ≥ 1 linha
ao fim do lote.

**Saída (falha do passo 1):** passos 3 e 4 completos; passo 2 pendente;
sub-plan de investigação aberto e pausa para decisão do usuário
(continuar sem token-stack até Lote N, ou investigar primeiro).

### Lote 1 — CI Quality Gates de baixo custo (60-90 min)
*PR único; bloqueante em `main`.*

Adicionar ao `.github/workflows/ci.yml` como jobs paralelos:

1. **cargo-deny job** — `cargo deny check` usando o `deny.toml` existente
   (GAR-NEW-Q1).
2. **dependency-review job** — `actions/dependency-review-action@v4`
   (GAR-NEW-Q2). Só roda em `pull_request`.
3. **gitleaks job** — `gitleaks/gitleaks-action@v2` com config mínima
   (GAR-NEW-Q3).
4. **MSRV check** — `cargo +1.85 check --workspace` (rust-version já é 1.85
   no `Cargo.toml`).

**Saída:** 4 jobs novos verdes em um PR limpo. `/pre-commit` + `/review-pr`.

### Lote 2 — Reativar e2e + playwright (90-120 min)
*Depende: Lote 1 mergeado.*

Usar `systematic-debugging` para root-cause confirmado (gateway exit
imediato sem Postgres):

1. Adicionar `services: postgres:16` com `POSTGRES_PASSWORD` em job `e2e`.
2. Mesma coisa em `playwright`.
3. Gatear startup do gateway em perfil `ci-minimal` que tolera ausência de
   configs opcionais.
4. Remover os 6 `continue-on-error: true` dos dois jobs (GAR-NEW-Q8).
5. Aceitar job `security` continuando não-bloqueante **até** Lote 4
   fechar os RUSTSEC.

**Saída:** CI verde com e2e + playwright realmente exercitando o gateway.
Evidência = logs do gateway respondendo `200 OK` no `/health` antes do
teste rodar.

### Lote 3 — Instrumentação de cobertura (3-5h)
*Depende: Lote 1.*

**Ajuste pós-aprovação (ajuste #2 do usuário):** zero dependência em
serviços externos no arranque. Sem codecov, sem coveralls, sem actions
de terceiros que exigem token/webhook. Apenas:

1. Adicionar `cargo-llvm-cov` ao workflow em job `coverage` (paralelo a
   `test`, só Linux).
2. Gerar dois formatos de relatório:
   * **`lcov.info`** — upload como artifact da action (`actions/upload-artifact@v4`,
     retenção 14 dias). Baixável pelo autor do PR via UI GitHub.
   * **`summary.txt`** (saída `cargo llvm-cov --summary-only`) —
     postada como comentário no PR usando o próprio `gh pr comment`
     via `GITHUB_TOKEN` default (não requer secret externo).
3. Thresholds **não-bloqueantes no primeiro PR** (apenas relatar). Após
   baseline estabelecido (~3 PRs), transformar em soft-gate com
   threshold por crate, ainda postado via `gh pr comment` sem falhar
   o PR.
4. Rodar manualmente no piloto → baseline por crate publicado em
   `docs/coverage-baseline-2026-04.md` (committed, não externo).
5. Criar issue de fechamento de gap para cada crate `< 50%`.

**Saída:** Relatório LCOV + summary em cada PR, **zero serviços
externos**. Baseline committed no repo, versionado.

**Se no futuro quisermos Codecov/Coveralls:** decisão separada,
fora deste plano, com ADR próprio.

### Lote 4 — RUSTSEC triage (4-8h, crítico antes de 2026-05-20)
*Depende: Lote 2 (CI estável).*

**Ajuste pós-aprovação (ajuste #3 do usuário):** o cluster **wasmtime
28 → 29** é tratado como **subplano separado desde já**
(`plans/0050.4-wasmtime-upgrade.md`), não como sub-bullet deste lote.
Razão: o bump mexe em sandbox WASM, pode quebrar API pública da crate
`garraia-plugins`, pode exigir refactor de chamadores em
`garraia-gateway`/`garraia-agents` e tem risco de afetar performance.
É escopo próprio.

Este lote passa a ter duas faixas:

**Lote 4a — RUSTSEC "fácil" (4-6h, 4 PRs pequenos):**

Sequencialmente, um PR por cluster:
1. **Cluster rustls-webpki** (4 advisories) — bump `rustls-webpki >= 0.103.10`,
   validar com `cargo tree -i rustls-webpki`.
2. **Cluster idna** (punycode bypass) — bump transitivo via `hyper`/`url`.
3. **Cluster rsa + core2 + tokio-tar** — cada um em PR pequeno, cuidando
   de quem puxa a dep.
4. Remover entradas expiradas do `.cargo/audit.toml`.
5. Tornar job `security` **parcialmente** bloqueante: passa a falhar
   em qualquer advisory nova, mas mantém ignore temporário explícito
   para wasmtime até o Lote 4b fechar.

**Lote 4b — wasmtime upgrade (subplano próprio, ref. `plans/0050.4-wasmtime-upgrade.md`):**

Será escrito como sub-plan apartado, contendo:
* inventário dos pontos de `garraia-plugins` que tocam wasmtime 28 API;
* matriz de breaking changes 28→29;
* estratégia de upgrade (one-shot vs. duas etapas 28 → 28.X → 29);
* plano de teste (testes existentes + novos) — o risco de
  `RUSTSEC-2025-0118` (unsound shared linear memory) exige cobertura
  adversarial mínima;
* rollback plan;
* aprovação explícita por `@security-auditor`.

**Saída do Lote 4a:** `cargo audit` verde exceto para o ignore
explícito de wasmtime com link ao sub-plan 0050.4. Entradas expiradas
limpas. Jobo `security` passa a falhar em qualquer NOVA advisory.

**Saída do Lote 4b:** wasmtime em 29+, `.cargo/audit.toml` com ≤ 2
ignores (ou zero), `security` 100% bloqueante.

### Lote 5 — Mutation testing piloto (4h)
*Depende: Lote 3 (cobertura visível).*

1. Adicionar `cargo-mutants` como dev-dependency / CI-optional job
   agendado semanalmente (não por PR — custo alto).
2. Configurar `mutants.out/` excluded do git.
3. Rodar piloto em `garraia-auth` (crate mais bem coberto).
4. Documentar baseline de score e falhas (testes que não matam mutantes).
5. Criar sub-issues para cada mutant escapado em `auth`.

**Saída:** Relatório `cargo mutants -p garraia-auth` com ≥ 65% killed;
evidência anexada ao comentário de GAR-NEW-Q6.

### Lote 6 — Refactor dos hotspots (em PRs pequenos, revisáveis)
*Depende: Lote 3 (cobertura) + Lote 4a (CI verde).*

**Ajuste pós-aprovação (ajuste #4 do usuário):** PRs grandes de
refactor são difíceis de revisar com segurança. Cada sub-lote abaixo
é **ele próprio decomposto em múltiplos PRs** de ≤ ~500 LOC movidos
por PR, com git `M` (rename) preservando blame quando possível.
Regra: **um PR = um módulo extraído**, não "vários módulos de uma vez".

**Sub-lote 6.1 — `admin/handlers.rs` (3.300 LOC → 6 módulos em 6 PRs):**

Cada PR move exatamente UM conjunto de handlers para seu próprio
arquivo, mantém todas as assinaturas públicas, re-exporta do
`admin/mod.rs`, e roda toda a suite de testes:
* PR 6.1.a — extrair `admin/shared.rs` (tipos comuns, helpers,
  macros) — PR menor, pré-requisito dos outros.
* PR 6.1.b — `admin/projects.rs`
* PR 6.1.c — `admin/credentials.rs`
* PR 6.1.d — `admin/channels.rs`
* PR 6.1.e — `admin/mcp_registry.rs`
* PR 6.1.f — `admin/agents.rs`
* PR 6.1.g (opcional) — o que sobrar + reorganização de `mod.rs`.

Ordem: a → b, c, d, e, f em paralelo possíveis via worktrees; g fecha.
Cada PR: TDD REFACTOR puro, zero mudança de comportamento. Review
obrigatório de `@code-reviewer` por PR. Métricas antes/depois em cada PR.

**Sub-lote 6.2 — `bootstrap.rs` (2.405 LOC → pipelines em 5+ PRs):**

Extração por fase de inicialização, uma por vez:
* PR 6.2.a — `bootstrap/config.rs` (loading + validation + watch).
* PR 6.2.b — `bootstrap/storage.rs` (ObjectStore wiring).
* PR 6.2.c — `bootstrap/agents.rs` (AgentRuntime + provider pool).
* PR 6.2.d — `bootstrap/channels.rs` (channel adapters wiring).
* PR 6.2.e — `bootstrap/telemetry.rs` (OTel + Prometheus).
* PR 6.2.f — slimming final de `bootstrap.rs` como orquestrador puro,
  idealmente < 500 LOC.

Cada PR: preservar `AppState` builder; cada pipeline testável em
isolamento. Review obrigatório de `@code-reviewer` +
(para config/storage/telemetry) `@security-auditor`.

**Sub-lote 6.3 — `migrate_workspace.rs` (1.590 LOC → stages em 6+ PRs):**

Cada stage já é lógica-próxima-transacional (planos 0039/0040/0045).
Extrair um stage por PR mantendo atomicidade. Ordem natural:
* PR 6.3.a — `migrate_workspace/common.rs` (tx helpers, audit writer,
  progress logger).
* PR 6.3.b — `migrate_workspace/users.rs` (stage 1, plan 0039).
* PR 6.3.c — `migrate_workspace/groups.rs` (stage 3, plan 0040).
* PR 6.3.d — `migrate_workspace/chats.rs` (stage 5, plan 0045).
* PR 6.3.e — `migrate_workspace/api_keys.rs` + stages futuros.
* PR 6.3.f — `lib.rs`/`mod.rs` ficam como driver puro < 300 LOC.

Regra do sub-lote: **cada PR preserva atomicidade transacional**
(os stages continuam dentro da mesma transação Postgres — a extração
só move código, não muda fronteira de `BEGIN`/`COMMIT`). Teste
integration `cli_migrate_workspace.rs` (1.225 LOC existentes) deve
continuar verde.

**Critérios do Lote 6 inteiro:**
* Nenhum PR > 500 LOC movidos.
* Nenhum PR introduz arquivo novo > 900 LOC (alvo: ≤ 700).
* Cobertura ±2% em cada PR; regressão funcional = zero.
* Review humano + `@code-reviewer` em cada PR; security review em PRs
  que tocam auth/crypto/storage/telemetry.
* Status Linear atualizado a cada PR.

**Saída do Lote 6:** 15-18 PRs mergeados, hotspots pulverizados.
Top 3 arquivos maiores passam de 3.300/2.405/1.590 para
≤ 700 cada. `admin/handlers.rs`, `bootstrap.rs` e `migrate_workspace.rs`
viram módulos-orquestradores enxutos.

### Lote 7 — Cobertura crítica em voice/agents/channels/security (8-12h)
*Depende: Lote 3. Paralelizável com Lote 6.*

1. **`garraia-voice`** — adicionar 1 integration test por provider
   (Chatterbox, ElevenLabs, Kokoro) com mock HTTP via `wiremock`.
2. **`garraia-security`** — adversarial test suite para
   `CredentialVault::{encrypt, decrypt}`; fuzzing rápido via
   `proptest` (proptest não é fuzzing real, mas é útil).
3. **`garraia-channels`** — contract tests por adapter (parsing de webhook
   Telegram/Discord/Slack); fixtures em `tests/fixtures/`.
4. **`garraia-agents`** — E2E orchestration com provider mock.
5. **`garraia-plugins`** — 1 teste que carrega um WASM minimal e verifica
   sandbox.

**Saída:** Cobertura média do grupo low-coverage sobe de ~5% para ≥ 35%.

### Lote 8 — Documentação + ADR final + handoff (1-2h)
*Sempre após cada lote; lote 8 consolida.*

1. **Novo ADR** `docs/adr/0009-quality-gates-phase-3-6.md` — registrar
   decisões (cargo-deny no CI, mutation via cargo-mutants, cobertura via
   llvm-cov sem codecov).
2. Atualizar `CLAUDE.md` com nova seção "Quality Gates".
3. Atualizar `plans/README.md` com este plan.
4. Fechar issues Linear com comentário resumo + link de PR.

---

## F. Quality Gates — o que bloqueia avanço

Gates obrigatórios **em cada PR** do plano:

1. `cargo fmt --check --all` (já).
2. `cargo clippy --workspace --exclude garraia-desktop --all-targets -- -D warnings` (já).
3. `cargo test --workspace --exclude garraia-desktop` Linux (já, com Docker).
4. **`cargo deny check`** (a adicionar no Lote 1).
5. **`actions/dependency-review-action`** em PR (Lote 1).
6. **`gitleaks`** em PR (Lote 1).
7. **`cargo audit`** bloqueante (Lote 4).
8. **`cargo-llvm-cov` report** publicado (Lote 3; não bloqueia inicialmente).
9. **E2E + Playwright reais** (Lote 2).
10. **Review humano ou `@code-reviewer`+`@security-auditor`** em PRs
    de crypto/auth/RLS/plugin (obrigatório).
11. **`verification-before-completion` skill** executada antes de marcar
    Linear como done — cada afirmação de "verde" deve ter comando+output
    colado no comentário.

Gates adicionais por lote:
* Lote 4: `cargo tree -i <crate>` provando upgrade correto do transitivo.
* Lote 5: `cargo mutants` rodado; score publicado.
* Lote 6: zero delta em testes falhando; cobertura estável ±2%;
  benchmarks `cargo bench` (se existirem) estáveis.
* Lote 7: novos testes efetivamente rodam em CI Linux (não só
  `--no-run`).

---

## G. Fluxo Git / Linear / GitHub Actions — rastreabilidade da execução

**Convenção de branch:** `feat/quality-gates-LOTE<N>-<slug>` criada via
`using-worktrees` a partir de `main`.

**Convenção de commit:** Conventional Commits, referência obrigatória
ao Linear no footer:

```
feat(ci): adiciona cargo-deny ao pipeline principal

Closes GAR-NEW-Q1.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
```

**Convenção de PR:**
* Título ≤ 70 chars, referência Linear.
* Seção **Summary** com bullets do que mudou.
* Seção **Test plan** com checklist de verificação executada.
* Seção **Métricas** antes/depois (LOC, cobertura, CI runtime).
* Labels: `epic:quality-gates`, `lote-NN`.

**Linear:**
* Status transitions: Backlog → In Progress (ao criar branch) →
  In Review (ao abrir PR) → Done (ao merge).
* Comentário por PR com: link, commit hash, output dos gates.
* `startedAt` e `completedAt` atualizados automaticamente pela
  integração nativa Linear-GitHub.

**GitHub Actions:**
* Workflow `ci.yml` recebe jobs novos incrementalmente — um PR por
  adição.
* Branch protection (se aplicado pelo usuário) passa a exigir todos
  os jobs verdes em `main`.

**Token-stack `.acts/store.db`:**
* Cada sessão deste plano grava `observation` com tag `plan:foamy-origami`
  + `lote:<N>`.
* `turn_metrics` populado por `stop.sh` hook (Lote 0).
* Ao final, relatório de consumo (tokens in/out) via `acts observe stats`.

---

## H. Bloqueios / Limitações

Declarados aberta e honestamente para não iludir o usuário:

1. **MCP `acts` não está visível nesta sessão** — wiring existe em
   `~/.claude/settings.json` mas o runtime desta janela não expôs as
   tools `mcp__acts__*`. Lote 0 fecha isso; nenhum dado quantitativo
   de token-stack pode ser mostrado *antes* do Lote 0.
2. **Complexidade ciclomática real não foi medida** (só estimativa de
   tamanho LOC). Requer instalar `rust-code-analysis-cli` — fará parte
   do Lote 3 como métrica publicada junto da cobertura.
3. **Branch protection do GitHub não foi inspecionada** — requer
   `gh api repos/michelbr84/GarraRUST/branches/main/protection` (pode
   faltar scope). Se ausente, Lote 1 inclui instrução opcional para
   o usuário ativar manualmente.
4. **CodeQL e semgrep não estão no plano inicial** — custo alto de
   configuração vs. valor marginal enquanto os outros gates básicos
   ainda não foram ligados. Considerar em Fase 3.7 futura.
5. **Flutter coverage (garraia-mobile) não faz parte deste plano** —
   o backlog já tem GAR-331 (Mobile Alpha). Coordenar para não
   duplicar trabalho.
6. **RUSTSEC em wasmtime pode exigir refactor de `garraia-plugins`**
   — se a API quebrar entre wasmtime 28 → 29, Lote 4 pode precisar
   de sub-plan dedicado.
7. **cargo-mutants pode ter runtime proibitivo** em crates grandes —
   por isso piloto apenas em `garraia-auth`; expansão é decisão pós-Lote 5.
8. **Execução em Windows local** — qualquer teste Docker-bound só roda
   no CI Linux (reconhecido na própria `ci.yml`). Usar WSL2 localmente
   se necessário.
9. **Skill `subagent-driven-development` requer plan file estruturado**
   por task — este plan está no formato tradicional; ao executar, cada
   lote pode precisar de um plan file filho (`plans/0050.X-lote-N.md`).

---

## I. Aprovação Necessária

**Este é o ponto de parada obrigatório.** Aguardo resposta do usuário a
*uma* das opções abaixo antes de qualquer execução:

* `Aprovo — executar em ordem (Lote 0 → 8).`
* `Aprovo com ajustes — <ajustes>.`
* `Rejeito — refazer a seção X.`
* `Fazer apenas o(s) Lote(s) N, M...`

**Recomendação objetiva do próximo passo:** começar pelo **Lote 0**
(wiring token-stack + Epic Linear) porque:
* Custo ≤ 90 min.
* Destrava observabilidade real das sessões subsequentes.
* Nenhum risco em código de produção.
* Evidência imediata no `.acts/store.db.turn_metrics`.

Em seguida, **Lote 1** (gates CI de baixo custo) — 4 jobs novos em um
PR pequeno, alto ROI, desbloqueia todos os lotes seguintes.

---

## Critical files a tocar (mapeamento do plano → paths)

| Lote | Arquivos |
|---|---|
| 0 | `.claude/hooks/stop.sh`; novo `plans/0050-foamy-origami.md` (opcional, pode reusar este arquivo); Linear via MCP |
| 1 | `.github/workflows/ci.yml`; `deny.toml` (já existe) |
| 2 | `.github/workflows/ci.yml` (jobs e2e + playwright); possivelmente `crates/garraia-gateway/src/bootstrap.rs` (perfil ci-minimal) |
| 3 | `.github/workflows/ci.yml` (job coverage); novo `scripts/coverage.sh` |
| 4a | `Cargo.toml`, `Cargo.lock`, `.cargo/audit.toml` (rustls-webpki, idna, rsa, core2, tokio-tar) |
| 4b | **sub-plan separado** `plans/0050.4-wasmtime-upgrade.md`; `crates/garraia-plugins/` como principal afetado |
| 5 | `.github/workflows/mutants.yml` (novo workflow semanal); `Cargo.toml` dev-dep |
| 6.1 | `crates/garraia-gateway/src/admin/handlers.rs` → `admin/{shared,projects,credentials,channels,mcp_registry,agents}.rs` — 6 PRs ≤ 500 LOC cada |
| 6.2 | `crates/garraia-gateway/src/bootstrap.rs` → `bootstrap/{config,storage,agents,channels,telemetry}.rs` — 6 PRs ≤ 500 LOC cada |
| 6.3 | `crates/garraia-cli/src/migrate_workspace.rs` → `migrate_workspace/{common,users,groups,chats,api_keys,...}.rs` — 6+ PRs preservando atomicidade tx |
| 7 | `crates/{garraia-voice,garraia-security,garraia-channels,garraia-agents,garraia-plugins}/tests/*.rs` novos |
| 8 | `docs/adr/0009-quality-gates-phase-3-6.md`; `CLAUDE.md`; `plans/README.md` |

## Utilitários/funções existentes a reutilizar

* **Hooks:** `.claude/hooks/{session-start,stop}.sh` (já funcionais;
  extensão mínima).
* **Agents:** `@code-reviewer`, `@security-auditor`, `@doc-writer`,
  `@team-coordinator` (já carregados).
* **Skills:** listadas em A.2.
* **Workspace harness de testes:** `garraia-workspace/tests/` (2.099 LOC
  já compartilham fixtures Postgres via testcontainers — padrão a
  replicar em `voice` e `channels`).
* **`.cargo/audit.toml`:** já tem 13 ignores bem catalogados — editável.
* **`deny.toml`:** já define license allow-list + 5 clarifications — só
  precisa ser invocado.
* **`scripts/setup.sh`, `scripts/verify.sh`:** preflight reutilizável.

## Verification Section — como validar o plano end-to-end

Após cada lote:

1. `cargo fmt --check --all && cargo clippy --workspace --exclude garraia-desktop --all-targets -- -D warnings`.
2. `cargo test --workspace --exclude garraia-desktop` (Linux; em Windows
   usa `--no-run` + WSL2 opcional).
3. `cargo deny check` (pós-Lote 1).
4. `cargo audit` (bloqueante pós-Lote 4).
5. `cargo llvm-cov --workspace --lcov --output-path lcov.info` (pós-Lote 3).
6. `cargo mutants -p garraia-auth` (Lote 5; publicar score).
7. `gh pr checks <PR>` verde em todos os gates.
8. `mcp__linear-server__get_issue GAR-NEW-QN` com status Done e
   comentário com evidência.
9. Query `SELECT COUNT(*) FROM turn_metrics` no `.acts/store.db` deve
   crescer monotonicamente a cada sessão (evidência do token-stack real).

**Gate final de todo o plano:** dashboard simples em PR final mostrando
antes/depois das métricas da seção D. Se qualquer métrica regredir
sem justificativa, revert e reexecutar o lote.
