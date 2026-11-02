- **Base de voz com limites, timeouts e sem gasto duplicado (#1754).** Os
  clientes Whisper, Chatterbox, Hibiki e LM Studio leem respostas com teto de
  tamanho (16 MiB de audio, 1 MiB de metadados), tem timeout de conexao e total
  e nao seguem redirect. O LM Studio so cai no `/api/v1/chat` quando a rota de
  fala nao existe (404/405/501): antes qualquer erro, inclusive timeout, gerava
  uma segunda sintese. O Whisper idem: timeout nao dispara uma segunda
  transcricao. `POST /api/stt` deixa de gravar o audio em arquivo temporario e
  limita o upload; `POST /api/tts` limita o texto e nao reflete a mensagem do
  provedor. O `VoicePipeline` usa diretorio proprio por chamada, limpo em todo
  caminho de erro, e o `ffmpeg` morre junto com a tarefa cancelada. O idioma
  `"default"` que o Telegram passa vira o idioma configurado nos tres TTS.
