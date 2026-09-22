- **O `garraia config check` passa a rodar em todo boot (#1247).** `garraia
  start`, `garraia restart` e `garraia start -d` rodam o mesmo `run_check` do
  comando, uma vez, antes do fork, do PID file e do stop do daemon atual. Cada
  `Error` sai no log uma vez em nivel de erro e cada `Warning` em nivel de
  aviso, com um resumo apontando para `garraia config check`; no `start -d` as
  mesmas linhas vao para o stderr do terminal antes do fork, porque depois
  dele o log e invisivel. Antes, uma config com `Error` subia em silencio, e o
  check so valia para quem lembrasse de roda-lo. Recusar o boot fica restrito
  a uma lista fechada de achados que falham abertos (hoje so o TLS pela
  metade): todo outro `Error`, como uma entrada `llm` sem chave, e dito mas
  nao derruba instalacoes que funcionam hoje. Os achados de
  `gateway.host`/`gateway.port` nao se repetem no boot: quem julga o bind e a
  recusa do #1261, sobre o endereco real. Migracao: quem precisa subir apesar
  de um achado bloqueante usa `GARRAIA_ALLOW_INVALID_CONFIG=1` (exatamente
  `1`; outro valor conta como ausente), e o achado segue logado como erro. A
  retencao da memoria com `interval_hours` ou `max_age_days` fora da faixa nao
  derruba mais o worker com panic de `interval(0)` nem apaga por um corte que
  ninguem pediu: a varredura nao sobe, com `error!` no log.
