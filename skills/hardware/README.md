# Hardware skills (#1131)

Cada diretório aqui é uma integração empacotada: manifesto em `SKILL.md`,
lido por `garraia_skills` e convertido em catálogo por
`garraia_hardware::skills` (feature `skills`).

| Skill | `kind` | Transporte | O que traz |
|---|---|---|---|
| `home-assistant/` | `hardware-adapter` | `home_assistant` | entidades do hub → dispositivos |
| `mqtt/` | `hardware-adapter` | `mqtt` | descoberta e pub/sub no broker |
| `serial-arduino/` | `hardware-adapter` | `serial` | placas por USB, protocolo JSONL |
| `esp32/` | `hardware-preset` | `mqtt` | vocabulário de placas ESP32 |
| `zigbee/` | `hardware-preset` | `home_assistant` | vocabulário Zigbee (via ZHA/z2m) |
| `matter/` | `hardware-preset` | `home_assistant` | vocabulário Matter/Thread |

## As duas regras

1. **A lista de transportes é fechada** — `mqtt`, `home_assistant`, `serial`,
   `gpio`. Um manifesto que declare outro (`modbus`, `ros2`) carrega
   **inerte**: aparece na listagem com o motivo e nunca vira adapter. Por isso
   não há skill de Modbus nem de ROS2 aqui: eles entram quando o adapter
   entrar no core, com PR e avaliação de risco próprios.
2. **Um skill só sobe risco, nunca baixa.** O risco nasce no adapter; um
   preset pode declarar `risk:` maior (a tomada que alimenta o portão), e o
   catálogo aplica `max()`. Um manifesto que tente rebaixar `door_unlock` de
   R3 para R1 não muda o gate — há teste para isso.

Detalhes e o formato completo do frontmatter: `docs/hardware-skills.md`.
