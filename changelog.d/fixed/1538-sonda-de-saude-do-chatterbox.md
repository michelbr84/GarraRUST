- **Chatterbox saudavel deixa de ser reportado como fora do ar (#1538).** A
  sonda de fallback batia em `/gradio_api/config`, rota que nenhuma versao do
  Gradio serve (4.x a 6.x servem o config em `/config`): com a raiz
  indisponivel o gateway dizia `❌ tts-chatterbox` e mantinha o modo voz
  desligado contra um `multilingual_app.py` de pe. A cadeia agora e `/` →
  `/config` → `/gradio_api/info`, para na primeira 2xx, e e a MESMA nos dois
  lugares que sondavam separado (`ChatterboxClient::health_check` e o check
  `tts-chatterbox` do `/api/health`), que antes podiam discordar. O WARN da
  falha nomeia cada rota e o que ela respondeu, entao "nada escutando"
  (`connect failed`) e "de pe na rota errada" (`HTTP 404`) deixam de sair com a
  mesma linha sem status. `docs/voice.md` troca o `curl /health` do
  troubleshooting — rota que o app stock nao serve — pelas tres rotas reais.
