# ADR 0026 — Schema de billing: plan / subscription / invoice no workspace

- **Estado:** Accepted (decisao do dono 2026-10-22; escopo: SO o schema — plan/subscription/invoice, plano Free sem preco, RLS e testes)
- **Deciders:** @michelbr84 (dono) + Claude (implementacao)
- **Date:** 2026-10-22
- **Tags:** fase-3, billing, garra-cloud, workspace, rls, schema-only
- **Supersedes:** none
- **Superseded by:** none
- **Issue:** [#1652](https://github.com/michelbr84/GarraRUST/issues/1652)
- **Relacionadas (fora de escopo aqui):** #1653 (webhook Stripe/Pix), #1654 (entitlements), #1655 (metering/usage_ledger), #1656 (signup/billing page); #1576 (decisao de precos, fechada)
- **Links de schema:**
  - Migration: [`crates/garraia-workspace/migrations/035_billing_plan_subscription_invoice.sql`](../../crates/garraia-workspace/migrations/035_billing_plan_subscription_invoice.sql)
  - Padrao de RLS herdado: [`...migrations/014_tus_uploads.sql`](../../crates/garraia-workspace/migrations/014_tus_uploads.sql), [`...migrations/013_audit_events_policy_with_check.sql`](../../crates/garraia-workspace/migrations/013_audit_events_policy_with_check.sql)
  - Catalogo global sem RLS (precedente): [`...migrations/002_rbac_and_audit.sql`](../../crates/garraia-workspace/migrations/002_rbac_and_audit.sql) (`roles`/`permissions`)
  - Ledger append-only (precedente de grants): [`...migrations/034_data_subject_rights.sql`](../../crates/garraia-workspace/migrations/034_data_subject_rights.sql) (`account_deletion_requests`)
  - Decisoes relacionadas: [`0003-database-for-workspace.md`](0003-database-for-workspace.md), [`0005-identity-provider.md`](0005-identity-provider.md)

---

## Context and Problem Statement

O GarraRUST segue MIT; a monetizacao e do **Garra Cloud** (hospedagem gerenciada). O backlog minimo de billing (plano de monetizacao, revisao 02 de 09-10/2026, decisao do dono #1576) precisa de um lugar para registrar, por tenant, qual plano um grupo assina e o historico de faturas. Hoje nao existe nenhuma entidade de billing no schema do workspace (verificado por scanner na #1652): nenhuma tabela `plan`/`subscription`/`invoice`. Stripe existe so em prosa (ADR 0004).

O multi-tenancy real ja entrega o *locus* onde o billing mora: por `group_id`, sob `FORCE ROW LEVEL SECURITY` e o contrato `app.current_group_id` (migration 007). Falta so a fundacao de schema.

**Fronteira de produto (nao de schema, mas guia o desenho):** free/pago e *conveniencia, nunca poder* — nada que quebre o self-hosted MIT entra no pago. O Free e BYOK obrigatorio com limites duros. Nenhum numero de preco entra neste repositorio: unit economics e tiers vivem no repo privado `GarraIA/GarraIA` (`commercial/`).

**Escopo desta decisao (decisao do dono 2026-10-22):** SO a fundacao — ADR + migration das tres tabelas com o plano Free sem preco, RLS e testes. As outras quatro issues da sequencia (#1653 webhook, #1654 entitlements, #1655 metering, #1656 signup) ficam esperando Stripe/Pix e os valores dos planos. A obrigacao aqui e **nao fechar portas** a elas.

## Decision Drivers

- **Isolamento multi-tenant inquebravel** — assinatura e fatura de um grupo nunca visiveis/gravaveis por outro, fail-closed quando o contexto nao e setado.
- **Nenhum preco no repo MIT** — o schema registra valores, mas o seed nao fixa preco; preco e concern do Checkout (#1653).
- **Dinheiro correto** — inteiro de centavos + ISO 4217; nunca float; nunca PII de cartao.
- **Fatura como registro imutavel** — historico de cobranca nao e reescrito.
- **Nao fechar portas** a webhook (#1653), entitlements (#1654), metering (#1655) e signup (#1656).
- **Seguir exatamente os padroes existentes** do `garraia-workspace` (migrations 002/006/013/014/034).
- **Self-hosted MIT intacto** — o gateway sem billing continua subindo; nenhuma tabela nova e obrigatoria para bootar.

## Considered Options

### 1. `plans` com ou sem RLS

- **(escolhida) `plans` GLOBAL, fora de RLS.** `plans` nao tem `group_id`: e um catalogo igual para todos os tenants, mesma classe de `roles`/`permissions`/`role_permissions`, que a migration 007 lista explicitamente como "static lookup, public read" fora de RLS. Semeado por migration; `GRANT SELECT` ao `garraia_app`, sem INSERT/UPDATE/DELETE.
- (rejeitada) `plans` sob RLS por `group_id`. Exigiria denormalizar o catalogo por tenant ou inventar um `group_id` nullable — complexidade sem ganho, porque nao ha dado de tenant a proteger no catalogo.
- (rejeitada) planos como enum Rust hardcoded. Fecharia a porta a planos definidos em runtime (#1653/#1654) e espalharia a verdade entre codigo e banco.

### 2. Representacao de dinheiro

- **(escolhida) inteiro de centavos (`bigint`) + moeda ISO 4217 (`text`, tres maiusculas).** Sem erro de arredondamento; padrao de todo sistema de cobranca serio.
- (rejeitada) `numeric`/`decimal`. Correto numericamente, mas introduz ambiguidade de escala por moeda e e mais pesado de bindar; centavos cobre o caso.
- (rejeitada) float. Proibido para dinheiro.

### 3. Preco do plano Free: `0` ou `NULL`

- **(escolhida) `NULL`.** `NULL` = "preco nao fixado neste repositorio", distinto de `0` que afirmaria "custa exatamente zero". Honra "sem numeros de preco neste repo" e "sem preco fixado no seed". `price_cents` e `currency` viajam juntos (CHECK de coerencia): ou os dois definidos, ou os dois `NULL`.
- (rejeitada) `0`. Transformaria uma decisao de produto (o Free custa zero) num fato de schema; e se amanha o Free tiver um teto pago opcional, o `0` ja estaria mentindo.

### 4. Imutabilidade da fatura

- **(escolhida) ledger append-only no grant layer.** `garraia_app` recebe so `SELECT, INSERT` em `invoices` (sem UPDATE/DELETE) — mesmo padrao de `account_deletion_requests` (migration 034). Uma fatura emitida nunca muda pelo pool do app. As transicoes de pagamento (`open -> paid/void`) chegam em #1653 por caminho controlado (funcao `SECURITY DEFINER` ou role dedicada), com os campos financeiros congelados.
- (rejeitada) trigger `BEFORE UPDATE/DELETE`. Um trigger que bloqueia DELETE quebraria o `ON DELETE CASCADE` de `groups` (delete de grupo / purge LGPD precisa levar as faturas junto). O grant layer e mais simples e nao colide com o cascade (acoes referenciais nao passam pelo check de privilegio de tabela).
- (rejeitada) confiar na ADP da migration 007. A `ALTER DEFAULT PRIVILEGES ... TO garraia_app` da 007 **nao cobre** tabelas de migrations posteriores neste repo (documentado em 017/020/026); portanto os grants seletivos sao o conjunto real de privilegios — o teste de migration prova isso com `has_table_privilege`.

### 5. Historico de assinatura: uma linha mutavel vs. multiplas linhas

- **(escolhida) uma assinatura VIVA por grupo, historico cancelado preservado.** Indice parcial `UNIQUE (group_id) WHERE status <> 'canceled'` garante no maximo uma assinatura ativa/past_due por tenant; linhas canceladas ficam como historico. Mesmo padrao de "um pedido aberto por titular" (034) e single-owner (002).
- (rejeitada) uma linha unica por grupo, atualizada para sempre. Perde o historico de trocas de plano.

### 6. Ids de provedor de pagamento

- **(escolhida) colunas opacas opcionais, UNIQUE por provedor.** `payment_provider` (`stripe`|`pix`), `provider_customer_id`, `provider_subscription_id` em `subscriptions`; `provider_invoice_id` em `invoices`. Indices parciais `UNIQUE (payment_provider, <id>)` onde preenchidos dao idempotencia ao webhook de #1653. Todos `NULL` no plano Free. **Nunca** numero de cartao, CVV, PAN ou token de cartao — em nenhuma coluna, agora e sempre.

## Decision Outcome

Migration 035 cria tres entidades. Datas narrativas em America/New_York; toda coluna de tempo e `timestamptz` em UTC.

### `plans` — catalogo global (sem RLS)

- `id text PK` (slug, `^[a-z][a-z0-9-]{0,62}$`), `display_name`, `description`, `tier int`.
- `price_cents bigint NULL` (>= 0 quando setado), `currency text NULL` (ISO 4217), CHECK de coerencia `(price_cents IS NULL) = (currency IS NULL)`.
- `is_active boolean` (aposentar sem apagar), `created_at`, `updated_at`.
- **Seed:** apenas `('free', tier 0, price/currency NULL)`. Planos pagos entram com #1653 (seus valores vivem no repo privado). Seria migration forward-only trivial adiciona-los.
- **Grants:** `SELECT` ao `garraia_app`. Sem INSERT/UPDATE/DELETE (catalogo semeado por migration). Sem grant a `garraia_login`/`garraia_signup`.

### `subscriptions` — assinatura por tenant (FORCE RLS)

- `id uuid PK`, `group_id uuid NOT NULL -> groups ON DELETE CASCADE`, `plan_id text NOT NULL -> plans` (NO ACTION: plano em uso nao apaga).
- `status text` CHECK `IN ('active','past_due','canceled')`, default `active` (maquina minima, sem proration/trialing nesta issue).
- `current_period_start/end` (`end >= start` quando ambos setados), `canceled_at` com CHECK de coerencia `(status='canceled') = (canceled_at IS NOT NULL)`.
- Ids de provedor (`payment_provider`, `provider_customer_id`, `provider_subscription_id`), `NULL` no Free.
- `UNIQUE (id, group_id)` para a FK composta de `invoices`.
- Indices: parcial `UNIQUE (group_id) WHERE status <> 'canceled'`; `(group_id, status)`; `(plan_id)`; parcial `UNIQUE (payment_provider, provider_subscription_id)`; parcial `(payment_provider, provider_customer_id)`.
- **RLS:** `ENABLE`+`FORCE`, policy direta por `group_id` com `NULLIF(current_setting('app.current_group_id', true), '')::uuid`, `USING` e `WITH CHECK` identicos (padrao 013/014).
- **Grants:** `SELECT, INSERT, UPDATE` ao `garraia_app` (sem DELETE — cancelar e mudar status; so o cascade de group delete remove).

### `invoices` — fatura imutavel por tenant (FORCE RLS)

- `id uuid PK`, `group_id uuid NOT NULL -> groups ON DELETE CASCADE`, `subscription_id uuid NOT NULL`.
- FK composta `(subscription_id, group_id) -> subscriptions(id, group_id) ON DELETE CASCADE` — fail-closed contra drift cross-group (padrao `tasks -> task_lists` da 006).
- `status text` CHECK `IN ('open','paid','void','uncollectible')` (dominio completo para #1653 nao ampliar depois), `amount_cents bigint NOT NULL >= 0`, `currency text NOT NULL` ISO 4217.
- `issued_at timestamptz NOT NULL` (marco do "imutavel apos emitida"), `paid_at`, ids de provedor, `created_at`. Sem `updated_at` (append-only).
- Indices: `(group_id, issued_at DESC)`; `(subscription_id)`; `(group_id, status)`; parcial `UNIQUE (payment_provider, provider_invoice_id)` (idempotencia de webhook).
- **RLS:** igual `subscriptions`.
- **Grants:** `SELECT, INSERT` ao `garraia_app` (sem UPDATE/DELETE — ledger imutavel; #1653 adiciona o caminho controlado de transicao de pagamento).

### Como se liga a `groups` como tenant

Todo dado de billing scoped-por-tenant ancora em `group_id -> groups(id) ON DELETE CASCADE`, exatamente como messages/tasks/files. A isolacao e por `app.current_group_id` (contrato da migration 007): o chamador (gateway) seta `SET LOCAL`/`set_config` no inicio da transacao; sem isso, zero linhas (fail-closed). O catalogo `plans` e global e nao participa desse contrato.

## Consequences

### Positivas

- Fundacao de billing multi-tenant com isolamento provado por teste (RLS cross-group, fail-closed, WITH CHECK write-block).
- Nenhum preco no repo MIT; o self-hosted continua intacto (as tabelas sao opcionais para bootar o gateway).
- Fatura imutavel no grant layer, sem trigger e sem colidir com cascade/purge.
- Portas abertas para as quatro issues seguintes (ver abaixo).

### Negativas / divida assumida

- Transicoes de pagamento de `invoices` exigem, em #1653, um caminho controlado (funcao `SECURITY DEFINER` ou role dedicada) porque o `garraia_app` nao tem UPDATE. E deliberado (imutabilidade), mas e trabalho explicito que #1653 herda.
- Sem criacao automatica da assinatura Free no signup (#1656): o schema permite, o wiring nao existe ainda (ver "O que fica para #1653-#1656").

### Neutras

- `plans` fora de RLS exige disciplina: nenhum dado de tenant pode ser adicionado a essa tabela no futuro sem reconsiderar a decisao.
- Contagem de tabelas do `garraia-workspace` sobe de 39 para 42; tabelas sob FORCE RLS sobem de 34 para 36 (`plans` e a 6a tabela global fora de RLS, ao lado de users/roles/permissions/role_permissions/group_invites).

## O que fica para #1653-#1656 (portas deixadas abertas)

- **#1653 (webhook Stripe/Pix):** colunas `payment_provider`/`provider_*_id` com UNIQUE por provedor ja existem (idempotencia). O dominio de `invoices.status` ja cobre `paid/void/uncollectible`. Falta o endpoint de webhook (via `garraia_common::ssrf` para qualquer saida HTTP), o caminho controlado de UPDATE em `invoices` e o seed dos planos pagos com seus valores.
- **#1654 (entitlements):** encaixa como catalogo `plan_entitlements` (global, por `plan_id`) + override por `subscription` se necessario. `plans` e `subscriptions` ja existem como ancora; nenhuma mudanca destrutiva aqui.
- **#1655 (metering / usage_ledger):** encaixa como tabela tenant-scoped sob FORCE RLS (mesmo padrao de 014/035) referenciando `group_id` e, se preciso, `subscription_id`. Desenhada aqui apenas como ponto de extensao — NAO criada nesta issue (decisao do dono: metering fica esperando).
- **#1656 (signup/billing page):** a assinatura Free de todo tenant novo sera inserida pelo `garraia_app` na transacao de criacao do grupo (que ja seta `app.current_group_id`), passando pela policy `WITH CHECK`. `garraia_app` ja tem `INSERT` em `subscriptions`. Falta so o wiring no fluxo de criacao de grupo/signup.

## Open questions (para o dono)

1. **Placeholders de planos pagos no seed:** o corpo da #1652 menciona "placeholders dos planos pagos (sem preco)"; a decisao do dono de 2026-10-22 fala so em "um plano Free sem preco" e deixa "os valores dos planos" esperando. Este ADR semeia **apenas** `free`. Quer linhas placeholder (ex.: `pro`, `team`) sem preco agora, ou elas entram com #1653? (Adicionar e migration trivial.)
2. **`usage_ledger` nesta issue:** o corpo da #1652 diz "desenhar aqui, popular depois"; a decisao do dono lista metering (#1655) como "esperando". Este ADR **nao cria** `usage_ledger`, so documenta onde ele encaixa. Confirma que fica para #1655?
