- **O turno em streaming aplica o prompt do modo, e a persona padrao nao
  manda mais o modelo chamar `garra_status` (#1347).** O caminho de
  streaming do `AgentRuntime` montava o prompt de sistema so com override,
  prompt do runtime e persona, ignorando o `system_prompt_template` e o
  `max_tokens` do modo que o caminho batch ja aplicava: o mesmo turno no
  piso `search` recebia prompts diferentes conforme o ramo. Agora os dois
  seguem a mesma precedencia (chamador > modo > runtime > persona). A persona
  padrao (PT e EN) citava `garra_status` pelo nome, e ela vale tambem em
  `garraia chat` e `garraia ask`, que nunca registram essa ferramenta; a
  linha ficou neutra ("use as ferramentas disponiveis nesta conversa"), e a
  instrucao de consultar `garra_status` continua vindo do runtime, so quando
  a ferramenta esta entre as oferecidas no turno.
