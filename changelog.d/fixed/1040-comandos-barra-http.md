- **Comandos com barra passam a funcionar pelo HTTP, e o app sugere ao
  digitar `/` (#1040).** No celular, `/help` voltava "nao ha comandos com barra
  registrados nesta instalacao" — alucinacao: o registry estava cheio (17
  comandos populados no boot), mas o unico call site de
  `CommandRegistry::dispatch` era o adapter do Telegram, e pelo HTTP o texto ia
  cru para o modelo. `POST /api/sessions/{id}/messages` despacha pelo registry
  quando o texto comeca com um nome registrado (desconhecido segue ao modelo,
  como antes; papel `User`, entao comandos de owner respondem "permission
  denied"). `CommandContext` ganha `session_id`, e os comandos de sessao
  (`/clear`, `/mode`, `/goal`) usam o id do HTTP quando existe. `GET
  /api/slash-commands` passa a listar o registry em vez de uma tabela paralela
  de dois itens — acabou a divergencia com `/api/capabilities`. No app,
  `SlashSuggestions` mostra chips dos comandos que casam com o prefixo digitado
  e os chips da tela Skills abrem o chat com o comando pronto.
