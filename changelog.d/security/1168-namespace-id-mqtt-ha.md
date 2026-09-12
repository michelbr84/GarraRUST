- **Adapters MQTT e Home Assistant namespaceiam o id de registro (#1168).**
  `valida_segmento_topico` do adapter MQTT so recusava segmento vazio, `/`,
  `+` e `#` — `:` e `.` continuavam validos, os mesmos separadores usados
  pela convencao de id de outros adapters (`serial:<id>` e o
  `<dominio>.<objeto>` do Home Assistant). Um dispositivo MQTT malicioso ou
  mal configurado que publicasse um manifesto com esse formato de `id`
  reivindicava a mesma chave no `DeviceRegistry` compartilhado, e
  `DeviceRegistry::register` substituia o ocupante em silencio — um
  `digital_write` endereçado a um dispositivo ja adotado por outro
  transporte passaria a sair por um broker diferente. Os dois adapters agora
  registram sob namespace proprio (`mqtt:<id>`, `ha:<entity_id>`) em vez do
  id bruto/nativo, o que torna a colisao entre adapters estruturalmente
  impossivel independente do que um dispositivo anuncie. O
  `DeviceStateStore` e o `HardwareEventBus` (motor de automacoes, #1128)
  seguem o mesmo id namespaceado, para ficar consistente com o que
  `device_list` mostra ao agente.
  `DeviceRegistry` ganha `register_if_absent` (falha fechada: recusa
  colisao em vez de substituir) como alternativa explicita ao `register`
  existente, que passa a ser documentado como a variante intencional de
  "sim, isto deve sobrescrever" (reconexao MQTT, reentrega de estado do
  Home Assistant).
  Namespacear o adapter serial e o GPIO da #1130 fica para quando a PR
  #1167 (ainda aberta) integrar — o codigo deles nao existe em `main` hoje.
  Sem impacto em producao: `garraia-hardware` nunca foi lancado (todo o
  slice de hardware, incluindo automacoes, chegou depois da tag v0.4.1).
