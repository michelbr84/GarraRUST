# GarraIA — Direitos dos titulares: endpoints e procedimento

> **O que este documento e:** o mapa entre os direitos do titular (LGPD art. 18
> e art. 20; GDPR arts. 15 a 20) e o que o codigo do GarraIA **faz hoje**,
> mais o procedimento **manual** para o que a automacao ainda nao cobre.
>
> Cada afirmacao esta marcada:
>
> - **[CODIGO]** — ja funciona, ha endpoint ou worker, com o arquivo citado;
> - **[MANUAL]** — depende de pessoa; o procedimento esta aqui;
> - **[PENDENTE]** — conhecido, nao implementado, com o motivo.
>
> **Instalacao self-host e o default.** Nesse modelo quem opera a instalacao e
> o controlador dos dados e **e a quem o titular enderaca o pedido** — o
> projeto GarraIA nao tem acesso a esses dados. Ver `docs/legal/privacy-policy-draft.md` §1.
> Este documento e o manual do operador.

## 1. Os quatro endpoints

Todos em `crates/garraia-gateway/src/rest_v1/me.rs`, todos autenticados por
JWT (`Principal`), nenhum exigindo `X-Group-Id` — eles sao de conta, nao de
grupo.

| Direito | Endpoint | Estado |
|---|---|---|
| Acesso e portabilidade (LGPD art. 18 II/V, art. 20; GDPR arts. 15 e 20) | `GET /v1/me/export` | **[CODIGO]** |
| Correcao (LGPD art. 18 III; GDPR art. 16) | `PATCH /v1/me` | **[CODIGO]** parcial — ver §3 |
| Anonimizacao (LGPD art. 18 IV, art. 12; GDPR art. 4(5)) | `POST /v1/me/anonymize` | **[CODIGO]** |
| Eliminacao (LGPD art. 18 VI; GDPR art. 17) | `DELETE /v1/me` + worker | **[CODIGO]** — ver §4 |

> A rota de exportacao e `/v1/me/export`, **com barra**. Nao ha alias sem
> barra; cliente que chamar `/v1/meexport` recebe 404.

## 2. `GET /v1/me/export` — o que sai e o que nao sai

**[CODIGO]** Devolve um JSON com `Content-Disposition: attachment`
(`garraia-export-<data>.json`), `schema_version: "2"`. Emite
`account.data_exported` no audit.

### Esta no arquivo

| Secao | Conteudo | Escopo |
|---|---|---|
| `profile` | `user_id`, `display_name`, `email`, `status`, data de criacao | conta |
| `sessions` | id, `device_id`, expiracao, criacao | conta |
| `api_keys` | id, label, scopes, criacao, revogacao — **hash nunca** | conta |
| `audit_events` | eventos de conta do titular (teto 1000) | conta |
| `group_memberships` | grupos, papel, data de entrada | conta |
| `messages` | mensagens **escritas pelo titular**, corpo completo | todos os grupos dele |
| `mentions_received` | @mencoes recebidas, **trecho de 200 caracteres** | todos os grupos dele |
| `files` | arquivos enviados: nome, MIME, tamanho, pasta — **metadado** | todos os grupos dele |
| `memory_items` | memorias criadas pelo titular, conteudo completo | pessoal + por grupo |
| `tasks` | tarefas criadas por ele e/ou atribuidas a ele, com `authored`/`assigned` | todos os grupos dele |
| `email_change_requests` | pedidos de correcao de e-mail dele | conta |
| `deletion_requests` | pedidos de eliminacao, **com a data `purge_after`** | conta |
| `truncated_sections` | secoes que bateram no teto — a exportacao esta **incompleta** nelas | — |
| `exclusions` | o que foi deixado de fora, e por que | — |

Tetos: 5.000 linhas por secao de conteudo, 1.000 no audit. Bater o teto **nao
e silencioso**: a secao e nomeada em `truncated_sections`. Exportacao
incompleta que se declara incompleta e honesta; incompleta em silencio seria
declaracao falsa de conformidade. Quando uma secao aparece ali, complete pelo
procedimento de §6.

### NAO esta no arquivo (e por que)

