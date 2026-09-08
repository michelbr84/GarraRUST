- gateway: com `gateway.api_key` configurada, as rotas `/api/*` passam a
  exigir `Authorization: Bearer`. Antes a chave valia so no handshake do
  `/ws`, e todo o REST — sessoes, memoria, providers, logs, diagnosticos —
  respondia a qualquer um que alcancasse a porta, o que num gateway em
  `0.0.0.0` e a rede inteira. Ficam abertas `/api/health`,
  `/api/capabilities` e `/api/auth-check`, que o onboarding do app e o
  console consultam antes de haver chave. Sem chave configurada nada muda.
  A comparacao de token do `/ws` deixa de sair cedo por comprimento, que
  entregava um oraculo de tamanho por tempo de resposta. (#1045)
