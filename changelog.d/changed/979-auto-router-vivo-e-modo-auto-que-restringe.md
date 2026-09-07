- **`/mode auto` passa a restringir de verdade (#979).** Ate aqui escolher
  `auto` resolvia para um perfil de politica vazia — ou seja, nao mudava nada.
  Agora o perfil do turno sai da classificacao da mensagem: "escreve uma funcao
  que soma" ganha permissao de escrita, "onde fica o handler de login" roda
  somente-leitura. A linha que isto **nao** cruza e a do #988: deduzir para quem
  escolheu `auto` e executar a escolha; deduzir para quem nao escolheu nada
  continua sem ligar politica nenhuma.
- **O classificador entende portugues.** As listas de palavra-chave eram so em
  ingles. Enquanto o modo era decoracao de prompt, um classificador que nunca
  dispara para "implementar o parser de config" era so inerte; com a politica
  valendo, `/mode auto` para quem escreve em portugues virava **nenhuma
  restricao**, em silencio, justamente para o publico principal do projeto. O
  vocabulario veio do `AutoRouter` morto de `agent_mode.rs`; o algoritmo dele
  nao veio — era primeiro-que-casar-vence e devolvia sempre um modo, nunca
  `None`, e classificar "oi" como `code` sob politica aplicada seria pior que
  nao classificar.
- **Os limites do modo alimentam o orcamento de execucao (#979).**
  `ModeLimits` existia desde o desenho dos modos e o runtime nunca o leu: todo
  turno rodava com o padrao fixo, entao um modo que se declarava mais curto nao
  era mais curto em lugar nenhum. `max_tool_loops` e `timeout_secs` passam a
  valer. Precedencia: override explicito de `max_tool_calls` > limites do modo >
  padrao.
