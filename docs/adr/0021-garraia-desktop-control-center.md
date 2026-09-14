# 21. GarraIA Desktop — Control Center (`garraia desktop`)

- **Status:** Proposed (a aceitação é do dono do projeto)
- **Deciders:** @michelbr84 (decisão final) + Claude (levantamento, sessão autônoma 2026-09-13; revisado no mesmo dia após verificação independente — ver §Histórico de revisões)
- **Date:** 2026-09-13 (America/New_York)
- **Tags:** fase-4, desktop, ui, agentes, arquitetura, seguranca
- **Supersedes:** none
- **Superseded by:** none
- **Links:**
  - Épico: [#1181](https://github.com/michelbr84/GarraRUST/issues/1181)
  - Exposição cross-origin do gateway, que condiciona as abas mutantes: [#1182](https://github.com/michelbr84/GarraRUST/issues/1182)
  - Relacionados: [ADR 0007](0007-desktop-frontend.md) (o plano atinge o gatilho S1 — ver §Consequências) · [ADR 0009](0009-web-console-design-system.md) (design system)
  - Baseline atual: `crates/garraia-desktop/src-tauri/`
  - Superfície HTTP reaproveitada: `crates/garraia-gateway/src/router.rs:295-376`
  - Motor multi-agente existente: `plans/0360-garra-agents-setup.md`, `crates/garraia-cli/src/agents.rs` e o repositório `michelbr84/AgentDeck`

---

## Context and Problem Statement

O GarraIA hoje é excelente no terminal e mínimo no desktop. O `garraia-desktop`
(Tauri v2, 804 linhas de Rust em oito arquivos) entrega o pássaro (overlay
transparente sempre-no-topo), a Chat Bar (topo central, `Ctrl+Space`), uma
bandeja com nove itens de menu e uma janela Settings mínima (atalhos e checagem
de update).

Configurar provider, ver logs e diagnosticar **já têm GUI**: o Web Console que o
próprio gateway serve em `GET /`. O que falta é o desktop levar até ela — a
bandeja não tem item para abri-la. Gerenciar agentes e ligar/desligar
funcionalidade do desktop exigem terminal ou edição de `config.yml`.

O pedido é um **control center**: uma aplicação gráfica central (`garraia
desktop`) que seja companion, agent manager, AI launcher, settings app e chat
hub ao mesmo tempo, sem o usuário precisar do terminal, sem múltiplos
executáveis abertos, e **sem quebrar ou remover o pássaro e a Chat Bar**.

A pergunta arquitetural que este ADR decide **não é** "qual framework de UI"
(isso é o ADR 0007). É: **onde mora a lógica do control center, quantos
processos o usuário passa a ter e sobre qual superfície as abas se apoiam com
segurança?**

### Inventário do que já existe (levantado em código, 2026-09-13)

Quatro ativos mudam a resposta e precisam estar na mesa antes de qualquer opção:

**1. O desktop já supervisiona processo — de forma primitiva.** `gateway.rs`
roda o `garraia` como *sidecar* Tauri (`externalBin: ["binaries/garraia"]`,
invocado com `start`), com `launch`/`restart`/`kill` e morte do filho no
`RunEvent::Exit`. Não detecta queda nem reinicia sozinho.

**2. O gateway já expõe boa parte do painel como HTTP.** `router.rs:295-376`
serve `/api/health`, `/api/status`, `/api/stats`, `/api/capabilities`,
`/api/providers` (list/add/**test**/**default**), `/api/channels`, `/api/mcp` +
`/api/mcp/tools` + `/api/mcp/health`, `/api/modes`, `/api/slash-commands`,
`/api/logs`, `/api/sessions/{id}/messages` + `/history`, `/api/tts`, `/api/stt`,
`/api/settings/{schema,effective}` + `PATCH /api/settings` e `/api/diagnostics`.
Duas ressalvas mudam como as abas podem usá-la:

- **"Auth-free" só vale sem chave.** Com `gateway.api_key` configurada, todo
  `/api/*` exceto `/api/health`, `/api/capabilities` e `/api/auth-check` exige
  bearer (#1045). As abas precisam carregar essa credencial.
- **Na config default ela aceitou pedido de qualquer origem nas rotas medidas**
  (#1182) — ver §Superfície de segurança das abas.

**3. O Web Console já é GUI sobre essa superfície.** `webchat.html` (`GET /`,
Garra Glass) já chama `/api/providers*`, `/api/diagnostics`, `/api/settings*`,
`/api/logs`, `/api/mcp*`, `/api/channels` e `/api/modes`.

**4. O gerenciamento multi-agente já existe — fora deste repositório.**
`garra agents {setup,status,link,rollback,web}` (`agents.rs`) é casca fina sobre
o **AgentDeck** (`michelbr84/AgentDeck`, TypeScript/Node). Lido no fonte em
2026-09-13:

- **O que ele tem:**
  - o contrato `AgentAdapter` (`packages/adapter-sdk/src/index.ts:89`): `detect`,
    `getLatestVersion`, `checkHealth`, `backupConfig`, `install`, `upgrade`,
    `rollback` e `execute`;
  - adapters para os quatro agentes do pedido (GarraIA, Hermes, OpenClaw, Claude
    Code) e outros quatro;
  - Rooms com cinco modos (`mention`, `panel`, `debate`, `round_robin`,
    `coordinator`);
  - a API REST `/api/v1/*` no daemon `agentdeck web` (Fastify, `127.0.0.1:4321`)
    e um `agentdeck mcp-server`.
- **O que ele não tem:**
  - ciclo de vida de processo: o contrato não tem start/stop/restart, e
    `agentdeck stop` só manda apertar Ctrl+C;
  - suporte a Windows: o `scripts/install.sh:50` recusa tudo fora de
    Linux/macOS e exige Node 20+;
  - saída legível por máquina na CLI.

A restrição estruturante do plan 0360 continua valendo: *"Reimplementar isso em
Rust seria manter duas cópias da mesma lógica em duas linguagens."*

### O problema que não aparece no diagrama

`garraia-desktop` fica **fora de todos os checks obrigatórios**. O `ci.yml` a
exclui (`--exclude garraia-desktop`) de clippy, build, test, cobertura e MSRV,
porque a crate precisa das libs de GTK/webkit e de um sidecar preparado
(`binaries/garraia-<target-triple>`) que aqueles jobs não instalam.

Quem compila a crate é o `desktop.yml`. Em todo PR que toca
`crates/garraia-desktop/**` ou `crates/garraia-cli/**`, ele gera MSI/NSIS no
`windows-latest` e `.deb`/AppImage no `ubuntu-22.04`. Mas é filtrado por path —
não pode ser check obrigatório sem travar os PRs que não o disparam — e não roda
clippy nem teste. Resultado:

- a crate tem **build** verificado em PR;
- não tem **nenhum lint e nenhum teste**;
- uma quebra nela não bloqueia merge.

Em máquina de desenvolvimento sem `webkit2gtk-4.1` e `libsoup-3.0`, a crate nem
compila.

Isso é restrição de projeto, não detalhe de infraestrutura: lógica colocada
dentro da crate Tauri nasce sem teste e sem gate obrigatório.

---

## Decision Drivers

1. **★★★★★ Não quebrar o que funciona.** Pássaro, Chat Bar, bandeja, hotkeys,
   autostart e o sidecar do gateway continuam idênticos. O updater está ligado
   mas inerte (`pubkey` vazio, e nenhum workflow publica `latest.json` —
   `ROADMAP.md` §4.1): não é comportamento a preservar, é débito do M7.
2. **★★★★★ Um aplicativo para o usuário.** Nada de dois ícones na bandeja, duas
   janelas de processo, dois lugares para "sair". Processo auxiliar — como o
   sidecar do gateway hoje — é filho invisível do app: sobe e morre com ele.
3. **★★★★★ Lógica testável.** A crate Tauri não tem lint, teste nem check
   obrigatório; o que for testável precisa morar onde os checks obrigatórios
   alcançam.
4. **★★★★☆ Não duplicar motor existente.** O gateway já tem a API; o AgentDeck
   já tem os adapters. Reimplementar qualquer um dos dois é custo permanente.
5. **★★★★☆ A CLI não pode passar a exigir GUI.** `garraia` roda em Termux,
   RunPod, Docker e servidor headless. Uma dependência de Tauri/GTK no binário
   da CLI quebra `cargo build --workspace --exclude garraia-desktop` e toda
   instalação sem display.
6. **★★★★☆ Superfície de segurança.** Achar e executar binários de terceiros na
   máquina do usuário (`hermes`, `claude`, `openclaw`) é R4. E a superfície HTTP
   em que as abas se apoiam aceitou pedido cross-origin na config default, em
   todas as rotas mutantes medidas (#1182): construir abas mutantes sobre ela
   sem fechar isso amplia o problema.
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
- ❌ **Toda a lógica nova nasce sem lint, sem teste e fora dos checks
  obrigatórios.** A crate sai de ~800 linhas para alguns milhares; o
  `desktop.yml` só as compila e empacota.
- ❌ Duplica em comandos Tauri o que `/api/*` já serve por HTTP — duas
  implementações de "listar providers", "ler logs", "aplicar settings".
- ❌ O que for escrito ali não serve ao mobile nem ao Garra Cloud depois.

### B) Novo aplicativo, o desktop atual vira subprocesso

Um binário novo (control center) que lança, mostra e oculta o `garraia-desktop`
atual.

- ✅ Separação conceitual limpa entre "painel" e "companion".
- ❌ **Dois processos, dois trays, IPC entre eles.** Contraria o driver #2
  frontalmente.
- ❌ **Empacotamento em dobro.** O bundle Tauri já embute o `garraia` como
  sidecar. Um terceiro binário precisa de empacotamento e instalador próprios e,
  quando o débito do updater for pago, de manifesto e assinatura próprios — tudo
  sob as regras 15/16 do `CLAUDE.md`.
- ❌ Duplica supervisão: quem mata o gateway se o painel morrer mas o companion
  não?

### C) Um app, tudo em módulos ativáveis

Um aplicativo com pássaro, barra, chat, agentes, integrações e settings como
módulos ligáveis/desligáveis.

- ✅ Instinto correto sobre modularidade e sobre o requisito de ligar/desligar.
- ⚠️ **Não responde a pergunta.** "Módulo" descreve o ciclo de vida, não onde o
  código mora. Módulos dentro da crate Tauri herdam exatamente o problema da
  opção A.

### D) Shell único, núcleo testável fora do Tauri, superfícies web reaproveitadas *(escolhida)*

Uma aplicação (a atual, expandida — a forma da opção A), com módulos ligáveis (a
intenção da opção C), mas com a lógica repartida por **testabilidade**, não por
tela:

- **`garraia-desktop-core`** — crate **nova, sem Tauri**: tipos de módulo,
  estado ligado/desligado, supervisão de processos, resolução de caminhos e o
  cliente do AgentDeck (atrás de trait, testável sem ele instalado). Por não
  depender de Tauri, cai nos checks obrigatórios (`Clippy Linting`,
  `Test (ubuntu-latest)`, `Test (windows-latest)`) sem workflow novo. Ressalva:
  no leg Windows os testes são só **compilados** (`cargo test --no-run`,
  `ci.yml`); execução real acontece só no Linux, então a resolução de
  caminhos específica de Windows precisa de testes de tabela que rodem em
  Linux, não só de `cfg(windows)`.
- **`garraia-desktop`** — continua sendo a casca Tauri: janelas, bandeja,
  hotkeys, autostart, updater. Fica fina de propósito; chama o core.
- **Dados das abas** — vêm do gateway que o desktop **já roda como sidecar**. As
  abas com dados prontos consomem `/api/*`; o que falta vira rota nova no gateway
  (que está nos checks obrigatórios) e fica disponível de graça para o Web
  Console, o mobile e o Garra Cloud. A **UI** das abas é local, empacotada no
  bundle — ver §Superfície de segurança das abas.
- **Agentes** — o desktop **lê e comanda o AgentDeck**, não reimplementa
  adapters. Onde o AgentDeck já resolve (detect, install, health, backup,
  rollback, grupos), o painel é cliente. O que ele não tem — ciclo de vida de
  processo, Windows, saída legível por máquina — está em §Pontos em aberto, como
  pré-requisito do M3.

---

## Decision Outcome

**Escolha: Opção D.**

### Por que é superior às três alternativas

Contra a **B**, ganha no driver #2 e no empacotamento: um processo de app, um
tray, um bundle e nenhuma dúvida sobre quem supervisiona o quê.

Contra a **A**, ganha no driver #3, que é o que realmente separa as duas: a
opção A não é errada na forma — a forma é a mesma —, é errada no *destino do
código*. Colocar o control center inteiro numa crate sem lint, sem teste e fora
dos checks obrigatórios é aceitar que ninguém vai saber quando ele quebrar. A
opção D tem a mesma UX com a lógica onde os checks obrigatórios alcançam.

Contra a **C**, ganha por responder a pergunta que a C deixa aberta. A
modularidade da C está inteiramente preservada aqui — pássaro, Chat Bar e cada
aba são módulos com estado próprio e desligáveis — só que ancorada num lugar
testável.

E contra as três, ganha no driver #4: o que o gateway já serve não é reescrito
em comandos Tauri, e os adapters do AgentDeck não ganham segunda cópia.

Das cinco rotas que a primeira versão chamava de testadas, só `/api/diagnostics`
tem teste (15 unitários e Playwright). `/api/settings/schema`, `/api/logs`,
`/api/providers/test` e `/api/mcp/health` não têm, e cobri-las entra no
milestone da aba correspondente.

### Prontidão por aba

Os milestones nomeiam oito abas (o pedido original falava em nove; a nona não
está nomeada aqui):

| Aba | Fonte | Estado |
|---|---|---|
| Home | `/api/health`, `/api/status`, `/api/stats` | pronta |
| Desktop | toggles locais (pássaro, Chat Bar) | não depende do gateway |
| Models/Providers | `/api/providers` (GET, POST), `/api/providers/test`, `/api/providers/default` | leitura pronta; mutações dependem do #1182 |
| Integrations | `/api/mcp`, `/api/mcp/tools`, `/api/mcp/health`, `/api/channels` | pronta para leitura |
| Settings | `/api/settings/schema`, `/api/settings/effective`, `PATCH /api/settings` | só leitura: o `PATCH` é dry-run (`settings_handler.rs:6-8`) e anuncia persistência para um "plan 0121a" que não existe em `plans/` |
| Logs | `/api/logs` | parcial: cauda do `garraia.log` do gateway, só quando o log em arquivo foi configurado; não há log por agente |
| Activity/Tasks | — | sem endpoint em `/api/*` |
| Agents | AgentDeck, fora do gateway | depende de §Pontos em aberto |

Um passo barato que a primeira versão deste ADR não considerou: um item de
bandeja que abra o Web Console entrega GUI para providers, diagnóstico, settings
e logs antes de qualquer aba nova.

### Como `garraia desktop` funciona

A CLI **localiza e lança** o aplicativo instalado. Ela **não** embute GUI e
**não** ganha dependência de Tauri — driver #5, inegociável:

```
garraia desktop             # localiza o app e lança; se já está rodando, foca
garraia desktop --status    # diz se está instalado, onde, e se está rodando
garraia desktop --no-launch # só resolve o caminho e imprime (scriptável)
```

Resolução, em ordem: caminho de instalação por plataforma → `PATH` → diretório
do próprio executável (instalação lado-a-lado). Não encontrado: erro acionável
dizendo como instalar, com exit code sysexits, no padrão que o `config check` já
usa (plan 0035).

**Inversão de controle explícita:** hoje Desktop→CLI (sidecar). Passa a existir
também CLI→Desktop. `garraia desktop` lança o executável do app
(`garraia-desktop`), nunca a si mesmo.

No Linux, o `.deb` do desktop instala o sidecar como `/usr/bin/garraia`
(`Provides/Conflicts/Replaces: garraia`; o `desktop.yml` confere os caminhos
`usr/bin/garraia-desktop` e `usr/bin/garraia` no pacote). Então, para quem
instalou o desktop, a CLI **é** o sidecar, e a resolução precisa funcionar a
partir dele. Não há
recursão, porque o app só invoca o sidecar com `start`.

### Como agentes externos são detectados e integrados

Quatro regras, todas fail-closed:

1. **Achar é leitura.** O painel varre `PATH` e caminhos de config conhecidos
   sem executar nada.
2. **Confirmar executa — por isso é ação explícita.** Confirmar identidade ou
   versão significa rodar o binário achado. É o que o `DeckProbe` faz:
   `agents.rs:69-80` roda `agentdeck agents --help`, porque o nome na `PATH`
   pode ser o pacote npm homônimo. E é o que o `detect()` dos adapters do
   AgentDeck faz, com `--version`. Executar o que está na `PATH` é executar o
   que estiver lá; então a confirmação só acontece por clique do usuário, ou
   dentro do AgentDeck que o usuário instalou e aceitou — nunca numa varredura
   automática ao abrir o painel.
3. **Adicionar é decisão do usuário.** Detectado ≠ adicionado. Nada é escrito na
   config de outro agente sem confirmação, e o AgentDeck já faz backup antes de
   aplicar roteamento.
4. **Executar é ação explícita.** Start/stop/restart partem de clique do
   usuário, com o comando exato visível antes de rodar.

Onde o AgentDeck está instalado, ele é a fonte de verdade — é ele que tem os
adapters. Onde não está, o painel **não** reimplementa a detecção deles, que
seria a segunda cópia que o driver #4 veta. Mostra no máximo o que achou por
nome na `PATH`, marcado como não confirmado, e oferece instalar o AgentDeck onde
ele é suportado — sem nunca instalar sozinho. No Windows, que o instalador do
AgentDeck não suporta, a aba diz isso.

### Superfície de segurança das abas

Medido em 2026-09-13 (#1182), na config que o desktop embarca (loopback, sem
`allowed_origins`, sem chave):

- as rotas mutantes medidas (`PATCH /api/settings`, `POST /api/mode/select`,
  `POST /api/mcp/marketplace/install`, `POST /api/skills`) aceitaram pedido
  cross-origin, e o router mostra o mesmo padrão, sem guarda própria, nas demais
  mutantes de `/api/*`;
- `/ws` e `/ws/parrot` aceitaram upgrade de qualquer origem;
- `GET /api/sessions` respondeu com `allow-origin: *`, legível por outra origem;
- a mutação de learning medida foi recusada (403) pela guarda do #1093, que só
  cobre `/api/learning/*`.

Daí quatro restrições para a opção D:

1. **Nenhuma aba mutante antes do #1182.** As mutações do M4 (providers) e do M6
   (settings) dependem dele, e rota nova no gateway nasce com a guarda, não sem
   ela.
2. **Como as abas se autenticam é decisão do M2**, compatível com o conserto do
   #1182. A webview do Tauri tem origem própria, então para o gateway ela também
   é cross-origin. Os caminhos são dois:
   - allowlist exata da origem da webview, **medida em runtime** e sem curinga;
   - token emitido a cada lançamento do sidecar e mantido no lado Rust.

   Com `gateway.api_key` configurada, a chave não deve ficar ao alcance do JS
   da webview sem necessidade.
3. **A UI das abas é local.** A janela principal carrega a UI empacotada no
   bundle, como as janelas atuais (`frontendDist: "../ui"`), nunca uma página
   servida pelo gateway. Hoje o `tauri.conf.json` tem `withGlobalTauri: true` e
   `csp: null`, e a capability das janelas concede `shell:allow-execute`.
   Conteúdo remoto, ou dado do gateway renderizado sem escape (logs, histórico,
   saída de LLM), teria o IPC ao alcance. O M2 define CSP antes de a primeira
   aba renderizar dado do gateway.
4. **Controle de processo nunca vira rota HTTP nem escopo de shell da webview.**
   Start/stop/restart de agentes mora no Rust (core), chamado por comando Tauri
   tipado e estreito.

### Como o desktop atual é controlado

Pássaro e Chat Bar viram **módulos com estado persistido**, sem mudança de
comportamento: continuam respondendo a `Alt+G`, `Ctrl+Space` e à bandeja
exatamente como hoje (ver §Critério de "não quebrou nada"). O que muda é que
passam a ter um segundo lugar de controle — a aba Desktop. A persistência da
Chat Bar (`chat-bar.json`, com debounce de 500ms e validação de monitor) já é
exatamente o padrão que os outros módulos vão seguir; ela não é reescrita, é
generalizada no core.

**Compatibilidade:** nenhum arquivo de config existente muda de formato. Chaves
novas de módulo nascem com default = comportamento atual, então um
`config.yml`/`chat-bar.json` de hoje produz amanhã exatamente a mesma tela.

---

## Consequences

### Positive

- Um aplicativo, um tray, um bundle — a UX pedida (e um updater, quando o débito
  de assinatura for pago).
- A lógica nova do desktop passará a ter lint e teste nos checks obrigatórios
  (o M0 verifica que a crate core não recebe `--exclude` nos jobs). Hoje a crate
  só tem build, num workflow que não é obrigatório.
- As abas reaproveitam a API do gateway em vez de duplicá-la em comandos Tauri.
- O que for feito no gateway serve mobile e Garra Cloud sem porte.
- Pássaro e Chat Bar permanecem intocados no comportamento.
- Cada módulo desliga sozinho; quebrar um não derruba o resto.

### Negative

- **Uma crate a mais** no workspace (24ª). Justificada pela fronteira dos checks
  obrigatórios, mas é custo real de manutenção.
- **Dependência viva do AgentDeck** para a aba Agents em sua plenitude: outro
  repositório, outro ciclo de release, Node 20+, sem Windows e sem saída legível
  por máquina na CLI. O cliente precisa do daemon `agentdeck web` (mais um
  processo, porta 4321) ou do `agentdeck mcp-server` — em qualquer caso, filho
  supervisionado pelo app (§Pontos em aberto).
- **A casca Tauri continua sem lint e sem teste.** Ela tem build no
  `desktop.yml`. Acrescentar `cargo clippy` e `cargo test -p garraia-desktop` no
  job Linux dele, que já instala as libs e prepara o sidecar, parece barato e
  entra no M0 (não medido). Validação visual continua manual.
- **Dois lugares para ligar/desligar o pássaro** (bandeja e aba Desktop). Precisa
  de uma fonte de verdade só, ou divergem.
- **As abas mutantes esperam o #1182.**

### Neutral

- **O plano atinge o gatilho S1 do ADR 0007.** O critério 1 do S1 é "≥ 10 telas
  distintas **em produção** com state compartilhado não-trivial". Hoje há três
  janelas (pássaro, Chat Bar, Settings); os milestones acrescentam a janela
  principal e oito abas, e o limiar é cruzado quando as abas do M5 entrarem em
  produção — não por este ADR, que nada põe em produção.

  O ADR 0007 pede migração incremental, "sem big-bang". Por isso a decisão de
  framework vem **no início do M2**, antes de construir a navegação: fazer as
  oito abas em vanilla e decidir no fim seria o big-bang. Este ADR não toma essa
  decisão; ela vira o ADR próprio que o 0007 prevê.
- O Garra Glass (ADR 0009) segue valendo, e a proibição de CDN junto.

---

## Plano de implementação

| Milestone | Entrega | Risco |
|---|---|---|
| **M0** | Crate `garraia-desktop-core` sem Tauri: módulos, estado, supervisão, resolução de caminhos, cliente do AgentDeck atrás de trait. Nos checks obrigatórios (verificado que ela não recebe `--exclude`), com testes. Clippy e teste da casca no job Linux do `desktop.yml`, e o `paths:` desse workflow ganha `crates/garraia-desktop-core/**` **no mesmo PR** que cria a crate — o glob `crates/garraia-desktop/**` não cobre a irmã, e uma mudança só no core deixaria de compilar e empacotar a casca em PR | R2 |
| **M1** | `garraia desktop` na CLI — localiza, lança, `--status`, `--no-launch`. Zero dependência de Tauri no CLI | R3 |
| **M2** | Abre com duas decisões: framework (gatilho S1, ADR próprio) e autenticação das abas junto ao gateway (§Superfície de segurança). Depois: janela principal + navegação e CSP definida — gate antes de qualquer aba que renderize dado do gateway —, abas **Home** e **Desktop** | R3 |
| **M3** | Aba **Agents** — só depois de decididos os pré-requisitos de §Pontos em aberto | **R4 — `security-auditor` obrigatório** |
| **M4** | Abas **Models/Providers** e **Integrations** sobre `/api/providers*`, `/api/mcp*`, `/api/channels`. Mutações só depois do #1182 | R2 leitura · **R4** mutações (chave de provider, URL de teste) |
| **M5** | Abas **Activity/Tasks** e **Logs** — exigem endpoints novos no gateway | R2 |
| **M6** | Aba **Settings** sobre `/api/settings/{schema,effective}`; a escrita depende de o `PATCH` persistir e do #1182 | R3 · **R4** para settings `secret` |
| **M7** | Empacotamento, updater (hoje inerte: chave de assinatura + `latest.json`), docs | **R5 — escala ao humano** (regras 15/16) |

Ordem de dependência: **core → CLI → casca → abas**.

- **M3:** não começa sem o `security-auditor` no time.
- **M4 e M6:** não fazem mutação antes do #1182.
- **M7:** encosta em release e instaladores, então para e escala.

### Pontos em aberto

Decisões que este ADR deixa explicitamente para depois, cada uma com o momento
em que precisa ser tomada:

1. **Framework da UI** (S1 do ADR 0007) — início do M2, em ADR próprio.
2. **Autenticação das abas junto ao gateway** — início do M2, compatível com o
   conserto do #1182.
3. **Dono do ciclo de vida dos agentes** (start/stop/restart) — antes do M3.
   Pode ser o AgentDeck, com trabalho no outro repositório, ou o
   `garraia-desktop-core`, o que contraria o driver #4.
4. **Canal de integração com o AgentDeck** — antes do M3. Pode ser o daemon
   `agentdeck web` (REST) ou o `agentdeck mcp-server`. Em qualquer caso, o
   processo do AgentDeck é filho supervisionado pelo app, como o sidecar do
   gateway: sobe e morre com ele, sem ícone nem janela próprios. É o que mantém
   o driver #2 de pé.
5. **Aba Agents no Windows**, que o instalador do AgentDeck não suporta — antes
   do M3.

### Critério de "não quebrou nada"

Antes de cada merge, e obrigatoriamente antes do M7:

- **Pássaro:** nasce visível no canto inferior direito, posicionado pela
  resolução. "Open Garra" na bandeja e o clique esquerdo no ícone o mostram e
  ocultam.
- **`Alt+G`:** com o pássaro visível, abre e fecha a barra de input dele; com o
  pássaro oculto, mostra o pássaro (`hotkey.rs:24-30`).
- **Chat Bar:** nasce visível no topo central na primeira execução. Depois
  restaura posição e visibilidade de `chat-bar.json`, validando a posição contra
  os monitores conectados (`chat_bar.rs:52-59`). `Ctrl+Space` e a bandeja
  alternam; `Esc`/✕ oculta e persiste.
- **Bandeja:** mantém os nove itens, e cada um faz o que fazia.
- **Gateway:** sobe como sidecar e morre junto com o app.
- **Config:** `config.yml` e `chat-bar.json` existentes produzem a mesma tela de
  antes.
- **CLI sem GUI:** `cargo build --workspace --exclude garraia-desktop` continua
  verde.

---

## Links de referência

- Baseline Tauri: `crates/garraia-desktop/src-tauri/src/{lib,tray,overlay,chat_bar,gateway,hotkey,commands}.rs`, `tauri.conf.json`, `capabilities/default.json`
- CI do desktop: `.github/workflows/ci.yml` (`--exclude garraia-desktop` em clippy, build, test, cobertura e MSRV) e `.github/workflows/desktop.yml`; os comentários desatualizados sobre isso foram corrigidos no PR #1183 (`cbef1d6`, 2026-09-13)
- Superfície HTTP: `crates/garraia-gateway/src/router.rs:295-376`; exposição cross-origin: #1182, com a guarda do #1093 (`learning_auth.rs`) como precedente
- AgentDeck: `plans/0360-garra-agents-setup.md`; no repositório `michelbr84/AgentDeck`, `packages/adapter-sdk/src/index.ts:89`, `scripts/install.sh:50` e `packages/server/src/index.ts`
- Probe de binário externo: `crates/garraia-cli/src/agents.rs:52-81`
- Updater inerte: `ROADMAP.md` §4.1 e `docs/releasing.md` (débito conhecido)
- Exit codes acionáveis: `plans/0035-gar-379-cli-config-check.md`
- [ADR 0007](0007-desktop-frontend.md) · [ADR 0009](0009-web-console-design-system.md) · [ADR 0014](0014-anthropic-messages-shim.md) · [ADR 0019](0019-process-hardening-and-sandbox.md)

---

## Histórico de revisões

### 2026-09-13 — revisão após verificação independente

A primeira versão (commit `15ffca1`) foi conferida contra o código, o CI, a API
do GitHub, o fonte do AgentDeck e um gateway descartável. Esta revisão corrige:

- **Premissa de CI.** A primeira versão dizia que a crate tinha zero cobertura
  porque os runners não têm GTK. O `desktop.yml` compila e empacota a crate em
  PR; o que falta é lint, teste e check obrigatório. A opção D se mantém, com
  essa justificativa.
- **Regra de detecção.** Citava `agents.rs:70` como precedente de "nunca
  executar o binário", mas ele executa `agentdeck agents --help`, e o `detect()`
  do AgentDeck executa `--version`. Virou "achar é leitura, confirmar é ação
  explícita". Saiu o fallback em Rust sem AgentDeck, que duplicava a detecção.
- **AgentDeck.** Não tem start/stop/restart, não suporta Windows e não tem saída
  legível por máquina na CLI; isso virou §Pontos em aberto do M3.
- **"Seis das nove abas prontas".** Virou §Prontidão por aba. Das rotas ditas
  testadas, só `/api/diagnostics` tem teste, e o `PATCH /api/settings` é
  dry-run. O Web Console, que já é GUI sobre essa superfície, entrou no
  inventário.
- **Segurança.** Nova §Superfície de segurança das abas, a partir da exposição
  cross-origin medida (#1182).
- **Critério de regressão.** `Alt+G` não alterna o pássaro, o updater não
  funciona hoje e a Chat Bar restaura a visibilidade salva.
- **Gatilho S1 do ADR 0007.** Nove abas não somam dez telas, e nada está em
  produção. A decisão saiu do M7 e foi para o início do M2.
- **Menores.**
  - "~480 linhas" virou 804.
  - O campo "Amends: ADR 0007" saiu, porque este ADR não altera o 0007.
  - O risco de M4 e M6 sobe para R4 onde há secret.
