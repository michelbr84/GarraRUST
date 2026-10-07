# Coverage Baseline — outubro 2026 (#1566)

Sucessor de [`coverage-baseline-2026-04.md`](coverage-baseline-2026-04.md),
que descreve o job e a política de exclusões e **intencionalmente não fixava
números**. Este documento fixa números: é o baseline versionado que a #1566
pede, para que a cobertura deixe de viver só em artifact de 14 dias.

## Medição

| | |
|---|---|
| Commit | `13c0ce5ad7573a542b4aed08a031f6e14883043e` (`main`) |
| Data | 2026-10-05 |
| Run de CI | [`37273592712`](https://github.com/michelbr84/GarraRUST/actions/runs/37273592712), job `Coverage (cargo-llvm-cov)` |
| Comando | `cargo llvm-cov --workspace --exclude garraia-desktop --exclude garraia-auth --exclude garraia-workspace --lcov` |

### Agregado do workspace

Direto do `coverage-summary.txt` do run (linha `TOTAL`):

| Métrica | Coberto / total | % |
|---|---|---|
| Regions | 170.324 / 227.127 | **74,99%** |
| Functions | 12.051 / 16.766 | **71,88%** |
| **Lines** | **110.794 / 153.910** | **71,99%** |
| Branches | 0 / 0 | n/d — não instrumentado |

O número canônico é **71,99% de linha**. `Branches` aparece zerado porque
a instrumentação de branch do `cargo-llvm-cov` não está ligada; não é
cobertura de branch igual a zero, é ausência de medição. Não citar esse `0`
como se fosse resultado.

### Por crate

Derivado do mesmo `lcov.info` (registros `DA:`), agregado por diretório de
crate. O total desta tabela (72,80%) difere ligeiramente do `TOTAL` do
`llvm-cov` (71,99%) porque agrega apenas arquivos sob `crates/` e `apps/`,
enquanto o `llvm-cov` conta todo arquivo instrumentado. Para citar um número
único, use o 71,99% acima; esta tabela serve para priorizar.

| Crate | Linhas | Cobertas | % |
|---|---:|---:|---:|
| `garraia-storage` | 794 | 764 | 96,22% |
| `garraia-common` | 1.789 | 1.705 | 95,30% |
| `garraia-config` | 7.347 | 6.993 | 95,18% |
| `garraia-embeddings` | 337 | 320 | 94,96% |
| `garraia-learning` | 2.888 | 2.710 | 93,84% |
| `garraia-security` | 1.456 | 1.365 | 93,75% |
| `garraia-hardware` | 4.159 | 3.898 | 93,72% |
| `garraia-db` | 6.779 | 6.320 | 93,23% |
| `garraia-skills` | 956 | 883 | 92,36% |
| `garraia-desktop-core` | 1.080 | 971 | 89,91% |
| `garraia-telemetry` | 395 | 349 | 88,35% |
| `garraia-media` | 802 | 699 | 87,16% |
| `garraia-glob` | 1.004 | 873 | 86,95% |
| `garraia-agents` | 20.078 | 17.443 | 86,88% |
| `garraia-channels` | 10.996 | 8.418 | 76,56% |
| `garraia-cli` | 21.718 | 16.259 | 74,86% |
| `garraia-gateway` | 60.621 | 35.398 | **58,39%** |
| `garraia-plugins` | 655 | 359 | **54,81%** |
| `garraia-voice` | 620 | 99 | **15,97%** |
| `garraia-auth` | 1.011 | 113 | 11,18% — excluído, ver abaixo |
| `garraia-workspace` | 96 | 43 | 44,79% — excluído, ver abaixo |

## A meta do ROADMAP, avaliada contra o número real

`ROADMAP.md` não pede 70% de workspace. Pede:

> Cobertura de testes ≥ 70% em crates de domínio (`garraia-agents`,
> `garraia-db`, `garraia-security`, `garraia-workspace`).

São quatro crates nomeados, e é contra eles que a meta se mede:

| Crate de domínio | Medido | Meta ≥ 70% |
|---|---:|---|
| `garraia-agents` | 86,88% | ✅ cumprida |
| `garraia-db` | 93,23% | ✅ cumprida |
| `garraia-security` | 93,75% | ✅ cumprida |
| `garraia-workspace` | — | ⚠️ **não mensurável** sob a exclusão atual |

Três das quatro cumprem com folga. A quarta **não é medida**: o
`garraia-workspace` está na lista de `--exclude` do job porque seus testes
precisam de Postgres testcontainer, e as 96 linhas que aparecem na tabela
por crate são código genérico/inline arrastado por testes de outros crates —
não são uma amostra da crate. Tratar esses 44,79% como "cobertura do
`garraia-workspace`" seria erro de leitura.

**Conclusão:** a meta não precisa ser corrigida para baixo, e o código das
três crates medidas não precisa de trabalho para alcançá-la. O que falta é
**medir a quarta** — o que depende de dar Postgres ao job de cobertura, a
mesma sub-issue de reinclusão já prevista no baseline de abril. Até lá, a
afirmação honesta é "3 de 4 crates de domínio cumprem a meta; a quarta não é
instrumentada", e não "a meta está cumprida".

## Por que o `quality-report.md` dizia `coverage_pct: None`

Não é falta de instrumentação, e não é o modo `fast` do coletor. A causa é
de **ordem entre dois workflows**, e é determinística:

1. `ci.yml` e `quality-ratchet.yml` são workflows separados, disparados em
   paralelo pelo mesmo push (invariante deliberada do plan 0064, "ajuste #3":
   o trigger é `pull_request`, nunca `workflow_run`).
2. O job `ratchet` tem `timeout-minutes: 10` e faz ~2-3 min de trabalho. O
   job `coverage` tem `timeout-minutes: 45` e leva 21-30 min.
3. O step *Try downloading lcov.info from parallel coverage job* procura o
   run de `ci.yml` do mesmo SHA e filtra por
   `select(.conclusion == "success")`. Quando o ratchet olha, aquele run
   ainda está `in_progress` e `conclusion` é `null` — então o filtro o
   descarta e `// empty` devolve vazio.
4. Sem `lcov.info`, `parse-llvm-cov.py` devolve, corretamente,
   `{"coverage_pct": null, "status": "not_collected_this_run"}`.

Ou seja: o ratchet **sempre** perde essa corrida, não às vezes. O
`coverage_pct` nunca foi coletado por ele desde que o step existe — e a
cobertura, enquanto isso, sempre foi produzida pelo `ci.yml`, só não chegava
até o relatório. O parser está correto; a coleta é que era inalcançável.

Verificação local, contra o `lcov.info` real deste run:

```console
$ python3 scripts/quality/parse-llvm-cov.py lcov.info
{"coverage_pct": 71.99, "files": 451, "lines_hit": 110794,
 "lines_total": 153910, "status": "present"}
```

### O que mudou

Duas correções no step, ambas best-effort — nenhuma falha ali derruba o job,
exatamente como antes:

1. A busca passa a filtrar por `status == "completed" and conclusion ==
   "success"` sobre os últimos 20 runs, em vez de olhar só o mais recente.
   Um re-run do ratchet depois que o `ci.yml` fechou agora encontra o
   artifact; antes nem esse caso funcionava.
2. Se nenhum run do SHA da PR terminou, cai para o último run de `ci.yml`
   **em `main`** que concluiu com sucesso.

A procedência do `lcov.info` é escrita no `GITHUB_STEP_SUMMARY`, logo acima
do próprio `quality-report.md` que o job já anexa ali — rotulada com o
commit e, no caso do fallback, com um `NOT this PR` explícito. O relatório
passa a mostrar um número real em vez de `None`, sem fingir que o número é
da PR.

O número da própria PR continua chegando apenas num re-run posterior do
ratchet. Isso é consequência direta do "ajuste #3" e está registrado aqui
para não ser redescoberto como bug.

O delta por PR não fica enganoso: o `compare.py` já distingue
`nao mensuravel no merge-base` como terceiro estado e rebaixa o veredito de
✅ para ⚠️ nesse caso, em vez de contar a cobertura do fallback como ganho
da PR. O `base-metrics.json` é coletado num worktree em `/tmp/base`, onde
não há `lcov.info`, então a base continua não medida — e o comparador já
sabe dizer isso.

## Política de exclusões — aceita explicitamente

Reafirmando o baseline de abril, com o estado de hoje:

| Crate | Razão | Aceito como | Quando reincluir |
|---|---|---|---|
| `garraia-desktop` | Casca Tauri + GTK ausentes em runner GHA. Já fora de clippy/build/test/msrv. | Permanente | Nunca — o Desktop tem pipeline própria. O núcleo testável vive em `garraia-desktop-core` (89,91%), que **está** instrumentado. |
| `garraia-auth` | Testes de integração exigem Postgres testcontainer + secrets (`GARRAIA_JWT_SECRET`, `GARRAIA_REFRESH_HMAC_SECRET`, `GARRAIA_LOGIN_DATABASE_URL`, `GARRAIA_SIGNUP_DATABASE_URL`). | Temporário | Quando o job de cobertura ganhar service container de Postgres. |
| `garraia-workspace` | Mesma razão. | Temporário — **e é o que bloqueia a meta do ROADMAP** | Mesma sub-issue. |

As três têm cobertura real pelos jobs `Test (ubuntu-latest)` e `E2E Tests`,
que rodam contra Postgres de verdade; o que falta é a *medição*, não o teste.

## Onde a cobertura rende mais

Por linhas descobertas, não por porcentagem — é onde o esforço compra mais:

| Crate | Linhas descobertas | % |
|---|---:|---:|
| `garraia-gateway` | 25.223 | 58,39% |
| `garraia-cli` | 5.459 | 74,86% |
| `garraia-channels` | 2.578 | 76,56% |
| `garraia-agents` | 2.635 | 86,88% |
| `garraia-voice` | 521 | 15,97% |

O `garraia-gateway` sozinho responde por **58% de todas as linhas
descobertas do workspace**. Qualquer meta agregada que suba vai ser decidida
lá — e é por isso que a escolha de um threshold de bloqueio não é cosmética.

## Threshold de bloqueio — implementado e aprovado pelo maintainer

O critério 2 da #1566 pede um threshold que **falhe** o build. Uma versão
anterior desta seção dizia que o gate não seria escrito, por ser decisão do
dono. O código foi então escrito e ligado, e o maintainer **aprovou o hard
gate de 70% e o merge da #1581 em 2026-10-07** (decisão registrada na
GAR-24), com a condição de que a proveniência do piso ficasse explícita:
70% agregado do workspace é decisão nova do maintainer e guardrail
adicional, não exigência literal do ROADMAP.

