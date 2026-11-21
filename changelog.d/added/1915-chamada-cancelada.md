- **Chamada de ferramenta interrompida pelo cancelamento do turno agora fecha
  com o desfecho `cancelled` (#1915).** Cancelar um turno (Ctrl+C da
  `garra chat`, teto de parede, cliente que desistiu) descarta o futuro do turno;
  com uma ferramenta em andamento, o `ToolStarted` saia e o `ToolFinished` nunca
  vinha, e quem lia o fluxo ficava com a chamada aberta. Um vigia armado logo
  depois do inicio emite o fim com `outcome: cancelled` (`success: false`) se o
  futuro cair antes do fim normal; num `tool_program`, o passo fecha antes do
  envelope. E melhor esforco (`try_send` no `Drop`): a `garra chat` drena o canal
  depois do cancelamento e recebe o evento. O codigo novo chega aditivo ao frame
  `tool_finished` do `/ws` e ao resumo de tool do host MCP.
