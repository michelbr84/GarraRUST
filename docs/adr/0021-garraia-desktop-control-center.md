# 21. GarraIA Desktop — Control Center (`garraia desktop`)

- **Status:** 📋 proposed — a aceitação é do dono do projeto
- **Deciders:** @michelbr84 + Claude (sessão autônoma 2026-09-13)
- **Date:** 2026-09-13 (America/New_York)
- **Tags:** fase-4, desktop, ui, agentes, arquitetura
- **Supersedes:** none
- **Amends:** [ADR 0007](0007-desktop-frontend.md) (dispara o gatilho S1 documentado lá — ver §Consequências)
- **Links:**
  - Épico: [#1181](https://github.com/michelbr84/GarraRUST/issues/1181)
  - Baseline atual: `crates/garraia-desktop/src-tauri/`
  - Superfície HTTP reaproveitada: `crates/garraia-gateway/src/router.rs:295-376`
  - Motor multi-agente existente: `plans/0360-garra-agents-setup.md` + `crates/garraia-cli/src/agents.rs`
  - Design system: [ADR 0009](0009-web-console-design-system.md)

---

## Context and Problem Statement

O GarraIA hoje é excelente no terminal e mínimo no desktop. O `garraia-desktop`
(Tauri v2, ~480 linhas de Rust) entrega três coisas: o pássaro (overlay
transparente sempre-no-topo), a Chat Bar (topo central, `Ctrl+Space`) e uma
bandeja com nove itens de menu. Tudo o mais — configurar provider, ver logs,
diagnosticar, gerenciar agentes, ligar/desligar funcionalidade — exige terminal
ou edição de `config.yml`.

O pedido é um **control center**: uma aplicação gráfica central (`garraia
desktop`) que seja companion, agent manager, AI launcher, settings app e chat
hub ao mesmo tempo, sem o usuário precisar do terminal, sem múltiplos
executáveis abertos, e **sem quebrar ou remover o pássaro e a Chat Bar**.

A pergunta arquitetural que este ADR decide **não é** "qual framework de UI"
(isso é o ADR 0007). É: **onde mora a lógica do control center e quantos
processos o usuário passa a ter?**

### Inventário do que já existe (levantado em código, 2026-09-13)

Três ativos mudam a resposta e precisam estar na mesa antes de qualquer opção:

**1. O desktop já supervisiona processo.** `gateway.rs` roda o `garraia` como
*sidecar* Tauri (`externalBin: ["binaries/garraia"]`), com `launch`/`restart`/
`kill` e morte do filho no `RunEvent::Exit`. Supervisão de serviço não é
território novo — é território primitivo.

**2. O gateway já expõe quase todo o painel como HTTP.** `router.rs:295-376`
serve, auth-free e secret-free: `/api/health`, `/api/status`, `/api/stats`,
`/api/capabilities`, `/api/providers` (list/add/**test**/**default**),
`/api/channels`, `/api/mcp` + `/mcp/tools` + `/mcp/health`, `/api/modes`,
`/api/slash-commands`, `/api/logs`, `/api/sessions/{id}/messages` +
`/history`, `/api/tts`, `/api/stt`, `/api/settings/{schema,effective}` +
`PATCH /api/settings`, `/api/diagnostics`. Em Garra Glass, já desenhado.
Das nove abas pedidas, **seis** têm dados prontos atrás dessa superfície.

**3. O gerenciamento multi-agente já existe — fora deste repositório.**
`garra agents {setup,status,link,rollback,web}` (`agents.rs`) é casca fina sobre
o **AgentDeck** (`michelbr84/AgentDeck`, Node/TS), que o plan 0360 descreve com
adapters completos de detect/install/upgrade/health/backup/rollback para os
quatro agentes exatos do pedido — GarraIA, Hermes, OpenClaw, Claude Code — mais
Rooms com cinco modos de roteamento, SQLite e um painel de controle com
construtor de grupos. A restrição estruturante registrada lá vale aqui, palavra
por palavra: *"Reimplementar isso em Rust seria manter duas cópias da mesma
lógica em duas linguagens."*

### O problema que ninguém vê no diagrama

`garraia-desktop` está **excluído de todos os gates de CI**: clippy
(`ci.yml:307`), build (`:550`), test (`:566`) e o restante (`:729`) rodam com
`--exclude garraia-desktop`, porque o `build.rs` do Tauri exige GTK/webkit que
os runners não têm. A crate tem hoje **zero cobertura automatizada**.

Isso não é detalhe de infraestrutura: é uma restrição de projeto. Qualquer
lógica que a gente colocar dentro da crate Tauri nasce invisível para o CI. Um
control center inteiro ali dentro seriam milhares de linhas sem gate nenhum.

---

## Decision Drivers

1. **★★★★★ Não quebrar o que funciona.** Pássaro, Chat Bar, bandeja, hotkeys,
   autostart, updater e o sidecar do gateway continuam idênticos. Requisito
   explícito do pedido, e qualquer opção que os reescreva já perdeu.
2. **★★★★★ Um aplicativo para o usuário.** Nada de dois ícones na bandeja, duas
   janelas de processo, dois lugares para "sair".
3. **★★★★★ Lógica testável.** Com `garraia-desktop` fora do CI, o que for
   testável precisa morar onde o CI enxerga.
4. **★★★★☆ Não duplicar motor existente.** O gateway já tem a API; o AgentDeck
   já tem os adapters. Reimplementar qualquer um dos dois é custo permanente.
5. **★★★★☆ A CLI não pode passar a exigir GUI.** `garraia` roda em Termux,
   RunPod, Docker e servidor headless. Uma dependência de Tauri/GTK no binário
   da CLI quebra `cargo build --workspace --exclude garraia-desktop` e toda
   instalação sem display.
6. **★★★★☆ Superfície de segurança.** Detectar e executar binários de terceiros
   na máquina do usuário (`hermes`, `claude`, `openclaw`) é R4 — não é feature
   de UI.
7. **★★★☆☆ Extensibilidade.** Dezenas de agentes, plugins, equipes, Garra Cloud,
   múltiplas máquinas. A camada que responde isso deve ser a mesma para desktop,
   mobile e nuvem.
8. **★★★☆☆ Reversibilidade.** Cada módulo deve poder ser desligado sem derrubar
   o resto.

---

## Considered Options

### A) Expandir o Desktop atual

Crescer `garraia-desktop` com uma janela principal e implementar cada aba como
`#[tauri::command]`.

- ✅ Um processo, um binário, um tray. UX exatamente como pedida.
- ✅ Reaproveita 100% do pássaro/Chat Bar sem tocar neles.
- ❌ **Toda a lógica nova nasce fora do CI.** A crate sai de ~480 linhas para
  alguns milhares, nenhuma delas compilada ou testada no pipeline.
- ❌ Duplica em comandos Tauri o que `/api/*` já serve por HTTP — duas
  implementações de "listar providers", "ler logs", "aplicar settings".
- ❌ O que for escrito ali não serve ao mobile nem ao Garra Cloud depois.

### B) Novo aplicativo, o desktop atual vira subprocesso

Um binário novo (control center) que lança, mostra e oculta o `garraia-desktop`
atual.

- ✅ Separação conceitual limpa entre "painel" e "companion".
- ❌ **Dois processos, dois trays, IPC entre eles.** Contraria o driver #2
  frontalmente.
- ❌ **Empacotamento circular.** O bundle Tauri já embute o `garraia` como
  sidecar; um terceiro binário que também precisa ser empacotado e atualizado
  multiplica o problema de `latest.json`, assinatura e das regras 15/16 do
  `CLAUDE.md`.
- ❌ Duplica supervisão: quem mata o gateway se o painel morrer mas o companion
  não?

### C) Um app, tudo em módulos ativáveis

Um aplicativo com pássaro, barra, chat, agentes, integrações e settings como
módulos ligáveis/desligáveis.

- ✅ Instinto correto sobre modularidade e sobre o requisito de ligar/desligar.
- ⚠️ **Não responde a pergunta.** "Módulo" descreve o ciclo de vida, não onde o
  código mora. Módulos dentro da crate Tauri herdam exatamente o problema de CI
  da opção A.

### D) Shell único, núcleo testável fora do Tauri, superfícies web reaproveitadas *(escolhida)*

Uma aplicação (a atual, expandida — a forma da opção A), com módulos ligáveis (a
intenção da opção C), mas com a lógica repartida por **testabilidade**, não por
tela:

- **`garraia-desktop-core`** — crate **nova, sem Tauri**: tipos de módulo,
  estado ligado/desligado, detecção de agentes instalados, supervisão de
  processos, resolução de caminhos. Entra no workspace e **no CI**, com testes.
- **`garraia-desktop`** — continua sendo a casca Tauri: janelas, bandeja,
  hotkeys, autostart, updater. Fica fina de propósito; chama o core.
- **Conteúdo das abas** — servido pelo gateway que o desktop **já roda como
  sidecar**, em Garra Glass. As seis abas com dados prontos consomem `/api/*`;
  as que faltam viram rotas novas no gateway (que **está** no CI) e ficam
  disponíveis de graça para o Web Console, o mobile e o Garra Cloud.
- **Agentes** — o desktop **lê e comanda o AgentDeck**, não reimplementa
  adapters. Onde o AgentDeck já resolve (detect, install, health, backup,
  rollback, grupos), o painel é cliente.

---

## Decision Outcome

**Escolha: Opção D.**

### Por que é superior às três alternativas

Contra a **B**, ganha nos drivers #2 e #5: um processo, um tray, um bundle,
nenhum problema de empacotamento circular e nenhuma dúvida sobre quem
supervisiona o quê.

Contra a **A**, ganha no driver #3, que é o que realmente separa as duas: a
opção A não é errada na forma — a forma é a mesma —, é errada no *destino do
código*. Colocar o control center inteiro numa crate que o CI não compila é
aceitar que ninguém vai saber quando ele quebrar. A opção D tem a mesma UX com a
lógica onde o pipeline alcança.

Contra a **C**, ganha por responder a pergunta que a C deixa aberta. A
modularidade da C está inteiramente preservada aqui — pássaro, Chat Bar e cada
aba são módulos com estado próprio e desligáveis — só que ancorada num lugar
testável.

E contra as três, ganha no driver #4: as seis abas que já têm dados não são
reescritas. `/api/providers/test`, `/api/diagnostics`, `/api/settings/schema`,
`/api/logs` e `/api/mcp/health` existem, estão testados e já falam Garra Glass.
O AgentDeck já detecta e governa os quatro agentes. O trabalho que sobra é o
trabalho que só o desktop pode fazer: a casca, os toggles e a costura.

### Como `garraia desktop` funciona

A CLI **localiza e lança** o aplicativo instalado. Ela **não** embute GUI e
**não** ganha dependência de Tauri — driver #5, inegociável:

```
garraia desktop            # localiza o app e lança; se já está rodando, foca
garraia desktop --status   # diz se está instalado, onde, e se está rodando
garraia desktop --no-launch# só resolve o caminho e imprime (scriptável)
```

Resolução, em ordem: caminho de instalação por plataforma → `PATH` →
diretório do próprio executável (instalação lado-a-lado). Não encontrado: erro
acionável dizendo como instalar, com exit code sysexits, no padrão que o
`config check` já usa (plan 0035). É o mesmo contrato do `DeckProbe` em
`agents.rs`, que já se recusa a dirigir um binário homônimo — precedente bom,
reaproveitado.

**Inversão de controle explícita:** hoje Desktop→CLI (sidecar). Passa a existir
também CLI→Desktop. O binário lançado é sempre o **instalado**, nunca o sidecar
empacotado dentro do bundle — senão a CLI de dentro do bundle lançaria o
aplicativo que a contém.

### Como agentes externos são detectados e integrados

Três regras, todas fail-closed:

1. **Detecção é leitura.** Varre `PATH` e caminhos de config conhecidos. Nunca
   executa o binário encontrado para "ver se é ele". Um nome na `PATH` não é
   prova de identidade — a lição já registrada em `agents.rs:70`.
2. **Adicionar é decisão do usuário.** Detectado ≠ adicionado. O painel lista o
   que achou; o usuário confirma. Nada é escrito na config de outro agente sem
   confirmação, e o AgentDeck já faz backup antes de aplicar roteamento.
3. **Executar é ação explícita.** Start/stop/restart partem de clique do
   usuário, com o comando exato visível antes de rodar.

Onde o AgentDeck está instalado, ele é a fonte de verdade (é ele que tem os
adapters). Onde não está, o painel mostra o que detectou e oferece instalar —
sem nunca instalar sozinho.

### Como o desktop atual é controlado

Pássaro e Chat Bar viram **módulos com estado persistido**, sem mudança de
comportamento: continuam nascendo ligados, continuam respondendo a `Alt+G`,
`Ctrl+Space` e à bandeja. O que muda é que passam a ter um segundo lugar de
controle — a aba Desktop. A persistência da Chat Bar (`chat-bar.json`, com
debounce de 500ms e validação de monitor) já é exatamente o padrão que os outros
módulos vão seguir; ela não é reescrita, é generalizada no core.

**Compatibilidade:** nenhum arquivo de config existente muda de formato. Chaves
novas de módulo nascem com default = comportamento atual, então um
`config.yml`/`chat-bar.json` de hoje produz amanhã exatamente a mesma tela.

---

## Consequences

### Positive

- Um aplicativo, um tray, um bundle, um updater — a UX pedida.
- A lógica nova entra no CI pela primeira vez na história do desktop.
- Seis das nove abas reaproveitam API testada em vez de nascer duplicadas.
- O que for feito no gateway serve mobile e Garra Cloud sem porte.
- Pássaro e Chat Bar permanecem intocados no comportamento.
- Cada módulo desliga sozinho; quebrar um não derruba o resto.

### Negative

- **Uma crate a mais** no workspace (24ª). Justificada pela fronteira de CI, mas
  é custo real de manutenção.
- **Dependência viva do AgentDeck** para a aba Agents em sua plenitude. O painel
  precisa degradar com elegância quando ele não está instalado — e o AgentDeck
  vive em outro repositório, com outro ciclo de release.
- **A casca Tauri continua sem CI.** A opção D reduz a superfície não-testada ao
  mínimo; não a elimina. Validação visual continua manual ou em runner com GTK.
- **Dois lugares para ligar/desligar o pássaro** (bandeja e aba Desktop). Precisa
  de uma fonte de verdade só, ou divergem.

### Neutral

- **Dispara o gatilho S1 do ADR 0007.** Nove abas passam do limite de "≥ 10 telas
  distintas com estado compartilhado" que aquele ADR fixou como momento de
  reavaliar HTML+vanilla contra SolidJS. Este ADR **não** decide isso: a decisão
  de framework é outra decisão, e merece o ADR próprio que o 0007 já prevê. O
  milestone M2 é onde a evidência para ela aparece.
- O Garra Glass (ADR 0009) segue valendo, e a proibição de CDN junto.

---

## Plano de implementação

| Milestone | Entrega | Risco |
|---|---|---|
| **M0** | Crate `garraia-desktop-core` sem Tauri: módulos, estado, detecção, supervisão. No CI, com testes | R2 |
| **M1** | `garraia desktop` na CLI — localiza, lança, `--status`, `--no-launch`. Zero dependência de Tauri no CLI | R3 |
| **M2** | Janela principal + navegação; abas **Home** e **Desktop** (toggles de pássaro e Chat Bar). Evidência para o gatilho do ADR 0007 | R3 |
| **M3** | Aba **Agents**: detecção, status, start/stop/restart, abrir chat, cliente do AgentDeck | **R4 — `security-auditor` obrigatório** |
| **M4** | Abas **Models/Providers** e **Integrations** sobre `/api/providers*`, `/api/mcp*`, `/api/channels` | R2 |
| **M5** | Abas **Activity/Tasks** e **Logs** consolidados, por agente | R2 |
| **M6** | Aba **Settings** sobre `/api/settings/{schema,effective}` + `PATCH` | R3 |
| **M7** | Empacotamento, atualização, docs, decisão do gatilho do ADR 0007 | **R5 — escala ao humano** (regra 15/16) |

Ordem de dependência: **core → CLI → casca → abas**. M3 não começa sem o
`security-auditor` no time. M7 encosta em release e instaladores: para e escala.

### Critério de "não quebrou nada"

Antes de cada merge, e obrigatoriamente antes do M7:

- pássaro nasce visível, `Alt+G` alterna, posição adapta a resolução;
- Chat Bar nasce no topo central, `Ctrl+Space` alterna, `Esc`/✕ oculta, posição
  persiste entre execuções e é validada contra monitor conectado;
- bandeja mantém os nove itens e cada um faz o que fazia;
- gateway sobe como sidecar e morre junto com o app;
- `config.yml` e `chat-bar.json` existentes produzem a mesma tela de antes;
- `cargo build --workspace --exclude garraia-desktop` continua verde (a CLI não
  ganhou GUI).

---

## Links de referência

- Baseline Tauri: `crates/garraia-desktop/src-tauri/src/{lib,tray,overlay,chat_bar,gateway,hotkey,commands}.rs`
- Exclusão de CI: `.github/workflows/ci.yml:303-307, 549-568, 729`
- Superfície HTTP: `crates/garraia-gateway/src/router.rs:295-376`
- AgentDeck e a restrição estruturante: `plans/0360-garra-agents-setup.md`
- Padrão de probe de binário externo: `crates/garraia-cli/src/agents.rs:53-127`
- Exit codes acionáveis: `plans/0035-*` (`garraia config check`)
- [ADR 0007](0007-desktop-frontend.md) · [ADR 0009](0009-web-console-design-system.md) · [ADR 0014](0014-anthropic-messages-shim.md) · [ADR 0019](0019-process-hardening-and-sandbox.md)
