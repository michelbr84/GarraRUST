- **CLI Unix amplia a espera pela limpeza do gateway antes do SIGKILL (#1913).**
  `stop`, `restart` e `uninstall` aguardam ate 30 s, em vez de 5 s, antes de
  escalar a parada, reservando tempo para o dreno TLS e a limpeza MCP. O retorno
  continua imediato quando o processo termina; um gateway travado pode levar
  aproximadamente 32 s para cair. Testes com processos sinteticos isolados cobrem
  limpeza lenta, retorno rapido, escalada e preservacao de outro dominio. Nao
  altera o Windows, nao limita o dreno HTTP/canais e nao garante contencao apos
  parada forcada.
