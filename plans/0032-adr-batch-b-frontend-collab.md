# Plan 0032 — ADR batch B: Desktop frontend + Doc collaboration

**Status:** Em execução
**Autor:** Claude Opus 4.7 (sessão autônoma, ClaudeMaxPower + Superpowers)
**Data:** 2026-04-21 (America/New_York)
**Issues fechadas:** GAR-377, GAR-378
**Branch:** `feat/0032-adr-batch-b-frontend-collab`

---

## 1. Goal

Completar o sweep de ADRs bloqueados no backlog fechando os 2 restantes (`0007` desktop frontend + `0008` doc collaboration). Após este batch, todos os 8 slots ADR do índice estão populados (8/8 ✅ accepted ou accepted-with-deferred).

## 2. Non-goals

- Não implementa desktop frontend nem doc collab.
- Não escolhe componentes específicos (biblioteca de UI, state manager) — ADR fixa framework + arquitetura, não bibliotecas pontuais.
- Não supersede ADR 0007 baseline atual (HTML+Vanilla no `garraia-desktop` atual).

## 3. Scope

Arquivos criados:
- `docs/adr/0007-desktop-frontend.md` (GAR-377)
- `docs/adr/0008-doc-collaboration.md` (GAR-378)

Arquivos atualizados:
- `docs/adr/README.md` — status de 2 ADRs passa de `🔒 blocked` para `✅ accepted 2026-04-21`. Todos os 8 ADRs ficam populated.
- `plans/README.md` — entrada 0032.

## 4. Acceptance criteria

1. Cada ADR tem todas as seções MADR obrigatórias.
2. Cada ADR considera ≥ 3 opções com prós/contras numerados.
3. ADR 0007 define trigger de migração do HTML+Vanilla atual quando a complexidade justificar.
4. ADR 0008 define fase S0 (Tier 1 — sem colab tempo real, apenas drafts individuais) + trigger para S1 (collab real-time quando Tier 2 Docs entrar).
5. Zero código tocado.

## 5. Design rationale

### 0007 — Desktop frontend

- **Escolha primária**: **manter HTML + Vanilla JS** como baseline até complexidade de UI justificar framework. Trigger: quando > 10 telas, > 5 componentes reutilizáveis de complexidade alta, OU animation/interaction > 60fps em múltiplas superfícies.
- **Próxima opção** (quando trigger bater): **SolidJS** — fine-grained reactivity, bundle ~8kb gzip, melhor fit para Tauri (startup latency importa).
- SvelteKit considerado mas descartado para Tauri: SSR é overhead dead em webview local; file-system routing é desnecessário em single-window desktop.

### 0008 — Doc collaboration

- **S0 (Tier 1 Docs)**: single-editor drafts, optimistic lock per-doc. Zero CRDT. Suficiente para "notas pessoais no grupo" que é o Tier 1.
- **S1 (quando Tier 2 Docs entrar)**: **y-crdt (Rust-native)** como CRDT engine. Mature, Rust port mantido por Yrs team, compat com y.js em clients.
- Automerge considerado mas descartado: API JSON-like é menos eficiente para doc estruturado (blocks/nested); compactação menos otimizada.
- OT simplificado descartado: complexidade de prova de convergência não justifica vs CRDTs maduros em 2026.

## 6. Work breakdown

| Task | Arquivo | Estimativa |
|---|---|---|
| T1 | `plans/0032-adr-batch-b-frontend-collab.md` | 5 min |
| T2 | `docs/adr/0007-desktop-frontend.md` | 15 min |
| T3 | `docs/adr/0008-doc-collaboration.md` | 15 min |
| T4 | `docs/adr/README.md` + `plans/README.md` update | 5 min |
| T5 | Review + commit + PR | 10 min |

Total: ~50 min.

## 7. Verification

- Cada ADR é autocontido e pode ser lido sem o outro.
- Links para ADR 0003 + 0005 + 0004 quando referência cruzada for útil.
- ADR 0007 explicita que atual baseline (HTML+Vanilla) é válido e não deprecated.

## 8. Rollback plan

Revertir commit único.

## 9. Risk assessment

| Risco | Likelihood | Impact | Mitigação |
|---|---|---|---|
| ADR 0008 amarra a y-crdt prematuramente | Baixo | Baixo | Opção S0 é CRDT-free; trigger empírico para S1. |
| ADR 0007 parece "conservador demais" | Médio | Baixo | Rationale explícito (não premature complexity). Trigger documentado. |

## 10. Open questions

Nenhuma.