O proprio JSON carrega essa lista em `exclusions` — a funcao
`export_exclusions()` e a fonte unica, para o documento e o codigo nao
divergirem. Resumo:

- **mensagens escritas por outras pessoas** — mensagem que outro membro
  escreveu e dado pessoal **dele**, mesmo num chat do titular. Exportar a
  conversa inteira entregaria a uma pessoa uma copia do conteudo de todas as
  outras. Mensagem que **cita** o titular aparece em `mentions_received` como
  trecho de 200 caracteres — a mesma largura que `GET /v1/me/mentions` ja
  devolve, entao a exportacao nao amplia exposicao nenhuma;
- **bytes dos arquivos** — so metadado. O conteudo fica no object store e sai
  pelo endpoint de download, um por um. Mantem a resposta um documento JSON
  limitado em vez de um arquivo sem teto;
- **audit com `group_id`** — e o log de seguranca da organizacao, legivel
  pelos admins dela. So a fatia de conta do titular sai;
- **doc pages, blocos e versoes** — **[PENDENTE]** pagina de wiki e
  conhecimento conjunto e e editada por outros depois. As que ele escreveu
  estao em `GET /v1/me/doc-pages`; copia de documento editado a varias maos e
  pedido manual (§6);
- **material de credencial** — hash de senha e chave de API crua nunca saem de
  endpoint nenhum.

### Delimitacao por titular — como e garantida

**[CODIGO]** As consultas de conteudo rodam numa **unica transacao** que
reaponta `app.current_group_id` por grupo via `set_config(..., true)`,
iterando **so** os grupos onde o titular tem linha **ativa** em
`group_members`. Duas barreiras independentes:

1. **FORCE RLS** em todas as tabelas tocadas, com politica fail-closed: a GUC
   vazia passa por `NULLIF` e vira NULL, o que da zero linhas. Grupo do qual o
   titular nao participa nao e alcancavel;
2. **filtro explicito** pelo id do titular em cada statement
   (`sender_user_id = $1`, `created_by = $1`, ...).

Nenhuma das duas e usada sozinha. A transacao unica tambem da um snapshot
consistente: a exportacao nao mistura um estado de antes e um de depois de uma
escrita concorrente.

Teste de regressao: `crates/garraia-gateway/tests/rest_v1_me_data_subject.rs`
(cenarios cross-tenant) e a matriz
`crates/garraia-gateway/tests/authz_http_matrix.rs`.

## 3. `PATCH /v1/me` — correcao

**[CODIGO]** `display_name` e aplicado na hora.

**[CODIGO] + [MANUAL]** `email` **nao troca o e-mail**. Ele registra um pedido.

### Por que nao troca

`users.email` e a chave de login: `user_identities.provider_sub` carrega o
mesmo endereco para `provider = 'internal'` e `verify_internal` casa por ele.
Trocar com base so num token de acesso e primitiva de account takeover — quem
roubou um token aponta a conta para a propria caixa e captura todo fluxo de
recuperacao futuro. Entao posse da caixa nova tem de ser provada, e troca as
cegas **nao esta implementada, de proposito**.

Provar posse significa enviar link de confirmacao, e **este repositorio nao
tem infraestrutura de e-mail** (nenhum crate depende de SMTP, lettre ou
provedor). Em vez de inventar o envio ou pular a prova, o endpoint faz o
minimo honesto que o repo suporta:

1. grava linha `pending` em `user_email_change_requests` (migration 034);
2. emite `account.email_change_requested` no audit — com o **tamanho** do
   endereco pedido, nunca o endereco (e PII);
3. devolve `email_change_pending` no corpo e deixa `users.email` intacto.

No maximo um pedido aberto por titular (indice parcial UNIQUE); um segundo
`PATCH` **supersede** o primeiro, para o operador nao receber dois pedidos
concorrentes para desempatar.

**Sem checagem de duplicidade, de proposito:** o endpoint nao diz se o
endereco pedido ja pertence a outra conta. Responder isso transformaria a rota
em oraculo de enumeracao de contas para qualquer endereco que o atacante
quisesse testar. A colisao aparece para o operador na verificacao, onde nao
custa nada ao atacante e nao revela nada por HTTP.

### [MANUAL] Procedimento para aplicar uma correcao de e-mail

