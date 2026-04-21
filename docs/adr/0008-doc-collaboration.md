# 8. Doc collaboration (Tier 1 single-editor → y-crdt trigger-driven)

- **Status:** Accepted
- **Deciders:** @michelbr84 + Claude (sessão autônoma 2026-04-21; review: `@code-reviewer`)
- **Date:** 2026-04-21
- **Tags:** fase-3, ws-docs, collab, crdt, gar-378
- **Supersedes:** none
- **Superseded by:** none
- **Links:**
  - Issue: [GAR-378](https://linear.app/chatgpt25/issue/GAR-378)
  - Plan: [`plans/0032-adr-batch-b-frontend-collab.md`](../../plans/0032-adr-batch-b-frontend-collab.md)
  - Roadmap: [ROADMAP §3.8 Docs Tier 1+2](../../ROADMAP.md)
  - y-crdt (yrs): <https://github.com/y-crdt/y-crdt>
  - Automerge: <https://github.com/automerge/automerge>
  - Yjs (JS): <https://github.com/yjs/yjs>

---

## Context and Problem Statement

A Fase 3.8 do ROADMAP promete **Docs colaborativos Notion-like**. Duas camadas de expectativa:

- **Tier 1**: notas + drafts individuais por usuário no escopo de um grupo. Sem colab tempo real. "Meu documento compartilhado com o grupo."
- **Tier 2**: edição simultânea multi-usuário com awareness (cursores + seleções visíveis), offline-first, histórico convergente — **Notion / Google Docs level**.

Escolher CRDT para Tier 2 **antes** da Fase 3.8 iniciar é a ADR que essa issue (GAR-378) pede. Porém, escolher sem experimentar é custoso — cada CRDT tem trade-offs específicos de payload size, convergência, memory.

Esta decisão impacta:
- Schema de `docs` / `doc_blocks` / `doc_ops` (migration futura).
- Design de endpoints WebSocket para sync (`/v1/docs/{id}/stream`).
- Storage: state é binary blob do CRDT, não JSON estruturado.
- Undo/redo design.
- Backup/restore.

---

## Decision Drivers

1. **★★★★★ Convergence correctness** — regra não-negociável. Dois usuários editando offline + merge não pode perder dados nem divergir estado.
2. **★★★★★ Rust-native primary** — backend é Rust. CRDT deve ter implementação Rust performática (sem subprocess / sem FFI pesado).
3. **★★★★ Browser/mobile client fit** — client edita em JS/TS (desktop webview) ou Dart (mobile). CRDT deve ter client mature nesses runtimes ou ser facilmente portável.
4. **★★★★ Payload size / bandwidth** — collab em mobile flaky network: cada op deve ser compacta.
5. **★★★ History + undo** — Tier 2 quer histórico temporal (Notion page history).
6. **★★★ Awareness (cursores, seleções)** — não faz parte do CRDT propriamente dito mas precisa hook.
7. **★★ Rich text model** — blocks com children (list items, nested), inline formatting, embeds, backlinks.

---

## Considered Options

### A) **S0 single-editor Tier 1 + S1 y-crdt Tier 2** *(recommended)*

**O que é:** estratégia em duas fases.

- **S0 (effective agora quando Tier 1 entrar)**: docs são rows em Postgres com `content JSONB`. Edição = transactional `UPDATE` com optimistic lock (`version BIGINT` incrementado; mismatch retorna 409). Sem multi-editor concorrência — usuário vê doc, edita, salva, conflito resolvido server-side ou por user.
- **S1 (quando Tier 2 entrar, trigger-driven)**: adota **y-crdt** (Rust port maduro de Yjs). Blob binary em `doc_ops` table; sync delta-based via WebSocket; client JS usa `yjs` standard; client Dart usa `yrs-dart` bindings (ou WASM fallback).

**Pros:**
- ✅ S0 destrava Tier 1 imediatamente sem CRDT complexity.
- ✅ S1 usa stack já-validada por Figma, Notion-alike tools (millions of users).
- ✅ y-crdt tem Rust-native implementação (`yrs` crate) performática.
- ✅ `yjs` client JS é de facto standard para collaborative editing.
- ✅ Compacta ops (delta encoding + compression).
- ✅ Awareness module separado mas integrado (`y-protocols/awareness`).

**Cons:**
- ⚠️ Mobile Dart ainda requer WASM wrapper ou binding via FFI (menos ergonômico).
- ⚠️ Rich-text model em Yjs (`Y.XmlFragment` + `Y.Text`) tem curva de aprendizado.

**Fit score:** 9/10.

### B) **Automerge direto (sem fase S0)**

**O que é:** adotar Automerge desde Tier 1 — escolhido por toda entrega ser CRDT-backed.

**Pros:**
- ✅ API JSON-like familiar.
- ✅ Rust + JS + Rust core via FFI.
- ✅ Awareness built-in em automerge-repo.

**Cons:**
- ⚠️ CRDT complexity desde day one — overhead para Tier 1 que não precisa collab real-time.
- ⚠️ Payload maior que yjs em experimentos comparativos (2023 benchmark Kevin Jahns).
- ⚠️ Document compaction menos maduro; docs longos crescem disk.
- ⚠️ Community adoption menor que Yjs em 2026.

**Fit score:** 7/10.

### C) **OT simplificado (operational transformation)**

**O que é:** algoritmo OT clássico (Google Wave / Google Docs legacy) implementado sob medida.

**Pros:**
- ✅ Modelo mental familiar.

**Cons:**
- ⚠️ **Prova de convergência é O(N²) em operações** — bugs sutis podem permanecer dormentes por anos.
- ⚠️ Requer **central server** authoritative — não oferece offline-first real.
- ⚠️ CRDT venceu a arquitetura em 2020s (Yjs, Automerge ganharam).
- ⚠️ Implementar OT do zero = pelo menos 6 meses + risco alto de bug.

**Fit score:** 2/10. Descartado.

### D) **ShareDB + JSON0**

**O que é:** biblioteca Node.js para realtime collab + JSON OT.

**Cons:**
- ⚠️ Backend Node, não Rust.
- ⚠️ JSON0 OT foundation (não CRDT) — mesmos problemas de C.

**Fit score:** 2/10. Descartado.

### E) **Rogue own-CRDT implementation**

Descartado preemptivamente. Escrever CRDT next to production é o erro clássico. Yjs/Automerge são consequência de décadas de pesquisa + PhDs — não competir.

---

## Decision Outcome

**Escolha: Opção A — S0 Tier 1 single-editor Postgres-only + S1 Tier 2 y-crdt quando trigger bater.**

### S0 Tier 1 (current + imediato)

**Schema Postgres (migration futura, não neste ADR):**

```
docs (
  id UUID PK,
  group_id UUID FK, -- RLS scope
  author_id UUID FK,
  title TEXT NOT NULL,
  content JSONB NOT NULL,    -- block tree
  version BIGINT NOT NULL,   -- optimistic lock
  created_at, updated_at, deleted_at
)
```

- **Update**: `UPDATE docs SET content=$1, version=version+1 WHERE id=$2 AND version=$3 RETURNING ...`. 0 rows = 409 conflict; client refetches + retries.
- **No real-time sync**. Polling OR "Refresh" button por user.
- **Offline edits** = drafts locais (IndexedDB / Hive em Flutter); sync em próximo online.
- **Rich text model**: TipTap / Lexical / Slate JSON schema — framework-agnostic intermediate format.

### S1 Tier 2 trigger

Qualquer um:

1. **Produto aprova "collab real-time multi-user" como feature explícita** do roadmap.
2. **Conflito de version em S0 > 10% das saves** em 30 dias (indica que usuários estão pisando uns nos outros).
3. **Cliente pede "Google Docs-like" editing** como requisito.

Quando bater:

- Nova tabela `doc_ops (id, doc_id, op_blob BYTEA, client_id UUID, lamport BIGINT, created_at)` armazenando deltas y-crdt.
- Novo endpoint `GET /v1/docs/{id}/stream` (WebSocket) sincroniza ops em tempo real.
- Compactação periódica: `docs.content_snapshot BYTEA` + `doc_ops` acumuladas depois do snapshot.
- Awareness via `y-protocols/awareness` (in-memory, não-persistido).

### Client compatibility (S1)

| Runtime | Client recomendado | Status |
|---|---|---|
| Browser / Tauri webview | `yjs` (JS, canonical) | Mature |
| Desktop Rust (futuro) | `yrs` direto | Mature |
| Mobile Flutter/Dart | `yrs-dart` via FFI OR WASM wrapper | **Fragile** — monitorar + PoC antes de S1 |

**Decision**: se mobile Dart não tiver client maduro quando S1 chegar, mobile entra em **read-only mode** em docs Tier 2 inicialmente (ler state reconstruído server-side, editar em desktop/web only). Documentar em UX.

### Backup + recovery (S1)

- `doc_ops` + `docs.content_snapshot` são ambos backed by Postgres pg_dump.
- Restore: replay de `doc_ops` desde último snapshot.
- Offline: CRDT é design offline-first — não há "restore" além do storage local do client.

---

## Consequences

### Positive

- Tier 1 entrega valor imediato sem CRDT ceremony.
- Trigger para S1 é empírico, não prematuro.
- y-crdt é stack de fato para collab editing em 2026.
- Mobile caveat documentado antes da decisão ser take-it-or-leave-it.

### Negative

- Migrar S0 → S1 exige conversão de `content JSONB` → y-crdt Y.Doc inicial (seed state). Feito via migration SQL + job de conversão.
- Sem collab em Tier 1 pode frustrar early users esperando Notion experience (mitigação: TOS explicita).

### Neutral

- Rich-text model em JSONB pode ser TipTap-compat, facilitando futura port para y-crdt.
- ADR não amarra cliente de editor (TipTap, Lexical, ProseMirror) — isso fica em plano de implementação.

---

## Supersession path

Supersede via ADR novo (próximo inteiro monotônico disponível) se:

- y-crdt project stall (unlikely; Yrs team + Bloomberg + Alphabet contribs em 2024-2026).
- Novo CRDT fundamentalmente superior emerge com momentum (ex.: sem-lamport-clocks novelty).
- Requisito de colab cross-system (Yjs + Automerge mix) surge.

---

## Links de referência

- Yjs (JS canonical): <https://github.com/yjs/yjs>
- y-crdt (Rust yrs): <https://github.com/y-crdt/y-crdt>
- Automerge: <https://github.com/automerge/automerge>
- CRDT survey paper (Shapiro et al.): <https://hal.inria.fr/inria-00609399>
- Kevin Jahns "Yjs vs Automerge" benchmark (2023): <https://github.com/dmonad/crdt-benchmarks>
- TipTap (editor agnostic to CRDT backend): <https://tiptap.dev/>
- ADR 0003 Postgres: [`0003-database-for-workspace.md`](0003-database-for-workspace.md)
