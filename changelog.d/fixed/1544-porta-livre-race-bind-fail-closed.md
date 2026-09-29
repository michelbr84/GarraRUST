- **`bind_fail_closed_cli` deixa de flakear por porta compartilhada (#1544).**
  `porta_livre()` pedia uma porta efemera ao kernel e a liberava na hora; o
  kernel reusa porta liberada, entao dois dos seis testes que rodam em paralelo
  podiam receber a mesma. Um deles sobe o gateway de verdade e o deixa vivo por
  3s, e era esse listener que o outro encontrava ao provar que a recusa nao
  escutou — `Address already in use` sem nada errado no binario. As portas
  entregues agora ficam num registro process-wide. So alvo de teste.