1. Liste os pedidos abertos:

   ```sql
   SELECT r.id, r.user_id, r.requested_email, r.requested_at, u.email AS email_atual
   FROM user_email_change_requests r
   JOIN users u ON u.id = r.user_id
   WHERE r.status = 'pending'
   ORDER BY r.requested_at;
   ```

2. **Verifique a posse da caixa nova fora de banda.** Pelo menos um de:
   envie um codigo para o endereco novo por um canal de e-mail que voce
   controle e peca que o titular o repita; ou confirme por um canal ja
   autenticado e distinto (o canal de mensageria onde a conta ja esta
   vinculada). **Nao aplique sem isso** — e a unica coisa que separa a
   correcao do takeover.
3. Verifique colisao: `SELECT 1 FROM users WHERE email = '<novo>';`. Se
   existir, **rejeite** (`status = 'rejected'`) e explique ao titular sem
   confirmar de quem e a conta.
4. Aplique nas **duas** tabelas, na mesma transacao — o login quebra se so uma
   mudar:

   ```sql
   BEGIN;
   UPDATE users SET email = '<novo>', updated_at = now() WHERE id = '<user_id>';
   UPDATE user_identities SET provider_sub = '<novo>'
     WHERE user_id = '<user_id>' AND provider = 'internal';
   UPDATE user_email_change_requests
     SET status = 'applied', resolved_at = now(), resolved_by = '<operador_user_id>'
     WHERE id = '<request_id>';
   COMMIT;
   ```

5. Revogue as sessoes ativas do titular (`UPDATE sessions SET revoked_at = now()
   WHERE user_id = '<user_id>' AND revoked_at IS NULL`) para forcar novo login
   com a identidade corrigida.
6. Avise o titular pelos **dois** enderecos, antigo e novo. O antigo e o que
   permite ao dono legitimo reclamar se a troca nao foi ele.

**[PENDENTE]** Verificacao automatizada por e-mail. Entra quando houver
infraestrutura de envio; o passo `pending -> applied` ja existe e nao muda o
contrato HTTP.

## 4. `DELETE /v1/me` — eliminacao definitiva

**[CODIGO]** Uma transacao faz tres coisas: marca `users.status = 'deleted'`,
revoga todas as sessoes ativas e **enfileira** o apagamento em
`account_deletion_requests` com `purge_after = now() + 30 dias`. Emite
`account.self_deleted` e `account.purge_scheduled`. Devolve 204; 409 se a
conta ja estava `deleted` ou `purged`.

Atomicidade importa: se a lapide commitasse sem a linha de pedido, a conta
pareceria apagada para sempre e nunca seria apagada de fato — e exatamente a
falha silenciosa que o "deferido a um worker futuro" deixava aberta.

### Carencia de 30 dias

**[CODIGO]** `DEFAULT_GRACE_PERIOD_DAYS` em
`crates/garraia-gateway/src/account_purge_worker.rs` (um teste falha se a
constante mudar sem este documento mudar junto).

Apagamento irreversivel imediato transformaria um token roubado em arma: um
pedido e nada se recupera. A carencia da ao titular legitimo janela para
reverter pelo suporte e ao operador janela para aplicar retencao legal antes
do ponto sem volta.

O titular le a data em `deletion_requests[].purge_after` da propria
exportacao. A resposta do `DELETE` continua 204 sem corpo de proposito —
incluir corpo quebraria o contrato publicado para os clientes existentes.

**[MANUAL]** Para **cancelar** dentro da carencia (pedido do titular
legitimo, autenticado por voce):

```sql
BEGIN;
UPDATE account_deletion_requests
  SET status = 'canceled', completed_at = now()
  WHERE user_id = '<user_id>' AND status = 'pending';
UPDATE users SET status = 'active', updated_at = now()
  WHERE id = '<user_id>' AND status = 'deleted';
COMMIT;
```

Depois de `purge_after`, ou com `status = 'completed'`, **nao ha cancelamento**
— o dado nao existe mais.

### O que o worker apaga, tabela por tabela

**[CODIGO]** `account_purge_worker.rs` chama as funcoes `SECURITY DEFINER` da
migration 034. O enunciado completo esta no cabecalho daquela migration;
aqui, o resumo:

