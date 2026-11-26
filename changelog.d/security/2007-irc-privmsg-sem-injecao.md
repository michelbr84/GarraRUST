- **IRC: resposta do modelo nao vira mais comando IRC (#2007).** A resposta
  do turno separava as linhas so em `\n`, e um `\r` isolado no texto do
  modelo (influenciavel por injecao de prompt) chegava cru ao socket: num
  servidor que trata `\r` como fim de mensagem, o resto virava comando
  (`QUIT`, `JOIN`, `PRIVMSG NickServ ...`). O `send_privmsg` nem quebrava
  linha. Agora os dois caminhos passam por um ponto so: o texto e quebrado em
  `\r\n`, `\n` e `\r` isolado, perde `\0` e o delimitador de CTCP `\x01`,
  cada linha sai como `PRIVMSG <alvo> :<linha>` cortada sem partir
  caractere (antes um caractere multibyte cortado virava `U+FFFD`) e dentro
  dos 512 bytes do protocolo mesmo com alvo longo, e alvo com espaco,
  virgula, `:` inicial ou controle e recusado. O `send_raw` recusa qualquer
  linha com terminador ou NUL embutido, e o `PONG` ecoa o token do servidor
  sem caractere de controle (um PING hostil nao derruba a conexao).
