# 23. WhatsApp pessoal por dispositivo vinculado (`garra whatsapp`)

- **Status:** Accepted (2026-09-16, decisão do coordenador autônomo sob o
  requisito de release do dono; risco residual de banimento de conta registrado
  — ver §Riscos e mitigações)
- **Deciders:** coordenador autônomo (decisão registrada aqui) + Claude
  (levantamento em código nos dois repositórios, sessão autônoma 2026-09-16);
  @michelbr84 como dono do requisito de produto da v0.4.3 e das decisões R5
  listadas em §Follow-ups
- **Date:** 2026-09-16 (America/New_York)
- **Tags:** canais, whatsapp, cli, seguranca, subprocesso, fase-4
- **Supersedes:** none
- **Superseded by:** none
- **Links:**
  - Issue de acompanhamento: #1237 (guarda-chuva de `garra whatsapp`)
  - Canal atual (Cloud API): `crates/garraia-channels/src/whatsapp/`,
    `crates/garraia-gateway/src/bootstrap/whatsapp.rs`
  - [ADR 0019](0019-process-hardening-and-sandbox.md) — confinamento de
    processo, que este ADR estende para um filho que não é uma tool
  - [ADR 0016](0016-mobile-termux-local-first.md) — Termux como camada de
    execução, o ambiente onde a dependência de Node.js dói mais
  - [ADR 0021](0021-garraia-desktop-control-center.md) — `garraia-desktop-core`,
    de onde vêm o molde de máquina de estados pura e o de supervisão
  - `CLAUDE.md` regras 8 (ADR antes de decisão irreversível), 14 (SSRF /
    `garraia_common::ssrf`), 15 e 16 (assets de release e paridade dos
    instaladores)
  - Documentação de canais afetada: `docs/channels.md`,
    `docs/GARRAIA_VS_HERMES_VS_OPENCLAW.md`

---

## Context and Problem Statement

Hoje o GarraIA tem **um** WhatsApp, e ele é o WhatsApp errado para o usuário
doméstico.

