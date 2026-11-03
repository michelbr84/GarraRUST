- **Audio do Chatterbox so e baixado da origem configurada (#1754).** A URL do
  arquivo vinha do SSE do servidor Gradio e era baixada sem conferir o destino:
  uma resposta adulterada transformava o gateway num buscador de URL arbitraria.
  Agora ela tem de ter a mesma origem do `tts_endpoint`, sem credenciais na URL.
  Erros de STT/TTS (rotas `/api/stt`, `/api/tts` e `/api/voice/*`) deixam de
  ecoar o corpo do provedor, que pode conter chave, URL interna ou transcricao.
