# Plan 0064 — AI Quality Ratchet PR-1: scaffold report-only

> **Note (renumber 2026-05-05):** This plan was originally numbered `0060` but conflicted with `plans/0060-gar-503-cargo-bin-exe-cleanup.md` which was merged in parallel via `/garra-routine`. Renamed to 0064 (next free number after the 0061..0063 sequence on main) before merge.

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Linear issue:** TBD — abrir após PR mergeado (epic Quality Ratchet).

**Status:** ✅ Approved — 2026-05-05 (Florida). Aprovação registrada em `~/.claude/plans/voc-est-no-projeto-buzzing-volcano.md` após duas rodadas de ajustes solicitados por Michel.

**Goal:** Entregar o scaffold do AI Quality Ratchet em PR pequeno e conservador, **sem bloquear nenhum PR**, **sem tocar código de produção (`.rs` em `crates/`)**, e validando arquitetura para evolução em PR-2/3/4 futuros.

**Architecture:**

1. `.quality/` — diretório versionado com `baseline.json` (números reais capturados nesta branch), `README.md` (filosofia + comandos), `thresholds.toml` (`max_file_lines = 3500`, `tolerance_pct = 0.1`, `audit_high_max = 0`).
2. `scripts/quality/` — orquestração shell + parsing Python:
   - `collect-metrics.sh` (fast mode default; `--full` opt-in para uso local)
   - `rust-file-stats.sh` (file size stats via `git ls-files` + `wc`)
   - `compare.py` com flag explícita `--mode report-only|enforce`
   - `freeze-baseline.py` (gera `.quality/baseline.proposed.json`, NUNCA commita)
   - `parse-llvm-cov.py`, `parse-cargo-audit.py`, `parse-clippy.py` (Python parsers fixture-tested)
   - `tests/` com pytest fixtures
3. `.github/workflows/quality-ratchet.yml` — trigger `pull_request` puro, chama `compare.py --mode report-only`, posta `quality-report.md` como comentário PR com marker `<!-- quality-ratchet-comment -->`. **Sem `continue-on-error`**, sem `workflow_run`.
4. `.claude/commands/quality-babysit.md` — skill **documentada** com auto-loop N=5 + 12 guardrails. Em PR-1 modo é `manual-only` (lê e propõe, não commita).
5. `CODEOWNERS` (criar em `.github/CODEOWNERS`) — entry `/.quality/baseline.json @michelbr84` como camada inicial de visibilidade.
6. `CLAUDE.md` ganha §"Quality Ratchet"; `ROADMAP.md` ganha entrada para epic; `plans/README.md` ganha row 0064.

**Tech stack:** Bash 4+ (com `set -euo pipefail`), Python 3.x via `py` (Windows) ou `python3` (Linux CI), `jq`, pytest, GitHub Actions standard runners (`ubuntu-latest` em PR-1).

---

## Design invariants (não-negociáveis nesta slice)