| Tratamento | Tabelas |
|---|---|
| **DELETE** (ligacao ou credencial puramente pessoal) | `message_reactions`, `message_mentions`, `doc_page_mentions`, `task_assignees`, `task_subscriptions`, `chat_members`, `group_members`, `sessions`, `api_keys`, `tus_uploads` |
| **DELETE** (conteudo pessoal) | `memory_items` (+ `memory_embeddings` por CASCADE), `files` (+ `file_versions`, `message_attachments`, `task_attachments` por CASCADE) **e os blobs no object store** |
| **Redacao em lugar** (conteudo autoral em espaco compartilhado) | `messages.body`/`sender_label`, `task_comments.body_md`/`author_label`, `message_threads.title` (vira NULL — a thread fica, com as respostas de terceiros) |
| **Anonimizacao de rotulo** (`*_label` e copia do display_name = PII) | `files`, `folders`, `file_versions`, `tasks`, `task_lists`, `task_labels`, `doc_pages`, `doc_page_versions`, `message_attachments`, `task_attachments`, `task_activity` |
| **Desidentificacao parcial** | `audit_events`: `actor_label`, `ip`, `user_agent` viram NULL; a linha fica |
| **Token nao-identificavel** | `users.email`, `users.display_name`, `user_identities.provider_sub`, `group_invites.invited_email`, `user_email_change_requests.requested_email` |
| **Destruicao de credencial** | `user_identities.password_hash` vira NULL |

Dois tokens substituem o que sai: o corpo redigido vira
`[conteudo removido a pedido do titular]` e o rotulo vira
`Usuario Removido` — sempre os mesmos, para o apagamento ser verificavel por
consulta. `doc_page_versions.created_by` e UUID `NOT NULL` sem FK
(migration 028) e **fica** (aponta para a lapide); so o rotulo e redigido.

Ao fim, `users.status = 'purged'` e emite `account.purged` com o relatorio
**estrutural** (contagem por tabela, contadores de blob) — nunca o conteudo
apagado.

### Por que a linha de `users` sobrevive

**Nao e descuido.** `DELETE FROM users` e estruturalmente impossivel: cinco
FKs apontam para `users(id)` **sem** clausula `ON DELETE`, logo com semantica
`NO ACTION`, e o banco recusa o DELETE enquanto houver linha referenciando —
`messages.sender_user_id`, `chats.created_by`,
`message_threads.created_by`, `group_invites.created_by`,
`group_members.invited_by`.

Essas linhas vivem em espaco compartilhado: a mensagem do titular e a raiz da
thread onde outras pessoas responderam; o chat que ele criou e onde a equipe
conversa. Remove-las para atender um pedido individual apagaria contexto de
**terceiros**.

Entao "apagamento definitivo" aqui e: **conteudo destruido, identificadores
substituidos, credenciais destruidas, e a linha de `users` fica como lapide
sem nenhum dado pessoal.** O UUID remanescente nao e reversivel a uma pessoa
sem a linha de `users`, que ja nao guarda identificador nenhum — o que a LGPD
art. 12 e o GDPR art. 4(5) (e o considerando 26) tratam como dado nao pessoal.

### Por que o audit sobrevive

**Decisao consciente e conservadora** (GDPR art. 17(3)(b) e (e); LGPD art. 16,
I e IV): o audit e obrigacao legal/registro de seguranca **e** a unica prova
de que o apagamento foi pedido e executado. Apaga-lo junto destruiria a
evidencia do proprio direito exercido.

O meio-termo: a **linha** fica (`action`, `resource`, `created_at`), os
**identificadores dentro dela** saem (`actor_label`, `ip`, `user_agent` viram
NULL). `actor_user_id` fica, pseudonimo, apontando para a lapide. O `metadata`
dos eventos de workspace e estrutural por contrato do repo
(`audit_workspace_event` nunca escreve PII), por isso nao e tocado.

Retencao do audit: **[PENDENTE]** o projeto nao define prazo. Enquanto nao
definir, trate como "mantido pelo tempo da instalacao" e declare isso na
politica de privacidade.

### Sem object store configurado

