- **A aprovacao de um pedido de confirmacao nao sobrevive a uma mensagem
  humana no meio (#1340).** `detect_confirmation_approval` aceitava o pedido
  mais recente em qualquer ponto das ultimas 6 mensagens, e a janela dizia so
  "recente". Na pratica: o turno 1 pausava pedindo para apagar uma pasta, o
  humano respondia "nao", o modelo perguntava outra coisa em texto, e um "ok"
  dado a ESSA pergunta ainda aprovava o apagamento. Agora o marcador so vale
  quando o historico termina no resultado pausado, com no maximo a narracao do
  assistente depois dele — a forma da retomada GAR-187 em todo canal. Uma
  mensagem humana entre o pedido e o "ok" encerra o pedido, e um resultado de
  ferramenta posterior sem marcador tambem, sem depender de o turno humano
  estar no historico (no caminho compativel com a OpenAI ele vem do corpo do
  request). A ordem dentro de cada mensagem e a neutralizacao de marcador em
  saida de ferramenta, das #1339 e #1226, seguem como estavam.
