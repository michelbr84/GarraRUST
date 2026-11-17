- **Requisicao HTTP pendurada deixa de segurar o desligamento do gateway (#1913).**
  O caminho HTTP puro drenava as conexoes sem prazo: uma unica requisicao que
  nao terminasse (stream longo, handler travado) segurava o servidor depois do
  `SIGTERM`, a limpeza de MCP e canais nao rodava, e o gateway so saia pelo
  `SIGKILL` da CLI. O dreno agora tem teto de 10 s, igual ao do TLS: passado
  isso as conexoes abertas sao largadas com aviso no log e o desligamento
  segue (dreno 10 s + MCP 10 s + canais 5 s cabem no grace de 30 s da CLI).
