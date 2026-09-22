- **O erro do `garraia whatsapp link` quando o bridge morre antes de
  conectar volta a trazer as ultimas linhas do stderr do Node (#1368).** A cauda so
  era lida com o que a task do stderr ja tinha encaminhado; com o filho
  recem-terminado, a ultima linha, justamente a que explica o erro, ainda
  estava no pipe, e a mensagem saia so com "o bridge encerrou (codigo 1)
  antes de conectar". O CI de cobertura pegou a corrida em `main`. Agora,
  quando o filho ja saiu, o erro espera o fim do stderr por ate 2 s antes de
  montar a cauda, que continua redigida.