**[CODIGO]** Se ha blob para remover e nenhum `ObjectStore` esta ligado, o
worker **nao fecha** o pedido: chama `fail_account_purge` e emite
`account.purge_failed`. Fechar como `completed` com blob vivo seria declarar
no audit um apagamento que nao ocorreu. Esgotadas as tentativas (5), o pedido
vira `failed`. Monitore:

```sql
SELECT id, user_id, attempts, last_error, requested_at
FROM account_deletion_requests
WHERE status = 'failed' ORDER BY requested_at;
```

Pedido em `failed` e **trabalho pendente de conformidade**: ligue o backend de
storage (ou remova os blobs a mao com as `object_keys` de
`purge_report->'object_keys'`) e devolva o pedido para a fila com
`UPDATE account_deletion_requests SET status = 'pending', attempts = 0 WHERE id = '<id>'`.

### Retomada: o pedido nunca fecha com blob vivo

**[CODIGO]** O apagamento e retomavel passo a passo. A migration 034 grava na
propria linha do pedido, **na mesma transacao dos deletes**, as marcas de
progresso: `purge_report->'object_keys'` (coletadas antes de apagar) e
`purge_report->>'db_purged'`. Se o processo cair depois do banco e antes da
remocao dos blobs, o proximo tick le as marcas, **pula a passada de banco** e
remove so os blobs pendentes. E o contrario tambem e verdade: sem a marca
`db_purged` persistida, a retomada reexecutaria os deletes, a segunda coleta
de chaves voltaria vazia, e o pedido fecharia como `completed` com o blob
vivo — a mentira exata que o teste de retomada em
`tests/rest_v1_me_data_subject.rs` impede.

### Como ligar o worker (configuracao do operador)

**[CODIGO]** O worker so nasce quando `GARRAIA_PURGE_DATABASE_URL` esta
definida. Sem ela o gateway sobe normalmente, `DELETE /v1/me` continua
enfileirando pedidos e o procedimento manual acima continua valendo — mas
**ninguem executa o apagamento automatico**, e o operador tem de rodar
`purge_account_data` a mao depois da carencia.

A URL autentica como o role dedicado `garraia_purge` (migration 034): o role
tem `EXECUTE` nas quatro funcoes `SECURITY DEFINER` de apagamento e **mais
nada** — nenhuma privilegio de tabela, nao e `BYPASSRLS`. Reutilizar aqui as
credenciais de `garraia_app`/`garraia_login`/`garraia_signup` derrubaria a
separacao que mantem a capacidade de apagacao fora das credenciais da
aplicacao, e o pool recusa qualquer role que nao seja `garraia_purge`
(validacao `SELECT current_user`, igual aos demais pools dedicados).

Provisioning (as migrations criam o role `NOLOGIN`; promova como os outros):

```sql
ALTER ROLE garraia_purge WITH LOGIN PASSWORD '<senha-forte-distinta>';
```

Depois defina no ambiente do gateway:

```
GARRAIA_PURGE_DATABASE_URL=postgres://garraia_purge:<senha>@<host>:5432/<banco>
```

Sem essa variavel o log emite um warn explicito dizendo que o worker nao vai
nascer — silencio la nao e opcao: um apagamento pedido e nunca executado tem
de ser visivel.

## 5. Protecao de terceiros e espacos compartilhados

O principio: **um pedido individual nao apaga nem exporta os dados de uma
organizacao.** Como isso aparece em cada direito:

- **Exportacao** — mensagem de outro membro no mesmo chat **nao sai**. Sai o
  que o titular escreveu, mais um trecho de 200 caracteres das mensagens que o
  citam;
- **Eliminacao** — mensagem de outro membro **nao e apagada nem redigida**.
  Fica no espaco compartilhado. O que sai e o conteudo do titular; a mensagem
  dele vira lapide para a thread de terceiros nao se partir;
- **Chats, grupos, listas de tarefa e paginas que o titular criou** — a
  **estrutura fica** (a equipe depende dela); o vinculo com a pessoa cai
  (`created_by` vira NULL, rotulo vira lapide);
