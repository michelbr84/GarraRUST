# Plan 0049 — GAR-429 Lote Q.1 (clippy strict prep + workspace `[lints]` + rustfmt.toml)

**Status:** Aprovado 2026-04-22 — Lote B-Q.1 (ship primeiro da onda Q por ser pequeno e destravar clippy strict)
**Autor:** Claude Opus 4.7 (sessão autônoma, ClaudeMaxPower + Superpowers, Lote B-Q.1)
**Data:** 2026-04-22 (America/New_York)
**Issue:** [GAR-429](https://linear.app/chatgpt25/issue/GAR-429) — CI quality gates (parcial, slice Q.1 de 3)
**Branch:** `plan/0049-clippy-strict`
**Pré-requisitos:** Gate Zero cargo-audit verde ([run #24805793514](https://github.com/michelbr84/GarraRUST/actions/runs/24805793514)); nenhum plan files concorrente tocando `Cargo.toml` workspace ou `.github/workflows/ci.yml`.
**Unblocks:** desliga `continue-on-error: true` no step clippy do `ci.yml`, passando clippy a bloqueante. Destrava a política "zero warning novo em arquivos tocados" exigida pelo usuário para os slices 0047 + 0048 + 0052.

---

## 1. Goal

Transformar `cargo clippy --workspace --exclude garraia-desktop -- -D warnings` em **bloqueante** no CI, eliminando os 19 warnings pré-existentes em `main` via **um único PR pequeno** (~180 LOC de diff líquido esperado).

Entrega:

1. **Inventário e fix** dos 19 warnings capturados via baseline pré-merge (exit code 0 — só `warning:`, nenhum `error:`).
2. **Workspace `[lints]` section** em `Cargo.toml` raiz com política explícita (`clippy::cognitive_complexity`, `clippy::too_many_lines`, `clippy::all`, `clippy::pedantic` seletivo).
3. **Novo `clippy.toml`** com overrides específicos do repo (threshold `too-many-arguments = 10` para helpers de audit que legitimamente têm muitos params, em vez de refactor prematuro).
4. **Novo `rustfmt.toml`** documentando o style enforcement do `cargo fmt --check` (hoje tudo default — formaliza a convenção).
5. **Flip do step clippy em `ci.yml`**: `continue-on-error: true` → removido; step passa a failar o job.
6. **Micro-PR único** — não quebrar em 0049a/b/c como originalmente considerado: diff pequeno o bastante (≤ 200 LOC) para um review + merge rápido, validado por `@code-reviewer`.

**O que NÃO entra neste slice:**
- Refactor das 5 APIs com `too_many_arguments` em structs de config tipadas (`CreateCustomModeRequest`, etc.) — deferido para sliver follow-up se o operador escolher perseguir clareza API. Estratégia neste slice: `#[allow(clippy::too_many_arguments)]` inline com justificativa (ver §5.2).
- Novos lints estritos além dos já listados (`clippy::dbg_macro`, `clippy::print_stderr`, etc.) — ficam para 0049-b/c em iteração separada.
- `#![deny(warnings)]` no crate root — explicitamente recusado (bloqueia evolução do compilador; política fica no CI).

## 2. Non-goals

- Zero mudança em cobertura de testes (é 0050).
- Zero mudança em `cargo-deny` / `cargo-mutants` (é 0051).
- Zero mudança de API pública.
- Zero nova dependência Rust.
- Zero mudança em `garraia-desktop` (ainda excluído da workspace clippy conforme convenção pós-plan 0027).
- Zero alteração do behavior runtime de qualquer função tocada — todos os fixes são reescritas sintaticamente equivalentes.

## 3. Scope

### Arquivos modificados (6 fontes com warnings + 3 config)

| Arquivo | Warnings | Lint | Fix estratégico |
|---|---|---|---|
| `crates/garraia-db/src/session_store.rs:802` | 1 | `too_many_arguments` (8/7) em `create_custom_mode` | `#[allow]` inline + comment |
| `crates/garraia-agents/src/runtime.rs:1038` | 1 | `too_many_arguments` (8/7) em `process_message_streaming_with_context` | `#[allow]` inline + comment |
| `crates/garraia-gateway/src/admin/audit.rs:41` | 1 | `too_many_arguments` (9/7) em `log_action` | `#[allow]` inline + comment |
| `crates/garraia-gateway/src/admin/store.rs:417, 450` | 2 | `too_many_arguments` (9/7, 8/7) em `append_audit` + `list_audit_log_filtered` | `#[allow]` inline ambos |
| `crates/garraia-gateway/src/bootstrap.rs:108, 556` | 2 | `collapsible_if` nested | reescrever `if let Some(x) = y { if cond { .. } }` → `if let Some(x) = y && cond { .. }` (let-chains estáveis desde Rust 1.92) |
| `crates/garraia-gateway/src/rate_limiter.rs:263` | 1 | `useless_conversion` | drop `u32::try_from(u32)` |
| `crates/garraia-gateway/src/rest_v1/uploads.rs:397` | 1 | `collapsible_if` | idem bootstrap |
| `crates/garraia-gateway/src/server.rs:989` | 1 | `needless_as_bytes` | drop `.as_bytes()` |
| **Remanescentes** (~8, não mapeados no head 150) | 8 | mix | descobrir via `cargo clippy --workspace --exclude garraia-desktop --message-format=short 2>&1 \| grep "^warning:"` na primeira task; categorizar antes de fixar |
| **`Cargo.toml`** (workspace raiz) | — | — | nova seção `[lints.rust]` + `[lints.clippy]` com política (ver §5.1) |
| **`clippy.toml`** (novo, raiz) | — | — | overrides de threshold (ver §5.2) |
| **`rustfmt.toml`** (novo, raiz) | — | — | formalização do style default (ver §5.3) |
| **`.github/workflows/ci.yml`** | — | — | flip do step clippy (ver §5.4) |

### Arquivos novos

- `clippy.toml` — 5-10 linhas, apenas overrides.
- `rustfmt.toml` — 3-5 linhas, formaliza defaults.
- `plans/0049-gar-429-clippy-strict.md` (este arquivo).

### Atualização do índice

- `plans/README.md` — acrescenta entrada `| 0049 | ... | GAR-429 | 🟡 Em execução ... |`.

## 4. Acceptance criteria

1. `cargo fmt --check --all` verde.
2. `cargo clippy --workspace --exclude garraia-desktop -- -D warnings` **exit code 0** (zero warning).
3. `cargo check --workspace --exclude garraia-desktop` verde.
4. `cargo test --workspace --exclude garraia-desktop` verde em Linux (compile-only em mac/win via matrix CI existente).
5. Step `clippy` em `.github/workflows/ci.yml` **sem** `continue-on-error: true`.
6. `cargo fmt` sem diff (rustfmt.toml novo não provoca reformatação em massa — é só documentação do default).
7. Grep `#[allow(clippy::too_many_arguments)]` retorna exatamente **5 matches** (todos com comentário `// TODO(plan-0049+): refactor to typed config struct`).
8. Grep `continue-on-error: true` em `.github/workflows/ci.yml` retorna zero matches no step clippy.
9. `cargo clippy --workspace --exclude garraia-desktop --all-targets -- -D warnings` (incluindo test-support features) também verde.
10. CI passa verde em **PR único** — não quebrar em múltiplos PRs.

## 5. Implementation details

### 5.1 Workspace `[lints]` — política

Acrescentar ao fim do `Cargo.toml` raiz:

```toml
[workspace.lints.rust]
unsafe_code = "warn"   # uso deliberado precisa de #[allow] explícito — já é convenção, formaliza.

[workspace.lints.clippy]
# Política baseline: all = warn, pedantic opt-in seletivo.
all = { level = "warn", priority = -1 }
# Thresholds operacionais (refinamento aprovado pelo usuário 2026-04-22):
cognitive_complexity = "warn"   # threshold via clippy.toml (ver §5.2)
too_many_lines = "warn"         # threshold via clippy.toml (ver §5.2)
# Pedantic sample (opt-in — manter pequeno no v1; expandir em 0049-b/c se precisar):
# missing_errors_doc = "warn"  # deferido
# missing_panics_doc = "warn"  # deferido

# Herdar em todos os crates:
# cada crates/*/Cargo.toml ganha [lints] workspace = true
```

Cada `crates/*/Cargo.toml` (exceto `garraia-desktop` que fica fora da pasta `crates/` workspace-side) ganha `[lints]` workspace = true. É 1 linha nova por crate × 19 crates = 19 linhas triviais — conta no diff total mas é mecânico.

### 5.2 `clippy.toml` — overrides operacionais

```toml
# GarraRUST clippy overrides (plan 0049).
# Thresholds escolhidos com base em baseline real do workspace (2026-04-22).

# too-many-arguments: default 7 → 10. Audit helpers (`log_action`, `append_audit`,
# `list_audit_log_filtered`) têm 8-9 params por razão de compliance trail
# (user_id + username + action + resource + outcome + timestamps + ...) — refactor para
# struct é cosmético, e dispara o limite default. 10 bate com o PR real.
too-many-arguments-threshold = 10

# cognitive-complexity: default 25 → 25 (mantém). Refinamento aprovado pelo usuário.
cognitive-complexity-threshold = 25

# too-many-lines: default 100 → 150. Fns de migration (Stage-* em migrate_workspace.rs)
# legitimamente passam de 100 linhas por atomic-tx design.
too-many-lines-threshold = 150
```

### 5.3 `rustfmt.toml` — documentação do default

```toml
# GarraRUST rustfmt config (plan 0049).
# Tudo explicit-default — formaliza a convenção que hoje roda implícita via ci.yml `cargo fmt --check --all`.

edition = "2024"
max_width = 100
tab_spaces = 4
newline_style = "Unix"
```

(Se `edition = "2024"` ainda não for o workspace target, cair para `"2021"` — checar `Cargo.toml` `resolver`/`edition` na primeira task.)

### 5.4 Flip do step clippy em `ci.yml`

```yaml
# antes
- name: cargo clippy
  run: cargo clippy --workspace --exclude garraia-desktop --all-targets -- -D warnings
  continue-on-error: true  # ← remover esta linha

# depois
- name: cargo clippy
  run: cargo clippy --workspace --exclude garraia-desktop --all-targets -- -D warnings
```

Nenhuma outra mudança no step.

### 5.5 Estratégia dos fixes (ordem em commits atômicos dentro do mesmo PR)

1. **Commit 1 — baseline reload:** re-rodar clippy completo (sem `head -150`), capturar os ~8 warnings restantes não inventariados. Atualizar este plan file se algum warning surpresa aparecer (ex.: `clippy::dbg_macro` esquecido).
2. **Commit 2 — mechanical fixes:** `collapsible_if` × 3, `useless_conversion` × 1, `needless_as_bytes` × 1, `if_same_then_else` × 1, `explicit_auto_deref` × 1. Sem mudança de comportamento; testes existentes cobrem.
3. **Commit 3 — `too_many_arguments` suppressions:** `#[allow(clippy::too_many_arguments)]` + comment TODO em cada um dos 5 sites. (Alternativa considerada: subir threshold em `clippy.toml` para `too-many-arguments-threshold = 10` — sim, isso já sobe para 10 na §5.2 e cobre os 8/7 e 9/7 sem precisar de `#[allow]`. **Reavaliar no commit 1 se já cobre tudo**: `clippy.toml` 10 vira os 8/7 e 9/7 em warning-free, `#[allow]` fica desnecessário — ajustar este plan se for o caso).
4. **Commit 4 — workspace `[lints]` + clippy.toml + rustfmt.toml:** commit único docs-config. Nenhum `.rs` tocado.
5. **Commit 5 — flip do ci.yml:** 1 linha removida. Último commit antes do PR abrir.
6. **Commit 6 — remanescentes:** caso os ~8 warnings do head-truncated output tenham outros lints não cobertos pelos commits 2-3, aplicar fix aqui.

### 5.6 Ordenação de tasks dentro da execução

a. Baseline reload + categorização (sem Edit no repo).
b. Commit 2 mechanical (rápido, alto ROI, zero risco).
c. Commit 4 config files (prepara o ambiente).
d. Commit 1→Commit 3 reavaliado (decidir se `clippy.toml` threshold 10 cobre todos os `too_many_arguments` ou se ainda preciso de `#[allow]`).
e. Commit 6 (remanescentes) se houver.
f. Commit 5 flip ci.yml (último).
g. `@code-reviewer` + `@security-auditor` (security por convenção do Lote A, embora este slice seja zero-security-surface — auditor emite APPROVE imediato esperado).

## 6. Estratégia de CI/GitHub Actions

- Este PR **passará** no próprio step clippy (porque o step ainda tem `continue-on-error: true` no momento que os fixes entram — o flip é o último commit do PR e é o que ativa a nova régua). Alternativa: o PR pode ter um commit final que flipa e roda CI uma última vez como sanity.
- Workflow `ci.yml` continua testando a matrix (fmt + clippy + check + test linux + build).
- Nenhum outro workflow (audit, deploy, release) afetado.
- Cargo-audit nightly (recém-validado via Gate Zero) fica intocado.

## 7. Estratégia de review

- `@code-reviewer` obrigatório — foca em: zero mudança de comportamento, 5 `#[allow(too_many_arguments)]` justificados (ou zero se threshold 10 resolver), diff < 200 LOC, commits atômicos.
- `@security-auditor` obrigatório por convenção Lote A — aqui é "skim APPROVE" esperado (sem crypto, sem SQL, sem PII, sem auth touched).
- **Review synchronous** — pequeno o bastante para 1 ciclo. Se vier REQUEST CHANGES com > 3 blockers, fatiar em 0049-b seguindo a convenção.

## 8. Rollback plan

- **Reversível:** sim, totalmente. Revert do PR volta o workspace ao estado pré-0049 (18 warnings re-introduzidos, `continue-on-error: true` restaurado).
- **Testado:** localmente via `git revert <sha>` antes do merge.
- **Impacto em produção:** zero — este PR não toca código runtime.
- **Impacto em deploys existentes:** zero — clippy é gate de desenvolvimento, não runtime.

## 9. Impacto em docs

- `CLAUDE.md` — atualizar seção "Convenções de código → Rust" adicionando: "`cargo clippy --workspace --exclude garraia-desktop -- -D warnings` é bloqueante em CI desde plan 0049."
- `CLAUDE.md` — acrescentar mensão dos 3 arquivos de config (`clippy.toml`, `rustfmt.toml`, workspace `[lints]`) em "Ferramentas preferenciais".
- Sem novo ADR (decisão técnica menor, não arquitetural).
- `plans/README.md` — entrada 0049 com status inicial "🟡 Em execução".

## 10. Impacto em workflows Linear

- GAR-429 (criada em 2026-04-22) fica **In Progress** assim que este PR abrir.
- Comentário no PR merge: "Slice Q.1/3 shipado. Plan 0050 (Q.2 coverage + deps + metrics) e 0051 (Q.3 deny + mutants) continuam em preparação."

## 11. Critério claro de pronto

- PR #XX mergeado em `main`.
- `gh run list --branch main --limit 3 --json conclusion` retorna `[{"conclusion":"success"}, ...]` em todas.
- `cargo clippy --workspace --exclude garraia-desktop -- -D warnings` no clone local de `main` sai exit 0.
- `ci.yml` diff na PR mostra linha `continue-on-error: true` removida.
- GAR-429 com comment "Q.1 shipped".

## 12. Open questions

1. **Threshold vs allow para `too_many_arguments`:** decidir no commit 1. Se `clippy.toml too-many-arguments-threshold = 10` cobre todos os 5 sites (8/7 e 9/7 ⇒ ambos ≤ 10), usar só threshold e **remover** `#[allow]` do plano — reduz diff e é mais limpo. Se algum for > 10 no baseline expandido, manter `#[allow]` naquele site.
2. **`edition = "2024"` vs `"2021"` no rustfmt.toml:** checar `Cargo.toml` na primeira task. Se workspace ainda é `resolver = "2"` + `edition = "2021"`, rustfmt.toml deve dizer `edition = "2021"`.
3. **Workspace `[lints.clippy]` pedantic opt-in:** no v1 fica só `all = warn`. Se o baseline expandido mostrar warnings `clippy::pedantic::*`, criar exception inline (`#[allow(clippy::pedantic::foo)]`) e tracking para 0049-b/c.

## 13. Métricas esperadas

- **LOC delta (líquido):** -15 a +180
  - Mechanical fixes: ~15 linhas (collapsible_if × 3 troca 2 linhas por 1; outros são 1-for-1)
  - 5× `#[allow]` com comment TODO: ~15 linhas (se necessário após reavaliação do §5.2)
  - `Cargo.toml` workspace [lints]: ~10 linhas + 19 linhas em crates (1 cada) = 29 linhas
  - `clippy.toml` novo: ~10 linhas
  - `rustfmt.toml` novo: ~5 linhas
  - `ci.yml`: -1 linha
- **Crates tocados:** 6 (com warning fix) + 19 (com workspace `[lints]` inheritance line) = 25 de 19 crates ativos + root.
- **Testes novos:** 0 (fixes são reescritas mecânicas; CI existente valida).
- **Cobertura:** neutro (nenhum path novo).
- **Mutação:** neutro (nenhum novo código para mutate).

## 14. Follow-ups conhecidos

- **0049-b (opcional, baixa prioridade):** refactor das 5 `too_many_arguments` APIs para structs de config tipadas (`CreateCustomModeRequest`, `AuditLogRequest`, `AuditFilter`, etc.). Justificativa: clareza API + teste mais fácil. Baixa prioridade porque as APIs são internas (não cross-crate).
- **0049-c (opcional, baixa prioridade):** adicionar lints estritos adicionais — `clippy::dbg_macro = "deny"`, `clippy::print_stderr = "warn"`, `clippy::todo = "warn"`, `clippy::missing_errors_doc = "warn"` (pedantic seletivo).
- Tracking em GAR-429 como sub-tasks.
