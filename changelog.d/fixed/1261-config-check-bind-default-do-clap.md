- **`config check` resolve o bind como o `garra start`: env, senao o default do clap —
  nunca o arquivo (#1261).** A #1325 fazia o achado de exposicao cair em
  `gateway.host`/`gateway.port` do arquivo quando `HOST`/`PORT` nao estavam na env, mas
  o `start` nunca le essas chaves: o arg do clap sempre traz um valor (flag, env ou
  `127.0.0.1:3888`) e o `main.rs` escreve por cima da config carregada. Resultado: um
  `0.0.0.0` no arquivo ainda virava "binds all interfaces" sobre um valor morto, e o
  texto com env dizia que ela "sobrescreve o arquivo" — falso. Agora o veredito de
  exposicao usa env > default do clap (constantes espelhadas de `main.rs`, presas por
  um teste que le aquele fonte), julga qualquer IP fora do loopback como o gateway faz
  (nao so `0.0.0.0`/`::`) e avisa a parte quando um hostname nao da para julgar; as
  chaves do arquivo que diferem do bind efetivo viram um aviso proprio, "not read by
  `garra start`", em vez de fingir exposicao; e a ressalva do achado passa a dizer que
  `garra restart` ignora `HOST`/`PORT`. Sem mudanca de comportamento de boot.
  `docs/auth-config.md` 5.1 descreve o estado pos-#1325 e aponta a #1261 (reaberta)
  para as decisoes R5 que seguem em aberto.
