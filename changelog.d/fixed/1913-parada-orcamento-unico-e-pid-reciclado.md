- **Parada por porta com orcamento unico e sem PID reciclado (#1913).** Sem PID
  file, o `stop` e o pre-`stop` do `restart` encerravam os PIDs que o `lsof`
  listasse na porta um por um, cada um com os 30 s de grace inteiros: dois alvos
  custavam ate um minuto, e o segundo so recebia o `SIGTERM` depois de o
  primeiro sair. Agora todos recebem `SIGTERM` antes da espera e o orcamento e
  um so — a espera e coletiva e a escalada para `SIGKILL` tambem. A escalada
  passou a conferir o start-time do processo (`/proc/<pid>/stat`, Linux): um PID
  reciclado durante a espera nunca e atingido. Quando a identidade nao pode ser
  lida, vale o comportamento anterior (escalar); nas outras plataformas nada
  muda.
