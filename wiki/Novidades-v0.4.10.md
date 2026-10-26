# Novidades da v0.4.10

> 🇬🇧 [English version](Whats-New-v0.4.10) · 📋 [CHANGELOG completo](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md) · 📦 [Baixar](https://github.com/michelbr84/GarraRUST/releases/tag/v0.4.10)

Release estável de 25 de outubro de 2026 — substitui a v0.4.9 como release
`latest`. Validada como rc1 (23/10) e rc2 (24/10); as correções achadas na
validação do rc1 estão aqui.

A versão é de três frentes. A primeira é o **Garra Desktop** saindo da casca:
o app ganha um **Assistente de Primeiro Uso** — wizard de cinco telas em pt-BR
que instala, pareia o WhatsApp lendo um QR e coloca o usuário conversando sem
abrir terminal nenhuma vez — e os instaladores passam a carregar um runtime
Node privado (baixado com verificação GPG + SHA-256), com smoke de instalador
nas três plataformas rodando no CI.

A segunda é o **produto aprendendo a se explicar**: o binário agora carrega um
catálogo versionado de funcionalidades (tool `garra_capabilities`), o gateway
ganha diagnóstico nativo somente leitura (`garra_diagnostics`) e o projeto
publica um **site de documentação Docusaurus** no GitHub Pages, gerado do
markdown que já existe no repositório.

A terceira aponta para o **Garra Cloud**: entra a fundação do schema de
billing (planos, assinaturas e faturas por tenant, ADR 0026 — sem preço
definido neste repositório MIT) e o gate de dogfood D1/D3a/D9 vira job de CI
(`dogfood-linux.yml`).

Fechando: banda larga de correções — instaladores que não caem mais num
prerelease, voz com o Chatterbox oficial, desktop sem processos órfãos, Docker
com os forks de `third_party/`, diagnóstico honesto numa instalação nova — e o
lote de segurança: revoke dos privilégios que a migration 007 vazava,
`wasmtime` 49.0.4 (RUSTSEC crítica) e `jsonwebtoken` trocando o backend para
`aws_lc_rs`, o que tira vinte crates da cadeia RustCrypto do lockfile.

---

## O desktop instala, pareia e conversa sem terminal

- **Assistente de Primeiro Uso (#1636, fatia 3).** Na primeira execução, o
  Garra Desktop abre um wizard de cinco telas em pt-BR — Boas-vindas → Modo →
  QR → Personalidade → Pronto. O QR é rasterizado num canvas a partir da
  matriz NDJSON que o sidecar `garraia whatsapp link --machine` (fatia 1)
  emite — o payload cru (credencial) nunca chega à webview. A escolha
  "próprio" vs "dedicado" persiste em `onboarding.json` e é configurável
  depois em Ajustes → WhatsApp.
- **Mapeamento de modo e persona para a config (#1636).** "Meu próprio
  número" aplica `whatsapp access open --yes`, só com confirmação explícita
  do usuário; "Número dedicado" aplica `whatsapp access restricted` **e**
  `whatsapp allow <número pessoal>` (o número pessoal precisa estar
  autorizado, senão o portão ficaria com 0 autorizados e descartaria o
  próprio dono em silêncio). Nenhum default de segurança é afrouxado em
  silêncio; a persona vira `agent.system_prompt` e o wizard só grava
  `concluido` — e reinicia o gateway — depois de todos os passos saírem com
  exit 0.
- **Endurecimento da webview (#1636).** CSP restritiva no `tauri.conf.json` e
  todo o JS do desktop por arquivo externo, sem script inline; o wizard tem
  capability própria e mínima, fora da `default` ampla.
- **Tela "Pronto" honesta (#1700).** Sem provedor de LLM no config, o
  assistente avisa antes do primeiro "oi" sem resposta, com o caminho do
  `garraia init` (e o caminho no Windows) — a chave nunca atravessa a webview.
  A escolha do provedor dentro do assistente fica para a 0.5.0.
- **Runtime Node.js privado empacotado nos instaladores (#1636, fatia C).**
  MSI/NSIS no Windows, `.deb`/AppImage no Linux e DMG/`.app` no macOS trazem
  o Node v24.22.1 LTS e a ponte pré-construída do WhatsApp — nada de
  instalar Node/npm ou baixar dependência na primeira execução.
  `docs/desktop.md` documenta o layout por formato, a desinstalação e o que
  fica (`~/.garraia/whatsapp/` preservado).
- **Smoke de instalador em ambiente limpo no CI (#1636).** Jobs por
  plataforma extraem a árvore num caminho com espaço, escondem todo Node
  global do `PATH` e exigem `node_origin == privado` + `self_test == ok` do
  sidecar instalado; jobs irmãos cobrem a inicialização pelo app (`/api/health`
  em ≤15s), a atualização (v0.4.9 → novo, sessão sobrevive) e a desinstalação
  (dado do usuário preservado).

## O Node privado da ponte, verificado de ponta a ponta

- **Construção verificada (#1636, fatia B).** `build-node-runtime.sh` baixa o
  artefato OFICIAL do Node, prova a procedência por GPG contra as chaves do
  release team vendorizadas no repo (allowlist de fingerprints, GNUPGHOME
  isolado) e a integridade por SHA-256, monta a ponte com o npm do próprio
  tarball e gera `THIRD_PARTY_NOTICES.txt` + `manifest.json`. O workflow
  `node-runtime.yml` prova também a recusa de artefato ou SHASUMS adulterado.
- **Node por caminho explícito (#1636).** A ordem é `GARRAIA_NODE_BIN` → Node
  privado ao lado do executável → `PATH`, sempre por caminho absoluto, sem
  tocar no `PATH` do processo nem substituir instalações existentes; CLI e
  gateway usam o mesmo resolver e registram a origem (`env`/`privado`/`path`).
- **`garra whatsapp self-test [--json]` (#1636).** Importa
  Baileys/pino/sharp sem socket nem pareamento e reporta a origem do Node, o
  caminho e as versões — é o smoke que os instaladores rodam sem Node global.
- **Pareamento em NDJSON: `garra whatsapp link --machine` (#1636).** Uma
  linha por evento (`status`, `qr`, `error`, `log`), QR como matriz de `0`/`1`
  pelo mesmo motor do terminal, payload cru nunca impresso, erros e exit
  codes iguais aos do `link` interativo. Feito para o app desktop consumir
  como sidecar (ver `docs/whatsapp.md`, seção "Modo máquina").
- **Detalhes de robustez.** `npm` só é exigido quando há dependências a
  instalar; a árvore pré-construída da ponte é semeada quando bate com o
  `package-lock.json` embutido no binário; e a máquina de estados do
  onboarding vive em `garraia-desktop-core` — lógica pura, testada no CI,
  sem Tauri.

## O produto explica a si mesmo

- **Catálogo versionado de funcionalidades embutido no binário (#1670).** O
  Garra responde o que sabe fazer a partir do manifesto
  `crates/garraia-gateway/assets/catalogo-funcionalidades.json`, embutido via
  `include_str!` — funciona até em instalação limpa, sem clone do
  repositório. Cada recurso traz descrição, pré-requisitos, limitações e
  estado (`disponivel`/`configuravel`/`opcional`), com marcador de
  feature/config para o que depende de compilação ou interruptor. Tool nova
  `garra_capabilities` (filtros `name`/`category`; sem filtro, só contagens)
  e seção `catalog` no `garra_status`. Em turno restrito, devolve só o que a
  permissão permite.
- **Diagnóstico nativo e seguro (`garra_diagnostics`, #1669).** Relatório
  estruturado e somente leitura da própria instalação — versão, gateway,
  canais, servidores MCP, ferramentas, config efetiva validada, presença e
  ORIGEM de credenciais (vault / config / env, nunca valores), amostra
  redigida do log — com vocabulário fechado de estados
  (`ok`/`erro`/`nao_configurado`/`bloqueado`/`degradado`/`nao_suportado`),
  file jail na leitura do log e acesso por sessão do operador + turno,
  fail-closed.
- **Site de documentação Docusaurus no GitHub Pages (#1639).** O conteúdo é o
  markdown que já existe no repositório (`README`, `docs/**`, `wiki/**`,
  `CHANGELOG`, ...), sincronizado por script Node puro — fonte única de
  verdade, nada reescrito à mão. Português como idioma padrão, inglês
  parcial, busca offline, tema claro/escuro e publicação a cada push na
  `main` (https://michelbr84.github.io/GarraRUST/). Não altera o `ci.yml` nem
  o site garraia.org.

## Rumo ao Garra Cloud

- **Fundação de schema de billing: plan/subscription/invoice (#1652, ADR
  0026).** `plans` é catálogo global semeado só com o plano `free` sem preço
  (preço não se fixa neste repositório MIT); `subscriptions` e `invoices` são
  por tenant sob FORCE RLS (fail-closed), dinheiro em centavos inteiros +
  ISO 4217, fatura como ledger imutável, ids de provedor de pagamento
  opcionais e UNIQUE, nunca PII de cartão. Todo grupo sem assinatura viva
  resolve para o Free, então um tenant novo nasce Free sem intervir em
  ninguém. Só o schema: webhook Stripe/Pix, entitlements, metering e a página
  de signup ficam para as próximas issues.
- **O gate de dogfood D1/D3a/D9 roda no CI.** O `dogfood-linux.yml`
  (`workflow_dispatch`) instala o Ollama pinado por versão + SHA-256
  verificado, sobe o servidor, puxa o modelo e roda
  `scripts/dogfood/linux-clean-install.sh --source local` contra o candidato;
  o job falha se o resumo disser FALHOU. As linhas que precisam de dois
  telefones reais seguem manuais por desenho.

## Cadência e confiabilidade do WhatsApp

- **Guarda de cadência entre turnos por chat (#1650 + #1658).** Passando de
  20 turnos admitidos em 60s no mesmo chat (teto configurável,
  `channels.whatsapp_linked.max_turnos_per_minute`), o turno é cortado com
  log `possivel loop entre agentes`; o turno cortado não conta na janela, o
  chat volta sozinho quando o outro lado cala. Backoff crescente por rajada
  (60s → 120s → 240s, teto 8min, zera após 2min de silêncio) e
  `turnos_ultima_hora` no `garra_diagnostics`.

## Correções que valem release

- **Instaladores não caem mais num prerelease pelo caminho não pinado.** O
  fallback de `install.sh`/`install.ps1` pula `prerelease: true` (além de
  `draft: true`) e escolhe a estável mais recente, classificando cada release
  por `tag_name`/`draft`/`prerelease`. Para instalar um RC de propósito, pine
  `GARRAIA_VERSION=vX.Y.Z-rcN`.
- **Voz com o Chatterbox oficial volta a funcionar.** O cliente descobre a
  assinatura do `generate_tts_audio` (o app oficial liga 7 entradas, não as 4
  que o cliente mandava — o Gradio 6.x recusava a chamada curta) e monta a
  chamada com os defaults do servidor, cacheando a assinatura; `event: error`
  do SSE vira erro com a mensagem do servidor, e `/api/tts` e o canal de voz
  do Telegram registram o motivo antes do fallback de texto.
- **O Docker volta a compilar com os forks de `third_party/` (#1694).** O
  `[patch.crates-io]` pós-v0.4.9 (`tauri-plugin-updater`,
  `utoipa-swagger-ui`) quebrou o `cargo chef` — o Deploy do rc1 não publicou
  imagem. O Dockerfile copia `third_party/`, um teste prende a regra em todo
  PR e o `docker-build.yml` constrói a imagem e responde `/ping` quando muda
  Dockerfile/Cargo/third_party.
- **`/api/diagnostics` honesto numa instalação nova (#1695, #1696, #1698).**
  Ausência de configuração vira `not_configured` (neutro) em vez de
  `warning`; sem `npx` no `PATH` o boot não provisiona um MCP `filesystem`
  que nunca subiria (e diz como ligar); e no Windows a comparação de caminhos
  passa pela mesma forma canônica, então o workspace não é mais acusado de
  estar "fora do data_dir". Configurado e quebrado continua amarelo.
- **Nenhum órfão quando o pai morre de forma abrupta (#1636).** No Windows,
  ponte e sidecar entram num Job Object `KILL_ON_JOB_CLOSE`; no Unix, grupo
  de processos próprio + `kill(-pgid)` e `garra start --exit-with-parent`
  vigiando o PID do app (prova de reuso de PID por `getppid()`); em todas as
  plataformas, EOF no stdin encerra. Testes de orfão em
  ubuntu/windows/macos no CI, inclusive com `TerminateProcess`.
- **Banda de correções pontuais.** O boot sonda o Whisper antes de declarar
  o STT pronto (`STT ready` só com o servidor respondendo); o dashboard do
  Web Console mostra o bind real em vez de "0.0.0.0" fixo; o seletor de
  providers mostra as instâncias reais (`llm.<chave>`), não só tipos fixos;
  `garraia doctor` avisa quando um binário mais velho mascara o novo no
  `PATH`; o log de reconexão do WhatsApp informa `reason`/`reason_code`;
  `find_executable` exige bit de execução no Unix; o caminho não-streaming
  não publica mais `[no textual response provided by the model]`; e o
  `@modelcontextprotocol/server-filesystem` autoprovisionado fica fixado em
  `@2.0.0` (o upstream deprecou a série por data).

## Segurança

- **Revoke dos privilégios que a migration 007 vazava (migration 036,
  forward-only).** A ALTER DEFAULT PRIVILEGES da 007 dava CRUD completo a
  toda tabela criada depois dela; nove tabelas carregavam UPDATE/DELETE em
  excesso no role `garraia_app`. O revoke é seletivo — SELECT/INSERT e o
  privilégio de mutação legítimo de cada tabela permanecem — e o apagamento
  LGPD cross-tenant continua em funções `SECURITY DEFINER` dedicadas, nunca
  pelo pool do app. O caso de peso é `account_deletion_requests`: sem o
  revoke, um handler podia marcar pedido como concluído sem o purge.
- **`jsonwebtoken` troca o backend criptográfico para `aws_lc_rs` (#1701).**
  A RUSTSEC-2026-0387 não tem fix em `rust_crypto` (feature monolítica) e o
  código só usa HS256/RS256 — a cadeia RustCrypto EC/RSA inteira (20 crates,
  incluindo `crypto-bigint` vulnerável, que só tem fix em 0.7.6) sai do
  lockfile sem nenhuma exceção no `deny.toml`/`audit.toml`. Trade-off
  documentado: `aws-lc-sys` compila C nativo (cmake + perl).
- **Família wasmtime 49.0.4** fecha a RUSTSEC-2026-0379 (crítica) e a
  RUSTSEC-2026-0378 no sandbox de plugins; `rustls-webpki` 0.103.16 e
  `webpki-roots`/`webpki-root-certs` 1.0.10 fecham as
  RUSTSEC-2026-0381/0382/0383; `ctutils` 0.4.5 fecha a RUSTSEC-2026-0385
  (geração de máscara e comparações de `u128` sem tempo constante) — entra
  por `digest` 0.11, usado por `argon2`/`blake2`/`hmac`.
- **Jail fail-closed das tools MCP de filesystem (#1482)** — uma tool do
  servidor filesystem que o confinamento não mapeia (tool nova ou renomeada
  de uma versão futura) é recusada em vez de repassada sem checar o caminho.
- **Jail e workdir por orquestrador no `garra_agent` do MCP HTTP (#1626).**
  Cada orquestrador passa a ter subárvore gravável própria sob
  `<data_dir>/workspace`, derivada da identidade dele por digest — a única
  raiz efetiva das file tools e o workdir do sandbox. Antes o jail era
  global: o que um orquestrador escrevia, o outro lia.
