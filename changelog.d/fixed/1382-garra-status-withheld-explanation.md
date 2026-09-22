- **`garra_status` explica o que `withheld` significa (#1382, #1387).** O relatorio
  ja retinha o que so interessa ao operador num turno restrito e listava os campos
  retidos em `withheld`, mas nada dizia ao modelo como ler isso: um campo retido sai
  `null`, e com `mcp_servers: null` o modelo respondia que este Garra nao tem MCP. A
  `description()` da tool e o proprio relatorio (campo `withheld_means`, presente so
  quando ha algo retido) agora dizem que campo citado em `withheld` esta OCULTO POR
  POLITICA desta conversa, e nunca deve ser lido como capacidade ausente, desligada ou
  nao suportada. O texto tambem nao afirma o contrario: `withheld` e lista fixa, entao
  o `null` significa apenas "nao divulgado nesta conversa" e nao prova que o recurso
  exista. Nenhum dado novo e exposto: e so honestidade de texto sobre o que o relatorio
  ja fazia.
