- Fix(security): fronteira de autorizacao dos turnos — os wrappers de conveniencia
  do runtime sem `ExecContext` explicito (`process_message`,
  `process_message_with_context`, `process_heartbeat` e o streaming equivalente)
  passam a ser fail-closed (teto de leitura, sem prova de operador); o teto da
  superficie vira camada propria composta por E com o teto do principal
  (nenhuma camada amplia a outra — inclusive no WhatsApp vinculado); o bot do
  workspace e as tarefas agendadas passam pelo construtor central de contexto,
  preservando o vinculo com a sessao que autorizou o turno. A2A, MCP HTTP e CLI
  mantem seus fluxos por confianca declarada (credencial de operador / operador
  local). Follow-up do #1666 (#1985).
