- **`GET /admin/api/logs` passa a ler so a cauda do `garraia.log` e deixa de
  responder 500 com byte que nao e UTF-8 (#1371).** O handler fazia
  `read_to_string` do arquivo inteiro a cada requisicao para devolver as
  ultimas `limit` linhas. Com o log do daemon crescendo entre restarts, isso
  custava memoria e CPU proporcionais a toda a historia do daemon, e a
  escrita crua que chega pelo descritor herdado (panic, filho com stderr
  herdado) nao tem garantia de ser UTF-8, o que derrubava a leitura inteira.
  Agora o `/admin/api/logs` usa a mesma leitura do `GET /api/logs`: no
  maximo 512 KiB do fim do arquivo, com teto que vale mesmo se o daemon
  escrever durante a leitura, em UTF-8 lossy (o byte invalido vira U+FFFD),
  e sem a linha partida pelo corte.
