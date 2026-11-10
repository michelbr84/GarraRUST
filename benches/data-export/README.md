# data-export — latencia de `GET /v1/me/export`

Medicao reprodutivel do endpoint de exportacao de dados do titular (#1801,
origem #1565). **Mede, nao reimplementa:** o harness e o teste de integracao
`crates/garraia-gateway/tests/rest_v1_me_export_perf.rs`, que chama o router
de producao com JWT valido contra um Postgres real.

Nao e crate nem membro do workspace — so a receita (`run.sh`) e os resultados
versionados (`results/`).

## O que e medido

O tempo entre enviar `GET /v1/me/export` e receber o **ultimo byte** do corpo,
passando por: verificacao do JWT, extractor `Principal`, a transacao do
handler `export_me` (RLS do role `garraia_app`, GUC de tenant reapontada por
grupo), serializacao do JSON e coleta do corpo inteiro.

O que **nao** entra: rede, TLS e o listener TCP. O router e chamado em
processo (`tower::ServiceExt::oneshot`), como no resto da suite de integracao.
O numero e custo de servidor, nao tempo de download.

O parse do JSON e a validacao de conteudo/isolamento rodam **fora** da janela
cronometrada, sobre o corpo da execucao fria e o da ultima quente.

## Carga

Tudo sintetico, gerado por `generate_series` num Postgres efemero
(`pgvector/pgvector:pg16` em testcontainer). Nenhuma conta, servico ou dado
de producao; nenhum token alem do JWT de teste emitido pelo proprio harness.

| Dado | Quantidade | Onde |
| --- | --- | --- |
| Mensagens da titular (corpo de ~30 a ~270 caracteres) | 10 000 (`MENSAGENS`) | 75 % em 3 chats do grupo dela, 25 % num chat do grupo de uma terceira |
| Mencoes recebidas pela titular | 200 | chat compartilhado |
| Arquivos da titular (so metadado) | 100 | grupo dela |
| Memorias pessoais | 100 | escopo `user` |
| Tarefas | 100 | grupo dela |
| Mensagens de terceira no **mesmo chat** (nao podem sair) | 1 000, das quais 200 mencionam a titular | chat compartilhado |
| Mensagens de **outro tenant** (nao podem sair) | 1 000 | grupo sem relacao com a titular |

Depois da carga roda um `ANALYZE`, para o planner ter as estatisticas que um
banco vivo com autovacuum teria.

### O teto de 5 000 linhas

`export_me` corta cada secao em 5 000 linhas e declara o corte em
`truncated_sections`. Com 10 000 mensagens o export devolve **5 000** e marca
`messages` como truncada — a latencia do cenario de 10 mil e a de um export
truncado, e o teste verifica exatamente isso. Para a latencia do export
completo no limite do teto, rode com `MENSAGENS=5000`.

## Execucao fria e quente

- **Fria:** a primeira requisicao depois da carga — conexao do pool de
  handlers e planos ainda nao aquecidos. As paginas recem-escritas ja estao no
  cache do Postgres; nao e um cold start de disco. Uma amostra.
- **Quente:** as `RUNS` requisicoes seguintes (padrao 30), em sequencia, sem
  concorrencia. Reportadas como minimo, p50, p95 (posto mais proximo), maximo
  e media, com todas as amostras no JSON cru.

Cada requisicao sai de um IP de par diferente, porque o rate limit da cadeia
de producao e por IP e o objeto da medida e o handler.

## O que o teste afirma

- Conteudo e isolamento: quantidade exata por secao, toda mensagem e da
  titular e de um grupo dela, o corte e declarado se e so se houve, e nenhuma
  marca ou identificador do outro tenant (nem mensagem sem mencao da terceira)
  aparece no corpo.
- Latencia: **so** a referencia historica do `ROADMAP.md` ("Export LGPD ... em
  < 30s") — toda amostra, fria ou quente, tem de caber nela. Os percentis sao
  reportados, nao afirmados: um limiar apertado viraria teste instavel em
  runner lento.

O teste roda no CI junto das outras suites `test-helpers` (workflow
`security-gate-bola.yml`), com os padroes acima.

## Como rodar

Requisitos: `cargo`, `git` e Docker em execucao.

```bash
benches/data-export/run.sh                  # perfil dev (o que o CI compila)
PERFIL=release benches/data-export/run.sh   # binario otimizado
MENSAGENS=5000 benches/data-export/run.sh   # export completo, no limite do teto
```

Ou direto, sem registrar o ambiente:

```bash
cargo test -p garraia-gateway --features test-helpers \
    --test rest_v1_me_export_perf -- --nocapture
```

Knobs do teste: `GARRAIA_EXPORT_PERF_MESSAGES`, `GARRAIA_EXPORT_PERF_RUNS` e
`GARRAIA_EXPORT_PERF_OUT` (caminho do JSON com as amostras).

`run.sh` falha (exit 70) se o log nao tiver `1 passed`: teste filtrado ou zero
casos nao e medida.

## Resultados

`results/<DATA>-<host>/`, com `<DATA>` em America/New_York e timestamps
internos em UTC (convencao do `CLAUDE.md`):

```text
results/<DATA>-<host>/
├── README.md                        # sumario humano
├── environment-<perfil>-<N>msgs.txt # maquina, versoes, SHA, CARGO_PROFILE_*
└── raw/
    ├── <perfil>-<N>msgs.json        # relatorio com todas as amostras
    └── <perfil>-<N>msgs.out.txt     # saida do teste
```

Qualquer `CARGO_PROFILE_*` exportado na hora do build fica registrado no
`environment-*.txt`: um numero de `release` sem LTO nao pode passar por um
numero do binario de release.
