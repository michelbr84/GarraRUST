- As quatro copias do loop de turno do `AgentRuntime` agora despacham tools
  por uma unica funcao (`dispatch_tool_call`): orcamento, deteccao de loop,
  gate do modo (#988), eventos de ciclo de vida, timeout e pausa de
  confirmacao vivem num lugar so (#1226, slice S-A).
