- **O id da chamada de ferramenta chega ao `/ws` e ao host MCP (#1915).** Os
  frames `tool_started` e `tool_finished` ganharam o campo `call_id`, e cada item
  de `tool_calls` no envelope `garra.agent.v1` tambem. E o mesmo valor no inicio
  e no fim da chamada. Antes so o nome da ferramenta ia no frame, entao duas
  execucoes da mesma ferramenta no mesmo turno nao tinham como ser pareadas por
  quem lia o stream. Campo aditivo: quem nao o le nao muda.
