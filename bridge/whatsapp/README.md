# `bridge/whatsapp` — ponte Node/Baileys do GarraIA

Processo Node de vida curta que o Rust spawna para falar com o WhatsApp Web
como **dispositivo vinculado** (linked device). Nao e crate, nao e membro do
workspace Cargo e nao tem servidor HTTP: fala **NDJSON v1 por stdin/stdout** e
so isso.

A divisao de trabalho e deliberada:

| quem | faz |
|---|---|
| esta ponte | protocolo WhatsApp (Baileys), socket, reconexao |
| o Rust | desenha o QR, cifra e grava a sessao, aplica allowlist, roteia para o agente |

Slice S4a do ADR 0023. O wiring da CLI e do gateway vem no PR companheiro —
hoje nada no workspace Rust consome este diretorio.

## Protocolo

O contrato completo (tabela de eventos, tabela de comandos, codigos de saida) e
o do **NDJSON v1** descrito no ADR 0023. Resumo operacional:

- uma mensagem JSON por linha, UTF-8, `\n`, campo `type` obrigatorio;
- linha acima de **256 KiB** e erro de framing;
- ordem estrita: `started` (bridge) -> `session_load` (Rust) -> `start` (Rust);
- codigos de saida: `0` normal - `1` erro fatal - `2` `logged_out` - `3` erro de
  protocolo.

Eventos bridge -> Rust: `started`, `qr`, `status`, `authenticated`, `connected`,
`disconnected`, `logged_out`, `session_update`, `message`, `sent`, `error`,
`log`.
Comandos Rust -> bridge: `session_load`, `start`, `send`, `read`, `typing`,
`logout`, `shutdown`.

`scripts/protocol-lint.mjs` e a definicao executavel disso: le NDJSON no stdin e
valida tipo, campos obrigatorios, enums, ordem e `seq`. E o mesmo lint que
valida a ponte real e a fixture Python — se as duas passam, as duas falam o
mesmo protocolo.

## Rodando na mao (debug)

```bash
cd bridge/whatsapp
npm ci

# 1. So o handshake: imprime `started` e sai 0, sem tocar a rede.
#    E o que `garra whatsapp status` / doctor usa.
node bridge.mjs --protocol-check

# 2. Pareamento de verdade (precisa de telefone e de saida para o WhatsApp):
printf '{"type":"session_load","session":null}\n{"type":"start","mode":"pair"}\n' | node bridge.mjs

# 3. Validando o fluxo contra o protocolo:
node bridge.mjs --protocol-check | node scripts/protocol-lint.mjs
```

O QR sai como evento `qr` com a string crua que o Baileys entrega — **a ponte
nunca desenha nada**. Para olhar com o olho humano, jogue o campo `data` num
renderizador de QR qualquer.

Sem telefone nao da para parear: use a fixture determinista em
`crates/garraia-channels/tests/fixtures/fake_whatsapp_bridge.py`, que fala o
mesmo protocolo em milissegundos.

```bash
python3 ../../crates/garraia-channels/tests/fixtures/fake_whatsapp_bridge.py \
    --scenario pair-ok --qr-expires 1 | node scripts/protocol-lint.mjs
```

Cenarios: `pair-ok`, `pair-expire-then-ok`, `logged-out`, `crash-after-qr`,
`network-flap`, `hang`, `serve-echo`.

## Testes

```bash
npm test          # node --test, sem dependencia de teste nenhuma
```

Nenhum teste abre socket de rede: `createBridge()` recebe o `makeWASocket` por
injecao (`socketFactory`), e os testes passam um socket de mentira. A suite
cobre framing, round-trip do estado de auth, redacao, mapeamento de motivo de
queda, agenda de backoff, o fluxo inteiro (QR -> pareado -> conectado -> envio
-> queda -> logout) e a paridade com a fixture Python.

## Seguranca

- **Nunca escreve no disco.** O estado de autenticacao vive em memoria
  (`makeMemoryAuthState`, equivalente ao `useMultiFileAuthState` oficial menos o
  disco) e sai por `session_update` — snapshot COMPLETO, base64 de JSON
  serializado com `BufferJSON.replacer`. Quem grava, cifra e faz rotacao e o
  Rust (`CredentialVault`, AES-256-GCM). Um teste varre o proprio fonte atras de
  `writeFile`/`mkdir`/`useMultiFileAuthState` para que isso continue verdade.
- **stdout e exclusivamente NDJSON.** Sem banner, sem QR desenhado, sem
  `console.log` (tambem vigiado por teste de fonte). O logger do Baileys e um
  `pino` em nivel `silent` apontado para o **fd 2**: mesmo que uma versao futura
  suba o nivel sozinha, o texto sai no stderr e nao contamina o fluxo.
- **Redacao.** Todo evento `log` passa por `redact()`: JIDs viram
  `***1234@dominio` e qualquer corrida de 6+ digitos vira `***1234`. Mensagem de
  excecao vai por `describeError()`, que extrai so nome e texto — um `Boom` do
  Baileys carrega `data` arbitrario e nunca e serializado. Material de auth nunca
  entra num `log`.
  Os eventos `message` e `connected` carregam JID inteiro **de proposito**: e o
  Rust que precisa dele para a allowlist. Redacao vale para diagnostico.
- **Allowlist e do Rust.** A ponte entrega todo `message` que chega; quem decide
  responder e o gateway. Uma unica camada de politica, num lugar so.

## Politica de versao

`@whiskeysockets/baileys` e pinado em **versao exata** (hoje `7.0.0-rc14`, a
dist-tag `latest`) e reportado em `started.baileys_version`, junto com
`protocol: 1`. Um teste de fonte recusa `^`/`~` em qualquer dependencia.

O Baileys nao tem release estavel 7.x: a linha `7.0.0-rc*` e o que o projeto
publica como `latest` (a `legacy` e a 6.7.x). Subir de rc e uma mudanca
deliberada — o protocolo do WhatsApp Web muda por baixo e uma versao velha
simplesmente para de conectar, entao a rotina e: subir o pin, rodar `npm test`,
e **parear de verdade com um telefone** antes de mergear. `npm ci` e obrigatorio
no CI para que o lock mande.

`sharp` aparece em `node_modules` porque e peer dependency nao-opcional do
Baileys (thumbnail de midia). A ponte nao envia midia em v1 e nao importa
`sharp` em lugar nenhum.

## Limitacoes conhecidas da v1

- **`@lid` nao e resolvido para telefone.** Quando o remetente vem como
  `<id>@lid`, `sender_phone` sai `null` e o `sender_jid` vai cru para o Rust.
  Mapear LID -> telefone exigiria consultar o mapa interno do Baileys, que muda
  entre versoes; a decisao fica para o Rust.
- **Legenda de midia nao e entregue.** Midia vira `text: null` +
  `media_kind`, conforme o protocolo. Download de midia tambem nao existe.
- **Sem historico.** `syncFullHistory: false`: a ponte e um dispositivo
  vinculado que trata o que chega dali para frente, nao um arquivo de conversas.
