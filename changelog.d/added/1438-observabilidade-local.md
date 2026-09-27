- **Registro local de confiabilidade de tools, MCP e canais (#1438).** Modulo puro
  `garraia_agents::observabilidade`: por ferramenta, chamadas por desfecho
  (sucesso, erro, timeout, negada pela politica, indisponivel, recusada pelo
  breaker, aguardando confirmacao), aberturas do breaker, latencia em baldes
  fixos (p50/p95 aproximados) e ultima falha; por servidor MCP, quedas e
  reconexoes (o health monitor relata o transporte a cada tick, uma queda por
  transicao vivo -> morto, e o desfecho de cada reconexao automatica); por canal,
  conexoes, quedas e reconexoes; series de tamanho de armazenamento com teto.
  Sessao nao e dimensao, nome fora do registro cai em `(unknown)`, tetos de
  cardinalidade, `Instant` injetado e lock sem panico. Nada sai da maquina: so
  nome de servidor e booleanos — nunca `last_error`, comando ou caminho. O
  endpoint autenticado que expoe o snapshot vem em fatia propria.
