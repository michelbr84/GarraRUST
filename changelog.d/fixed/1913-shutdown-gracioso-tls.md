- **Gateway com TLS passa a encerrar de forma graciosa (#1913).** No caminho
  HTTPS (binario compilado com `--features tls`) o listener nao tinha gancho de
  encerramento: Ctrl+C e `SIGTERM` matavam o processo sem desconectar servidores
  MCP nem canais, e, desde que cada servidor MCP stdio tem grupo de processos
  proprio, os descendentes dele ficavam para tras. Agora o sinal para o
  listener, da ate 10 s para as conexoes abertas terminarem e segue para a
  mesma limpeza do caminho HTTP. O CI passa a compilar e testar a feature
  `tls`, que nenhum job exercitava.
