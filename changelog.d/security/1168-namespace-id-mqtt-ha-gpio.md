- **Adapters MQTT, Home Assistant e GPIO namespaceiam o id de registro (#1168).**
  `valida_segmento_topico` do adapter MQTT so recusava segmento vazio, `/`,
  `+` e `#` — `:` e `.` continuavam validos, os mesmos separadores usados
  pela convencao de id de outros adapters (`serial:<id>` e o
  `<dominio>.<objeto>` do Home Assistant). Um dispositivo MQTT malicioso ou
  mal configurado que publicasse um manifesto com esse formato de `id`
  reivindicava a mesma chave no `DeviceRegistry` compartilhado, e
  `DeviceRegistry::register` substituia o ocupante em silencio — um
  `digital_write` enderecado a um dispositivo ja adotado por outro
  transporte passaria a sair por um broker diferente. O adapter GPIO tinha o
  mesmo defeito por outra porta: o id da config aceita `.`, entao um
  `light.sala` declarado nos pinos do Pi colidia com o `entity_id` nativo de
  uma entidade do Home Assistant.
  Os tres adapters agora registram sob namespace proprio (`mqtt:<id>`,
  `ha:<entity_id>`, `gpio:<id>`) em vez do id bruto/nativo, fechando o
  conjunto com o `serial:<id>` que a #1130 ja tinha. Com os quatro
  transportes namespaceados, a colisao entre adapters deixa de ser uma
  coincidencia a evitar e vira estruturalmente impossivel, independente do
  que um dispositivo anuncie. O `DeviceStateStore` e o `HardwareEventBus`
  (motor de automacoes, #1128) seguem o mesmo id namespaceado, para ficar
  consistente com o que `device_list` mostra ao agente.
  `DeviceRegistry::register` passa a ser documentado como a variante
  intencional de "sim, isto deve sobrescrever" (reconexao MQTT, reentrega de
  estado do Home Assistant), com `register_if_absent` — fail-closed, recusa
  a colisao em vez de substituir — como a alternativa para quem registra uma
  vez so.
  Sem impacto em producao: `garraia-hardware` nunca foi lancado (todo o
  slice de hardware, incluindo automacoes e os adapters serial/GPIO, chegou
  depois da tag v0.4.1).
