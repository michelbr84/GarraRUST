- **Erro lendo a resposta de um provider OpenAI-compativel passa a dizer a
  causa, e o `garraia max-power` passa a gravar no `garraia.log` (#1228).** O
  dogfood parou numa etapa com `failed to read response body: error decoding
  response body`: o reqwest da esse mesmo rotulo para qualquer falha lendo o
  corpo (conexao fechada no meio, corpo truncado) e guarda a causa real em
  `source()`, que o texto descartava. A mensagem agora desce a cadeia de
  causas. E o `max-power` nao inicializava o tracing, entao o pipeline nao
  deixava rastro no log; agora deixa, como os outros comandos.
