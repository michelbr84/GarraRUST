# Plan 0144 — GAR-642: scaffold do crate `garraia-learning` + promover ADR 0010 a Accepted

**Linear issue:** [GAR-642](https://linear.app/chatgpt25/issue/GAR-642) — "Learning Agent Architecture — ADR 0010 → Accepted + scaffold crate garraia-learning" (Backlog → In Progress, High, +label `adr-needed`). Parent epic: [GAR-641](https://linear.app/chatgpt25/issue/GAR-641). Project: "Fase 1 — Core & Inferência".

**Status:** ⏳ Draft — aprovado 2026-05-17 (Florida) na routine. Sub-issue 1/10 do épico Learning Agent (ADR 0010).

**Branch:** `routine/202605180015-gar-642-learning-scaffold`.

---

## Goal

Promover [ADR 0010](../docs/adr/0010-garra-learning-agent.md) de **Proposed → Accepted** entregando o scaffold do novo crate `garraia-learning/` com a topologia de 10 módulos descrita em §"Topologia de crates" do ADR + tipos base re-exportados em `lib.rs` + trait de hook `BeforeAction` (default `None`) como contrato shape-only para o futuro Retriever (GAR-646).

Esta entrega **não implementa nenhum sub-componente**: cada módulo é `//! placeholder` com referência cruzada à issue filha (GAR-643..GAR-651) que vai materializar a lógica.

## Architecture

Espelha exatamente a topologia descrita em ADR 0010 §"Topologia de crates":

```text
crates/garraia-learning/
├── Cargo.toml
└── src/
    ├── lib.rs           — fachada: re-exporta tipos base + módulos + BeforeAction trait
    ├── miner.rs         — placeholder (GAR-643)
    ├── generator.rs     — placeholder (GAR-644)
    ├── registry.rs      — placeholder (GAR-645)
    ├── retriever.rs     — placeholder (GAR-646)
    ├── evaluator.rs     — placeholder (GAR-647)
    ├── updater.rs       — placeholder (GAR-648)
    ├── safety.rs        — placeholder (GAR-649, Urgent)
    ├── versioning.rs    — placeholder (GAR-650)
    └── override.rs      — placeholder (CLI/UI approve/reject/lock/delete; subsumido por GAR-651 + futuro)
```

`override` é palavra reservada do Rust; declaração `pub mod r#override;` no `lib.rs` mantém o nome de arquivo `override.rs` alinhado ao ADR. Acesso externo via `garraia_learning::r#override::…`.

### Tipos base em `lib.rs`

Mínimo para que sub-issues 2-10 tenham contratos estáveis para implementar:

- `enum SkillScope { Global, Project }` — `~/.garra/skills/` vs `.garra/skills/`.
- `enum SkillSource { Mined, Authored, Imported }` — origem do skill.
- `struct SkillScore(f32)` — EMA em [0.0, 1.0]; `SkillScore::MIN_PROMOTE = 0.5`.
- `enum SafetyDenial` — 5 razões (DangerousCommand, CriticalPath, ScoreTooLow, AntiFlap, PiiLeak) com `thiserror::Error`.
- `struct Skill { name, scope, source, score }` — shape mínimo; campos adicionais (frontmatter completo) ficam para GAR-645.
- `trait BeforeAction { fn before_action(&self, ctx: &SkillRequestContext) -> Option<Skill> { None } }` — contrato do hook que o `AgentRuntime` vai consumir quando Retriever existir.
- `struct NoopBeforeAction` impl `BeforeAction` (usado pelo runtime até GAR-646).

### Integração com `AgentRuntime`

**Shape-only nesta slice.** A trait `BeforeAction` mora em `garraia-learning` e o `AgentRuntime` (`garraia-agents`) **não** é modificado neste PR — implementação concreta + wiring acontece em GAR-646 (Retriever). Justificativa: evita criar dependência `garraia-agents → garraia-learning` antes de existir comportamento útil, e mantém o scaffold revertível com um único `git rm -r crates/garraia-learning`.

A trait é o contrato; `NoopBeforeAction::default()` é a implementação default que o futuro wiring vai injetar.

### Deps do novo crate

Mínimas para compilar limpo sob `cargo clippy --workspace -- -D warnings`:

- `serde` (workspace) — derive em `SkillScope`/`SkillSource`/`SkillScore`/`Skill`.
- `thiserror` (workspace) — `SafetyDenial`.

Deps explícitas mencionadas pelo issue (`garraia-skills`, `garraia-common`, `garraia-tools`, `tokio`, `tracing`) **não** são adicionadas neste PR porque o scaffold ainda não as usa — adicionar agora dispara `unused-crate-dependencies` se algum CI ratchet futuro elevar a allow-by-default. Cada sub-issue (GAR-643..GAR-651) adiciona a dep que precisa quando precisar (`garraia-skills` em GAR-645, `garraia-tools` em GAR-649, etc.). Documentado em `lib.rs`.

---

## Design invariants

1. **Zero behaviour change.** Nenhum crate existente é modificado a não ser:
   - `Cargo.toml` (adiciona `crates/garraia-learning` aos `members`).
   - `CLAUDE.md` (move bullet de "Crates planejados" para "Crates ativos").
   - `docs/adr/README.md` (adiciona linhas para 0009 + 0010 — ambas faltando atualmente; ver §"Drift colateral" abaixo).
   - `docs/adr/0010-garra-learning-agent.md` (header `Status: Proposed → Accepted` + data + PR ref).
   - `plans/README.md` (linha 0139).
2. **`r#override` em vez de renomear.** Manter alinhamento literal com a topologia do ADR. Custo: 1 caractere a mais em call sites; benefício: ADR/código nunca divergem.
3. **`BeforeAction` é trait, não método em `AgentRuntime`.** Decisão revertida-se trivialmente; alternativa "modificar AgentRuntime agora" é escopo creep.
4. **Nenhum `unwrap()` ou `expect()`.** Mesmo em placeholders. Doc comments apenas.
5. **`#![forbid(unsafe_code)]` no crate root.** Garante que sub-issues futuras precisem justificar `unsafe` em ADR/plan separado.
6. **`#![deny(missing_docs)]` no crate root.** Força que cada tipo público tenha doc comment desde o scaffold (acabamos com a janela em que o crate compila sem docs e sub-issues herdam o débito).

---

## Drift colateral coberto neste PR

`docs/adr/README.md` está com índice congelado em 0008. Faltam:

- Linha para [ADR 0009 — Web Console Design System "Garra Glass"](../docs/adr/0009-web-console-design-system.md) (mergeado em main há semanas, ver §1.5 do ROADMAP).
- Linha para [ADR 0010 — Garra Learning Agent](../docs/adr/0010-garra-learning-agent.md) (criada na sessão de plan 0138).

Ambas adicionadas neste PR para fechar o gap. Adicionar só a 0010 sem adicionar a 0009 deixaria o índice ainda parcialmente errado — corrigir as duas é a única forma honesta.

---

## Validações pré-plano (gate executado nesta sessão)

- ✅ `docs/adr/0010-garra-learning-agent.md` existe com Status: Proposed (verificado).
- ✅ ADR 0010 §"Acceptance criteria" tem 12 itens; deste plan, fechamos os critérios marcados como "compila", "estrutura", "ADR Accepted", "CLAUDE.md atualizado". Critérios de comportamento (`garra skills list/approve/reject/lock/rollback`, Safety Gate runtime, Web UI, audit_events) ficam para GAR-643..GAR-651.
- ✅ Workspace `Cargo.toml` membros listados — nenhum conflito com `garraia-learning`.
- ✅ `garraia-glob` (template de scaffold simples) usado como referência de tamanho mínimo (`Cargo.toml` + `lib.rs` com `pub mod`/`pub use`).
- ✅ `garraia-skills` já existe e expõe `SkillFrontmatter` / `SkillScanner` / `SkillInstaller` — confirmado que serão reusados em GAR-645 (não neste PR).
- ✅ `override` é reserved keyword em Rust 2024 — `r#override` é a forma canônica.
- ✅ `docs/adr/README.md` realmente está parado em 0008 (verificado linha 51 do arquivo).
- ✅ Branch `routine/202605180015-gar-642-learning-scaffold` criada a partir de `main@29f9493`.

---

## Out of scope (rejeitado explicitamente)

- Implementação de Miner, Generator, Registry, Retriever, Evaluator, Updater, Safety, Versioning, Override — uma issue cada (GAR-643..GAR-651).
- Modificação de `crates/garraia-agents/` para wiring real do `BeforeAction` — GAR-646.
- Adicionar `garraia-skills` / `garraia-common` / `garraia-tools` / `tokio` / `tracing` como deps — cada sub-issue adiciona quando usar.
- Web UI no Web Console Garra Glass — GAR-651.
- `garra skills <subcommand>` CLI — sub-issue separada (subsumida por GAR-642a futuro ou casada com GAR-645).
- Migration de Postgres para `learning_*` tables — out of scope por design (ADR 0010 §Alternativa D rejeita Postgres no v1).
- Atualizar §1.5 do ROADMAP — esta seção já cobre o épico em 1.4; flip de status do GAR-642 vai num doc-only PR pós-merge (T8).
- Atualizar `.quality/baseline.json` — scaffold não muda métricas existentes; quality-ratchet vai relatar no PR.

---

## Rollback plan

Cada task é commit independente; revert é cirúrgico:

- T0 (registrar 0144 em `plans/README.md`) — revert remove a linha.
- T1 (criar `crates/garraia-learning/Cargo.toml`) — revert deleta o arquivo.
- T2 (criar `crates/garraia-learning/src/lib.rs` + 9 módulos placeholder) — revert deleta o diretório `src/`.
- T3 (adicionar `crates/garraia-learning` ao `Cargo.toml` `members`) — revert remove a linha.
- T4 (promover ADR 0010 Status + adicionar 0009/0010 ao `docs/adr/README.md`) — revert volta os 3 deltas.
- T5 (mover bullet `garraia-learning` em CLAUDE.md de "planejados" para "ativos") — revert volta o bullet.

Worst-case rollback: 6 `git revert` sequenciais. Zero risco para crates existentes (eles não foram tocados).

---

## §12 Open questions (pré-start)

1. **Adicionar `garraia-skills` como dep agora?** → **Decisão:** não. Scaffold não usa o parser. GAR-645 (Registry) adiciona quando wire `SkillScanner`/`SkillInstaller`. Justificativa: `unused-crate-dependencies` é allow-by-default hoje, mas se um ratchet futuro promover, scaffold já cai limpo.
2. **Adicionar `tokio` como dep agora?** → **Decisão:** não. Sem `async fn`/`#[tokio::test]` no scaffold. GAR-643 (Miner) ou GAR-646 (Retriever) adicionam quando precisarem.
3. **Re-export de `SkillFrontmatter` do `garraia-skills`?** → **Decisão:** não nesta slice. GAR-645 decide se re-exporta (frontmatter unificado) ou se Learning Agent estende com seu próprio struct (frontmatter aprendido tem campos extras: `score`, `last_used_at`, `source`, etc., per ADR 0010 §Formato de skill). Acoplar agora amarra a decisão.
4. **`SkillScore` é `f32` ou `f64`?** → **Decisão:** `f32`. EMA com 4 casas de precisão é suficiente; `f64` ocupa o dobro em registries grandes. ADR 0010 não especifica; documentado em doc-comment do struct.
5. **`BeforeAction::before_action` recebe `&str` ou um struct `SkillRequestContext`?** → **Decisão:** struct dedicado (`SkillRequestContext { intent: String, scope_hint: Option<SkillScope> }`) — `&str` amarra contrato ao primeiro uso e força refactor breaking em GAR-646. Struct vazio-extensível resolve.
6. **`Skill` struct expõe `name: String` ou `name: SkillName(String)` newtype?** → **Decisão:** `String` simples. Newtype com validação (alpha-num + hyphen) já existe em `garraia_skills::parser::validate_skill` — duplicar agora é débito. GAR-645 decide a integração canônica.
7. **`#[non_exhaustive]` em `SafetyDenial`?** → **Decisão:** sim. GAR-649 vai adicionar variants (ex: novo padrão de PII, novo critical path); marcar como `#[non_exhaustive]` evita breaking change quando isso acontecer.

---

## File Structure

**Criar:**
- `crates/garraia-learning/Cargo.toml` — package + 2 deps (`serde`, `thiserror`).
- `crates/garraia-learning/src/lib.rs` — fachada com tipos base + 9 `pub mod` (incluindo `r#override`).
- `crates/garraia-learning/src/miner.rs` — placeholder.
- `crates/garraia-learning/src/generator.rs` — placeholder.
- `crates/garraia-learning/src/registry.rs` — placeholder.
- `crates/garraia-learning/src/retriever.rs` — placeholder.
- `crates/garraia-learning/src/evaluator.rs` — placeholder.
- `crates/garraia-learning/src/updater.rs` — placeholder.
- `crates/garraia-learning/src/safety.rs` — placeholder.
- `crates/garraia-learning/src/versioning.rs` — placeholder.
- `crates/garraia-learning/src/override.rs` — placeholder.

**Modificar:**
- `Cargo.toml` — adicionar `"crates/garraia-learning"` ao `members`.
- `docs/adr/0010-garra-learning-agent.md` — header `Status: Proposed → Accepted`, adicionar data + PR ref.
- `docs/adr/README.md` — adicionar linhas para 0009 + 0010.
- `CLAUDE.md` — mover bullet `garraia-learning/` da seção "Crates planejados" para a lista de crates ativos (após `garraia-skills/`).
- `plans/README.md` — registrar linha 0144.

---

## M1 tasks (commit-by-commit)

- [ ] **T0** — `docs(plans): add plan 0144 for GAR-642 learning scaffold` — só o arquivo deste plan.
- [ ] **T1** — `feat(learning): scaffold garraia-learning crate (GAR-642)` — `Cargo.toml` + `src/lib.rs` + 9 módulos placeholder + adiciona ao workspace `members`. Inclui `cargo check -p garraia-learning` verde local.
- [ ] **T2** — `docs(adr): promote ADR 0010 to Accepted + index 0009/0010 (GAR-642)` — flip header de 0010 + 2 linhas novas em `docs/adr/README.md`.
- [ ] **T3** — `docs(claude): move garraia-learning to active crates (GAR-642)` — CLAUDE.md edits.

Cada task tem unitary commit; T1 e T2 podem rodar em paralelo no review (não há ordem rígida), mas commit-time é sequencial.

---

## Risk register

| Risco | Severidade | Mitigação |
|---|---|---|
| `cargo check --workspace` falha por edition mismatch | Baixa | Workspace é `edition = "2024"`; `garraia-glob` é `2021` mas funciona — herdar 2024 do workspace é safer. |
| `cargo clippy --workspace -- -D warnings` falha em `missing_docs` | Média | `#![deny(missing_docs)]` é invariante; cada tipo público tem doc comment desde o scaffold. |
| `r#override` quebra rust-analyzer ou outras tools | Baixa | Raw identifiers existem desde 2015; padrão estável. Caso falhe, T1 vira "rename to manual_override.rs" sem mexer no ADR (o ADR descreve componentes, não nomes de arquivo). |
| Ratchet `unused-crate-dependencies` é elevado antes do PR mergear | Baixa | Scaffold tem só `serde` + `thiserror`, ambos usados em código. Zero deps mortas. |
| Test runner detecta novo crate sem `#[test]` e exit-code não-zero | Baixa | Crates sem testes são válidos para `cargo test`; saída é "test result: ok. 0 passed; 0 failed". |
| Quality Ratchet bloqueia PR por LOC delta | Baixa | PR-1 ratchet é report-only (ver CLAUDE.md §"AI Quality Ratchet"). |

---

## Acceptance criteria (gate antes de marcar GAR-642 Done)

- [ ] `cargo check --workspace` verde.
- [ ] `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` verde.
- [ ] `cargo check -p garraia-learning` verde em isolado.
- [ ] `docs/adr/0010-garra-learning-agent.md` header mostra `**Status:** Accepted` + data `2026-05-17` (Florida) + PR `#NNN`.
- [ ] `docs/adr/README.md` lista as 10 ADRs sequencialmente (0001..0010) com Status correto.
- [ ] CLAUDE.md "Crates ativos" inclui `garraia-learning/` com bullet preservado do "Crates planejados".
- [ ] CLAUDE.md "Crates planejados" não menciona mais `garraia-learning`.
- [ ] `plans/README.md` linha 0144 presente.
- [ ] Linear GAR-642: status flipado de Backlog → In Progress → Done; comment com link do PR e commit sha.

---

## Cross-references

- ADR: [`docs/adr/0010-garra-learning-agent.md`](../docs/adr/0010-garra-learning-agent.md)
- Epic Linear: [GAR-641](https://linear.app/chatgpt25/issue/GAR-641)
- Plan-mãe: [`plans/0138-gar-learning-agent-epic.md`](0138-gar-learning-agent-epic.md)
- Crate base reusado: [`crates/garraia-skills/`](../crates/garraia-skills/)
- ROADMAP §1.4 / §7 priorização que escolheu este slice.

---

## Estimativa

0.5 / 1 / 2 horas (apenas scaffold + 4 doc edits). Issue original estimava 0.5 / 1 / 2 semanas mas isso assumia integração real com `AgentRuntime` — escopo trimado para "shape-only" via trait neste plan.
