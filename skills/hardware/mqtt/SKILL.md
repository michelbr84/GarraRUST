---
name: mqtt
description: Adapter oficial MQTT — descoberta e pub/sub genéricos sobre o broker da casa, o transporte com o maior leque de dispositivos por linha de código.
kind: hardware-adapter
triggers:
  - mqtt
  - broker
  - mosquitto
provides:
  transport: mqtt
  capabilities: [light, switch, sensor, button]
  presets:
    - entity: tomada_varanda
      capability: power
      synonyms: ["tomada da varanda", "balcony outlet"]
    - entity: sensor_sala
      capability: temperature
      synonyms: ["temperatura da sala", "living room temperature"]
---

# MQTT

O primeiro transporte do epic, e o de maior alcance: qualquer dispositivo que
publique num broker (ESP32 caseiro, zigbee2mqtt, Tasmota, Shelly) entra sem
driver de fabricante.

## O que este skill declara

- `transport: mqtt` — o core (`garraia-hardware`, feature `mqtt`) fala TCP
  com o broker via `rumqttc` e segue a convenção de tópicos
  `garra/devices/<id>/…`;
- capabilities genéricas e dois presets de exemplo.

## Convenção de tópicos

| Tópico | Sentido |
|---|---|
| `garra/devices/<id>/manifest` | o dispositivo se anuncia (retained): id, capabilities e risco |
| `garra/devices/<id>/state` | estado atual, resposta de leitura |
| `garra/devices/<id>/get` | o Garra pede uma leitura |
| `garra/devices/<id>/set` | o Garra executa uma ação |

O id de registro sai namespaceado como `mqtt:<id>` (#1168) — um dispositivo
não alcança a chave de outro transporte.

## Risco

No MQTT, o risco chega no manifesto que o **dispositivo** publica, e o
adapter o aceita como piso — placa hostil não consegue se declarar mais
segura do que é, porque o catálogo de skills só aplica `max()` sobre ele.
Para uma tomada que na sua casa alimenta algo sério (o portão, uma bomba),
declare o risco maior num preset: subir sempre vale.

## Configuração

Endereço do broker, porta e credenciais são config do gateway, não deste
arquivo.
