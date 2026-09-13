---
name: matter
description: Presets Matter via Home Assistant — mesma decisão do Zigbee, a stack vive no hub e o Garra herda as entidades.
kind: hardware-preset
triggers:
  - matter
  - thread
provides:
  transport: home_assistant
  presets:
    - entity: light.matter_escritorio
      capability: power
      synonyms: ["luz do escritorio", "office light"]
    - entity: switch.matter_tomada_escritorio
      capability: power
      synonyms: ["tomada do escritorio", "office outlet"]
---

# Matter

Mesma tese do skill `zigbee`: a stack Matter/Thread (comissionamento,
fabric, border router) fica no Home Assistant, e o Garra consome as
entidades resultantes pelo adapter que já existe.

Se e quando um transporte Matter nativo fizer sentido, ele entra como
**adapter no core** — com PR, avaliação de risco e entrada na lista fechada
de transportes —, não como manifesto de skill. Um skill que declarasse
`transport: matter` hoje carregaria inerte: visível na listagem, com o
motivo, e sem nunca virar dispositivo.