`scripts/ci/coverage_gate.py`, último step do job `Coverage (cargo-llvm-cov)`:

| | |
|---|---|
| Piso | `COVERAGE_FLOOR_PCT: "70.0"`, env do job em `ci.yml` |
| Origem do número | Decisão nova do maintainer, aprovada em 2026-10-07 (#1566 criterio 2, PR #1581): guardrail adicional sobre o agregado do workspace. **Não** é exigência literal do ROADMAP — a meta ≥ 70% dele vale para crates de domínio |
| Folga hoje | 72,00% medido − 70,0% de piso = **~2 pp** |
| Fonte do número | `lcov.info`, pelo **mesmo** `scripts/quality/parse-llvm-cov.py` que alimenta o `.quality/baseline.json` — um parser só, gate e baseline não podem divergir |
| Comparação | `>=`, inclusiva |
| `lcov.info` ausente/vazio | **falha** — fail-closed. Um run não medido não é um run aprovado |
| Posição no job | **último** step, depois dos artifacts e do comentário de PR: mede, publica, só então reprova |

Três escolhas que merecem registro, porque errá-las esvazia o gate:

1. **Não é o AI Quality Ratchet.** O `compare.py` segue `--mode report-only`
   e sua promoção a bloqueante continua sendo o PR-4 do plan 0064. O que o
   `CLAUDE.md` reserva para aprovação explícita é *aquele* controle. Este é
   independente e enforça um único número: o piso do guardrail aprovado.
2. **Piso no agregado, meta nas crates de domínio.** As duas coisas não são a
   mesma, e a seção acima explica por quê. O piso agregado de 70% é uma
   decisão *nova* do maintainer (2026-10-07), aprovada como guardrail
   adicional — não é uma leitura do ROADMAP nem uma exigência dele. O
   ROADMAP mantém a sua meta ≥ 70% nas crates de domínio, avaliada na seção
   acima. Mexer no piso é mexer nesse guardrail: decisão de maintainer,
   registrada no PR que mudar.
3. **Sem pipe para `tee`.** O runner roda `bash -e`, não `pipefail`. Um
   `python3 … | tee -a "$GITHUB_STEP_SUMMARY"` devolveria o status do `tee`,
   e o gate reprovaria em silêncio com o job verde. O status é capturado à
   mão e o step termina em `exit "$gate_status"`.

O risco real do piso de 70 é o `garraia-gateway`: ele responde por 58% de
todas as linhas descobertas do workspace, então qualquer PR grande sem teste
ali é o que vai encostar nos ~2 pp de folga primeiro. Isso é o gate
funcionando, não um falso positivo — mas é a razão de a folga ser a
informação mais importante desta seção.
