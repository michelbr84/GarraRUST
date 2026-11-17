- **Teste de parada da CLI deixa de travar a espera pelo SIGTERM (#1913).**
  A fixture Python dos testes de `stop` no Linux podia perder o SIGTERM
  entre o readiness e o `signal.pause()`. Agora ela bloqueia o sinal antes
  do readiness e espera com `sigwait`. Nao muda codigo de producao, prazos
  nem assercoes.
