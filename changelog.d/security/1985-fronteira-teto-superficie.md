- Fix(security): fronteira de autorizacao dos turnos — os wrappers de conveniencia
  do runtime sem `ExecContext` explicito (`process_message`,
  `process_message_with_context`, `process_heartbeat` e o streaming equivalente)
  passam a ser fail-closed (teto de leitura, sem prova de operador); o teto da
  superficie vira camada propria composta por E com o teto do principal
  (nenhuma camada amplia a outra — inclusive no WhatsApp vinculado); o bot do
  workspace e as tarefas agendadas passam pelo construtor central de contexto,
  preservando o vinculo com a sessao que autorizou o turno. A2A, MCP HTTP e CLI
  mantem seus fluxos por confianca declarada (credencial de operador / operador
  local). **Mudanca de comportamento:** com o gateway sem `gateway.api_key`, as
  tarefas de `/a2a/*` passam a rodar com teto de leitura (sem `bash`, escrita ou
  envio de mensagem), porque nesse caso a rota nao exige credencial; com a chave
  configurada nada muda. Follow-up do #1666 (#1985).
- **Tarefa agendada roda com a autoridade de quem a criou (#1985).** As tools
  de agenda gravam na tarefa os tetos do turno que a criou
  (`scheduled_tasks.authority`, coluna aditiva); na execucao valem esses tetos,
  compostos por E com a politica de acesso ATUAL do WhatsApp vinculado para
  quem criou a tarefa. Agendar nao eleva privilegio e tambem nao reduz: o dono
  do WhatsApp com `full` nao perde a escrita na tarefa que agendou, e um
  contato rebaixado ou bloqueado depois perde nas tarefas que ja tinha. Fora do
  WhatsApp vinculado, a tarefa do operador cai para leitura se a sessao ganhar
  um canal remoto depois da criacao (quem escreve na conversa alcanca o
  historico que a tarefa le).
  **Mudanca de comportamento:** tarefa criada antes deste registro, numa sessao
  remota, passa a rodar so com leitura ate o operador reautorizar; a resposta
  dela comeca por um aviso na primeira execucao. Tarefa de sessao do operador
  nao muda.
