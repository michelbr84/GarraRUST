- **`docs/voice.md` documentava um provider de TTS e uma chave de config que nunca existiram.** A pagina
  mostrava `tts_provider: openai` com `tts_voice: "alloy"`; o `server.rs` so casa `hibiki` e `lmstudio`,
  e qualquer outro valor cai em silencio no Chatterbox — `VoiceConfig` nao tem campo `tts_voice`. A secao
  passa a documentar o provider `lmstudio` (servidor compativel com `POST /v1/audio/speech`), traz a tabela
  dos tres valores que fazem alguma coisa e diz que o resto vira Chatterbox sem aviso. `CLAUDE.md` e
  `ROADMAP.md` atribuiam ElevenLabs e Kokoro a crate `garraia-voice`: os adaptadores vivem em
  `garraia-channels::voice_channel`, atras da feature `voice` que nenhuma crate do workspace liga, e nao
  chegam ao gateway.