- **Tarefa que outro criou e atribuiu ao titular** — a tarefa fica (e trabalho
  da organizacao); o que sai e a **atribuicao** (`task_assignees`). Na
  exportacao ela vem marcada `authored: false, assigned: true`, para o titular
  saber que aquilo nao e conteudo dele;
- **Anexo que o titular enviou** — e apagado, e **desaparece da mensagem de
  outra pessoa** que o citava. Consequencia assumida: o que sai e o **arquivo
  dele**, nao a mensagem dela. Nao ha como apagar o arquivo e manter o
  arquivo;
- **Convite que o titular enviou** — fica (e a trilha do grupo); o e-mail dele
  em `invited_email` vira token.

**[PENDENTE] Titular que e o unico `owner` de um grupo.** O apagamento remove
a linha de `group_members`, o que pode deixar um grupo sem dono. O indice
`group_members_single_owner_idx` garante no maximo um owner ativo, nao no
minimo um. Antes de executar um pedido, o operador deve verificar:

```sql
SELECT g.id, g.name FROM groups g
WHERE g.archived_at IS NULL
  AND EXISTS (SELECT 1 FROM group_members m
              WHERE m.group_id = g.id AND m.user_id = '<user_id>'
                AND m.role = 'owner' AND m.status = 'active')
  AND NOT EXISTS (SELECT 1 FROM group_members m2
                  WHERE m2.group_id = g.id AND m2.user_id <> '<user_id>'
                    AND m2.role = 'owner' AND m2.status = 'active');
```

Se vier linha, transfira a propriedade (ou arquive o grupo) **antes** de a
carencia vencer.

## 6. [MANUAL] Procedimento para pedidos que a automacao nao cobre

### 6.1 Quem tem conta no Garra

Mande usar os endpoints de §1 — e o caminho auditado e mais rapido. Use o
procedimento manual so para o que esta marcado **[PENDENTE]**: secao em
`truncated_sections`, copia de doc page editada a varias maos, ou
cancelamento dentro da carencia.

Identifique o titular pela sessao autenticada dele. Nunca por e-mail de
entrada sozinho: e-mail de origem e falsificavel e atender um pedido de
exclusao vindo de endereco forjado e, ele mesmo, um vazamento.

### 6.2 Quem NAO tem conta no Garra

**Este e o caso mais comum e o que nenhum endpoint atende.** Uma pessoa que
conversou com um bot do GarraIA no WhatsApp, Telegram, Discord ou Slack tem
dado pessoal dentro do sistema (o conteudo das mensagens dela, armazenado como
mensagem do canal) e **nao consegue autenticar** em `/v1/me/*` — ela nao tem
conta.

**[PENDENTE]** Nao ha automacao para esse caso. Procedimento:

1. **Canal de entrada.** O operador publica um endereco de contato na politica
   de privacidade e o mantem monitorado. Em instalacao self-host, e o endereco
   do **operador**, nao do projeto GarraIA — ver a nota de abertura deste documento e
   `docs/legal/privacy-policy-draft.md` §1. `[PENDENTE: o operador define o
   endereco e o encarregado/DPO]`;
2. **Identificacao.** Peca o identificador do canal pelo qual a pessoa falou
   com o bot (numero de telefone, handle, id de usuario) e **prove o
   controle**: mande um codigo por aquele mesmo canal e peca que ela o repita.
   O canal e a unica credencial que ela tem;
3. **Localizacao.** Busque as mensagens pelo identificador do canal nas
   tabelas de sessao/mensagem de canal (`garraia-db` em instalacao
   single-user; `messages`/`chats` no workspace quando o canal esta vinculado
   a um grupo). **[PENDENTE]** Nao ha consulta unica que cubra os dois
   armazenamentos; documente o que voce rodou junto da resposta;
4. **Resposta.** Para acesso, entregue a extracao pelo mesmo canal
   verificado. Para eliminacao, apague as mensagens dela e registre o pedido;
5. **Registre o pedido fora do audit da aplicacao** (uma planilha ou ticket do
   operador serve), com data, identificador verificado, o que foi feito e
   quando. Pedido de quem nao tem conta nao produz `audit_events`, porque nao
   ha `actor_user_id` — sem registro externo nao sobra prova de atendimento;