O canal existente é a **Meta Cloud API**, 100% orientado a webhook:
`connect()` é um no-op deliberado (`crates/garraia-channels/src/whatsapp/mod.rs:131-136`),
o envio sai por uma chamada à Graph API (`api.rs:30-63`), e a entrada chega em
`POST /webhooks/whatsapp` (`crates/garraia-gateway/src/router.rs:287-293`), com
verificação HMAC-SHA256 obrigatória (`signature.rs`, `app_secret` exigido no
boot, #1070). O bootstrap já faz o trabalho pesado do canal: gates de allowlist
e pareamento (`bootstrap/whatsapp.rs:83-138`), id de sessão `whatsapp-<from>`
(`:140`), `InputValidator` + `check_prompt_injection` (`:142-147`), hidratação
de histórico (`:150`) e persistência do turno (`:201-209`). O canal está
declarado em `KNOWN_CHANNELS` como push (`router.rs:1359-1376`).

Isso funciona — para quem tem uma conta WhatsApp Business, um número
cadastrado na Meta, um app Meta aprovado e uma URL pública. **Não é o caso do
usuário que o GarraIA mira.** Quem instala o Garra em casa quer conversar com
o agente pelo **seu próprio WhatsApp**, e a única forma de fazer isso sem a
Cloud API é o mesmo mecanismo que o app web da Meta usa: **dispositivo
vinculado**, pareado por leitura de QR code em Configurações → Aparelhos
conectados.

Não existe hoje, em Rust, um cliente de dispositivo vinculado nesta árvore —
nem crate de QR code (nada em `Cargo.lock`; o único QR do repositório é
`qr_flutter`, no app mobile). Os dois sistemas comparáveis resolveram isso da
mesma maneira: **Hermes** e **OpenClaw** usam a biblioteca Node.js
[Baileys](https://github.com/WhiskeySockets/Baileys). No Hermes, `hermes
whatsapp` é um wizard linear sem flags (`hermes_cli/subcommands/whatsapp.py:8-14`)
que exige TTY (`hermes_cli/main.py:403-412`), pergunta o modo, roda `npm
install` sob demanda, executa `node bridge.js --pair-only` **herdando o
terminal** e desenha o QR pelo `qrcode-terminal`; o comando irmão `hermes
whatsapp-cloud` (`:16-24`) é que cobre a Cloud API. Ou seja: o próprio Hermes
trata os dois WhatsApps como **dois comandos distintos**, e não como modos de
um só.

O requisito de produto da **v0.4.3** é que `garraia whatsapp` ofereça as duas
opções ao usuário, com o QR code funcionando no terminal. A pergunta que este
ADR decide não é "qual biblioteca" isolada, e sim: **qual runtime passa a ser
requisito, onde mora o estado de sessão do WhatsApp, e sob qual regime de
processo esse runtime roda** — porque a resposta cria a primeira dependência
de runtime não-Rust do produto e o primeiro segredo que muda a cada mensagem.

### O que torna isto diferente de "adicionar mais um canal"

Três coisas, e todas têm consequência arquitetural:

**1. O estado de sessão não é uma credencial estática.** O `access_token` da
Cloud API é uma string que o usuário cola uma vez. O estado de um dispositivo
vinculado é um conjunto de chaves Signal que **muda a cada mensagem**
(pre-keys, sender keys, app-state sync). O `CredentialVault`
(`crates/garraia-security/src/credentials.rs`) reserializa o `HashMap` inteiro
a cada `save()` (`:150-175`) — é adequado para uma chave de API, não para um
blob que rotaciona em tempo real. Além disso, hoje o `save()` do cofre **não**
chama `harden_secret_file` (`:167-172`) — um gap pré-existente que este caminho
não pode herdar.

**2. O estado de sessão não pode aparecer em log, e a redação atual não o
pegaria.** `redact_secrets` (`crates/garraia-security/src/redaction.rs:69+`)
funciona por **prefixos conhecidos** (`sk-`, `xoxb-` e afins). Um blob JSON de
chaves Signal em base64 não casa com nenhum prefixo. A proteção precisa ser
estrutural — o tipo nunca implementa `Display`, o `Debug` é redigido, e um
teste varre o fonte — e não depender do filtro de saída.

**3. O runtime é um processo filho de terceiros que fala com a internet.** Não
é uma tool; é um daemon de longa duração falando um protocolo proprietário e
não documentado com servidores da Meta. Ele cai sob a mesma disciplina do ADR
0019 e do supervisor de MCP (`crates/garraia-agents/src/mcp/manager.rs:303-370`),
não sob a de um plugin.

### Inventário do que já existe e muda a resposta

- **Molde de supervisão de subprocesso.** `mcp/manager.rs:303-370` já resolve o
  difícil: wrapper `cmd /c` no Windows (`:309-331`), `LD_PRELOAD` do Termux
  (`:339-352`), `RLIMIT_AS` (`:1219-1235`), `PDEATHSIG` (`:1202-1210`) e
  timeouts (`:373-392`). `allowed_child_env()`
  (`crates/garraia-common/src/safety_gate.rs:334-339`) é a allowlist de env.
  `crates/garraia-desktop-core/src/supervise.rs` traz `Spawner`/`ProcessHandle`
  injetáveis, `RestartPolicy::backoff` que **devolve** o intervalo em vez de
  dormir, e `Drop` que mata o filho.
- **Molde de máquina de estados pura.** `garraia-desktop-core/src/state.rs`
  (`Desired` vs `Power`) e `garraia-cli/src/ui/spinner.rs:12-38` (estado sem
  relógio, renderizado como braço de `tokio::select!`).
- **Molde de prompt testável sem TTY.** trait `Prompter`
  (`garraia-cli/src/wizard/prompts.rs:18-28`) + `DialoguerPrompter` (`:31-68`),
  com guarda de ambiente não-interativo já testada (`tests/wizard_smoke.rs:25-64`).
- **Molde de comando que roda antes do config.** `garra desktop` e `garra
  verify` são interceptados antes do `ConfigLoader`
  (`garraia-cli/src/main.rs:1330-1355`), com exit codes sysexits
  (`desktop.rs:41`, `:44`) e launcher injetável (`:47-67`).
- **Molde de escrita de config e de arquivo 0600.**
  `wizard/config_writer.rs:213-222` (canal Telegram), merge só-se-ausente
  (`:289-295`), clamp 0600 (`:550-559`), `harden_secret_file`
  (`garraia-config/src/loader.rs:219-238`), `resolved_data_dir()`
  (`garraia-config/src/model.rs:528-531`). **Não existe** helper de diretório
  0700 na árvore — só de arquivo.
- **Precedente de bridge externo com protocolo versionado.** O canal OpenClaw
  (`garraia-channels/src/openclaw/`) e `garraia-channels/src/protocol.rs:7-90`
  (`CONNECTOR_PROTOCOL_VERSION = 1`, `MAX_CONNECTOR_FRAME_BYTES = 256 KiB`,
  `ConnectorFrame`).
- **Diagnóstico secret-free já padronizado.**
  `garraia-gateway/src/diagnostics_handler.rs:33-57`
  (`DiagnosticCheck { id, label, status, detail, next_step }`), com checagem de
  canais em `:560-587`.
- **Licenças.** `deny.toml:155-176` permite MIT e Apache-2.0 — `qrcode` e
  `fast_qr` passam.
- **Node.js não é dependência declarada** em lugar nenhum (`install.sh`,
  `doctor`, README). `npx` aparece apenas como comando default de servidores MCP
  **opcionais**.

---

## Decision Drivers

1. **O usuário doméstico precisa do WhatsApp dele, não de uma conta Business.**
   Esse é o requisito; qualquer opção que não o entregue está fora.
2. **Um comando, uma tela.** `garra whatsapp` tem que levar do nada ao pareado
   sem o usuário abrir um segundo terminal, instalar um daemon ou editar YAML.
3. **O segredo mais sensível do produto não pode ficar em claro no disco.** O
   Hermes grava `creds.json` em texto puro e pede `chmod 700` na documentação;
   o GarraIA tem cofre AES-256-GCM e não vai regredir para menos que isso.
4. **Processo filho de terceiro roda sob o regime de processo do projeto**, não
   sob um regime novo: env allowlist, `PDEATHSIG`, `RLIMIT_AS`, morte no `Drop`.
5. **CI sem Node e sem telefone.** A lógica precisa ser testável em runner
   limpo — o que empurra o estado para uma máquina pura em Rust e o protocolo
   para algo que um fixture em stdlib consegue falar.
6. **"12 canais num binário" é uma afirmação de marketing que o projeto faz.**
   Qualquer runtime novo a contradiz em parte e precisa ser declarado, não
   escondido.
7. **O caminho não-oficial pode custar a conta do usuário.** Isso não é risco
   do projeto apenas: é risco do usuário, e exige consentimento informado antes
   do primeiro QR.
8. **Nada do que já funciona pode piorar.** O canal Cloud API continua exatamente
   como está, com o mesmo `app_secret` obrigatório e o mesmo roteamento por
   assinatura.

---

## Considered Options

### A) Bridge Node.js + Baileys, falando NDJSON por stdio, com o Rust dono da sessão *(escolhida)*

Um bridge próprio (`bridge/whatsapp/`: `package.json` + `bridge.mjs`, na ordem
de 300 linhas) embutido no binário e materializado no diretório de dados. O
Rust dirige tudo por stdio; o bridge é **stateless**.

- **A favor:** é o único caminho com biblioteca madura e mantida para o
  protocolo de dispositivo vinculado; Baileys é exatamente o que Hermes e
  OpenClaw usam em produção; não exige Chromium nem Puppeteer; o protocolo
  stdio permite fixture de teste em stdlib.
- **Contra:** introduz Node.js como requisito deste caminho; a biblioteca
  acompanha um protocolo proprietário que a Meta muda sem aviso.

### B) Go + whatsmeow, distribuído como asset estático de release

`whatsmeow` é a implementação de referência em Go, compila para binário
estático e eliminaria o requisito de runtime na máquina do usuário.

- **A favor:** um arquivo, zero dependência na máquina do usuário, sem `npm
  install`, sem `node_modules`; tecnicamente a solução mais limpa para o
  usuário final.
- **Contra, e é o que decide agora:** introduz uma **terceira linguagem** no
  repositório, uma toolchain Go na matriz de CI de 3 OS, e — o ponto pesado —
  **um asset de release novo por plataforma**. A regra 15 do `CLAUDE.md` torna
  a superfície de assets de release um contrato congelado (`update.rs:42-48`
  resolve por nome exato e `:127` exige o `.sha256` irmão), e mexer nela é
  trabalho R5, do dono, não de uma slice de canal. Fica como follow-up, não
  como recusa definitiva.

### C) Implementar o protocolo de dispositivo vinculado nativamente em Rust

- **A favor:** binário único de verdade, sem runtime externo, sem asset extra.
- **Contra:** inviável no horizonte da v0.4.3. É o protocolo Signal sobre um
  transporte proprietário não documentado, com sincronização de app-state e
  rotação de chaves; as bibliotecas Rust que existem são parciais e sem uso em
  produção conhecido. O custo é de ordem de grandeza maior que o do canal
  inteiro, e o risco de quebra a cada mudança da Meta continua.

### D) Não fazer — manter só a Cloud API e documentar melhor

- **A favor:** custo zero, superfície de ataque zero, nenhum risco de conta
  banida, nenhum runtime novo.
- **Contra:** **falha o requisito de produto.** Cloud API exige conta Business,
  número dedicado e app Meta; é precisamente a barreira que faz o usuário
  doméstico desistir. Adiar não muda nada: o pedido é conversar pelo WhatsApp
  que a pessoa já tem.

### E) Reaproveitar o bridge do OpenClaw como daemon externo

O canal OpenClaw já existe na árvore, com cliente WS persistente e reconexão.
O OpenClaw também usa Baileys.

- **A favor:** nada de código novo de WhatsApp; o canal já está escrito.
- **Contra:** transfere ao usuário a tarefa de instalar, configurar e manter um
  **daemon de terceiro inteiro** para obter um canal. Isso destrói o driver #2
  (um comando, uma tela) e amarra um canal do produto ao ciclo de release de
  outro projeto. O bridge que queremos tem ~300 linhas; o OpenClaw é um gateway
  completo.

---

## Decision Outcome

Escolhida a **opção A**. As oito decisões abaixo formam a decisão; elas foram
tomadas no dia 2026-09-16 pelo coordenador autônomo sob o requisito de release
do dono.

### 1. Superfície de CLI: duas opções explícitas, e subcomandos para quem já sabe

`garra whatsapp` sem argumento abre um menu de duas opções — o equivalente
funcional do par `hermes whatsapp` / `hermes whatsapp-cloud`, mas numa porta de
entrada só:

```text
pt-BR:  1) Conectar meu WhatsApp pessoal (ler um QR code)
        2) Conectar um WhatsApp Business (API oficial da Meta)

