# Novidades da v0.4.2

> 🇬🇧 [English version](Whats-New-v0.4.2) · 📋 [CHANGELOG completo](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md) · 📦 [Baixar](https://github.com/michelbr84/GarraRUST/releases/tag/v0.4.2)

A release em que o Garra saiu da tela. O epic de hardware fechou inteiro — as
sete partes, do `trait Device` aos skills empacotados — e com ele o agente
passa a ler sensores, acender luzes e reagir a eventos da casa **sob o mesmo
gate de risco que governa o resto das ferramentas**.

Junto vieram o streaming no app, a conversa que sobrevive ao fim do processo,
um painel admin que já não depende de `UPDATE` no SQLite para nada, e uma
faixa de segurança que fechou um vazamento de segredo pelo `/proc`.

---

## O Garra no mundo físico

Antes: o agente via arquivos, chat e a web. Agora vê também o que está
plugado.

```text
Garra Core   (runtime · memória · automações · segurança)
     |
garraia-hardware   trait Device · Capability com risco R0-R5 · registry · gate
     |
Adapter / Hardware Skill   mqtt · home_assistant · serial · gpio
     |
Device   a lâmpada, o sensor, a fechadura, a placa
```

| Transporte | O que entra | Configuração |
|---|---|---|
| **MQTT** | qualquer dispositivo que publique no broker da casa — ESP32 caseiro, Tasmota, Shelly, zigbee2mqtt | `hardware.mqtt` |
| **Home Assistant** | as entidades do hub: luz, clima, sensor, fechadura, cortina — e com elas Zigbee, Matter, Z-Wave | `hardware.home_assistant` |
| **Serial/USB** | Arduino e ESP32 no cabo, protocolo JSON por linha | `hardware.serial` |
| **GPIO** | pinos do Raspberry Pi | `hardware.gpio` |

**Nada disso liga sozinho.** Sem a seção no config, o registro nasce vazio e
`device_list` não lista nada — não há descoberta de rede acontecendo por
padrão, e cada transporte é uma feature de compilação desligada.

### Automações declarativas

Com `hardware.automations` no config, regras em TOML/JSON viram
`gatilho → condição → ação`: mudança de estado ou cron, condições sobre
leituras, e a ação passando pela **mesma policy** do runtime, com teto de
risco declarado.

### Risco R0-R5: o que o agente pode fazer sozinho

| Nível | Exemplo | O que acontece |
|---|---|---|
| R0 | ler temperatura | automático |
| R1 | acender a luz | policy do turno |
| R2 | abrir a cortina | policy + limite de frequência |
| R3 | destrancar a porta | **pede confirmação humana** |
| R4 | ligar o forno | aprovação explícita |
| R5 | atuador industrial | negado, salvo allowlist do operador |

Três invariantes prendem a tabela: leitura é sempre R0 e nunca escreve; sem
canal real de confirmação, R3/R4/R5 **não rodam** (nunca viram execução
silenciosa); e quem não passou por autenticação nenhuma — uma placa no cabo
USB — não classifica o próprio risco: a tabela dela é fechada no código.

### Integrações empacotadas como skill

Uma integração agora cabe num `SKILL.md` com `kind: hardware-adapter` ou
`hardware-preset`: transporte, capabilities e presets "entidade → capability"
com sinônimos em português e inglês, para você dizer *"acende a luz da sala"*
em vez de `light.sala_teto` + `power`.

Seis skills oficiais acompanham o repo — Home Assistant, MQTT,
serial/Arduino, ESP32, Zigbee e Matter. **Zigbee e Matter entram como preset
sobre o hub**, não como stack própria: o Home Assistant já resolveu
coordenador e pareamento.

E as duas regras que um skill não escolhe: a lista de transportes é fechada
(qualquer outro carrega inerte, visível e nunca ativo) e **um skill só sobe
risco, nunca baixa** — um manifesto que tentasse rebaixar `door_unlock` de R3
para R1 para escapar da confirmação não muda nada.

📖 [`docs/hardware.md`](https://github.com/michelbr84/GarraRUST/blob/main/docs/hardware.md) · [`docs/hardware-skills.md`](https://github.com/michelbr84/GarraRUST/blob/main/docs/hardware-skills.md)

---

## No app e no terminal

- **O chat do Garra Mobile mostra a resposta enquanto ela acontece.** O app
  abre o WebSocket do gateway, retoma a sessão que já tinha e vai
  concatenando os pedaços numa bolha que cresce; um indicador diz qual
  ferramenta está rodando, e o botão Parar cancela só o turno endereçado. Se o
  socket não abrir — gateway antigo, proxy que recusa upgrade, LAN caída —, o
  app volta sozinho para o HTTP de sempre, sem mostrar erro.
- **`garra chat --persist` e `--resume <SESSION_ID>`.** A conversa deixa de
  morrer com o processo. É opt-in explícito: sem nenhuma das duas flags,
  **nenhum** banco é aberto e nada vai para o disco — exatamente o
  comportamento anterior.
- **`garra about`** parou de escrever códigos de cor quando a saída não é um
  terminal (`about > arquivo`, pipe, `NO_COLOR`, `TERM=dumb`).

---

## Painel admin: nada mais depende de SQL manual

- **2FA no login.** O TOTP que o projeto já implementava só era alcançável
  pelo fluxo mobile; agora o login do painel exige o segundo fator quando a
  conta tem 2FA ligado — com lockout que sobrevive a restart e segredo
  cifrado com a chave do cofre.
- **Troca de senha pela própria UI**, reverificando a senha atual.
- **Recuperação sem e-mail:** `garra admin recovery start --username X` gera
  um código de uso único que **não volta na resposta HTTP** — o gateway
  guarda só o hash e escreve o texto num arquivo `0600`, então ler o código
  exige shell na máquina. A rota responde sempre igual, existindo o usuário
  ou não, para não virar oráculo de enumeração.

---

## Segurança

- **O canal do `/proc` está fechado.** O processo passa a rodar
  `prctl(PR_SET_DUMPABLE, 0)`: até aqui um processo filho de mesmo UID lia
  `/proc/<pid>/environ` do pai e alcançava `GARRAIA_JWT_SECRET`,
  `ANTHROPIC_API_KEY` e companhia. Medido com controle: sem o fix o filho lê o
  segredo; com o fix recebe `EACCES`.
- **O modo `ask` passa a negar `bash`.** Negar `file_write` era decorativo
  enquanto o modelo podia escrever o arquivo pelo shell.
- **A allowlist do modo `orchestrator` passa a valer** — ela era declarada e
  ignorada.
- **`agent.bash_allowlist`** deixa o operador declarar comandos confiáveis,
  com sintaxe pobre de propósito (prefixo com coringa só no fim, ou comando
  exato) e que nunca perdoa comando perigoso nem comando composto.
- **Rotas mutantes de `/api/learning/`** passam a exigir origem própria e peer
  local quando não há API key configurada.

---

## Operação e higiene

- **O circuit breaker de restart do gateway passou a existir de verdade:**
  `StartLimitIntervalSec`/`StartLimitBurst` viviam em `[Service]`, onde o
  systemd os ignora em silêncio — um crash-loop reiniciava para sempre. Agora
  há teste em CI que falha se o unit tiver qualquer chave ignorada.
- **Voz fora do ar não passa mais batido:** `GET /api/diagnostics` ganhou as
  linhas `voice.tts` e `voice.stt`, com o comando exato de subida no
  `next_step` — e a documentação parou de citar comandos que não existem no
  PyPI.
- **CI mais fechado:** lint do YAML dos agentes, os pares clippy+test das
  features de hardware (`mqtt`, `home-assistant`, `automations`, `skills`) e o
  teste que carrega os skills oficiais do repo.
- **rmcp 2.2 → 3.3** no cliente e no servidor MCP.

---

## Atualizando

```bash
garra update          # instalações existentes
```

Ou reinstale: `curl -fsSL https://garraia.org/install.sh | sh`
(Windows: `irm https://garraia.org/install.ps1 | iex`).

Nenhuma migração é necessária, e nenhuma configuração antiga muda de
significado — hardware só liga com seção nova no config.
