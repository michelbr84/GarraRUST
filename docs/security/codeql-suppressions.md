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
> Last updated: **2026-08-30**.
> Audit re-triage por: **2026-08-01** — ⚠️ **VENCIDO** desde 2026-08-01
> (entradas com mais de 90 dias devem ser revisitadas; alertas que não existem
> mais no Security tab devem ser removidos do ledger).
>
> Estado do re-audit em 2026-08-29: a **metade mecânica** está feita e provada —
> as 23 entradas ainda casam `rule_id`/`path`/`linha` (§5.2). A **metade de
> julgamento** — cada justificativa ainda procede? o guard citado ainda está na
> linha citada? — **não** foi refeita, e por isso a data **não** foi renovada:
> mexer nela sem re-auditar seria exatamente o que §3 regra 4 existe para
> impedir. Renovar só junto com o re-audit de verdade.

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

### Amendment 2026-08-29 — fixture de teste sai por escopo, não por ledger

A tabela acima descartou `paths-ignore` porque *"testes do GarraRUST são INLINE
(`#[cfg(test)] mod tests {}`) dentro de produção; ignorar `mobile_auth.rs`
esconde alertas reais"*. Isso continua verdade para `paths-ignore` — mas existe
um mecanismo que resolve o problema pela raiz e que não estava em uso em
2026-05: o extractor Rust do CodeQL liga `cfg(test)` incondicionalmente
(`rust/extractor/src/config.rs::to_cfg_overrides`) e expõe a opção oficial
`cargo_cfg_overrides` para desligá-lo.

Desde 2026-08-29, `.github/workflows/codeql.yml` passa
`CODEQL_EXTRACTOR_RUST_OPTION_CARGO_CFG_OVERRIDES: "-test"` no passo
**Initialize CodeQL**. Consequência para este ledger:

- **Fixture de teste não entra mais aqui.** Se um alerta está dentro de um
  `#[cfg(test)]` ou em `crates/*/tests/`, ele deixa de existir por escopo de
  análise — não se dismissa, não se justifica linha a linha.
- **O ledger fica só para falso-positivo em código de produção**, que é onde a
  justificativa por linha realmente agrega (ex.: `credentials.rs:49`, um
  `vec![0u8; SALT_LEN]` sobrescrito por `SystemRandom::fill` na linha seguinte).
- As 5 entradas de `rust/hard-coded-cryptographic-value` que são fixture
  (`mobile_auth.rs`, `validation.rs`) **continuam válidas** — ao contrário do que
  esta seção previa antes de 2026-08-29. A previsão era que ficariam stale
  (`exit 3`) assim que o `-test` chegasse à `main`; a medição mostrou o
  contrário, e a razão está em §5.2. `credentials.rs:49` é produção e permanece
  de qualquer forma.
- O `-test` continua valendo para **alertas novos**: fixture que ainda não virou
  alerta não vira mais. O que ele não faz é apagar alerta já dispensado.
- As 16 entradas de `rust/path-injection` são High e não são afetadas.

Contexto completo da onda que motivou a mudança (bundle 2.26.3 → 2.26.4,
cobertura de extração de 118 para 422 arquivos):
[`codeql-setup.md`](codeql-setup.md) §"A onda de 2026-08-28".