en:     1) Link my personal WhatsApp (scan a QR code)
        2) Connect a WhatsApp Business account (official Meta Cloud API)
```

Subcomandos explícitos, para script e para quem já conhece o caminho:
`garra whatsapp link | cloud | status | logout`. O menu usa o trait `Prompter`
(`wizard/prompts.rs:18-28`), não `dialoguer` direto, para continuar testável
sem TTY; o default é injetado por `inject_default_subcommand`
(`cli_args.rs:42-90`), e qualquer flag nova com valor entra na lista
`value_flags` (`:98-109`, que tem teste de drift).

**Sem TTY o comando não falha:** imprime as duas opções, o subcomando
equivalente de cada uma, e sai **0**. É a mesma guarda já testada em
`tests/wizard_smoke.rs:25-64`, e é uma divergência deliberada do Hermes, que
sai 1 com "requires an interactive terminal" (`hermes_cli/main.py:403-412`) —
um comando informativo que sai 0 é melhor cidadão em pipeline.

### 2. Runtime: bridge Node/Baileys **stateless**, NDJSON por stdio

O bridge (`bridge/whatsapp/`: `package.json` + `bridge.mjs`) fala **NDJSON por
stdio** com o Rust:

- **eventos (bridge → Rust):** `started`, `qr`, `status`, `authenticated`,
  `connected`, `disconnected`, `logged_out`, `message`, `session_update`,
  `error`;
- **comandos (Rust → bridge):** `session_load`, `send`, `logout`, `ack`.

O bridge **não persiste nada**. O auth state vive em memória, como um
`AuthenticationState` sobre `initAuthCreds` + `BufferJSON`, e cada mutação sai
como um evento `session_update` (debounce ~500 ms) com o snapshot serializado;
**quem grava é o Rust**. Isto é a diferença central em relação ao Hermes, que
deixa o `useMultiFileAuthState` do Baileys escrever uma dúzia de arquivos em
claro num diretório — e que por isso tem a armadilha de dois diretórios de
sessão divergentes (CLI em `~/.hermes/whatsapp/session`, gateway em
`platforms/whatsapp/session`). Com um dono único do estado, essa classe de bug
não existe.

O QR também segue essa regra: o bridge emite a **string crua** do QR; quem
renderiza é o Rust, com `qrcode` ou `fast_qr` (ambos MIT/Apache-2.0,
`deny.toml:155-176`). O Hermes renderiza no Node com `qrcode-terminal` e por
isso tem que herdar o TTY do processo filho; com a string crua, o pipe continua
sendo um pipe e o terminal continua sendo do Rust — que já sabe detectar
Unicode versus ASCII (`ui/spinner.rs:323-344`) e filtrar ANSI
(`ui/ansi_filter.rs`).

O protocolo de eventos é herança direta do modo `--pair-json` do Hermes, que é
a parte do desenho dele que vale copiar; o versionamento segue o precedente de
`garraia-channels/src/protocol.rs:7-90`.

**Node.js é requisito só deste caminho**, como `npx` já é requisito só de
servidores MCP opcionais. `garra whatsapp` detecta `node`/`npm` e, se faltar,
explica em uma frase o que instalar — sem stack trace e sem falhar em outro
comando.

Os arquivos do bridge são **embutidos no binário** (`include_str!`) e
materializados em `<data_dir>/whatsapp/bridge/`, com `npm install --no-fund
--no-audit` na primeira vez. É o mesmo movimento do Hermes copiando o bridge
para `~/.hermes/scripts` quando a instalação é read-only, e resolve o caso do
binário instalado em diretório sem permissão de escrita.

### 3. Sessão: um blob cifrado, dono é o Rust

Arquivo único: `<resolved_data_dir()>/whatsapp/<account>/session.enc`,
**AES-256-GCM**.

- **Chave:** derivada da passphrase do cofre via `vault_passphrase_from_env()`
  (`garraia-security/src/credentials.rs:193-208`) quando ela existir; caso
  contrário, chave aleatória de 32 bytes em `session.key`, modo 0600, no mesmo
  diretório 0700 — **com aviso explícito no `garra whatsapp status`** de que a
  proteção nesse caso é de permissão de arquivo, não de passphrase.
- **Escrita atômica** (tmp + rename), arquivo 0600, diretório 0700 (helper novo
  — hoje só existe clamp de arquivo, `config_writer.rs:550-559` e
  `loader.rs:219-238`).
- **Nunca em log.** `Debug`/`Display` redigidos, e um teste varre o fonte atrás
  de log do tipo de sessão. A redação por prefixo (`redaction.rs:69+`) não
  pegaria esse conteúdo, então a garantia é estrutural.
- **Logout** = zeroize + remoção.
- **Re-link** arquiva a sessão anterior como `session.enc.prev` e só a descarta
  quando o novo pareamento tiver sucesso.

O blob **não** vai para o `CredentialVault`: o cofre reserializa o mapa inteiro
a cada `save()` (`credentials.rs:150-175`), o que é errado para um segredo que
muda a cada mensagem, e hoje ele nem endurece o próprio arquivo (`:167-172`).

### 4. Supervisão: o bridge roda sob o regime de processo do projeto

Spawn com `env_clear()` + `allowed_child_env()`
(`garraia-common/src/safety_gate.rs:334-339`, R3), mais as poucas variáveis que
o Node de fato precisa (`PATH`, `HOME`); `PDEATHSIG`
(`mcp/manager.rs:1202-1210`); `RLIMIT_AS` (`:1219-1235`); wrapper de Windows
(`:309-331`); backoff de `garraia-desktop-core/src/supervise.rs`
(`RestartPolicy::backoff` devolve o intervalo, quem tem o relógio espera);
`Drop` mata o filho.

Para CI sem Node e sem telefone: fixture `tests/fixtures/fake_whatsapp_bridge.py`
em stdlib, no mesmo molde do `fake_mcp_server.py` que `tests/mcp_lifecycle.rs`
já dirige.

### 5. Máquina de estados pura

No molde de `garraia-desktop-core/src/state.rs` — sem relógio, sem I/O,
testável por tabela:

```text
NotConnected → QrRequired → QrGenerated{expires_in} → WaitingScan
             → Authenticated → Connected

