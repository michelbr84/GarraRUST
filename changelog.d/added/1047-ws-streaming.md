- gateway: o `/ws` passa a emitir o turno enquanto ele acontece. Com
  `"stream": true` na mensagem, o cliente recebe `delta` a cada pedaco de
  texto e `tool_started`/`tool_finished` no ciclo de vida de cada ferramenta,
  e pode cancelar o turno com `{"type":"stop"}` — que responde `stopped` e
  nao persiste o parcial. Sem a flag, a sequencia de frames e a de sempre: so
  o `message` final. Como efeito, o socket passa a ser lido durante o turno,
  entao o heartbeat deixa de passar fome num turno longo. (#1047)
