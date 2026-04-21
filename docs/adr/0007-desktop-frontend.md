# 7. Desktop frontend (HTML+Vanilla → SolidJS trigger-driven)

- **Status:** Accepted
- **Deciders:** @michelbr84 + Claude (sessão autônoma 2026-04-21; review: `@code-reviewer`)
- **Date:** 2026-04-21
- **Tags:** fase-4, desktop, ui, aaa, gar-377
- **Supersedes:** none
- **Superseded by:** none
- **Links:**
  - Issue: [GAR-377](https://linear.app/chatgpt25/issue/GAR-377)
  - Plan: [`plans/0032-adr-batch-b-frontend-collab.md`](../../plans/0032-adr-batch-b-frontend-collab.md)
  - Current baseline: `crates/garraia-desktop/src-tauri/` (Tauri v2 + HTML/CSS/JS)
  - Roadmap: [ROADMAP §4 UX Multi-Plataforma AAA](../../ROADMAP.md)

---

## Context and Problem Statement

Hoje `garraia-desktop` (Tauri v2) serve um webview com **HTML + CSS + Vanilla JS** — scaffold mínimo do sidecar com overlay do gateway. Para atingir o target AAA da Fase 4 (micro-interações 60-120Hz, Lighthouse ≥ 95, bundle < 2MB, First Contentful Paint < 1s em máquina mediana), precisamos decidir se e quando adotar um framework web.

Três opções práticas em 2026:

1. **Manter HTML + Vanilla JS** — zero bundle, zero dep, máxima velocidade de webview.
2. **SvelteKit** — ergonomia top, grande ecosystem, SSR opcional.
3. **SolidJS** — fine-grained reactivity, bundle mínimo, sem VDOM.

A escolha impacta o design do Tauri bridge (bindings Rust ↔ JS), o padrão de estado no webview, a estratégia de testes de UI, e a velocidade de desenvolvimento de telas complexas (chat com streaming + drag-drop de arquivos + menções).

---

## Decision Drivers

1. **★★★★★ Startup latency** — desktop apps são julgados pelo tempo até a primeira interação. Webview Tauri arranca rápido; framework pesado destrói essa vantagem. Bundle size + parse + hydrate importa.
2. **★★★★★ Não adicionar complexidade prematura** — `CLAUDE.md` default é "não abstrair antes da dor". Atual scaffold é simples, funciona, não está doendo.
3. **★★★★ Tauri bridge ergonomics** — framework deve integrar bem com `@tauri-apps/api` IPC e `specta`/`tauri-bindgen` para type-safe commands.
4. **★★★★ Micro-interações 60-120Hz** — qualquer framework moderno consegue, mas abstrações pesadas (VDOM diffing) custam frames.
5. **★★★ Dev velocity para múltiplas telas** — quando tivermos 10+ telas com state compartilhado, vanilla JS escala mal (manual DOM wiring, manual event listeners, manual state sync).
6. **★★★ Type safety** — TS em framework moderno (Svelte, Solid) > JSDoc em vanilla.
7. **★★ Ecosystem** — UI kits, animation libs, testing tools.
8. **★★ Hot reload DX** — vite + HMR é esperado em dev.

---

## Considered Options

### A) **Manter HTML + Vanilla JS + micro-libs** *(recommended baseline)*

**O que é:** continuar com HTML + CSS (com Tailwind CSS opcional via CDN) + Vanilla JS. Para state, `nanostores` (< 1kb) ou Proxy-based observables. Para Tauri bindings, `@tauri-apps/api` direto + helper wrappers.

**Pros:**
- ✅ Zero build step complexity (só `npm run dev` → Vite serving arquivos estáticos).
- ✅ Bundle minúsculo (< 50kb inclusive Tailwind subset).
- ✅ Startup imediato no webview.
- ✅ Curva zero para novo contributor que saiba web básico.
- ✅ Full transparency: DOM é DOM, sem mágica.

**Cons:**
- ⚠️ Boilerplate para reactive UI (manual DOM update / event binding).
- ⚠️ Sem TS de primeira classe sem setup extra.
- ⚠️ Escala mal com > 10 telas complexas compartilhando state.

**Fit score para 2026-04:** 9/10. Para 2027 (Fase 4.x complexa): 5/10.

### B) **SolidJS**

**O que é:** framework reactive fine-grained (sem VDOM) com compilador que produz código imperativo otimizado. Bundle ~8kb gzip. Fully TS-typed.

**Pros:**
- ✅ Bundle mínimo entre frameworks (~7-10kb vs SvelteKit ~30kb+).
- ✅ Fine-grained reactivity — sem re-render cascade.
- ✅ API similar a React (JSX) — curva suave.
- ✅ TypeScript nativo + excelente DX.
- ✅ Integra bem com Vite.
- ✅ Testing via `@solidjs/testing-library` maduro.
- ✅ Tauri bindings trivial.

**Cons:**
- ⚠️ Ecosystem menor que Svelte/React.
- ⚠️ Component libraries específicas Solid ainda em crescimento.
- ⚠️ Novo build step + bundler complexity vs pure HTML.

**Fit score:** 8.5/10 quando complexidade justificar.

### C) **SvelteKit**

**O que é:** framework Svelte com SSR/SSG/SPA opcional. File-based routing, store nativo, bundle pequeno para apps simples.

**Pros:**
- ✅ Ergonomia top (syntax compacta).
- ✅ Ecosystem grande + componentes (Skeleton, Flowbite-Svelte).
- ✅ Store nativo bom.
- ✅ Animation API nativa (`svelte/motion`, `svelte/transition`).

**Cons:**
- ⚠️ **SSR é feature dead em webview local** — overhead sem ganho.
- ⚠️ **File-based routing** é desnecessário em single-window desktop (router programático basta).
- ⚠️ Bundle ~30kb+ vs ~10kb do Solid.
- ⚠️ SvelteKit traz complexidade de server adapter mesmo para SPA.

**Fit score:** 6/10 para desktop (é mais para web app SSR-friendly).

### D) **React + Vite**

Descartado preemptivamente: bundle maior, mais deps (React DOM, etc.), reconciler overhead. Há React-like alternatives (Preact, Solid) com melhor fit. GarraIA desktop não ganha nada com React ecosystem que Solid não entregue com menor overhead.

### E) **Leptos (Rust → WASM)**

Descartado: WASM compile time pesado, bundle WASM > 500kb mesmo com optimizations, e o webview Tauri-já-Rust não ganha performance significativa por ter "UI em Rust". Webview é JS-first; forçar WASM é complicado.

---

## Decision Outcome

**Escolha: Opção A (HTML + Vanilla baseline) com trigger documentado para Opção B (SolidJS).**

### Baseline atual (S0, effective 2026-04-21)

- **Linguagens**: HTML5, CSS (com Tailwind CSS via CDN em modo dev; Tailwind CLI em release build), TypeScript **opt-in** via `.ts` files compilados by Vite (progressivo).
- **State**: `nanostores` (~1kb) quando precisar de state compartilhado entre scripts.
- **Tauri bridge**: `@tauri-apps/api` direto. Helper wrapper em `src/tauri/bridge.ts` quando for util.
- **Router**: programático (simple URL hash + `switch`).
- **Build**: Vite como dev server + bundler; Tauri consome `dist/` em release.
- **Testing**: `vitest` para unit JS + `playwright` para E2E.

### Trigger para migração a SolidJS (S1)

Qualquer um:

1. **≥ 10 telas distintas** em produção com state compartilhado não-trivial.
2. **≥ 5 componentes reutilizáveis** de alta complexidade (editor, chat bubble com streaming + actions, file tree, dropdown search).
3. **Animação / micro-interação 120Hz em ≥ 3 superfícies simultâneas** requerendo orchestration fine-grained (vanilla com `requestAnimationFrame` fica caótico).
4. **Time de contributors ≥ 3** declarando que boilerplate Vanilla está degradando velocity.

Quando qualquer bater: abrir novo ADR `docs/adr/NNNN-desktop-frontend-migration.md` com plano de migração incremental (tela-a-tela, sem big-bang).

### Tauri bridge pattern (aplicável nos dois regimes)

- Use `specta` para emitir type definitions TS a partir de structs Rust que são parâmetros/retornos de `#[tauri::command]`.
- Manter `crates/garraia-desktop/src-tauri/src/commands/mod.rs` como módulo único exportando todos os commands.
- Test commands via `#[tauri::test]` em Rust; test IPC calls via playwright em E2E.

### Micro-interação performance target

- FPS ≥ 60 em hover/focus/click em laptop médio (MBP 2020 / Windows i5 2020).
- FPS ≥ 120 em high-refresh setups (configurable via CSS `@media (prefers-reduced-motion)` + `will-change`).
- LCP < 1s em máquina mediana.
- Lighthouse ≥ 95 no bundle (Performance + Accessibility + Best Practices).

---

## Consequences

### Positive

- Zero migration cost today.
- Bundle mínimo maximiza startup + frame budget.
- Trigger empírico previne escolha prematura.
- Caminho claro para S1 (SolidJS) documentado.

### Negative

- Algum boilerplate inicial para telas complexas.
- TS é opt-in, não mandatory (pode gerar mix JS/TS — aceitável em single-dev phase).

### Neutral

- Tailwind CSS via CDN é opcional; equipe pode preferir CSS custom.
- Vite já está no stack e não adiciona ops burden.

---

## Supersession path

Supersede via ADR novo com próximo inteiro monotônico disponível quando trigger S1 bater ou quando uma tecnologia ainda não-conhecida tornar-se obviamente melhor (ex.: WebGPU-based UI ganha tração em 2027+).

---

## Links de referência

- SolidJS: <https://www.solidjs.com/>
- SvelteKit: <https://kit.svelte.dev/>
- Tauri 2.0 Specta integration: <https://v2.tauri.app/develop/calling-frontend/#via-specta>
- Vite: <https://vitejs.dev/>
- Nanostores: <https://github.com/nanostores/nanostores>
- ADR 0003 Postgres (contexto de backend): [`0003-database-for-workspace.md`](0003-database-for-workspace.md)
