- **Comandos com barra passam a funcionar pelo HTTP, e o app sugere ao
  digitar `/` (#1040).** No celular, `/help` voltava "nao ha comandos com barra
  registrados nesta instalacao" — alucinacao: o registry estava cheio (17
  comandos populados no boot), mas o unico call site de
  `CommandRegistry::dispatch` era o adapter do Telegram, e pelo HTTP o texto ia
  cru para o modelo. `POST /api/sessions/{id}/messages` despacha pelo registry
  quando o texto comeca com um nome registrado (desconhecido segue ao modelo,
  como antes; papel `User`, entao comandos de owner respondem "permission
  denied"; `/start`, que reivindica o dono da allowlist do Telegram, nem e
  aceito pelo HTTP). `CommandContext` ganha `session_id`, e os comandos de
  sessao (`/clear`, `/mode`, `/goal`) usam o id do HTTP quando existe. `GET
  /api/slash-commands` e o `commands` de `/api/capabilities` passam a listar o
  registry no papel do HTTP, em vez de uma tabela paralela de dois itens e da
  lista completa com comandos de owner. O dispatcher solta o lock do registry
  antes de executar (o `/help` le o registry de novo; um read segurado sobre
  outro read trava assim que um writer entra na fila) e `/model` valida o nome.
  No app,
  `SlashSuggestions` mostra chips dos comandos que casam com o prefixo digitado
  e os chips da tela Skills abrem o chat com o comando pronto.
