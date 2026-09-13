# O mundo físico no Garra — a plataforma de hardware

> Epic [#1124](https://github.com/michelbr84/GarraRUST/issues/1124), entregue em
> 2026-09-13. Decisão: [ADR 0020](adr/0020-crate-garraia-hardware.md).
> Formato do manifesto de skill: [`docs/hardware-skills.md`](hardware-skills.md).

O Garra não "suporta GPIO" como feature. O mundo físico é mais uma superfície
que o agente enxerga e opera, ao lado de arquivos, chat e ferramentas web — e
ela entra pela mesma porta que todas as outras: uma tool, um gate, uma policy.

```text
Garra Core   (agent runtime · memória · automações · segurança)
     |
garraia-hardware   trait Device · Capability com risco R0-R5 · registry · gate
     |
Adapter / Hardware Skill   mqtt · home_assistant · serial · gpio
     |
Device   a lâmpada, o sensor, a fechadura, a placa
```

Cada camada só conhece a de baixo pela interface. O core nunca sabe o nome de
um fabricante; o adapter nunca decide se uma ação é permitida; o dispositivo
nunca escolhe o próprio nível de risco quando não passou por ACL nenhuma.

## O que foi entregue

| Slice | Issue | O que existe hoje |
|---|---|---|
| Crate + `trait Device` | [#1125](https://github.com/michelbr84/GarraRUST/issues/1125) | `Device` (async, `dyn`), `Capability`, `DeviceRegistry`, `DeviceStateStore`, `MockDevice` |
| Modelo de risco R0-R5 | [#1129](https://github.com/michelbr84/GarraRUST/issues/1129) | `RiskClass` + tabela `decisao()` fail-closed + `HardwareGate`, nascidos junto com a crate |
| Adapter MQTT | [#1126](https://github.com/michelbr84/GarraRUST/issues/1126) | `rumqttc` sobre TCP, convenção `garra/devices/…`, descoberta por manifesto retido |
| Adapter Home Assistant | [#1127](https://github.com/michelbr84/GarraRUST/issues/1127) | REST + WebSocket, entidades → capabilities, URL pelo guard de SSRF |
| Motor de automações | [#1128](https://github.com/michelbr84/GarraRUST/issues/1128) | specs declarativas TOML/JSON, `trigger → condição → ação`, mesma policy do runtime |
| Serial/USB + GPIO | [#1130](https://github.com/michelbr84/GarraRUST/issues/1130) | JSONL sobre porta serial, pinos do Raspberry Pi, tabela **fechada** de periféricos |
| Hardware skills | [#1131](https://github.com/michelbr84/GarraRUST/issues/1131) | manifesto `hardware-adapter`/`hardware-preset`, catálogo, 6 skills oficiais |

A porta do agente são três tools em `garraia-agents`: `device_list` (descoberta),
`device_read` (leitura) e `device_execute` (ação).

## O north star, passo a passo

> "Garra, quando eu chegar em casa depois das 19h, se estiver escuro, ligue as
> luzes da sala, coloque o ar em 23 graus e me diga se alguma porta ficou
> aberta."

O que cada pedaço dessa frase vira no código entregue:

| Pedaço | Camada | Como |
|---|---|---|
| "quando eu chegar em casa depois das 19h" | automações (#1128) | trigger de evento + condição de horário (`croner`, fuso local do host) |
| "se estiver escuro" | `device_read` (R0) | sensor de luminosidade, leitura automática pela tabela de risco |
| "ligue as luzes da sala" | `device_execute` (R1) | resolvido por preset ("luz da sala" → `ha:light.sala_teto`), decidido pela policy do turno |
| "coloque o ar em 23 graus" | `device_execute` (R1/R2) | entidade `climate.*` do Home Assistant |
| "me diga se alguma porta ficou aberta" | `device_read` (R0) | `binary_sensor.*`, leitura pura |

E o que a mesma frase **não** consegue: se ela terminasse em "e destranque a
porta da frente", a automação pararia. `lock.*` é R3, R3 exige confirmação
humana, e automação roda desacompanhada — sem canal de confirmação, a decisão
é `Denied`. O que o usuário não pediria ao agente diretamente, a automação
também não faz.

## O modelo de risco, em uma tela

| Nível | O que é | Decisão |
|---|---|---|
| R0 | leitura (`temperature`, `door_status`) | automático |
| R1 | ação reversível (`light_on`, `volume`) | policy/modos do turno |
| R2 | mudança física leve (`cover_position`, `scene`) | policy + rate limit |
| R3 | acesso/segurança (`door_unlock`, `garage_open`) | confirmação humana (fluxo GAR-187) |
| R4 | risco físico (`robot_motion`, `oven_on`) | approval explícito |
| R5 | proibido por padrão (industrial, alarme) | deny, só com allowlist do operador |

Três invariantes prendem a tabela:

1. **Leitura ↔ R0.** Não existe capability read-only com risco > R0 nem
   capability R0 que escreva. `Capability::validar()` recusa as duas formas.
2. **Fail-closed sem canal.** Sem canal real de confirmação (o caminho MCP
   stateless full-auto, heartbeats, automações), R3/R4/R5 não rodam. Um pedido
   de confirmação que ninguém pode responder nunca vira execução silenciosa.
3. **Risco não vem de quem não passou por ACL.** Uma placa plugada num cabo USB
   não classifica a si mesma: serial e GPIO usam a tabela fechada de
   `perifericos` (`digital_read`/`analog_read` R0, `digital_write`/`pwm` R2). E
   um skill empacotado só consegue **subir** risco — o efetivo é
   `max(adapter, skill)`.

## Duas camadas de decisão, não uma

1. **Policy (modos/`ToolGate`)** decide se a tool roda no turno: modos
   read-only negam `device_execute` estaticamente.
2. **Risco (`HardwareGate`)** decide, dentro da tool, o que aquela capability
   específica exige — com impressão digital `(tool, assunto)` no fluxo de
   confirmação, onde o assunto é `"{device}/{capability}: {args}"`. Um "ok"
   dado a `lampada/power` não autoriza `garagem/door_unlock` no mesmo turno.

Nenhuma das duas é sistema novo: são o `ToolPolicy` e o `ToolApproval` que já
governam o bash e as demais tools.

## O registry começa vazio

Em produção, `DeviceRegistry` nasce sem nada. Sem adapter configurado,
`device_list` lista zero dispositivos — não há descoberta de rede acontecendo
por padrão, e cada transporte é uma feature de compilação desligada por
default (`mqtt`, `home-assistant`, `hardware-serial`, `hardware-gpio`,
`skills`). Quem não usa hardware não paga a árvore de dependências dele nem
carrega superfície de ataque.

Ids de registro são namespaceados por transporte (`mqtt:`, `ha:`, `serial:`,
`gpio:` — #1168): um dispositivo que se anuncie com o id de outro nunca
alcança a chave alheia, e no transporte em que o *dispositivo* escolhe o id
(serial) o registro é `register_if_absent` — a segunda placa com o mesmo id é
recusada, não promovida.

## O que fica de fora, e por quê

- **Zigbee e Matter** chegam pelo Home Assistant, como preset. A stack
  (coordenador, pareamento, fabric) já está resolvida no hub; escrevê-la no
  core seria assumir manutenção de firmware para ganhar dispositivos que o hub
  já entrega.
- **Modbus e ROS2** não têm adapter. Um manifesto que os declare hoje carrega
  **inerte** — visível na listagem, com o motivo, nunca ativo. Eles entram
  quando entrar o adapter, com PR e avaliação de risco próprios: ROS2 por
  último, como o epic sequenciou, porque é o de maior superfície física.
- **Credenciais** nunca moram em skill. Token do hub, senha do broker e
  caminho de porta são config do gateway.

## Por onde começar

```bash
cargo test -p garraia-hardware                     # core: trait, risco, gate, registry
cargo test -p garraia-hardware --features mqtt     # adapter MQTT (broker in-process)
cargo test -p garraia-hardware --features skills   # catálogo + skills oficiais do repo
```

- Formato do manifesto e as regras do catálogo: [`docs/hardware-skills.md`](hardware-skills.md)
- Skills oficiais: [`skills/hardware/`](../skills/hardware/)
- Decisão de arquitetura e alternativas consideradas: [ADR 0020](adr/0020-crate-garraia-hardware.md)