**A entrada nova desta leva já está registrada**: alerta
[#165](#alert-165), o salt PBKDF2 legado em
`crates/garraia-gateway/src/admin/shared.rs:68` (`LEGACY_KDF_SALT`). O salt
constante deixou de ser usado para derivar qualquer chave nova — agora é
aleatório por instalação, com 600k iterações — mas a constante permanece no
fonte porque a migração forward-only precisa dela para decifrar os segredos já
gravados uma última vez antes de re-cifrá-los.

Ela estreia a disposição **`dismissed-wont-fix`** neste ledger, e a distinção
importa: as 22 entradas anteriores são `false_positive` (o CodeQL errou) ou
`used_in_tests` (não é superfície de produção). Esta é a primeira em que o
CodeQL está **certo** e mesmo assim mantemos o código — o valor é de fato um
salt constante, e a alternativa seria perder os segredos de toda instalação
existente. Registrar isso como falso-positivo seria mentir para o ledger.

**Efeito medido do `-test`** (PR #869, head `7a9b5fc`): os alertas Critical
novos do PR caíram de 3 para 1, e o que apontava para um `assert!` dentro de
`#[cfg(test)]` passou a `outdated`. Sobrou exatamente o #165, que é produção.

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

### Wiring em workflow (2026-08-29) — fecha o GAR-491.2

O comando acima era executado **à mão**, de uma máquina com PAT de escopo
`security_events` — foi assim que os 22 alertas de GAR-490/GAR-491 foram
dispensados (§5). O header do script registrava a lacuna:

> *"Per amendment A8: there is intentionally NO schedule wiring here. (…) A
> future sub-issue (GAR-491.2) decides if/when to wire it into a workflow."*

Agora existe [`.github/workflows/codeql-apply-dismissals.yml`](../../.github/workflows/codeql-apply-dismissals.yml):
`workflow_dispatch`, `permissions: security-events: write`, rodando o mesmo
script com o `GITHUB_TOKEN` do job — sem PAT.

Três decisões que importam:

- **Dry-run é o default.** O input `apply` é `false` até alguém marcar. Rodar o
  workflow sem pensar não escreve nada.
- **Workflow separado do de triagem.** `codeql-triage.yml` continua
  `security-events: read`. Um relatório não carrega permissão de escrita sobre
  alertas de segurança.
- **Sem `schedule`.** Dispensar um alerta é ato deliberado, não rotina. E sem
  `continue-on-error`: `exit 2` (ledger divergente) e `exit 3` (alerta sumiu)
  **são** o resultado útil quando acontecem — significam "reaudite à mão".

O input `alert` restringe a um número só, que foi como a prova empírica do #43
foi conduzida. Isso também contorna uma armadilha real: o loop percorre as
entradas em ordem e aborta na primeira stale, então uma entrada obsoleta no topo
impediria as de baixo de serem aplicadas.

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
7. **`alert_number` não é chave estável — reconciliar antes de reaplicar.**
   Ver "Amendment 2026-08-30" abaixo.

### Amendment 2026-08-30 — renumeração silenciosa e o `--rekey`

Em 2026-08-29 o Security tab voltou a acusar 16 alertas `rust/path-injection`
que já estavam dispensados desde o PR
[#111](https://github.com/michelbr84/GarraRUST/pull/111). Não foi regressão de
código, nem o dismissal ter sido revertido: o dry-run do
`codeql-reapply-dismissals.sh` no run
[33261527354](https://github.com/michelbr84/GarraRUST/actions/runs/33261527354)
reportou `skipped: 23 (already-dismissed)` — as entradas #67–#82 continuavam
vivas e dispensadas.

O que aconteceu foi **duplicação**. Quando o escopo da análise mudou no mesmo
dia (`paths-ignore: crates/*/tests/**` mais o
`CODEQL_EXTRACTOR_RUST_OPTION_CARGO_CFG_OVERRIDES` movido para o nível do job),
os `partialFingerprints` do SARIF mudaram junto, o GitHub deixou de casar os
achados com os alertas existentes e **criou alertas novos** — #147–#162 — para
o mesmo código, nas mesmas linhas. Os dismissals ficaram grudados nas
duplicatas aposentadas.

A lição: `alert_number` é um identificador de apresentação, não uma identidade.
A identidade estável é `(rule_id, path, line)` — que é, aliás, exatamente o que
o `codeql-reapply-dismissals.sh` já validava fail-closed antes de reaplicar.

Por isso existe [`scripts/security/codeql-rekey-ledger.py`](../../scripts/security/codeql-rekey-ledger.py):
ele lista os alertas abertos, casa por `(rule_id, path, line)` e reaponta o
`alert_number` das entradas, reescrevendo `.json` e `.md` em sincronia para o
`--check-md` continuar passando. Ele **não** emite `PATCH`, **não** inventa
entrada para alerta sem justificativa, e **não** apaga entrada — nada nele
afrouxa a regra 1 (`no bulk suppression`): a justificativa de cada linha já foi
revisada uma vez e continua valendo, só o ponteiro muda, e o diff vai a review
em PR como qualquer outro.

Duas chaves iguais casando com dois alertas abertos é `exit 2`: o script não
adivinha. Entrada sem alerta aberto correspondente **não** é erro — é o estado
saudável de uma entrada dispensada e quieta —, só entra no resumo para alimentar
o re-audit de 90 dias da regra 4.

O `codeql-triage.yml` roda esse plano em `--dry-run` a cada execução e publica
no job summary, então a divergência aparece sozinha na próxima triagem em vez de
esperar alguém reparar no Security tab.

## §4. Ledger

| # | Rule | File:line | Disposition | Reason | Justificativa | GAR |
|---|------|-----------|-------------|--------|---------------|-----|
| <a id="alert-40"></a>[#40](https://github.com/michelbr84/GarraRUST/security/code-scanning/40) | `rust/hard-coded-cryptographic-value` | `crates/garraia-gateway/src/mobile_auth.rs:738` | dismissed-used-in-tests | `used_in_tests` | Test fixture em `#[tokio::test] argon2id_register_and_login_roundtrip`. Literal salt `""` é placeholder do path PHC Argon2id (que embute seu próprio salt); coluna legacy não-usada. | GAR-491 |
| <a id="alert-41"></a>[#41](https://github.com/michelbr84/GarraRUST/security/code-scanning/41) | `rust/hard-coded-cryptographic-value` | `crates/garraia-gateway/src/mobile_auth.rs:749` | dismissed-used-in-tests | `used_in_tests` | Test fixture em `#[tokio::test] argon2id_register_and_login_roundtrip` — branch negativo, password `"nope"` deve retornar false. Input intencionalmente inválido para coverage. | GAR-491 |
| <a id="alert-42"></a>[#42](https://github.com/michelbr84/GarraRUST/security/code-scanning/42) | `rust/hard-coded-cryptographic-value` | `crates/garraia-gateway/src/mobile_auth.rs:870` | dismissed-used-in-tests | `used_in_tests` | Test fixture em `#[tokio::test] second_login_after_upgrade_still_works`. `"seq-password-xyz"` exercita o PBKDF2 → Argon2id lazy-upgrade transactional path; nunca persistido. | GAR-491 |
| <a id="alert-43"></a>[#43](https://github.com/michelbr84/GarraRUST/security/code-scanning/43) | `rust/hard-coded-cryptographic-value` | `crates/garraia-security/src/credentials.rs:49` | dismissed-false-positive | `false_positive` | `vec![0u8; SALT_LEN]` é buffer initializer imediatamente sobrescrito por `ring::SystemRandom::fill` na linha 50. API do `ring` exige `&mut [u8]` como backing; literal `0u8` nunca vira salt real. **Anchor da empirical proof do mecanismo.** | GAR-491 |
| <a id="alert-44"></a>[#44](https://github.com/michelbr84/GarraRUST/security/code-scanning/44) | `rust/hard-coded-cryptographic-value` | `crates/garraia-security/src/validation.rs:233` | dismissed-used-in-tests | `used_in_tests` | Test fixture em `#[test] validate_password_length`. Literal `"short"` intencionalmente abaixo do mínimo para asserir `Err`. Negative-path coverage. | GAR-491 |
| <a id="alert-45"></a>[#45](https://github.com/michelbr84/GarraRUST/security/code-scanning/45) | `rust/hard-coded-cryptographic-value` | `crates/garraia-security/src/validation.rs:234` | dismissed-used-in-tests | `used_in_tests` | Test fixture em `#[test] validate_password_length`. Literal `"validpass123"` intencionalmente acima do mínimo para asserir `Ok`. Positive-path coverage. | GAR-491 |
| <a id="alert-149"></a>[#149](https://github.com/michelbr84/GarraRUST/security/code-scanning/149) | `rust/path-injection` | `crates/garraia-gateway/src/skins_handler.rs:84` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `create_skin` guards `body.name` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 60 before `tokio::fs::create_dir_all` / `tokio::fs::write`. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skins_test.rs::create_skin_with_path_traversal_returns_400` + `create_skin_with_dot_in_name_returns_400` + `create_skin_rejects_underscore_per_project_convention`. | GAR-490 |
| <a id="alert-147"></a>[#147](https://github.com/michelbr84/GarraRUST/security/code-scanning/147) | `rust/path-injection` | `crates/garraia-gateway/src/skins_handler.rs:111` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `get_skin` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 99 before `format!("{name}.json")`. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skins_test.rs::get_skin_with_dot_in_name_returns_400`. | GAR-490 |
| <a id="alert-148"></a>[#148](https://github.com/michelbr84/GarraRUST/security/code-scanning/148) | `rust/path-injection` | `crates/garraia-gateway/src/skins_handler.rs:141` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `delete_skin` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 129 before `tokio::fs::remove_file`. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skins_test.rs::delete_skin_with_backslash_returns_400`. | GAR-490 |
| <a id="alert-150"></a>[#150](https://github.com/michelbr84/GarraRUST/security/code-scanning/150) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:177` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `get_skill` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 162 before `std::fs::read_to_string`. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::get_skill_rejects_path_traversal`. | GAR-490 |
| <a id="alert-153"></a>[#153](https://github.com/michelbr84/GarraRUST/security/code-scanning/153) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:269` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `create_skill` guards `body.name` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 220 before `std::fs::write`. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::create_skill_rejects_{path_traversal,empty_name,nul_byte,windows_drive}`. | GAR-490 |
| <a id="alert-154"></a>[#154](https://github.com/michelbr84/GarraRUST/security/code-scanning/154) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:344` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `update_skill` double-guards URL `name` + `body.name` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at lines 300 and 307 before `std::fs::write`. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::update_skill_rejects_dot_in_name`. | GAR-490 |
| <a id="alert-151"></a>[#151](https://github.com/michelbr84/GarraRUST/security/code-scanning/151) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:533` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `export_skill` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 519 before `std::fs::read_to_string`. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::export_skill_rejects_dot_in_name`. | GAR-490 |
| <a id="alert-152"></a>[#152](https://github.com/michelbr84/GarraRUST/security/code-scanning/152) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:590` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `set_skill_triggers` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 574 before `std::fs::read_to_string`. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::set_skill_triggers_rejects_dot_in_name`. | GAR-490 |
| <a id="alert-155"></a>[#155](https://github.com/michelbr84/GarraRUST/security/code-scanning/155) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:632` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `set_skill_triggers` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 574 before `std::fs::write` of updated trigger content. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::set_skill_triggers_rejects_dot_in_name`. | GAR-490 |
| <a id="alert-161"></a>[#161](https://github.com/michelbr84/GarraRUST/security/code-scanning/161) | `rust/path-injection` | `crates/garraia-gateway/src/skins_handler.rs:104` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `get_skin` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 99 before `tokio::fs::read_to_string`. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skins_test.rs::get_skin_with_dot_in_name_returns_400`. | GAR-490 |
| <a id="alert-162"></a>[#162](https://github.com/michelbr84/GarraRUST/security/code-scanning/162) | `rust/path-injection` | `crates/garraia-gateway/src/skins_handler.rs:134` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `delete_skin` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 129 before `file_path.is_file()` check. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skins_test.rs::delete_skin_with_backslash_returns_400`. | GAR-490 |
| <a id="alert-156"></a>[#156](https://github.com/michelbr84/GarraRUST/security/code-scanning/156) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:167` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `get_skill` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 162 before `skill_path.exists()` check. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::get_skill_rejects_path_traversal`. | GAR-490 |
| <a id="alert-157"></a>[#157](https://github.com/michelbr84/GarraRUST/security/code-scanning/157) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:227` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `create_skill` guards `body.name` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 220 before `skill_path.exists()` check. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::create_skill_rejects_{path_traversal,empty_name,nul_byte,windows_drive,underscore_per_project_convention}`. | GAR-490 |
| <a id="alert-158"></a>[#158](https://github.com/michelbr84/GarraRUST/security/code-scanning/158) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:312` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `update_skill` double-guards URL `name` + `body.name` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at lines 300 and 307 before `skill_path.exists()` check. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::update_skill_rejects_dot_in_name`. | GAR-490 |
| <a id="alert-159"></a>[#159](https://github.com/michelbr84/GarraRUST/security/code-scanning/159) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:523` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `export_skill` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 519 before `skill_path.exists()` check. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::export_skill_rejects_dot_in_name`. | GAR-490 |
| <a id="alert-160"></a>[#160](https://github.com/michelbr84/GarraRUST/security/code-scanning/160) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:579` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `set_skill_triggers` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 574 before `skill_path.exists()` check. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::set_skill_triggers_rejects_dot_in_name`. | GAR-490 |
| <a id="alert-111"></a>[#111](https://github.com/michelbr84/GarraRUST/security/code-scanning/111) | `rust/cleartext-logging` | `crates/garraia-cli/src/config_cmd.rs:210` | dismissed-false-positive | `false_positive` | **Wave 3 — cleartext.** `garraia config check` imprime presenca, nunca valor: o campo `summary.gateway_api_key_set` e `bool` e as unicas strings possiveis sao os literais `"set"` / `"not set"`. A secao sai sob o cabecalho literal `"Summary (redacted)"` (config_cmd.rs:203). CodeQL rastreia o identificador chamado `api_key`, nao o booleano que ele carrega. | GAR-491 |
| <a id="alert-112"></a>[#112](https://github.com/michelbr84/GarraRUST/security/code-scanning/112) | `rust/cleartext-logging` | `crates/garraia-cli/src/config_cmd.rs:233` | dismissed-false-positive | `false_positive` | **Wave 3 — cleartext.** Mesma superficie do [#111](#alert-111): imprime `summary.llm_providers_api_key_set.join(", ")`, que e a lista de **nomes de provider** com chave configurada (ex.: `[openai, anthropic]`), nao as chaves. Invariante de redaction documentado em CLAUDE.md (garraia-config, plan 0046). | GAR-491 |
| <a id="alert-113"></a>[#113](https://github.com/michelbr84/GarraRUST/security/code-scanning/113) | `rust/cleartext-logging` | `crates/garraia-cli/src/wizard/mod.rs:673` | dismissed-false-positive | `false_positive` | **Wave 3 — cleartext.** `println!("  Cloud provider API key encrypted in vault (entry {entry}).")` imprime o **nome** da entrada do cofre, nunca o segredo. `cloud_secret` e `Option<(&str, &str)>` ([wizard/mod.rs:630](../../crates/garraia-cli/src/wizard/mod.rs)); o destructuring `(entry, key)` manda o `key` exclusivamente para `vault.set(entry, key)` na linha anterior, e o `println!` interpola apenas `entry`. CodeQL contamina `entry` por ele vir do mesmo tuple que `key`. **Re-audit 2026-08-30:** esta entrada apontava para `:640` e descrevia outro statement — o `eprintln!` do `vault open failed`, hoje em `:643`. O alerta vivo sempre esteve no `println!` de `:673`, que e o que [§5.3](#53--os-16-path-injection-foram-renumerados-nao-reabertos-2026-08-29) ja dizia; era o `.json` que estava errado. Ver a nota de re-audit apos esta tabela. | GAR-491 |
| <a id="alert-115"></a>[#115](https://github.com/michelbr84/GarraRUST/security/code-scanning/115) | `rust/cleartext-storage-database` | `crates/garraia-db/src/session_store.rs:1326` | dismissed-false-positive | `false_positive` | **Triagem 2026-08-30.** O que chega ao banco e um hash, nunca o token: `validate_session_token` liga `params![hash_session_token(token)]`, e `hash_session_token` ([session_store.rs:1261](../../crates/garraia-db/src/session_store.rs)) e SHA-256 hex de mao unica. O lado da escrita declara o invariante em `session_store.rs:1283` — *"Only the hash is persisted; the raw token goes to the caller and is never written to disk"* — e ha migracao que reescreve tokens legados em claro, detectados por `length(token) <> 64 OR token GLOB '*[^0-9a-f]*'` (`session_store.rs:344`). CodeQL trata `token: &str` como credencial fluindo para uma query, sem modelar o hash como transformacao irreversivel. | GAR-491 |
| <a id="alert-144"></a>[#144](https://github.com/michelbr84/GarraRUST/security/code-scanning/144) | `rust/cleartext-transmission` | `crates/garraia-channels/src/signal/mod.rs:279` | dismissed-false-positive | `false_positive` | **Triagem 2026-08-30.** O sink (`:279` neste PR, `:272` no alerta vivo, que foi emitido contra a main antes de este PR deslocar o arquivo) esta **dentro** de `connect()`, depois do guard: `connect()` chama `ensure_url_vetted()?` antes de qualquer IO, e o `?` aborta se a URL for recusada. O `config` e clonado **antes** do `tokio::spawn` e `SignalConfig` nao tem mutabilidade interior, entao a URL do loop e o mesmo valor imutavel ja validado. CodeQL nao liga o clone a validacao anterior. Auditado por `security-auditor`. Contraste com o **#143**, mesma regra e mesmo arquivo, que **nao** entrou neste ledger: era codigo morto (`receive_messages`, `#[allow(dead_code)]`, zero call sites) e foi **removido**, fechando como `Fixed`. | GAR-491 |
| <a id="alert-145"></a>[#145](https://github.com/michelbr84/GarraRUST/security/code-scanning/145) | `rust/cleartext-transmission` | `crates/garraia-channels/src/whatsapp/api.rs:48` | dismissed-false-positive | `false_positive` | **Wave 3 — cleartext.** O destino e HTTPS: `const GRAPH_API_BASE: &str = "https://graph.facebook.com/v21.0"` ([whatsapp/api.rs:5](../../crates/garraia-channels/src/whatsapp/api.rs)), entao `.bearer_auth(token)` sai sempre sobre TLS. O esquema e constante de compilacao e nenhum caminho de config o altera. CodeQL nao dobra a const dentro do `format!` e trata o esquema como desconhecido. | GAR-491 |
| <a id="alert-146"></a>[#146](https://github.com/michelbr84/GarraRUST/security/code-scanning/146) | `rust/cleartext-transmission` | `crates/garraia-channels/src/whatsapp/api.rs:79` | dismissed-false-positive | `false_positive` | **Wave 3 — cleartext.** Identico ao [#145](#alert-145), em `mark_as_read`. Contraste deliberado com os alertas 143/144 do canal Signal: mesmo rule, mas **nao** eram falso-positivo — la a URL base vinha da config do operador e admitia `http://` remoto, e foi corrigida em codigo (guard `vet_signal_cli_url`). | GAR-491 |
| <a id="alert-165"></a>[#165](https://github.com/michelbr84/GarraRUST/security/code-scanning/165) | `rust/hard-coded-cryptographic-value` | `crates/garraia-gateway/src/admin/shared.rs:68` | dismissed-wont-fix | `wont_fix` | `LEGACY_KDF_SALT`. **Não é falso-positivo nem fixture** — o CodeQL está certo, é um salt PBKDF2 constante. Permanece deliberadamente porque a migração forward-only do PR [#869](https://github.com/michelbr84/GarraRUST/pull/869) precisa dele para decifrar **uma última vez** os segredos de instalações anteriores a 2026-08-29, antes de re-cifrá-los sob a chave derivada do salt aleatório por instalação. Nenhuma chave nova é derivada dele: `derive_with_passphrase` só o usa no arm de migração pendente e no fallback de parâmetros ilegíveis. Os parâmetros novos vivem na tabela `kdf_params` do `admin.db`, gravados na mesma transação que re-cifra os segredos. Só sai quando não houver mais instalações por migrar. ✅ Número reconferido na `main` em 2026-08-29 (squash `a47d3c3`): o dry-run casou `rule_id`/`path`/`line` sem `exit 2`, então 165 sobreviveu ao squash. Dispensado no mesmo dia — ver §5.2. | PR-869 |

**Total**: 30 entries (6 from GAR-491 Wave 2 + 16 from GAR-490 Wave 1 PR A +
1 from PR [#869](https://github.com/michelbr84/GarraRUST/pull/869) + 5 from
Wave 3 cleartext + 2 da triagem de 2026-08-30, #115 e #144).
Bulk-dismissal proibido — cada linha foi revisada individualmente, com
referência ao helper guard, ao handler afetado, e à regressão de teste
correspondente.

### Triagem 2026-08-30 — os 3 alertas que sobraram sem entrada

Depois da renumeração e do `apply` dos 21 dismissals, o `--dry-run` do
`codeql-rekey-ledger.py` deixou exatamente 3 alertas abertos sem entrada:
**#115**, **#143** e **#144**. Os três foram auditados, e o desfecho **não** foi
o mesmo para os três — que é o ponto de auditar um a um.

**#143 saiu por remoção de código, não por supressão.** O sink era
`receive_messages()` em `signal/mod.rs`, marcada `#[allow(dead_code)]`, com zero
call sites em todo o workspace, duplicando a construção de URL e o GET que o loop
dentro de `connect()` já faz. Ela também era uma armadilha: não passava pelo
guard, ao contrário do loop. Foi **removida**, e o alerta fecha como `Fixed`.
Nenhuma entrada foi criada — o ledger é último recurso (§3), e código morto não
precisa de justificativa, precisa de `git rm`.

**#144 e #115 são falso-positivo de verdade** e entraram na tabela acima, cada um
com a evidência de linha.

**O que a auditoria destapou de brinde.** O doc comment do próprio
`vet_signal_cli_url` afirmava cobrir *"every request this channel makes"*,
nomeando `send_text` / `send_to_group` porque carregam o corpo da mensagem. Mas o
guard tinha **um único call site**: `connect()`. E `Channel::send_message` alcança
os dois sends sem `connect()` jamais rodar — nada no tipo nem em runtime ordena os
dois. Era vazamento real de número de telefone e corpo de mensagem sobre `http://`
remoto, e o CodeQL **não** flagou (só apontou `:143` e `:272`). Corrigido no mesmo
PR: os sends passam por `ensure_url_vetted`, que memoiza o veredito num
`OnceLock` — o guard resolve DNS com syscall bloqueante, e `SignalConfig` é
imutável depois de construída, então pagar isso a cada envio seria caro e inútil.

Mitigante que a auditoria também estabeleceu: `SignalChannel` hoje não é
instanciado em lugar nenhum — `bootstrap/channels.rs` não tem arm `"signal"`.
Risco imediato baixo, risco latente alto, que é exatamente o caso em que se
fecha a porta antes de alguém abrir.

### Re-audit 2026-08-30 — a renumeração aplicada, e o que ela destapou

As 16 entradas `rust/path-injection` foram reapontadas dos números antigos
(#67-#82) para as duplicatas vivas (#147-#162), fechando o que a
[Amendment 2026-08-30](#amendment-2026-08-30--renumeração-silenciosa-e-o---rekey)
previa. A condição que §5.3 impunha para adiar — "as linhas vão mudar de novo,
por causa do autofix do #874" — deixou de valer: o dry-run do run
[33299044785](https://github.com/michelbr84/GarraRUST/actions/runs/33299044785),
sobre `c7d7b08`, casou as 16 por `(rule_id, path, line)` sem ambiguidade.
Nenhuma justificativa foi tocada: só o ponteiro andou, que é o que mantém a
regra §3.1 de pé.

**O #113 não era renumeração — era entrada errada.** O `.json` apontava para
`wizard/mod.rs:640` e descrevia o `eprintln!` do `vault open failed`. Esse
`eprintln!` está hoje em `:643` (deriva de +3), mas o alerta vivo está em
`:673`, que é outro statement, em outro bloco: o
`println!("  Cloud provider API key encrypted in vault (entry {entry}).")`.
Fosse o `eprintln!`, o alerta teria acompanhado a deriva de +3. A §5.3 acima já
descrevia o #113 corretamente ("imprime o nome da entrada do cofre") — ou seja,
`.json` e `.md` discordavam sobre o que a entrada era. Ambos foram corrigidos
para o sink real, e a conclusão de falso-positivo continua de pé, agora pela
razão certa.

**Por que passou despercebido por três meses.** `plan_rekey` pulava toda entrada
cujo `alert_number` seguisse aberto, sem nunca comparar a chave — o comentário
dizia "ja aponta para um alerta aberto **nesta chave**", mas o código só
comparava o número. Uma entrada com número vivo e linha podre era invisível ao
relatório. O script agora reporta esses casos como `drifted` e sai com `exit 2`
(mesma semântica de "stale, reaudite à mão" que o
`codeql-reapply-dismissals.sh` já usa), com cobertura em
`scripts/security/tests/test_rekey_ledger.py`.


## §5. Empirical validation

### §5.1 — Persistência do dismissal entre re-análises (2026-05-01)

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

### §5.2 — O `-test` não apaga alerta já dispensado (2026-08-29)

**Previsão que foi falsificada.** §1 afirmava que as 5 entradas de fixture
(`mobile_auth.rs`, `validation.rs`) ficariam stale — `exit 3` — assim que o
`CODEQL_EXTRACTOR_RUST_OPTION_CARGO_CFG_OVERRIDES: "-test"` chegasse à `main`.
O raciocínio era: sem extração, sem alerta.

**Medição.** Com o `-test` já em `main` desde `c3e7521` (PR #869, 13:29Z), o
workflow `codeql-apply-dismissals.yml` rodou em `--dry-run` **sem `--alert`**,
percorrendo as 23 entradas:

| Run | Inputs | Resultado |
|---|---|---|
| [`33261274173`](https://github.com/michelbr84/GarraRUST/actions/runs/33261274173) | `apply:false, alert:165` | `DRY-RUN: would PATCH alert #165 state=dismissed reason='won't fix'` — `applied: 0, dry-run: 1, errors: 0` |
| [`33261314173`](https://github.com/michelbr84/GarraRUST/actions/runs/33261314173) | `apply:true, alert:165` | `applied: 1, skipped: 0, errors: 0` |
| [`33261350845`](https://github.com/michelbr84/GarraRUST/actions/runs/33261350845) | `apply:true, alert:165` | `skip: already dismissed (reason='won't fix')` — idempotência confirmada |
| [`33261527354`](https://github.com/michelbr84/GarraRUST/actions/runs/33261527354) | `apply:false` (sem escopo) | **`skipped: 23 (already-dismissed)`, `applied: 0`, `errors: 0`** |

**Verdict**: ❌ **previsão falsa.** Nenhuma das 5 entradas ficou stale. A
transição para `state=fixed` acontece para alertas que estavam **`open`** e
deixaram de aparecer na análise; um alerta já `dismissed` permanece `dismissed`
independentemente de o código continuar sendo extraído. O `-test` reduz alertas
**novos**, não retroage sobre dispensados.

**Consequência prática.** A armadilha descrita em §2 — "o loop aborta na primeira
entrada stale, então uma obsoleta no topo impediria as de baixo" — é uma
propriedade real do script, mas **não se materializou aqui**: a run sem escopo
chegou até o #165. O input `alert` continua útil como bisturi, não como
contorno obrigatório.

**Ganho colateral, que vale registrar.** A validação fail-closed (`rule_id` +
`path` + `start_line`) roda **antes** do skip de já-dispensado
(`codeql-reapply-dismissals.sh:184-200` vs `:229`). Como a run fechou com
`errors: 0` e sem `exit 2`, ela provou mecanicamente que **as 23 entradas ainda
apontam para o mesmo rule/path/linha** que o ledger registra. Isso cobre a
metade mecânica do re-audit de 90 dias exigido por §3 regra 4 — não a metade de
julgamento (a justificativa continua fazendo sentido?), que segue pendente. Ver
o aviso de expiração no topo deste arquivo.

### §5.3 — Os 16 `path-injection` foram RENUMERADOS, não reabertos (2026-08-29)

Medido pelo `codeql-triage.yml` em `ba6d86b`
([run 33263640687](https://github.com/michelbr84/GarraRUST/actions/runs/33263640687)),
`severity=high, state=open`: **24 alertas**, e 16 deles são
`rust/path-injection` nos mesmos arquivos e linhas que este ledger já dispensa.

Não é regressão nem o dismissal falhando. §5.2 provou que #67–#82 continuam
`dismissed`. O que aconteceu é que a re-análise pós-upgrade do bundle
(2.26.3 → 2.26.4) emitiu um **segundo conjunto de alertas** para os mesmos
achados, com números novos. O mapeamento é um-para-um por `rule_id` + `path` +
`linha`:

| Ledger (dismissed) | Live (open) | Arquivo:linha |
|---|---|---|
| #67 | #149 | `skins_handler.rs:84` |
| #68 | #147 | `skins_handler.rs:111` |
| #69 | #148 | `skins_handler.rs:141` |
| #76 | #161 | `skins_handler.rs:104` |
| #77 | #162 | `skins_handler.rs:134` |
| #70 | #150 | `skills_handler.rs:177` |
| #71 | #153 | `skills_handler.rs:269` |
| #72 | #154 | `skills_handler.rs:344` |
| #73 | #151 | `skills_handler.rs:533` |
| #74 | #152 | `skills_handler.rs:590` |
| #75 | #155 | `skills_handler.rs:632` |
| #78 | #156 | `skills_handler.rs:167` |
| #79 | #157 | `skills_handler.rs:227` |
| #80 | #158 | `skills_handler.rs:312` |
| #81 | #159 | `skills_handler.rs:523` |
| #82 | #160 | `skills_handler.rs:579` |

**Por que o `alert_number` NÃO foi atualizado agora.** Duas razões, nenhuma
delas burocrática:

1. As linhas vão mudar de novo. O merge de
   [#874](https://github.com/michelbr84/GarraRUST/pull/874) (Copilot Autofix
   para o #161) inseriu ~25 linhas em `skins_handler.rs::get_skin`, e as linhas
   dos sinks pinados já se moveram no fonte: `:104 → :136`, `:134 → :159`,
   `:141 → :166`, `:111 → :117`. Só `:84` (#67/#149) não mexeu. Renumerar antes
   da próxima análise do CodeQL sobre `ba6d86b` produziria um ledger que nasce
   stale.
2. O autofix pode ter **resolvido** #161 de verdade — ele adicionou
   `canonicalize` + checagem de prefixo em `get_skin`, que é exatamente o que o
   CodeQL queria. Se resolveu, a entrada correspondente sai do ledger em vez de
   ser renumerada.

**Próximo passo, na ordem certa:** esperar a análise CodeQL de `ba6d86b`
concluir → rodar `codeql-apply-dismissals.yml` com `apply:false` **sem escopo**
→ ler o `exit 2`/`exit 3` que ele der → reconciliar `.json` **e** `.md` num PR
revisável. §3 regra 5 é explícita: divergência não se auto-corrige.

**Os outros 8 High**, todos verificados no fonte e todos falso-positivo ou
material de ledger, nenhum exigindo código:

| Regra | # | Local | Leitura |
|---|---|---|---|
| `rust/cleartext-logging` | #111, #112 | `config_cmd.rs:210,233` | Imprime `"set"`/`"not set"` e a **lista de nomes** de provider com chave — nunca o valor. É o invariante de redaction do plan 0046. |
| `rust/cleartext-logging` | #113 | `wizard/mod.rs:640` | Imprime o **nome da entrada** do cofre; o `key` vai só para `vault.set(entry, key)`. |
| `rust/cleartext-transmission` | #143–#146 | `signal/mod.rs:143,203`, `whatsapp/api.rs:48,79` | Ainda não auditados linha a linha. |
| `rust/cleartext-storage-database` | #115 | `session_store.rs:1261` | Ainda não auditado. |

Correção de escopo: o plano anterior falava em "33 sites de `non-https-url`".
Essa regra **não aparece** entre os alertas abertos. A onda High real é
16 + 4 + 3 + 1 = 24.

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
