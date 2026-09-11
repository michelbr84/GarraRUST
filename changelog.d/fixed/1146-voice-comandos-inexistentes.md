- **#1146 as instrucoes de instalacao de voz paravam de citar comandos que nao
  existem.** `docs/voice.md`, os hints do wizard (`garraia init`) e o `next_step`
  do `/api/diagnostics` mandavam rodar `chatterbox-tts serve` e `fwsh serve`:
  a wheel `chatterbox-tts` do PyPI e biblioteca e nao expoe CLI nenhuma, e `fwsh`
  nao existe em pacote algum. A doc tambem apontava para as imagens
  `ghcr.io/garraia/chatterbox` e `ghcr.io/garraia/hibiki`, que nunca foram
  publicadas. Os tres lugares agora descrevem o protocolo que os clientes em
  `garraia-voice` realmente falam: TTS pelo app Gradio `multilingual_app.py` do
  repo `resemble-ai/chatterbox` na porta 7860, e STT pelo `whisper-server` do
  `ggml-org/whisper.cpp` na porta 9090 (com a alternativa de qualquer servidor
  OpenAI-compatible expondo `/v1/audio/transcriptions`). Testes de regressao no
  wizard e no diagnostics impedem os comandos inventados de voltarem.
