- **`/clear` apaga o historico gravado, nao so a copia em memoria (#1723).** O
  comando respondia "Conversation history cleared", mas so esvaziava a memoria;
  no turno seguinte `hydrate_session_history` via a memoria vazia e recarregava
  tudo do `sessions.db` -- o agente voltava a saber o que o usuario pediu para
  esquecer, e `GET /api/sessions/{id}/history` seguia devolvendo as mensagens
  antigas, inclusive depois de um restart. Agora `SessionStore::
  clear_session_messages` apaga mensagens e resumos da sessao numa transacao, e
  o `/clear` do gateway (API, Telegram, WhatsApp) e o do Discord chamam a
  limpeza; uma falha ao apagar o disco e dita na resposta em vez de escondida.
