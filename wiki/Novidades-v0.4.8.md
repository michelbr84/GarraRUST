# Novidades da v0.4.8

> 🇬🇧 [English version](Whats-New-v0.4.8) · 📋 [CHANGELOG completo](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md) · 📦 [Baixar](https://github.com/michelbr84/GarraRUST/releases/tag/v0.4.8)

**Segurança no topo.** Quem está na v0.4.7 executa `bash` no host **sem**
fronteira de allowlist. A v0.4.8 estreia o modo `HostComAllowlist` (#1272): a
`agent.bash_allowlist` deixa de ser dispensa de confirmação e vira um limite
real. Junto, o `wasmtime` que alimenta o sandbox WASM sobe de 48.0.2 para
49.0.2 em dois passos (#1524, #1559) e fecha **onze advisories**.

A release também fecha o que a v0.4.7 tinha adiado: a página global **Agents &
Permissions** do Web Console (#1433) e o **import/export** da política de acesso
do WhatsApp (#1435). Em volta, religa os seis canais que o README já anunciava
(#1567), corrige o escape de aspas na allowlist do `bash` (#1579) e conserta a
resolução de provider LLM.

---

## Bash no host com fronteira de allowlist (#1272)

Até a v0.4.7, a `agent.bash_allowlist` só dispensava a confirmação de um comando.
Fora de um sandbox `docker`/`podman`, não havia como rodar `bash` no host com
limite real.

O modo `HostComAllowlist` muda isso. Quando a config declara padrões de comando
(prefixo com `*` no fim, ou comando exato), o `BashTool` entra em modo
allowlist-only: a lista vira **fronteira**, e todo comando fora dela é negado
antes do tier arriscado. A denylist do `safety_gate` continua na frente de tudo.
É o caminho para dar shell ao agente num aparelho sem Docker — Android/Termux,
por exemplo.

Dois ajustes acompanham o modo:

- **Aspas protegem a prosa do argumento (#1579).** A varredura antiga tratava
  parênteses, `;`, `|` e `$` **dentro de aspas** como comando composto. Um
  comando com `"(transcreva o conteúdo completo)"` na mensagem era negado mesmo
  com o prefixo liberado. Agora a checagem segue a tokenização do shell: aspas
  simples literalizam tudo, aspas duplas mantêm ativos só crase, escape e `$` de
  expansão. Composto de verdade (`;`, `&&`, pipe, `$(...)`, redirecionamento,
  nova linha) segue nunca casando.
- **`garraia doctor` mostra o shell do agente no Termux (#1560).** Sem
  docker/podman (sempre o caso no Android) e sem `agent.bash_allowlist`, o bloco
  Termux mostra o item `Tool bash` como aviso, com a receita do modo
  allowlist-only — em vez de o operador só descobrir pelo próprio agente. A
  descrição de `HostComAllowlist` em `doctor`, `/api/diagnostics` e
  `garra_status` agora diz explicitamente "SEM sandbox".

## wasmtime 49 fecha onze advisories (#1524, #1559)

O runtime WASM subiu em dois passos:

- **48.0.2 → 49.0.1 (#1524):** fecha quatro advisories — combustível não
  contabilizado em `call_ref` e no lifting de record dinâmico, exaustão de
  memória do host na escrita de corpo HTTP de saída, e pânico com datetime fora
  de faixa no `set-times` do filesystem WASI. Este passo **eleva o piso de
  MSRV para 1.96**, porque a árvore do cranelift 0.136.1 declara `rustc 1.96.0`.
- **49.0.1 → 49.0.2 (#1559):** fecha sete advisories — três no `wasmtime`
  (corrupção da heap do GC e estouro de buffer em callback async-lifted) e
  quatro no `wasmtime-wasi` (`poll_oneoff` burlando a contabilidade de
  combustível, memória alocada em excesso sem stdio, `fd_readdir` copiando
  padding não inicializado, e timestamp de filesystem anterior à epoch
  derrubando o host). Este passo **não** eleva o MSRV: a árvore do 49.0.2 segue
  em `rustc 1.96.0`.

Nenhum dos dois exigiu migração de API no `garraia-plugins`. O `Security - cargo
audit` agendado tinha ficado vermelho com os sete advisories do segundo passo.

## O Web Console fecha o que a v0.4.7 adiou (#1433, #1435)

A v0.4.7 deixou duas peças da Access Policy v2 em aberto. A v0.4.8 entrega as
duas.

**Página global Agents & Permissions (#1433).** Generaliza a página *WhatsApp
Access* para um lugar só: cada principal e suas capacidades efetivas por canal,
pelo mesmo motor de política (ADR 0025) e pelo vocabulário do registro de
capacidades, sem lógica de autorização duplicada. A rota de leitura
`GET /admin/api/permissions/overview` devolve o perfil de execução, a legenda
das classes (cada uma marcada sensível ou não) e cada canal com a sua fonte de
política — hoje só o `whatsapp_linked` tem motor por principal, e os demais são
listados com honestidade. A página não edita: leva ao caminho único da página
*WhatsApp Access*. Conceder a um principal uma classe mutante que ele não tinha
passa a exigir confirmação explícita, **enforçada no servidor**: sem
`confirm_elevation: true` o endpoint recusa com `409`.

**Exportar e importar a política, sem segredo (#1435).** `garraia whatsapp
access export` e `GET /admin/api/whatsapp/access/export` produzem um documento
versionado montado campo a campo por allowlist — token, cofre e material de
sessão ficam de fora por construção. `--redact-pii` mascara as identidades para
os quatro últimos dígitos e marca o export como não-importável. O `import`
valida formato e versão, recusa export redigido, substitui a política inteira,
mostra o impacto por principal com `--dry-run` e exige confirmação explícita
para qualquer alargamento. A escrita é atômica, com backup para rollback, e vai
para o audit.

## Os seis canais que o README anunciava voltam a contar (#1567)

README, README.pt-BR e o ROADMAP diziam que Google Chat, Microsoft Teams,
Matrix, LINE, IRC e Signal estavam "implementados no crate, wiring no gateway
pendente", e o badge anunciava "5 channels". Os seis já estavam ligados no build
default: o `garraia-gateway` liga as features de canal incondicionalmente e o
boot instancia os seis, sem `cfg`. O badge passa a **11**, e a tabela de status
distingue o canal com cobertura unitária no crate do canal coberto por teste de
integração do gateway — que é a pendência real nomeada.

## Voz: Chatterbox saudável e a Garra falando por iniciativa própria

- **Chatterbox saudável deixa de ser reportado como fora do ar (#1538).** A
  sonda batia em `/gradio_api/config`, rota que nenhuma versão do Gradio serve:
  com um `multilingual_app.py` de pé, o gateway dizia `❌ tts-chatterbox` e
  mantinha o modo voz desligado. A cadeia agora é `/` → `/config` →
  `/gradio_api/info`, para na primeira resposta 2xx, e é a mesma nos dois lugares
  que antes sondavam separado. O WARN da falha nomeia cada rota e o que ela
  respondeu.
- **Tool `telegram_send_voice` (#1557).** A Garra sintetiza o texto (até 1000
  caracteres) com o TTS configurado e entrega como voice message no Telegram. O
  destino é a mesma decisão do `telegram_send`, com teto próprio de 2 vozes por
  minuto por sessão. Falha de TTS vira erro explícito, sem fallback silencioso
  para texto; a tool some da lista quando falta TTS ou o canal está offline.
- **`docs/voice.md` ganha "Known pitfalls with the stock app" (#1539).** Duas
  armadilhas de quem sobe o app do Chatterbox pela primeira vez: a sonda de
  saúde na rota errada (corrigida acima) e os prompts de voz default, que são
  URLs passadas cruas ao `librosa.load` e quebram a síntese.

## LLM: o provider certo, e o erro que não é mais engolido

Uma série de defeitos fazia o gateway atender pelo provider errado, ou mentir
sucesso numa resposta vazia. A v0.4.8 fecha todos (#1540, #1541, #1554):

- **Dois perfis `llm:` do mesmo tipo param de disputar um slot.** Cada perfil
  passa a ser registrado pela **chave** (`with_name`), não pelo tipo. Antes,
  duas entradas `provider: openai` colidiam e o provider efetivo era sorteado a
  cada boot. Vale também para `anthropic` e `llamacpp` (#1554).
- **O fallback usa o modelo DELE.** O laço repassava o `model` do primário ao
  fallback, e `model` é o nome que *aquele* backend entende — um `openrouter`
  recebendo `glm53-flash` respondia `400`. Agora o fallback usa o modelo que a
  config já dizia para ele.
- **O SSE para de engolir o erro do provider.** Em `stream: true`, um `Err` do
  turno era descartado e o stream terminava em `finish_reason: "stop"` — uma
  resposta em branco com cara de sucesso. Agora o stream emite
  `{"error": {...}}`.
- **`POST /v1/chat/completions` sem `model` para de mandar `"gpt-4"`.** O corpo
  sem `model` chega ao provider como o sentinel vazio que ele resolve com o
  próprio modelo configurado, e o `model` do response ecoa quem de fato atendeu.

## Confiabilidade e operação

- **`restart` não cria mais daemon órfão sob systemd (#1542).** Uma instalação
  v0.4.6 chegou a 4606 reinícios em ~8h porque o `restart` matava o daemon da
  unit e subia um processo por fora dela, brigando por `Address already in use`.
  A CLI agora lê o cgroup de quem está na porta e, se for uma unit systemd,
  recusa **antes** de parar qualquer coisa (exit 78), nomeando o `systemctl
  restart` certo. Escotilha: `GARRAIA_ALLOW_SYSTEMD_RESTART=1`.
- **A remediação do MCP aponta para um comando que existe (#1543).** Os textos
  diziam `garraia mcp restart <nome>`, que o clap não conhece; agora apontam
  para `POST /admin/api/mcp/<nome>/restart`, o mesmo do botão Restart no console.
  Um teste varre o fonte para o comando fantasma não voltar.
- **`.mcp.json` da raiz volta a subir o servidor MCP (#1536).** A entrada
  apontava para `./target/release/garra`, caminho que só existe depois de um
  build e só resolve a partir da raiz do repo. Agora usa `garra` do `PATH`.

## Qualidade: cobertura medida e um piso que derruba o build (#1566)

O `.quality/baseline.json` dizia `coverage_pct: null` em todo run — o job do
ratchet sempre olhava antes de o job de cobertura terminar. Agora a busca varre
os últimos runs por um `completed`+`success` e cai para o último verde de `main`
quando o SHA da PR ainda não fechou. O baseline passa a trazer **72,00%** de
linha, medidos e gravados a partir do `lcov.info` — nunca à mão, o invariante
anti-fraude continua valendo.

Junto entra um piso de cobertura que **reprova** o build: o
`scripts/ci/coverage_gate.py` falha o job abaixo de `70,0%`, lê o `lcov.info`
pelo mesmo parser que escreve o baseline, é fail-closed (ausente ou vazio
reprova) e roda como último step, para a reprovação nunca esconder a informação
de quem vai consertar. O AI Quality Ratchet segue **report-only**: nenhuma PR é
bloqueada por ele.

---

## Como atualizar

```bash
garra update            # CLI; guarda a versão anterior e aceita `garra rollback`
```

Instalação nova segue pelo [instalador de sempre](Instalacao-e-Primeiros-Passos).

## Limites conhecidos

- Instaladores do Windows e bundles de desktop continuam **não assinados** — o
  SmartScreen avisa na primeira execução do MSI.
- O auto-updater do desktop Tauri segue desativado; o caminho de atualização
  suportado é o `garra update` da CLI.
- Os seis canais religados (#1567) têm cobertura unitária no crate, mas ainda
  falta teste de integração do gateway para eles.
