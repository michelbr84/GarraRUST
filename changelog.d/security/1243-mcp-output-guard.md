- **O resultado de tool MCP entra no contexto do modelo pelo guard de
  injecao indireta, com teto de bytes (#1243, fatia 1).** O guard entregue
  na #1213 estava ligado em um ponto so (`web_fetch`); todo outro canal
  por onde texto de terceiro chega ao modelo escapava dele. O canal mais
  grave era o MCP: um servidor de terceiro — tipicamente um `npx` buscado
  sob demanda — devolvia texto que entra no contexto cru, sem guard e sem
  teto de tamanho, e um payload gigante era vetor de exaustao de
  contexto/custo. Agora `McpTool::execute` passa o conteudo por
  `garraia_security::sanitize_indirect` (caracteres invisiveis removidos;
  suspeito chega precedido do banner de dado nao-confiavel, como no
  `web_fetch`) e ganha teto de 256 KiB alinhado ao
  `MAX_CONNECTOR_FRAME_BYTES` dos conectores, com truncamento explicito e
  visivel — nunca em silencio, e nunca no meio de um caractere UTF-8
  multibyte (corte por fronteira de char). Prova ponta a ponta com
  servidor MCP de verdade: payload com instrucao injetada chega
  emoldurado; payload de 300 KiB chega truncado com marca. Pendencias
  nomeadas na issue: `file_read` (fatia 2) e `device_read`/`device_list`
  (fatia 3); a cobertura real agora esta nomeada no threat-model §5.5 e
  na tabela de comparacao (que vendia cobertura de `review` que nao
  existia).
