- **Fixture dos testes de parada nao mata processo alheio nem vaza o gateway sintetico (#1913).**
  O descendente sintetico passa a ser seguido por `pidfd` em vez de PID cru:
  depois que ele sai, um PID reciclado nao conta mais como "descendente vivo" e a
  limpeza do teste nao manda `SIGKILL` a outro processo. O gateway sintetico fica
  sob guarda desde o `spawn`, entao uma linha de prontidao invalida mata e colhe o
  processo em vez de deixa-lo vivo depois do teste. So testes; producao intacta.