1. **Zero arquivos `.rs` de produção tocados.** Validação: `git diff main...HEAD -- 'crates/**/*.rs' | wc -l == 0`.
2. **Zero `continue-on-error: true` introduzidos.** Modo report-only é controlado pela flag `--mode report-only` no `compare.py`, não pelo `continue-on-error` (Michel ajuste #1).
3. **Sem `duplication_pct` no schema PR-1.** Duplicação é PR-3 (Michel ajuste #2). Schema `current-metrics.json` e `baseline.json` em PR-1 NÃO contém esse campo.
4. **Trigger `pull_request` puro, não `workflow_run`** (Michel ajuste #3). Coverage entra como best-effort: se `lcov.info` estiver disponível no working dir, parseia; senão marca `coverage_status = "not_collected_this_run"`.
5. **`collect-metrics.sh` default = fast mode** (Michel ajuste #4). `--full` é opt-in para uso local.
6. **CODEOWNERS é camada inicial / visibilidade**, não enforcement (Michel ajuste #5). Branch protection update fica para PR-4 com aprovação explícita de Michel.
7. **`freeze-baseline.py` NUNCA commita.** Sempre escreve `.quality/baseline.proposed.json` para review humano.
8. **`compare.py` é determinístico.** Mesma entrada → mesma saída. Sem chamada de IA, sem network.

---

## Validações pré-plano (executadas neste sessão)

- ✅ Diff PR #131 verificado +19/-0 antes do merge — escopo limpo.
- ✅ PR #131 mergeado (`a19e5f8`) com CI 17/17 verde após `gh pr update-branch`.
- ✅ Working tree limpo: só `AI Quality Gates/` untracked (que **NÃO entra no PR-1** — fora do escopo).
- ✅ `.github/workflows/ci.yml` confirmado: 0 `continue-on-error` ativos (memória estava desatualizada).
- ✅ `coverage` job pattern lido: artifact `coverage-lcov-${{ github.run_id }}` + comment marker `<!-- coverage-comment -->` + fork-PR guard `head.repo.full_name == github.repository`.
- ✅ Arquivos `.rs` mapeados: 334 total, 33 >700, 7 >1500, max 3240 (`admin/handlers.rs`).
- ✅ `clippy.toml`/`rustfmt.toml`/`deny.toml`/`.gitleaks.toml` mapeados.
- ✅ Sem CODEOWNERS pré-existente (vai ser criado).
- ✅ Plans dir: 0001..0063 ocupados (com gap em 0062); 0064 disponível. (Nota: o número 0060 já foi tomado por GAR-503/cargo-bin-exe-cleanup que mergeou em paralelo via `/garra-routine`.)

---

## Scope

**Inclui:**

- Diretório `.quality/` (3 arquivos)
- Diretório `scripts/quality/` (~7 scripts + tests)
- 1 workflow novo (`quality-ratchet.yml`)
- 1 slash-command novo (`.claude/commands/quality-babysit.md`)
- 1 arquivo `.github/CODEOWNERS` (criar)
- 3 arquivos editados em modo append/section: `CLAUDE.md`, `ROADMAP.md`, `plans/README.md`
- 1 arquivo de plan: este (`plans/0064-quality-ratchet-pr1.md`)

**Out of scope** (PRs futuros):

- Promoção a bloqueante (PR-4): trocar `--mode report-only` por `--mode enforce` + branch protection update.
- Detecção de duplicação (PR-3): `parse-jscpd.py` + step `npx jscpd@4 --format rust`.
- Auto-loop real do `/quality-babysit` (PR-2): em PR-1 é apenas documentado / manual-only.
- Mutation score gate (PR-N).
- Complexidade ciclomática (PR-N).
- LOC delta (PR-N).
- `cargo-semver-checks` (PR-N futuro do epic).
- Tocar branch protection (sempre com aprovação explícita de Michel).
- Tocar arquivos `.rs` de produção.

---

## Acceptance criteria

- [ ] Branch `chore/quality-ratchet-pr1-scaffold` criada off main, off de `a19e5f8`.
- [ ] Plan `plans/0064-quality-ratchet-pr1.md` versionado (este arquivo).
- [ ] `.quality/baseline.json` capturado a partir do estado real desta branch (não hardcoded).
- [ ] `bash scripts/quality/collect-metrics.sh > /tmp/m.json` roda em <2 min em fast mode.
- [ ] `python3 scripts/quality/compare.py --mode report-only .quality/baseline.json /tmp/m.json` exit 0 + escreve `quality-report.md`.
- [ ] `python3 scripts/quality/compare.py --mode enforce .quality/baseline.json /tmp/m.json` exit 0 quando current = baseline; exit 1 em regressão fabricada.
- [ ] `pytest scripts/quality/tests/` todos verdes.
- [ ] Workflow `quality-ratchet.yml` roda na próprio PR, posta comentário com marker `<!-- quality-ratchet-comment -->`, sem introduzir `continue-on-error`.
- [ ] CI inteiro verde (workflow novo é report-only via flag, então não falha).
- [ ] `git diff main...HEAD -- 'crates/**/*.rs' | wc -l` = 0.
- [ ] `git diff main...HEAD -- '.quality/**' 'scripts/quality/**' '.github/workflows/quality-ratchet.yml' '.claude/commands/quality-babysit.md' '.github/CODEOWNERS' 'CLAUDE.md' 'ROADMAP.md' 'plans/0064-quality-ratchet-pr1.md' 'plans/README.md'` cobre 100% do diff.
- [ ] PR aberto com checklist do PULL_REQUEST_TEMPLATE.md preenchido + seção explícita de "Como honra cada ajuste de Michel".

---

## Rollback plan

PR-1 é puramente aditivo. Se algo der errado:

1. Revert do squash commit no `main` (single revert, idempotent).
2. Workflow `quality-ratchet.yml` desliga sozinho ao ser deletado (não há job dependente).
3. CODEOWNERS é o único arquivo de config sensível; revert remove o entry sem outros impactos (já que enforcement não foi ligado).
4. Sem migrations, sem mudanças de schema DB, sem mudanças de API.

Tempo de rollback: <2 min.

---

## §12 Open questions

(nenhuma — decisões fechadas após duas rodadas de ajustes com Michel em 2026-05-05)

---

## File structure

```text
.quality/
  baseline.json
  README.md
  thresholds.toml

scripts/quality/
  collect-metrics.sh
  rust-file-stats.sh
  compare.py
  freeze-baseline.py
  parse-llvm-cov.py
  parse-cargo-audit.py
  parse-clippy.py
  tests/
    __init__.py
    fixtures/
      lcov.info
      cargo-audit.json
      clippy.json
    test_parse_llvm_cov.py
    test_parse_cargo_audit.py
    test_parse_clippy.py
    test_compare.py

.github/
  CODEOWNERS                 (NEW)
  workflows/
    quality-ratchet.yml      (NEW)

.claude/commands/
  quality-babysit.md         (NEW)

CLAUDE.md                    (edited — append §Quality Ratchet)
ROADMAP.md                   (edited — append entry para epic Quality Ratchet)
plans/README.md              (edited — add row 0064)
plans/0064-quality-ratchet-pr1.md  (THIS FILE)
```

---

## Risk register

| Risco | Mitigação |
|---|---|
| `lcov.info` não disponível em fast mode → coverage marcada como `not_collected` | Esperado: best-effort. Documentado no `quality-report.md`. |
| `parse-clippy.py` não roda em fast mode (clippy é caro) | `--full` é opt-in. CI já roda clippy hard-blocking em `ci.yml`. Em PR-1 a métrica `clippy_warnings` fica como `not_collected_this_run` em fast mode. |
| Race entre baseline initial e PRs concorrentes mergeando em main | Baseline capturado AGORA off `a19e5f8` (HEAD desta branch). Se outro PR mergear primeiro, esta branch fica BEHIND e `gh pr update-branch` é o playbook. |
| Workflow novo falha por `actions/upload-artifact@v7` indisponível | Repo já usa v7 em ci.yml (verificado linha 287). |
| `gh pr comment` falha em fork PR | Guard `head.repo.full_name == github.repository` (mesmo pattern do coverage job). |
| Python 3 não disponível em ubuntu-latest | É padrão. Workflow usa `actions/setup-python@v6` por defesa em depth. |
| `jq` não disponível | Padrão em `ubuntu-latest`. Local Windows: pode estar ausente — `collect-metrics.sh` deve falhar com mensagem clara. |
| baseline `_doc` field comido por jq strict mode | Schema explícito + comentários inline em `.quality/README.md`. |

---

## Acceptance pattern (verification end-to-end)

Conforme §10 do plan-mãe (`voc-est-no-projeto-buzzing-volcano.md`):

```bash
bash scripts/quality/collect-metrics.sh > current-metrics.json
jq -e '.max_file_lines, .files_over_700, .files_over_1500, .audit, .coverage' current-metrics.json
python3 scripts/quality/compare.py --mode report-only .quality/baseline.json current-metrics.json && test $? -eq 0
python3 scripts/quality/compare.py --mode enforce .quality/baseline.json current-metrics.json && test $? -eq 0
python3 -c "import json; d=json.load(open('current-metrics.json')); d['max_file_lines'] += 500; json.dump(d, open('regressed.json','w'))"
python3 scripts/quality/compare.py --mode enforce .quality/baseline.json regressed.json; test $? -eq 1
python3 scripts/quality/compare.py --mode report-only .quality/baseline.json regressed.json; test $? -eq 0
grep -q "Regressões Detectadas" quality-report.md
pytest scripts/quality/tests/
time bash scripts/quality/collect-metrics.sh > /dev/null  # <2min
```

---

## Cross-references

- Plan-mãe (decisões + ajustes): `~/.claude/plans/voc-est-no-projeto-buzzing-volcano.md`
- ADR Quality Ratchet (a criar em PR-3 ou PR-4 com decisão final de duplicação): TBD
- Kit referência (não copiado): `AI Quality Gates/` (untracked, fora do PR)
- CLAUDE.md §"Quality Ratchet" (a ser criado neste PR)

---

## Estimativa

- Plan: 1h (este arquivo)
- Parsers + pytest: 2h
- Shell orchestrators: 1h
- compare.py + freeze: 1.5h
- Workflow + skill + CODEOWNERS: 1h
- Verificação local + push + PR: 1h
- **Total: ~7.5h** de trabalho focado.

LOC alvo PR-1: ~600 (scripts + workflow + docs), bem abaixo do limite de 800 do briefing.
