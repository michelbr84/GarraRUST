- **`/status`, `/tools` e `cli_inspect` passam a ler a MESMA fonte do inventario
  de ferramentas (#1712, fatia 5).** O `/status` contava
  `AgentRuntime::list_tool_info()` — toda tool registrada — e chamava o total de
  "ferramentas"; a tool `cli_inspect` reportava so o que o portao do turno
  liberou. Os dois numeros pareciam discordar, e o painel `/tools` se chamava
  "Ferramentas disponiveis" listando o que estava apenas REGISTRADO. Agora um
  coletor comum (`AgentRuntime::visao_das_ferramentas`, em
  `garraia-agents/src/ferramentas_visiveis.rs`) produz inventario + origem +
  disponibilidade operacional + liberacao observada, e as duas projecoes o
  consomem: a de interface mostra as registradas e diz que a liberacao e por
  turno (o portao da CLI depende do texto do pedido, entao nao ha "liberado"
  honesto fora de um turno); a do modelo continua fail-closed, omitindo o que o
  portao nega. `/tools` passa a marcar a ferramenta registrada mas indisponivel
  com o codigo do motivo, e o relatorio do `cli_inspect` ganha
  `registered_total` ao lado de `count`. Nenhuma permissao mudou: o conjunto que
  o modelo enxerga e o mesmo que o despacho libera.
