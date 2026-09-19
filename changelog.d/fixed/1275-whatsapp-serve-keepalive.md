- **O `serve` do WhatsApp vinculado nao via o caso "conectou e emudeceu".**
  Os relogios do driver so valiam antes do `connected` — com razao: depois
  dele, silencio e o estado normal de uma conta sem mensagens, e cortar por
  silencio derrubaria o canal saudavel toda madrugada. Uma ponte que
  conectava, entregava a sessao e parava de falar sem fechar o stdout e sem
  morrer ficava morta em silencio para sempre: sem queda visivel, sem
  reconexao, sem ninguem olhando um terminal. O conserto e prova de vida no
  protocolo, nao mais um prazo: depois do `connected` o driver manda `ping`
  a cada `ServeOptions::ping_every_secs` (15 s em producao) e exige resposta
  — qualquer evento, e um `pong` explicito — dentro de
  `ServeOptions::pong_deadline_secs` (45 s em producao). Prazo vencido cai em
  `ServeExit::Dropped { was_connected: true }`, e o backoff que ja existia
  assume. Um bridge de versao anterior que nao conhece `ping` responde com o
  evento `error` de comando desconhecido — que tambem prova vida e por isso
  tambem serve. A fixture ganhou o cenario `serve-wedge` (conecta, entrega a
  sessao e para de falar e de ler o stdin), exercitado pelo driver.
