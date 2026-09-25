# Plan 0363 — GarraIA Desktop: instalar, configurar e conectar ao WhatsApp sem terminal

- **Data:** 2026-09-25 (America/New_York)
- **Status:** proposta — diagnóstico + arquitetura + plano; **nenhum código
  alterado**. As decisões R5 estão reunidas em §9 e são do dono.
- **Origem:** pedido direto (brief "arquiteto multiplataforma + UX para
  usuários não técnicos": *"Instalei o aplicativo, cliquei no ícone, autorizei
  o que era necessário, escaneei o QR Code e comecei a conversar com o Garra
  pelo WhatsApp"*).
- **Antecedentes:** [ADR 0021](../docs/adr/0021-garraia-desktop-control-center.md)
  (Control Center, opção D — casca Tauri fina + `garraia-desktop-core` sem
  Tauri + superfícies do gateway), [ADR 0023](../docs/adr/0023-whatsapp-dispositivo-vinculado.md)
  (WhatsApp pessoal por QR: ponte Node/Baileys stateless, sessão cifrada, dono
  é o Rust), [ADR 0007](../docs/adr/0007-desktop-frontend.md) (HTML+vanilla
  até o gatilho S1), [ADR 0015](../docs/adr/0015-linux-packaging-toolchain.md),
  [ADR 0022](../docs/adr/0022-default-llm-identity.md) (OpenRouter
  `z-ai/glm-5.3-flash` como padrão), [ADR 0024](../docs/adr/0024-perfis-de-execucao-isolated-pod.md);
  plans [0359](0359-windows-installer-and-release-matrix.md),
  [0361](0361-linux-packages-and-windows-arm64.md),
  [0362](0362-garra-chat-bar-e-desktop-linux.md); issues #1426 (dogfood
  instalação limpa → WhatsApp), #1419/#1420 (doctor WhatsApp na CLI e no
  console), #1429 (acesso logo após o QR), #1431 (cofre para a sessão),
  #1449 (workspace compartilhado, R4).
- **Método:** leitura do código em `main` (`95c17409`, workspace `0.4.5`),
  do binário instalado nesta máquina (`garraia 0.4.4`), dos ADRs/plans, dos
  workflows e das 70 issues abertas. Onde a documentação e o código
  divergem, vale o código, e a divergência está anotada.

---

## 0. Resumo executivo

**Veredito: viável, sem reescrever nada, e mais perto do que parece.** O
backend que a experiência pedida precisa já existe e está testado: gateway com
`/api/health`, `/api/diagnostics` e `/api/channels`; canal `whatsapp_linked`
com ponte Node/Baileys supervisionada, sessão AES-256-GCM, máquina de estados
pura (QR expira em 20 s, 5 tentativas, backoff 1–30 s) e classificação de
saúde única para CLI e gateway; `garraia update` com checksum obrigatório e
cache de 24 h; um app Tauri v2 que já roda a CLI como *sidecar* e é
empacotado em MSI/NSIS e `.deb`/AppImage; e uma crate `garraia-desktop-core`
sem Tauri, dentro dos gates de CI, criada exatamente para receber a lógica
que este plano descreve.

O que **falta** cabe em cinco peças, nenhuma delas uma reescrita:

1. **Um driver de pareamento sem terminal.** Hoje o QR só nasce em
   `garraia whatsapp link`, que exige TTY. O `runner::pair` já é agnóstico de
   terminal (trait `PairUi`); falta quem o dirija a partir do gateway e
   entregue os eventos por HTTP.
2. **Node.js dentro do aplicativo.** A ponte precisa de Node 20+ e o Garra o
   procura **só na `PATH`** — que num app gráfico (Finder, Explorer, GNOME) é
   mínima. Sem isso, o passo "escaneie o QR" morre em "node não encontrado"
   mesmo em máquina que tem Node.
3. **Supervisão de gateway que verifica saúde e adota um Garra existente.**
   O `gateway.rs` da casca dá `spawn` cego em `garraia start`, sem checar
   porta, saúde ou versão.
4. **Um caminho não interativo para gravar provedor e chave.** O wizard
   `garraia init` é TTY; `POST /api/providers` só registra em memória;
   `PATCH /api/settings` é dry-run.
5. **A janela principal** (estados, ações, diagnóstico) — a casca hoje só tem
   papagaio, Chat Bar, bandeja e uma Settings mínima.

Mais dois bloqueios que **não são de código** e ficam com o dono: assinatura
(certificado Windows; conta Apple Developer para notarizar e para o Node
embutido passar no Gatekeeper) e a chave do auto-updater do Tauri, hoje
inerte.

