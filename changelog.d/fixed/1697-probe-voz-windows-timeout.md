- **Diagnostico de voz nao afirma "sem resposta" quando no Windows o loopback ainda recusa (#1697).**
  No Windows, um `connect` para porta fechada no loopback so devolve a recusa depois de
  retransmitir o SYN (~2s em `127.0.0.1`), mais que o budget de 1.5s do probe. O texto de
  `voice.tts`/`voice.stt` passa a dizer "nothing listening or no answer within 1.5s" nesse caso
  (so Windows + loopback); o status segue `Unreachable`. Os testes do probe esperam o texto
  correto por plataforma.
