- **`docs/voice.md` ganha a secao "Known pitfalls with the stock app" (#1539).**
  Duas armadilhas de quem sobe o `multilingual_app.py` do Chatterbox pela
  primeira vez: a sonda de saude que batia na rota errada (bug do gateway,
  corrigido na #1538 — a secao diz para atualizar, e so descreve o workaround
  para quem esta preso em 0.4.7 ou anterior) e os prompts de voz default, que
  sao URLs `https://` passadas cruas ao `librosa.load` e quebram a sintese sem
  voz enviada. O passo de troubleshooting "TTS not responding" para de mandar
  consultar `/health`, rota que o Gradio nao serve.
