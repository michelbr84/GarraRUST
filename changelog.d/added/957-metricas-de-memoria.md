- **A memoria passa a aparecer no `/metrics` (#957).** Ate aqui o `/metrics`
  tinha quatro metricas HTTP genericas e nada sobre o componente central do
  produto: o operador nao tinha como monitorar a saude do recall semantico.
  Entram quatro, com prefixo `garraia_` — nao `garra_` como a issue propoe,
  porque duas familias de prefixo no mesmo endpoint quebrariam todo dashboard
  que agrupa por `garraia_.*`: `garraia_memory_embed_latency_seconds`
  {provider,operation}, `garraia_memory_embed_failures_total`{provider,operation},
  `garraia_memory_recall_latency_seconds` e `garraia_memory_ingested_total`
  {outcome}. A que mais importa e a de falha: o #948 tirou a falha de embedding
  do silencio no log, e esta a tira do painel — log conta o caso, metrica conta
  a tendencia, e e a tendencia que faz alguem descobrir que o provider caiu
  antes de o recall degradar. `no_provider` e `failed` sao desfechos separados
  de proposito (a diferenca entre "ninguem configurou" e "configurou e esta
  quebrado"), e `noise` existe por causa do filtro do #952 — sem ele o total de
  entradas sem vetor subiria sem que ninguem distinguisse defeito de politica.
  Emitidas pelo facade `metrics` via `garraia-common`, e **nao** pelo
  `garraia-telemetry`: quem emite e o `garraia-agents`, que a CLI linka, e
  depender da telemetria arrastaria OpenTelemetry, OTLP, tonic e axum para
  dentro do binario da CLI. Sem recorder instalado cada chamada e um no-op.
  Toda label vem de conjunto fechado, com teste afirmando que id de sessao, id
  de usuario e conteudo nunca chegam a uma label.
