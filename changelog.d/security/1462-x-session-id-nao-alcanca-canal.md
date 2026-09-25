- **Um `session_id` escolhido pelo cliente deixa de alcancar a sessao de
  outra superficie (#1462).** `POST /v1/chat/completions` aceitava
  `X-Session-Id` verbatim e `POST /api/sessions/{id}/messages` aceitava o id
  no caminho, e nenhum dos dois conferia de quem era a sessao: um id forjado
  — `whatsapp-linked-<numero>`, `telegram-<chat>`, adivinhaveis por
  construcao — hidratava a conversa da vitima (resumo + ate 100 turnos) para
  dentro do request do atacante e gravava o turno dele na conversa dela. Por
  id, agora so se alcanca sessao das superficies locais do operador (`api`,
  `vscode`, `web`, `parrot`); sessao de canal com humano do outro lado ou do
  mobile responde `404 session not found` sem confirmar que existe, e nao e
  hidratada nem escrita. A leitura `GET /api/sessions/{id}/history`, que o
  Web Console usa para exportar qualquer sessao, fica como esta — e decisao
  de produto registrada na issue.
