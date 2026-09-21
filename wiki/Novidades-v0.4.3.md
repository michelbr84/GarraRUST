# Novidades da v0.4.3

> 🇬🇧 [English version](Whats-New-v0.4.3) · 📋 [CHANGELOG completo](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md) · 📦 [Baixar](https://github.com/michelbr84/GarraRUST/releases/tag/v0.4.3)

A release em que o Garra entrou no WhatsApp **pessoal**. Até aqui o canal
WhatsApp exigia conta Business, número cadastrado na Meta e URL pública —
barreira que o usuário doméstico não passa. Agora `garra whatsapp` mostra um
QR code no terminal, você lê com o celular, e o número que já está no seu bolso
passa a falar com o agente, com a sessão cifrada em disco e o gateway
supervisionando a ponte.

Junto veio a maior faixa de segurança de uma release até hoje: cinco fail-opens
do MCP fechados, um jail de diretório nas file tools, o guard de injeção
indireta estendido a MCP, arquivos e dispositivos, e o sandbox por tool
(`agent.sandbox`) que existia inteiro e nenhuma instalação conseguia ligar.
E, no runtime, um LLM padrão único (`z-ai/glm-5.3-flash` via OpenRouter), o
auto-router de modo em todas as portas de entrada, fallback local quando a rede
cai, e um `garra chat` que sobrevive a crash, timeout e Ctrl+C.

---

## WhatsApp pessoal por dispositivo vinculado

Antes: só a Meta Cloud API. Agora há dois caminhos, e eles não se misturam.

```text
garra whatsapp
  1) Conectar meu WhatsApp pessoal (ler um QR code)   →  garra whatsapp link
  2) WhatsApp Business pela Cloud API oficial da Meta  →  garra whatsapp cloud
```

| Comando | O que faz | Exit code |
|---|---|---|
| `garra whatsapp` | menu de duas opções | 0, ou 1 se cancelado |
| `garra whatsapp link` | vincula por QR | 0 · 1 cancelado · 69 sem Node, sem terminal ou QR não lido · 70 erro interno |
| `garra whatsapp cloud` | wizard da Cloud API | 0 · 1 cancelado · 69 sem terminal · 70 erro interno |
| `garra whatsapp status` | diz se há vínculo e se a sessão abre | 0 vinculado · 69 não vinculado ou ilegível |
| `garra whatsapp logout` | apaga a sessão e desliga o canal | 0 · 1 cancelado |
| `garra whatsapp restore` | devolve o `session.enc.prev` ao lugar | 0 · 69 nada arquivado ou sessão em uso · 70 erro interno |

Sem terminal (`ssh servidor 'garra whatsapp'`), o menu imprime as duas opções
com o comando de cada uma e sai 0, como o `garra init`; quem já escolheu
`link` recebe o motivo, a saída (`ssh -t …`) e exit 69 em vez de um falso
sucesso (#1238).

### Como funciona por dentro

```text
garra whatsapp link / gateway (serve)
     |  NDJSON v1 por stdio · ping/pong · env_clear + allowlist · PDEATHSIG · Drop mata o filho
bridge/whatsapp/bridge.mjs   Node + Baileys 7.0.0-rc14, STATELESS — nunca escreve no disco
     |
WhatsApp Web (dispositivo vinculado)
```

- **A ponte é burra de propósito.** O Node cuida só do protocolo do WhatsApp;
  o Rust desenha o QR, cifra e grava a sessão, aplica allowlist e roteia para o
  agente. O estado de autenticação vive em memória na ponte e volta como
  snapshot completo (`session_update`) para o Rust gravar (#1238, ADR 0023).
- **Sessão cifrada em repouso.** AES-256-GCM (a mesma pilha do
  `CredentialVault`) em `<data_dir>/whatsapp/default/session.enc`, arquivos
  `0600` num diretório `0700`, escrita atômica. A chave deriva de
  `GARRAIA_VAULT_PASSPHRASE` quando ela existe; sem ela fica num `session.key`
  local, e o `status` avisa disso em letras claras.
- **Nada destrutivo antes da última confirmação.** Há uma tela de
  consentimento antes do primeiro QR (cliente não oficial, a conta pode ser
  bloqueada, use um número secundário). Um re-vínculo que não chega a gravar
  sessão nova — QR expirado, Ctrl+C, ponte que morre, WhatsApp recusando —
  restaura a sessão anterior; `enabled = true` só vai para a config **depois**
  de a sessão existir em disco, então um pareamento abortado não deixa o
  gateway pagando timeout a cada boot.
- **Todo relógio tem teto.** Handshake, QR, "conectando", autenticado-mas-
  nunca-conecta, `shutdown` de cortesia (500 ms), `npm ci` (600 s, sem órfão):
  cada fase do pareamento diz quanto tempo falta e desiste com a mensagem
  certa. Um teste enumera todas as fases e não compila se alguém acrescentar
  uma sem dizer qual é o teto dela.

### O canal `whatsapp_linked` no gateway

A mensagem lida pelo QR chega ao agente por um canal **pull** próprio, e o
modelo de ameaça dele é outro: a mensagem vem de qualquer pessoa que conheça o
número pessoal do operador. Três decisões saem disso, nenhuma configurável para
menos (#1238):

1. **Allowlist própria, fail-closed.** Entra-se com código do `/pair` ou pela
   lista `allow` da config; portão vazio significa **ninguém**. A allowlist
   global não serve aqui porque todo canal irmão a preenche sozinho por
   auto-claim do primeiro remetente.
2. **Ferramentas somente-leitura por padrão.** Sessão sem modo escolhido
   resolve para o perfil `search` até o operador subir com `/mode`.
3. **Guard de injeção indireta** no texto recebido.

Grupo só responde com opt-in explícito, e mensagem própria (`from_me`) nunca
gera turno. O gateway supervisiona a ponte em `mode: "serve"`, com prova de
vida no protocolo: depois de conectar, `ping` a cada 15 s e resposta exigida em
45 s, senão cai no backoff de reconexão (#1283). Ctrl+C no gateway **cancela**
o supervisor — a ponte Node não fica viva depois do "shut down gracefully".
`garra whatsapp status`, `GET /api/channels` e o check `whatsapp.linked` do
`GET /api/diagnostics` classificam pela mesma função, com o próximo passo
acionável ("rode `garra whatsapp link`", "rode `npm ci` em <dir>").

### O que sai no seu terminal

A cauda do stderr do Node é a única saída crua de ferramenta externa no fluxo,
e ela passa por redação antes de aparecer: sequências longas que parecem
material cifrado (base64 padrão e url-safe, inclusive coladas a nomes de campo
como `creds.noiseKey=` ou `npm ERR! _auth=`) viram `<redigido: N caracteres>`,
medido sobre 2000 chaves com vazamento residual de ~1,3 caractere (#1238,
#1276). O nome da conta recusa `con`, `prn`, `aux`, `nul`, `com1`–`com9`,
`lpt1`–`lpt9`, que não podem virar diretório no Windows.

- A redação do stderr também cobre base64 **percent-encoded** e com a barra
  escapada como `\/`, formas que um `throw` de biblioteca ou um JSON serializado
  produzem (#1276).

**O que precisa de Node:** só o caminho do QR. `node` e `npm` são procurados
na `PATH` e a exigência é **Node.js 20 ou mais novo** (`engines` da ponte:
`>=20`). A Cloud API não precisa de Node. A ponte vive em `bridge/whatsapp/`
— diretório, não crate — e os testes Rust rodam contra uma ponte falsa em
Python, sem Node e sem telefone; o pareamento com aparelho real é validação
manual, descrita passo a passo na doc.

📖 [`docs/whatsapp.md`](https://github.com/michelbr84/GarraRUST/blob/main/docs/whatsapp.md) · [ADR 0023](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0023-whatsapp-dispositivo-vinculado.md)

---

## Segurança

### Sandbox por tool: `agent.sandbox`

A contenção da #1222 existia inteira — `SandboxPolicy`, backends, fail-closed
quando o backend falta, onze testes — e **nenhuma instalação conseguia
ligá-la**: os três construtores do `BashTool` fixavam `off`, e a chave de
config que as mensagens de erro citavam não existia no schema. Agora há a
seção, lida pela mesma função nos três pontos de produção (gateway, `garra
chat`, `garra mcp-agent`), e o `garra config check` recusa sandbox que parece
ligado e não está (#1225).

| Chave | Valores | Nota |
|---|---|---|
| `mode` | `off` (default) · `all` · `allowlist` | seção ausente = `off`, byte a byte o comportamento anterior |
| `backend` | `docker` · `podman` · `ssh` | `mode != off` sem backend é **erro** |
| `image` | imagem do container | default `debian:bookworm-slim` |
| `ssh_host` | host remoto | obrigatório em `ssh` |
| `sandboxed_tools` / `elevated` | listas de tools | `elevated` escapa e roda **no host** |
| `mount_workdir` / `network_disabled` | bool, default `true` | **ignorados** por `ssh` |

O que cada backend garante — e não garante — está na tabela do threat model
§5.13. Três limites valem para todos: **hoje só a tool `bash` é envolvida**
(`run_tests`, `git_diff`, `code_review` e `repo_search` seguem no host mesmo
com `mode: all`); o backend `ssh` **não é sandbox**, é execução remota que
isola o host local e nada mais; e na prática isto é Unix — no Windows o
`wrap_command` falha fechado e o `config check` reporta erro.

Fecharam junto dois furos da primeira versão: metacaractere de shell escapava
para o host porque o comando era citado com `{:?}` em vez de quoting POSIX
(#1231), e um valor de config começando com `-` (`ssh_host: "-oProxyCommand=…"`)
virava **opção** do `ssh`/`docker` em vez de argumento — recusado em três
camadas, sem nunca logar o valor (#1225). O comando sandboxado passa por
`redact_secrets` e truncagem antes de ir ao log, em `debug!`.

- `backend: ssh` com `network_disabled` ou `mount_workdir` ligados passa a ser
  **recusado fail-closed**: como o ssh ignora os dois, a config tem de dizer
  `false` explicitamente para o operador reconhecer que não há contenção
  (#1225 S3).
- `mode: all` avisa na subida — uma vez por ponto de entrada: gateway, `garra
  chat` e `garra mcp-server` — que cobre só `bash`; `run_tests`, `git_diff`,
  `code_review` e `repo_search` são host-only, e o `config check` aponta quando a
  config lista uma delas em `sandboxed_tools`/`elevated` (#1225 S2).
- Teste de integração com Docker real prova o `--network none` no CI Linux
  (#1225 S4).

📖 [`docs/security/threat-model.md` §5.13](https://github.com/michelbr84/GarraRUST/blob/main/docs/security/threat-model.md) · [`config.hardened.example.yml`](https://github.com/michelbr84/GarraRUST/blob/main/config.hardened.example.yml)

### MCP: cinco fail-opens fechados

- **Reiniciar um servidor pela admin API não apaga mais a allowlist de tools**
  (#1242). O restart reconectava com lista vazia — que significa "permite
  tudo" — depois de o `disconnect` ter descartado a única cópia. Agora a
  allowlist é resolvida **antes** do teardown, contra o mesmo merge
  (`mcp.json` + `config.yml`) que o boot lê, e o restart nunca alarga o que
  está em vigor.
- **`DELETE /admin/api/mcp/{id}` revoga de verdade** (#1262): derruba a
  conexão viva, limpa o `pending` (de onde o monitor de saúde ressuscitava o
  servidor deletado com os segredos recém-revogados) e solta as tools do
  runtime.
- **A whitelist do modo do agente passa a valer para tool MCP** (#1264).
  `ToolGate` isentava qualquer nome com `__`; modo somente-leitura deixava
  passar ferramenta de escrita de servidor MCP. A permissão agora é declarada:
  `meu-servidor/*` libera o servidor, nome completo libera só a tool,
  `denied` vence tudo.
- **Entrada HTTP do `mcp.json` não é mais descartada pelo loader** (#1274) —
  e com ela não se perde mais a `allowed_tools` que o operador escreveu.
- **Uma escrita de admin não apaga mais `allowed_tools`, `inherit_env` e
  `enabled` dos outros servidores do `mcp.json`** (#1273): os dois tipos com o
  mesmo nome passaram a concordar num schema só.

E na fronteira do processo filho:

- **Servidor MCP stdio deixa de herdar o ambiente inteiro do gateway**
  (#1075). Todo `npx -y algum-server` recebia `GARRAIA_JWT_SECRET`, as chaves
  de provider e o `.env` inteiro sem precisar de tool call nenhuma. Agora o
  ambiente nasce de uma allowlist mínima (`PATH`, `HOME`, locale, `TMPDIR`,
  CA bundles) mais o mapa `env` daquele servidor; `inherit_env: true` é a
  válvula de escape por servidor, com `warn!` e indisponível pela admin API.
- **`vault:` no `env` de servidor MCP passa a ser resolvido no boot**, e
  fail-closed (#1237): referência que não resolve impede o servidor de subir,
  nomeando servidor e chave, nunca o valor. Antes a string literal `vault:…`
  chegava ao filho como se fosse o segredo.
- **O resultado de tool MCP entra no contexto pelo guard de injeção indireta,
  com teto de 256 KiB** e truncamento visível (#1243).
- **`POST /api/mcp/marketplace/install` exige admin autenticado** com
  `Permission::ManagePlugins` e recusa `env` que decide *qual* código o filho
  executa: `PATH`, `LD_PRELOAD`, `NODE_OPTIONS`, `npm_config_*` (#1245).

### O gate de `api_key` e o bind exposto

- **`gateway.api_key` passa a cobrir o plano de conversa e o A2A** (#1240):
  `POST /v1/chat/completions`, `/v1/messages`, `/v1/messages/count_tokens` e
  `/a2a/*` estavam fora do gate, que testava só o prefixo `/api/`. O socket do
  papagaio (`/ws/parrot`) entrou junto, e o Garra Desktop passou a de fato
  mandar a credencial. **Sem `gateway.api_key` configurada nada muda.**
- **`garra init` numa VM como root não deixa mais o gateway na internet sem
  credencial** (#1241): bind fora de loopback gera 32 bytes do CSPRNG em
  `gateway.api_key`, o `config.yml` nasce `0600`, e a chave **não** é impressa
  no terminal. O boot avisa uma vez quando o endereço ligado alcança a rede e
  não há credencial — inclusive em `garra start -d`, antes do fork.
- **Página web qualquer não dispara mais POST/PATCH/DELETE contra o gateway
  local** (#1182): guarda anti-CSRF genérica em toda a superfície mutante,
  `Origin` checado no handshake de `/ws` e `/ws/parrot`, e **CORS default
  fechado** — `gateway.allowed_origins` vazio agora significa nenhuma origem
  cross-origin (ver "Atualizando").
- **`/pair` volta a funcionar em todos os canais** (#1189) — cada canal montava
  um `PairingManager` próprio, sempre vazio — e o código de pareamento ganha
  limite de tentativas (5 por usuário / 15 min, 20 globais queimam o código),
  comparação constant-time e aviso ao dono quando algo foi queimado (#1191).

### As tools do agente

- **Jail de diretório nas file tools** (#1244). `file_read`, `file_write` e
  `list_dir` aceitavam `~` e caminho absoluto sem confinamento: um prompt por
  Telegram mandava ler `~/.ssh/id_rsa`. Agora `FileJail` confina à união de
  `agent.file_roots` (+ `GARRAIA_FILE_ROOTS`) com o `working_dir` da sessão;
  conjunto vazio **nega tudo**. Canonicaliza antes de comparar (`..`,
  symlink, `~`, absoluto) e recusa symlink pendurado; a recusa é uma frase só,
  sem caminho, para não virar oráculo de existência. Residuais declarados:
  TOCTOU e hardlink.
- **Guard de injeção indireta em `file_read` e nas tools de hardware**
  (#1243): conteúdo de arquivo e o que vem do barramento (id de dispositivo,
  `state` do Home Assistant, payload MQTT/serial) chegam ao modelo emoldurados
  como dado não-confiável quando suspeitos; texto limpo segue byte a byte.
- **A query do modelo não cai mais em posição de flag**: `repo_search` monta
  `rg`/`grep`/`findstr` com a query depois do `--` — `--pre=/bin/sh` fazia o
  próprio ripgrep executar um `.txt` (#1266); `git_diff` põe o `file_path`
  depois do `--` e recusa revisão começando com `-` — `--ext-diff` reabria
  execução externa via `.git/config` plantado (#1269). Os filhos de
  `run_tests` e `bash` deixam de herdar o stdin do gateway (#1270); inventário
  completo das 14 chamadas de `Command` no threat model.
- **`POST /api/providers` conecta com cliente HTTP pinado aos IPs validados e
  redirects desligados** (#1248), fechando redirect laundering para
  `169.254.169.254` e DNS rebinding; os construtores default dos providers
  também deixam de seguir redirect.

Também: `rustls` 0.23.45 fecha RUSTSEC-2026-0285 (#1206); `deny.toml` perde um
`ignore` morto pós-poise 0.7 (#1202); `ConfigLoader::save` virou atômica
(tmp `0600` + `rename` + fsync) e um `config.yml` vazio deixa de virar
defaults em silêncio (#1238).

---

## Agentes e runtime

### Um LLM padrão só

`z-ai/glm-5.3-flash` via OpenRouter passa a ser o padrão oficial em todas as
superfícies (#1180, ADR 0022). A pergunta "qual LLM o Garra usa quando o
usuário não escolhe?" tinha cinco respostas conforme a porta: `openrouter/auto`
no chat e no wizard, `openrouter/free` no `mcp-server`, `openai/gpt-4o` no
gateway, `lmstudio` no desktop. Agora há uma constante única, travada por
teste. O local (Ollama) continua inteiro, como **segunda** opção em
`agent.fallback_providers`; o autodetect do `garra chat` tenta nuvem com
credencial antes do Ollama; e o wizard passa a destacar "Cloud-first" sem
pré-selecionar os ~18 GB de download local. **Quem fixou
`GARRAIA_MCP_MODEL_ALLOWLIST=openrouter/free` precisa ajustar** (ver
"Atualizando").

### O turno que não morre

- **Rede caída aciona o fallback local** (#1249). `connection refused`, DNS e
  timeout não casavam com nenhum padrão de retry e matavam o turno antes do
  laço de fallback. Agora `Error::Transport` é tipado: gasta **uma** tentativa
  e cai direto no provider seguinte, sem queimar ~3,5 s de backoff.
- **Stream que quebra no meio refaz o turno** pelo caminho batch, quando nada
  foi ao sink ainda (#1176).
- **404 `No allowed providers are available` do OpenRouter** vira erro de
  configuração de roteamento com modelo, restrição efetiva e caminho de
  recuperação, e nunca retry (#1299).
- **Parâmetro ausente numa tool vira observação soft**, com o schema e pedido
  de reenvio — o modelo se autocorrige no mesmo turno em vez de o turno falhar
  com `agent error:` (#1296).
- **O auto-router de modo vale em todos os pontos de entrada** (#1223):
  `garra chat`, `POST /api/chat`, webchat, app e canais. Só age quando
  `agent.auto_router_llm_enabled` está ligada (default **off**), a sessão não
  tem modo e há texto; modo deduzido **não** concede tool nenhuma.

- **O erro do loop detector diz o que repetiu**: a tool, a contagem dentro da
  janela e o input repetido, truncado (#1295).

### `garra chat`

- **`--resume` sem valor (ou `latest`) retoma a sessão mais recente**, e a
  pergunta é gravada **antes** do turno rodar, com marcador
  `[turno interrompido: timeout|cancelado|erro]` quando ele acaba sem resposta
  — crash, timeout e Ctrl+C não apagam mais o turno. Novo `/resume [id]` no
  REPL (#1300).
- **`/model` virou transacional** (#1298): valida contra o catálogo real do
  provider (OpenRouter: `GET /models` completo) antes de trocar; modelo
  ausente ou provider sem resposta mantém o estado anterior.
- **O REPL não exibe mais tracing cru** — retry, fallback e circuit breaker
  ficam no `garraia.log`; `--verbose`, `--debug` e `RUST_LOG` vencem (#1301).
- **O contexto de projeto para de gastar prompt com `target/`** e passa a
  dizer qual projeto é (do `README.md`) e em que ramo (#1219).
- **`memory.auto_extract` e `memory.max_facts`** finalmente existem: desligue
  só a chamada de extração de fatos por turno sem desligar a memória
  semântica (#1221).

- **Editor de linha com rustyline**: setas e edição na linha; histórico em disco
  **só com `--persist`/`--resume`** (`0600` em `~/.garraia/history`; sem as
  flags fica em memória), Ctrl+D encerra; pipe e CI continuam no `read_line`
  (#1297).

### Runtime: ledger de runs e despacho único

- **Runs de sub-agente deixam rastro** na tabela `agent_runs` (#1224), e o
  ledger tem o primeiro escritor de produção (#1227): a subida do gateway e do
  CLI converte runs `running` deixados por uma queda em `interrupted` (ids no
  log, nunca o `goal`), e cada execução do scheduler grava uma linha com
  desfecho terminal. Falha do ledger é fail-soft.
- **As quatro cópias do loop de turno despacham tools por uma função só**
  (`dispatch_tool_call`): orçamento, detecção de loop, gate do modo, timeout
  e pausa de confirmação vivem num lugar (#1226).
- **`git_diff` e `code_review` respondem sobre o repositório certo**: o git
  roda no `working_dir` da sessão, e sem ele a resposta diz de qual
  repositório falou (#1258).
- **`ToolRegistry::execute_program`** — um programa JSON de N passos num turno
  só, substituição de variáveis tudo-ou-nada, `max_steps` 16 (#1224). Ainda
  sem chamador de produção.

- O scheduler **reivindica tarefas com lease** (`pending → running`), e o
  re-poll após queda é explícito e logado (#1227 S2).
- `DbRunLedger` aceita o `Mutex` tokio do gateway; `SubAgentConfig.session_id`
  (#1227 S3).

---

## Hardware, desktop, mobile e storage

- **O catálogo de hardware skills passa a valer em runtime** (#1250). A regra
  "risco efetivo = max(adapter, skill)" existia e nenhum deploy a exercitava.
  Agora o boot carrega o catálogo antes de subir qualquer adapter, cada device
  entra embrulhado num decorator que só **sobe** risco, e `device_list` mostra
  os `aliases` pt/en dos presets — "luz da sala" liga ao id que o gate vê.
- **ADR 0021 (Desktop Control Center) aceito**, e os dois primeiros milestones
  entregues (#1181): a crate `garraia-desktop-core` — o núcleo **sem Tauri**
  que entra nos gates de CI (`state`, `detect`, `supervise`, `locate`) — e
  `garra desktop [--status] [--no-launch]`, que localiza e lança o app
  instalado (exit 0/69/70) sem que a CLI ganhe dependência de Tauri. A
  resolução **nunca** devolve o próprio executável, que no `.deb` é irmão do
  app.
- **Garra Mobile em pt-BR e inglês** com seletor de idioma em Settings (255
  chaves, `flutter gen_l10n`, teste que falha em string hard-coded nova)
  (#1178), e o **ícone de launcher** deixa o padrão Flutter pelo WolfMark
  (#1177).
- **Multipart upload S3 acima de 16 MiB** (#1214): partes de 8 MiB, pico de
  memória de 5 GiB para 8 MiB por upload, qualquer falha aborta o multipart.
  **Checksum SHA-256 verificado por parte pelo servidor** (#1229), com
  `request_checksum_calculation` pinado para o deploy não rebaixar. E a suite
  MinIO volta a subir um container de verdade (`quay.io`, #1230).

---

## Operação e CI

- **Toda crate declara a MSRV** (`rust-version = "1.95"`, direta ou por
  herança do workspace), e o **formato dos fragmentos de `changelog.d/` virou
  gate de CI** (#1228).
- **O ledger CodeQL ancora por conteúdo, não por número de linha** (#1263):
  o checker deriva a linha do `sink_snippet`, e o gate só fica vermelho quando
  o statement mudou, sumiu ou ficou ambíguo. O `reapply` aceita statement
  multilinha (alerta 173).
- **Código `#[cfg(windows)]` sob a feature `mcp` compila no CI** num step
  Windows nativo (#1253), e a doc diz que o hardening de permissão de arquivo
  (`0600`/`0700`) é só Unix.
- **`garra config check` é comando opt-in, não gate de boot** — as prosas que
  afirmavam o contrário foram corrigidas (#1247), e a cláusula de idle do
  `validate_session_token` foi por bind, não `format!`.
- Docs: precedência real do bind (`--host` > `HOST` > default `127.0.0.1:3888`; as chaves
  `gateway.host`/`gateway.port` do arquivo **não** alimentam o `garra start`)
  (#1261); providers de TTS reais em `docs/voice.md` (#1246); testes de PDF e
  de magic bytes do `garraia-media` voltam a rodar (#1208, #1209).

- **Quality Ratchet reporta o delta contra o merge-base da PR** (sinal
  distinguível) e avisa quando o baseline tem mais de 90 dias; o baseline
  **não** foi re-congelado (#1254).

### Deprecated

- `ToolRegistry::execute_program` em `garraia-tools` marcada `deprecated`; o
  fragmento do #1224 foi corrigido (#1226 S-E).

---

## Atualizando

```bash
garra update          # instalações existentes
```

Ou reinstale: `curl -fsSL https://garraia.org/install.sh | sh`
(Windows: `irm https://garraia.org/install.ps1 | iex`).

**Nenhuma migração de dado é necessária.** Três coisas mudam de significado, e
uma seção é nova:

- **`agent.sandbox` é novo e default `off`** — seção ausente reproduz byte a
  byte o comportamento anterior.
- Quem já ligou `backend: ssh` tem de declarar `network_disabled: false` e
  `mount_workdir: false` explicitamente, senão a config é recusada (#1225 S3).
- **`gateway.allowed_origins` vazio passou a significar "nenhuma origem
  cross-origin"** (#1182). Quem alcança o Web Console por **nome DNS** —
  reverse proxy, `nas.local`, MagicDNS, ingress — precisa listar a origem
  (`https://garraia.seudominio.com`). IP, `localhost` e o perfil default não
  mudam.
- **`GARRAIA_MCP_MODEL_ALLOWLIST=openrouter/free` quebra** (#1180): o default
  novo é avaliado antes do allowlist. Inclua `z-ai/glm-5.3-flash` ou remova a
  variável.
- **O autodetect do `garra chat` tenta nuvem antes do Ollama** (#1180). Quem
  contava com o Ollama vencendo uma chave de nuvem exportada deve fixar
  `agent.default_provider: ollama`.

### Limitações conhecidas

- O sandbox envolve **só a tool `bash`**; `run_tests`, `git_diff`,
  `code_review` e `repo_search` seguem no host. `backend: ssh` não é sandbox.
  No Windows o sandbox falha fechado.
- A ponte WhatsApp exige **Node.js 20+ e `npm`** no host; a Cloud API não.
- A ponte **não tem teto de memória**: `RLIMIT_AS`, que o MCP aplica, derruba
  o V8 do Node já no boot (ele reserva vários GiB de memória virtual), então a
  contenção é ambiente limpo, PDEATHSIG e `Drop`.
- O WhatsApp pessoal é **uma conta só** (`default`).
- O pareamento com aparelho real é validação **manual** — o CI cobre
  protocolo, máquina de estados, store cifrado e ciclo de vida do filho contra
  uma ponte falsa.
- ARM64 e os instaladores por plataforma continuam **best-effort**, como na
  v0.4.2: uma release pode sair sem um deles.
