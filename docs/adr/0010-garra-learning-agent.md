# ADR 0010 — Garra Learning Agent / Self-Improving Operations Manual

**Status:** Accepted
**Date:** 2026-05-17 (local America/New_York)
**Owner:** @michelbr84
**Linear epic:** [`GAR-641`](https://linear.app/chatgpt25/issue/GAR-641) (criado 2026-05-17 com 10 sub-issues GAR-642..GAR-651)
**Related ADRs:** [0002 Vector Store](0002-vector-store.md) (Skill Retriever), [0009 Web Console Design System](0009-web-console-design-system.md) (Skills/Logs UI)
**Related plans:** [`plans/0138-gar-learning-agent-epic.md`](../../plans/0138-gar-learning-agent-epic.md)
**Related crates:** existing `garraia-skills` (parser/scanner/installer), new `garraia-learning`

---

## Context

O Garra hoje executa comandos, escreve código e roda agentes — mas **não aprende**
operacionalmente. Cada sessão começa do zero. Se descobrimos que "para limpar
branches do GitHub a sequência X funciona melhor que Y", essa descoberta vira no
máximo um item de memória local (`MEMORY.md`) ou um plano (`plans/NNNN-*.md`),
não vira um manual operacional reutilizável que o próprio Garra consulta antes
de agir.

O **Hermes Agent** (referência conceitual de produto, não fonte de código)
demonstrou que um agente pode:

1. Observar suas próprias execuções.
2. Detectar padrões repetíveis.
3. Transformar padrões em skills (procedimentos versionados).
4. Recuperar skills relevantes antes de atacar tarefas novas.
5. Atualizar skills quando descobre formas melhores.
6. Versionar tudo com rollback.

Queremos essa capacidade no Garra **com arquitetura própria**, focada em:

- **Segurança** — comandos perigosos nunca aprendidos sem revisão humana.
- **Auditabilidade** — toda mudança de skill tem diff, motivo, evidência e rollback.
- **CI-first** — nenhuma skill é promovida sem passar nos checks (mesma
  filosofia da AI Quality Ratchet existente).
- **Controle humano** — usuário pode aprovar, rejeitar, travar ou editar.
- **Separação rígida** entre memória, skill, log e manual distribuível.

## Decision

Criar um novo crate `garraia-learning` (Fase 1.4 do ROADMAP, épico
[`GAR-641`](https://linear.app/chatgpt25/issue/GAR-641)) com 10 sub-componentes, sentando sobre o crate
`garraia-skills` existente para reuso do `SkillScanner` /
`SkillInstaller` / formato `SkillFrontmatter`.

### Topologia de crates

```text
garraia-learning/        (novo, este ADR)
├── miner.rs             — analisa session logs, detecta padrões
├── generator.rs         — LLM-assisted skill drafting
├── registry.rs          — wrapper sobre garraia-skills, dual-scope (~/.garra/ + .garra/)
├── retriever.rs         — embedding match (consome garraia-embeddings da Fase 2.1)
├── evaluator.rs         — métricas objetivas (exit/tests/CI/diff/logs)
├── updater.rs           — diff + PR + branch flow (consome gh CLI)
├── versioning.rs        — git-backed history wrapper
├── safety.rs            — denylist + score threshold + path-protected files
├── override.rs          — CLI/UI API para approve/reject/lock/delete
└── lib.rs               — fachada + types compartilhados

garraia-skills/          (existente, 371 LOC)
├── parser.rs            — REUSADO pela registry
├── scanner.rs           — REUSADO pela registry
└── installer.rs         — REUSADO pela updater para promover skill
```

### Fronteira semântica (rígida)

| Tipo | Crate | Persistência | Mutabilidade |
|---|---|---|---|
| **Memória** (facts sobre usuário/grupo) | `garraia-workspace::memory_items` | Postgres, RLS-scoped | append + edit + delete |
| **Skill** (procedimento operacional) | `garraia-learning::registry` (via `garraia-skills`) | Markdown+YAML em disco, git-tracked | versioned, rollback-able |
| **Log de execução** (o que aconteceu) | `garraia-telemetry::traces` | Spans OTLP + Prometheus | append-only |
| **Manual distribuível** (skill pública instalável) | `garraia-skills::installer` | Tarball assinado | imutável após assinatura |

Skills aprendidas SÃO um manual operacional — quando promovidas via
`SkillInstaller`, podem ser distribuídas. Mas elas **nascem** mutáveis no
`garraia-learning::registry` antes de virar manual.

### Formato de skill (extende `SkillFrontmatter` existente)

```yaml
---
name: cleanup-merged-branches
version: 0.3.2
source: mined                # mined | authored | imported
scope: project               # project | global
score: 0.82                  # 0.0..1.0, exponential moving average
promoted_at: "2026-05-17T03:26:34Z"
last_used_at: "2026-05-17T04:03:51Z"
last_diff_sha: "efb295c"
locked: false                # human-override: nunca auto-update
critical_paths_touched:      # detectado pela Safety Gate
  - none
embeddings_model: "mxbai-embed-large-v1"
---

# Skill: cleanup-merged-branches

## When to use
Quando o usuário pede pra deletar branches já mergeadas no GitHub.

## Steps
1. `git fetch --all --prune`
2. Para cada branch sem-merge: `git push origin --delete <branch>`
3. Para PRs verdes: `gh pr merge <N> --squash --delete-branch`

## Evidence (auto-appended por evaluator)
- Run 2026-05-17T03:26:34Z: exit=0, tests=N/A (doc-only), 4 branches deletadas, score: 0.82
- Run 2026-05-15T...: exit=0, ...
```

### Loops principais

**Loop 1: Mine → Generate → Validate → Promote (skill nova)**

```
session log → Miner detecta padrão repetido (≥3 ocorrências em contextos similares)
            → emite candidate em ~/.garra/skills/_candidates/<name>-<sha>.md
            → Generator chama LLM com prompt provider-agnóstico (default openrouter/free)
              para escrever frontmatter + steps em Markdown
            → Safety Gate valida (denylist + paths críticos)
            → Evaluator roda em sandbox dry-run (se aplicável)
            → se passar todos: human approval via CLI (garra skills approve <name>)
              ou Web UI; só então: Registry.promote() → SkillInstaller
```

**Loop 2: Use → Evaluate → Update (skill existente)**

```
agente vai agir → Retriever consulta registry (embedding match + scope filter)
                → top-1 skill injetada no prompt do AgentRuntime
                → ação executada
                → Evaluator coleta sinais (exit/tests/CI/diff/logs)
                → atualiza score (EMA, decay configurável)
                → se nova execução foi MELHOR que skill atual:
                  Updater.propose_update() → git branch + diff + PR
                → se nova execução falhou + skill executada:
                  Updater.propose_fix() → mesmo flow, branch nomeado learning/skill-X-fix-N
```

**Loop 3: Promote → Distribute (skill amadurecida)**

```
skill com score ≥ threshold + N execuções sem regressão por T tempo
  → elegível para promoção a manual distribuível
  → SkillInstaller.pack(skill) → tarball assinado
  → upload para ~/.garra/skills/_distributed/ ou marketplace (Fase 7)
```

### Safety Gate (hard wall, sem bypass)

A função `garraia_learning::safety::gate(skill: &Skill) -> Result<(), SafetyDenial>`
é chamada antes de QUALQUER promoção (auto ou manual). Bloqueia:

1. **Comandos perigosos aprendidos**: denylist hard-coded de
   `rm -rf /`, `rm -rf ~`, `git push --force` em main, `:DROP TABLE`,
   `TRUNCATE`, etc. Compartilhada com `garraia-tools::safety_gate` do
   GarraMaxPower (§1.2.1), single source of truth.
2. **Paths críticos**: skill que altera `garraia-auth/`, `garraia-security/`,
   `garraia-workspace/migrations/`, `.github/workflows/`, `deny.toml`,
   `.gitleaksignore`, `Cargo.lock` exige aprovação obrigatória de
   `@security-auditor` + `@code-reviewer` (mesmo critério da AI Quality
   Ratchet quando toca segurança).
3. **Score threshold**: skill com score < `min_promote_score` (default 0.5)
   não promove — fica em `proposed`.
4. **Anti-flap**: skill que falhou 3+ vezes consecutivas no Evaluator é
   marcada `deprecated` automaticamente; precisa human override pra reativar.
5. **PII leak**: skill cujo conteúdo contém matches de regex de email,
   path absoluto contendo nome de usuário, token-shaped string
   (32+ alfanumérico) é rejeitada — Generator deve passar input por
   `garraia-telemetry::redact` antes de mostrar ao LLM.

## Alternatives considered

### Alternativa A: Importar Hermes Agent diretamente
- **Rejeitada.** Hermes tem licença e arquitetura próprias; copiar código
  cria dependência conceitual e legal. Também não temos garantia que
  o Safety Gate do Hermes mapeia para os paths críticos do GarraRUST
  (auth/RLS/secrets).

### Alternativa B: Estender `garraia-skills` em vez de criar `garraia-learning`
- **Rejeitada.** `garraia-skills` tem responsabilidade clara — consumir
  e distribuir skills externas. Adicionar Miner/Generator/Evaluator/
  Updater duplica a responsabilidade ("produzir" vs "consumir") e mistura
  fronteiras. Crate novo mantém Single Responsibility.

### Alternativa C: Skills como entradas em `garraia-workspace::memory_items`
- **Rejeitada.** Memória é fato sobre usuário/grupo; skill é procedimento.
  Fundir os dois quebra a separação rígida (ver tabela acima) e expõe
  skills à filtragem RLS, que faz sentido para memória mas não para
  procedimentos compartilhados entre projetos.

### Alternativa D: Skills armazenadas em Postgres
- **Rejeitada para v1.** Postgres requer Group Workspace (Fase 3) e
  embeddings precisam pgvector. Quebra o princípio "Learning Agent
  funciona sem Postgres em dev/CLI single-user". Fica como opção
  futura quando Postgres já está obrigatório.

### Alternativa E: Sem Skill Retriever — só Miner/Generator passivos
- **Rejeitada.** Sem Retriever, skills ficam latentes e o ROI cai a
  zero. Retriever é o que fecha o loop.

## Consequences

### Positivas

- **Compounding returns**: cada sessão bem-sucedida potencialmente vira
  ganho permanente de eficiência.
- **Auditabilidade total**: toda skill é arquivo git-tracked com history.
- **Human-in-the-loop preservado**: promoção em paths críticos exige
  aprovação manual; Safety Gate é hard wall.
- **Reuso de infraestrutura**: `garraia-skills` (parser/scanner/installer),
  `garraia-embeddings` (Retriever), `garraia-telemetry` (Evaluator),
  `garraia-tools::safety_gate` (Safety) — não reinventamos nada.
- **Fronteira clara** com memória/log/manual — usuários e
  desenvolvedores sabem onde cada tipo de conhecimento vive.

### Negativas

- **Crate novo + 10 sub-módulos** = ~3k-5k LOC iniciais.
- **Dependência forte de `garraia-embeddings`** (Fase 2.1) para Retriever
  funcionar de verdade. MVP pode rodar com retrieval por tag/scope até
  embeddings estarem prontos.
- **Risco de UX confusa**: usuário não sabe diferença entre "memória"
  vs "skill". Mitigação: Web UI no Console Garra Glass com tabs
  separados + tooltips explicativos + onboarding.
- **Custo LLM** para Skill Generator. Mitigação: default
  `openrouter/free`, batch processing, miner roda offline (cron,
  não sob latência de sessão).
- **Acúmulo de lixo em ~/.garra/skills/_candidates/**. Mitigação: TTL
  90d para candidates não-promovidos, garbage collection diária.

## Acceptance criteria

- [ ] Crate `garraia-learning` compila com `cargo check -p garraia-learning`.
- [ ] `garra skills list` mostra registry (global + por-projeto).
- [ ] `garra skills mine --from session-log.json` cria candidate em `~/.garra/skills/_candidates/`.
- [ ] `garra skills approve <name>` promove candidate → registry após Safety Gate passar.
- [ ] `garra skills reject <name>` move para `~/.garra/skills/_rejected/<name>-<ts>.md`.
- [ ] `garra skills lock <name>` bloqueia auto-update; futuras propostas viram PR-only.
- [ ] Tentativa de promover skill contendo `rm -rf /` (test fixture) é bloqueada com erro determinístico (SafetyDenial::DangerousCommand).
- [ ] Tentativa de promover skill que altera `crates/garraia-auth/src/lib.rs` (test fixture) exige label `security-audit-passed` no candidate, senão SafetyDenial::CriticalPath.
- [ ] Skill com score < 0.5 não é promovida (SafetyDenial::ScoreTooLow).
- [ ] Web UI no Console Garra Glass mostra aba "Skills" com lista + detalhe + diff + rollback.
- [ ] `garra skills rollback <name>` faz `git revert` do commit que introduziu a versão atual e retorna à anterior do history.
- [ ] Toda skill criada/atualizada tem entry em `audit_events` (action: `skill.{created,updated,promoted,rejected,locked,rolled_back}`) com `actor_user_id`, `resource_id` = skill name, `metadata` com diff_sha.
- [ ] Separação clara documentada em CLAUDE.md + README do crate `garraia-learning` + tabela neste ADR.
- [ ] Hermes Agent é mencionado em ADR como **referência conceitual** apenas; busca por importações de código do Hermes em `Cargo.lock` retorna zero.

## Implementation roadmap (issues filhas do épico [GAR-641](https://linear.app/chatgpt25/issue/GAR-641))

Issues criadas em 2026-05-17 sob o épico, label `epic:learning-agent`, projeto Fase 1, todas em Backlog:

1. [**GAR-642**](https://linear.app/chatgpt25/issue/GAR-642) **Learning Agent Architecture** (Priority: High, +label `adr-needed`) — ADR 0010 (este) → Accepted + scaffold do crate `garraia-learning` + integração mínima com `AgentRuntime`.
2. [**GAR-643**](https://linear.app/chatgpt25/issue/GAR-643) **Skill Miner** (Medium) — análise de session logs, detection de padrões repetíveis (≥3 ocorrências em contextos similares).
3. [**GAR-644**](https://linear.app/chatgpt25/issue/GAR-644) **Skill Generator** (Medium) — LLM-assisted skill drafting com prompt provider-agnóstico, default `openrouter/free`.
4. [**GAR-645**](https://linear.app/chatgpt25/issue/GAR-645) **Skill Registry** (High) — wrapper sobre `garraia-skills`, dual-scope global + project, lock-file em `_locks/`.
5. [**GAR-646**](https://linear.app/chatgpt25/issue/GAR-646) **Skill Retriever** (Medium) — embedding match via `garraia-embeddings` (Fase 2.1 prereq), scope filter, score threshold.
6. [**GAR-647**](https://linear.app/chatgpt25/issue/GAR-647) **Skill Evaluator** (High) — métricas objetivas (exit codes / cargo test pass count / `gh pr checks` / diff stats / log scan).
7. [**GAR-648**](https://linear.app/chatgpt25/issue/GAR-648) **Skill Auto-Updater** (Medium) — diff + branch nomeado `learning/skill-X-vN-vN+1` + PR via `gh`, nunca auto-merge.
8. [**GAR-649**](https://linear.app/chatgpt25/issue/GAR-649) **Skill Safety Gates** (**Urgent** — hard wall) — denylist + path-protected critical files + score threshold + anti-flap + PII redaction.
9. [**GAR-650**](https://linear.app/chatgpt25/issue/GAR-650) **Skill Versioning/Rollback** (Medium) — git-tracked, history em `_history/`, rollback via `git revert`.
10. [**GAR-651**](https://linear.app/chatgpt25/issue/GAR-651) **Web UI for Skills and Learning Logs** (Medium) — aba "Skills" + "Learning Logs" no Web Console Garra Glass (ADR 0009).

**Estimativa MVP** (issues 1+2+3+4+8 funcionais, sem Retriever/Web UI):
3 / 5 / 7 semanas isolado.

**Estimativa completa** (todas as 10): 4 / 7 / 12 semanas, depende de
`garraia-embeddings` (Fase 2.1) estar pronto para Retriever.

## Out of scope (explícito)

- **Não treinar pesos do modelo.** Skills são prompts/scripts versionados,
  não fine-tuning data.
- **Não copiar código do Hermes Agent.** Hermes é referência conceitual
  apenas.
- **Não bypass do Safety Gate** por flag de "modo dev" ou env var. Hard wall.
- **Não substituir `garraia-skills`** — Learning Agent integra; não duplica.
- **Não promover skill sem human-in-the-loop em paths críticos** (auth /
  crypto / CI / RLS — mesmo critério da AI Quality Ratchet).
- **Não promover skill sem que Evaluator tenha rodado pelo menos 1×.**

## References

- [Hermes Agent landing](https://hermes.example.com) — referência conceitual de produto (não usado como fonte)
- [`crates/garraia-skills/`](../../crates/garraia-skills/) — crate base reusado
- [`docs/adr/0002-vector-store.md`](0002-vector-store.md) — vector store para Retriever
- [`docs/adr/0009-web-console-design-system.md`](0009-web-console-design-system.md) — UI para Skills/Logs tabs
- [`.quality/README.md`](../../.quality/README.md) — filosofia AI Quality Ratchet (Safety Gate reusa o conceito)
- [`CLAUDE.md`](../../CLAUDE.md) §"Regras absolutas" — invariantes que Safety Gate enforce
- [`ROADMAP.md`](../../ROADMAP.md) §1.4 — entrada no roadmap
