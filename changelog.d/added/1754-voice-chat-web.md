- **Voice Chat continuo no Web Console (#1754).** Nova pagina `#/voice`: inicia
  uma chamada uma vez, fala sem apertar botao a cada turno, interrompe o GarraIA
  falando por cima e troca STT, modelo de conversa, TTS, voz e idioma no meio da
  chamada sem perder a sessao. Captura por AudioWorklet (PCM 16 kHz) com VAD por
  energia (pre-roll, fim de fala configuravel, teto de 30 s), STT por fala
  (`POST /api/voice/transcribe`), turno no `/ws` existente e TTS por frase
  (`POST /api/voice/synthesize`). Catalogo em `GET /api/voice/models`: servidores
  locais do `--with-voice` e instancias OpenRouter ja configuradas, filtradas por
  `architecture.*_modalities` (sem heuristica por nome), com preco como o
  provedor informa ("unidade nao informada"), horario da consulta e motivo de
  indisponibilidade; speech-to-speech aparece como nao executavel. O navegador
  so escolhe IDs: URL, chave e politica de roteamento ficam no servidor, e STT/TTS
  nao tem fallback. ADR 0027, plan 0366 e `docs/voice.md`. Validacao com
  microfone e alto-falante reais segue pendente.
- **`/ws` ganha `turn_id`, `stop` por turno e contexto de voz fiel (#1754).**
  Opt-in e aditivo: com `turn_id`, todo frame do turno o leva de volta e um
  `stop` de turno velho nao derruba o turno atual; com `voice: true`, a resposta
  diz quem respondeu (`provider`, `model`, `fallback`), o `stop` aceita
  `heard_chars` e o historico separa o trecho falado do gerado e nao ouvido.
  Cliente sem esses campos recebe os frames de sempre.
