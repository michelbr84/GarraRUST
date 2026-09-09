- O chat do Garra Mobile passa a mostrar a resposta enquanto ela acontece, em
  vez de esperar o turno inteiro. O app abre o `/ws` do gateway, retoma a
  sessao que ja tinha e vai concatenando os `delta` numa bolha que cresce;
  `tool_started` / `tool_finished` viram um indicador de qual ferramenta esta
  rodando. Um botao Parar aparece durante o turno e manda `{"type":"stop"}`
  **sempre com o `session_id`** — um stop sem endereco cancelaria qualquer
  turno que o socket estivesse carregando. O texto parcial que chegou antes do
  `stopped` vira a mensagem final: o gateway nao persiste turno cancelado,
  entao descartar aqui apagaria o que o usuario ja tinha lido (#1081).
- Quando o socket nao abre — gateway antigo sem `/ws`, proxy que recusa o
  upgrade, LAN que caiu — o app volta sozinho para o `POST` de sempre, sem
  mostrar erro. O fallback so vale antes de o turno ser submetido; depois
  disso, repetir por HTTP mandaria a mesma mensagem duas vezes (#1081).
- Um frame que o app nao conhece e ignorado em vez de derrubar o turno: o
  gateway pode ganhar um tipo novo antes de o app aprender (#1081).
