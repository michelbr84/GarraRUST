---
name: home-assistant
description: Adapter oficial do Home Assistant — entidades do hub viram dispositivos do Garra (luz, clima, sensor, fechadura, cortina).
kind: hardware-adapter
triggers:
  - home assistant
  - hass
  - casa inteligente
provides:
  transport: home_assistant
  capabilities: [light, climate, sensor, lock, cover]
  presets:
    - entity: light.sala_teto
      capability: power
      synonyms: ["luz da sala", "luz do teto da sala", "living room light"]
    - entity: climate.sala
      capability: temperature
      synonyms: ["ar da sala", "ar condicionado da sala", "living room ac"]
    - entity: lock.porta_frente
      capability: door_unlock
      risk: r3
      synonyms: ["porta da frente", "fechadura da frente", "front door"]
---

# Home Assistant

O adapter de maior alavancagem do epic: um hub de Home Assistant já fala
Zigbee, Matter, Z-Wave, Thread e centenas de integrações de fabricante. O
Garra não reimplementa nenhuma delas — ele lê as **entidades** do hub e as
expõe como dispositivos com capabilities.

## O que este skill declara

- `transport: home_assistant` — o transporte vive no core
  (`garraia-hardware`, feature `home-assistant`): REST para estados e
  serviços, WebSocket para `state_changed`. O skill **descreve**; quem fala
  com o hub é o adapter compilado.
- Presets de exemplo com os nomes que as pessoas usam de verdade ("luz da
  sala"), em português e inglês.

## Configuração

O adapter precisa da URL do hub e de um token de acesso de longa duração.
Ambos são config do gateway — **nunca** entram neste arquivo: skill é
conteúdo versionado e distribuível, token é segredo.

A URL passa pelo guard de SSRF (`garraia_common::ssrf`) com
`IpScope::AllowPrivate`, porque o hub legítimo é local/LAN — e o guard ainda
bloqueia link-local (`169.254.169.254`), CGNAT e multicast.

## Risco

O risco de cada capability nasce do domínio da entidade no adapter:
`light.*` e `switch.*` em R1, `cover.*` em R2, `lock.*` em R3. Um preset
deste skill pode **subir** esse risco (é o caso do `lock.porta_frente`
acima, que reafirma R3), nunca baixá-lo — o catálogo aplica `max()` entre o
risco do adapter e o do skill.

## Editar para a sua casa

Copie os presets, troque os `entity:` pelos seus `entity_id` reais (a lista
está em Developer Tools → States, no próprio Home Assistant) e ajuste os
sinônimos para como você fala. Um preset errado não quebra nada: ele
simplesmente não resolve para dispositivo nenhum.
