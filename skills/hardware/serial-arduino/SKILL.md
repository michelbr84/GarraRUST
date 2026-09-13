---
name: serial-arduino
description: Adapter oficial Serial/USB — placas Arduino e compatíveis falando o protocolo JSONL de linha, com o firmware de referência versionado no repo.
kind: hardware-adapter
triggers:
  - arduino
  - serial
  - usb
provides:
  transport: serial
  capabilities: [digital_read, analog_read, digital_write, pwm]
  presets:
    - entity: bancada
      capability: analog_read
      synonyms: ["placa da bancada", "workbench board"]
    - entity: bancada
      capability: digital_write
      synonyms: ["rele da bancada", "workbench relay"]
---

# Serial / Arduino

A ponte direta: uma placa plugada no cabo USB, falando JSON por linha.

## O que este skill declara

- `transport: serial` — o core (`garraia-hardware`, feature
  `hardware-serial`) abre a porta, faz o handshake e lê/escreve JSONL;
- as quatro capabilities da tabela **fechada** de periféricos.

## Firmware de referência

O sketch está em `crates/garraia-hardware/examples/firmware/` — é código
Arduino/C++, não um alvo Cargo.

## Risco: a tabela é fechada, e por quê

Ao contrário do MQTT, aqui o risco **não** vem do dispositivo. Uma placa
plugada num cabo USB não passou por ACL nenhuma, então ela não pode ser a
fonte da própria classificação: `digital_read` e `analog_read` são R0,
`digital_write` e `pwm` são R2, e isso está no código
(`garraia_hardware::perifericos`), não no manifesto que a placa envia.

O id que a placa declara também não vira chave de registry sozinho: ele sai
como `serial:<id>` e o registro usa `register_if_absent`, então uma segunda
placa não sequestra as leituras endereçadas à primeira.

## Editar para a sua bancada

Troque `entity: bancada` pelo id que o seu firmware declara no handshake.
