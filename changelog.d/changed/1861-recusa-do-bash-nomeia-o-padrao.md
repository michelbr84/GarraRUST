- **Recusa da tool `bash` diz ao modelo qual padrao casou (#1861, criterio 3).**
  O bloqueio da denylist devolvia so "padrao perigoso detectado", e na sessao
  que originou a #1861 o modelo recebeu ~13 recusas seguidas sem saber por que,
  em loop. A mensagem agora nomeia o padrao estatico que casou (texto fixo do
  `DENY_LIST`, nunca um trecho do comando, que pode levar segredo), explica que
  o gate le o texto literal inclusive dentro de aspas, heredoc e comentario, e
  aponta o `file_write` para quem so esta citando o texto, pedindo
  explicitamente que o comando nao seja reescrito para escapar do bloqueio. A
  recusa fail-closed do tier de confirmacao tambem nomeia o padrao. Nenhuma
  decisao do gate mudou: o que bloqueava continua bloqueando.
