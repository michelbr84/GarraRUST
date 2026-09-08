- **Ferramenta `garra_status`: o agente passa a conseguir descrever o proprio
  runtime (#1035).** Perguntado o que era, o Garra no celular respondia que "nao consegue
  inspecionar o estado interno do runtime a partir desta conversa" — e estava
  certo, nada permitia: `/api/health` e `/api/capabilities` existem para o console,
  `web_fetch` recusa loopback por desenho (guard de SSRF) e `bash` teria de
  adivinhar host e porta. A tool le o mesmo `AppState` daqueles endpoints (versao,
  uptime, provider e modelo ativos, ferramentas registradas, features, canais,
  modo e diretorio da sessao) sem fazer requisicao nenhuma, entao a superficie de
  SSRF fica onde estava. Sem segredo na saida: ids de provider e nomes de modelo,
  nunca chaves. A persona menciona as ferramentas, para o modelo usa-las antes de
  dizer que nao consegue.
