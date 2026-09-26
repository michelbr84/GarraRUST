# WhatsApp no GarraIA

Existem **dois** jeitos de conectar o WhatsApp, e eles nao se misturam.

| | Numero pessoal (QR) | WhatsApp Business (Cloud API) |
|---|---|---|
| Comando | `garraia whatsapp link` | `garraia whatsapp cloud` |
| Conta | qualquer numero | conta Business aprovada pela Meta |
| Precisa de | Node.js 20+ na maquina | dominio publico com HTTPS |
| Credencial | sessao cifrada em disco | `access_token` da Meta na config |
| Suporte | **nao oficial** | oficial |
| Risco de bloqueio | **sim** | nao |

Este documento e sobre o **primeiro**. O segundo esta em
[`channels.md`](channels.md#whatsapp).

---

## Tutorial (o caminho inteiro)

```bash
garraia whatsapp
```

> Desde a #1430 voce nao precisa saber que este comando existe: o
> `garraia init` pergunta, no passo de canal, se quer conectar Telegram,
> **WhatsApp (numero pessoal, por QR)**, os dois ou nenhum — e a opcao do
> WhatsApp cai exatamente no fluxo descrito abaixo, com a mesma tela de
> aviso. O default daquele passo e **nenhum**, e um vinculo que nao
> complete nao derruba o `init`: a config ja esta salva e voce volta aqui
> quando quiser.

1. Escolha a opção **1) Conectar meu WhatsApp pessoal (ler um QR code)**.
2. Leia a tela de aviso e confirme.
3. No celular: **Configurações → Aparelhos conectados → Conectar um aparelho**.
4. Aponte a camera para o QR no terminal.
5. Diga **quem pode falar com o GarraIA** — o numero com o codigo do pais:

```text
✓ Autenticado. Sincronizando sessão…
✓ WhatsApp conectado com sucesso.
✓ Sessão salva em ~/.config/garraia/data/whatsapp/default.

Quem pode falar com o GarraIA por este WhatsApp? Ninguém, até você autorizar.
Número autorizado, com código do país (ex.: +55 11 99999-8888; vazio = ninguém por enquanto): +55 11 98888-0000
✓ Número terminado em 0000 autorizado.

Acesso em vigor neste WhatsApp (o mesmo que `garraia whatsapp users` mostra):
Canal:    ligado
Autorizados: 1 · Donos: 0
  autorizado · número terminado em 0000

✓ GarraIA está pronto para receber mensagens (inicie o gateway: `garraia start`)
```

