# WhatsApp no GarraIA

Existem **dois** jeitos de conectar o WhatsApp, e eles nao se misturam.

| | Numero pessoal (QR) | WhatsApp Business (Cloud API) |
|---|---|---|
| Comando | `garra whatsapp link` | `garra whatsapp cloud` |
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
garra whatsapp
```

1. Escolha a opção **1) Conectar meu WhatsApp pessoal (ler um QR code)**.
2. Leia a tela de aviso e confirme.
3. No celular: **Configurações → Aparelhos conectados → Conectar um aparelho**.
4. Aponte a camera para o QR no terminal.
5. Pronto:

```text
✓ Autenticado. Sincronizando sessão…
✓ WhatsApp conectado com sucesso.
✓ Sessão salva em ~/.config/garraia/data/whatsapp/default.
✓ GarraIA está pronto para receber mensagens (inicie o gateway: `garra start`)
```

Rodar `garra whatsapp` de novo com uma sessao valida **nao** repareia e **nao**
apaga nada: ele valida e responde `✓ Sessão encontrada e válida`.

> **Aviso.** Conectar pelo QR usa o recurso de "aparelho conectado" do WhatsApp
> por um cliente **nao oficial**, o que contraria os termos de uso da Meta. A
> conta pode ser bloqueada, temporaria ou permanentemente, sem recurso
> garantido. **Use um numero secundario.** Se voce precisa de suporte oficial,
> use a Cloud API (opcao 2).

## Comandos

| Comando | O que faz | Exit code |
|---|---|---|
| `garra whatsapp` | menu de duas opcoes | 0, ou 1 se cancelado |
| `garra whatsapp link` | vincula por QR | 0 · 1 cancelado · 69 sem Node / QR nao lido · 70 erro interno |
| `garra whatsapp cloud` | wizard da Cloud API | 0 · 1 cancelado · 70 erro interno |
| `garra whatsapp status` | diz se ha vinculo e se a sessao abre | 0 vinculado · 69 nao vinculado ou ilegivel |
| `garra whatsapp logout` | apaga a sessao e desliga o canal | 0 · 1 cancelado |
| `garra whatsapp restore` | devolve o `session.enc.prev` ao lugar | 0 · 69 nao ha arquivada, ou ha sessao em uso · 70 erro interno |

Os codigos seguem `sysexits` (69 = `EX_UNAVAILABLE`, 70 = `EX_SOFTWARE`), como
`garra desktop` e `garra config check`.

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
expiracao ele desiste com `Nenhum QR foi lido. Rode `garra whatsapp` de novo.`
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

**`session.enc.prev` tambem e uma credencial viva.** Ele nasce quando voce
responde "sim" ao re-vincular. Por isso `garra whatsapp status` o reporta mesmo
quando nao ha sessao ativa, e `garra whatsapp logout` o apaga — sem isso, os
dois comandos afirmariam que nao ha nada enquanto a credencial estivesse no
disco.

**E `garra whatsapp restore` o traz de volta.** Se um re-vinculo foi
interrompido de um jeito que nao deu ao GarraIA a chance de desfaze-lo — um
`kill -9`, uma queda de energia —, a sessao boa fica no `.prev` sem
`session.enc`. O `restore` a devolve ao lugar e religa o canal. Ele **nunca**
passa por cima de uma sessao em uso: nesse caso diz o que ha e sai 69, sem
apagar nada. Restaurar nao garante que o WhatsApp ainda aceite o aparelho — rode
`garra whatsapp status` depois.

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
| Outro usuario do sistema (UID diferente) | protegido pelo modo 0600/0700 | protegido pelo modo 0600/0700 |
| Root, ou seu proprio usuario | nao protegido | nao protegido |

Por isso o aviso aparece **na tela de consentimento**, antes de a pessoa
decidir, e de novo no `garra whatsapp status`:

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

### `logout`

`garra whatsapp logout` sobrescreve e remove `session.enc`, `session.enc.prev`,
`session.key` e `session.salt`, e grava `enabled = false` na config. A
sobrescrita e best-effort: em SSD com wear leveling ela nao garante que os bytes
sumiram do meio fisico.

O aparelho **continua listado no celular** ate voce remove-lo em
*Configurações → Aparelhos conectados*.

### `restore`

`garra whatsapp restore` renomeia `session.enc.prev` de volta para
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
do Baileys faz parte do contrato — e a mesma arvore que o CI audita. Uma
atualizacao do `garra` que traga uma ponte nova reescreve o diretorio sozinha
(o carimbo `.garraia-bridge-sha256` e quem detecta) **e reinstala as
dependencias**, para um bump de versao por CVE nao ficar parado atras de um
`node_modules` antigo.

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

| Sintoma | O que fazer |
|---|---|
| **QR sai embaralhado / quadrado** | O terminal precisa de **pelo menos 60 colunas** e UTF-8. Abaixo disso o GarraIA imprime a string crua em vez de um QR que nao le. |
| **O QR expirou** | Normal: ele e regenerado ate 5 vezes, com `QR anterior expirou — novo QR (tentativa N/5)`. Depois da quinta, rode `garra whatsapp` de novo. |
| **`node nao encontrado na PATH`** | Instale Node.js 20 ou mais novo (<https://nodejs.org/en/download>). So este caminho precisa dele. |
| **`o bridge esta sem dependencias instaladas`** | Rode `npm ci` no diretorio que a mensagem cita, ou apague o diretorio e rode `garra whatsapp` de novo. |
| **`Esta sessão não vale mais`** / `status` diz nao vinculado | A sessao morreu (401/403/419) e o comando imprime o codigo cru do WhatsApp. Rode `garra whatsapp` de novo e leia um QR novo. |
| **`status` diz `Leitura: FALHOU`** | A chave mudou: `GARRAIA_VAULT_PASSPHRASE` diferente, ou `session.key` perdida. Rode `garra whatsapp` de novo. |
| **Mensagem sobre outro aparelho ter assumido** | Alguem conectou o mesmo numero em outro lugar. A sessao gravada **continua valendo**; rode `garra start` de novo. |
| **Conta bloqueada pelo WhatsApp** | Nao ha o que o GarraIA faca. Foi o risco avisado na tela de consentimento. Use a Cloud API. |

## Validacao manual (o que os testes automatizados nao cobrem)

O pareamento de ponta a ponta com um telefone real **nao e testado no CI** e nao
tem como ser: exige uma conta do WhatsApp e uma camera. O CI cobre o protocolo,
a maquina de estados, o store cifrado, o desenho do QR e o ciclo de vida do
processo filho contra uma ponte falsa em Python.

Antes de cada release que toque este caminho, rode a mao:

1. `garra whatsapp` num terminal de ≥ 80 colunas, com um numero **secundario**.
2. Leia o QR. Confirme as tres linhas de sucesso.
3. `garra whatsapp status` → `Vinculado: sim` e `Leitura: ok`.
4. Rode `garra whatsapp` de novo → `✓ Sessao encontrada e valida`, sem QR.
5. Remova o aparelho no celular e rode `garra whatsapp status` → deve falhar
   claramente, e nao dizer que esta tudo bem.
6. `garra whatsapp logout` → o diretorio da conta fica vazio.

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
decidir se confia a propria conta ao GarraIA.

| pt-BR | en |
|---|---|
| `Conectar meu WhatsApp pessoal (ler um QR code)` | `Link my personal WhatsApp (scan a QR code)` |
| `Conectar um WhatsApp Business (API oficial da Meta)` | `Connect a WhatsApp Business account (official Meta Cloud API)` |
| `Abra o WhatsApp no celular: Configurações → Aparelhos conectados → Conectar um aparelho` | `Open WhatsApp on your phone: Settings → Linked devices → Link a device` |
| `QR anterior expirou — novo QR (tentativa N/5)` | `Previous QR expired — new QR (attempt N/5)` |
| `✓ Autenticado. Sincronizando sessão…` | `✓ Authenticated. Syncing the session…` |
| `✓ WhatsApp conectado com sucesso.` | `✓ WhatsApp connected successfully.` |
| `Nenhum QR foi lido. Rode `garra whatsapp` de novo.` | `No QR was scanned. Run `garra whatsapp` again.` |

### O que **nao** esta nas duas linguas

As frases da tabela acima sao as da CLI (`crates/garraia-cli`), que tem `t()`.
As mensagens de **erro do driver** — `RunError` e `BridgeError`, em
`garraia-channels` — saem sempre em pt-BR, inclusive com `Lang::En`: a
`garraia-channels` nao tem i18n, e por isso o prazo estourado, a ponte que
caiu e a cauda do `npm ci` chegam ao usuario em portugues nos dois idiomas.

E limitacao estrutural conhecida, nao esquecimento. Fecha-la e mapear os
`RunError` na CLI, onde o `t()` existe — trabalho de outra fatia.

## Estado da integracao

O comando **vincula e guarda a sessao**. O canal ainda **nao e consumido pelo
gateway**: receber e responder mensagens entra no slice seguinte, que liga o
canal pull `whatsapp_linked`, os `channel_gates` (allowlist + pairing) e o check
`whatsapp.linked` em `/api/diagnostics`.

Decisao e alternativas avaliadas: [ADR 0023](adr/0023-whatsapp-dispositivo-vinculado.md).
