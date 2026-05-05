# `.quality/` — AI Quality Ratchet baseline

Sistema de Quality Gates inspirado no padrão **Catraca (Ratchet)**: as métricas de qualidade do GarraRUST só podem **melhorar ou manter**. Nunca regredir.

> **Status atual: PR-1 — report-only.** Nenhum PR é bloqueado pelo Quality Ratchet ainda. O workflow `.github/workflows/quality-ratchet.yml` apenas observa, posta `quality-report.md` como comentário no PR e segue. Promoção a bloqueante (`--mode enforce`) entra em PR-4 com aprovação explícita de Michel.

## Filosofia

1. **Catraca**: métricas só sobem ou ficam.
2. **Determinismo**: `compare.py` é pura comparação de dados — mesma entrada, mesma saída, sem chamada de IA.
3. **Anti-fraude**: baseline só atualiza com aprovação manual (CODEOWNERS lock + revisão humana via `.quality/baseline.proposed.json`). Vide `plans/0064-quality-ratchet-pr1.md` §"Design invariants".
4. **Conservador**: PR-1 é puramente report-only. Bloqueante vem só depois de validação por 2+ semanas.

## Arquivos versionados

| Arquivo | Papel | Quem atualiza |
|---|---|---|
| `baseline.json` | Métricas congeladas — fonte de verdade do "estado atual aceito" | Apenas Michel via `freeze-baseline.py --seed` (1ª vez) ou via review humano de `.quality/baseline.proposed.json` |
| `README.md` | Este arquivo — explica filosofia e comandos | Documentação |
| `thresholds.toml` | Limites hardcoded (ex.: `max_file_lines`) | Documentação + reuso futuro |

## Arquivos ephemeral (gitignored)

| Arquivo | Papel |
|---|---|
| `baseline.proposed.json` | Saída de `freeze-baseline.py`. Review humano antes de virar `baseline.json`. **NUNCA commitar diretamente.** |
| `current-metrics.json` | Métricas instantâneas coletadas por `collect-metrics.sh`. Sempre regenerável. |
| `quality-report.md` | Relatório legível pelo Claude. Sempre regenerável. |

## Comandos

```bash
# Coleta rápida (default — sob 10 s):
bash scripts/quality/collect-metrics.sh > current-metrics.json

# Coleta completa (lenta — re-roda clippy):
bash scripts/quality/collect-metrics.sh --full > current-metrics.json

# Comparar contra baseline (report-only — sempre exit 0):
python3 scripts/quality/compare.py --mode report-only \
    .quality/baseline.json current-metrics.json

# Comparar (enforce — exit 1 em regressão; uso de PR-4 em diante):
python3 scripts/quality/compare.py --mode enforce \
    .quality/baseline.json current-metrics.json

# Propor novo baseline (gera .proposed.json — NÃO commita automaticamente):
python3 scripts/quality/freeze-baseline.py current-metrics.json

# Rodar testes dos parsers:
python3 -m pytest scripts/quality/tests/
```

## Métricas trackedas (PR-1)

| Métrica | Fonte | Tipo |
|---|---|---|
| `max_file_lines` | `git ls-files '*.rs' \| wc -l` | hard-track (smaller is better) |
| `files_over_700` | idem, contando >700 | trend (smaller is better) |
| `files_over_1500` | idem, contando >1500 | trend |
| `files_over_2500` | idem, contando >2500 | trend |
| `coverage.coverage_pct` | `lcov.info` (best-effort) | bigger is better |
| `audit.critical` | `cargo audit --json` | absolute zero |
| `audit.high` | idem | smaller is better |
| `clippy.clippy_warnings` | `cargo clippy --message-format=json` (modo `--full`) | smaller is better |

## Métricas adiadas

| Métrica | Quando entra |
|---|---|
| Duplicação de código (`jscpd --format rust`) | PR-3 (Michel ajuste #2) |
| Mutation score (`cargo-mutants`) | PR-N futuro |
| Complexidade ciclomática | PR-N futuro |
| LOC delta no PR | PR-N futuro |
| `cargo-semver-checks` | PR-N futuro (epic separado) |

## Anti-fraud (camadas)

1. **CODEOWNERS** (`.github/CODEOWNERS`) — `@michelbr84` é dono de `/.quality/baseline.json`. Em PR-1 é apenas visibilidade (não enforcement, per Michel ajuste #5). Enforcement real (branch protection ou job dedicado bloqueante) vira PR futuro.
2. **`freeze-baseline.py` NUNCA commita** — sempre escreve `.quality/baseline.proposed.json`, exigindo `mv` + `git add` manual.
3. **`compare.py --mode enforce`** (PR-4+) detecta regressão mesmo se commit tocar `baseline.json` — porque a comparação é vs git history do baseline.
4. **Job na main NÃO auto-avança baseline** (diferença explícita vs. kit JS/TS de referência).

## Cross-references

- Plan 0064 (este PR): `plans/0064-quality-ratchet-pr1.md`
- Plan-mãe (filosofia + decisões): `~/.claude/plans/voc-est-no-projeto-buzzing-volcano.md` (não versionado)
- Skill de auto-correção: `.claude/commands/quality-babysit.md`
- Workflow: `.github/workflows/quality-ratchet.yml`
- Kit JS/TS de referência (não copiado): `AI Quality Gates/` (untracked)
