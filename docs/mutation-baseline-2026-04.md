# Mutation Testing Baseline — `garraia-auth` (April 2026)

> **Atualizado 2026-04-29** — pós-Q6.1 (PR #92) + Q6.7 (PR #93). Score subiu de **85.04% → 90.78%**. Ver §"Atualização — Run 25116031135" no fim deste documento.

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

---

## Atualização — Run 25116031135 (2026-04-29)

Primeira execução **completa** após PR #92 (Q6.1 — kill 5 critical bypasses) e PR #93 (Q6.7 — timeout 90→150 min). O run anterior (`25109221846`) cancelou em 90 min com 154 de 179 mutants processados; este completou em 2h12m49s, dentro do novo limite de 150 min.

### Metadata

| Campo | Valor |
|---|---|
| Run ID | `25116031135` |
| Workflow | `Mutation Testing — garraia-auth (pilot)` (`mutants.yml`) |
| Trigger | `workflow_dispatch` (post-merge de PR #93) |
| Commit base | `de8bee5` (`main` após Q6.1+Q6.7) |
| `cargo-mutants` versão | `25.x` |
| Timeout do workflow | **150 min** (era 90; bump aplicado por PR #93) |
| Duração efetiva | **2h12m49s** (133 min wall-clock — cabe em 150 com folga de ~12%) |
| Conclusão | `failure` (exit 3) — **comportamento esperado** quando há `MISSED`/`TIMEOUT`. NÃO é `cancelled` por timeout do workflow. |
| Artifact | `mutants-report-25116031135` (2.1 MB, completo) |

### Score

```
total_mutants:    179
caught:           125  (+19 vs baseline 25072579785 — Q6.1 + crescimento natural)
missed:            13  (-6 vs baseline)
timeout:            3  (+1 vs baseline)
unviable:          38  (+5 vs baseline — proporcional ao crescimento de código)
```

**Cálculo (mantendo a convenção do baseline original):**

- Mutantes viáveis: `179 - 38 (unviable) = 141`
- Killed (caught + timeout): `125 + 3 = 128`
- **Score: `128 / 141 = 90.78%`** (vs baseline **85.04%** → **+5.74 p.p.**)

### Comparativo

| Métrica | Run 25072579785 (baseline) | Run 25116031135 (atual) | Δ |
|---|---|---|---|
| Total mutants generated | 160 | 179 | +19 |
| Caught | 106 | **125** | +19 |
| Missed | 19 | **13** | **−6** |
| Timeout | 2 | 3 | +1 |
| Unviable | 33 | 38 | +5 |
| Mutantes viáveis | 127 | 141 | +14 |
| **Killed score** | **85.04%** | **90.78%** | **+5.74 p.p.** |
| Workflow conclusion | `cancelled` (timeout) | `failure` (esperado: há MISSED) | conclusion-correta agora |
| Duração wall-clock | 1h30m20s (cancelled) | 2h12m49s (completo) | +42m29s |

### Q6.1 confirmação final (5/5 CAUGHT)

Cross-check empírico via `caught.txt` do artifact:

| Q6.1 target | Status | Variants extras |
|---|---|---|
| `hashing.rs:81 verify_pbkdf2 → Ok(true)` | ✅ CAUGHT | + `Ok(false)` também CAUGHT (bonus) |
| `hashing.rs:107 consume_dummy_hash → Ok(())` | ✅ CAUGHT | timing assertion (≥ 8 ms) |
| `internal.rs:286 verify_credential → Ok(None)` | ✅ CAUGHT | + `Ok(Some(Default))` CAUGHT |
| `sessions.rs:115 verify_refresh → Ok(None)` | ✅ CAUGHT | `Ok(Some((Default,Default)))` é UNVIABLE (Uuid Default não compila) |
| `sessions.rs:158 revoke → Ok(())` | ✅ CAUGHT | observação direta de `revoked_at` via admin pool |

**Bônus**: 3 mutantes em `sessions.rs` (linhas 81, 136, 147 — TTL/time arithmetic) também CAUGHT pelos integration tests novos do `sessions_lifecycle.rs`. Esses pertenciam ao escopo de Q6.3 mas caíram com a infra de Q6.1.

### Q6.7 confirmação final

Workflow concluiu em **133 min** dentro do novo limite de **150 min**. Headroom efetivo: ~17 min (~13%). Decisão de 150 (em vez de 120 ou 180) validada empiricamente.

### 13 mutantes restantes (`missed`) — mapeamento atualizado para sub-issues

| # | Arquivo | Linha | Mutação | Sub-issue Q6.x |
|---|---|---|---|---|
| 1 | `audit_workspace.rs` | 156 | `audit_workspace_event` → `Ok(())` | GAR-467 (Q6.5 audit observability) |
| 2 | `internal.rs` | 430 | `is_unique_violation` → `true` | GAR-466 (Q6.4 unique-violation) |
| 3 | `jwt.rs` | 33 | `*` → `+` | GAR-465 (Q6.3 TTL arithmetic) |
| 4 | `jwt.rs` | 33 | `*` → `/` | GAR-465 (Q6.3) |
| 5 | `jwt.rs` | 63 | `Debug for JwtConfig` → `Ok(Default)` | GAR-468 (Q6.6 Debug skip) |
| 6 | `jwt.rs` | 81 | `Debug for RefreshTokenPair` → `Ok(Default)` | GAR-468 (Q6.6) |
| 7 | `jwt.rs` | 179 | `<` → `<=` em `JwtIssuer::new_for_test` | GAR-464 (Q6.2 boundary) |
| 8 | `jwt.rs` | 248 | `<` → `<=` em `extract_bearer_token` | GAR-464 (Q6.2) |
| 9 | `types.rs` | 47 | `Debug for Credential` → `Ok(Default)` | GAR-468 (Q6.6) |
| 10 | `signup_pool.rs` | 153 | `Debug for SignupPool` → `Ok(Default)` | GAR-468 (Q6.6) |
| 11 | `storage_redacted.rs` | 71 | `<` → `<=` em `redact_urls` | GAR-464 (Q6.2) |
| 12 | **`app_pool.rs`** | **203** | `!=` → `==` em `AppPool::from_dedicated_config` | GAR-464 (Q6.2) — **NOVO** |
| 13 | **`app_pool.rs`** | **218** | `Debug for AppPool` → `Ok(Default)` | GAR-468 (Q6.6) — **NOVO** |

**Distribuição**: Q6.6 = 5 (era 4 + 1 novo), Q6.2 = 4 (era 3 + 1 novo + 1 herdado), Q6.3 = 2, Q6.4 = 1, Q6.5 = 1.

> **Mudança vs baseline**: 8 mutantes do baseline original foram mortos (5 Q6.1 + 3 sessions bonus); 2 mutantes novos apareceram (`app_pool.rs:203,218` — código adicionado pós-baseline). Net: **−6 missed**.

### 3 mutantes timeout (`timeout`)

| # | Arquivo | Linha | Mutação |
|---|---|---|---|
| 1 | `storage_redacted.rs` | 91 | `+=` → `-=` em `redact_urls` |
| 2 | `storage_redacted.rs` | 91 | `+=` → `*=` em `redact_urls` |
| 3 | **`storage_redacted.rs`** | **104** | `+=` → `*=` em `redact_urls` (**novo no run completo**) |

Tratados como **killed** no score (mutated code didn't terminate within timeout — implícito kill). Tendência: regex/string redaction em loops com aritmética mutada → loops infinitos. Investigação opcional em follow-up; não bloqueia.

### Status das sub-issues Q6.x

| Sub-issue | State | Confirmação |
|---|---|---|
| GAR-463 (Q6.1 security bypass) | **Done** | 5/5 CAUGHT confirmado neste run |
| GAR-464 (Q6.2 boundary) | Backlog | 4 mutants no missed list |
| GAR-465 (Q6.3 TTL arithmetic) | Backlog | 2 mutants (3 já mortos como bônus de Q6.1) |
| GAR-466 (Q6.4 unique-violation) | Backlog | 1 mutant |
| GAR-467 (Q6.5 audit observability) | Backlog | 1 mutant |
| GAR-468 (Q6.6 Debug skip) | Backlog | **5 mutants** (mais provável "vitória rápida") |
| GAR-469 (Q6.7 timeout 90→150) | **Done** | 150 min validado empiricamente |
| GAR-481 (Q6.8 Node 24 migration) | Backlog | criada nesta sessão; deadline 2026-06-02 |

### Próximo melhor passo

Após este run, dois caminhos abertos com ROI distinto:

1. **GAR-481 (Node 24)** — High priority, deadline externa firme (2026-06-02 = 34 dias). Não fecha mutants, mas evita CI quebrar.
2. **GAR-468 (Q6.6 Debug skip)** — Low priority no Linear, mas é a vitória mais rápida em mutation score: 5 mutants podem virar killed com `// mutants: skip` annotations + rationale, ou via 1 unit test de redação cobrindo todos os 5 `impl Debug`. Score subiria a `133/141 = 94.33%`.

Recomenda-se atacar GAR-481 primeiro (deadline) e GAR-468 logo em seguida.

### Referências adicionais (este update)

- PR #92 (Q6.1): https://github.com/michelbr84/GarraRUST/pull/92 → merged em `a13517c`
- PR #93 (Q6.7): https://github.com/michelbr84/GarraRUST/pull/93 → merged em `de8bee5`
- Run anterior (cancelled): https://github.com/michelbr84/GarraRUST/actions/runs/25109221846
- Run atual (completo): https://github.com/michelbr84/GarraRUST/actions/runs/25116031135
- Linear Q6.1..Q6.8: GAR-463, GAR-464, GAR-465, GAR-466, GAR-467, GAR-468, GAR-469, GAR-481

---

## Atualização — Run 25307117776 (GAR-505, 2026-05-04)

Triage dos 6 missed novos + 3 timeouts em `garraia-auth`. Esta seção precede o
merge do PR de GAR-505 e documenta o estado pré-PR (run scheduled de segunda
05:00 UTC, ficou vermelho) e o estado esperado pós-PR.

### Metadata

| Campo | Valor |
|---|---|
| Run ID | `25307117776` |
| Workflow | `Mutation Testing — garraia-auth (pilot)` (`mutants.yml`) |
| Trigger | `schedule` (cron Monday 05:00 UTC) |
| Commit base | `5c63a162` (`main`, após PR #118) |
| Duração | 2h05m29s (dentro do limite de 150 min) |
| Conclusão | `failure` (esperado — havia missed/timeouts) |

### Score (pré-PR)

```
total_mutants:    179
caught:           128  (vs 125 em 25116031135 — +3 por crescimento de testes existente)
missed:            10  (-3 vs 13)
timeout:            3  (=)
unviable:          38  (=)
```

- Mutantes viáveis: `179 - 38 = 141`
- Killed (caught + timeout): `128 + 3 = 131`
- **Score: `131 / 141 = 92.91%`** (vs `90.78%` → **+2.13 p.p.**)

### Triage GAR-505 (5 caught + 4 skip via `mutants::skip`)

| # | File:line | Mutação | Resolução |
|---|---|---|---|
| 1 | `jwt.rs:31` | `*` → `+` em `ACCESS_TTL_SECS` | killed por `access_token_ttl_window_is_900_seconds` |
| 2 | `jwt.rs:31` | `*` → `/` em `ACCESS_TTL_SECS` | killed pelo mesmo teste |
| 3 | `jwt.rs:177` | `<` → `<=` em `JwtIssuer::new_for_test` | killed por `new_for_test_does_not_pad_already_32_byte_secret` (HMAC oracle) |
| 4 | `jwt.rs:250` | `<` → `<=` em `extract_bearer_token` | killed por `extract_bearer_token_accepts_seven_char_boundary` |
| 5 | `storage_redacted.rs:71` | `<` → `<=` em `redact_urls` | **equivalente** — `#[cfg_attr(any(), mutants::skip)]` em `redact_urls` |
| 6 | `app_pool.rs:203` | `!=` → `==` em `AppPool::from_dedicated_config` | killed por integration test `app_pool_role_guard.rs::from_dedicated_config_rejects_non_app_role` |
| T1 | `storage_redacted.rs:91` | `+=` → `-=` em `redact_urls` | **timeout** (underflow `usize` ou loop) — coberto pelo mesmo skip da função |
| T2 | `storage_redacted.rs:91` | `+=` → `*=` em `redact_urls` | **timeout** (`i = 0` estagnado em URL no offset 0) — coberto pelo mesmo skip |
| T3 | `storage_redacted.rs:104` | `+=` → `*=` em `redact_urls` | **timeout** (`i *= 1` estagnado em ASCII) — coberto pelo mesmo skip |

### Score esperado pós-PR

| Métrica | Pré-PR (`25307117776`) | Pós-PR (estimado) | Δ |
|---|---|---|---|
| `total_mutants` | 179 | ~179 (idem; novos tests não geram mutants extras significativos) | ~0 |
| `caught` | 128 | **133** (+5 dos novos tests) | +5 |
| `missed` | 10 | **4** (sites #1..#4 + #6 mortos; site #5 sai como skipped) | −6 |
| `timeout` | 3 | **0** (T1..T3 saem como skipped) | −3 |
| `unviable` | 38 | 38 | 0 |
| `skipped` | 0 | **4** (mutants em `redact_urls` cobertos pelo `cfg_attr(any(), mutants::skip)`) | +4 |
| **Mutantes viáveis** | 141 | **137** (`179 - 38 - 4`) | −4 |
| **Killed (caught+timeout)** | 131 | **133** | +2 |
| **Score** | **92.91%** | **97.08%** (`133 / 137`) | **+4.17 p.p.** |
| Workflow | `failure` | `failure` ainda esperado se ≥1 dos 4 missed remanescentes não estiver resolvido em outras issues | — |

> Os 4 missed remanescentes pertencem a GAR-464/467/483/468 e estão fora do escopo de GAR-505:
> `audit_workspace.rs:156` (GAR-467), `internal.rs:430` (GAR-466 ou GAR-464), `signup_pool.rs:153`
> (GAR-483, Debug), `app_pool.rs:218` (GAR-483, Debug). O workflow só ficará completamente
> verde quando essas issues fecharem ou quando os mutantes correspondentes forem reclassificados
> em PRs subsequentes.

### Decisão técnica — `redact_urls` skip

`cargo-mutants` 25.x não suporta skip por linha/coluna nem inline:

- `cargo-mutants.toml`: aceita só `exclude_globs` / `examine_globs` (file-path) — confirmado em
  https://mutants.rs/skip_files.html.
- Atributos: sempre function-level (`#[mutants::skip]` ou qualquer attr contendo essa
  string) — confirmado em https://mutants.rs/attrs.html ("it only looks for the
  sequence `mutants::skip` in the attribute").
- Não existe `// mutants: skip` line comment.

As três alternativas avaliadas:

- **A — function-level skip em `redact_urls`** (escolhida): perde mutation signal dentro de
  uma função. Compensação: 7 unit tests no `mod tests` (linhas 156-242) cobrem `redact()`
  end-to-end com inputs realistas (URL com porta/path, `postgresql://`, multi-key, passthrough,
  source chain).
- **B — só documentar, sem skip**: deixa workflow vermelho perpetuamente por causa do site #5
  (mutante equivalente — não há teste possível que o distingua). **Recusada.**
- **C — `exclude_globs` no arquivo**: perderia signal de `redact()` e `redact_key_value()`
  que estão saudáveis. **Recusada.**

### Padrão `cfg_attr(any(), mutants::skip)` — justificativa

`#[cfg_attr(any(), mutants::skip)]` evita adicionar a crate `mutants` (`0.0.3`) como
dependência. cargo-mutants greps o source pela string literal `mutants::skip` antes da
compilação; `cfg_attr(any(), …)` nunca é expandido pelo rustc porque `any()` é vazio
(= false), então o atributo `mutants::skip` interno nunca chega ao type-checker. Resultado:
cargo-mutants reconhece o skip; rustc compila sem dep adicional.

### Não-silenciamento — invariantes preservadas

- `.github/workflows/mutants.yml` permanece **inalterado** em GAR-505 (zero diff).
- `continue-on-error: true` continua **ausente** em todo o workflow.
- O skip é classificação explícita, não supressão de erro.
- Cada um dos 4 mutantes cobertos pelo skip tem prova técnica linha-a-linha no comentário
  acima de `redact_urls` (storage_redacted.rs).

### Status atualizado das sub-issues Q6.x

| Sub-issue | State (pós-GAR-505) | Mutantes restantes |
|---|---|---|
| GAR-463 (Q6.1 security bypass) | Done | 0 |
| GAR-464 (Q6.2 boundary) | Backlog | 0 ou 1 (depende do mapeamento de `internal.rs:430`) |
| GAR-465 (Q6.3 TTL arithmetic) | Done (incorporado em GAR-505 #1+#2) | 0 |
| GAR-466 (Q6.4 unique-violation) | Backlog | 1 (`internal.rs:430`) |
| GAR-467 (Q6.5 audit observability) | Backlog | 1 (`audit_workspace.rs:156`) |
| GAR-468 (Q6.6 Debug skip) | Done para `JwtConfig`/`RefreshTokenPair`/`Credential` | 2 (alocados a GAR-483) |
| GAR-469 (Q6.7 timeout 90→150) | Done | 0 |
| GAR-481 (Q6.8 Node 24) | Backlog | — (não relacionado a mutants) |
| GAR-483 (Debug skip pendentes) | Backlog | 2 (`signup_pool.rs:153`, `app_pool.rs:218`) |
| **GAR-505** (este triage) | **em PR** | 0 dentro do escopo |

### Referências adicionais (GAR-505)

- Run vermelho: https://github.com/michelbr84/GarraRUST/actions/runs/25307117776
- PR (a preencher após `gh pr create`): https://github.com/michelbr84/GarraRUST/pull/XX
- Linear: https://linear.app/chatgpt25/issue/GAR-505
- cargo-mutants attrs spec: https://mutants.rs/attrs.html
- cargo-mutants config spec: https://mutants.rs/skip_files.html
