---
name: zigbee
description: Presets Zigbee via Home Assistant (ZHA) ou zigbee2mqtt — o core não implementa a stack Zigbee, ele herda as entidades do hub.
kind: hardware-preset
triggers:
  - zigbee
  - zha
  - zigbee2mqtt
provides:
  transport: home_assistant
  presets:
    - entity: light.zigbee_quarto
      capability: power
      synonyms: ["luz do quarto", "bedroom light"]
    - entity: binary_sensor.zigbee_porta_quintal
      capability: door_status
      synonyms: ["porta do quintal", "backyard door"]
---

# Zigbee

**O Garra não fala Zigbee — e não precisa.** A stack (coordenador, rede
mesh, pareamento) já está resolvida pelo ZHA do Home Assistant ou pelo
zigbee2mqtt, e os dois publicam entidades que o Garra já enxerga. Este skill
é só o vocabulário.

É a decisão de desenho do epic #1124: Zigbee e Matter chegam pelo hub, não
como implementação própria. Escrever uma stack Zigbee no core seria assumir
manutenção de firmware de coordenador para ganhar dispositivos que o hub já
entrega hoje.

## Usando com zigbee2mqtt

O zigbee2mqtt publica em MQTT, não em Home Assistant. Se a sua casa for
assim, copie estes presets para um arquivo próprio com
`transport: mqtt` e ajuste as entidades para os tópicos do z2m.
