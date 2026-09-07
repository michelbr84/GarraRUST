# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

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
  (`health.rs::feature_flags`, aditivo, com teste que trava o contrato); a
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
