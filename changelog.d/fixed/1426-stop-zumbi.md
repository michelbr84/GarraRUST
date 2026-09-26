- **`garraia stop` nao trata mais um zumbi como processo vivo (#1426).**
  Achado pelo dogfood em container limpo: quando o pai do daemon nao colhe
  filhos (um container cujo PID 1 e `sleep`, um `docker exec` sem
  `--init`), o gateway encerrava limpo no SIGTERM — o `garraia.log` termina
  em "gateway shut down gracefully" — mas virava zumbi, e `kill(pid, 0)`
  continuava devolvendo 0. O `stop` entao esperava os 5 s de graca, mandava
  SIGKILL e anunciava "survived SIGTERM and SIGKILL" de um processo ja
  morto. No Linux, `is_process_running` agora le o estado em
  `/proc/<pid>/stat` e um `Z` conta como parado; fora do Linux o
  comportamento e o de sempre. Teste de unidade forka um filho, espera ele
  virar zumbi de fato e exige `false`.