6. **Prazo.** LGPD art. 18 §6: resposta imediata para pedido simplificado, ou
   15 dias para a declaracao completa. GDPR art. 12(3): um mes, prorrogavel
   por dois.

### 6.3 Pedido que NAO deve ser atendido como esta

- **Pedido de apagar dado de terceiro.** "Apaguem as mensagens do meu colega"
  nao e direito de titular: recuse e explique. O colega pede por si;
- **Pedido de apagar um espaco compartilhado.** "Apaguem o grupo inteiro" de
  um membro que nao e dono: recuse. O pedido dele alcanca o que e dele (§5);
- **Pedido que destroi registro sob obrigacao legal.** "Apaguem o audit": o
  audit e desidentificado, nao apagado (§4).

## 7. Retencao e limitacoes conhecidas

| Item | Estado |
|---|---|
| `audit_events` sobrevive ao apagamento, desidentificado | **[CODIGO]**, justificado em §4 |
| Prazo de retencao do audit | **[PENDENTE]** — nao definido pelo projeto |
| **Reaplicar exclusoes em restauracao de backup** | **[PENDENTE]** — ver abaixo |
| Doc pages na exportacao e no apagamento | **[PENDENTE]** — §2 e §5 |
| Verificacao automatizada de e-mail | **[PENDENTE]** — §3 |
| Pedido de quem nao tem conta | **[MANUAL]** — §6.2 |
| Titular unico owner de um grupo | **[PENDENTE]** — §5 |

### [PENDENTE] Restauracao de backup reaplicando exclusoes

**Nao esta implementado e nao vamos fingir que esta.** Um backup tirado antes
de um apagamento contem o dado apagado. Restaurar esse backup **ressuscita**
dado que o titular mandou apagar, e hoje nada no repositorio reaplica a
exclusao depois de uma restauracao.

O que existe de material para construir isso: `account_deletion_requests`
guarda o registro duravel de **quem** pediu e **quando** (`user_id`,
`requested_at`, `status = 'completed'`), e `purge_account_data(uuid)` e
idempotente. Logo o procedimento manual ja e possivel e e **obrigatorio** por
enquanto:

**Depois de QUALQUER restauracao de backup**, antes de liberar o sistema:

```sql
-- 1. Quem ja tinha sido apagado:
SELECT user_id FROM account_deletion_requests WHERE status = 'completed';
-- 2. Para cada user_id, reaplique (a funcao e idempotente):
SELECT * FROM purge_account_data('<user_id>');
-- 3. Remova os blobs que a funcao devolver em object_keys.
```

Se o proprio `account_deletion_requests` veio do backup antigo e nao conhece
os pedidos posteriores, a lista tem de vir do registro externo do operador
(§6.2 passo 5) — **mais uma razao para manter esse registro fora do banco.**

**[PENDENTE]** Automatizar isso (um gatilho de post-restore que varre os
pedidos concluidos e reexecuta a purga) e trabalho conhecido e nao feito.

## 8. Onde fica o que, no codigo

| Arquivo | Papel |
|---|---|
| `crates/garraia-gateway/src/rest_v1/me.rs` | os quatro endpoints |
| `crates/garraia-gateway/src/account_purge_worker.rs` | worker do apagamento, carencia, remocao de blob |
| `crates/garraia-auth/src/purge_pool.rs` | `PurgePool`/`PurgeConfig`: pool dedicado do role `garraia_purge` (EXECUTE-only), validado por `SELECT current_user` |
| `crates/garraia-workspace/migrations/034_data_subject_rights.sql` | tabelas de pedido, `users.status = 'purged'`, as quatro funcoes `SECURITY DEFINER`, o role `garraia_purge` |
| `crates/garraia-auth/src/audit_workspace.rs` | `account.email_change_requested`, `account.purge_scheduled`, `account.purged`, `account.purge_failed` |
| `crates/garraia-gateway/tests/rest_v1_me_data_subject.rs` | testes dos comportamentos novos, incluindo delimitacao cross-tenant |
| `crates/garraia-gateway/tests/authz_http_matrix.rs` | matriz cross-group dos endpoints |

Documentos vizinhos: `docs/legal/privacy-policy-draft.md`,
`docs/legal/dpa-template.md`, `docs/compliance/dpia.md`.