O resumo antes da ultima linha (#1429) e o **mesmo** do `garraia whatsapp users`
— canal, contagens e as identidades por papel e pelos quatro ultimos digitos,
nunca o numero inteiro. Ele sai tambem num re-vinculo que nao mudou nada, para
o operador nao sair do wizard sem ver o portao que herdou.

O "pronto" so aparece quando ha pelo menos um numero autorizado. Resposta
vazia deixa o portao fechado (ninguem recebe resposta), e o resumo termina no
aviso com o comando que resolve depois — uma vez so, sem repeti-lo como ultima
linha, e sem terminal inclusive:

```text
Acesso em vigor neste WhatsApp (o mesmo que `garraia whatsapp users` mostra):
Canal:    ligado
Autorizados: 0 · Donos: 0
⚠ Ninguém está autorizado a falar com o GarraIA por este WhatsApp — toda mensagem será ignorada em silêncio. Autorize um número: `garraia whatsapp allow <número>` (com o código do país).
```

Se o gateway ja esta rodando, a ultima linha diz o que fazer: com o canal ja
supervisionado, nada (a autorizacao vale na proxima mensagem); com o canal
recem-ligado, `garraia restart` para o gateway subi-lo. A CLI **nunca**
reinicia o gateway sozinha — ela nao sabe se ele roda em primeiro plano, sob
systemd ou num container, e um restart derrubaria turnos de outros canais.

Rodar `garraia whatsapp` de novo com uma sessao valida **nao** repareia e **nao**
apaga nada: ele valida e responde `✓ Sessão encontrada e válida`.

> **Aviso.** Conectar pelo QR usa o recurso de "aparelho conectado" do WhatsApp
> por um cliente **nao oficial**, o que contraria os termos de uso da Meta. A
> conta pode ser bloqueada, temporaria ou permanentemente, sem recurso
> garantido. **Use um numero secundario.** Se voce precisa de suporte oficial,
> use a Cloud API (opcao 2).

## Comandos

| Comando | O que faz | Exit code |
|---|---|---|
| `garraia whatsapp` | menu de duas opcoes | 0, ou 1 se cancelado |
| `garraia whatsapp link` | vincula por QR | 0 · 1 cancelado · 69 sem Node / QR nao lido · 70 erro interno |
| `garraia whatsapp cloud` | wizard da Cloud API | 0 · 1 cancelado · 70 erro interno |
| `garraia whatsapp status` | diz se ha vinculo e se a sessao abre | 0 vinculado · 69 nao vinculado ou ilegivel |
| `garraia doctor whatsapp [--json] [--strict]` | o caminho inteiro numa passada: vinculo, chave da sessao, gateway e ponte, acesso, perfil de execucao, workspace, MCP visivel no piso, provider — cada linha com o proximo passo, no vocabulario do `/api/diagnostics` (#1419) | 0 tudo verde · 2 aviso com `--strict` · 69 algo vermelho |
| `garraia whatsapp logout` | apaga a sessao e desliga o canal | 0 · 1 cancelado |
| `garraia whatsapp restore` | devolve o `session.enc.prev` ao lugar | 0 · 69 nao ha arquivada, ou ha sessao em uso · 70 erro interno |
| `garraia whatsapp allow <numero> [--owner] [--yes]` | autoriza um numero a falar com o GarraIA; funciona sem terminal | 0 · 1 cancelado · 64 `--owner` fora de `isolated-pod`, ou sem terminal e sem `--yes` · 65 numero invalido (inclusive `*`, ver abaixo) · 70 config ilegivel |
| `garraia whatsapp users [--json]` | lista quem esta autorizado: papel (`allow`/`owners`) e os quatro ultimos digitos de cada identidade | 0 · 70 config ilegivel |
| `garraia whatsapp remove <numero> [--yes]` | revoga o acesso: tira a identidade de `allow` **e** de `owners`; dono exige confirmacao | 0 (inclusive quem nao estava na lista) · 1 cancelado · 64 dono sem terminal e sem `--yes` · 65 numero invalido · 70 config ilegivel |
| `garraia whatsapp owner <numero> [--yes]` | promove a DONO: grava em `owners`, a mesma escrita do `allow --owner` | 0 (inclusive quem ja era dono) · 1 cancelado · 64 fora de `isolated-pod`, ou sem terminal e sem `--yes` · 65 numero invalido · 70 config ilegivel |
| `garraia whatsapp unowner <numero> [--yes]` | tira o papel de DONO **sem** tirar o acesso; o ultimo dono exige confirmacao | 0 (inclusive quem nao era dono) · 1 cancelado · 64 ultimo dono sem terminal e sem `--yes` · 65 numero invalido · 70 config ilegivel |
| `garraia whatsapp access [--json] [--reveal]` | a politica efetiva inteira (ADR 0025): admissao, default do desconhecido, grupos e cada principal com piso, nivel e o que pode de fato — pelo MESMO `ToolGate` do turno; identidades so por `…1234`, `--reveal` mostra os valores da config (local) | 0 · 70 config ilegivel |
| `garraia whatsapp access open [--yes] [--dry-run]` / `access restricted` | troca a admissao; `open` avisa (QUALQUER numero passa a entrar, com o default) e pede confirmacao | 0 · 1 cancelado · 64 `open` sem terminal e sem `--yes` · 70 |
| `garraia whatsapp access default chat\|read [--write] [--dry-run]` | o que um desconhecido recebe em `open` (`full` e recusado; guardado mesmo em `restricted`) | 0 · 65 combinacao invalida · 70 |
| `garraia whatsapp level <numero> chat\|read\|full [--dry-run]` | o nivel (teto) de uma identidade; no dono e recusado (use `unowner` antes) | 0 · 65 numero/combinacao invalida · 70 · 73 gravou mas o audit falhou |
| `garraia whatsapp write <numero> on\|off [--dry-run]` | escrita de arquivo (nativa e MCP) de uma identidade — e SO isso | 0 · 65 (tambem para quem nao esta autorizado ou esta em `chat`) · 70 · 73 |
| `garraia whatsapp block <numero>` / `unblock <numero>` | bloqueia (vence `open`, `allow` e pareamento; vale na mensagem seguinte) / desbloqueia | 0 · 65 · 70 · 73 |
| `garraia whatsapp access groups on\|off\|default <nivel>` / `access group <jid> <nivel> [--write]` | grupos: liga/desliga, o default e a politica por JID (`<digitos>@g.us`) | 0 · 65 · 70 · 73 |
| `garraia whatsapp access reset [--yes] [--dry-run]` | volta ao seguro: `restricted`, default `chat`, grupos desligados e sem politica, niveis/write removidos; donos e bloqueios ficam; idempotente | 0 · 1 cancelado · 64 sem terminal e sem `--yes` · 70 |
| `garraia whatsapp access audit [--json] [--limit N]` | a trilha local de mudancas (`<data_dir>/audit/whatsapp-access.jsonl`), mais recente primeiro | 0 · 70 |

O `users` nunca imprime a identidade inteira — nem na tela, nem no `--json`,
cujo documento e `{enabled, authorized, owners, users[{role, kind, last4}]}`.
O numero completo so existe no `config.yml` (0600). Escopo de hoje: listagem
simples, **sem** filtro por papel e **sem** paginacao.

`garraia whatsapp allow '*'` nao abre o canal para todo mundo: o `*` e
recusado com 65 e uma mensagem propria, porque essa semantica nao existe — o
portao e fail-closed e cada identidade entra uma a uma.

**`unowner` nunca e `remove`.** Como o `allow --owner` (e o `owner`) gravam
so em `owners`, o caso comum e a identidade existir apenas la; tirar dali e
pronto revogaria o acesso junto com o papel. Entao, quando `allow` ainda nao
a tem, a entrada passa para `allow` **na mesma escrita** — nao existe instante
no disco em que a pessoa nao esteja em nenhuma das duas listas. Quem quer
revogar de fato usa o `remove`.

Por isso **`owner X` seguido de `unowner X` nao e um no-op**: X termina em
`allow`, autorizado, mesmo que antes dos dois comandos ele nao estivesse em
lista nenhuma — promover da acesso (o portao e a uniao das duas listas) e
rebaixar nunca o tira. Para desfazer o acesso e preciso o terceiro comando,
`garraia whatsapp remove X`.

A assimetria entre os dois comandos e proposital: **promover** exige
`isolated-pod` (mesma porta do `allow --owner`: em `standard` o dono nao tem
poder nenhum e ganharia tudo em silencio no dia em que o perfil mudasse),
**rebaixar** funciona em qualquer perfil — e justamente em `standard` que um
`owners` esquecido e um privilegio latente, e recusar a limpeza ali deixaria o
operador sem o comando onde ele mais importa.

A confirmacao do `unowner` vale para o **ultimo** dono, nao para todo
rebaixamento: com outro dono na lista nada fica invalido e o acesso e
preservado nos dois casos, entao o que merece uma parada explicita e o estado
que o `owner` nao pode desfazer sozinho num pipe — a configuracao ficar sem
dono nenhum. O `remove` de um dono continua pedindo confirmacao sempre, porque
la o acesso cai junto.

`garraia whatsapp link --allow <numero> [--owner]` pre-responde a pergunta do
numero (e a do dono), mas continua exigindo terminal: o QR se le dali. Num
pipe ele sai 69, como o `link` puro.

Os codigos seguem `sysexits` (64 = `EX_USAGE`, 65 = `EX_DATAERR`,
69 = `EX_UNAVAILABLE`, 70 = `EX_SOFTWARE`), como
`garraia desktop` e `garraia config check`.

**Sem terminal** (pipe, CI, `curl … | sh`, systemd) o comando **nao trava e nao
falha**: ele imprime as duas opcoes com o comando de cada uma e sai 0.

## Estados

O pareamento passa por estes estados, e cada um aparece como uma linha curta:

```text
sem sessao:      not_connected → qr_required → qr_generated → waiting_scan
                 → authenticated → connected
com sessao:      session_found → validating → connected
sessao recusada: validating → (arquiva session.enc.prev) → qr_required → …
em operacao:     connected → reconnecting{tentativa} → connected
sessao morta:    → session_dead (apaga o material, exige QR novo)
```

O QR expira a cada ~20 s. O GarraIA regenera **ate 5 vezes**; na quinta
expiracao ele desiste com `Nenhum QR foi lido. Rode `garraia whatsapp` de novo.`
e sai 69. O teto e do GarraIA, nao da ponte: a ponte reconectaria para sempre.

## Onde a sessao fica, e como ela e protegida

```text
<data_dir>/whatsapp/default/          modo 0700
  session.enc        0600   blob cifrado (AES-256-GCM)
  session.enc.prev   0600   sessao anterior, arquivada ate o novo link dar certo
  session.key        0600   chave de 32 B — SO quando nao ha passphrase do cofre
  session.salt       0600   salt do PBKDF2 — SO quando ha passphrase do cofre
```

`<data_dir>` e o `data_dir` da config, ou `~/.config/garraia/data` por padrao.

**Os modos `0600`/`0700` acima valem em Unix.** No Windows hoje nao ha
equivalente: nenhum hardening de ACL e aplicado a esses arquivos, que herdam
as permissoes default do diretorio — qualquer processo do mesmo usuario le.
O gap esta registrado na #1253; no Windows, a linha "outro usuario" da tabela
abaixo **nao** se aplica.

**`session.enc.prev` tambem e uma credencial viva.** Ele nasce quando voce
responde "sim" ao re-vincular. Por isso `garraia whatsapp status` o reporta mesmo
quando nao ha sessao ativa, e `garraia whatsapp logout` o apaga — sem isso, os
dois comandos afirmariam que nao ha nada enquanto a credencial estivesse no
disco.

**E `garraia whatsapp restore` o traz de volta.** Se um re-vinculo foi
interrompido de um jeito que nao deu ao GarraIA a chance de desfaze-lo — um
`kill -9`, uma queda de energia —, a sessao boa fica no `.prev` sem
`session.enc`. O `restore` a devolve ao lugar e religa o canal. Ele **nunca**
passa por cima de uma sessao em uso: nesse caso diz o que ha e sai 69, sem
apagar nada. Restaurar nao garante que o WhatsApp ainda aceite o aparelho — rode
`garraia whatsapp status` depois.

**Um re-vinculo que nao termina devolve a sessao antiga.** QR expirado,
`Ctrl+C` na tela do QR, ponte que morre antes de conectar: em qualquer desfecho
sem sessao nova em disco, o `session.enc.prev` volta a ser `session.enc` e o
comando diz isso na ultima linha (`A sessao anterior foi restaurada`). Voce nao
fica sem WhatsApp por ter desistido no meio. O arquivado so e descartado — com
sobrescrita — quando o vinculo novo conclui de verdade.

**O blob de sessao e a conta.** Quem o tem fala como voce, le seu historico e
nao precisa do seu telefone. Trate-o como senha.

### Modelo de seguranca, em uma tabela

| Ameaca | Com `GARRAIA_VAULT_PASSPHRASE` | Sem ela |
|---|---|---|
| Backup / snapshot de container vazado | protegido: a chave nunca toca o disco | **nao protegido**: a chave vai junto |
| Disco roubado com a maquina desligada | protegido | **nao protegido** |
| Outro usuario do sistema (UID diferente, Unix) | protegido pelo modo 0600/0700 | protegido pelo modo 0600/0700 |
| Outro processo do mesmo usuario (Windows) | **nao protegido** — sem ACL restrita (#1253) | **nao protegido** |
| Root, ou seu proprio usuario | nao protegido | nao protegido |

Por isso o aviso aparece **na tela de consentimento**, antes de a pessoa
decidir, e de novo no `garraia whatsapp status`:

```text
⚠ a chave da sessao esta em session.key, no MESMO diretorio do arquivo
  cifrado: um backup do seu home, um snapshot do container ou um disco
  roubado levam a sessao junto. Defina GARRAIA_VAULT_PASSPHRASE para a
  chave deixar de tocar o disco.
```

Detalhes do que e feito:

- **AES-256-GCM** (`ring`), a mesma pilha do `CredentialVault`. O AAD e a string
  de versao do formato, entao um arquivo de outra versao falha a autenticacao em
  vez de decifrar lixo.
- **PBKDF2-HMAC-SHA256, 600 000 iteracoes**, com salt de 32 B em disco, quando ha
  passphrase. Mesmos parametros do cofre.
- **Escrita atomica**: temporario no mesmo diretorio, ja criado em 0600, `fsync`,
  `rename`.
- **Nada disso vai para log**, e o que garante isso e uma regra fechada, nao uma
  promessa. `SessionBlob` imprime `<redacted>` em `Debug` e `Display`; alem
  disso, um teste varre o proprio fonte do modulo e reprova **qualquer**
  ocorrencia de `SessionBlob::expose()` fora de uma allowlist de call sites
  nomeados — hoje ha exatamente dois, a declaracao e o `seal_in_place` do
  `save`. A garantia vale para o codigo destes sete arquivos: e o call site do
  `expose()` que e fixado, e nao a lista de macros de log, justamente porque
  enumerar macros nao alcanca `let s = blob.expose(); let t = s;`. Uma segunda
  varredura, essa por bloco de macro (nao por linha), continua pegando um campo
  `session`/`blob`/`creds`/`qr` que nunca passou por `expose()`.
  O `RedactingWriter` de `garraia-security` redige por prefixo conhecido
  (`sk-`, `xoxb-`…) e **nao** reconheceria um base64 generico — por isso a defesa
  esta no tipo e na allowlist, e nao no writer.
- **A cauda de stderr do Node tambem e redigida** antes de chegar a tela:
  sequencias longas que parecem base64 viram `<redigido: N caracteres>` e a
  linha tem teto de comprimento. Ela e a unica saida crua de ferramenta externa
  deste fluxo, e o lado JS nao cobre todo `throw` que passa por ela.
- **JIDs e conteudo de mensagem** tambem nao vao para log: `Jid` imprime so os 4
  ultimos digitos e `InboundMessage` imprime forma, nunca texto.

### Ferramentas e servidores MCP

> **Registrada nao e utilizavel (#1425).** Uma ferramenta que existe no
> gateway mas nao esta operacional agora — `telegram_send` sem o Telegram
> configurado na config viva ou com o canal fora do ar — fica **fora** da
> lista que o modelo recebe no turno; o `garra_status` e o `/api/diagnostics`
> e que a mostram, com o motivo (`not_configured`, `channel_offline`). Se o
> modelo a pedir pelo nome mesmo assim, ela nao roda e a explicacao volta como
> resultado de ferramenta, distinta de "negada pela politica". Desligar o
> canal na config tira a tool na mensagem seguinte, sem restart. O
> `garra_status` devolve a lista `capabilities` — cada capacidade com o seu
> estado nesta conversa (`visible`, `denied`, `unavailable`, `unhealthy`,
> `not_configured`), motivo e remediacao — e o prompt manda o modelo
> responder a partir dela: `denied` e "existe e nao esta liberada aqui",
> nunca "nao existe". O mesmo registro sai em `tools.capabilities` no
> `/api/diagnostics` e em `GET /admin/api/capabilities` (#1381, #1387). E uma sessao
> **sem raiz nenhuma** para as file tools (sem diretorio de trabalho nem
> `agent.file_roots`) recebe uma recusa propria e acionavel — "selecione um
> projeto com `/project <nome>`" — em vez da recusa generica de caminho fora
> das raizes (#1418).

> **Falha repetida abre o breaker (#1417).** Cada sessao tem um circuit
> breaker por ferramenta, no unico ponto de despacho do runtime. Uma falha
> **deterministica** — sem raiz para as file tools, `repo_search` sem
> repositorio — poe a ferramenta em pausa ate o fim do turno: se o modelo a
> pedir de novo, ela nao roda, e volta um resultado de ferramenta com o
> motivo (`no_roots`, `no_repository`) e a instrucao de nao repetir. Um
> **timeout** abre um cooldown que dobra a cada timeout seguido (15s, 30s,
> 60s, teto de 120s) e atravessa turnos; um erro generico so abre depois de
> tres iguais no mesmo turno (`repeated_error`). A recusa de caminho **fora
> das raizes** e generica de proposito: vale para aquele caminho, nao para a
> ferramenta — o modelo pede `/etc/x`, le a recusa, corrige para `./src/x` e
> a segunda chamada roda; so tres recusas iguais no turno pausam.
> Uma chamada bem-sucedida fecha o breaker daquela ferramenta; um turno com
> `working_dir` diferente limpa a sessao inteira. Ferramenta indisponivel
> (#1425) nao chega ao breaker: e recusada antes. O `garra_status` lista o que
> esta em pausa nesta sessao em `breaker` (`tool`, `reason_code`, `reason`),
> texto constante, sem caminho nem saida crua; o agregado por instalacao no
> `/api/diagnostics` fica para a #1438.

Quem manda mensagem para o numero vinculado e, para o agente, um remetente
**nao autenticado**: a allowlist do canal decide quem entra, e o que ele pode
fazer depois de entrar e decidido pelo `ToolGate` do modo — por **nome de
ferramenta**, em cada turno, contra o inventario vivo do runtime. Sao duas
camadas, e nenhuma substitui a outra.

- **O piso e o perfil `search`.** Sessao que nao escolheu modo (`/mode`)
  resolve para `search`, e nao para "sem politica de ferramenta":
  `whitelist_mode` ligado, `allowed` so de leitura (`file_read`, `repo_search`,
  `list_dir`, `web_search`, `web_fetch`, `device_list`, `device_read`,
  `garra_status` — que descreve o proprio runtime, sem segredo, #1347 — e as
  dez operacoes somente-leitura do MCP `filesystem`, na forma `*/<operacao>`,
  #1384) e `denied` para `file_write`, `bash` e `device_execute`.
  O `garra_status` e o que responde "voce tem acesso ao WhatsApp?": a lista
  `channels` dele sai da mesma funcao do `/api/channels`, entao este canal
  aparece `active` com a ponte conectada e `offline` com ela caida. Numa
  sessao deste canal (e em todo turno de portao restrito) o relatorio retem
  o que e do operador — diretorio da sessao, `project_id`, lista de
  provedores, nomes dos servidores MCP e a versao exata — e lista o que
  reteve em `withheld`; o `session.id` sai com o numero mascarado nos 4
  ultimos digitos.
  `channels.whatsapp_linked.default_mode` troca o perfil padrao por **outro
  modo nativo** (`ask`, `code`, `debug`, …); a escolha explicita do usuario
  (`/mode`) continua vencendo. Dois valores **nao** servem, e o canal nao sobe
  com eles (`canal nao subiu — channels.whatsapp_linked.default_mode = … nao e
  um modo nativo`): um nome que nao e modo nativo — typo, ou o nome de um modo
  customizado — porque `ToolGate::for_mode_name` trata nome desconhecido como
  portao **aberto** (`bash` incluso), e o gateway recusa em vez de subir assim;
  e `auto`, porque deixaria o texto da mensagem de um estranho escolher o
  perfil. Modo customizado (#986) nao serve de piso porque o perfil dele mora
  no banco e so e resolvido para o modo que a **sessao** escolheu; quem quiser
  um customizado neste canal o escolhe com `/mode`, que e escolha explicita e
  resolve o perfil.
  E o bloco `file_tools` do mesmo relatorio diz se as file tools tem raiz NESTA
  sessao (`ready`, `source`: workspace por sessao, `working_dir` de projeto,
  raiz declarada ou nenhuma) — sem caminho, e com a frase que o modelo deve
  dizer quando nao ha raiz (#1416, #1418).
- **Ferramenta MCP passa pelo mesmo portao.** Um servidor MCP registrado — o
  `filesystem` que toda instalacao nova ganha no primeiro boot, por exemplo —
  expoe ferramentas com nome `servidor__ferramenta`, e o whitelist as trata
  como qualquer outra: `filesystem__write_file` nao esta na `allowed` do
  `search`, entao e negada por nome, como `bash`, e o modelo nem a ve na
  lista. **Nao ha recusa de subida por "existe servidor MCP"**: ela existiu
  (ate a #1327) para compensar uma isencao do portao que a #1288 fechou, e o
  efeito que sobrou era o canal nunca subir em instalacao padrao.
- **Como uma ferramenta MCP chega ao modelo neste canal.** Pelo piso, so a
  **leitura** do `filesystem` (#1384): o `search` nativo declara, na forma
  `*/<operacao>`, as dez operacoes somente-leitura do
  `@modelcontextprotocol/server-filesystem` (`read_text_file`,
  `list_directory`, `search_files`, `get_file_info`…) — entao um remetente
  admitido le e lista o workspace **da propria sessao** (#1448), e nada mais
  — e isso vale de verdade porque a chamada MCP passa pelo **mesmo jail**
  das file tools nativas (#1482): `path`/`paths`/`source`/`destination`
  sao confinados ao diretorio da sessao (mais `agent.file_roots`), e
  `list_allowed_directories` responde as raizes **da sessao**, nunca a raiz
  do servidor, que e o pai de todas elas —
  `write_file`, `edit_file`, `create_directory`, `move_file` e qualquer
  operacao fora da lista continuam negadas pelo nome, e nenhum outro servidor
  passa (`servidor/*` libera o servidor inteiro, leitura **e** escrita;
  `servidor__ferramenta` libera uma so). Qualquer coisa alem disso chega por
  dois caminhos, os dois escolhidos por alguem: o operador poe em
  `default_mode` um nativo **sem** whitelist (`ask`, `code`), em que passa
  tudo que o `denied` nao nomeia; ou a sessao escolhe, com `/mode`, um modo
  customizado cujo `allowed` declara `servidor/*` (ou tem `allowed` vazia com
  `whitelist_mode` ligado, que permite tudo — comportamento preservado da
  #1264). Num canal exposto ao mundo, qualquer dos dois quer dizer "quem
  estiver na allowlist do WhatsApp pode acionar esse servidor".
- **O aviso de drift.** Na subida do canal o gateway monta o portao do perfil
  padrao — pelo mesmo caminho que o turno monta o seu, sobre o `default_mode`
  ja validado — e percorre o inventario MCP. A leitura do `filesystem` que o
  `search` libera por desenho **nao** e drift e nao gera aviso; ele so tem o
  que dizer quando o `default_mode` e um perfil **sem** whitelist (`ask`,
  `code`): sai **um** `WARN` nomeando os servidores e o motivo (nunca
  argumento nem segredo):

  ```text
  WARN whatsapp_linked: o perfil `ask` (`channels.whatsapp_linked.default_mode`) libera ferramentas MCP dos servidores filesystem a quem manda mensagem para este numero — o perfil nao tem whitelist de ferramenta, entao passa tudo que o `denied` nao nomeia; use `search` para um piso somente-leitura, ou confirme que e intencional
  ```

  E aviso, nao recusa: o `default_mode` e do operador. Se nao foi intencional,
  volte para `search` (ou remova a chave) e reinicie o gateway. O aviso sai na
  subida; um servidor registrado depois pela admin API, sob um perfil ja
  permissivo, nao o reemite.

### Quem pode falar com o GarraIA (`allow`)

O portao do canal e **fail-closed**: `allow` e `owners` vazios significam
**ninguem**. Mensagem de quem nao esta autorizado e descartada **em silencio**
— sem resposta, porque responder confirmaria ao estranho que o numero roda um
bot — e o log do gateway guarda so os 4 ultimos digitos. Nao existe auto-claim:
o primeiro remetente nunca vira dono, e o numero vinculado nao se autoriza
sozinho.

```bash
garraia whatsapp allow +55 11 98888-0000
```

- **`+` e codigo do pais obrigatorios**, nunca adivinhados: sem o `+`,
  `11 98888-0000` (DDD + numero) passaria por um numero de 11 digitos e nunca
  casaria com quem manda. Espacos, hifens, pontos e parenteses sao
  descartados; letra, zero inicial (prefixo de discagem local), a forma
  `@s.whatsapp.net` e qualquer coisa fora de 6 a 15 digitos (a faixa que a
  ponte entrega) sao recusados (exit 65). O numero e gravado so com digitos
  (`5511988880000`), a forma que o portao compara.
- **Celular brasileiro com ou sem o nono digito e o mesmo.** Muita conta
  antiga tem o JID sem o 9 (`55 31 9888-0000`, 12 digitos) e o numero se
  digita com ele (`+55 31 98888-0000`, 13). O portao compara as duas formas
  como uma so — so para `55` + DDD + `9` + numero que comeca com 6 a 9; fixo
  e outro pais nao mudam. O `config.yml` guarda o que voce digitou.
- `allow` **acrescenta** a lista sem mudar outro valor: as outras chaves da
  secao, as outras secoes e o `enabled` ficam como estavam. Mas o arquivo e
  **reescrito** a partir da config lida: comentarios, chaves que o GarraIA nao
  conhece e a formatacao original nao sobrevivem, e secoes com valor default
  podem passar a aparecer. Se voce cura o `config.yml` a mao, edite a lista a
  mao. Numa instalacao sem a secao ele a cria com `type: whatsapp_linked` e o
  canal desligado — quem liga e o `link`, depois de a sessao existir.
- **Contato por LID (`@lid`).** O WhatsApp as vezes identifica um contato so
  por um identificador opaco, `<id>@lid`, sem o numero. Quando o servidor
  manda o numero junto (o Baileys 7 o entrega em `remoteJidAlt`/
  `participantAlt`), a ponte o usa e o numero do `allow` casa normalmente.
  Quando nao manda, o portao compara o LID — e um numero no `allow` **nao**
  casa com ele (fail-closed: a mensagem e recusada em silencio). O
  `garraia whatsapp status` avisa quando o gateway em execucao recusou
  remetentes `@lid` sem numero (quantos, e o final do ultimo LID), e o
  `/api/diagnostics` mostra a contagem. Para autorizar esse contato: gere um
  codigo com `/pair` e peca para a pessoa manda-lo por WhatsApp (vale ate o
  gateway reiniciar), ou autorize o LID inteiro com
  `garraia whatsapp allow <id>@lid`, gravado como veio.
- **O celular vinculado nao conversa com o GarraIA.** Mensagens que ele envia
  saem da propria conta (`from_me`) e sao ignoradas, senao o canal responderia
  a si mesmo. O `link` avisa quando o numero digitado termina como o do
  aparelho vinculado e pede confirmacao (default nao). Use outro numero.
- **Vale sem reiniciar — com duas condicoes.** Com o gateway rodando, `allow`
  e `owners` sao relidos do `config.yml` a cada mensagem: autorizar vale na
  proxima, e **revogar tambem** — rode `garraia whatsapp remove <numero>`
  (ou, se preferir editar a mao, apague a entrada das listas no arquivo).
  Trocar o papel vale igual: `garraia whatsapp owner` e
  `garraia whatsapp unowner` mexem nas mesmas duas listas, e o piso do dono
  muda (ou some) na mensagem seguinte.
  `enabled: false` no arquivo recusa todo mundo,
  codigo de pareamento incluso, na mensagem seguinte (a ponte segue conectada
  ate o restart, e o `/api/diagnostics` avisa). As condicoes: o gateway tem de
  ter **subido com o canal ligado** (se o `link` ligou o canal depois, rode
  `garraia restart`), e o `config.yml` tem de **existir quando o gateway
  subiu** — so entao ele vigia o arquivo. O `allow` nao tem como saber
  nenhuma das duas coisas e diz as duas; o aviso de boot e o diagnostico
  dizem qual e o caso. Ja `enabled` de `false` para `true`, `default_mode`,
  `reply_in_groups` e `execution.profile` pedem restart do gateway.
- **O que a revogacao nao alcanca.** Quem entrou por um codigo `/pair` e
  **nunca** esteve no `allow` continua admitido ate o gateway reiniciar (o
  pareamento mora na memoria do processo); quem estava no `allow` e pareou
  perde os dois ao sair da lista. E se o `config.yml` editado nao for YAML
  valido, o gateway **mantem a lista anterior** e loga `config reload failed
  (keeping previous config)` — confira o log depois de revogar.
- **Grupos** continuam exigindo `reply_in_groups: true` (default `false`),
  mesmo para numero autorizado.
- `garraia whatsapp status` mostra a ponte, o gateway (rodando ou nao), o canal
  (ligado ou nao), `Autorizados: N · Donos: M` — contagens, nunca numeros — e
  avisa quando o canal esta ligado com ninguem autorizado. Para saber **quem**
  sao esses N, `garraia whatsapp users` lista papel e os quatro ultimos
  digitos de cada um (as linhas de canal/contagens sao as mesmas dos dois
  comandos, de proposito: uma funcao so). O
  `/api/diagnostics` rebaixa `whatsapp.linked` para `warning` no mesmo caso,
  com o passo `garraia whatsapp allow <numero>`, e o gateway loga um `WARN` ao
  subir com o portao vazio.

### Dono e perfil de execucao (`owners`)

Desde a v0.4.4 (ADR 0024, #1329) o piso acima depende tambem do **perfil de
execucao** do processo — `execution.profile`, `standard` (default) ou
`isolated-pod` — e de uma segunda lista na secao do canal:

```yaml
execution:
  profile: isolated-pod          # ou GARRAIA_EXECUTION_PROFILE=isolated-pod

channels:
  whatsapp_linked:
    type: whatsapp_linked
    enabled: true
    allow: ["5511888880000"]     # admitidos: piso `default_mode` (default `search`)
    owners: ["5511999998888"]    # donos: em isolated-pod, piso `code` em conversa 1:1
```

- `owners` usa a mesma normalizacao de `allow` (`normalizar_identidade`:
  digitos, ou JID `@lid` como veio) e a mesma comparacao (o nono digito
  brasileiro incluso). Quem esta em `owners` e admitido como se estivesse em
  `allow`.
- O `--owner` do `allow` decide pelo perfil que **este shell** ve: o
  `execution.profile` do arquivo ou `GARRAIA_EXECUTION_PROFILE` no ambiente
  do comando. Se o gateway roda com a env (num pod, por exemplo), rode o
  `allow --owner` com a mesma env.
- Em **`standard`** `owners` nao muda nada: todo admitido, dono incluso, fica
  em `default_mode`. O `config check` avisa (`owners so tem efeito em
  isolated-pod`) — e aviso, nunca poder.
- Em **`isolated-pod`** o dono em conversa **1:1** recebe o perfil completo:
  `default_mode` se explicito na config, senao **`code`** — sem whitelist,
  entao `file_write`, `bash`, subagentes e toda ferramenta MCP registrada
  (`filesystem__write_file` inclusa). O jail das file tools nativas
  (`agent.file_roots`) e o gate de comando arriscado do `bash` continuam
  valendo; o perfil libera ferramentas, nao desliga protecoes.
- **Pareamento nunca confere o perfil completo.** O codigo de 6 digitos e
  credencial fraca (memoria do processo, um codigo); so identidade declarada
  na config e dono.
- **Grupo nunca herda.** Dono mandando de um grupo, e admitido que nao e dono
  em qualquer conversa, caem no piso `standard` (`default_mode`).
- `from_me` continua fora (`deve_responder`): "note to self" nao e caminho
  suportado.
- Um `/mode` explicito da sessao continua vencendo o piso, nos dois perfis.
- Pela CLI: `garraia whatsapp allow <numero> --owner` grava em `owners` (e so
  la). Fora de `isolated-pod` ele recusa com exit 64 — um dono em `standard`
  seria um privilegio latente, que acordaria em silencio no dia em que o
  perfil mudasse. Sem terminal exige `--yes`; no terminal pergunta, com
  default nao. O `link` so oferece dono em `isolated-pod`, tambem com default
  nao. `garraia whatsapp owner <numero>` faz a MESMA escrita pelo caminho
  dedicado, com as mesmas duas portas (perfil e confirmacao): promover quem ja
  esta em `allow` deixa de exigir que se repita o comando de autorizar.
- Tirar o numero de `owners` no arquivo tira o piso do pod na mensagem
  seguinte, mesmo que ele continue em `allow` — e e isso que
  `garraia whatsapp unowner <numero>` faz, preservando o acesso (a entrada
  passa para `allow` se so existia em `owners`). Ja
  `garraia whatsapp remove <numero>` tira das **duas** listas, e por isso
  tambem pede confirmacao quando o numero e dono.

O perfil efetivo do turno e uma funcao pura (`perfil_do_turno`), avaliada
**depois** de `admitir` e **antes** de montar o `ExecContext`; ela alimenta
`piso_somente_leitura`. Cada turno loga `phone_last4` + `perfil` (`completo`
| `padrao`) + o modo do piso — nunca JID, telefone, `push_name` nem texto (a
varredura `fonte_nao_loga_jid_cru_nem_material_de_sessao` continua valendo).
`garraia whatsapp status` mostra o perfil, o piso do dono e a contagem de
donos; o check `execution.profile` do `/api/diagnostics` tambem. Sem
`owners`, `isolated-pod` nao muda nada neste canal — o diagnostico diz
"0 donos".

Detalhes do perfil, a lista do que ele **nao** isola e o exemplo para pod:
[`execution-profiles.md`](execution-profiles.md).

### Politica de acesso por principal (`access`, ADR 0025)

`allow` e `owners` respondem "quem entra". Desde a v0.4.6 a secao `access`
responde tambem "ate onde cada um vai", por **principal** — e o que
`garraia whatsapp` e o Web Console passam a editar:

```yaml
channels:
  whatsapp_linked:
    type: whatsapp_linked
    enabled: true
    allow: ["5511888880000"]       # continua valendo: admitido, sem teto
    owners: ["5511999998888"]      # continua valendo: dono
    access:
      admission: restricted        # restricted (default) | open
      default: { level: chat, write: false }   # o DESCONHECIDO, so com open
      users:
        "5511888880000": { level: read, write: false }
        "5511999998888": { role: owner }
        "5521955554444": { blocked: true }
      groups:
        enabled: false             # ou o `reply_in_groups` legado
        default: { level: read, write: false }
        "120363000000000000@g.us": { level: chat }
```

- **Niveis.** `chat` = nenhuma ferramenta; `read` = so leitura (`file_read`,
  `web_search`, `device_read`, MCP de leitura...), nunca shell, dispositivo,
  mensagem nem agenda; `full` = sem teto proprio (o modo e o perfil de
  execucao decidem). `write: true` libera **so** escrita de arquivo (nativa e
  MCP) — nao liga `bash`, nao desliga sandbox, jail nem a confirmacao de
  ferramenta perigosa. O nivel e um **teto** composto por E com o modo da
  sessao (`/mode` continua valendo, mas nunca acima do teto); a recusa diz
  se foi o modo ou a politica de acesso de quem fala.
- **Principais.** `dono` (`owners` ou `role: owner`, em conversa 1:1: sem
  teto; em `isolated-pod`, piso `code`), `usuario` (`allow` ou
  `access.users`), `pareado` (codigo do `/pair`: teto `read`, credencial
  fraca), `desconhecido` (so com `admission: open`, com o `default`),
  `grupo` (a politica e do grupo — o dono no grupo e o grupo), `bloqueado`
  (`blocked: true`: recusado mesmo em `open`, mesmo em `allow`, mesmo
  pareado). Cada turno loga `principal` e `alcance` (etiquetas fixas, nunca
  identidade).
- **Compatibilidade.** Sem `access:` nada muda: `allow` e usuario **sem
  teto** (o piso `default_mode` decide, como sempre), `owners` e dono,
  `reply_in_groups` liga grupos. Nivel e `write` so existem onde foram
  declarados; `access.users` vence o legado para a mesma identidade. As
  chaves de `users` aceitam qualquer grafia do numero (a comparacao e a
  mesma do `allow`, nono digito incluso) ou o JID `@lid`.
- **Fail-closed.** Nivel desconhecido vale `chat`; `chat` com `write: true`
  vale `chat`; `admission` desconhecida vale `restricted`; o `default` de
  `open` nunca chega a `full`. Cada normalizacao gera um aviso sem numero no
  log do boot (e uma vez por mudanca da config viva).
- **Pela CLI.** `garraia whatsapp access` mostra a matriz efetiva; `access
  open|restricted`, `access default`, `level`, `write`, `block`/`unblock`,
  `access groups`/`group` e `access reset` mudam a politica pelo **mesmo
  caminho** que a API admin e o Web Console usam
  (`whatsapp_linked_politica::mutacao`): validacao antes de gravar (`full`
  para desconhecido e `chat` com `write` sao recusados com exit 65), escrita
  atomica `0600`, e cada mudanca aplicada vai para
  `<data_dir>/audit/whatsapp-access.jsonl` (quando, quem, por onde, acao,
  alvo `…1234`, resumo antes/depois — nunca identidade inteira, chave ou
  mensagem; rotacao por tamanho, `audit_max_bytes` na secao). Toda mutacao
  aceita `--dry-run`: imprime o que mudaria e o **impacto por principal**
  (o que ganha e perde: escrita de arquivo, shell, dispositivo, mensagem,
  MCP), calculado pelo motor real, sem gravar nem auditar. `access audit`
  le a trilha.
- **Pela API admin (e o Web Console).** `GET /admin/api/whatsapp/access`
  devolve o mesmo documento de `access --json` (mais `hot_reload`);
  `POST /admin/api/whatsapp/access` com `{ "action": "level", "identity":
  "+55...", "level": "read", "dry_run": true }` (acoes: `open`, `restricted`,
  `default`, `level`, `write`, `block`, `unblock`, `groups`, `group-default`,
  `group`, `reset`) aplica pelo mesmo motor e devolve `changed`, `changes`,
  `impact` (o que cada principal ganha e perde), `audit` e a politica
  resultante; `GET /admin/api/whatsapp/access/audit?limit=N` le a trilha.
  Cookie do `/admin` + CSRF; leitura para `viewer`, mutacao para quem tem
  `Channels/Update`. A API nunca revela identidade. O `/api/diagnostics`
  (`whatsapp.access`) avisa quando a admissao esta `open` e quando a secao
  tem valor invalido.
- **Pelo Web Console.** A pagina **WhatsApp Access** (`/admin` → sidebar)
  mostra o resumo, a admissao, o default do desconhecido, os grupos, o
  formulario "Add phone / identity" e a matriz por principal (piso, nivel,
  write, o que pode de fato), com seletor de nivel, toggle de write,
  Block/Unblock, Make owner/Demote, Remove e "Reset to safe defaults" — e
  a trilha de audit. **Toda mudanca passa por um preview** (as mudancas e
  o que cada principal ganha e perde, calculados pelo motor) antes de
  confirmar; `open`, owner e reset trazem aviso. A pagina so conhece
  `…1234`: para agir numa linha manda `identity_last4`, que o gateway
  resolve entre as identidades declaradas (ambiguo = 409). A API tambem
  aceita `owner`, `unowner` e `remove`.
- **A quente.** A secao inteira e relida a cada mensagem (como `allow` e
  `owners` ja eram): um `blocked: true` vale na mensagem seguinte, sem
  restart. `access.groups.enabled` tambem; o `reply_in_groups` legado segue
  sendo lido no boot.

### Confirmacao de ferramenta perigosa ("sim")

Quando uma ferramenta pede confirmacao (o `bash` num comando arriscado com
`agent.tool_confirmation_enabled`, um `device_execute` R3/R4), o turno pausa e
o pedido chega como mensagem. Desde a v0.4.5 (#1343) responder **`sim`** (ou
`yes`, `ok`, `confirma`, `confirmar`, `proceed`, `approve` — a mensagem
inteira, nada mais) na mensagem seguinte roda o pedido, **uma vez**. Antes
disso o "sim" nunca aprovava: o historico deste canal e guardado como texto,
o pedido pausado nao voltava no turno seguinte, e a ferramenta perguntava de
novo para sempre.

- So aprova **quem recebeu o pedido**, na **mesma conversa**. Em grupo a
  conversa e o grupo e o remetente e quem falou: o "sim" de outro membro nao
  aprova — e **encerra** o pedido (fail-closed), entao o dono tem de pedir de
  novo.
- Qualquer outra mensagem no meio (inclusive "nao") encerra o pedido.
- O pedido vale **5 minutos** e **uma vez**: um segundo "sim" pausa de novo.
- Reiniciar o gateway cancela todo pedido pendente (ele vive em memoria, com
  uma chave que muda a cada processo).
- A ferramenta ainda precisa passar pelo piso do turno: em `search` o `bash`
  nem e oferecido, entao nao ha o que confirmar.

### `logout`

`garraia whatsapp logout` sobrescreve e remove `session.enc`, `session.enc.prev`,
`session.key` e `session.salt`, e grava `enabled = false` na config. A
sobrescrita e best-effort: em SSD com wear leveling ela nao garante que os bytes
sumiram do meio fisico.

O aparelho **continua listado no celular** ate voce remove-lo em
*Configurações → Aparelhos conectados*.

### `restore`

`garraia whatsapp restore` renomeia `session.enc.prev` de volta para
`session.enc`, aperta o modo do arquivo para 0600 (um `.prev` restaurado de
backup pode ter chegado frouxo) e grava `enabled = true` na config — nessa
ordem, a mesma do `link`, porque um `enabled` sem sessao faz o gateway pagar
timeout e retry a cada boot.

Ele recusa, com 69, quando nao ha arquivada ou quando ja ha `session.enc`: a
sessao em uso e a que o servidor conhece, e substitui-la as cegas trocaria um
problema por outro.

## O que precisa de Node

Só o caminho do QR. `node` e `npm` sao procurados na `PATH`; faltando qualquer
um, o comando sai 69 com o link de instalacao e lembra que a opcao 2 (Cloud API)
nao precisa de Node.

Na primeira execucao o GarraIA materializa a ponte em
`<data_dir>/whatsapp/bridge/` (0700) e roda `npm ci --no-fund --no-audit`.
`npm ci` e nao `npm install`: o `package-lock.json` e versionado e o pin exato
do Baileys faz parte do contrato — e a mesma arvore que o CI audita.

Uma atualizacao do `garraia` que traga uma ponte nova vale **no proximo boot
do gateway**, sem vincular de novo: antes de lancar a ponte, o supervisor do
canal regrava so os arquivos que diferem dos embutidos no binario. Mudou so o
`bridge.mjs`, nao ha `npm`. O `npm ci` so roda quando falta `node_modules` ou
quando nada prova que a arvore instalada e a do `package-lock.json` embutido
(um bump de versao por CVE nao fica parado atras de um `node_modules` antigo).
Duas coisas provam:

- o carimbo `.garraia-deps-sha256`, que o GarraIA grava quando o `npm ci` dele
  sai 0 — e que vira `pending` **antes** de um manifesto ser reescrito e antes
  de cada `npm ci`, entao um `npm ci` que falhou ou foi interrompido nunca deixa
  uma arvore pela metade passar por atual;
- o registro do proprio npm, `node_modules/.package-lock.json`, que o npm 7+
  grava por ultimo ao terminar uma instalacao: se ele lista exatamente os
  pacotes do lock embutido (mesma versao, mesma `integrity`; so os `optional`
  de outra plataforma podem faltar) e nao e mais velho que o `node_modules`, a
  arvore e adotada sem `npm`, e o carimbo e gravado. E o que faz uma instalacao
  da 0.4.4 (que nao tinha carimbo) e um `npm ci` rodado a mao valerem.

Se o `npm` do gateway falhar, a ponte **nao sobe** (nunca contra dependencias
de outra versao) e o `node_modules` que esse `npm ci` deixou pela metade sai do
disco. Se nao houver `npm` na `PATH` do gateway (um servico do systemd, por
exemplo), a ponte tambem nao sobe, mas **nada e apagado**: a arvore no disco
nao foi o gateway que tentou instalar. Nos dois casos o
`garraia whatsapp status` e o `/api/diagnostics` mostram "sem dependencias" com
o passo `rode npm ci em <dir> e reinicie o gateway` — e o passo funciona: rode
`npm ci` naquele diretorio, num shell que tenha o `npm`, e reinicie o gateway,
que adota a arvore no boot. A sessao vinculada nao e tocada. O
`garraia whatsapp link` segue a mesma regra.

Como o filho e contido:

- `env_clear()` + a allowlist R3 (`PATH`, `HOME`, `LANG`, `LC_ALL`, `TERM`,
  `USER`) mais `TMPDIR`/`TEMP`/`TMP`, `LANGUAGE` e as demais `LC_*`. **Nenhum
  segredo deste processo chega ao Node** — nem chave de LLM, nem `GARRAIA_JWT_SECRET`,
  nem a passphrase do cofre.
- `PDEATHSIG` no Linux/Android: gateway morto de `SIGKILL` nao deixa Node orfao.
- `kill_on_drop` e um `Drop` que mata o filho, inclusive no caminho de panic.
- **Sem `RLIMIT_AS`**, ao contrario dos servidores MCP: o V8 reserva uma "cage"
  de varios GiB de memoria *virtual* no boot, entao esse teto mata o Node com
  `SIGABRT` antes de ele fazer qualquer coisa. Medir memoria real exigiria cgroup.

## Troubleshooting

Comece por `garraia doctor whatsapp`: ele percorre o caminho inteiro (vinculo,
chave, gateway e ponte, acesso, perfil, workspace, MCP, provider) e cada linha
vermelha ou amarela traz o proximo passo. Com o gateway de pe, a ponte e o
provider sao o que o `/api/diagnostics` diz; sem ele, a linha diz que nao sabe
em vez de inventar. `--json` para scripts; nenhuma linha carrega numero, LID,
chave ou URL com credencial.

| Sintoma | O que fazer |
|---|---|
| **QR sai embaralhado / quadrado** | O terminal precisa de **pelo menos 60 colunas** e UTF-8. Abaixo disso o GarraIA imprime a string crua em vez de um QR que nao le. |
| **O QR expirou** | Normal: ele e regenerado ate 5 vezes, com `QR anterior expirou — novo QR (tentativa N/5)`. Depois da quinta, rode `garraia whatsapp` de novo. |
| **`node nao encontrado na PATH`** | Instale Node.js 20 ou mais novo (<https://nodejs.org/en/download>). So este caminho precisa dele. |
| **`o bridge esta sem dependencias instaladas`** / `status` diz `dependências faltando` | Rode `npm ci` no diretorio que a mensagem cita (num shell com `npm`) e reinicie o gateway, que adota a arvore no boot; ou rode `garraia whatsapp link` e responda nao ao re-vinculo: ele instala com o `npm` do seu shell, e depois reinicie o gateway, que so prepara o bridge uma vez por boot. |
| **`Esta sessão não vale mais`** / `status` diz nao vinculado | A sessao morreu (401/403/419) e o comando imprime o codigo cru do WhatsApp. Rode `garraia whatsapp` de novo e leia um QR novo. |
| **`status` diz `Leitura: FALHOU`** | A chave mudou: `GARRAIA_VAULT_PASSPHRASE` diferente, ou `session.key` perdida. Rode `garraia whatsapp` de novo. |
| **`whatsapp_linked: canal nao subiu — …`** no log do gateway | A frase depois do travessao e a acao: ligar `channels.whatsapp_linked.enabled`; corrigir `channels.whatsapp_linked.default_mode` para um modo nativo (`search`, `ask`, `code`…; nao `auto`, nao modo customizado, nao typo) ou remover a chave; rodar `garraia whatsapp link`; ou instalar Node.js 20+ e garantir `node` na PATH **do processo do gateway** (um servico systemd nao herda a PATH do seu shell). Canal desligado de proposito sai em `INFO`, nao aqui. |
| **`o perfil `…` (…default_mode) libera ferramentas MCP dos servidores …`** no log do gateway | Aviso de drift, nao erro: o `default_mode` do canal e um perfil nativo sem whitelist (`ask`, `code`) e esta expondo aqueles servidores MCP a quem manda mensagem. O trecho depois do travessao diz por que o portao liberou. Veja "Ferramentas e servidores MCP" acima. |
| **`execution.profile = isolated-pod` e o WhatsApp continua em `search`** | `owners` vazio (o diagnostico diz "0 donos"); a mensagem veio de grupo (grupo nunca herda); `default_mode` explicito na config (em `isolated-pod` vale para o dono tambem — remova a chave para o default `code`); ou a sessao escolheu `/mode`. Veja "Dono e perfil de execucao" acima e [`execution-profiles.md`](execution-profiles.md). |
| **`channels.whatsapp_linked.owners lists N identit… but the effective execution profile is standard`** no `config check` | `owners` so tem efeito em `isolated-pod`. Ou ligue o perfil (se este processo roda num pod descartavel), ou remova a chave. |
| **Vinculado, gateway de pe, e ninguem recebe resposta** | `garraia whatsapp status` diz `Autorizados: 0`: o portao esta vazio e toda mensagem e descartada em silencio. Rode `garraia whatsapp allow <numero>` com `+` e o codigo do pais (vale sem reiniciar se o gateway subiu com o canal ligado e com o `config.yml` ja no disco; senao `garraia restart`). Se `Autorizados` e maior que zero: o `status` avisa de remetente `@lid` sem numero recusado (veja "Contato por LID" acima); o numero que manda e o do proprio celular vinculado (ignorado, `from_me`); a conversa e um grupo sem `reply_in_groups`; o gateway subiu antes de o `link` ligar o canal (`garraia restart`); ou o `config.yml` editado a mao nao parseia (log `config reload failed`). |
| **Mensagem sobre outro aparelho ter assumido** | Alguem conectou o mesmo numero em outro lugar. A sessao gravada **continua valendo**; rode `garraia start` de novo. |
| **Conta bloqueada pelo WhatsApp** | Nao ha o que o GarraIA faca. Foi o risco avisado na tela de consentimento. Use a Cloud API. |

## Validacao manual (o que os testes automatizados nao cobrem)

O pareamento de ponta a ponta com um telefone real **nao e testado no CI** e nao
tem como ser: exige uma conta do WhatsApp e uma camera. O CI cobre o protocolo,
a maquina de estados, o store cifrado, o desenho do QR e o ciclo de vida do
processo filho contra uma ponte falsa em Python.

Antes de cada release que toque este caminho, rode a mao:

1. `garraia whatsapp` num terminal de ≥ 80 colunas, com um numero **secundario**.
2. Leia o QR. Confirme as tres linhas de sucesso.
3. `garraia whatsapp status` → `Vinculado: sim` e `Leitura: ok`.
4. Rode `garraia whatsapp` de novo → `✓ Sessao encontrada e valida`, sem QR.
5. Remova o aparelho no celular e rode `garraia whatsapp status` → deve falhar
   claramente, e nao dizer que esta tudo bem.
6. `garraia whatsapp logout` → o diretorio da conta fica vazio.

### O que JA foi exercitado contra o Baileys real (2026-09-17)

Estes tres pontos sairam de rodar a ponte de verdade (`@whiskeysockets/baileys`
`7.0.0-rc14`, Node 22.22.2) num ambiente **sem alcance** aos servidores do
WhatsApp. Ficam registrados para o roteiro acima nao reexaminar o que ja se
sabe, e para deixar explicito o que continua em aberto.

1. **A ponte sobe e carrega o Baileys.** `node bridge.mjs --protocol-check`
   devolve `started` com `protocol: 1` e a versao do Baileys.
2. **O blob de sessao faz round-trip byte a byte.** Um `session_update` emitido
   pelo Baileys real, devolvido a uma instancia nova via `session_load`, produz
   `log: "session restored"` e um `creds` **identico** ao de entrada
   (`registrationId`, `advSecretKey`, `noiseKey`, `signedIdentityKey` e o
   objeto inteiro comparados). E o contrato de persistencia da §Sessao valendo
   contra a biblioteca, e nao so contra a fixture Python.
3. **O caminho de rede ruim tem feedback, e nao so um teto.** Sem alcance ao
   WhatsApp, o Baileys emite `status connecting` aos 2 s e fica **85 segundos**
   mudo. Era nessa janela que o terminal exibia uma linha congelada; hoje o
   pulso de `PairUi::connecting` diz de 5 em 5 s ha quanto tempo tenta e em
   quanto desiste. Ver o teste `silence_before_the_first_qr_still_shows_the_user_something`
   e o cenario `quiet-before-qr` da fixture.

**Continua sem prova, e e o motivo de o roteiro acima existir:** nenhum QR real
apareceu, porque isso exige alcancar o WhatsApp. A fidelidade do Baileys aos
eventos `qr`, `connection.update` e `creds.update` contra o servidor de
verdade — e o comportamento nos codigos 401/403/419 e no `restart_required`
515 — segue dependendo dos 6 passos com um telefone.

## Textos (pt-BR / en)

O comando responde em pt-BR por padrao e em ingles quando `GARRAIA_LANG`,
`LC_ALL`, `LC_MESSAGES` ou `LANG` comecam com `en`. O resto da CLI segue em
pt-BR: a traducao existe aqui porque estas sao as frases que alguem le antes de
decidir se confia a propria conta ao GarraIA. O `garraia doctor whatsapp` segue
a mesma regra (toda linha e todo passo nas duas linguas) sem entrar na tabela
abaixo, que e das frases do pareamento.

| pt-BR | en |
|---|---|
| `Conectar meu WhatsApp pessoal (ler um QR code)` | `Link my personal WhatsApp (scan a QR code)` |
| `Conectar um WhatsApp Business (API oficial da Meta)` | `Connect a WhatsApp Business account (official Meta Cloud API)` |
| `Abra o WhatsApp no celular: Configurações → Aparelhos conectados → Conectar um aparelho` | `Open WhatsApp on your phone: Settings → Linked devices → Link a device` |
| `QR anterior expirou — novo QR (tentativa N/5)` | `Previous QR expired — new QR (attempt N/5)` |
| `✓ Autenticado. Sincronizando sessão…` | `✓ Authenticated. Syncing the session…` |
| `✓ WhatsApp conectado com sucesso.` | `✓ WhatsApp connected successfully.` |
| `Nenhum QR foi lido. Rode `garraia whatsapp` de novo.` | `No QR was scanned. Run `garraia whatsapp` again.` |
| `Número autorizado, com código do país (ex.: +55 11 99999-8888; vazio = ninguém por enquanto)` | `Authorized number, with the country code (e.g. +1 555 123 4567; empty = nobody for now)` |
| `⚠ Ninguém está autorizado a falar com o GarraIA por este WhatsApp — …` | `⚠ Nobody is authorized to talk to GarraIA through this WhatsApp — …` |
| `Autorizados: N · Donos: M` | `Authorized: N · Owners: M` |
| `Acesso em vigor neste WhatsApp (o mesmo que `garraia whatsapp users` mostra):` | `Access in effect on this WhatsApp (the same `garraia whatsapp users` shows):` |

### O que **nao** esta nas duas linguas

As frases da tabela acima sao as da CLI (`crates/garraia-cli`), que tem `t()`.
As mensagens de **erro do driver** — `RunError` e `BridgeError`, em
`garraia-channels` — saem sempre em pt-BR, inclusive com `Lang::En`: a
`garraia-channels` nao tem i18n, e por isso o prazo estourado, a ponte que
caiu e a cauda do `npm ci` chegam ao usuario em portugues nos dois idiomas.

E limitacao estrutural conhecida, nao esquecimento. Fecha-la e mapear os
`RunError` na CLI, onde o `t()` existe — trabalho de outra fatia.

## Estado da integracao

O comando **vincula e guarda a sessao**, e o gateway **consome o canal**: o
canal pull `whatsapp_linked` (`bootstrap/whatsapp_linked.rs`) sobe no boot
quando `channels.whatsapp_linked.enabled = true`, ha `session.enc` legivel e
`node` na PATH — e qualquer outro motivo de nao subir sai em `WARN` com a acao
(ver Troubleshooting). Quem entra e decidido pelo `allow` do canal (e
`owners`), relidos a quente a cada mensagem, mais o pareamento; o que pode fazer, pela secao "Ferramentas e servidores MCP". O
check `whatsapp.linked` em `/api/diagnostics` e o `/api/channels` leem o mesmo
estado do supervisor.

Decisao e alternativas avaliadas: [ADR 0023](adr/0023-whatsapp-dispositivo-vinculado.md).
