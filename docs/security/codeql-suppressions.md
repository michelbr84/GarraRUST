# CodeQL Suppressions Ledger

> **Convenção operacional**: Rust CodeQL ainda **não suporta** comentários inline
> `// codeql[rule]: justification` em 2026 (PR github/codeql#21638 aberto, sem
> merge). Este ledger é o mecanismo escolhido pelo projeto para registrar
> supressões — versionado em git, justificado por linha, auditável em PR review.
>
> Owner: GAR-491 (CodeQL Triage Wave 2). Plan: `personal-api-key-revogada-vectorized-matsumoto` §Step 4.
> GAR-490 (Wave 1) extension: PR [#111](https://github.com/michelbr84/GarraRUST/pull/111)
> (squash `613510d`) added 16 entries for `rust/path-injection` after empirical
> evidence that CodeQL's Rust pack does not model `validate_skill_name`
> (`crates/garraia-gateway/src/path_validation.rs`) as a sanitizer. Each
> entry references the helper guard line, the dismissed-as-FP rationale,
> and the integration test that pins the rejection — see §4 alerts #67-#82.
> Last updated: **2026-05-01**.
> Audit re-triage por: **2026-08-01** (entradas com mais de 90 dias devem ser
> revisitadas; alertas que não existem mais no Security tab devem ser removidos
> do ledger).

## §1. Background

GarraRUST adotou CodeQL advanced setup em 2026-04-30 (PR
[#106](https://github.com/michelbr84/GarraRUST/pull/106), runbook em
[`docs/security/codeql-setup.md`](codeql-setup.md)). O Security tab inicial
mostrou 90 alertas abertos. Esta sub-issue (GAR-491) endereça **6 alertas**
do rule `rust/hard-coded-cryptographic-value` que estão estruturalmente em
test fixtures ou em buffer initializers — não são vulnerabilidades reais.

Tres alternativas avaliadas para suprimi-los:

| Mecanismo | Por que NÃO | Por que NÃO |
|---|---|---|
| Inline `// codeql[...]` | ❌ não suportado em Rust | PR github/codeql#21638 aberto |
| `paths-ignore` em `codeql-config.yml` | ❌ silencia arquivo inteiro | testes do GarraRUST são INLINE (`#[cfg(test)] mod tests {}`) dentro de produção; ignorar `mobile_auth.rs` esconde alertas reais |
| `query-filters: exclude` por rule-id | ❌ silencia regra inteira | perde sinal de alertas reais futuros |

A solução adotada: **REST API dismissal + este ledger versionado** + script
de reaplicação (`scripts/security/codeql-reapply-dismissals.sh`).

## §2. Mechanism

Cada alerta dismissed via:

```bash
gh api -X PATCH repos/michelbr84/GarraRUST/code-scanning/alerts/<N> \
  -f state=dismissed \
  -f dismissed_reason="<used_in_tests|false_positive|won't_fix>" \
  -f dismissed_comment="GAR-491 — <justificativa>. See docs/security/codeql-suppressions.md row #<N>."
```

A fonte de verdade machine-readable é
[`docs/security/codeql-suppressions.json`](codeql-suppressions.json) (schema
version 1.0.0). O script consome o JSON; este `.md` é a versão humana auditável.
**Manter ambos sincronizados** — o script tem flag `--check-md` que valida que
os números de alerta listados em §4 batem com `entries[].alert_number` do JSON.

## §3. Operational rules

1. **No bulk suppression.** Cada entrada precisa justificativa por linha.
2. **No silencing real alerts as FPs.** Se em dúvida, NÃO suprime — abre
   sub-issue de investigação.
3. **Audit trail.** Cada dismissal emite `dismissed_comment` referenciando
   GAR-# **e** linha do ledger.
4. **Re-audit obrigatório a cada 90 dias.** Audit expiration: `2026-08-01`.
   Entradas vencidas devem ser revistas; se ainda válidas, renovar com nova
   justificativa + commit hash; se não, abrir fix real.
5. **Fail-closed reaplicação.** O script verifica `rule_id`/`path`/`line` do
   alerta atual contra o ledger antes de reaplicar. Se divergir → exit 2,
   manual re-audit obrigatório (alerta pode ter sido renumerado, código pode
   ter mudado, regra pode ter sido renomeada).
6. **Sem fallback global.** Se a empirical proof (§5) falhar, **NÃO**
   recorrer a `query-filters: exclude` global — abrir nova sub-issue para
   decidir entre custom query suite, path-specific approach, ou manual UI
   dismissal mantendo este ledger.

## §4. Ledger

| # | Rule | File:line | Disposition | Reason | Justificativa | GAR |
|---|------|-----------|-------------|--------|---------------|-----|
| <a id="alert-40"></a>[#40](https://github.com/michelbr84/GarraRUST/security/code-scanning/40) | `rust/hard-coded-cryptographic-value` | `crates/garraia-gateway/src/mobile_auth.rs:738` | dismissed-used-in-tests | `used_in_tests` | Test fixture em `#[tokio::test] argon2id_register_and_login_roundtrip`. Literal salt `""` é placeholder do path PHC Argon2id (que embute seu próprio salt); coluna legacy não-usada. | GAR-491 |
| <a id="alert-41"></a>[#41](https://github.com/michelbr84/GarraRUST/security/code-scanning/41) | `rust/hard-coded-cryptographic-value` | `crates/garraia-gateway/src/mobile_auth.rs:749` | dismissed-used-in-tests | `used_in_tests` | Test fixture em `#[tokio::test] argon2id_register_and_login_roundtrip` — branch negativo, password `"nope"` deve retornar false. Input intencionalmente inválido para coverage. | GAR-491 |
| <a id="alert-42"></a>[#42](https://github.com/michelbr84/GarraRUST/security/code-scanning/42) | `rust/hard-coded-cryptographic-value` | `crates/garraia-gateway/src/mobile_auth.rs:870` | dismissed-used-in-tests | `used_in_tests` | Test fixture em `#[tokio::test] second_login_after_upgrade_still_works`. `"seq-password-xyz"` exercita o PBKDF2 → Argon2id lazy-upgrade transactional path; nunca persistido. | GAR-491 |
| <a id="alert-43"></a>[#43](https://github.com/michelbr84/GarraRUST/security/code-scanning/43) | `rust/hard-coded-cryptographic-value` | `crates/garraia-security/src/credentials.rs:49` | dismissed-false-positive | `false_positive` | `vec![0u8; SALT_LEN]` é buffer initializer imediatamente sobrescrito por `ring::SystemRandom::fill` na linha 50. API do `ring` exige `&mut [u8]` como backing; literal `0u8` nunca vira salt real. **Anchor da empirical proof do mecanismo.** | GAR-491 |
| <a id="alert-44"></a>[#44](https://github.com/michelbr84/GarraRUST/security/code-scanning/44) | `rust/hard-coded-cryptographic-value` | `crates/garraia-security/src/validation.rs:233` | dismissed-used-in-tests | `used_in_tests` | Test fixture em `#[test] validate_password_length`. Literal `"short"` intencionalmente abaixo do mínimo para asserir `Err`. Negative-path coverage. | GAR-491 |
| <a id="alert-45"></a>[#45](https://github.com/michelbr84/GarraRUST/security/code-scanning/45) | `rust/hard-coded-cryptographic-value` | `crates/garraia-security/src/validation.rs:234` | dismissed-used-in-tests | `used_in_tests` | Test fixture em `#[test] validate_password_length`. Literal `"validpass123"` intencionalmente acima do mínimo para asserir `Ok`. Positive-path coverage. | GAR-491 |
| <a id="alert-67"></a>[#67](https://github.com/michelbr84/GarraRUST/security/code-scanning/67) | `rust/path-injection` | `crates/garraia-gateway/src/skins_handler.rs:84` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `create_skin` guards `body.name` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 60 before `tokio::fs::create_dir_all` / `tokio::fs::write`. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skins_test.rs::create_skin_with_path_traversal_returns_400` + `create_skin_with_dot_in_name_returns_400` + `create_skin_rejects_underscore_per_project_convention`. | GAR-490 |
| <a id="alert-68"></a>[#68](https://github.com/michelbr84/GarraRUST/security/code-scanning/68) | `rust/path-injection` | `crates/garraia-gateway/src/skins_handler.rs:111` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `get_skin` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 99 before `format!("{name}.json")`. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skins_test.rs::get_skin_with_dot_in_name_returns_400`. | GAR-490 |
| <a id="alert-69"></a>[#69](https://github.com/michelbr84/GarraRUST/security/code-scanning/69) | `rust/path-injection` | `crates/garraia-gateway/src/skins_handler.rs:141` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `delete_skin` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 129 before `tokio::fs::remove_file`. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skins_test.rs::delete_skin_with_backslash_returns_400`. | GAR-490 |
| <a id="alert-70"></a>[#70](https://github.com/michelbr84/GarraRUST/security/code-scanning/70) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:177` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `get_skill` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 162 before `std::fs::read_to_string`. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::get_skill_rejects_path_traversal`. | GAR-490 |
| <a id="alert-71"></a>[#71](https://github.com/michelbr84/GarraRUST/security/code-scanning/71) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:269` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `create_skill` guards `body.name` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 220 before `std::fs::write`. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::create_skill_rejects_{path_traversal,empty_name,nul_byte,windows_drive}`. | GAR-490 |
| <a id="alert-72"></a>[#72](https://github.com/michelbr84/GarraRUST/security/code-scanning/72) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:344` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `update_skill` double-guards URL `name` + `body.name` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at lines 300 and 307 before `std::fs::write`. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::update_skill_rejects_dot_in_name`. | GAR-490 |
| <a id="alert-73"></a>[#73](https://github.com/michelbr84/GarraRUST/security/code-scanning/73) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:533` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `export_skill` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 519 before `std::fs::read_to_string`. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::export_skill_rejects_dot_in_name`. | GAR-490 |
| <a id="alert-74"></a>[#74](https://github.com/michelbr84/GarraRUST/security/code-scanning/74) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:590` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `set_skill_triggers` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 574 before `std::fs::read_to_string`. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::set_skill_triggers_rejects_dot_in_name`. | GAR-490 |
| <a id="alert-75"></a>[#75](https://github.com/michelbr84/GarraRUST/security/code-scanning/75) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:632` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `set_skill_triggers` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 574 before `std::fs::write` of updated trigger content. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::set_skill_triggers_rejects_dot_in_name`. | GAR-490 |
| <a id="alert-76"></a>[#76](https://github.com/michelbr84/GarraRUST/security/code-scanning/76) | `rust/path-injection` | `crates/garraia-gateway/src/skins_handler.rs:104` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `get_skin` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 99 before `tokio::fs::read_to_string`. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skins_test.rs::get_skin_with_dot_in_name_returns_400`. | GAR-490 |
| <a id="alert-77"></a>[#77](https://github.com/michelbr84/GarraRUST/security/code-scanning/77) | `rust/path-injection` | `crates/garraia-gateway/src/skins_handler.rs:134` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `delete_skin` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 129 before `file_path.is_file()` check. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skins_test.rs::delete_skin_with_backslash_returns_400`. | GAR-490 |
| <a id="alert-78"></a>[#78](https://github.com/michelbr84/GarraRUST/security/code-scanning/78) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:167` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `get_skill` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 162 before `skill_path.exists()` check. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::get_skill_rejects_path_traversal`. | GAR-490 |
| <a id="alert-79"></a>[#79](https://github.com/michelbr84/GarraRUST/security/code-scanning/79) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:227` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `create_skill` guards `body.name` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 220 before `skill_path.exists()` check. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::create_skill_rejects_{path_traversal,empty_name,nul_byte,windows_drive,underscore_per_project_convention}`. | GAR-490 |
| <a id="alert-80"></a>[#80](https://github.com/michelbr84/GarraRUST/security/code-scanning/80) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:312` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `update_skill` double-guards URL `name` + `body.name` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at lines 300 and 307 before `skill_path.exists()` check. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::update_skill_rejects_dot_in_name`. | GAR-490 |
| <a id="alert-81"></a>[#81](https://github.com/michelbr84/GarraRUST/security/code-scanning/81) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:523` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `export_skill` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 519 before `skill_path.exists()` check. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::export_skill_rejects_dot_in_name`. | GAR-490 |
| <a id="alert-82"></a>[#82](https://github.com/michelbr84/GarraRUST/security/code-scanning/82) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:579` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `set_skill_triggers` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 574 before `skill_path.exists()` check. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::set_skill_triggers_rejects_dot_in_name`. | GAR-490 |

**Total**: 22 entries (6 from GAR-491 Wave 2 + 16 from GAR-490 Wave 1 PR A).
Bulk-dismissal proibido — cada linha foi revisada individualmente, com
referência ao helper guard, ao handler afetado, e à regressão de teste
correspondente.

## §5. Empirical validation

**Hypothesis**: dismissals via REST API persistem entre re-análises CodeQL
no mesmo repositório; o `state=dismissed` não é resetado quando o workflow
`codeql.yml` re-roda na mesma branch (ou após merge para `main`).

**Procedure**:

1. Aplicar dismissal apenas no alerta [#43](#alert-43) (`credentials.rs:49`,
   `dismissed_reason=false_positive`) na branch
   `security/gar-491-codeql-suppressions-2026-05-01`.
2. Imediato: `gh api repos/michelbr84/GarraRUST/code-scanning/alerts/43 --jq
   '{state,dismissed_reason}'` deve retornar `{"state":"dismissed",
   "dismissed_reason":"false_positive"}`.
3. Push commit no-op (esta seção §5 atualizada com run IDs) na branch para
   acionar `codeql.yml` re-run.
4. Aguardar CodeQL run completar (~16 min, baseline conhecido do PR #106).
5. Re-query o alerta — `state` deve continuar `dismissed`.

**Result** (preenchido após CodeQL re-run completar em
`security/gar-491-codeql-suppressions-2026-05-01` — última atualização
2026-05-01):

| Step | Run ID | Timestamp (UTC) | Status |
|------|--------|------------------|--------|
| Pre-dismissal CodeQL on main (baseline) | [`25202502297`](https://github.com/michelbr84/GarraRUST/actions/runs/25202502297) | 2026-05-01T04:39:43Z | success |
| Initial dismissal of #43 (PATCH) | n/a | 2026-05-01T12:33:36Z | success — `state=dismissed`, `reason="false positive"`, by `michelbr84` |
| Verify state immediate (gh api re-query) | n/a | 2026-05-01T12:33:38Z | success — confirmed dismissed |
| Push commit `34b155b`, trigger CodeQL re-run on branch | [`25214464719`](https://github.com/michelbr84/GarraRUST/actions/runs/25214464719) | 2026-05-01T12:35:44Z (start) | **success** |
| Re-query #43 post-rerun | n/a | 2026-05-01T~12:43Z | **`state=dismissed` PERSISTED** — `dismissed_reason="false positive"`, `dismissed_at=2026-05-01T12:33:36Z` (unchanged) |
| Apply remaining 5 dismissals via `--apply` | n/a | 2026-05-01T~12:44Z | success — 5 applied, 0 errors |
| Final verification: all 6 dismissed | n/a | 2026-05-01T~12:45Z | success — all 6 `{state:"dismissed"}` |

**Verdict**: ✅ **Empirical proof PASSED.** The REST-dismissal mechanism
preserves `state=dismissed` across CodeQL re-analysis of the same branch.
Mechanism approved for the batch.

Final state of all 6 alerts:

```
{"n":40,"reason":"used in tests","state":"dismissed"}
{"n":41,"reason":"used in tests","state":"dismissed"}
{"n":42,"reason":"used in tests","state":"dismissed"}
{"n":43,"reason":"false positive","state":"dismissed"}
{"n":44,"reason":"used in tests","state":"dismissed"}
{"n":45,"reason":"used in tests","state":"dismissed"}
```

**Idempotency**: confirmed empirically — a second `--apply` run on the
same ledger reports `6 skipped, 0 applied, 0 errors`. The script's
fail-closed validation (rule_id + path + start_line) re-passes for each
entry, and the API-form-aware skip check correctly identifies
already-dismissed alerts.

## §6. Failure handling (no global filter fallback)

Se a empirical proof §5 falhar (`state` reverte para `open` após CodeQL
re-run):

1. **PARAR** — não aplicar os 5 dismissals restantes.
2. **DOCUMENTAR** aqui em §5 com run IDs e timestamps do failure.
3. **ABRIR** sub-issue Linear `GAR-491.X` com o problema empírico observado.
4. **NÃO** silenciar globalmente via `query-filters: exclude` (proibido por
   §3 rule 6).
5. **PR #1 fica em draft permanente** até nova decisão.

Decisões aceitáveis para nova sub-issue:

- Custom query suite `.qls` com predicates Rust customizados (alta granularidade,
  alto custo de manutenção).
- Path-specific approach a definir caso a caso.
- Manual UI dismissal mantendo este ledger versionado (admite que o script de
  reaplicação não é confiável; revogação via UI vira fonte de verdade).

## §7. Reapply automation

Script: [`scripts/security/codeql-reapply-dismissals.sh`](../../scripts/security/codeql-reapply-dismissals.sh)

Funcionalidades:

- `--dry-run` (default em CI; mostra o que seria reaplicado sem PATCH).
- `--apply` (oposto explícito; faz PATCH).
- `--check-md` (valida que `.md` ↔ `.json` listam os mesmos `alert_number`).
- `--alert <N>` (escopo a um único alerta — usado pela empirical proof).

**Fail-closed**: para cada entry, antes de PATCH, o script confirma que o
alerta atual em GitHub tem mesmo `rule_id`, `path`, `line` que o ledger.
Se divergir → exit 2 + diagnóstico, manual re-audit obrigatório.

**Sem schedule automático nesta PR** (per amendment A8). Decisão de
agendamento fica em sub-issue follow-up `GAR-491.2` quando o mecanismo
estiver provado e estável.

## §8. See also

- [`docs/security/codeql-setup.md`](codeql-setup.md) — runbook do advanced
  setup, contexto histórico, paths-ignore.
- [`docs/security/dependabot-status.md`](dependabot-status.md) — sister
  ledger para Dependabot residuals.
- [`.github/codeql-config.yml`](../../.github/codeql-config.yml) —
  `paths-ignore` (não usado para suppression; só para autobuild safety).
- [`.github/workflows/codeql.yml`](../../.github/workflows/codeql.yml) —
  workflow advanced.
- Linear:
  [GAR-486](https://linear.app/chatgpt25/issue/GAR-486) (umbrella),
  [GAR-491](https://linear.app/chatgpt25/issue/GAR-491) (this),
  [GAR-490](https://linear.app/chatgpt25/issue/GAR-490) (Wave 1, blocked-by 491).
