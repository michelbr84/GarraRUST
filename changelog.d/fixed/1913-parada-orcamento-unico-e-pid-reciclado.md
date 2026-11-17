- **Pre-stop do `restart` com orcamento unico e sem PID reciclado (#1913).** Sem
  PID file, o pre-stop do `restart` (e do `start -d` que reinicia) encerrava os
  PIDs que o `lsof` listasse na porta um por um, cada um com os 30 s de grace
  inteiros: dois alvos custavam ate um minuto, e o segundo so recebia o
  `SIGTERM` depois de o primeiro sair. Agora todos recebem `SIGTERM` antes da
  espera e o orcamento e um so — a espera e coletiva e a escalada para `SIGKILL`
  tambem. O `stop` (que sempre olhou um PID so) e o resto da parada passam a
  conferir o start-time do processo (`/proc/<pid>/stat`, Linux): um PID
  reciclado durante a espera nunca e atingido, e conta como encerrado em vez de
  segurar o `stop` ate o fim do grace. Quando a identidade nao pode ser lida,
  vale o comportamento anterior (escalar); nas outras plataformas nada muda.
