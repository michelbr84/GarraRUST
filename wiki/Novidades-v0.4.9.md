# Novidades da v0.4.9

> 🇬🇧 [English version](Whats-New-v0.4.9) · 📋 [CHANGELOG completo](https://github.com/michelbr84/GarraRUST/blob/main/CHANGELOG.md) · 📦 [Baixar](https://github.com/michelbr84/GarraRUST/releases/tag/v0.4.9)

Release de outubro de 2026, grande e de duas frentes.

A primeira são os **direitos dos titulares de dados**: exportação, correção de
e-mail, anonimização e apagamento deixam de ser promessa e viram caminho
funcional no gateway, cada um fechado o suficiente para não virar porta de
account takeover nem oráculo de enumeração.

A segunda é o **gateway como servidor MCP por HTTP** ganhando delegação de
tarefas: um orquestrador externo passa a perguntar ao LLM do Garra e a pedir uma
tarefa completa, com identidade própria, orçamento próprio e redação de
segredos. Em volta, uma varredura de advisories de dependências, o updater do
desktop com DMG e `latest.json`, e uma faixa de correções de confiabilidade.

---

## Direitos dos titulares saem do papel

Quatro endpoints de direitos do titular (`GET /v1/me/export`, `PATCH /v1/me`,
`POST /v1/me/anonymize`, `DELETE /v1/me`) passam a funcionar de ponta a ponta.

- **Exportação completa (`GET /v1/me/export`).** Antes devolvia só dado de
  conta; agora inclui o conteúdo pessoal do titular em todos os grupos dele —
  mensagens que escreveu (corpo completo), menções recebidas (trecho), arquivos
  enviados, memórias e tarefas — numa única transação que reaponta a GUC de
  tenant por grupo, com RLS fail-closed e filtro pelo id do titular nas duas
  pontas. O arquivo declara o que ficou de fora (`exclusions`) e o que bateu no
  teto (`truncated_sections`), porque exportação incompleta em silêncio é
  declaração falsa de conformidade.
- **Correção de e-mail sem troca às cegas (`PATCH /v1/me`).** `users.email` é a
  chave de login, então trocar com base só num token de acesso seria primitiva
  de account takeover. Como o repositório não tem infraestrutura de envio de
  e-mail, o pedido fica registrado com estado `pending` mais audit, e a
  aplicação é passo manual do operador, com verificação de posse fora de banda.
  O endpoint também não informa se o endereço pedido já existe — responder isso
  viraria oráculo de enumeração de contas.
- **Apagamento com carência e worker dedicado (`DELETE /v1/me`).** A mesma
  transação que marca a lápide e revoga as sessões enfileira o pedido com
  carência de 30 dias. O novo `account_purge_worker` destrói conteúdo e
  identificadores do titular (mensagem, comentário, memória, arquivo mais
  versões e blob no object store) e deixa a linha de `users` como lápide
  (`status = 'purged'`) — ela não é removida porque cinco FKs apontam para ela e
  removê-la apagaria contexto de terceiros; mensagem de outro membro nunca é
  apagada por pedido de um. O `audit_events` sobrevive desidentificado: é a
  única prova de que o apagamento aconteceu. **Sem object store ligado, o pedido
  NÃO é fechado** — declarar `completed` com blob vivo seria mentir no audit.

O worker roda com credenciais dedicadas e é retomável. O role `garraia_purge`
(migration 034) só tem `EXECUTE` nas quatro funções `SECURITY DEFINER` de
apagamento — nenhum privilégio de tabela, não é `BYPASSRLS`. O worker só nasce
se `GARRAIA_PURGE_DATABASE_URL` estiver definida; sem ela o log avisa e o
procedimento manual segue valendo. O progresso é persistido na própria linha do
pedido, na mesma transação dos deletes: uma queda no meio não reexecuta a passada
de banco nem fecha o pedido com blob vivo.

## O gateway MCP HTTP ganha delegação de tarefas

O `POST /mcp` já lia o estado e mandava mensagens. Agora um orquestrador externo
também pode perguntar ao LLM do Garra e pedir uma tarefa completa. Tudo é
**desligado por padrão** e tem orçamento próprio que nunca divide o de envio.

- **`garra_ask` por HTTP (#1612).** Só aparece com
  `gateway.mcp_http.allow_ask: true`, aceita só o modelo default do projeto
  enquanto `ask_allowed_models` estiver vazio, e tem `ask_budget_per_minute`
  (padrão 10). O núcleo do `garra ask` foi extraído para o crate `garraia-ask`,
  compartilhado pela CLI e pelo gateway.
- **Orquestradores com identidade própria (#1613).** `mcp_http.orchestrators[]`
  declara `nome` e `key_env` — o valor da chave nunca vai para a config. Cada
  orquestrador tem política própria (subconjunto de tools e destinos em
  interseção com a allowlist global), auditoria atribuível por chamada e redação
  de padrões de segredo no payload e no erro. Lista vazia mantém o comportamento
  de antes.
- **`garra_agent` por HTTP (#1615).** O orquestrador pede uma tarefa completa —
  ler e gravar arquivos dentro das raízes liberadas, buscar na web, ver o git e,
  **só com sandbox**, rodar comando — e recebe o envelope `garra.agent.v1` com o
  resumo de cada tool usada. Só aparece com `gateway.mcp_http.allow_agent: true`
  e com `agent` na lista de tools do orquestrador. Tem `agent_budget_per_minute`
  (padrão 2) e teto de tempo `agent_max_seconds` (padrão 300, máximo 600).
  Entrada e saída passam pela redação de segredos, e nenhum prompt, resposta ou
  saída de tool vai para o log.
- **Redação extensível pelo operador (#1635).** `gateway.mcp_http.redact_patterns`
  é opt-in: até 32 regex de até 512 caracteres, somadas aos padrões de segredo
  embutidos. Útil para mascarar um formato de token específico do seu ambiente
  no payload e no erro que atravessam a ponte.

O schema de `garra_agent` passa a anunciar o teto do operador: `timeout_secs`
sai com `maximum` e `default` derivados de `agent_max_seconds`, em vez de valores
fixos — antes o schema dizia que um valor valia e a recusa vinha depois; dois
testes de contrato passam a exercitar o `bash` por essa ponte de ponta a ponta
(#1626).

## Segurança: varredura de advisories de dependências (#1630)

A varredura de outubro fecha os advisories abertos no grafo:

- **`rustls` 0.23.45 → 0.23.46** (RUSTSEC-2026-0346 a 0349).
- **`quinn-proto` 0.11.16 → 0.11.18** (RUSTSEC-2026-0338 a 0342).
- **`wasmtime`/`wasmtime-wasi` 49.0.2 → 49.0.3** (RUSTSEC-2026-0352 a 0362),
  que inclui um escape do sandbox de filesystem ao renomear um symlink com barra
  final. O piso de MSRV segue **1.96**.
- **`chacha20` 0.10.0 → 0.10.2.**
- **`serde_yaml` trocado pelo fork mantido `serde_norway` 0.9.42**, de mesma
  API, para tirar do grafo o `unsafe-libyaml` arquivado (RUSTSEC-2024-0449).

No mesmo eixo de supply chain: cada asset da release passa a ganhar **atestação
de proveniência** verificável com `gh attestation verify` (#1606); o
`music-metadata` da ponte WhatsApp sobe para 11.16.1, fechando dois alertas do
Dependabot; e os oito alertas `rust/path-injection` de `uploads.rs` e
`session_workspace.rs` entram no ledger do CodeQL como **falso-positivo**,
auditados por entrada — o caminho nasce de um `Uuid` tipado ou de um digest
SHA-256 hex, nenhum dos dois expressa traversal.

## Confiabilidade e operação

- **Workers de uploads e de recorrência voltam a varrer (#1611).** Os dois
  adquiriam um advisory lock do Postgres numa conexão do pool e tentavam
  liberá-lo noutra — e advisory lock é por sessão, então o lock sobrevivia a
  todo tick e, a partir do primeiro, as réplicas ficavam **permanentemente**
  bloqueadas de varrer. Os dois passam a fixar uma conexão do acquire ao unlock,
  como o `account_purge_worker` já fazia. O teste de regressão achou um segundo
  defeito independente: a varredura de recorrência morria antes de varrer por um
  `bigint` que o Postgres não casava com a assinatura da função.
- **Timeout do LLM por provider (#1593).** `llm.<nome>.timeout_secs` sobrescreve
  a janela global para aquele provider — útil para um host lento sem derrubar os
  demais. E `0` agora significa de fato "sem timeout"; antes virava um sleep já
  vencido que fazia toda chamada LLM falhar em milissegundos.
- **Telegram: falha de rede deixa de virar "token inválido" (#1594).** O
  `connect()` tratava qualquer erro do `GetMe` como token inválido — sob
  Termux, uma falha de SSL/DNS mandava o operador trocar um token que estava
  certo. Agora o erro é classificado: a API respondendo falha de primeira;
  rede/IO re-tenta com backoff e aponta o diagnóstico de rede.
- **Allowlist do `bash` avalia o encadeamento segmento a segmento (#1592).** A
  allowlist negava QUALQUER comando com `|`, `&&`, `||` ou `;` — com `git *` e
  `grep *` liberados, um `git status | grep modificado` morria. Agora os
  separadores são quebrados em segmentos (varredura ciente de aspas) e a lista
  confere CADA um: casa só quando todos casam. Substituição de comando,
  redirecionamento e `&` solto seguem negando o comando inteiro.
- **`garraia doctor mcp` valida que o servidor MCP subiu (#1595).** O doctor
  conferia instalação, mas nunca o plano MCP: uma falha de subida passava
  despercebida. O novo subcomando spawna cada servidor stdio pelo mesmo código
  do boot do gateway, conta as tools do handshake e desliga os filhos; exit 0
  quando tudo subiu, **exit 2 quando algum não subiu**, útil em CI.
- **`garra update` e `garra rollback` não saem mais com exit 0 na falha
  (#1603).** A mensagem agora vai para stderr com a cadeia de erro completa e o
  processo sai com 70, no padrão de `config check`, `doctor` e `desktop`.
- **O `.env` é lido só do diretório atual (#1604).** O `dotenvy` subia os
  diretórios pais até achar um `.env`, e um arquivo na raiz do repo injetava
  config e credenciais em todo comando rodado de uma subpasta. Agora é
  `File::open` puro: o `.env` do pai deixa de valer.
- **`GARRAIA_CONFIG_DIR` inválida cai no padrão com aviso (#1620).** Um valor
  placeholder do `.env.example` ou vazio agora cai no config_dir padrão com
  aviso, em vez de derrubar qualquer comando — inclusive o `garra config check`
  — com um `EACCES` opaco.

## Desktop: DMG no macOS e updater com `latest.json` (#1568)

O desktop não era vendável como distribuição empacotada: não havia DMG no
macOS, o plugin de updater estava ligado com `pubkey` vazio e nenhum workflow
publicava `latest.json` — o botão de atualizar nunca funcionou. A v0.4.9 emite
o `.app.tar.gz` e o DMG, compila o DMG num job `build-macos-desktop` e passa a
gerar e publicar um `latest.json` assinado ao lado dos binários, no endpoint que
o updater do app já consultava. Um espelho em PR verifica o bundle no macOS
antes de existir tag. O item que ainda bloqueia o canal distribuidor — a
assinatura de código EV/OV — continua externo e rastreado.

## Qualidade e CI

- **O request-path do gateway nega `unwrap()`/`expect()` (#1569, primeira
  leva).** Dez módulos que recebem input externo ganham
  `#![deny(clippy::unwrap_used, clippy::expect_used)]`, com os dois liberados em
  teste. As ocorrências de produção viraram erro tipado ou constante checada em
  compile-time.
- **Fixture de Postgres destrava 10 testes de integração (#1570).** Um job novo
  sobe um `postgres:16.8-alpine` com as env vars que o bootstrap do gateway
  exige, e os dez `#[ignore]` de auth, integração e voz — que saíam em silêncio
  sem Postgres — passam a rodar de verdade no CI.
- **Dependências em bloco e testes flaky.** `tokio` 1.53.3, `serde_json`
  1.0.154, `uuid` 1.28, `opentelemetry` 0.33.1, Tauri 2.12.2 e plugins,
  Playwright 1.64 e o Dockerfile em `rust:1.99-slim`; mais dois testes flaky de
  CI corrigidos.

## Novo na CLI: `garra uninstall`

`garra uninstall` remove a CLI da máquina — binário, alias `garra`, wrappers do
Termux e backups `.old`/`.new`. `--purge` inclui config, dados e o legado
`~/.garraia`; `--all-binaries` varre o `PATH`. Recusa com exit 78 quando o
daemon pertence a uma unit systemd, e com exit 64 num pipe sem `--yes`.

## Documentos: limpeza e contribuição

- **Rascunhos legais e de precificação saem do repositório público (#1616).**
  Uma auditoria de exposição encontrou documentos marcados "RASCUNHO — NÃO
  PUBLICAR" e o modelo financeiro interno na `main`. Os arquivos deixam de ser
  versionados (agora cobertos pelo `.gitignore`); versões finais revisadas
  publicam em garraia.org. Histórico git e forks existentes não são afetados.
- **Domínio de contato corrigido para `garraia.org` (#1574).** Alguns caminhos
  apontavam para `garraia.cloud`, que não resolve. O domínio real do projeto é
  `garraia.org`, e o e-mail de segurança é `security@garraia.org`.
- **Guia de contribuição do usuário final (#1577).** Como rodar o pré-flight
  antes de abrir issue, qual template usar, onde pedir ajuda e por que
  vulnerabilidade nunca vai em issue pública. Linkado a partir do `SUPPORT.md`.
- **`channels.md` deixa de dizer que `whatsapp_linked` não existe no
  `config.yml` (#1605).** O canal de dispositivo vinculado tem entrada
  documentada e validação no `config check`; o texto antigo apontava para uma
  ausência que já não existia.

---

## Como atualizar

```bash
garra update            # CLI; guarda a versão anterior e aceita `garra rollback`
```

Instalação nova segue pelo [instalador de sempre](Instalacao-e-Primeiros-Passos).

## Limites conhecidos

- A troca de e-mail (`PATCH /v1/me`) e o fechamento do apagamento com blobs
  dependem de passos de operador: envio de e-mail e object store ligado não
  vêm montados por padrão.
- Instaladores do Windows e bundles de desktop continuam **não assinados**
  (Gatekeeper no macOS, SmartScreen no Windows); a assinatura de código EV/OV do
  canal distribuidor segue externa.
- A delegação por `garra_ask`/`garra_agent` no MCP HTTP é **desligada por
  padrão**; nada muda até o operador ligar e declarar um orquestrador.
