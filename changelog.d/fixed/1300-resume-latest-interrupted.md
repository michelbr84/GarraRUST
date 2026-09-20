- O chat agora grava a pergunta ANTES do turno rodar e grava um marcador
  explicito (`[turno interrompido: timeout|cancelado|erro]`) quando o turno
  acaba sem resposta. Um crash, timeout ou Ctrl+C nao apaga mais o turno do
  ponto de vista do `--resume`: a sessao restaurada termina na pergunta
  interrompida, com o motivo marcado. `garraia chat --resume` sem valor (ou
  `--resume latest`) retoma a sessao com a atividade mais recente do CLI
  (nova query `latest_session_id` no SessionStore, isolada por canal), e o
  novo comando `/resume [id]` do REPL faz o mesmo sem reiniciar, avisando
  quando o ultimo turno terminou interrompido.
