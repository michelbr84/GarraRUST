- **O job de cobertura do CI deixa de ser cancelado por tempo.** Ele levava
  de 21 a 24 minutos ate a v0.4.4, e com os testes da v0.4.5 passou do teto
  de 30: em `main`, depois do trem da onda B, saiu cancelado com todos os
  testes verdes e o relatorio ja gerado. O teto sobe para 45 minutos, que
  continua pegando um teste travado.
