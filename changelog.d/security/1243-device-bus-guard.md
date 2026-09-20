- **A saida de `device_read`/`device_list` (e o retorno de `device_execute`)
  entra no contexto do modelo pelo guard de injecao indireta (#1243,
  fatia 3).** Id de dispositivo, nome de capability, `state` do Home
  Assistant e payload de MQTT/serial sao escritos por quem esta no
  barramento, e leitura e R0 — acontece sem confirmacao humana. Agora
  as tools de hardware passam o texto do bus por
  `garraia_security::sanitize_indirect` (caracteres invisiveis
  removidos; suspeito chega precedido do banner de dado nao-confiavel,
  com a origem nomeada). A moldura vive na tool do runtime, nao na
  crate `garraia-hardware`, pela regra do ADR 0020 de a crate de
  hardware nao classificar o proprio risco. Dispositivo que nomeia a
  si mesmo com instrucao e leitura hostil chegam emoldurados; leitura
  limpa segue byte a byte. Pendencias da issue: nenhuma — as tres
  fatias de codigo estao fechadas.
