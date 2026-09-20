- **A saida de `file_read` entra no contexto do modelo pelo guard de
  injecao indireta (#1243, fatia 2).** O conteudo de arquivo e dado de
  terceiro: quem controla o arquivo controla o texto, e ate aqui ele
  chegava cru. Agora `FileReadTool::execute` passa o conteudo por
  `garraia_security::sanitize_indirect` (caracteres invisiveis
  removidos; suspeito chega precedido do banner de dado nao-confiavel,
  com a origem nomeada) e o criterio de nao-mutilacao e testado
  explicitamente: codigo-fonte limpo segue byte a byte, sem banner e
  sem corte. Payload com instrucao injetada chega emoldurado. Pendencia
  restante da issue: `device_read`/`device_list` (fatia 3).
