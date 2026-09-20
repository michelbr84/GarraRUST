# Hardware skills — adapters e presets empacotados

> Issue [#1131](https://github.com/michelbr84/GarraRUST/issues/1131), parte do
> epic [#1124](https://github.com/michelbr84/GarraRUST/issues/1124).
> Decisão de arquitetura: [ADR 0020](adr/0020-crate-garraia-hardware.md).

O core do Garra não conhece fabricante nem protocolo. A arquitetura é em
camadas, e a terceira é empacotável:

```text
Garra Core (agent/runtime/memory/automations/security)
        |
  garraia-hardware   (trait Device + Capability com risco R0-R5 + registry + gate)
        |
  Hardware Skill     (manifesto: transporte declarado + presets)  ← este documento
        |
      Device
```

Um skill de hardware **descreve**; quem executa é sempre código compilado no
binário. É essa separação que permite instalar integração de terceiros com o
mesmo gating dos skills comuns sem abrir um caminho para o mundo físico.

## O manifesto

Um skill de hardware é um `SKILL.md` — frontmatter YAML + corpo markdown, o
mesmo formato de sempre — em `skills/hardware/<slug>/SKILL.md` (no repo) ou
`<config_dir>/skills/hardware/<slug>/SKILL.md` (instalado). A varredura desce
em subdiretórios até três níveis; skills soltos na raiz continuam funcionando.

```yaml
---
name: home-assistant
description: Adapter oficial do Home Assistant
kind: hardware-adapter          # instruction (default) | hardware-adapter | hardware-preset
provides:
  transport: home_assistant     # lista fechada: mqtt | home_assistant | serial | gpio
  capabilities: [light, climate, sensor, lock, cover]
  presets:
    - entity: light.sala_teto   # sem ':' — o prefixo é do adapter
      capability: power
      risk: r1                  # opcional, r0..r5 — só sobe (ver abaixo)
      synonyms: ["luz da sala", "living room light"]
---

Corpo do skill em markdown.
```

| Campo | Obrigatório em | Papel |
|---|---|---|
| `kind` | — (default `instruction`) | a categoria; `hardware-*` habilita `provides` |
| `provides.transport` | ambos os kinds de hardware | qual transporte do core este skill usa |
| `provides.capabilities` | `hardware-adapter` | o que o transporte expõe |
| `provides.presets` | `hardware-preset` | entidade → capability, com sinônimos |
| `presets[].synonyms` | opcional (máx. 32) | vocabulário pt/en de descoberta |
| `presets[].risk` | opcional | risco declarado — honrado só se **maior** |

`kind: instruction` não pode carregar `provides`: um skill que declarasse
hardware sem se declarar de hardware ficaria invisível para a triagem por
categoria, e a validação recusa.

## As duas regras que um skill não escolhe

### 1. A lista de transportes é fechada

`mqtt`, `home_assistant`, `serial`, `gpio` — os quatro que o core sabe falar
(`garraia_hardware::skills::TRANSPORTES_SUPORTADOS`). Um manifesto que declare
`transport: modbus` é carregado **inerte**: aparece em
`CatalogoDeSkills::inertes()` com o transporte que pediu, e nunca vira adapter
ativo nem participa da resolução por linguagem natural.

Crescer a lista é trabalho de código — um adapter novo, com PR e avaliação de
risco próprios —, nunca de manifesto. É por isso que o repo ainda não
distribui skill de Modbus ou de ROS2.

### 2. Um skill só sobe risco, nunca baixa

O risk class nasce no adapter: da tabela fechada de `garraia_hardware::perifericos`
para serial/GPIO, do domínio da entidade para o Home Assistant, do manifesto
do dispositivo para o MQTT. Um preset pode declarar `risk:` **maior** — é o
caso de "esta tomada aqui alimenta o portão, trate como acesso" — e o catálogo
honra:

```text
risco efetivo = max(risco do adapter, risco declarado pelo skill)
```

O caminho inverso é exatamente o ataque que o empacotamento abriria: um
manifesto rebaixando `door_unlock` de R3 para R1 para escapar da confirmação
humana. `CatalogoDeSkills::risco_efetivo` o fecha, e há teste para as duas
direções.

Capability de leitura é o caso limite e não abre exceção: `R0 ↔ read_only` é
invariante de `Capability`, então um preset não transforma um sensor em
atuador subindo o risco dele — `capability_efetiva` devolve a leitura intacta.

## Descoberta por linguagem natural

`CatalogoDeSkills::resolver("luz da sala")` devolve os presets que casam,
normalizando caixa, acento e espaços. O casamento é por **igualdade** contra o
id de registry (`ha:light.sala_teto`), a entidade crua (`light.sala_teto`) ou
qualquer sinônimo — não por substring, de propósito: "luz" não pode casar com
as sete luzes da casa e deixar o agente escolher uma.

Skills inertes não participam: um preset apontando para um transporte que não
existe resolveria para um device que nunca será registrado.

## Skills oficiais

Versionados em [`skills/hardware/`](../skills/hardware/):

| Skill | `kind` | Transporte |
|---|---|---|
| `home-assistant` | `hardware-adapter` | `home_assistant` |
| `mqtt` | `hardware-adapter` | `mqtt` |
| `serial-arduino` | `hardware-adapter` | `serial` |
| `esp32` | `hardware-preset` | `mqtt` |
| `zigbee` | `hardware-preset` | `home_assistant` |
| `matter` | `hardware-preset` | `home_assistant` |

Zigbee e Matter chegam como **preset sobre o Home Assistant**, não como stack
própria: o hub já resolveu coordenador, pareamento e fabric, e o Garra herda
as entidades. Escrever a stack no core seria assumir manutenção de firmware de
coordenador para ganhar dispositivos que o hub já entrega.

## Segredos nunca entram no manifesto

Token do Home Assistant, credencial do broker, caminho de porta serial: tudo
isso é config do gateway. Skill é conteúdo versionado e distribuível — o que
entra nele é vocabulário e topologia, não credencial.

## Como usar do código

Em runtime o catálogo é aplicado por um wiring só —
`spawn_hardware_adapters` no `bootstrap` do gateway (a CLI a chama
também): ele carrega o catálogo do **mesmo** dir de skills que o scanner
de instruções usa, avisa os inertes e injeta o elevador no registry
**antes** de subir qualquer adapter.

```rust
use garraia_hardware::skills::CatalogoDeSkills;

let catalogo = CatalogoDeSkills::carregar(config_dir.join("skills"))?;

for skill in catalogo.inertes() {
    tracing::warn!(skill = %skill.nome, transporte = %skill.transporte_declarado,
                   "skill de hardware inerte: transporte nao suportado");
}

registry.com_elevador(catalogo.clone());           // risco efetivo na visao
// ... e a mesma Arc entra como fonte de aliases no `DeviceToolsConfig`
// das device tools — o `device_list` mostra `aliases: ...` por device.
```

O registro embrulha o device num decorator cuja visão de capabilities é
a efetiva: leitura continua R0 (invariante de `Capability`), ação recebe
o máximo entre adapter e preset, e os caminhos de `read`/`execute` passam
por dentro. Fail-closed: sem skills dir (ou catálogo vazio, ou erro de
leitura) nada muda — risco no teto do adapter, descoberta sem aliases —
e, como o elevador só sobe risco (`max`), um catálogo parcial (skill cujo
parse falhou não entra) nunca abaixa risco nenhum.

A feature `skills` do `garraia-hardware` é OFF por default, como os adapters:
quem não empacota integração como skill não paga o parser de manifesto.

## Testes

```bash
cargo test -p garraia-skills                        # manifesto, validacao, varredura
cargo test -p garraia-hardware --features skills    # catalogo + skills oficiais do repo
```

O teste de integração `crates/garraia-hardware/tests/skills.rs` carrega
`skills/hardware/` de verdade: é o gate que impede um manifesto oficial de
entrar quebrado, e o CI o roda no job de features.
