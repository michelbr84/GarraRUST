- **Leitura de sessao por id separada em cliente e operador (#1462, opcao 3).**
  `GET /api/sessions/{id}/history` devolvia o historico verbatim de qualquer
  sessao em memoria — e a de um canal esta em memoria no caso normal, porque
  a hidratacao do canal a poe la —, numa rota `/api/*` auth-free por padrao,
  sem LLM no meio e com ids adivinhaveis por construcao. Agora a leitura de
  **cliente** por id (`GET /api/sessions`, `GET /api/sessions/{id}/history`,
  `DELETE /api/sessions/{id}` e o `resume` sem token do `/ws`) so alcanca
  sessao das superficies locais do operador (`api`, `vscode`, `web`,
  `parrot`), como a escrita ja fazia desde a PR #1468, e a regra mora num
  lugar so (`AppState::sessao_da_api`): sessao de canal ou do mobile responde
  o mesmo `404 session not found` de id inexistente, byte a byte, e nao e
  hidratada, desconectada nem listada — em memoria e no `sessions.db`. A
  leitura do **operador** ganha `GET /admin/api/sessions/{id}/history`
  (cookie do `/admin` + `manage_sessions`; `viewer` recebe 403), que le
  qualquer sessao, em memoria ou so no disco, sem hidratar — a hidratacao
  reescreveria a linha do canal — e registra cada leitura na auditoria. O
  Export da pagina Sessions do Web Console passa a usar essa rota (e a
  listagem administrativa) quando o navegador esta logado no `/admin`; sem
  login, mostra so as sessoes locais e diz onde entrar. O historico do mobile
  (`GET /chat/history`) continua vindo do `sub` do JWT — cada usuario le so a
  propria sessao.
