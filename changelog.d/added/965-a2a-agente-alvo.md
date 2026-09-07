- **`POST /a2a/tasks` passa a rotear para um agente nomeado (#965).** O corpo
  aceita `target` (e as grafias `agentId` e `agent_id`, que a issue cita) para
  escolher entre os agentes que o operador configurou. Sem o campo, nada muda:
  o agente padrao atende como sempre atendeu, e nenhum cliente A2A existente
  quebra.
- **Um alvo desconhecido e recusado com 400, e nunca cai no padrao em
  silencio.** O `agent_router::resolve` cai — e esta certo, porque ele existe
  para escolher quando ninguem escolheu. Mas quem pede a Hera e recebe a Garra
  com 200 e sem sinal nenhum acredita ter falado com quem nao falou; e a mesma
  forma do `/mode` decorativo que este lote inteiro vem eliminando. Entrou um
  `resolve_exact` que devolve ausencia, e o teste poe os dois lado a lado na
  mesma entrada para o contraste ficar no CI.
- **A resposta 400 lista os nomes conhecidos.** Nao vaza nada: o
  `GET /.well-known/agent.json` ja publica todos como `skills`. Sem a lista, o
  chamador so poderia adivinhar.
- **O nome pedido nao entra no log.** Ele vem de um endpoint sem
  autenticacao, e ecoar entrada de fora para o arquivo de log e o caminho
  curto para poluicao e injecao de linha.