A recomendação é **evoluir o que existe** (opção D do ADR 0021, já aceita),
com um princípio novo e explícito: **o gateway passa a ser o único dono da
ponte do WhatsApp — inclusive do pareamento.** É isso que elimina o "rode
`garraia restart`" do fluxo, evita duas pontes disputando a mesma sessão e
entrega, de graça, as mesmas rotas para o Web Console (#1402–#1420) e para o
mobile.

---

## 1. Diagnóstico: o que está pronto, o que precisa de adaptação, o que falta

Legenda: ✅ implementado e funcionando · ⚠️ existe, precisa de adaptação ·
❌ não existe.

### 1.1 Instalação, detecção de versão e atualização

| Item | Estado | Evidência |
|---|---|---|
| Instaladores da CLI (`install.sh`/`install.ps1`, binários crus, `.tar.gz`/`.zip`, `.deb`/`.rpm`/AppImage) | ✅ | `release.yml`; regra 15/16 do `CLAUDE.md` |
| Instaladores do **desktop**: Windows MSI + NSIS (x86_64), Linux `.deb` + AppImage (x86_64) | ✅ best-effort, **sem assinatura** | `.github/workflows/desktop.yml`, `release.yml:483-599`; `docs/installation.md:134` (SmartScreen) |
| Desktop para **macOS** (`.app`/`.dmg`) | ❌ nenhum job; só a CLI `garraia-macos-{x86_64,aarch64}` | `release.yml:147-218`; ROADMAP §4.1 "DMG (Mac, notarizado — exige segredos Apple)" |
| Desktop para Windows ARM64 / Linux ARM64 | ❌ (CLI existe para os dois) | `release.yml:232`, `:66` |
| Sidecar dentro do bundle (`binaries/garraia-<triple>`) | ✅ | `tauri.conf.json` `externalBin`; `scripts/build-desktop-linux.sh:44`; `build-installer.ps1:43`; assert no `desktop.yml` |
| `.deb` do desktop instala o sidecar como `/usr/bin/garraia` (`Provides/Conflicts/Replaces: garraia`) | ✅ | `tauri.conf.json` `bundle.linux.deb`; `desktop.yml` verifica os dois caminhos |
| `garraia update`: releases/latest, asset por nome exato, `.sha256` irmão **obrigatório**, `.old` + `rollback`, cache de 24 h, `release_is_newer` numérico, confirma a menos de `--yes` | ✅ | `crates/garraia-cli/src/update.rs` (`run_update`, `check_for_update_notice`, `spawn_background_check`) |
| Versão mais nova exposta ao painel | ✅ `GET /api/status` devolve `latest_version` a partir do cache que o `start` alimenta em background | `router.rs:702-716`; `main.rs:1877` (`update::spawn_background_check`) |
| `garraia update` num sidecar dentro de `Program Files` / `/usr/bin` | ⚠️ escreve ao lado do binário real; sem privilégio falha com EACCES e sem mensagem específica | `update.rs` (`installed_exe`, `fs::rename`) |
| Auto-updater do **desktop** (`tauri-plugin-updater`) | ❌ inerte: `pubkey` vazio, nenhum workflow publica `latest.json`; "Check for Updates" mostra erro | `tauri.conf.json` `plugins.updater`; `docs/releasing.md:84-106` |
| Detecção de Garra instalado fora da `PATH` | ⚠️ existe para o **desktop** (`garraia-desktop-core::locate`: instalador > `PATH` > lado a lado); para o gateway em execução só há PID file + porta | `locate.rs:148-215`; `main.rs:1071` (`garraia.pid`), `:1155` (`find_pid_on_port`) |

### 1.2 Gateway: subir, verificar, encerrar

| Item | Estado | Evidência |
|---|---|---|
| `garraia start` em primeiro plano grava PID file; bind default `127.0.0.1:3888` (`--host/--port` > `HOST/PORT` > default; chaves do arquivo deprecadas) | ✅ | `main.rs:1856-1897`; `garraia_config::bind` |
| `start -d` (daemon) | ✅ Unix (double-fork + `setsid`) · ❌ **Windows** ("not supported") | `main.rs:2782`, `:2885` |
| `stop`/`restart`: PID file → fallback por porta (`lsof` / `taskkill /F`) | ✅ | `main.rs:2900-2990` |
| `status`: reconcilia PID com `GET /api/status` (com bearer se houver `api_key`) | ✅ | `main.rs:2000-2072` |
| Saúde de verdade: `/api/health` (`healthy/degraded/unhealthy`, `version`, `provider`, `model`, `channels`, `checks`) e `/api/diagnostics` (checks com `next_step`, secret-free) | ✅ | `health.rs`; `diagnostics_handler.rs` (`gateway.responds`, `provider.default`, `whatsapp.linked`, …) |
| Porta ocupada | ⚠️ `TcpListener::bind(addr).await?` estoura como erro cru; não há "já está rodando, vou reutilizar" | `server.rs:1291` |
| Bind exposto sem credencial | ✅ recusado (exit 78, #1261); loopback sem `api_key` continua permitido | `server.rs:192`, `:1253` |
| Hot reload de config (`ConfigWatcher` quando `config.yml` existe) | ✅ para admissão do WhatsApp (`enabled`/`allow`/`owners` por mensagem) e allowlist do Telegram; **não** recria provedores nem sobe canal novo | `server.rs:600-616`; `bootstrap/whatsapp_linked.rs:1130` |
| Sidecar no desktop: `spawn("garraia start")`, `restart` = kill + 800 ms + spawn, `kill` no `Exit` | ⚠️ sem checagem de porta/saúde/versão, `unwrap()` em lock, sem reinício em queda | `crates/garraia-desktop/src-tauri/src/gateway.rs` |
| Supervisão testável (Spawner injetável, `RestartPolicy::backoff` que devolve o intervalo, `Drop` mata o filho, `Desired` ≠ `Power`) | ✅ **não consumida pela casca ainda** | `garraia-desktop-core/src/{supervise,state}.rs`; README da crate ("nenhuma outra crate a consome") |

### 1.3 WhatsApp pessoal (dispositivo vinculado, ADR 0023)

| Item | Estado | Evidência |
|---|---|---|
| Ponte Node/Baileys `7.0.0-rc14` **stateless**, NDJSON v1 por stdio, nunca desenha QR nem toca disco | ✅ | `bridge/whatsapp/bridge.mjs`, README; testes `node --test` |
| Ponte embutida no binário (`include_str!`) e materializada em `<data_dir>/whatsapp/bridge/` | ✅ | `bridge.rs:455-476` |
| `npm ci` só quando `node_modules` não casa com o lock; **adoção sem npm** quando o `node_modules/.package-lock.json` bate | ✅ | `bridge.rs:781-801` (`prepare`), `:662-692` (`tree_matches_lock`); `docs/whatsapp.md:531-545` |
| Node/npm: procura **somente na `PATH`** | ⚠️ | `bridge.rs:401-432` (`find_executable`, `NodeRuntime::detect`); gateway idem em `bootstrap/whatsapp_linked.rs:1532` |
| Node **não** é instalado por nenhum instalador nem embutido em bundle nenhum | ❌ | `install.sh`/`install.ps1` sem menção; ADR 0023 §Follow-ups 1 (R5) |
| Sessão: `session.enc` AES-256-GCM, 0600/0700, escrita atômica, arquivo `.prev` no re-vínculo | ✅ | `session.rs` |
| Chave da sessão: `GARRAIA_VAULT_PASSPHRASE` (PBKDF2 600k) ou `session.key` aleatório ao lado, com aviso | ✅ (aviso é parte do contrato) | `session.rs` §"As duas origens de chave"; #1431 aberto |
| Máquina de estados pura (QR 20 s, `MAX_QR_ATTEMPTS = 5`, backoff 1–30 s, `Desired::Off` nunca chega a `Connected`) | ✅ | `state.rs` |
| `runner::pair` (uma vez, termina) e `runner::serve` (longa duração, reconecta, ping 15 s / prazo 45 s) | ✅ ambos sem TTY por dentro (`PairUi` é trait) | `runner.rs:1-140` |
| CLI `garraia whatsapp {link,cloud,status,logout,restore,allow,users,remove,owner,unowner}` | ✅ | `whatsapp.rs`, `whatsapp/acesso.rs` |
| `link` sem terminal | ❌ por desenho: "O QR code é desenhado neste terminal" → exit 69 | `whatsapp.rs:1121-1125`, `:340` |
| Saída legível por máquina | ⚠️ só `users --json`; `status` e `link` não têm | `whatsapp.rs:76-79` |
| Consentimento obrigatório antes do primeiro QR (cliente não oficial, risco de bloqueio, número secundário) | ✅ | `whatsapp.rs:1439-1516` |
| `enabled = true` só depois do `session.enc` em disco | ✅ | `whatsapp.rs:1360-1372`; decisão 8 do ADR |
| Gateway sobe o canal **só no boot** (`spawn_whatsapp_linked`) quando `enabled` + sessão + `node`; sessão criada depois exige `restart` | ⚠️ | `bootstrap/whatsapp_linked.rs:1529-1583`; `acesso.rs:645-660` (`dica_do_gateway`) |
| `allow`/`owners`/`enabled` relidos a quente por mensagem | ✅ | `bootstrap/whatsapp_linked.rs:1120-1135` |
| Saúde única CLI ↔ gateway (`health::classify`: `NotLinked`, `MissingDependencies`, `BridgeDown`, `Connected`, `Linked`) + portão vazio vira `Warning` | ✅ | `health.rs`; `diagnostics_handler.rs:482-560`; teste que varre os dois call sites (`router.rs:2163-2190`) |
| Ponte morre quando o pai some (fecha stdin → sai) | ✅ verificado | `bridge.mjs:797`; `bridge.rs` (`kill_on_drop`, PDEATHSIG no Linux) |
| Envio proativo por HTTP para testar o caminho de saída | ❌ não há rota; `BridgeCommand::Send` só é alcançado pelo sink do turno | `bootstrap/whatsapp_linked.rs:1170-1180`; `channel_send.rs` é só Telegram |
| Mensagem do **próprio** celular vinculado é ignorada (`from_me`) — o dono não conversa com o Garra a partir do mesmo número | ✅ comportamento, ⚠️ implicação de produto | `bridge.mjs:373-393`; `acesso.rs` (`aviso_do_proprio_numero`) |
| Nome que aparece em "Aparelhos conectados": `GarraIA (Desktop)` | ✅ | `bridge.mjs:574` |
| Rate limit de envio (mitigação de banimento do ADR) | ❌ follow-up 6 do ADR 0023, não encontrado no bootstrap | grep em `bootstrap/whatsapp_linked.rs` |
| Cloud API da Meta (`garraia whatsapp cloud`, webhook assinado) | ✅ intocada por este plano | `crates/garraia-channels/src/whatsapp/` |

### 1.4 Configuração de modelos, provedores e credenciais

| Item | Estado | Evidência |
|---|---|---|
| Wizard `garraia init`: presets OpenRouter (default, ADR 0022) / OpenAI / Anthropic, detecção de Ollama e GPU, estratégia `MergeUpdate` que nunca sobrescreve chave existente, arquivo 0600 | ✅ **TTY only** | `wizard/mod.rs`, `wizard/config_writer.rs`, `wizard/env_detect.rs` |
| Gravar provedor/chave sem terminal | ❌ `config set-model` só cobre o launcher local (`ollama-launch`); não há `set-provider` | `main.rs:787-801` |
| `POST /api/providers` | ⚠️ registra **em memória** (vale até o próximo boot); não escreve `config.yml` | `router.rs:874-1000` |
| `PATCH /api/providers/default` | ⚠️ em memória | `router.rs:1556` |
| `POST /api/providers/test` (`available_models`, latência) | ✅ | `router.rs:1448` |
| `PATCH /api/settings` | ⚠️ **dry-run** ("plan 0121a" que não existe em `plans/`) | `settings_handler.rs:6-8` |
| Provedores recarregados quando `config.yml` muda | ❌ o `AgentRuntime` é montado no boot | `server.rs`; watcher só alimenta `current_config()` |
| `config.default.yml` que o desktop copia na primeira execução | ⚠️ ainda traz `gateway.host/port` (deprecados, #1261), `voice.enabled: true` e embeddings apontando para LM Studio em `:1234` — um diagnóstico de instalação limpa nasce com avisos que não são do usuário | `crates/garraia-desktop/src-tauri/resources/config.default.yml` |
| Diretório de config/dados canônico, o mesmo para CLI, gateway e desktop | ✅ | `ConfigLoader::default_config_dir()` (`loader.rs:19-46`); `lib.rs` da casca (#1240) |

### 1.5 Interfaces que já existem

| Superfície | O que cobre | Limite para este objetivo |
|---|---|---|
| **Web Console** (`GET /`, 235 KB, Garra Glass) | Dashboard, Chat, Providers, Channels, Settings (dry-run), Diagnostics, Logs | Nenhuma tela de WhatsApp/onboarding; roda no navegador do usuário, não supervisiona processo; 20+ issues P1 pedem páginas de WhatsApp lá (#1402–#1411, #1420) |
| **Desktop Tauri** (`crates/garraia-desktop`) | Papagaio, Chat Bar (`Ctrl+Space`), bandeja (9 itens), hotkeys, autostart, notificações, Settings mínima, sidecar | Sem janela principal, sem onboarding, sem estado do gateway/WhatsApp; `csp: null`, `withGlobalTauri: true`, `shell:allow-execute` na capability; porta `3888` fixa em `ui/ws.js`; **fora de todos os gates de CI** (`--exclude garraia-desktop`) |
| **`garraia desktop`** (CLI, M1) | Localiza e lança o app instalado; `--status` diz "execução não detectável" de propósito | Sem instância única (M2) |
| **Mobile** (`apps/garraia-mobile`) | Negocia `/api/capabilities` | Fora de escopo aqui; beneficia-se das rotas novas |

### 1.6 O que a documentação sugere e o código não entrega (para não presumir)

- "Check for Updates" do desktop **não funciona** (erro visível; `docs/releasing.md:84`).
- `garraia whatsapp` só pareia **no terminal**; não há QR em nenhuma GUI.
- Vincular com o gateway já rodando **exige** `garraia restart` (o canal só
  sobe no boot); o `allow` posterior vale a quente, o vínculo não.
- O Web Console "configura provider", mas **não persiste**: some no restart.
- `install.sh`/`install.ps1` **não** instalam Node; o passo do QR falha em
  máquina sem Node com uma frase de instalação (`whatsapp.rs:1540`).
- macOS: há CLI, **não há app desktop**; Windows ARM64 idem.
- Nada foi testado ponta a ponta em macOS neste repositório; o Windows tem
  build de MSI em CI, sem execução.

---

## 2. Arquitetura recomendada

### 2.1 Evoluir, complementar ou empacotar?

| Opção | Veredito | Por quê |
|---|---|---|
| **A. Evoluir `garraia-desktop` (opção D do ADR 0021)** | **✅ escolhida** | Já é um app, já roda o sidecar, já é empacotado nos dois SOs; a lógica vai para `garraia-desktop-core` (nos gates de CI) e para rotas do gateway (idem). Zero processo novo, zero tray novo |
| B. Aplicativo complementar novo ("Garra Setup") | ❌ | Dois processos, dois trays, dois empacotamentos e dois updaters — exatamente o que o ADR 0021 rejeitou na opção B; e a casca atual continuaria sem saber se o gateway está saudável |
| C. Só empacotar o que existe (Web Console no navegador + script de instalação) | ❌ | Não há QR fora do terminal, ninguém supervisiona o gateway, e "abrir o navegador em `localhost:3888`" já é a experiência que o pedido descarta |

### 2.2 Princípios que decidem o resto

1. **O gateway é o único dono da ponte do WhatsApp — pareamento incluído.**
   Hoje a CLI pareia (`runner::pair`) e o gateway serve (`runner::serve`),
   em processos diferentes, e por isso existe o "rode `restart`". Se o
   supervisor do gateway souber **parear** (ele já tem store, chave, ponte,
   `prepare`, config writer compartilhado e classificação de saúde), o app
   só consome eventos; não há duas pontes disputando a mesma sessão, e o
   canal entra em `serve` sem reiniciar nada. Web Console e mobile ganham o
   mesmo caminho.
2. **Lógica onde o CI alcança.** Máquina de estados do onboarding, cliente
   HTTP do gateway, detecção/adoção de gateway existente e regra de
   compatibilidade moram em `garraia-desktop-core`; rotas e pareamento moram
   em `garraia-gateway`/`garraia-channels`. A casca Tauri só desenha,
   supervisiona o processo (com o `supervise` do core) e injeta ambiente.
3. **A webview nunca muta o gateway diretamente.** A guarda anti-CSRF
   (#1182) recusa `POST`/`PATCH` vindos de `tauri://localhost`
   (`origin_guard.rs:561-576` não tem exceção Tauri; só a leitura e o
   handshake de WS têm, `:591-604`, `:624`). Isso é correto e fica assim: o
   lado **Rust** do app fala HTTP com o gateway (sem `Origin`, como a CLI) e
   expõe comandos Tauri **estreitos e tipados** à UI. Consequência: dá para
   fechar a CSP e tirar `shell:allow-execute` da capability, como o ADR 0021
   §Superfície de segurança pede antes da primeira aba.
4. **O runtime vai dentro do aplicativo.** A CLI já vai (sidecar). O Node
   passa a ir também (§2.4). Nenhum passo "instale isto antes".
5. **Nada em dobro.** Reusar `runner::pair`/`serve`, `SessionStore`,
   `health::classify`, `bridge::prepare`, o cache do `update.rs` (via
   `/api/status`), `diagnostics`, `config_writer` (movido para
   `garraia-config`), `supervise`/`state`/`locate` do core.

### 2.3 Componentes e onde mora cada um

| Componente | Crate | Novo / adaptado | Testado por |
|---|---|---|---|
| Controlador do canal `whatsapp_linked` com comandos (`Pair`, `Logout`, `Reload`) e **eventos** (`broadcast`) | `garraia-gateway::bootstrap::whatsapp_linked` (+ `garraia-channels::whatsapp_linked::runner`) | adaptado: o supervisor passa de "só `serve` no boot" a máquina `Idle → Pairing → Serving` | fixture `fake_whatsapp_bridge.py` (cenários `pair-ok`, `pair-expire-then-ok`, `logged-out`, `crash-after-qr`, `network-flap`) |
| Rotas `GET /api/whatsapp/linked`, `POST …/pair`, `GET …/events` (SSE), `POST …/logout`, `POST …/allow`, `GET …/activity` | `garraia-gateway` | novo | testes de rota (Axum) + fixture |
| `PairUi` que publica no `broadcast` (QR já renderizado em **SVG** pelo `qrcode` que a crate já usa) | `garraia-channels::whatsapp_linked::qr` | novo (pequeno) | unit |
| Gravação de `allow`/`owners`/`enabled` compartilhada CLI ↔ gateway | mover de `garraia-cli::whatsapp::acesso` para `garraia-gateway::bootstrap` (a CLI já depende do gateway para `whatsapp_linked_settings`) | adaptado | os testes de `acesso.rs` (3 726 linhas em `whatsapp/tests.rs`) continuam valendo na CLI |
| `NodeRuntime::detect` aceita caminho explícito (`channels.whatsapp_linked.node_bin` / `GARRAIA_NODE_BIN`) antes da `PATH` | `garraia-channels::whatsapp_linked::bridge` | adaptado | unit (já há teste de detecção injetada) |
| `garraia config set-provider <preset> --model … --api-key-stdin` (não interativo) + `config_writer` movido para `garraia-config` | `garraia-cli`, `garraia-config` | novo/adaptado | testes do `config_writer` migram junto |
| `garraia whatsapp status --json` | `garraia-cli` | novo (pequeno) | `whatsapp_smoke` |
| `onboarding` (máquina de estados pura das telas de §3), `gateway_client` (HTTP, sem Tauri), `runtime` (sonda/adota/lança gateway via `supervise`), `compat` (regra de versão) | `garraia-desktop-core` | novo | tabelas + servidor HTTP falso |
| Janela principal, telas, bandeja com status, instância única, fechar-para-bandeja, injeção de env no sidecar, CSP | `garraia-desktop` (casca) | novo/adaptado | build no `desktop.yml` + checklist manual "não quebrou nada" (ADR 0021) |
| Node + `node_modules` pré-instalados por plataforma em `resources/` | `desktop.yml`, `release.yml`, `scripts/build-*` | novo | asserts no workflow (binário presente, `node --version`, `node_modules/.package-lock.json` bate com o lock embutido) |

### 2.4 Processos em execução (um ícone, um app)

```text
garraia-desktop (Tauri)                      ← único processo visível ao usuário
 ├─ webview: janela principal · papagaio · Chat Bar   (UI local, CSP fechada)
 ├─ desktop-core: onboarding + supervise + gateway_client (HTTP → 127.0.0.1:<porta>)
 └─ filho: garraia start --port <p>          ← sidecar do bundle, env: GARRAIA_NODE_BIN=<bundle>/node
      └─ filho: node bridge.mjs             ← só quando há WhatsApp vinculado; morre com o pai
```

Na bandeja continuam os nove itens de hoje; entram "Abrir Garra Desktop" (a
janela principal) e uma linha de status ("WhatsApp conectado · pronto").

### 2.5 Runtime embutido ou baixado? (a pergunta do §2 do pedido)

Duas peças de runtime, avaliadas separadamente.

| | **Embutir no instalador** | **Baixar na configuração** |
|---|---|---|
| **Garra (CLI/gateway)** | Já é assim (sidecar). Tamanho: o binário `garraia` (~50–60 MB). Versão do app e do runtime **sempre casam**. Offline funciona. | Instalador menor (só a casca), mas reintroduz o dialogo "O Garra precisa ser instalado" no caminho feliz, um download com checksum, um `.deb` que exige root para `/usr/bin`, e **duas versões que podem divergir** (protocolo do `/ws/parrot`, JSON da CLI). |
| **Node (ponte do WhatsApp)** | Medido nesta máquina: binário Node 22 ≈ **120 MB** descompactado (≈ 30 MB comprimido); `node_modules` da ponte ≈ **76 MB** (inclui `sharp`, peer dep que a ponte não usa). Estimativa: **+45–60 MB** no instalador, +200 MB em disco — *a medir no CI*. Offline funciona; nada de `npm ci` na máquina do usuário; versão fixada e verificada no build; no macOS o Node **precisa entrar na assinatura/notarização** (hardened runtime com `allow-jit`). | Instalador pequeno; download de ~30 MB do `nodejs.org/dist` **com SHA-256 pinado no build** (não confiar no `SHASUMS256.txt` baixado); mais um passo com progresso, mais um modo de falha (proxy, offline, disco); `npm ci` ainda roda (rede + npm) ou o app baixa também um tarball de `node_modules` que ele mesmo publica. |

**Recomendação: embutir os dois.** A experiência pedida é "instalei, cliquei,
escaneei"; cada download em tempo de configuração é um estado a mais para
explicar e um lugar a mais para falhar. O custo é tamanho, e tamanho é
previsível. Duas otimizações a medir antes de decidir o número final:
(a) excluir `sharp` da árvore pré-instalada (a ponte não o importa; o README
da ponte já diz isso), (b) `node` sem `npm` (o runtime pré-instalado torna o
`npm` desnecessário; `prepare` adota a árvore pelo `.package-lock.json`).

Consequência direta para o pedido: **o diálogo "O Garra precisa ser
instalado… Deseja instalar agora?" deixa de existir no caminho feliz** — o
instalador traz tudo. Ele sobrevive como texto de **um** caso de borda: o
app achou um Garra externo incompatível e o usuário escolheu "usar o Garra
do aplicativo" (§2.6). Os consentimentos que ficam são os que importam:
cliente não oficial do WhatsApp (obrigatório, ADR 0023), atualização,
iniciar com o sistema.

A alternativa estratégica **whatsmeow (Go, binário estático — opção B do ADR
0023)** eliminaria o Node de vez. O ADR a deixou como follow-up R5 porque
mexe nos assets crus da release (regra 15). Observação nova deste
levantamento: **dentro do bundle do desktop** um sidecar Go não toca os
assets crus — mas criaria dois transportes de WhatsApp (Node na CLI, Go no
desktop), duas implementações da mesma coisa. Fica registrado em §9 como
decisão do dono, não como recomendação deste plano.

### 2.6 "Garra já instalado": detecção e adoção

Regra em `desktop-core::runtime`, nesta ordem, **lendo antes de agir**:

1. **Há algo respondendo na porta?** `GET /api/health` em `127.0.0.1:3888`
   (ou na porta que o app gravou da última vez).
   - Responde e `version == versão do sidecar do app` → **adota** (não lança
     nada; marca `origem = externo`; **nunca** o encerra ao fechar o app).
   - Responde com mesmo `MAJOR.MINOR`, patch diferente → adota, com nota em
     "Diagnóstico".
   - Responde com versão incompatível → estado *"Precisamos da sua
     atenção"*: "Há um Garra vX em execução (PID N, pelo `garraia.pid`). Este
     aplicativo precisa da vY." Ações: **"Usar o Garra do aplicativo em outra
     porta"** (lança o sidecar com `--port 3889`, `ws.js` passa a receber a
     porta do Rust) ou **"Abrir instruções"**. O app **não mata** processo
     que não lançou.
   - Não responde, mas a porta está ocupada por outra coisa → "Porta 3888 em
     uso por outro programa" + mesma ação de porta alternativa.
2. **PID file** (`<config_dir>/garraia.pid`) vivo sem HTTP → "ainda
   iniciando" por até 15 s, depois "atenção".
3. **Nada** → lança o sidecar do bundle com o `supervise` do core
   (`Desired::On`; queda vira `Power::Failed` e reinício com backoff, não
   silêncio) e espera `/api/health` antes de sair de *"Preparando sua
   conexão…"*.

Config e dados são **sempre** reaproveitados: o app usa
`ConfigLoader::default_config_dir()` como a CLI (já é assim, #1240), então
`config.yml`, `session.enc`, `allow` e histórico de um Garra instalado por
`install.sh` valem no app sem cópia. `garraia status` no terminal mostra o
gateway do app (o sidecar grava o mesmo `garraia.pid`).

### 2.7 Atualização: aplicativo × runtime

| Pergunta | Resposta |
|---|---|
| O que se atualiza? | **O aplicativo, como unidade** (casca + sidecar + Node). O app nunca roda `garraia update` no próprio sidecar: isso quebraria a igualdade de versão e falharia em `Program Files`/`/usr/bin` sem privilégio. |
| Como sabe que há versão nova? | `GET /api/status.latest_version`, que já vem do cache de 24 h que o sidecar alimenta em background (`update.rs`). Zero rede no caminho do WhatsApp; a checagem **nunca** atrasa a conexão. Regra de comparação: a mesma `release_is_newer` (numérica, sem convite a rebaixar). |
| O que o usuário vê? | Faixa discreta na janela principal: *"Uma nova versão do Garra está disponível (vY)."* — **Atualizar agora** / **Mais tarde**. Nunca silencioso. |
| "Atualizar agora" faz o quê? | **v1 (sem chave de assinatura):** baixa o bundle da plataforma da release (`garraia-desktop-<os>-<arch>.<msi/exe/deb/AppImage/dmg>`), confere o `.sha256` irmão (contrato da regra 15), avisa *"O WhatsApp vai desconectar por alguns segundos"*, e lança o instalador (MSI/NSIS/DMG) ou, no AppImage, troca o arquivo e relança; `.deb` exige `pkexec`, então mostra o comando e abre o gerenciador de pacotes. **v2 (M7, R5):** `tauri-plugin-updater` com `latest.json` assinado — os comandos `check_for_updates`/`install_update` já existem na casca. |
| Adiar impede o uso? | Não. Sidecar e app são da mesma versão por construção; um gateway externo adotado obedece à regra de `MAJOR.MINOR` de §2.6. |
| Falhou no meio? | O instalador da plataforma é transacional (MSI) ou substitui por rename (AppImage: `.new` → rename, como o `update.rs`). Config, sessão e `allow` moram fora do bundle e não são tocados. |
| E a CLI instalada à parte (`install.sh`)? | Continua com `garraia update`. O app mostra as duas versões em "Diagnóstico" e avisa se o `garraia` da `PATH` for de outra versão (o `update.rs` já faz essa varredura: `report_other_binaries`). |

---

## 3. Fluxo de telas: da primeira abertura à primeira resposta

Uma janela, uma ação principal por etapa, estado sempre visível no topo.
Textos abaixo em pt-BR; a CLI já mantém tabela pt-BR/en para o WhatsApp
(`docs/whatsapp.md` §Textos) e o app reaproveita as frases.

| # | Estado (topo) | O que a tela mostra | Ação principal | Por baixo |
|---|---|---|---|---|
| S0 | **Preparando sua conexão…** | Logo + barra indeterminada + linha do passo ("verificando o Garra", "iniciando") | — | §2.6: sonda → adota ou lança sidecar → espera `/api/health` |
| S1 | **Como o Garra vai pensar?** (só se `provider.default` não estiver `ok` em `/api/diagnostics`) | Três cartões: **OpenRouter** (recomendado; "serviço externo, pode gerar custo; você precisa de uma chave" + link `openrouter.ai/keys`), **Ollama neste computador** (só aparece se detectado em `:11434`; "grátis, local, mais lento"), **Já configurei** | "Testar e continuar" | `garraia config set-provider` (chave por stdin), restart do sidecar, `POST /api/providers/test` |
| S2 | **Antes de conectar** | O aviso do ADR 0023 (cliente não oficial; a conta pode ser bloqueada; use um número secundário) + **"O celular que você vai escanear não conversa com o Garra — use outro número para falar com ele"** | "Entendi, continuar" / "Agora não" | Consentimento gravado no pedido de `pair` (`accept_unofficial_client_terms: true`); sem ele o gateway recusa |
| S3 | **Escaneie o QR Code** | QR grande (SVG do gateway), instruções curtas (Configurações → Aparelhos conectados → Conectar um aparelho), contador "expira em 18 s", "tentativa 2/5" | — (renova sozinho; "Cancelar" secundário) | `POST /api/whatsapp/linked/pair` + SSE `qr`/`status`; após 5 QRs: *"Nenhum QR foi lido"* + "Tentar de novo" |
| S4 | **WhatsApp conectado** | ✓ + "número terminado em 1234" + **"Quem pode falar com o Garra?"** (campo de número com código do país, botão "+ outro", opção "Ninguém por enquanto" com aviso de que toda mensagem será ignorada) | "Continuar" | `authenticated → connected`, `session.enc` salvo, `enabled: true`; `POST …/allow` (vale a quente); o supervisor entra em `serve` sem restart |
| S5 | **Garra pronto para responder** | Quatro checks verdes: Garra ativo · modelo respondendo · WhatsApp conectado · N número(s) autorizado(s). Cartão **"Testar agora"**: *"Do número autorizado, envie 'oi' para este WhatsApp"* com três ticks ao vivo: recebida → respondida → enviada | "Concluir" | `/api/diagnostics` (`gateway.responds`, `provider.default`, `whatsapp.linked` sem `Warning`) + SSE de `activity` (contadores, sem conteúdo nem número) |
| S6 | **Início** (estado de regime) | Cartão de status (WhatsApp conectado / Reconectando… / Precisamos da sua atenção), último recebimento/resposta (horário, nunca texto), ações **Reconectar**, **Desconectar**, **Trocar conta**; faixa de atualização quando houver; toggles *Iniciar com o sistema* e *Ao fechar, continuar na bandeja*; link "Diagnóstico" | — | `GET /api/whatsapp/linked` + SSE; `logout` = `POST …/logout` (apaga sessão, `enabled: false`); *Trocar conta* = logout + S2 |
| S7 | **Diagnóstico** (secundário) | Lista de checks do `/api/diagnostics` com `next_step`, versões (app, sidecar, `garraia` na `PATH`, Node, Baileys), cauda do log redigida, botão **Copiar** / **Exportar** | — | `/api/diagnostics`, `/api/logs` (já passam por `RedactingWriter`; nunca JID inteiro, nunca chave) |

Regras transversais:

- **Reabrir com sessão válida** pula S2–S4: S0 → (S1 se faltar provedor) →
  S6 com "WhatsApp conectado" (o `serve` do gateway revalida a sessão;
  `session_found → validating → connected`).
- **Fechar a janela**: primeira vez pergunta *"Manter o Garra respondendo no
  WhatsApp em segundo plano?"* (padrão: sim, fica na bandeja; a escolha vira
  preferência). **Sair** na bandeja avisa: *"O Garra vai parar de responder
  no WhatsApp"* e encerra o sidecar (e a ponte com ele). Nunca há processo
  oculto contra a escolha do usuário; um gateway **adotado** (externo) não é
  encerrado por definição.
- **Iniciar com o sistema** é opcional (plugin de autostart já existe;
  `--autostart` abre minimizado na bandeja).

Erros e recuperação (mínimo exigido pelo pedido):

| Situação | Mensagem principal | Recuperação |
|---|---|---|
| Sem internet | "Sem conexão com a internet. O Garra vai tentar de novo sozinho." | `serve` reconecta com backoff; S1 e a checagem de versão esperam |
| Falha de download (atualização) | "Não consegui baixar a atualização. Nada foi alterado." | "Tentar de novo" / "Mais tarde"; checksum inválido = descarta e avisa |
| Instalação incompleta / dependências da ponte | "Um componente do WhatsApp está incompleto." (só possível fora do bundle padrão) | "Reparar" = `bridge::prepare` de novo; detalhe em Diagnóstico |
| Permissões insuficientes (diretório de dados, porta) | "Não consegui gravar em <pasta>." | Caminho exato + "Abrir pasta"; nunca pede elevação para isso |
| Versão incompatível (gateway externo) | §2.6 | "Usar o Garra do aplicativo em outra porta" |
| Gateway indisponível / caiu | "Reconectando…" (até 30 s), depois "Precisamos da sua atenção: o Garra parou." | Reinício automático com backoff pelo `supervise`; botão "Reiniciar agora"; log em Diagnóstico |
| Porta ocupada | "A porta 3888 está em uso por outro programa." | Porta alternativa |
| QR expirado | (silencioso) "QR renovado — tentativa n/5" | Automático; após 5, S3 com "Tentar de novo" |
| Sessão inválida (celular removeu o aparelho, Meta recusou) | "O WhatsApp desconectou este aparelho. Vamos conectar de novo." + código do WhatsApp em Diagnóstico | Volta a S2/S3; `session_dead` já apaga o material |
| Erro de autenticação no provedor (401) | "A chave do <provedor> foi recusada." | Volta a S1 com o campo em foco; a chave antiga não é apagada até a nova passar no teste |
| Ninguém autorizado | Aviso amarelo permanente em S6 (o `/api/diagnostics` já rebaixa para `Warning`) | "Autorizar um número" |

---

## 4. Mudanças necessárias

### 4.1 Gateway (`garraia-gateway` + `garraia-channels::whatsapp_linked`) — caminho crítico

1. **Controlador do canal com pareamento.** `WhatsAppLinkedRuntime` ganha um
   `mpsc` de comandos (`Pair { consent, allow }`, `Logout`, `Reload`) e um
   `broadcast` de eventos (`Phase`, `Qr { svg, attempt, max, expires_in }`,
   `Connected { phone_last4 }`, `Failed { reason }`, `Activity {…}`). O
   supervisor vira máquina `Idle → Pairing → Serving → Idle` sobre o
   `state.rs` que já existe: `Pair` pausa o `serve` (cancel watch), roda
   `runner::pair_with` com um `PairUi` que publica no `broadcast`, grava
   sessão (o runner já grava), liga `enabled` (função compartilhada), e
   entra em `serve` — **sem restart**. Guardas: um pareamento por vez;
   consentimento obrigatório no comando; `Desired::Off` respeitado.
2. **Rotas** (todas secret-free, sob a guarda anti-CSRF e sob o gate de
   `gateway.api_key` quando ela existir; mutantes exigem `Content-Type:
   application/json` e corpo explícito):
   - `GET /api/whatsapp/linked` → saúde (`health::classify`), fase, Node
     (achado? versão), Baileys, `autorizados`, `donos`, `enabled`, `origem da
     chave` (passphrase | arquivo), contadores de atividade.
   - `POST /api/whatsapp/linked/pair` `{accept_unofficial_client_terms: true,
     relink: bool}` → `202` + `pairing_id`.
   - `GET /api/whatsapp/linked/events` → SSE dos eventos acima (o QR como
     SVG; nunca JID inteiro).
   - `POST /api/whatsapp/linked/logout`, `POST /api/whatsapp/linked/allow`
     `{number, owner?}` (mesma validação E.164 e as mesmas recusas de
     `acesso.rs`: sem código do país, curinga, `owner` fora de `isolated-pod`).
   - `GET /api/whatsapp/linked/activity` (ou dentro do SSE): `inbound_total`,
     `answered_total`, `rejected_total`, `last_inbound_at`, `last_outbound_at`,
     `last_error` — **contagens e horários, nunca conteúdo nem remetente**.
   - Opcional, a validar contra o Baileys real: `POST
     /api/whatsapp/linked/self-test` envia *"Teste do GarraIA ✓"* para o
     **próprio** JID (conversa "Você"), provando o caminho de saída sem
     mandar nada a terceiros.
3. **Subida a quente.** O watcher já relê `enabled`; falta o supervisor
   reagir: `enabled` virou `true` + `session.enc` presente + Node → `Reload`.
   Fecha o caso "vinculou pela CLI com o app aberto".
4. **`node_bin` explícito.** `NodeRuntime::detect()` e o `find_executable`
   do boot passam a olhar `channels.whatsapp_linked.node_bin` e
   `GARRAIA_NODE_BIN` **antes** da `PATH` (e o `/api/diagnostics` diz qual
   Node está em uso). Sem isso nada do resto funciona num app gráfico.
5. **Porta ocupada com mensagem.** `server.rs:1291` traduz `AddrInUse` em
   erro nomeado com a porta e o PID (quando o `find_pid_on_port` souber).
6. **Rate limit de envio** (ADR 0023 follow-up 6): entra junto, porque o
   app vai tornar o canal muito mais acessível a quem não lê o ADR.

Risco **R4** (superfície de conta de WhatsApp em rota local; `security-auditor`
obrigatório). Postura recomendada: as rotas de `pair`/`logout`/`allow`
exigem bearer **sempre que** `gateway.api_key` existir e, sem ela, valem só
em bind loopback (que já é a única forma de subir sem credencial). O app
lê a chave do mesmo `config.yml` (o comando `gateway_api_key` já faz isso).

### 4.2 CLI e config (`garraia-cli`, `garraia-config`)

1. `garraia config set-provider <openrouter|openai|anthropic|ollama>
   [--model …] [--base-url …] [--api-key-stdin | --api-key-env VAR]
   [--default]`: não interativo, chave **nunca em argv**, estratégia
   `MergeUpdate` (não sobrescreve chave existente sem `--replace-key`),
   arquivo 0600. Para isso `config_writer` (1 219 linhas, com testes) migra
   de `garraia-cli::wizard` para `garraia-config` — o desktop já depende de
   `garraia-config`, e o wizard passa a chamar o mesmo código.
2. `garraia whatsapp status --json` (mesmo `classify`, mesmos fatos de disco).
3. `garraia whatsapp link` **não muda**: continua o caminho do terminal.
   Quando o gateway local estiver de pé com o controlador de §4.1, o `link`
   pode delegar a ele (um `link` que não pede `restart`); é melhoria, não
   pré-requisito.
4. `update.rs`: mensagem específica para EACCES em diretório do sistema
   ("este `garraia` faz parte do Garra Desktop; atualize pelo aplicativo").
5. `config.default.yml` do desktop: remover `gateway.host/port`, desligar
   `voice.enabled` e o bloco de embeddings do LM Studio por padrão (uma
   instalação limpa não pode nascer com avisos de serviço ausente).

### 4.3 `garraia-desktop-core` (sem Tauri, nos gates)

- `onboarding`: enum de telas de §3 + transições dirigidas por fatos
  (`GatewayProbe`, `Diagnostics`, `LinkEvent`, ações do usuário) — puro,
  tabelado, no molde de `state.rs`.
- `gateway_client`: `reqwest` bloqueante ou `tokio` mínimo para `/api/health`,
  `/api/status`, `/api/diagnostics`, `/api/channels`, `/api/whatsapp/linked/*`
  e SSE; sem `Origin`; bearer quando houver chave; testado contra servidor
  falso.
- `runtime`: a regra de §2.6 (sonda → adota → lança), sobre `supervise` e
  `locate`; `compat::mesma_linha(app, gateway)`.
- `update`: leitura de `latest_version`, escolha do asset do bundle por
  plataforma, verificação do `.sha256` (mesma função do `update.rs`, extraída
  para `garraia-common` se preciso).
- `env`: montagem do ambiente do sidecar (`GARRAIA_NODE_BIN`, porta,
  `GARRAIA_NO_UPDATE_CHECK` **não** — a checagem em background é desejada).

### 4.4 Casca Tauri (`garraia-desktop`)

- Janela `main` com as telas de §3 em HTML/JS vanilla e tokens `--garra-*`
  (ADR 0009; sem CDN). Gatilho S1 do ADR 0007 é reavaliado no início do M2,
  como já previsto; este plano não o antecipa.
- `gateway.rs` passa a delegar a `desktop-core::runtime` (some o `unwrap()`,
  some o `sleep(800)`, entra reinício com backoff e leitura de saúde).
- Comandos Tauri estreitos: `onboarding_state`, `start_pairing`,
  `authorize_number`, `logout`, `run_self_test`, `open_diagnostics`,
  `set_close_to_tray`, `apply_update`. **Nenhum** recebe caminho, comando ou
  URL da UI.
- Segurança da webview (ADR 0021 §Superfície): CSP definida, `withGlobalTauri`
  só onde a UI precisa, `shell:allow-execute`/`allow-kill` **fora** da
  capability das janelas (o `open_log_dir` já usa `Command` no Rust).
- `tauri-plugin-single-instance` (pré-requisito do M2; hoje não está no
  `Cargo.lock`): segunda abertura foca a janela; `garraia desktop --status`
  passa a saber "rodando".
- `ui/ws.js`: porta e chave vêm do Rust (`gateway_endpoint`), não da
  constante `3888`.
- Preferência *fechar-para-bandeja* persistida ao lado de `chat-bar.json`.

### 4.5 Ponte (`bridge/whatsapp`)

- Nada obrigatório. Recomendado: publicar `activity` (contadores) como
  evento periódico ou derivá-los no Rust a partir de `message`/`sent` (mais
  simples: no Rust). `self-test` usa `send` com `chat_jid = ownJid`, a
  validar contra o Baileys real antes de prometer.

### 4.6 O que **não** muda

Papagaio, Chat Bar, hotkeys, os nove itens da bandeja, `garraia whatsapp
link` no terminal, o canal Cloud API, `install.sh`/`install.ps1`, os nomes
dos assets crus da release, `garraia update` para quem instala a CLI à parte,
o formato de `config.yml`, `session.enc` e `chat-bar.json`.

---

## 5. Plano por etapas

Ordem de dependência: **E0 → (E1 ∥ E2) → E3 → E4 → E5 → E6 → E7.** E1 é o
caminho crítico; E2 e E3 podem começar com E1 em revisão porque falam com
E1 por contrato HTTP (mock).

| Etapa | Entrega | Depende de | Risco / time | Critério de aceite |
|---|---|---|---|---|
| **E0 — Decisões** | Amendment do ADR 0021 ("o gateway é dono do pareamento"; onboarding do WhatsApp como primeira aba do Control Center, antes das abas genéricas do M2) e do ADR 0023 (Node embutido no desktop; rotas de pareamento); respostas de §9 | dono | R5 | ADRs com status atualizado; decisões de §9 respondidas ou explicitamente adiadas |
| **E1 — Gateway pareia** | §4.1 itens 1–6 | E0 | **R4** — `security-auditor` + `code-reviewer` | `curl` + `fake_whatsapp_bridge.py`: `pair-ok` sobe até `Connected` e entra em `serve` sem restart; `pair-expire-then-ok` renova QR; `logged-out` limpa e volta a `NotLinked`; consentimento ausente → 400; `Origin` web → 403; matriz de aceite dos testes de rota; `/api/diagnostics` continua fonte única (teste `router.rs:2163` estendido) |
| **E2 — CLI/config não interativa** | §4.2 itens 1, 2, 4, 5 | E0 | R2 | `set-provider` com chave por stdin grava 0600 e não sobrescreve chave existente; `whatsapp status --json` casa com o `/api/whatsapp/linked`; `config check` de instalação limpa do desktop sem avisos espúrios |
| **E3 — desktop-core** | §4.3 | contrato de E1 (mock) | R2 | tabelas: toda transição de §3 e todo erro da tabela de erros; adoção/porta/versão de §2.6 em tabela (incl. Windows via `Platform` injetada); zero `unwrap`, zero Tauri; cobre nos gates |
| **E4 — Casca** | §4.4 | E1, E2, E3 | R3 | checklist "não quebrou nada" do ADR 0021 (papagaio, `Alt+G`, Chat Bar, bandeja, sidecar morre com o app, configs antigas) + fluxo S0→S6 em Linux (X11 e Wayland) e Windows com telefone real; CSP definida; `desktop.yml` roda clippy/test da casca no job Linux |
| **E5 — Runtime embutido e macOS** | Node + `node_modules` por triple em `resources/`; `GARRAIA_NODE_BIN` injetado; jobs `.dmg` (arm64 e x86_64) best-effort; asserts | E4 | **R5** (toca `release.yml`; regras 15/16 continuam intactas porque os bundles são aditivos) | instalação **limpa** sem Node no sistema pareia (cenário 1 de §6.3); tamanho medido e registrado; `node_modules/.package-lock.json` bate com o lock embutido (adoção sem npm); `.dmg` abre com o passo do Gatekeeper documentado |
| **E6 — Atualização com consentimento** | §2.7 v1 (faixa + download + checksum + instalador); item 4 de §4.2 | E5 | R3 (R5 quando entrar a chave do updater) | "Mais tarde" não impede o uso; checksum inválido descarta; aviso de desconexão antes de aplicar; config/sessão intactas após atualizar |
| **E7 — Dogfood, docs e issues** | Cenário #1426 automatizado onde couber (container + fixture); `docs/desktop.md` (instalar, atualizar, desinstalar por SO, Gatekeeper/SmartScreen), wiki, fragmentos de changelog; fechar/atualizar #1419/#1420/#1429/#1431 conforme o que E1–E6 entregarem | E6 | R2 | os 8 cenários de §6.3 com coluna "executado em" preenchida por SO |

Cada etapa é um PR (ou um trem curto) com fragmento em `changelog.d/`. E1 e
E5 param para revisão humana por regra do repositório.

---

## 6. Empacotamento, atualização e testes nos três sistemas

### 6.1 Matriz de distribuição proposta para a v1

| SO | Arquitetura | Formato | Estado hoje | Versão mínima | Requisitos reais |
|---|---|---|---|---|---|
| Windows | x86_64 | MSI (por máquina) + NSIS (por usuário, `%LOCALAPPDATA%`) | ✅ best-effort, sem assinatura | Windows 10 1809+ (WebView2 evergreen; o bootstrapper padrão do Tauri **baixa** o WebView2 se faltar — offline exige `embedBootstrapper`/`offlineInstaller`, +~150 MB) | Certificado de assinatura (OV/EV) para tirar o SmartScreen — **dono** |
| Windows | ARM64 | — | ❌ | — | Fora da v1; a CLI ARM64 existe, o Node ARM64 existe; entra quando houver runner/teste |
| Linux | x86_64 | `.deb` + AppImage | ✅ best-effort | glibc 2.35 (Ubuntu 22.04+/Debian 12+) | AppImage: `libfuse2`; bandeja: `libayatana-appindicator3` (o `.deb` declara) e, no GNOME, a extensão AppIndicator; Wayland: overlay sem `always_on_top` (limitação conhecida, não bloqueia o onboarding) |
| Linux | ARM64 | — | ❌ | — | Fora da v1 (a CLI e o AppImage da CLI existem) |
| macOS | arm64 + x86_64 | `.dmg` (app bundle) | ❌ | **macOS 11 Big Sur** (piso dos binários oficiais do Node 22 — confirmar no `BUILDING.md` do Node antes de fixar; Tauri 2 aceita 10.15) | Apple Developer ID + notarização; **o Node embutido também precisa ser assinado** com hardened runtime e entitlement `allow-jit`, senão o Gatekeeper mata a ponte — **dono**. Sem isso: `.dmg` não assinado com instrução de "Abrir mesmo assim" |

Instalação, atualização e desinstalação por formato: MSI/NSIS (Adicionar ou
Remover Programas; atualização = instalador novo por cima), `.deb` (`apt`
gerencia; `apt remove garraia-desktop`), AppImage (arquivo único; remover =
apagar; atualização = trocar o arquivo), `.dmg` (arrastar; remover =
Lixeira). Em todos, **dados e config ficam em
`ConfigLoader::default_config_dir()`** e não são removidos pelo
desinstalador — o app documenta onde estão e oferece "Desconectar" antes de
desinstalar (a sessão do WhatsApp continua válida no servidor da Meta até o
logout).

### 6.2 O que só o mantenedor pode fazer

1. Certificado de assinatura Windows (secrets no repositório; o `release.yml`
   ganha o passo de `signtool`).
2. Conta Apple Developer, Developer ID Application, credenciais de
   notarização (`xcrun notarytool`), e a decisão de assinar o Node embutido.
3. Chave do `tauri-plugin-updater` (`cargo tauri signer generate`) +
   `TAURI_SIGNING_PRIVATE_KEY*` — passo a passo já escrito em
   `docs/releasing.md:93-101`.
4. Máquinas de teste: Windows 10/11 real, macOS Intel e Apple Silicon, dois
   telefones (o vinculado e o autorizado).

### 6.3 Estratégia de testes e os oito cenários de aceite

**Automatizado (CI):** unit + tabelas em `garraia-desktop-core` e
`garraia-channels`; rotas do gateway com a fixture Python (sem Node, sem
telefone); `whatsapp_smoke` e testes do `config_writer` na CLI; asserts de
empacotamento no `desktop.yml` (sidecar, sprite, `ws.js` com token, Node
presente e executável, árvore da ponte igual ao lock); Playwright contra a
UI da janela principal servida em arquivo com `invoke` mockado, usando
`data-testid` (contrato do plan 0052).

**Manual, obrigatório antes de cada release do desktop** (não há como
automatizar telefone nem Gatekeeper): matriz por SO abaixo. O documento
`docs/whatsapp.md` já tem a seção "Validação manual (o que os testes
automatizados não cobrem)" — o desktop ganha a sua.

| # | Cenário do pedido | Como se valida | Automatizável? | Executado hoje |
|---|---|---|---|---|
| 1 | Máquina sem Garra: instala, configura, conecta o WhatsApp e recebe resposta, sem terminal | VM limpa por SO, instalador da release, telefone real, número autorizado responde "oi" | Parcial (container Linux + fixture cobre até "conectado"; a resposta real exige telefone) | ❌ |
| 2 | Garra já instalado: reaproveita instalação e configurações | `install.sh` antes do app; `config.yml` com chave; `session.enc` existente → S0 → S6 sem QR | Sim (tabela do `runtime` + integração com gateway falso) | ❌ |
| 3 | Atualização disponível: aceitar ou adiar, versão compatível | `update-check.json` forjado com versão maior; "Mais tarde" mantém tudo; "Atualizar agora" verifica `.sha256` | Sim para a decisão; manual para o instalador | ❌ |
| 4 | Gateway desligado: o app inicia e segue | Nada na porta → sidecar sobe → `/api/health` | Sim (core) + manual | ❌ |
| 5 | Gateway já ativo: reutiliza sem duplicar | `garraia start` no terminal antes do app → app adota, não lança, não encerra ao sair | Sim (core) + manual | ❌ |
| 6 | Sessão válida: reconecta sem novo QR | Reabrir o app → `session_found → connected` | Fixture (`serve-echo`) + manual | ❌ |
| 7 | Falha de conexão/pareamento: explica e recupera | Fixture `crash-after-qr`, `network-flap`, `logged-out`; Wi-Fi desligado no meio do QR | Sim (fixture) + manual | ❌ |
| 8 | Cancelamento de instalação/atualização: ambiente utilizável, dados preservados | Cancelar o MSI/NSIS/DMG no meio; cancelar download; comparar `config.yml`/`session.enc` antes e depois | Manual (instalador) + sim (download) | ❌ |

A coluna "Executado hoje" é honesta: **nenhum** dos oito foi exercitado
ponta a ponta num app gráfico, porque o app gráfico ainda não tem o fluxo. O
que já foi exercitado contra o WhatsApp real está registrado em
`docs/whatsapp.md` §"O que JÁ foi exercitado contra o Baileys real
(2026-09-17)" e vale para a ponte, não para a GUI. "Compila nos três SOs"
não será declarado como compatibilidade: a coluna só vira ✅ com data e SO.

---

## 7. Segurança (sem prejudicar a experiência)

- **Privilégio mínimo.** Nada no fluxo exige elevação: MSI por máquina pede
  UAC uma vez (instalador), NSIS/AppImage/`.dmg` não pedem nada; dados em
  diretório do usuário; gateway em loopback; o app nunca abre a porta para a
  rede (o boot recusa bind exposto sem credencial de qualquer forma).
- **Integridade.** Bundles e Node verificados por SHA-256 no build; a
  atualização v1 confere o `.sha256` irmão; v2 assina com a chave do updater.
- **Segredos.** Chave de provedor entra por stdin, vai para `config.yml`
  0600 (Windows: sem ACL restrita — limitação pré-existente, #1253, a
  registrar na tela de Diagnóstico e não a esconder); sessão do WhatsApp
  cifrada, chave em `session.key` 0600 por padrão. **Follow-up R4 (não v1):**
  passphrase gerada pelo app e guardada no cofre do SO (Credential Manager /
  Keychain / Secret Service) e injetada no sidecar como
  `GARRAIA_VAULT_PASSPHRASE` — protege backup/disco roubado, mas faz a CLI
  fora do app deixar de abrir a sessão; é o follow-up 3 do ADR 0023 e a
  #1431, e merece decisão própria.
- **Controles existentes preservados.** Portão fail-closed (`allow`/`owners`),
  piso `search` (só leitura) em `standard`, `owners` com poder só em
  `isolated-pod` e 1:1, guard de injeção — o app **nunca** afrouxa nada para
  "fazer funcionar"; a única mudança de política é o que o usuário digita em
  S4, com o mesmo validador da CLI.
- **Superfície nova.** Rotas de §4.1: guarda anti-CSRF + bearer quando houver
  chave + loopback; QR só por SSE; contadores em vez de conteúdo; nenhum JID
  inteiro; consentimento como campo obrigatório e auditado
  (`audit_events`/log redigido).
- **Webview.** CSP fechada, sem `shell:allow-execute`, comandos tipados; a UI
  não recebe caminho, comando nem URL para executar.
- **Processos.** Nunca matar processo que o app não lançou; gateway adotado
  fica vivo; ponte morre com o gateway (stdin, PDEATHSIG, `Drop`).

---

## 8. Riscos e bloqueios

| # | Risco / bloqueio | Impacto | Mitigação | Residual |
|---|---|---|---|---|
| 1 | **Meta bloquear a conta** (cliente não oficial) | usuário perde o número | consentimento em S2, recomendação de número secundário, rate limit (E1) | **alto e aceito no ADR 0023** |
| 2 | **Baileys quebrar** com mudança da Meta | canal para até um bump de versão | pin exato + versão em Diagnóstico + atualização do app | médio, inerente |
| 3 | **Assinatura/notarização** ausentes | SmartScreen no Windows; Gatekeeper no macOS pode **matar o Node embutido** | R5 do dono (§6.2); v1 macOS não assinada com instrução documentada | bloqueia "experiência de produto" no macOS até o dono agir |
| 4 | **Tamanho do instalador** (+45–60 MB estimados) | download mais lento; disco | medir; remover `sharp`; Node sem `npm` | baixo |
| 5 | **Rotas de pareamento locais auth-free** (sem `api_key`) | outro processo do mesmo usuário pode disparar `pair`/`logout` | bearer quando houver chave; loopback; consentimento no corpo; auditoria R4 | médio (mesma classe do resto de `/api/*`) |
| 6 | **Segredos sem ACL no Windows** (#1253) | outro processo do usuário lê `config.yml`/`session.key` | documentar; follow-up cofre do SO | médio, pré-existente |
| 7 | **Gateway externo incompatível / porta 3888** | onboarding trava | §2.6 (adotar por versão, porta alternativa, nunca matar) | baixo |
| 8 | **Casca Tauri fora dos gates** | regressão silenciosa | lógica no core; clippy/test da casca no `desktop.yml`; checklist manual | médio |
| 9 | **Linux: Wayland/tray/GNOME** | ícone de bandeja ausente, overlay sem sempre-no-topo | o onboarding não depende do overlay; documentar a extensão AppIndicator | baixo |
| 10 | **WebView2 ausente e offline** no Windows | instalador falha | `embedBootstrapper` ou documentar | baixo |
| 11 | **`from_me` ignorado**: o dono não fala com o Garra pelo celular vinculado | frustração na primeira mensagem | S2 e S4 dizem isso em uma frase; recomendar número secundário para o Garra | baixo (é regra de produto) |
| 12 | **70 issues abertas do WhatsApp** (P0 #1449 workspace compartilhado, #1387 honestidade do runtime, #1378 NoRoots) | "pronto para responder" pode ser verdade para conversa e mentira para arquivos | S5 mede **conversa** (turno de texto); capacidades de arquivo ficam fora do "pronto" até essas issues fecharem | médio |
| 13 | **macOS nunca exercitado** neste repositório | surpresas de WKWebView, LaunchAgent, `_NSGetExecutablePath` (já tratado no `update.rs`) | máquina de teste do dono; jobs best-effort primeiro | médio |
| 14 | **Orçamento de agentes** (memória da sessão: sem Opus 5 via OpenRouter) | E1 exige `security-auditor` | rodar com o modelo da sessão / revisão adversarial por workflow, como nas v0.4.3–0.4.5 | baixo |

---

## 9. Decisões que ficam com o dono (R5)

1. **Node embutido no bundle do desktop** (recomendado) × baixado na
   configuração × sidecar whatsmeow (Go) só no desktop. Toca ADR 0023
   §Follow-ups 1 e o `release.yml`.
2. **O gateway passa a parear** (rotas de §4.1) — amendment do ADR 0021/0023.
   Alternativa mais barata e pior: `garraia whatsapp link --json` dirigido
   pela casca, mantendo o `restart` e o risco de duas pontes.
3. **Assinatura Windows e Apple** (certificado, conta, notarização, assinar o
   Node) e **chave do updater** — sem isso a v1 sai com SmartScreen/Gatekeeper
   e sem auto-update.
4. **Política "o app atualiza como unidade"** (recomendada) e o piso de
   versão do macOS.
5. **Postura das rotas de pareamento sem `gateway.api_key`** (loopback + guarda
   anti-CSRF, como o resto de `/api/*`, ou exigir chave gerada pelo app).
6. **Passphrase no cofre do SO** (follow-up, não v1).

---

## 10. Fora de escopo da v1 (de propósito)

Abas genéricas do Control Center (M2–M6 continuam no ROADMAP §4.1, agora
**depois** do onboarding), aba Agents/AgentDeck, framework de UI (gatilho S1
do ADR 0007, decidido no início do M2), Windows/Linux ARM64 do desktop,
WhatsApp Business (Cloud API) pela GUI, múltiplas contas de WhatsApp (o
store já é parametrizado por conta, a UI não), mídia (a ponte v1 não envia
nem baixa), migração da CLI `link` para delegar ao gateway, cofre do SO.
Nada disso contribui para "instalar, conectar e conversar", e o pedido é
explícito em não ampliar o escopo.
