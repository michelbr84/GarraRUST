- **WhatsApp pessoal volta a subir em instalacao padrao: a recusa por servidor
  MCP saiu (#1327).** O canal `whatsapp_linked` recusava subir — e recusava
  cada turno — enquanto houvesse qualquer ferramenta de servidor MCP
  registrada. A recusa compensava uma isencao do `ToolGate` que a #1288
  fechou; como toda instalacao nova ganha o servidor `filesystem` no primeiro
  boot, "instalacao padrao + `garra whatsapp link`" nunca subia. O piso que
  fica e o `ToolGate` do perfil `search`, que nega ferramenta MCP por nome
  (`filesystem__write_file` como `bash`) em cada turno, contra o inventario
  vivo — um teste monta o portao exatamente como o turno monta e prova isso
  antes da remocao. Na subida, o gateway avisa (`WARN`, uma vez) quando o
  perfil padrao do canal LIBERA algum servidor MCP — `allowed:
  ["servidor/*"]` ou `allowed` vazia — porque isso e escolha declarada do
  operador, nao erro. Qualquer motivo de nao subir alem de "desligado" sai em
  `WARN` com a acao (`rode garra whatsapp link`, `instale Node.js 20+`), e o
  boot deixa de avisar `unknown channel type: whatsapp_linked` para a secao
  que a propria CLI escreve. `AgentRuntime::has_gate_bypassing_tool` foi
  removida (sem chamador). Docs: `docs/whatsapp.md` ("Ferramentas e
  servidores MCP") e `docs/security/threat-model.md` §5.14.
