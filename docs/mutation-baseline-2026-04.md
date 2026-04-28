# Mutation Testing Baseline — `garraia-auth` (April 2026)

## Visão geral

Baseline empírico do **cargo-mutants pilot em `garraia-auth`** (GAR-436 / Q6 do EPIC GAR-430 — Quality Gates Phase 3.6), produzido pela **primeira execução real** do workflow `.github/workflows/mutants.yml`.

## Metadata do run

| Campo | Valor |
|---|---|
| Run ID | `25072579785` |
| Workflow | `Mutation Testing — garraia-auth (pilot)` (`mutants.yml`) |
| Trigger | `workflow_dispatch` (manual) — sessão `abundant-thimble` |
| Commit base | `321a609` (`main` antes dos PRs #89/#90) |
| `cargo-mutants` versão | `25.3.1` |
| Runner | `ubuntu-latest` (hostname `runnervmhkfpo`) |
| Início | 2026-04-28T19:14:10Z |
| Fim do análise | ≈2026-04-28T20:43:23Z |
| Duração | 1h30m20s |
| Conclusão | `cancelled` (workflow timeout-minutes 90 — análise completou pouco antes do hard-stop; **artifact final foi parcialmente truncado**, mas `outcomes.json` é íntegro com 160/160 mutantes processados) |
| Artifact | `mutants-report-25072579785` (1.63 MB) |

> **Nota sobre conclusion=cancelled**: o cargo-mutants 25.3.1 completou os 160 mutantes (verificável em `outcomes.json` com `total_mutants: 160` + 161 outcomes = baseline + 160 mutantes). A flag `cancelled` veio do workflow GHA atingindo o limite `timeout-minutes: 90`, provavelmente durante post-processing/cache step. O upload step tinha `if: always()` então o artifact foi capturado integralmente. **Próxima execução**: bumpar `timeout-minutes` para 120 ou paralelizar via `--jobs N` no workflow para buffer extra.

## Score

```
total_mutants:    160
outcomes:         161  (160 mutantes + 1 baseline run)
caught:           106  (test killed mutant) ← desejado
missed:            19  (test passed despite mutation) ← test gap
timeout:            2  (test hung — implícito kill, mas suspeito)
unviable:          33  (mutant não compila — ignorado)
success:            1  (baseline unmutated — sanity check)
```

### Cálculo

- **Mutantes viáveis testados**: `160 - 33 (unviable) = 127`
- **Killed (caught + timeout)**: `106 + 2 = 108`
- **Missed**: `19`
- **Mutation score**: `108 / 127 = 85.04%`

### Comparação com alvo

| Alvo | Resultado |
|---|---|
| ≥ 65% (alvo inicial GAR-436) | **85% PASSED** com folga de 20 pontos percentuais |
| ≥ 75% (alvo intermediário) | **PASSED** |
| ≥ 90% (alvo aspiracional, futuro) | **NÃO atingido** — gap de ~5 pontos |

## Mutantes escapados (`missed`) — 19 instâncias

Esses mutantes alteraram a lógica do código mas o suite de testes **não detectou**. Cada um é um gap de cobertura comportamental que pode virar uma sub-issue Q6.X.

### Por arquivo

| Arquivo | Linha | Mutação |
|---|---|---|
| `audit_workspace.rs` | 156 | `audit_workspace_event` → `Ok(())` |
| `hashing.rs` | 81 | `verify_pbkdf2` → `Ok(true)` |
| `hashing.rs` | 107 | `consume_dummy_hash` → `Ok(())` |
| `internal.rs` | 286 | `verify_credential` → `Ok(None)` |
| `internal.rs` | 430 | `is_unique_violation` → `true` |
| `jwt.rs` | 33 | `*` → `+` |
| `jwt.rs` | 33 | `*` → `/` |
| `jwt.rs` | 63 | `Debug for JwtConfig` → `Ok(Default::default())` |
| `jwt.rs` | 81 | `Debug for RefreshTokenPair` → `Ok(Default::default())` |
| `jwt.rs` | 179 | `<` → `<=` em `JwtIssuer::new_for_test` |
| `jwt.rs` | 248 | `<` → `<=` em `extract_bearer_token` |
| `sessions.rs` | 81 | `+` → `-` em `SessionStore::issue` |
| `sessions.rs` | 115 | `verify_refresh` → `Ok(None)` |
| `sessions.rs` | 136 | `==` → `!=` em `verify_refresh` |
| `sessions.rs` | 147 | `<=` → `>` em `verify_refresh` |
| `sessions.rs` | 158 | `revoke` → `Ok(())` |
| `types.rs` | 47 | `Debug for Credential` → `Ok(Default::default())` |
| `signup_pool.rs` | 153 | `Debug for SignupPool` → `Ok(Default::default())` |
| `storage_redacted.rs` | 71 | `<` → `<=` em `redact_urls` |

### Mutantes timeout (`timeout`) — 2 instâncias

Tests hung — tratados como killed para fins do score, mas merecem investigação porque indicam custo de teste anômalo:

| Arquivo | Linha | Mutação |
|---|---|---|
| `storage_redacted.rs` | 91 | `+=` → `-=` em `redact_urls` |
| `storage_redacted.rs` | 91 | `+=` → `*=` em `redact_urls` |

## Análise qualitativa

### Padrões nos missed mutants

1. **`Debug` impls (4 casos)** — `JwtConfig`, `RefreshTokenPair`, `Credential`, `SignupPool` — todos os 4 `impl Debug` foram mutados para `Ok(Default::default())` sem detecção. Isso é **intencional**: a redaction policy do projeto (CLAUDE.md regra 6 + `RedactedStorageError`) exige que `Debug` em tipos com PII seja **opaco**. Tests não testam o output literal de `Debug` justamente para não criar acoplamento com formato. **Decisão recomendada**: mover para `unviable` via `// mutants: skip` annotation, com nota explicativa.

2. **Off-by-one em comparações (4 casos)** — `<` → `<=` em `jwt.rs:179`, `jwt.rs:248`, `storage_redacted.rs:71`; `<=` → `>` em `sessions.rs:147`. Esses são **gaps reais** de boundary testing. Adicionar tests para min/max valores.

3. **Aritmética de tempo (3 casos)** — `*` → `+`/`/` em `jwt.rs:33` (TTL calc); `+` → `-` em `sessions.rs:81` (issue time). Tests não exercitam aritmética com timestamps reais — provavelmente usam mocks fixos. Adicionar property tests com `proptest` ou tests com TTL ranges.

4. **Bypass de verificação (5 casos)** — `verify_pbkdf2 → Ok(true)`, `consume_dummy_hash → Ok(())`, `verify_credential → Ok(None)`, `verify_refresh → Ok(None)`, `revoke → Ok(())`. Esses são **CRÍTICOS de segurança**: tests não distinguem "verificação passou" de "verificação foi bypassed". Isto é o achado mais importante deste run.

5. **`is_unique_violation` → `true` constante** — sempre interpretar erro como violation. Tests do path de error handling em duplicate-signup não exercitam o caminho NÃO-violation.

6. **`audit_workspace_event` → `Ok(())`** — eventos de audit silenciados. Tests não verificam que o audit_event foi de fato escrito.

### Achados materiais

**Findings críticos (security gates)**:
- `verify_pbkdf2 → Ok(true)` (hashing.rs:81)
- `consume_dummy_hash → Ok(())` (hashing.rs:107)
- `verify_credential → Ok(None)` (internal.rs:286)
- `verify_refresh → Ok(None)` (sessions.rs:115)
- `revoke → Ok(())` (sessions.rs:158)

Cada um significa que **o teste do happy path não detectaria a remoção total da verificação**. Sub-issue prioritária: Q6.1.

## Próximos passos (sub-issues a abrir)

Recomendação: abrir 1 sub-issue por categoria (não 1 por mutant — overhead alto):

| Sub-issue (proposta) | Escopo |
|---|---|
| Q6.1 — Mutation Testing — security verification bypass coverage | Tests para `verify_pbkdf2`, `consume_dummy_hash`, `verify_credential`, `verify_refresh`, `revoke` que distinguem true/false-result paths. Prioridade Urgente. |
| Q6.2 — Mutation Testing — boundary tests for comparison operators | Property tests para off-by-one em `jwt.rs:179,248`, `sessions.rs:147`, `storage_redacted.rs:71`. Prioridade High. |
| Q6.3 — Mutation Testing — TTL and time arithmetic coverage | Tests com TTL ranges reais para `jwt.rs:33`, `sessions.rs:81`. Prioridade Medium. |
| Q6.4 — Mutation Testing — unique-violation error path | Tests para o caminho NÃO-unique em `internal.rs:430`. Prioridade Medium. |
| Q6.5 — Mutation Testing — audit_event observability tests | Tests que verificam `audit_workspace_event` foi de fato escrito. Prioridade Medium. |
| Q6.6 — Mutation Testing — Debug impl skip annotations | Marcar 4 `impl Debug` mutations como `// mutants: skip` com justificativa de redaction policy. Prioridade Low (cosmético). |
| Q6.7 — Mutation Testing — workflow timeout bump | Bumpar `timeout-minutes: 90` → `120` e/ou adicionar `--jobs 2` para shard. Prioridade Low. |

## Workflow operacional

- **Schedule**: Monday 05:00 UTC (configurado em `mutants.yml`).
- **Trigger manual**: `gh workflow run mutants.yml --repo michelbr84/GarraRUST --ref main`.
- **Recuperar baseline**: `gh run list --workflow=mutants.yml`, depois `gh run download <id> --repo michelbr84/GarraRUST`.
- **Não bloqueia merge**: schedule + workflow_dispatch only, fora do PR path.

## Referências

- Linear [GAR-436](https://linear.app/chatgpt25/issue/GAR-436) (Done — workflow shipped).
- Linear [GAR-430 EPIC](https://linear.app/chatgpt25/issue/GAR-430) (parent).
- Workflow: `.github/workflows/mutants.yml`.
- Plan: `C:/Users/miche/.claude/plans/voc-est-no-reposit-rio-abundant-thimble.md` (sessão `abundant-thimble`).
- Run: https://github.com/michelbr84/GarraRUST/actions/runs/25072579785
- Comparativo de coverage line vs mutation: `docs/coverage-baseline-2026-04.md`.
