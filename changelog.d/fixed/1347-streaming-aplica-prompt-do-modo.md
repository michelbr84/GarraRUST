- **O turno em streaming aplica o prompt do modo, e a persona padrao nao
  manda mais o modelo chamar `garra_status` (#1347).** O caminho de
  streaming do `AgentRuntime` montava o prompt de sistema so com override,
  prompt do runtime e persona, ignorando o `system_prompt_template` e o
  `max_tokens` do modo que o caminho batch (`process_message_with_agent_config`)
  ja aplicava: o mesmo turno no piso `search` recebia prompts diferentes
  conforme o ramo. Agora os dois montam o mesmo pedido: o prompt com a mesma
  precedencia (chamador > modo > runtime > persona) e o mesmo `max_tokens`
  (chamador > runtime > modo), e um teste prende que os dois ramos mandam o
  mesmo prompt, `max_tokens` e `temperature` num modo customizado. A
  `temperature` do modo continua fora dos dois ramos: so o
  `process_message_impl` (heartbeat, A2A) a manda. A persona padrao (PT e
  EN) citava `garra_status` pelo nome, e ela vale tambem em `garraia chat` e
  `garraia ask`, que nunca registram essa ferramenta; a linha ficou neutra
  ("use as ferramentas disponiveis nesta conversa"), e a instrucao de
  consultar `garra_status` continua vindo do runtime, so quando a
  ferramenta esta entre as oferecidas no turno.
