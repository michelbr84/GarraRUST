---
name: esp32
description: Presets para placas ESP32 — nomes em português e inglês para os pinos e sensores mais comuns, sobre o transporte MQTT.
kind: hardware-preset
triggers:
  - esp32
  - esp
provides:
  transport: mqtt
  presets:
    - entity: esp32_varanda
      capability: temperature
      synonyms: ["temperatura da varanda", "balcony temperature"]
    - entity: esp32_varanda
      capability: humidity
      synonyms: ["umidade da varanda", "balcony humidity"]
    - entity: esp32_portao
      capability: power
      risk: r3
      synonyms: ["portao", "portao da garagem", "garage gate"]
---

# ESP32

Skill de **presets**: nenhum transporte novo, nenhum driver. Um ESP32 que
publica no broker já é visto pelo adapter MQTT; o que falta é o vocabulário
— dizer "umidade da varanda" em vez de `esp32_varanda` + `humidity`.

## O preset do portão é o exemplo da regra

`esp32_portao` é, no protocolo, uma saída digital igual a qualquer outra —
o adapter a classificaria em R1/R2. Mas na casa real ela abre um portão, e
isso é acesso: o preset declara `risk: r3`, e o catálogo honra porque
**subir** risco é a direção permitida. Com R3, a execução passa a exigir
confirmação humana pelo fluxo do GAR-187, e sem canal de confirmação ela
simplesmente não roda.

O contrário não funciona: declarar `risk: r0` numa capability que o adapter
classificou em R2 não muda nada.