SessionFound → Validating → Connected
                          → ValidationFailed → QrRequired  (arquiva a antiga)

QrGenerated --tick--> expira → regenera (teto 5) → Failed
Connected → Reconnecting{attempt, backoff} → Connected | Failed
qualquer estado --logout/device removed--> LoggedOut  (apaga a sessão)
```

`Desired(Off)` nunca alcança `Connected` — a intenção do usuário é preservada
mesmo quando o processo cai, exatamente como `Desired`/`Power` do desktop-core.

### 6. Gateway: canal **pull** separado, `whatsapp_linked`

O canal novo **não** se mistura com o Cloud: `type = "whatsapp_linked"`,
`ChannelKind::Pull` em `KNOWN_CHANNELS` (`router.rs:1359-1376`). Reaproveita o
que já está provado no bootstrap do WhatsApp: os mesmos `channel_gates`, com
**allowlist e pareamento obrigatórios** (`bootstrap/whatsapp.rs:83-138`), o
mesmo `InputValidator` + `check_prompt_injection` (`:142-147`), a mesma
hidratação e persistência. Id de sessão: `whatsapp-linked-<jid>`.

Check `whatsapp.linked` em `/api/diagnostics`, no formato secret-free existente
(`diagnostics_handler.rs:33-57`), e `garra whatsapp status` lê **a mesma
fonte** — uma verdade só, duas superfícies.

**Tools read-only por default neste canal**, até o operador subir o teto
explicitamente. A justificativa é de superfície: no Cloud API a conversa chega
de um número que passou por cadastro na Meta; aqui ela chega do WhatsApp
pessoal de alguém, e o vetor de injeção por mensagem de estranho é
qualitativamente maior.

### 7. Consentimento antes do primeiro QR

Antes de desenhar o QR, uma tela curta (pt-BR/en) diz, sem eufemismo, que
clientes de dispositivo vinculado não-oficiais **podem levar ao bloqueio da
conta pela Meta**, recomenda usar um número secundário, e pede confirmação
explícita. Sem confirmação, não há QR.

### 8. `enabled = true` só depois da prova em disco

A regra mais transferível do Hermes, e a que eles têm teste de regressão
dedicado: a configuração só passa a declarar o canal habilitado **depois** que
o `session.enc` existe. Um wizard abortado no meio não pode deixar o gateway
pagando timeout e retry por um canal que nunca foi pareado.

### O que este ADR **não** decide

- A biblioteca de QR code (`qrcode` vs `fast_qr`) — ambas passam no `deny.toml`
  e a escolha é reversível.
- A versão exata de Baileys a fixar (só que **tem** que ser fixada — ver
  §Riscos).
- Se o bridge vira asset de release (opção B) — é decisão R5 do dono,
  §Follow-ups.
- Qualquer mudança no canal Cloud API, que fica como está.

---

## Consequences

### Positive

- **O requisito de produto da v0.4.3 é atendido**: o usuário conversa com o
  agente pelo WhatsApp que ele já tem, com um comando e um QR code.
- **A sessão fica cifrada em repouso** — estritamente melhor que o estado da
  arte nos dois sistemas comparáveis, que gravam as credenciais Signal em texto
  puro e pedem `chmod` na documentação.
- **Um dono único do estado de sessão** elimina por construção a classe de bug
  "dois diretórios de sessão divergentes" que o Hermes tem entre CLI e gateway.
- **O bridge sendo stateless e falando NDJSON torna o canal testável em CI sem
  Node e sem telefone**, por um fixture em stdlib.
- **O QR renderizado no Rust** mantém o terminal sob controle do processo que já
  sabe lidar com ele, sem herdar TTY para um filho.
- **O canal Cloud API não é tocado**: `app_secret`, roteamento por assinatura e
  o comportamento de múltiplos canais continuam idênticos.

### Negative

- **Node.js passa a ser requisito de um caminho do produto.** Isso contradiz em
  parte a afirmação "12 canais num binário" que o projeto faz em
  `docs/GARRAIA_VS_HERMES_VS_OPENCLAW.md` e no README. A afirmação não é
  apagada — ganha **nota de rodapé** apontando para este ADR. Quem não usa
  WhatsApp pessoal continua com um binário e nada mais.
- **Fragilidade de protocolo.** Baileys acompanha um protocolo proprietário que
  a Meta muda sem aviso; uma atualização do lado deles pode derrubar o canal.
  Mitigação: **versão do Baileys fixada** (nada de `^` no `package.json`) e a
  versão em uso **exposta no `/api/diagnostics`** e no `garra whatsapp status`,
  para que um relatório de bug diga de cara qual versão quebrou.
- **Risco de banimento da conta do usuário.** Cliente não-oficial é uso
  contrário aos termos da Meta. Mitigação: tela de consentimento (decisão 7),
  recomendação de número secundário, e rate limit de envio no canal. **O risco
  residual permanece e está registrado aqui** — é o ponto que justifica o
  status deste ADR ser uma decisão do coordenador autônomo com o risco
  explicitado, e não uma decisão silenciosa.
- **`npm install` na primeira execução** traz uma árvore de dependências
  transitiva que o `cargo-deny` não audita. Ela fica confinada a
  `<data_dir>/whatsapp/bridge/`.
- **Termux (ADR 0016) fica com um passo a mais.** Lá o Node se instala com `pkg
  install nodejs`, e a viabilidade do `npm install` de Baileys em aarch64 sob
  Termux não foi medida nesta sessão. É follow-up explícito, não promessa.

### Neutral

- O canal novo é **pull**, não push — o gateway passa a ter um canal que mantém
  conexão viva, o que o Cloud API não faz (`connect()` é no-op lá). Isso já é
  o caso do OpenClaw e do Slack Socket Mode.
- Um segundo `type` de WhatsApp na configuração significa que a documentação
  precisa dizer, na abertura da seção, **qual dos dois** o leitor quer — feito
  em `docs/channels.md`.
- O repositório ganha um diretório `bridge/whatsapp/` com JavaScript. É a
  primeira vez, e é um precedente; a alternativa B existe justamente para
  reverter isso depois.

---

## Riscos e mitigações

| # | Risco | Mitigação | Residual |
|---|---|---|---|
| 1 | Conta do usuário bloqueada pela Meta | Consentimento explícito antes do QR; recomendação de número secundário; rate limit de envio | **Sim — aceito e registrado.** Não há como eliminar sem abandonar o caminho |
| 2 | Mudança de protocolo da Meta derruba o canal | Versão do Baileys fixada; versão exposta em `status` e `/api/diagnostics`; canal isolado, não derruba o gateway | Sim — inerente a cliente não-oficial |
| 3 | Vazamento do `session.enc` (equivale a assumir o WhatsApp da pessoa) | AES-256-GCM; 0600 + diretório 0700; escrita atômica; `Debug`/`Display` redigidos; teste que varre o fonte | Baixo; menor sem passphrase de cofre, e o `status` avisa |
| 4 | Injeção de prompt por mensagem de terceiro | `InputValidator` + `check_prompt_injection` (já existentes); allowlist e pareamento **obrigatórios**; **tools read-only por default** | Baixo |
| 5 | Processo filho órfão consumindo memória | `PDEATHSIG`, `RLIMIT_AS`, `Drop` mata o filho, backoff com teto | Baixo |
| 6 | Dependências transitivas do `npm` não auditadas | Confinadas ao `data_dir`; `env_clear()` + allowlist de env; sem acesso ao cofre | Médio — é a contrapartida da opção A |
| 7 | `enabled = true` sem pareamento → gateway em retry infinito | Decisão 8: gravar só depois do `session.enc` existir; teste de regressão | Baixo |
| 8 | Usuário confunde os dois WhatsApps e configura o errado | Menu de duas opções; tabela "qual dos dois" no topo da seção de `docs/channels.md`; linhas separadas na comparação | Baixo |

---

## Follow-ups

1. **Bridge como asset de release (opção B ou empacotamento do Node).** Decisão
   **R5, do dono**: encosta em `CLAUDE.md` regra 15 (nomes de asset são
   contrato de `update.rs:42-48` e `:127`) e regra 16 (paridade
   `install.sh`/`install.ps1`). Enquanto não for tomada, Node é requisito local
   deste caminho.
2. **Migrar Cloud API e linked para `resolve_session`/`ChatSource`.** Os tipos
   existem (`garraia-db/src/chat_sync.rs:14-62,135`) e o canal Cloud monta a
   string de sessão à mão (`bootstrap/whatsapp.rs:140`). O canal novo deve
   nascer usando o caminho certo, e o Cloud deve ser migrado depois — em PR
   próprio, porque mexe em id de sessão de usuário existente.
3. **Gestão de chave quando não há passphrase de cofre.** O `session.key` em
   0600 é o degrau mínimo. Vale decidir depois se o caminho passa a exigir
   passphrase, ou a oferecer keyring do sistema operacional.
4. **Termux (ADR 0016):** medir `pkg install nodejs` + `npm install` de Baileys
   em aarch64 e documentar o resultado — inclusive se for "não recomendado".
5. **`CredentialVault::save()` não chama `harden_secret_file`**
   (`credentials.rs:167-172`). Gap **pré-existente**, fora do escopo deste ADR,
   mas encontrado no levantamento e que merece issue própria.
6. **Rate limit de envio** do canal `whatsapp_linked`, cujo número exato depende
   de medição e não é decidido aqui.

---

## Links de referência

**GarraIA (verificado em código, 2026-09-16)**

- Canal Cloud atual: `crates/garraia-channels/src/whatsapp/mod.rs:131-136`
  (`connect()` no-op), `api.rs:30-63` (envio Graph API), `webhook.rs`,
  `signature.rs`
- Bootstrap: `crates/garraia-gateway/src/bootstrap/whatsapp.rs:83-138` (gates),
  `:140` (session id), `:142-147` (validação + injection), `:150` (hidratação),
  `:201-209` (persistência)
- Rotas e catálogo: `crates/garraia-gateway/src/router.rs:287-293`,
  `:1359-1376` (`KNOWN_CHANNELS`), `:1391-1412` (`channel_status`)
- Config de canal: `crates/garraia-config/src/model.rs:493-501`;
  `resolved_data_dir()` `:528-531`
- Sessões: `crates/garraia-db/src/chat_sync.rs:14-62,135`
- CLI: `crates/garraia-cli/src/main.rs:82`, `:770-775`, `:1330-1355`,
  `:1357-1359`; `cli_args.rs:42-90`, `:98-109`;
  `wizard/prompts.rs:18-28`, `:31-68`; `desktop.rs:41`, `:44`, `:47-67`;
  `ui/spinner.rs:12-38`, `:323-344`; `tests/wizard_smoke.rs:17-64`
- Segredos: `crates/garraia-security/src/credentials.rs:150-175`, `:167-172`,
  `:193-208`; `redaction.rs:4`, `:48-67`, `:69+`;
  `crates/garraia-config/src/loader.rs:219-238`;
  `crates/garraia-cli/src/wizard/config_writer.rs:213-222`, `:289-295`,
  `:550-559`
- Subprocesso: `crates/garraia-agents/src/mcp/manager.rs:303-370`, `:309-331`,
  `:339-352`, `:373-392`, `:1202-1210`, `:1219-1235`;
  `crates/garraia-common/src/safety_gate.rs:334-339`;
  `process_hardening.rs:87`; `crates/garraia-desktop-core/src/supervise.rs`,
  `state.rs`
- Protocolo de conector existente: `crates/garraia-channels/src/protocol.rs:7-90`
- Diagnóstico: `crates/garraia-gateway/src/diagnostics_handler.rs:33-57`,
  `:560-587`
- Licenças: `deny.toml:155-176`
- Sidecar de release existente: `.github/workflows/release.yml:472-477`

**Hermes (`hermes whatsapp`, clone somente leitura, HEAD `784d5c3`)**

- `hermes_cli/subcommands/whatsapp.py:8-14` (wizard sem flags) e `:16-24`
  (`hermes whatsapp-cloud`, comando separado)
- `hermes_cli/main.py:403-412` (exige TTY, sai 1)
- `main_platform_setup.py:36-47` (o menu de duas opções), `:50-62`,
  `:125-202` (a sequência completa)
- `allowlist.js:66-78` (allowlist fail-closed no bridge)
- Persistência: `useMultiFileAuthState` do Baileys — `creds.json` e demais
  arquivos **em claro**, sem `chmod`, com armadilha de dois diretórios de
  sessão entre CLI e gateway
- Modo `--pair-json`: NDJSON com `started`/`qr`/`connected`/`disconnected`/
  `error` — o protocolo de eventos que este ADR adota
- QR: `qrcode-terminal ^0.12.0`, renderizado no Node, exige ≥60 colunas
- Regressão de ordenação: `tests/hermes_cli/test_whatsapp_setup_ordering.py`
  (`ENABLED=true` só após `creds.json`)

**ADRs e regras**

- [ADR 0016](0016-mobile-termux-local-first.md) ·
  [ADR 0019](0019-process-hardening-and-sandbox.md) ·
  [ADR 0021](0021-garraia-desktop-control-center.md)
- `CLAUDE.md` regras 8, 14, 15, 16
