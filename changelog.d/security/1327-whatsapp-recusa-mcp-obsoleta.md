- **WhatsApp pessoal volta a subir em instalacao padrao: a recusa por servidor
  MCP saiu (#1327).** O canal `whatsapp_linked` recusava subir — e recusava
  cada turno — enquanto houvesse qualquer ferramenta de servidor MCP
  registrada. A recusa compensava uma isencao do `ToolGate` que a #1288
  fechou; como toda instalacao nova ganha o servidor `filesystem` no primeiro
  boot, "instalacao padrao + `garra whatsapp link`" nunca subia. O piso que
  fica e o `ToolGate` do perfil `search`, que nega ferramenta MCP por nome
  (`filesystem__write_file` como `bash`) em cada turno, contra o inventario
  vivo — um teste monta o portao exatamente como o turno monta e prova isso
  antes da remocao. `channels.whatsapp_linked.default_mode` passa a aceitar
  so modo nativo e diferente de `auto`: `ToolGate::for_mode_name` trata nome
  desconhecido como portao ABERTO, entao um typo (ou o nome de um modo
  customizado, que nunca e resolvido para o piso do canal) daria `bash` e
  toda ferramenta MCP a quem manda mensagem — e a recusa por MCP mascarava
  isso em instalacao padrao. Agora o canal nao sobe (`ModoPadraoInvalido`,
  com a acao e o valor no log) e, por defesa em profundidade, o piso do turno
  cai em `search` quando o nome nao resolve. Na subida, o gateway avisa
  (`WARN`, uma vez) quando o perfil padrao do canal LIBERA algum servidor MCP
  — o que, com `default_mode` restrito a nativos, so acontece num perfil sem
  whitelist (`ask`, `code`) — dizendo o motivo, porque isso e escolha
  declarada do operador, nao erro. Qualquer motivo de nao subir alem de
  "desligado" sai em `WARN` com a acao (`rode garra whatsapp link`, `instale
  Node.js 20+`, `use search ou remova a chave`), e o boot deixa de avisar
  `unknown channel type: whatsapp_linked` para a secao que a propria CLI
  escreve. `AgentRuntime::has_gate_bypassing_tool` foi
  removida (sem chamador). Docs: `docs/whatsapp.md` ("Ferramentas e
  servidores MCP") e `docs/security/threat-model.md` §5.14.
