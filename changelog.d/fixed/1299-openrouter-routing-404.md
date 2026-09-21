- **OpenRouter 404 `No allowed providers are available` chegava como erro cru
  do provider.** A preferencia `provider.only` em vigor (sufixo `:provider`
  no slug do modelo ou preferencia da conta) sem intersecao com os providers
  que servem o modelo condena a requisicao antes de sair, mas a mensagem nao
  dizia o que configurar. Os dois caminhos do provider OpenAI-compativel
  (batch e streaming) agora classificam essa assinatura como erro de
  configuracao de roteamento nao-transitorio, com modelo, restricao efetiva
  (`permits only:`), providers compativeis e caminho de recuperacao
  (`/model <outro-modelo>` ou ajuste da preferencia na conta OpenRouter — o
  Garra nao altera `provider.only` por conta propria). O classificador de
  retry do runtime trata a assinatura como nao-retryable, inclusive se o
  texto carregar um numero de status retryavel no meio (#1299).
