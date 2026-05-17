# Plan 0138 — Garra Learning Agent (epic + ADR 0010 stub)

**Status:** 🚧 Proposed (ADR 0010 stub committed; implementation issues to be created)
**Issue:** [GAR-641](https://linear.app/chatgpt25/issue/GAR-641) (criado 2026-05-17 com 10 sub-issues GAR-642..GAR-651)
**Branch:** TBD (vai usar branch dedicada quando iniciar implementação)
**Epic parent:** Fase 1.4 do `ROADMAP.md`

---

## Goal

Criar o épico **Garra Learning Agent / Self-Improving Operations Manual** no
roadmap e no Linear, com ADR 0010 (Proposed) capturando a arquitetura decidida,
para que sub-issues de implementação possam ser materializadas em sprints
futuros.

Esta entrega é **apenas planejamento** (ROADMAP + ADR + CLAUDE.md + plans/README
+ Linear epic + 10 sub-issues). Não inclui código Rust.

## Architecture

Ver [`docs/adr/0010-garra-learning-agent.md`](../docs/adr/0010-garra-learning-agent.md)
para a decisão arquitetural completa, alternativas avaliadas, fronteira semântica
(memória ≠ skill ≠ log ≠ manual), formato de skill, loops principais, Safety
Gate e critérios de aceite.

## Files affected (esta entrega — só docs)

- [x] `docs/adr/0010-garra-learning-agent.md` — novo, Status: Proposed
- [x] `ROADMAP.md` — nova subseção §1.4, atualização §1.5, §7 reescrito, §4 épico table
- [x] `CLAUDE.md` — bullet `garraia-learning/` em "Crates planejados"
- [x] `plans/README.md` — entrada deste plan (0138) + flip 0137 status
- [x] `plans/0138-gar-learning-agent-epic.md` — este arquivo

## Linear issues a criar (após `/mcp` autenticar)

Épico: [**GAR-641**](https://linear.app/chatgpt25/issue/GAR-641) — `EPIC: Garra Learning Agent / Self-Improving Operations Manual` (label `epic:learning-agent`, project Fase 1, priority High, status Backlog)

Sub-issues criadas 2026-05-17 (todas com `parentId: GAR-641`, label `epic:learning-agent`, project Fase 1, status Backlog):

1. [**GAR-642**](https://linear.app/chatgpt25/issue/GAR-642) **Learning Agent Architecture** (High, +label `adr-needed`) — ADR 0010 → Accepted + scaffold + integração com `AgentRuntime`.
2. [**GAR-643**](https://linear.app/chatgpt25/issue/GAR-643) **Skill Miner** (Medium) — análise de session logs, detection de padrões repetíveis (≥3 ocorrências).
3. [**GAR-644**](https://linear.app/chatgpt25/issue/GAR-644) **Skill Generator** (Medium) — LLM-assisted, prompt provider-agnóstico, default `openrouter/free`.
4. [**GAR-645**](https://linear.app/chatgpt25/issue/GAR-645) **Skill Registry** (High) — dual-scope global + project, lock-file em `_locks/`.
5. [**GAR-646**](https://linear.app/chatgpt25/issue/GAR-646) **Skill Retriever** (Medium) — embedding match via `garraia-embeddings` (Fase 2.1 prereq).
6. [**GAR-647**](https://linear.app/chatgpt25/issue/GAR-647) **Skill Evaluator** (High) — exit / cargo test / `gh pr checks` / diff / log scan.
7. [**GAR-648**](https://linear.app/chatgpt25/issue/GAR-648) **Skill Auto-Updater** (Medium) — branch + PR via `gh`, nunca auto-merge.
8. [**GAR-649**](https://linear.app/chatgpt25/issue/GAR-649) **Skill Safety Gates** (**Urgent**) — hard denylist + critical paths + score threshold + anti-flap + PII redaction.
9. [**GAR-650**](https://linear.app/chatgpt25/issue/GAR-650) **Skill Versioning/Rollback** (Medium) — git-tracked, history em `_history/`, rollback via `git revert`.
10. [**GAR-651**](https://linear.app/chatgpt25/issue/GAR-651) **Web UI for Skills and Learning Logs** (Medium) — aba "Skills" + "Learning Logs" no Web Console Garra Glass (ADR 0009).

## Acceptance (este plan 0138)

- [x] ADR 0010 commitado com Status: Proposed.
- [x] ROADMAP.md §1.4 adicionada referenciando ADR 0010 + 10 sub-issues + critérios de aceite.
- [x] ROADMAP.md §4 (épicos table) inclui linha `GAR-641 | 1.4 | Garra Learning Agent`.
- [x] CLAUDE.md "Crates planejados" inclui bullet `garraia-learning/`.
- [x] plans/README.md inclui entrada 0138.
- [x] Linear: épico [GAR-641](https://linear.app/chatgpt25/issue/GAR-641) criado 2026-05-17.
- [x] Linear: 10 sub-issues criadas (GAR-642..GAR-651) linkadas ao épico via `parentId`.
- [x] Linear: épicos table do ROADMAP §4 atualizado com IDs reais.

## Out of scope deste plan

- Implementação Rust de QUALQUER sub-componente. ADR é Proposed; vira Accepted
  quando primeira issue filha (Architecture) for mergeada.
- Promoção do crate `garraia-learning` para "Crates ativos" em CLAUDE.md
  (está em "planejados" até o scaffold mergear).
- Mudança em outros crates existentes. Quando vier, será via issue filha.

## Risks

| Risco | Mitigação |
|---|---|
| Linear MCP segue bloqueado por SSL | Payload structured no commit body; usuário cria via `/mcp` depois |
| Implementação atrasa Fase 2.1 (embeddings) que é prereq do Retriever | MVP roda sem Retriever (tag/scope match); Retriever vira issue posterior |
| Sobreposição com `garraia-skills` confunde contribuidores | ADR 0010 §"Topologia de crates" + §"Fronteira semântica" deixam fronteira clara |
| Skill perigosa aprendida e promovida | Safety Gate é hard wall + paths críticos exigem aprovação humana |

## Bookkeeping

Concluído 2026-05-17:
- Substituído `GAR-LEARNING-1` por `GAR-641` em ROADMAP §1.4 + §4 + CLAUDE.md + ADR 0010 + este plan.
- Sub-issues recebem IDs reais GAR-642..GAR-651.
- Status deste plan permanece **🚧 Proposed** até a primeira issue filha (GAR-642 Architecture) mergear e promover ADR 0010 para Accepted.
