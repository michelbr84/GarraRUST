- **`garraia max-power` sem provider utilizavel roda offline, como a ajuda
  promete, em vez de parar na primeira etapa (#1228).** Numa instalacao sem
  provider configurado o comando dizia `execution: provider-backed` e morria
  com `ollama error status: 404 Not Found`: o palpite final da deteccao
  (Ollama com o modelo padrao) era tomado como provider, entao o caminho
  offline nunca rodava. Agora, sem nada alcancavel, com o Ollama sem o modelo
  ou sem conseguir listar os modelos, o pipeline roda deterministico e diz o
  que fazer (`ollama pull <modelo>` ou `garraia init`). O erro de status do
  Ollama passa a trazer o motivo que o servidor mandou (por exemplo
  `model 'qwen3.8:latest' not found`), a dica de cada etapa offline nomeia o
  binario em execucao em vez do alias `garra`, e uma etapa que falha fica
  registrada no `garraia.log`.
