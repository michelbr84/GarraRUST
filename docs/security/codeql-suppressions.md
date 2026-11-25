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
> Last updated: **2026-11-25**.
> Audit re-triage por: **2027-02-23** — 90 dias a partir da re-auditoria de
> 2026-11-25 (entradas com mais de 90 dias devem ser revisitadas; alertas que
> não existem mais no Security tab devem ser removidos do ledger).
>
> **Re-auditoria de mérito concluída em 2026-11-25** (issue
> [#1991](https://github.com/michelbr84/GarraRUST/issues/1991)), sobre as 43
> entradas que o ledger tinha. **Ficam 29; saem 14**, todas de alertas que a
> análise do `main` deixou de detectar. Uma justificativa continha afirmação
> falsa (#165) e três respondiam à pergunta de outra regra (#144, #145, #146).
> O registro completo — veredito por entrada, as 14 aposentadas e o porquê de
> cada uma — está em
> [§4, "Re-auditoria de mérito 2026-11-25"](#re-auditoria-de-mérito-2026-11-25--43-entradas-auditadas-29-ficam).
>
> **A data agora é verificada por código.** `check-ledger-anchors.py` compara
> `audit_expiration` com a data de hoje e sai com **exit 7** se estiver no
> passado; o job `CodeQL Ledger Anchors` roda em todo PR. Até 2026-08-30 o
> campo não era lido por *nada* — nem script, nem workflow, nem teste — e ficou
> vencido 29 dias sem que nenhum build reclamasse. Uma regra sem leitor é um
> comentário.
>
> Removida em 2026-08-30 a entrada do alerta **#43** (`credentials.rs:49`,
> `rust/hard-coded-cryptographic-value`): o padrão que a justificava —
> `let mut salt = vec![0u8; SALT_LEN];` seguido de `rng.fill` — deixou de
> existir. O site passou a usar `garraia_security::random_bytes`, que devolve
> o array já preenchido pelo CSPRNG e nunca materializa um buffer de zeros.
> É o mesmo desfecho do alerta #142 e a preferência declarada em §3: correção
> em código, não supressão. O ledger cai de 30 para 29 entradas. A §5 continua
> narrando o experimento empírico que usou o #43 como alvo — é registro
> histórico, e por isso a referência lá virou texto simples.
>
> **Re-auditoria de mérito concluída em 2026-08-30** (issue
> [#886](https://github.com/michelbr84/GarraRUST/issues/886)), sobre as 29
> entradas. A metade mecânica virou contínua com o `CodeQL Ledger Anchors`
> (PR #882), então esta passagem cobriu o que ferramenta não faz — se cada
> justificativa ainda procede:
>
> - **Os 16 `rust/path-injection`** se apoiam em `validate_skill_name`
>   (`path_validation.rs:76`) ser allow-list. Conferido: continua
>   `is_ascii_alphanumeric() || c == b'-'` com `MAX_NAME_LEN = 128`, exatamente
>   como o ledger alega. E é chamado **antes** de todos os 16 sinks, com
>   `return` imediato no erro — verificado função a função, não por amostragem.
> - **#111 / #112** (`config_cmd.rs`) imprimem `"set"`/`"not set"` e uma lista
>   de *nomes* de provider (`.map(|(name, _)| name.clone())` em
>   `check.rs:212`), nunca a chave.
> - **#113** (`wizard/mod.rs:769`; era `:673` até o #1180 mover o wizard): o
>   `key` vai só para `vault.set(entry, key)`; o `println!` interpola apenas
>   `entry`. A correção de 2026-08-30 procede.
> - **#115** (`session_store.rs:1326`): o único valor ligado à query é
>   `params![hash_session_token(token)]`, duas linhas abaixo.
> - **#144** (`signal/mod.rs`): reforçado desde a triagem — o POST de resposta
>   do mesmo `tokio::spawn` passou a chamar `api::send_text`, que roda o guard.
> - **#145 / #146** (`whatsapp/api.rs`): `GRAPH_API_BASE` é uma constante
>   `https://`; não há caminho para tráfego em claro.
> - **#40, #41, #42, #44, #45**: continuam dentro de `#[cfg(test)]`.
>
> Nenhuma entrada mudou de categoria e nenhuma virou achado real. A #43 saiu
> por correção em código (acima), não por vencimento.
>
> **#165 — a condição de saída não é verificável neste repositório.** A entrada
> diz que o `LEGACY_KDF_SALT` "só sai quando não houver mais instalações por
> migrar". Confirmei que o código que o consome segue vivo:
> `resolve_admin_encryption_key` é chamado no boot (`server.rs:657` e `:820`) e
> dispara `migrate_admin_secrets_kdf`. Mas *se ainda existem instalações
> anteriores a 2026-08-29 sem ter bootado* é fato de campo, não de código — o
> repositório não tem como responder. A entrada fica, e a condição de saída
> precisa de um critério observável (por exemplo: nenhuma instalação suportada
> em versão anterior à que introduziu o salt por instalação) antes de poder ser
> avaliada por quem for renovar em 2026-11-28.

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
- **Atualização de 2026-11-25:** as 5 entradas de fixture saíram do ledger na
  re-auditoria daquele dia. "Continuam válidas" era verdade sobre o `state` do
  alerta e escondia o resto: a instância delas em `refs/heads/main` está
  `fixed`, e a última análise que as viu foi a de `587d434f` (2026-08-29) — a
  análise não as vê há três meses. Ver §4, "Re-auditoria de mérito
  2026-11-25".
- As 16 entradas de `rust/path-injection` são High e não são afetadas.

Contexto completo da onda que motivou a mudança (bundle 2.26.3 → 2.26.4,
cobertura de extração de 118 para 422 arquivos):
[`codeql-setup.md`](codeql-setup.md) §"A onda de 2026-08-28".

**A entrada nova desta leva já está registrada**: alerta
[#165](#alert-165), o salt PBKDF2 legado em
`crates/garraia-gateway/src/admin/shared.rs:67` (`LEGACY_KDF_SALT`). O salt
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
version 1.2.0). O script consome o JSON; este `.md` é a versão humana auditável.
**Manter ambos sincronizados** — o script tem flag `--check-md` que valida que
os números de alerta listados em §4 batem com `entries[].alert_number` do JSON.

### Amendment 2026-09-19 — a âncora é o conteúdo, não a linha (issue #1263)

Até aqui a âncora de cada entrada era `(path, line)` e o
`check-ledger-anchors.py` conferia o `sink_snippet` **naquela** linha. Qualquer
commit que inserisse ou removesse linhas *acima* do sink derrubava o gate sem
encostar no statement suprimido. A entrada #113 andou **duas vezes no mesmo
PR** ([#1252](https://github.com/michelbr84/GarraRUST/pull/1252), `:769` →
`:826` → `:867`) sem o snippet mudar um byte — dois ciclos completos de uma
fila de ~110 jobs, para nada. Pior: o caminho mais rápido para ficar verde era
editar o `sink_snippet` até casar com o que estivesse na linha, ou seja,
fraudar o registro. Um guard cuja saída mais cômoda é a fraude depende de
disciplina, não de mecanismo.

Invertido: **o `sink_snippet` é o identificador** e o checker procura por ele no
arquivo, derivando a linha.

- **1 ocorrência** → é ela. A linha derivada é escrita de volta no `.json` e na
  coluna `File:line` deste `.md`, e a saída diz `linha derivada X (antes Y)`.
  Mover `line` não é fraude: é posição derivada, não afirmação de auditoria — é
  justamente por isso que o script faz isso sozinho, e é por isso que editar
  `sink_snippet` à mão continua sendo.
- **0 ocorrências** → **exit 6**, pedindo reauditoria. É o único caso em que o
  gate *deve* ficar vermelho: o statement suprimido mudou ou sumiu.
- **2+ ocorrências** → **exit 5**. O snippet não identifica um statement só, e
  escolher pela posição antiga seria voltar ao problema. A entrada precisa de
  âncora auxiliar, também de conteúdo:

  ```json
  "sink_snippet": "if !file_path.is_file() {",
  "disambiguator": { "kind": "function", "value": "delete_skin" }
  ```

  `kind` hoje só tem um valor, `function` — o nome da `fn` Rust envolvente,
  varrida para cima a partir do candidato. Deliberadamente **não** existe um
  `kind` "n-ésima ocorrência": ordinal reaponta em silêncio no dia em que
  alguém acrescenta um statement idêntico acima, que é a classe de bug que esta
  mudança fecha. Se duas ocorrências caírem na mesma `fn`, o gate acusa
  (exit 5) e pede âncora mais específica em vez de chutar.

  Na migração, **13 das 30 entradas** eram ambíguas (`skills_handler.rs` tem
  quatro `if !skill_path.exists() {` idênticos; `skins_handler.rs` e
  `whatsapp/api.rs`, dois cada). A `fn` envolvente da posição auditada foi
  única em todas as 13. A posição commitada foi confiada **uma vez**, e só
  aqui: o gate anterior exigia que o snippet casasse byte a byte com aquela
  linha, então o `line` daquele commit era um ponteiro verificado. Daí em
  diante manda o conteúdo.
- **Citações `arquivo:linha` dentro da `justification`** passaram a ser
  conferidas. Foi só por falta disso que um drift sobreviveu escondido num
  documento de auditoria de segurança: a justificativa do #113 apontava
  `cloud_secret` em `:630` (estava em `:824`) e o `eprintln!` do vault em
  `:643` (estava em `:837`), e nenhum gate teria pego. Caminho ancorado no repo
  (`crates/...`) que não existe mais, ou linha fora de faixa em arquivo que
  existe, é **exit 6**; nome de arquivo solto que não resolve sai como
  **aviso**, porque prosa não é caminho e o script não adivinha. Citação `:NNN`
  sem arquivo fica fora de escopo de propósito — quase toda ocorrência é
  histórica ("o sink desceu de `:1380` para `:1653`") e validar história por
  faixa é tripwire sem valor.

O job do CI roda com `--no-rewrite`: reporta a posição derivada e não escreve
nada (a árvore do runner é descartada, e "reescrito" ali seria mentira). Quem
consertar roda o checker **sem** a flag, localmente, e commita o `line`
atualizado junto com a mudança que o moveu.

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
4. **Re-audit obrigatório a cada 90 dias.** A data vigente é o campo
   `audit_expiration` do `.json` (esta regra citava `2026-08-01` em texto, e a
   cópia ficou velha por duas renovações). Entradas vencidas devem ser
   revistas; se ainda válidas, renovar com nova justificativa + commit hash; se
   não, abrir fix real. A re-auditoria confere **três** coisas por entrada, e a
   terceira só entrou em 2026-11-25: o código de hoje, o estado do alerta na
   API **incluindo a instância em `refs/heads/main`** (um alerta dispensado
   nunca passa a `fixed`, então `state` sozinho não diz se ele ainda é
   detectado), e o fluxo fonte → sink no SARIF da análise, que é o que diz qual
   pergunta a justificativa precisa responder.
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
| <a id="alert-149"></a>[#149](https://github.com/michelbr84/GarraRUST/security/code-scanning/149) | `rust/path-injection` | `crates/garraia-gateway/src/skins_handler.rs:84` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `create_skin` guards `body.name` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 59 before `tokio::fs::create_dir_all` / `tokio::fs::write`. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skins_test.rs::create_skin_with_path_traversal_returns_400` + `create_skin_with_dot_in_name_returns_400` + `create_skin_rejects_underscore_per_project_convention`. | GAR-490 |
| <a id="alert-147"></a>[#147](https://github.com/michelbr84/GarraRUST/security/code-scanning/147) | `rust/path-injection` | `crates/garraia-gateway/src/skins_handler.rs:111` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `get_skin` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 98 before `format!("{name}.json")`. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skins_test.rs::get_skin_with_dot_in_name_returns_400`. | GAR-490 |
| <a id="alert-148"></a>[#148](https://github.com/michelbr84/GarraRUST/security/code-scanning/148) | `rust/path-injection` | `crates/garraia-gateway/src/skins_handler.rs:141` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `delete_skin` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 128 before `tokio::fs::remove_file`. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skins_test.rs::delete_skin_with_backslash_returns_400`. | GAR-490 |
| <a id="alert-150"></a>[#150](https://github.com/michelbr84/GarraRUST/security/code-scanning/150) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:177` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `get_skill` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 162 before `std::fs::read_to_string`. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::get_skill_rejects_path_traversal`. | GAR-490 |
| <a id="alert-153"></a>[#153](https://github.com/michelbr84/GarraRUST/security/code-scanning/153) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:269` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `create_skill` guards `body.name` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 220 before `std::fs::write`. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::create_skill_rejects_{path_traversal,empty_name,nul_byte,windows_drive}`. | GAR-490 |
| <a id="alert-154"></a>[#154](https://github.com/michelbr84/GarraRUST/security/code-scanning/154) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:344` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `update_skill` double-guards URL `name` + `body.name` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at lines 300 and 307 before `std::fs::write`. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::update_skill_rejects_dot_in_name`. | GAR-490 |
| <a id="alert-151"></a>[#151](https://github.com/michelbr84/GarraRUST/security/code-scanning/151) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:533` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `export_skill` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 518 before `std::fs::read_to_string`. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::export_skill_rejects_dot_in_name`. | GAR-490 |
| <a id="alert-152"></a>[#152](https://github.com/michelbr84/GarraRUST/security/code-scanning/152) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:590` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `set_skill_triggers` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 574 before `std::fs::read_to_string`. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::set_skill_triggers_rejects_dot_in_name`. | GAR-490 |
| <a id="alert-155"></a>[#155](https://github.com/michelbr84/GarraRUST/security/code-scanning/155) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:632` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `set_skill_triggers` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 574 before `std::fs::write` of updated trigger content. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::set_skill_triggers_rejects_dot_in_name`. | GAR-490 |
| <a id="alert-161"></a>[#161](https://github.com/michelbr84/GarraRUST/security/code-scanning/161) | `rust/path-injection` | `crates/garraia-gateway/src/skins_handler.rs:104` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `get_skin` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 98 before `tokio::fs::read_to_string`. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skins_test.rs::get_skin_with_dot_in_name_returns_400`. | GAR-490 |
| <a id="alert-162"></a>[#162](https://github.com/michelbr84/GarraRUST/security/code-scanning/162) | `rust/path-injection` | `crates/garraia-gateway/src/skins_handler.rs:134` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `delete_skin` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 128 before `file_path.is_file()` check. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skins_test.rs::delete_skin_with_backslash_returns_400`. | GAR-490 |
| <a id="alert-156"></a>[#156](https://github.com/michelbr84/GarraRUST/security/code-scanning/156) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:167` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `get_skill` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 162 before `skill_path.exists()` check. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::get_skill_rejects_path_traversal`. | GAR-490 |
| <a id="alert-157"></a>[#157](https://github.com/michelbr84/GarraRUST/security/code-scanning/157) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:227` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `create_skill` guards `body.name` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 220 before `skill_path.exists()` check. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::create_skill_rejects_{path_traversal,empty_name,nul_byte,windows_drive,underscore_per_project_convention}`. | GAR-490 |
| <a id="alert-158"></a>[#158](https://github.com/michelbr84/GarraRUST/security/code-scanning/158) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:312` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `update_skill` double-guards URL `name` + `body.name` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at lines 300 and 307 before `skill_path.exists()` check. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::update_skill_rejects_dot_in_name`. | GAR-490 |
| <a id="alert-159"></a>[#159](https://github.com/michelbr84/GarraRUST/security/code-scanning/159) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:523` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `export_skill` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 518 before `skill_path.exists()` check. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::export_skill_rejects_dot_in_name`. | GAR-490 |
| <a id="alert-160"></a>[#160](https://github.com/michelbr84/GarraRUST/security/code-scanning/160) | `rust/path-injection` | `crates/garraia-gateway/src/skills_handler.rs:579` | dismissed-false-positive | `false_positive` | GAR-490 PR A (PR [#111](https://github.com/michelbr84/GarraRUST/pull/111), squash `613510d`): `set_skill_triggers` guards `Path(name)` via [`validate_skill_name`](../../crates/garraia-gateway/src/path_validation.rs) at line 574 before `skill_path.exists()` check. Charset `[A-Za-z0-9-]{1,128}` ASCII-only. CodeQL Rust pack does not model the helper as a sanitizer. Regression: `tests/skills_test.rs::set_skill_triggers_rejects_dot_in_name`. | GAR-490 |
| <a id="alert-111"></a>[#111](https://github.com/michelbr84/GarraRUST/security/code-scanning/111) | `rust/cleartext-logging` | `crates/garraia-cli/src/config_cmd.rs:222` | dismissed-false-positive | `false_positive` | **Wave 3 — cleartext.** `garraia config check` imprime presenca, nunca valor: o campo `summary.gateway_api_key_set` e `bool` e as unicas strings possiveis sao os literais `"set"` / `"not set"`. A secao sai sob o cabecalho literal `"Summary (redacted)"` (config_cmd.rs:215). CodeQL rastreia o identificador chamado `api_key`, nao o booleano que ele carrega. | GAR-491 |
| <a id="alert-112"></a>[#112](https://github.com/michelbr84/GarraRUST/security/code-scanning/112) | `rust/cleartext-logging` | `crates/garraia-cli/src/config_cmd.rs:245` | dismissed-false-positive | `false_positive` | **Wave 3 — cleartext.** Mesma superficie do [#111](#alert-111): imprime `summary.llm_providers_api_key_set.join(", ")`, que e a lista de **nomes de provider** com chave configurada (ex.: `[openai, anthropic]`), nao as chaves. Invariante de redaction documentado em CLAUDE.md (garraia-config, plan 0046). | GAR-491 |
| <a id="alert-113"></a>[#113](https://github.com/michelbr84/GarraRUST/security/code-scanning/113) | `rust/cleartext-logging` | `crates/garraia-cli/src/wizard/mod.rs:1005` | dismissed-false-positive | `false_positive` | **Wave 3 — cleartext.** `println!("  Cloud provider API key encrypted in vault (entry {entry}).")` imprime o **nome** da entrada do cofre, nunca o segredo. `cloud_secret` e `Option<(&str, &str)>` ([wizard/mod.rs:962](../../crates/garraia-cli/src/wizard/mod.rs)); o destructuring `(entry, key)` manda o `key` exclusivamente para `vault.set(entry, key)` na linha anterior, e o `println!` interpola apenas `entry`. CodeQL contamina `entry` por ele vir do mesmo tuple que `key`. **Re-audit 2026-08-30:** esta entrada apontava para `:640` e descrevia outro statement — o `eprintln!` do `vault open failed`, hoje em `:841`. O alerta vivo sempre esteve no `println!` de `:673`, que e o que [§5.3](#53--os-16-path-injection-foram-renumerados-nao-reabertos-2026-08-29) ja dizia; era o `.json` que estava errado. Ver a nota de re-audit apos esta tabela. **Reancorada 2026-09-16 (PR #1252):** o sink desceu de `:769` para `:826` porque a geracao de credencial em bind exposto acrescentou linhas acima deste ponto. Mesmo statement, mesmo sink, `sink_snippet` intocado. **Reancorada de novo 2026-09-16 (mesmo PR):** o sink desceu de `:826` para `:867` quando a credencial deixou de ser impressa e a escrita com permissao restrita entrou acima deste ponto. As duas citacoes de linha da propria justificativa tambem estavam velhas e foram corrigidas junto: a declaracao de `cloud_secret` esta em `:828` (era `:630`) e o `eprintln!` do vault em `:841` (era `:643`). Mesmo statement, mesmo sink, `sink_snippet` intocado. **Reancorada uma terceira vez 2026-09-16 (merge do #1256 com a main):** o #1256 e o #1252 reancoraram esta MESMA entrada em paralelo — aquele por 4 linhas de roteamento do `garra whatsapp`, este pela geracao de credencial. Depois do merge as duas somam, entao a linha nao e a de nenhum dos dois lados: foi RECALCULADA na arvore mesclada para `:871`. As duas citacoes internas subiram os mesmos 4: `cloud_secret` em `:828` e o `eprintln!` do vault em `:841`. Mesmo statement, mesmo sink, `sink_snippet` intocado. **Re-auditoria 2026-11-25 (#1991):** as citacoes desta justificativa tinham apodrecido de novo sem que o gate acusasse, porque ele so confere que a linha citada existe, nao o que ha nela. Na arvore de hoje: `cloud_secret` e declarado em wizard/mod.rs:962, o `eprintln!` do vault esta em wizard/mod.rs:975, o destructuring `(entry, key)` em wizard/mod.rs:1003 e o sink em wizard/mod.rs:1005. O SARIF da analise de `4d92764b` confirma o fluxo que a entrada descreve: fonte `cloud_secret`, um passo pelo `Some(...)` do destructuring, sink no `println!`. | GAR-491 |
| <a id="alert-144"></a>[#144](https://github.com/michelbr84/GarraRUST/security/code-scanning/144) | `rust/cleartext-transmission` | `crates/garraia-channels/src/signal/mod.rs:228` | dismissed-false-positive | `false_positive` | Falso-positivo. O que a regra acusa: `rust/cleartext-transmission` segue dado classificado como sensivel ate a URL de uma requisicao — aqui `config.phone_number`, interpolado em `format!("{}/v1/receive/{}", ...)` (crates/garraia-channels/src/signal/mod.rs:222) e entregue ao `client.get(&url)` desta linha. O esquema da URL nao entra na query: o exemplo BAD da documentacao da regra e uma URL `https://`. Duas coisas sustentam a dispensa, e as duas sao necessarias. (1) TRANSPORTE: o GET so roda depois de `connect()` chamar `self.ensure_url_vetted()?` (crates/garraia-channels/src/signal/mod.rs:200), e `vet_signal_cli_url` (crates/garraia-channels/src/signal/mod.rs:133) aceita `https` em qualquer host e `http` apenas quando todos os enderecos resolvidos sao loopback. O `config` e clonado antes do `tokio::spawn` (crates/garraia-channels/src/signal/mod.rs:206) e `SignalConfig` nao tem mutabilidade interior, entao a URL do loop e o valor ja validado. O POST de resposta do mesmo spawn passa por `api::send_text` (crates/garraia-channels/src/signal/mod.rs:302), que roda o mesmo guard. (2) O DADO: o numero no caminho e o do proprio bot do operador, vindo da config, e esta ali porque e o contrato da signal-cli-rest-api (`GET /v1/receive/{number}`) — nao ha como usar o endpoint sem ele, e nenhum numero de terceiro compoe a URL. LIMITE CONHECIDO do item 1: o veredito e memoizado e a resolucao nao e pinada — um `http://nome` que resolvia para loopback na checagem e passe a resolver para fora depois nao e reavaliado (o guard de voz, em crates/garraia-gateway/src/voice_chat.rs, fecha isso aceitando `http` so para IP literal). E endurecimento, nao achado: exige o operador configurar `http://` por nome e alguem controlar a resolucao desse nome. A SUPRESSAO DEIXA DE VALER se o guard passar a aceitar `http` fora de loopback, ou se um numero que nao seja o do proprio bot passar a compor a URL. Re-auditoria 2026-11-25 (#1991): a versao anterior argumentava so o item 1 e dizia que 'CodeQL nao liga o clone a validacao anterior'. O SARIF da analise de `4d92764b` mostra outra coisa: a fonte e `config.phone_number` e a validacao de esquema nunca fez parte do fluxo, porque a regra nao olha esquema — foi por isso que o guard, acrescentado em 2026-08-30, nao fechou o alerta. Sairam tambem as referencias a 'este PR' e ao alerta em `:279`, que descreviam a arvore daquele dia. Contraste com o #143, mesma regra e mesmo arquivo, que nao entrou neste ledger: era codigo morto (`receive_messages`, zero call sites) e foi removido. | GAR-491 |
| <a id="alert-145"></a>[#145](https://github.com/michelbr84/GarraRUST/security/code-scanning/145) | `rust/cleartext-transmission` | `crates/garraia-channels/src/whatsapp/api.rs:48` | dismissed-false-positive | `false_positive` | Falso-positivo. O que a regra acusa: `rust/cleartext-transmission` segue dado classificado como sensivel ate a URL de uma requisicao, e a fonte no SARIF e o parametro `phone_number_id`, interpolado no caminho de `format!("{GRAPH_API_BASE}/{phone_number_id}/messages")`. A heuristica casa o nome `phone_number`; o valor nao e um numero de telefone. `phone_number_id` e o identificador opaco que a Meta atribui ao numero da conta Business, vem da config do canal autenticado, e a Graph API exige que ele esteja no caminho (`POST /{phone-number-id}/messages`). O numero do destinatario (`to`) e o texto vao no corpo JSON e o token vai no header por `.bearer_auth(token)` — nenhum deles compoe a URL. O destino e HTTPS por constante de compilacao: `const GRAPH_API_BASE: &str = "https://graph.facebook.com/v21.0"` (crates/garraia-channels/src/whatsapp/api.rs:5), sem caminho de config que altere esquema ou host. A SUPRESSAO DEIXA DE VALER se um numero de telefone de verdade, ou qualquer segredo, passar a compor a URL. Re-auditoria 2026-11-25 (#1991): a versao anterior argumentava so que o destino e HTTPS e que 'CodeQL nao dobra a const dentro do `format!`'. Isso responde a `rust/non-https-url`, que e outra regra; esta nao olha o esquema — o exemplo BAD da documentacao dela e uma URL `https://`. | GAR-491 |
| <a id="alert-146"></a>[#146](https://github.com/michelbr84/GarraRUST/security/code-scanning/146) | `rust/cleartext-transmission` | `crates/garraia-channels/src/whatsapp/api.rs:79` | dismissed-false-positive | `false_positive` | Identico ao [#145](#alert-145), em `mark_as_read`: a fonte no SARIF e o mesmo `phone_number_id` no caminho da mesma URL — identificador opaco da conta Business, exigido ali pela Graph API. O `message_id` vai no corpo JSON e o token no header. Mesma const `GRAPH_API_BASE` HTTPS (crates/garraia-channels/src/whatsapp/api.rs:5). Re-auditoria 2026-11-25 (#1991): corrigida junto com o #145, porque o argumento de so-HTTPS respondia a outra regra. A versao anterior tambem dizia que os alertas 143/144 do canal Signal 'nao eram falso-positivo' e que a URL 'foi corrigida em codigo': o #143 saiu como codigo morto, e o #144 continua vivo e dispensado neste ledger — o guard de URL que entrou naquele dia nao e o que a regra observa (ver #144). | GAR-491 |
| <a id="alert-165"></a>[#165](https://github.com/michelbr84/GarraRUST/security/code-scanning/165) | `rust/hard-coded-cryptographic-value` | `crates/garraia-gateway/src/admin/shared.rs:67` | dismissed-wont-fix | `wont_fix` | LEGACY_KDF_SALT. Nao e falso-positivo nem fixture: o CodeQL esta certo, e um salt PBKDF2 constante. Ele permanece no fonte porque a migracao forward-only do PR #869 precisa dele para decifrar uma ultima vez os segredos gravados por instalacoes anteriores a 2026-08-29, antes de re-cifra-los sob a chave derivada do salt aleatorio por instalacao; os parametros novos vivem na tabela kdf_params do admin.db, gravados na mesma transacao que re-cifra os segredos. CORRECAO DA RE-AUDITORIA DE 2026-11-25 (#1991): ate aqui esta entrada afirmava 'Nenhuma chave nova e derivada dele'. E FALSO. `derive_with_passphrase` (crates/garraia-gateway/src/admin/shared.rs:185) usa a constante em tres pontos, e so um deles e a migracao. (a) crates/garraia-gateway/src/admin/shared.rs:218, arm de migracao pendente: deriva a chave `legacy`, usada so para decifrar. E o uso que a entrada descrevia. (b) crates/garraia-gateway/src/admin/shared.rs:204, parametros gravados ilegiveis (base64 invalido ou zero iteracoes): a chave `current`, a que CIFRA, sai do salt constante, com `legacy: None` e `pending: None` — sem migracao a caminho. (c) crates/garraia-gateway/src/admin/shared.rs:237, falha ao gerar o salt por instalacao: idem. Nos estados (b) e (c) todo segredo novo gravado pelo admin e cifrado sob a chave do salt global, com 100k iteracoes, e o unico sinal e um `warn!`. O teste `unusable_stored_params_fall_back_to_legacy_not_to_a_third_key` afirma exatamente isso (`keys.current == legacy`). O estado (b) tem um segundo problema: parametros gravados significam que o re-key ja commitou, entao os segredos existentes estao sob a chave NOVA e a chave legada nao abre nenhum deles. ALCANCE: (b) exige a linha de kdf_params corrompida, ou adulterada por quem ja escreve no admin.db; (c) exige o CSPRNG do sistema falhar. Nenhum dos dois e caminho normal. O QUE ESTA DISPENSA COBRE: a existencia da constante, para a migracao — disposicao `wont_fix`, que continua correta. O QUE ELA NAO COBRE: os dois fallbacks, que sao comportamento a corrigir (falhar fechado em vez de cifrar sob o salt constante) e estao na issue #1992, junto com o doc comment da propria constante, que repete a afirmacao falsa ('No new derivation ever uses these'). A constante so pode sair quando nao houver mais instalacoes por migrar, o que este repositorio nao tem como afirmar. | PR-869 |

| <a id="alert-176"></a>[#176](https://github.com/michelbr84/GarraRUST/security/code-scanning/176) | `rust/cleartext-logging` | `crates/garraia-cli/src/whatsapp.rs:1400` | dismissed-false-positive | `false_positive` | **PR [#1256](https://github.com/michelbr84/GarraRUST/pull/1256) (#1238).** Mensagem de sucesso do `garra whatsapp link`: o que chega ao stdout é um **caminho de diretório**, não credencial e não PII — `<data_dir>/whatsapp/<account>/`, com `<data_dir>` saindo de `AppConfig::resolved_data_dir()` (a máquina do próprio usuário). A entrada faz **duas** afirmações, e cada uma tem prova própria. **(1) O caminho não sai do data dir**, porque `for_data_dir` devolve `Result` e recusa (`SessionError::InvalidAccount`) qualquer conta que não seja um segmento `[A-Za-z0-9_-]{1,64}` — allowlist que fecha `..`, `/`, `\`, `.`, vazio, NUL e espaço de uma vez. **E `for_data_dir` é o único construtor que existe fora da crate**: a irmã `SessionStore::new`, que aceita qualquer `PathBuf` e não valida nada, é `pub(crate)` desde a revisão R4 — quem impede a CLI de contorná-la é o rustc, não a disciplina de quem escreve o call site. Regressão da *declaração*: `the_store_exposes_a_single_validating_constructor` ([source_scan.rs](../../crates/garraia-channels/src/whatsapp_linked/source_scan.rs)), que fica vermelho no dia em que `new` voltar a ser `pub`. Regressão: `an_account_that_escapes_the_data_dir_is_refused_before_any_write` ([session.rs](../../crates/garraia-channels/src/whatsapp_linked/session.rs)), que no ramo em que a validação aceitasse **grava** a sessão cifrada e afirma a contenção no data dir. **(2) O segmento impresso é a constante `DEFAULT_ACCOUNT`** — um número de telefone casaria a regra de (1) e seria PII no stdout, então validar não basta aqui. Regressão: `every_session_store_in_the_cli_uses_the_constant_account` ([whatsapp/tests.rs](../../crates/garraia-cli/src/whatsapp/tests.rs)), que varre **a árvore inteira** de `crates/garraia-cli/src` lida do disco e exige que o segundo argumento de cada `for_data_dir` seja literalmente `DEFAULT_ACCOUNT`, lendo o argumento até o parêntese que fecha a chamada — `account.unwrap_or(DEFAULT_ACCOUNT)` é reprovado, e o parser tem teste próprio. ⚠️ **Reauditada 2026-09-16 (revisão R4):** a versão anterior desta linha afirmava que o invariante estava pinado por um scan de janela de 160 caracteres num arquivo só; ele aprovava conta dinâmica com a constante como *fallback* e não enxergava call site em outro arquivo, e o `for_data_dir` não validava nada — o path traversal estava fechado por acidente. Guarda textual → validação estrutural. **Por que supressão e não correção:** o mesmo segmento já aparece em outros **dois** `println!` do comando — `store.blob_path()` no `status` e `store.archive_path()` no aviso de arquivado — e nenhum dos dois foi sinalizado; apagá-lo só neste sink calaria o scanner sem mudar nada do que ele teme, e dizer onde a sessão foi gravada é o motivo de a mensagem existir. **Deixa de valer** se a CLI passar a escolher conta em runtime: é o teste (2) que fica vermelho nesse dia. **Reancorada 2026-09-16 (PR #1256):** o sink desceu de `:631` para `:807` porque as revisões R3/R4 acrescentaram acima deste ponto o tratamento de erro do `for_data_dir`, o `ArchiveGuard` e a extração de `link_paired` — mesmo statement, `sink_snippet` intocado. **Reancorada de novo 2026-09-17 (PR #1256, rodada 4):** desceu de `:807` para `:994` — a rodada acrescentou acima dele o prazo do handshake, a saída injetável do `ArchiveGuard` e o comando `garra whatsapp restore`. Mesmo statement, `sink_snippet` intocado. **Reancorada de novo 2026-09-17 (PR #1256, rodada 5):** desceu de `:994` para `:1017` — a rodada acrescentou acima dele a prova de que o blob restaurado abre antes de `restore` ligar o canal. Mesmo statement, `sink_snippet` intocado. **Reancorada de novo 2026-09-17 (PR #1256, rodada 6):** desceu de `:1017` para `:1018` — a rodada inverteu a ordem daquela prova, que agora roda ANTES de mover o arquivado. Mesmo statement, `sink_snippet` intocado. **Reancorada de novo 2026-09-17 (PR #1256, rodada 9):** desceu de `:1018` para `:1067` — entrou acima dele o `needs_a_terminal`, a mensagem que `link` e `cloud` passaram a imprimir sem terminal, no lugar de repetirem o texto do menu e sairem 0. Linha **recalculada** nesta arvore, nao copiada. Mesmo statement, `sink_snippet` intocado. **Reancorada de novo 2026-09-17 (PR #1256, rodada 10):** desceu de `:1067` para `:1070` — entrou acima dele o pulso `PairUi::connecting`, que da sinal de vida enquanto a ponte esta muda antes do primeiro QR. Linha **recalculada**, nao copiada. Mesmo statement, `sink_snippet` intocado. **Reancorada de novo 2026-09-17 (PR #1265, fatia D):** desceu de `:1070` para `:1107` — o merge da fatia D entrou acima dele a integração de saúde do `garra whatsapp status`: o `use` do módulo `health`, `DiskFacts::read` + `classify`, o bloco de `MissingDependencies` e a função `print_next_step`. Linha **recalculada** nesta árvore com grep, não copiada. Mesmo statement, `sink_snippet` intocado. ⚠️ Número a reconferir após o merge — 173 foi atribuído na análise da branch. **Reapontada 2026-09-25 (#173 → #176):** confirmado depois do merge da v0.4.5 — o GitHub abriu o alerta **#176** para o mesmo sink (span `1356-1358`, sink em `:1358`; o `println!` virou multi-linha com o rustfmt) e o #173 ficou dispensado numa duplicata aposentada. Reapontamento feito pelo `codeql-rekey-ledger.py`, que passou a casar a linha do sink **dentro do span** do alerta, como o `codeql-reapply-dismissals.sh` já fazia. Mesmo statement, `sink_snippet` intocado. | PR-1256 |
| <a id="alert-181"></a>[#181](https://github.com/michelbr84/GarraRUST/security/code-scanning/181) | `rust/path-injection` | `crates/garraia-gateway/src/rest_v1/uploads.rs:1081` | dismissed-false-positive | `false_positive` | Falso positivo em `finalize_upload`. Falso positivo: `staging_path` junta `staging_dir` (canonicalizado no bootstrap) com `format!("{upload_id}.staging")` e `upload_id: Uuid` tipado (crates/garraia-gateway/src/rest_v1/uploads.rs:197) — os handlers recebem `Path(upload_id): Path<Uuid>` (crates/garraia-gateway/src/rest_v1/uploads.rs:656 em `patch_upload`, crates/garraia-gateway/src/rest_v1/uploads.rs:875 em `delete_upload`) e o extractor do axum recusa valor que nao parseie como UUID (400 antes do handler). Forma canonica de 36 chars `[0-9a-f-]`, sem traversal. CodeQL nao modela `Path<Uuid>` como sanitizer. Regressao: `uploads.rs::staging_path_is_traversal_safe` (crates/garraia-gateway/src/rest_v1/uploads.rs:1538). | GAR-491 |
| <a id="alert-182"></a>[#182](https://github.com/michelbr84/GarraRUST/security/code-scanning/182) | `rust/path-injection` | `crates/garraia-gateway/src/rest_v1/uploads.rs:1093` | dismissed-false-positive | `false_positive` | Falso positivo em `finalize_upload`. Falso positivo: `staging_path` junta `staging_dir` (canonicalizado no bootstrap) com `format!("{upload_id}.staging")` e `upload_id: Uuid` tipado (crates/garraia-gateway/src/rest_v1/uploads.rs:197) — os handlers recebem `Path(upload_id): Path<Uuid>` (crates/garraia-gateway/src/rest_v1/uploads.rs:656 em `patch_upload`, crates/garraia-gateway/src/rest_v1/uploads.rs:875 em `delete_upload`) e o extractor do axum recusa valor que nao parseie como UUID (400 antes do handler). Forma canonica de 36 chars `[0-9a-f-]`, sem traversal. CodeQL nao modela `Path<Uuid>` como sanitizer. Regressao: `uploads.rs::staging_path_is_traversal_safe` (crates/garraia-gateway/src/rest_v1/uploads.rs:1538). | GAR-491 |
| <a id="alert-183"></a>[#183](https://github.com/michelbr84/GarraRUST/security/code-scanning/183) | `rust/path-injection` | `crates/garraia-gateway/src/rest_v1/uploads.rs:1248` | dismissed-false-positive | `false_positive` | Falso positivo em `append_to_staging` (crates/garraia-gateway/src/rest_v1/uploads.rs:1244): o parametro vem do chamador via `staging.staging_path(upload_id)` (crates/garraia-gateway/src/rest_v1/uploads.rs:755, crates/garraia-gateway/src/rest_v1/uploads.rs:979, crates/garraia-gateway/src/rest_v1/uploads.rs:1075), nunca de dado cru. Falso positivo: `staging_path` junta `staging_dir` (canonicalizado no bootstrap) com `format!("{upload_id}.staging")` e `upload_id: Uuid` tipado (crates/garraia-gateway/src/rest_v1/uploads.rs:197) — os handlers recebem `Path(upload_id): Path<Uuid>` (crates/garraia-gateway/src/rest_v1/uploads.rs:656 em `patch_upload`, crates/garraia-gateway/src/rest_v1/uploads.rs:875 em `delete_upload`) e o extractor do axum recusa valor que nao parseie como UUID (400 antes do handler). Forma canonica de 36 chars `[0-9a-f-]`, sem traversal. CodeQL nao modela `Path<Uuid>` como sanitizer. Regressao: `uploads.rs::staging_path_is_traversal_safe` (crates/garraia-gateway/src/rest_v1/uploads.rs:1538). | GAR-491 |
| <a id="alert-184"></a>[#184](https://github.com/michelbr84/GarraRUST/security/code-scanning/184) | `rust/path-injection` | `crates/garraia-gateway/src/rest_v1/uploads.rs:811` | dismissed-false-positive | `false_positive` | Falso positivo em `patch_upload`. Falso positivo: `staging_path` junta `staging_dir` (canonicalizado no bootstrap) com `format!("{upload_id}.staging")` e `upload_id: Uuid` tipado (crates/garraia-gateway/src/rest_v1/uploads.rs:197) — os handlers recebem `Path(upload_id): Path<Uuid>` (crates/garraia-gateway/src/rest_v1/uploads.rs:656 em `patch_upload`, crates/garraia-gateway/src/rest_v1/uploads.rs:875 em `delete_upload`) e o extractor do axum recusa valor que nao parseie como UUID (400 antes do handler). Forma canonica de 36 chars `[0-9a-f-]`, sem traversal. CodeQL nao modela `Path<Uuid>` como sanitizer. Regressao: `uploads.rs::staging_path_is_traversal_safe` (crates/garraia-gateway/src/rest_v1/uploads.rs:1538). | GAR-491 |
| <a id="alert-185"></a>[#185](https://github.com/michelbr84/GarraRUST/security/code-scanning/185) | `rust/path-injection` | `crates/garraia-gateway/src/rest_v1/uploads.rs:980` | dismissed-false-positive | `false_positive` | Falso positivo em `delete_upload`. Falso positivo: `staging_path` junta `staging_dir` (canonicalizado no bootstrap) com `format!("{upload_id}.staging")` e `upload_id: Uuid` tipado (crates/garraia-gateway/src/rest_v1/uploads.rs:197) — os handlers recebem `Path(upload_id): Path<Uuid>` (crates/garraia-gateway/src/rest_v1/uploads.rs:656 em `patch_upload`, crates/garraia-gateway/src/rest_v1/uploads.rs:875 em `delete_upload`) e o extractor do axum recusa valor que nao parseie como UUID (400 antes do handler). Forma canonica de 36 chars `[0-9a-f-]`, sem traversal. CodeQL nao modela `Path<Uuid>` como sanitizer. Regressao: `uploads.rs::staging_path_is_traversal_safe` (crates/garraia-gateway/src/rest_v1/uploads.rs:1538). | GAR-491 |
**Total**: 30 entries (5 from GAR-491 Wave 2 + 16 from GAR-490 Wave 1 PR A +
1 from PR [#869](https://github.com/michelbr84/GarraRUST/pull/869) + 5 from
Wave 3 cleartext + 2 da triagem de 2026-08-30, #115 e #144 + 1 do PR
[#1256](https://github.com/michelbr84/GarraRUST/pull/1256)).

> **+8 na auditoria CodeQL de 2026-10-10 (alertas #181–#188).** Três em
> `crates/garraia-agents/src/tools/session_workspace.rs` (#186–#188) e cinco em
> `crates/garraia-gateway/src/rest_v1/uploads.rs` (#181–#185), todos `rust/path-injection`
> `severity=high`. Veredito por entrada: falso-positivo — o dado nao-confiavel (session_id,
> upload-id de URL) so alcanca o filesystem como SHA-256-hex de 32 chars ou UUID canonico,
> e nenhum dos dois charset expressa traversal; o CodeQL Rust pack nao modela nenhuma das
> duas transformacoes como sanitizer. Regressao nova: `o_nome_do_subdiretorio_e_sempre_hex_de_32_nunca_escapa_charset`.
> **Removida em 2026-10-10 (mesma missão):** a entrada #179 saiu do ledger porque o
> alerta vivo está `state=fixed` — o `--apply` trata entrada de alerta resolvido como
> `exit 3` (stale) e aborta fail-closed; remoção é o caminho (precedente: #143).
> **Total hoje: 40 entries** (33 + 8 − 1).

> **Total em 2026-11-25: 29 entries.** As contas acima são históricas e pararam
> de fechar: a tabela chegou a 43 linhas (40 + #195–#197 do PR
> [#1809](https://github.com/michelbr84/GarraRUST/pull/1809)) sem que nenhum
> dos totais fosse atualizado. A re-auditoria de 2026-11-25 aposentou 14, e o
> número que vale é o que o `--check-md` imprime: 16 `rust/path-injection` de
> `skills_handler.rs`/`skins_handler.rs`, 5 `rust/path-injection` de
> `uploads.rs`, 4 `rust/cleartext-logging`, 3 `rust/cleartext-transmission` e
> 1 `rust/hard-coded-cryptographic-value`. São exatamente os 29 resultados da
> análise Rust do `main` em `4d92764b`.

> **Correção de contagem, 2026-09-16 (PR #1256).** A `main` declarava **31
> entries** com **29 linhas** na tabela, e a parcela "GAR-491 Wave 2" dizia 6
> onde há 5 — a soma nunca fechou. Este PR acrescentou uma entrada (#173) e
> corrigiu a declaração para 30, que é o número que a tabela sustenta
> (5 + 16 + 1 + 5 + 2 + 1 = 30). O `31 → 30` no diff, portanto, **não** é uma
> entrada removida: é a soma passando a bater. Registrado porque num ledger de
> alertas de segurança dispensados uma contagem que muda calada é exatamente a
> espécie de número que ninguém reconfere.
> **Sink novo a esperar depois do merge, 2026-09-17 (PR #1256, rodada 6).**
> `crates/garraia-channels/src/whatsapp_linked/session.rs:694` ganhou um
> `tracing::warn!(caminho = %tmp.display(), …)` — a rodada 5 o acrescentou
> para que a falha ao triturar o temporário da sessão deixasse rastro em vez
> de sumir em silêncio. **Ele tem a mesma forma do #173**: o que vai para o
> log é um *caminho*, e o conteúdo (o blob cifrado) nunca. O ledger foi
> reancorado só para o #173, porque é o único alerta que a análise da branch
> emitiu até aqui; a análise pós-merge roda sobre outra base e **pode** abrir
> um alerta `rust/cleartext-logging` novo neste segundo sink. Fica registrado
> aqui para que ele chegue como coisa prevista, com a justificativa já pronta,
> e não como surpresa a ser despachada às pressas. Quem o triar: o argumento é
> o mesmo do #173 — caminho sob o data dir da própria máquina, conta fixa em
> `DEFAULT_ACCOUNT` — e a diferença é que aqui nem o segmento de conta é
> escolhido em runtime, porque `tmp` é derivado de `blob_path()`.

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

### Re-auditoria de mérito 2026-11-25 — 43 entradas auditadas, 29 ficam

Issue [#1991](https://github.com/michelbr84/GarraRUST/issues/1991), sobre a
`main` em `4d92764b` (análise Rust do CodeQL 2.27.6, 29 resultados). A passagem
anterior, de 2026-08-30, cobriu 29 entradas; as 14 que entraram depois dela
(#176, #177, #178, #181–#188, #195–#197) nunca tinham passado por uma.

**Quem auditou.** A re-auditoria foi conduzida por um agente (Claude Code) a
pedido do dono do repositório. As 43 justificativas foram divididas entre três
execuções independentes do agente `security-auditor`, cada uma com a instrução
de **falsificar** a justificativa, não de confirmá-la, e sem acesso às
conclusões das outras. Toda correção que entrou neste ledger foi reconferida no
código antes de ser escrita — o que pegou duas citações erradas que um dos
pareceres tinha dado como exatas (#151 e #159). A revisão humana é a do PR.

**O que esta passagem conferiu que as anteriores não.** Além do código de hoje,
duas fontes novas:

1. **A instância de cada alerta em `refs/heads/main`**
   (`GET /code-scanning/alerts/{n}/instances`). O `state` do alerta não serve:
   um alerta dispensado continua `dismissed` para sempre (§5.2). A instância
   diz `open` quando a análise ainda o detecta e `fixed` quando não.
2. **O SARIF das análises**, com o fluxo fonte → sink de cada resultado. É o
   que diz qual pergunta a justificativa precisa responder — e em quatro
   entradas a justificativa respondia outra.

#### As 29 que ficam

| Entradas | Veredito | O que foi conferido |
|---|---|---|
| #147–#162 (16, `rust/path-injection`) | Procede | `validate_skill_name` (`path_validation.rs:76`) segue allow-list `[A-Za-z0-9-]{1,128}` ASCII, e roda antes de qualquer uso do nome no filesystem nos 8 handlers com entrada, com `return` no erro. No SARIF a fonte é o parâmetro `Path`/`Json` do handler. `import_skill`, que não tem entrada, também passa pelo guard (`skills_handler.rs:455`). **Sete citações de linha do guard estavam erradas por 1** — apontavam para o `return`, não para a chamada: #149 (`:60` → `:59`), #147 e #161 (`:99` → `:98`), #148 e #162 (`:129` → `:128`), #151 e #159 (`:519` → `:518`). |
| #181–#185 (`rust/path-injection`, `uploads.rs`) | Procede | No SARIF a fonte é `Path(upload_id): Path<Uuid>` e o caminho é `staging_dir.join(format!("{upload_id}.staging"))`, como a justificativa diz. Citações exatas. |
| #111, #112 (`rust/cleartext-logging`) | Procede | `gateway_api_key_set` é `bool` (`check.rs:96`); a lista do #112 é montada em `check.rs:261` a partir do mapa `llm`, e o `.map` devolve só o nome de cada provider. O cabeçalho `"Summary (redacted)"` está em `config_cmd.rs:215`, não `:206`. |
| #113 (`rust/cleartext-logging`) | Procede | Fonte `cloud_secret`, sink no `println!` que só interpola `entry`. As citações internas tinham apodrecido de novo (`:828` → `:962`, `:841` → `:975`). |
| #176 (`rust/cleartext-logging`) | Procede | Fonte `account` (`session.rs:401`), que é o que a justificativa discute. `validate_account`, `SessionStore::new` `pub(crate)` e os quatro testes citados existem. O `line` estava em `:1367` com o sink em `:1400` — ver "O que estava quebrado sem ninguém ver". |
| #144, #145, #146 (`rust/cleartext-transmission`) | Procede, **justificativa reescrita** | Ver abaixo. |
| #165 (`rust/hard-coded-cryptographic-value`) | Procede como `wont_fix`, **afirmação falsa corrigida** | Ver abaixo. |

**#144, #145, #146 — a justificativa respondia a outra regra.**
`rust/cleartext-transmission` segue dado classificado como sensível até a URL
de uma requisição. O esquema não entra na query: o exemplo "BAD" da
documentação da regra é
`reqwest::get(format!("https://example.com/card/{creditCardNo}"))`. As três
entradas argumentavam que o destino é HTTPS (ou que o guard recusa `http://`
remoto), que é o argumento contra `rust/non-https-url`. O SARIF mostra as
fontes reais: `config.phone_number` no #144 e `phone_number_id` no #145/#146.
As três continuam falso-positivo, agora pela razão certa — no Signal, o número
é o do próprio bot e está no caminho por contrato da signal-cli-rest-api, com o
transporte restrito a `https` ou loopback; no WhatsApp, `phone_number_id` é um
identificador opaco da Meta, não um telefone. Isso também explica um fato que o
ledger registrava sem entender: o guard `vet_signal_cli_url` entrou em
2026-08-30 "corrigindo" o alerta, e o alerta não fechou.

**#165 — a justificativa afirmava o contrário do código.** "Nenhuma chave nova
é derivada dele" é falso: dois ramos de fallback de `derive_with_passphrase`
(`admin/shared.rs:204`, parâmetros gravados ilegíveis; `:237`, falha ao gerar o
salt) derivam do salt constante a chave que **cifra**, sem migração pendente. A
disposição `wont_fix` da constante continua correta — a migração precisa dela
—, mas a dispensa não cobre os fallbacks, que viraram a issue
[#1992](https://github.com/michelbr84/GarraRUST/issues/1992). É o desfecho que
a §3 regra 4 prevê para justificativa que não procede: fix real, não renovação.

#### As 14 aposentadas

Todas têm a instância de `refs/heads/main` em `fixed`. Os números abaixo são
texto simples de propósito: o `--check-md` conta todo link de alerta do arquivo.

| # | Regra | Arquivo | Última análise que detectou (UTC) | Por que deixou de ser detectado |
|---|---|---|---|---|
| 40, 41, 42 | `rust/hard-coded-cryptographic-value` | `mobile_auth.rs` | `587d434f`, 2026-08-29 | O `-test` tirou `cfg(test)` do escopo da análise (§1, Amendment 2026-08-29). As fixtures seguem dentro de `#[cfg(test)]`. |
| 44, 45 | `rust/hard-coded-cryptographic-value` | `validation.rs` | `587d434f`, 2026-08-29 | Idem. |
| 115 | `rust/cleartext-storage-database` | `session_store.rs` | `3d4f4721`, 2026-09-21 | **Corrigido em código** pela #1309, que trocou o SQL montado por `format!` por literal com bind. E a justificativa descrevia o fluxo errado: dizia que o CodeQL seguia o `token`; o SARIF mostra `session_idle_secs` → `format!` → `.prepare(&sql)`. A entrada foi "reancorada" no próprio PR que matou o alerta e de novo no trem de merge seguinte. |
| 177, 178 | `rust/path-injection` | `whatsapp_linked/bridge.rs` | `2a6e9aed`, 2026-09-26 | Sumiram no merge do trem v0.4.6-f (`f88340ae`), junto com o #179: os três tinham o mesmo fluxo (`State` do `diagnostics_handler` → `resolved_data_dir()` → `deps_pending`). O #179 estava `open`, virou `fixed` e saiu do ledger em 2026-10-10; estes dois estavam `dismissed` e ficaram para trás. |
| 195, 196, 197 | `rust/cleartext-transmission` | `voice_chat.rs` | `1afbf05b`, 2026-11-03 | Sumiram na troca do CodeQL 2.27.3 → 2.27.4, sem mudança no arquivo. O fluxo era a struct `Remote` inteira contaminada por `resolve_api_key` e lida como `remote.base` — a chave nunca esteve na URL, que é o que a justificativa dizia. |
| 186, 187, 188 | `rust/path-injection` | `session_workspace.rs` | `4bb76932`, 2026-11-20 | Sumiram na troca do CodeQL 2.27.4 → 2.27.5, sem mudança no arquivo. A fonte era o `State` do handler `chat_completions` chegando a `self.raiz`, não o `session_id` que a justificativa discutia. |

O mérito das 14 também foi auditado antes da saída, e nenhuma virou achado: as
fixtures continuam em `#[cfg(test)]`, só o hash do token chega ao banco, o
diretório da ponte vem só do `data_dir` da config, a raiz do workspace de sessão
vem só do boot, e a chave de voz vai só no header.

**Por que saem em vez de ficar "dispensadas e quietas".** Três razões, em ordem
de peso:

1. É a regra do topo deste arquivo, e é o precedente do #43, do #143 e do #179.
2. O ledger justifica dispensas que estão **em vigor**. Estas não suprimem
   nada: não há resultado para suprimir.
3. Os dois scripts não conseguem concordar sobre elas. O
   `check-ledger-anchors.py` deriva o `line` do código de hoje; o
   `codeql-reapply-dismissals.sh` exige que esse `line` caia no span do alerta
   na API — e o span de um alerta que não é mais detectado fica congelado na
   última análise que o viu. No #115 eram `:2237` contra `:1711`. Qualquer um
   que rodasse o checker sem `--no-rewrite`, como a §2 manda, quebraria o
   `--apply` na primeira entrada da tabela.

**O que a saída não faz.** Os 14 alertas continuam `dismissed` no GitHub, como
o #43 e os outros 52 dispensados sem entrada (na maior parte duplicatas
aposentadas por renumeração e fixtures dispensadas em 2026-05-12). Se uma versão futura do
CodeQL voltar a detectar um deles com o mesmo fingerprint, ele reaparece já
dispensado e sem linha neste ledger. O fecho correto é reabri-los
(`gh api -X PATCH repos/michelbr84/GarraRUST/code-scanning/alerts/<N> -f state=open`),
para que passem a `fixed` e uma eventual volta chegue como alerta aberto. Isso
**não foi feito** nesta passagem: é escrita em alerta de segurança, e fica para
decisão do dono.

#### O que estava quebrado sem ninguém ver

- **`codeql-reapply-dismissals.sh --dry-run` saía com `exit 2` na `main`**, na
  entrada #176 (`line` 1367, span do alerta [1398,1400]). O laço aborta ali, e
  as 13 entradas seguintes não eram conferidas. O `line` apodreceu porque o job
  do CI roda o checker com `--no-rewrite`, que só avisa. Com o ledger desta
  passagem: `skipped: 29 (already-dismissed)`, `errors: 0`, `exit 0`.
- **A conferência de citações só olha a faixa.** `check_citations` confirma que
  a linha citada existe, não o que há nela; dez entradas citavam linha errada e
  passavam.
- **Contagens e datas em prosa** estavam todas velhas: "Last updated" (três
  meses), os três totais de §4 e o `2026-08-01` da §3 regra 4.

As duas primeiras, mais a cegueira para alerta que deixou de ser detectado, são
lacuna de ferramenta e estão na issue
[#1993](https://github.com/michelbr84/GarraRUST/issues/1993). Enquanto ela não
fechar, a próxima re-auditoria tem de repetir à mão as três conferências da §3
regra 4.


## §5. Empirical validation

### §5.1 — Persistência do dismissal entre re-análises (2026-05-01)

**Hypothesis**: dismissals via REST API persistem entre re-análises CodeQL
no mesmo repositório; o `state=dismissed` não é resetado quando o workflow
`codeql.yml` re-roda na mesma branch (ou após merge para `main`).

**Procedure**:

1. Aplicar dismissal apenas no alerta #43 (`credentials.rs:49`,
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
| `rust/cleartext-logging` | #111, #112 | `config_cmd.rs:213,236` | Imprime `"set"`/`"not set"` e a **lista de nomes** de provider com chave — nunca o valor. É o invariante de redaction do plan 0046. |
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
