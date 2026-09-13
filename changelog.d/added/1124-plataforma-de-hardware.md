- Plataforma de hardware completa (epic #1124, ADR 0020): as sete slices do
  epic estao entregues — crate `garraia-hardware` com `trait Device` e
  `Capability` carregando risco R0-R5 desde o primeiro commit (#1125/#1129),
  adapters MQTT (#1126), Home Assistant (#1127) e Serial/USB + GPIO (#1130),
  motor de automacoes `trigger -> condicao -> acao` sob a mesma policy do
  runtime (#1128) e integracoes empacotadas como hardware skills (#1131). A
  visao geral da plataforma — arquitetura em camadas, o north star passo a
  passo, o modelo de risco em uma tela e o que fica de fora (Modbus e ROS2
  ainda sem adapter; Zigbee e Matter por preset sobre o hub) — esta em
  `docs/hardware.md`.
