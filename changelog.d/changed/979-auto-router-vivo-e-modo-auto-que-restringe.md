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
- **O modo baixa o teto de execucao, nunca levanta.** Achado de auditoria: dos
  nove perfis, um so pede mais que o padrao — o `orchestrator`, com 100 chamadas
  e 60s de timeout, que levaria o pior caso de um turno de ~25 para ~100
  minutos. Modo e um seletor do usuario (`/mode` e comando, e o
  `POST /api/mode/select` e aberto), entao deixa-lo levantar o teto faria o
  custo de API e o tempo de parede dependerem do que a pessoa digitou, sem o
  operador ter dito nada. O padrao — que o operador ja configura — vira o limite
  superior, e o modo so encurta a partir dali. Quem quer mais passa
  `max_tool_calls` explicito, que e knob de quem sobe o processo.
- **A lacuna do MCP passa a ser dita em voz alta.** Um modo whitelist
  (`search`, `review`, `architect`, `debug`, `edit`) lista so nomes nativos, e
  ferramenta de servidor MCP passa por ele — continua sujeita ao `denied`, mas
  nao a whitelist. Isso ja era assim; o que mudou e **quem encontra**, porque
  `/mode auto` deixou de ser inerte. Quem digitou `auto` e escreveu uma pergunta
  de busca agora acredita estar somente-leitura enquanto uma ferramenta MCP de
  escrita continua disponivel, e acreditar numa restricao que nao existe e pior
  que nao ter restricao. Fechar a lacuna derrubaria toda integracao MCP nesses
  cinco modos, em silencio, entao por ora o runtime emite `warn!` nomeando as
  ferramentas MCP que passaram — para o operador que conectou o servidor, nao
  para o modelo. Um whitelist que entenda servidor MCP precisa ser desenhado.
