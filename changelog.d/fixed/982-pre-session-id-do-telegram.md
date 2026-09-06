- **`/mode` e `/model` no Telegram gravavam numa sessao que a execucao nunca
  lia.** O GAR-202 migrou a chave de sessao de `telegram-{chat_id}` —
  adivinhavel — para um UUID do `ChatSessionManager`, mas a migracao pegou so o
  caminho de execucao. A camada de comandos continuou montando a string antiga,
  em tres lugares. O efeito era um recurso que parecia funcionar: `/mode code`
  respondia "modo definido", gravava no banco, e a proxima mensagem rodava sem
  ele; `/mode` sozinho lia da chave errada e mostrava "modo atual: ask" logo
  depois. So nao aparecia quando o `chat_session_manager` era `None`, que e o
  caminho de fallback.
- **A correcao e ter um lugar so.** `AppState::telegram_session_id` passa a ser
  a unica funcao que monta a chave, e os cinco pontos — tres de comando, dois de
  execucao — chamam ela. Duplicar a resolucao foi o que permitiu a divergencia.
- **Um teste varre o fonte** procurando `format!("telegram-{` fora do
  `state.rs`. E o unico jeito de pegar a proxima copia antes de ela divergir:
  um teste de comportamento so falharia depois de alguem reintroduzir o bug e
  alguem mais notar.
- O `user_id` continua sendo passado onde existe. Ele nao muda **qual** sessao
  e encontrada (a busca e por `chat_id`), mas define o dono no `upsert` de
  criacao — sem ele, um `/mode` antes da primeira mensagem criaria a sessao com
  dono `"anonymous"`.
