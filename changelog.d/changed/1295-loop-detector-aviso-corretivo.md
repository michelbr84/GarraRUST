- **O detector de loop de ferramenta avisa o modelo uma vez antes de abortar
  o turno (#1295).** Na primeira vez que uma tarefa repete a mesma chamada
  (mesma ferramenta, mesmos argumentos) tres vezes seguidas, a terceira
  continua sem rodar, mas o turno nao morre mais: o modelo recebe no lugar do
  resultado uma observacao corretiva com o nome da ferramenta, a contagem e o
  resumo redigido do input repetido, pedindo que leia o erro anterior e mude
  de abordagem. Qualquer deteccao seguinte na mesma tarefa, do mesmo loop ou
  de outro, aborta como antes, com a mensagem do #1318, e a chamada avisada
  aborta tambem na proxima vez que voltar na tarefa, mesmo com outra chamada
  no meio ou depois do reset do teto por turno. A chamada barrada, avisada
  ou abortada, nunca roda: antes do aviso a chamada em loop roda no maximo
  duas vezes seguidas (a mesma janela de tres chamadas de antes), depois
  dele nao roda mais na tarefa, e o resto segue limitado por
  `max_per_turn`/`max_per_task`. O custo extra e no maximo uma volta de
  LLM. O aviso so e rearmado por uma mensagem nova do usuario, nunca pelo
  reset do teto por turno, e nao ha chave de config para desliga-lo. Dentro de `tool_program` (e no
  programa que falha antes do primeiro passo) vale a mesma cadencia, com o
  motivo rotulado como loop e nao como gate. A chamada barrada, avisada ou
  abortada, agora emite o par `tool_started`/`tool_finished` com
  `success=false` e o veredito, entao aparece no `/tool` do `garraia chat` ao
  lado das chamadas identicas anteriores e do erro original. O sub-item de
  "diff dos payloads" fica sem codigo: a assinatura e nome mais hash dos
  argumentos, entao um loop detectado tem payloads identicos por definicao.
