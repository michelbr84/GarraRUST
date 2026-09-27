- **Classes de capacidade e teto por principal (#1385, #1392, ADR 0025).** Toda
  ferramenta declara o que faz em classes fechadas (`filesystem.read`,
  `filesystem.write`, `process.execute`, `network.read`, `message.send`,
  `device.*`, `memory.*`, `runtime.inspect`, `scheduling`, `mcp.read`,
  `mcp.write`): nativas por tabela fechada, MCP pela operacao de filesystem
  conhecida ou pelas anotacoes do servidor; sem classe e fail-closed em modo
  restrito. `ToolPolicy` ganha `allowed_capabilities`, `denied_capabilities` e
  `no_tools`; o `ExecContext` ganha um teto (`TetoDeCapacidades`) composto por
  E com o modo em todo ponto do runtime — a lista que o modelo ve, o despacho e
  o `garra_status`. Os niveis `chat|read|full` e o `write on|off` do WhatsApp
  compilam para esse teto (`politica_do_nivel`): `write` mexe so em escrita de
  arquivo, nativa e MCP, e nunca desliga sandbox, jail nem confirmacao. A
  recusa diz se foi o modo ou a politica de acesso. O inventario de
  ferramentas expoe as classes.
