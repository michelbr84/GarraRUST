-- 035_billing_plan_subscription_invoice.sql
--
-- Issue:    #1652 — fundacao do billing do Garra Cloud (plan/subscription/invoice).
-- ADR:      docs/adr/0026-billing-schema-plan-subscription-invoice.md
-- Depends:  001 (users/groups — tenant root), 007 (baseline FORCE RLS +
--           app.current_group_id contract), 013/014 (WITH CHECK explicito).
-- Forward-only per CLAUDE.md regra 9: so adiciona tabelas/colunas/indices;
-- nenhum DROP, nenhum ALTER destrutivo.
--
-- Escopo (decisao do dono 2026-10-22): SO a fundacao de schema das tres
-- entidades plan/subscription/invoice, com o plano Free sem preco, RLS, a
-- funcao `garraia_effective_plan_id` (ausencia de assinatura = Free, sem
-- trigger nem backfill) e testes. Fora de escopo e DELIBERADAMENTE ausente:
--   * webhook Stripe/Pix e transicoes de pagamento (#1653);
--   * entitlements por plano (#1654);
--   * usage_ledger de metering (#1655);
--   * pagina de signup e criacao explicita da assinatura paga (#1656);
--   * planos pagos no seed — os nomes dos tiers vivem no repo privado e so
--     chegam com os valores em #1653.
-- O schema abaixo e desenhado para NAO fechar portas a esses quatro — ver
-- o ADR 0026 §"Consequencias" para onde cada um encaixa.
--
-- ============================================================================
-- Por que `plans` fica FORA de RLS
-- ============================================================================
--
-- `plans` e um catalogo GLOBAL, nao-tenant: nao tem `group_id`. E a mesma
-- classe de `roles`/`permissions`/`role_permissions` da migration 002, que a
-- migration 007 lista explicitamente como "static lookup, public read" fora
-- de RLS. Toda assinatura referencia um plano por FK; o catalogo e o mesmo
-- para todos os tenants. Colocar RLS aqui nao protegeria nada (nao ha dado de
-- tenant) e so quebraria a leitura do catalogo. Grants: SELECT ao
-- `garraia_app` (leitura), sem INSERT/UPDATE/DELETE — o catalogo e semeado por
-- migration, igual a `roles`/`permissions`.
--
-- ============================================================================
-- Sem numero de preco neste repo (MIT) — preco e concern do Checkout (#1653)
-- ============================================================================
--
-- `price_cents`/`currency` do plano Free ficam NULL de proposito: NULL
-- significa "preco nao fixado neste repositorio" (distinto de 0, que afirmaria
-- "custa exatamente zero"). Os valores dos planos pagos vivem no repo privado
-- `GarraIA/GarraIA` (`commercial/`) e so chegam ao banco via #1653. Por isso
-- ESTE seed insere apenas `free`; linhas de planos pagos entram quando #1653
-- trouxer os valores — adiciona-las e migration forward-only trivial.
--
-- ============================================================================
-- Dinheiro = inteiro de centavos + ISO 4217 — nunca float, nunca PII de cartao
-- ============================================================================
--
-- Valor monetario e sempre `bigint` de centavos (menor unidade da moeda) com
-- CHECK de nao-negativo, e a moeda e `text` ISO 4217 (tres maiusculas). Nenhum
-- float em nenhum ponto. Os ids externos de provedor de pagamento
-- (`provider_customer_id`, `provider_subscription_id`, `provider_invoice_id`)
-- sao OPACOS do provedor e opcionais; UNIQUE por provedor onde preenchidos.
-- Numero de cartao, CVV, PAN, token de cartao: PROIBIDOS em qualquer coluna,
-- agora e sempre (#1653 nao muda isso).

-- ─── 035.1 plans — catalogo global (sem RLS) ──────────────────────────────

CREATE TABLE plans (
    id            text        PRIMARY KEY
                              CHECK (id ~ '^[a-z][a-z0-9-]{0,62}$'),
    display_name  text        NOT NULL CHECK (length(display_name) BETWEEN 1 AND 200),
    description   text        NOT NULL CHECK (length(description) BETWEEN 1 AND 1000),
    -- Ordenacao numerica: maior = mais privilegio. free = 0. Mesma ideia de
    -- roles.tier (migration 002).
    tier          int         NOT NULL CHECK (tier >= 0),
    -- Valor em centavos (menor unidade da moeda). NULL = preco nao fixado
    -- neste repo. Nunca negativo.
    price_cents   bigint      CHECK (price_cents IS NULL OR price_cents >= 0),
    -- Moeda ISO 4217 (tres maiusculas). NULL quando price_cents e NULL.
    currency      text        CHECK (currency IS NULL OR currency ~ '^[A-Z]{3}$'),
    -- Preco e moeda viajam juntos: ou os dois estao definidos, ou os dois sao
    -- NULL. Impede uma linha com valor sem moeda (ou vice-versa).
    CONSTRAINT plans_price_currency_coherent
        CHECK ((price_cents IS NULL) = (currency IS NULL)),
    -- Plano pode ser aposentado do catalogo sem ser apagado (assinaturas e
    -- faturas apontam para ele por FK; nunca hard-delete).
    is_active     boolean     NOT NULL DEFAULT true,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now()
);

COMMENT ON TABLE plans IS
    'Catalogo GLOBAL de planos (nao-tenant, sem group_id, sem RLS — mesma classe de roles/permissions). Semeado por migration. #1652 semeia apenas free; planos pagos entram via #1653 com seus valores. price_cents/currency NULL = preco nao fixado neste repo MIT.';
COMMENT ON COLUMN plans.id IS
    'Slug estavel do plano (ex.: free). Minuscula/digito/hifen. Referenciado por subscriptions.plan_id.';
COMMENT ON COLUMN plans.price_cents IS
    'Valor em centavos (menor unidade da moeda). NULL = preco nao fixado neste repositorio (preco e concern do Checkout, #1653). Nunca negativo, nunca float.';
COMMENT ON COLUMN plans.currency IS
    'Moeda ISO 4217 (tres maiusculas, ex.: USD/BRL). NULL quando price_cents e NULL. Viaja junto com price_cents (CHECK plans_price_currency_coherent).';
COMMENT ON COLUMN plans.is_active IS
    'false = plano aposentado do catalogo. Nunca hard-delete um plano: assinaturas/faturas o referenciam por FK.';

-- Seed: SO o plano Free, sem preco. BYOK obrigatorio e limites duros sao
-- concern de enforcement (#1654), nao do schema.
INSERT INTO plans (id, display_name, description, tier, price_cents, currency) VALUES
    ('free', 'Free', 'Plano gratuito do Garra Cloud: BYOK obrigatorio, limites duros e auto-sleep. Preco nao se aplica (sem preco fixado neste repositorio).', 0, NULL, NULL);

-- Catalogo e read-only para o app: semeado por migration, igual roles/permissions.
-- ATENCAO (ADP): a `ALTER DEFAULT PRIVILEGES ... TO garraia_app` da migration
-- 007 concede SELECT/INSERT/UPDATE/DELETE a TODA tabela criada depois dela pelo
-- mesmo role — e as migrations rodam todas como o mesmo role (documentado em
-- 006:31-33: "ALTER DEFAULT PRIVILEGES still applies going forward for tables
-- created by any migration > 007"). Logo `plans` ja nasce com os quatro
-- privilegios para `garraia_app`. O REVOKE abaixo e OBRIGATORIO: sem ele o pool
-- do app poderia escrever no catalogo, que e semeado so por migration.
GRANT SELECT ON plans TO garraia_app;
REVOKE INSERT, UPDATE, DELETE ON plans FROM garraia_app;

-- ─── 035.2 subscriptions — assinatura por tenant (FORCE RLS) ──────────────

CREATE TABLE subscriptions (
    id                       uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    group_id                 uuid        NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    -- NO ACTION (default) na FK para plans: um plano em uso nao pode ser
    -- apagado. Catalogo usa is_active para aposentar.
    plan_id                  text        NOT NULL REFERENCES plans(id),
    -- Maquina de estados minima desta issue (sem proration, sem trialing):
    status                   text        NOT NULL DEFAULT 'active'
                                         CHECK (status IN ('active', 'past_due', 'canceled')),
    current_period_start     timestamptz,
    current_period_end       timestamptz,
    canceled_at              timestamptz,
    -- Ids OPACOS do provedor de pagamento. NULL no plano Free. #1653 preenche.
    payment_provider         text        CHECK (payment_provider IN ('stripe', 'pix')),
    provider_customer_id     text        CHECK (provider_customer_id IS NULL OR length(provider_customer_id) BETWEEN 1 AND 255),
    provider_subscription_id text        CHECK (provider_subscription_id IS NULL OR length(provider_subscription_id) BETWEEN 1 AND 255),
    created_at               timestamptz NOT NULL DEFAULT now(),
    updated_at               timestamptz NOT NULL DEFAULT now(),
    -- Coerencia de estado no schema: cancelada tem carimbo; nao-cancelada nao.
    CONSTRAINT subscriptions_canceled_consistent
        CHECK ((status = 'canceled') = (canceled_at IS NOT NULL)),
    -- Periodo coerente quando ambos preenchidos.
    CONSTRAINT subscriptions_period_order
        CHECK (current_period_end IS NULL
            OR current_period_start IS NULL
            OR current_period_end >= current_period_start),
    -- Idempotencia hermetica: um id de provedor so existe acompanhado do
    -- provedor. Sem isto, o indice parcial UNIQUE (payment_provider, <id>)
    -- aceitaria linhas com provider NULL e id duplicado, furando a chave de
    -- idempotencia do webhook de #1653.
    CONSTRAINT subscriptions_provider_ids_require_provider
        CHECK ((provider_customer_id IS NULL OR payment_provider IS NOT NULL)
            AND (provider_subscription_id IS NULL OR payment_provider IS NOT NULL)),
    -- Permite FK composta (subscription_id, group_id) de invoices — mesmo
    -- padrao de task_lists/tasks (migration 006) contra drift cross-group.
    CONSTRAINT subscriptions_id_group_unique UNIQUE (id, group_id)
);

-- No maximo UMA assinatura VIVA (nao cancelada) por tenant. Historico de
-- assinaturas canceladas permanece. Mesmo padrao de indice parcial UNIQUE da
-- migration 034 (um pedido aberto por titular) e do single-owner da 002.
CREATE UNIQUE INDEX subscriptions_one_live_per_group_idx
    ON subscriptions(group_id)
    WHERE status <> 'canceled';

CREATE INDEX subscriptions_group_status_idx
    ON subscriptions(group_id, status);

CREATE INDEX subscriptions_plan_idx
    ON subscriptions(plan_id);

-- Idempotencia de webhook (#1653): uma assinatura do provedor mapeia para no
-- maximo uma linha. UNIQUE por (provedor, id) onde preenchido.
CREATE UNIQUE INDEX subscriptions_provider_subscription_idx
    ON subscriptions(payment_provider, provider_subscription_id)
    WHERE provider_subscription_id IS NOT NULL;

CREATE INDEX subscriptions_provider_customer_idx
    ON subscriptions(payment_provider, provider_customer_id)
    WHERE provider_customer_id IS NOT NULL;

COMMENT ON TABLE subscriptions IS
    'Assinatura por tenant (group_id). Uma assinatura viva por grupo (indice parcial), historico cancelado preservado. FORCE RLS direto por group_id. Transicoes de pagamento (past_due via webhook) chegam em #1653.';
COMMENT ON COLUMN subscriptions.status IS
    'active -> normal; past_due -> pagamento falhou (populado por #1653); canceled -> encerrada (canceled_at preenchido). Sem proration/trialing nesta issue.';
COMMENT ON COLUMN subscriptions.provider_subscription_id IS
    'Id OPACO da assinatura no provedor (ex.: sub_... do Stripe). NULL no Free. UNIQUE por provedor. Nunca PII de cartao.';

-- ─── RLS: ENABLE + FORCE + group-isolation (padrao migration 014) ──────────
ALTER TABLE subscriptions ENABLE ROW LEVEL SECURITY;
ALTER TABLE subscriptions FORCE ROW LEVEL SECURITY;

CREATE POLICY subscriptions_group_isolation ON subscriptions
    AS PERMISSIVE
    FOR ALL
    USING (group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid)
    WITH CHECK (group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid);

COMMENT ON POLICY subscriptions_group_isolation ON subscriptions IS
    'Class: direct. Context: app.current_group_id. Fail-closed via NULLIF (GUC vazia -> NULL -> zero linhas). USING e WITH CHECK identicos (padrao da migration 013): uma conversao futura para AS RESTRICTIVE tem de tocar os dois lados de proposito.';

-- Assinatura muda de plano/estado dentro do tenant: SELECT/INSERT/UPDATE.
-- A ADP da migration 007 ja concedeu os quatro privilegios (006:31-33), entao
-- e o REVOKE DELETE abaixo que de fato tira o DELETE do app: cancelamento e
-- mudanca de status, nao remocao de linha; so o cascade de group delete remove
-- (acoes referenciais nao passam pelo check de privilegio de tabela).
GRANT SELECT, INSERT, UPDATE ON subscriptions TO garraia_app;
REVOKE DELETE ON subscriptions FROM garraia_app;

-- ─── 035.3 invoices — fatura imutavel por tenant (FORCE RLS) ──────────────

CREATE TABLE invoices (
    id                  uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    group_id            uuid        NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    subscription_id     uuid        NOT NULL,
    -- Maquina de estados de cobranca. Dominio completo ja aqui para #1653 nao
    -- precisar ampliar o CHECK depois.
    status              text        NOT NULL DEFAULT 'open'
                                    CHECK (status IN ('open', 'paid', 'void', 'uncollectible')),
    amount_cents        bigint      NOT NULL CHECK (amount_cents >= 0),
    currency            text        NOT NULL CHECK (currency ~ '^[A-Z]{3}$'),
    -- Momento da emissao — imutavel. A fatura e historico: conteudo financeiro
    -- congelado apos emitida (ver nota de grants abaixo).
    issued_at           timestamptz NOT NULL DEFAULT now(),
    paid_at             timestamptz,
    -- Id OPACO da fatura no provedor (ex.: in_... do Stripe). Idempotencia de
    -- webhook em #1653. NULL ate o provedor emitir. Nunca PII de cartao.
    payment_provider    text        CHECK (payment_provider IN ('stripe', 'pix')),
    provider_invoice_id text        CHECK (provider_invoice_id IS NULL OR length(provider_invoice_id) BETWEEN 1 AND 255),
    created_at          timestamptz NOT NULL DEFAULT now(),
    -- Idempotencia hermetica: o id da fatura do provedor so existe acompanhado
    -- do provedor (mesma razao do CHECK em subscriptions).
    CONSTRAINT invoices_provider_id_requires_provider
        CHECK (provider_invoice_id IS NULL OR payment_provider IS NOT NULL),
    -- FK composta (subscription_id, group_id) -> subscriptions(id, group_id):
    -- fail-closed contra drift cross-group (uma fatura nao pode reivindicar o
    -- grupo A apontando para assinatura do grupo B). Mesmo padrao de tasks ->
    -- task_lists (migration 006). ON DELETE CASCADE e inofensivo: a unica vez
    -- que uma assinatura some e no delete do grupo, que ja cascata as faturas
    -- pela FK de group_id.
    FOREIGN KEY (subscription_id, group_id)
        REFERENCES subscriptions(id, group_id) ON DELETE CASCADE
);

CREATE INDEX invoices_group_issued_idx
    ON invoices(group_id, issued_at DESC);

CREATE INDEX invoices_subscription_idx
    ON invoices(subscription_id);

CREATE INDEX invoices_group_status_idx
    ON invoices(group_id, status);

-- Idempotencia de webhook (#1653): uma fatura do provedor mapeia para no
-- maximo uma linha.
CREATE UNIQUE INDEX invoices_provider_invoice_idx
    ON invoices(payment_provider, provider_invoice_id)
    WHERE provider_invoice_id IS NOT NULL;

COMMENT ON TABLE invoices IS
    'Fatura por tenant (group_id), historico IMUTAVEL. FORCE RLS direto por group_id. Imutabilidade e enforced no grant layer via REVOKE: a ADP da migration 007 concede os quatro privilegios, e os REVOKE UPDATE/DELETE desta migration deixam garraia_app so com SELECT/INSERT. As transicoes de pagamento (open->paid/void) chegam em #1653 por caminho controlado (funcao SECURITY DEFINER ou role dedicada), com os campos financeiros congelados.';
COMMENT ON COLUMN invoices.amount_cents IS
    'Valor em centavos (menor unidade da moeda). NOT NULL, nao-negativo, nunca float. Congelado apos emissao.';
COMMENT ON COLUMN invoices.issued_at IS
    'Momento da emissao (UTC). Imutavel — e o marco do "imutavel apos emitida".';
COMMENT ON COLUMN invoices.provider_invoice_id IS
    'Id OPACO da fatura no provedor. UNIQUE por provedor (idempotencia de webhook, #1653). Nunca PII de cartao.';

-- ─── RLS: ENABLE + FORCE + group-isolation (padrao migration 014) ──────────
ALTER TABLE invoices ENABLE ROW LEVEL SECURITY;
ALTER TABLE invoices FORCE ROW LEVEL SECURITY;

CREATE POLICY invoices_group_isolation ON invoices
    AS PERMISSIVE
    FOR ALL
    USING (group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid)
    WITH CHECK (group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid);

COMMENT ON POLICY invoices_group_isolation ON invoices IS
    'Class: direct. Context: app.current_group_id. Fail-closed via NULLIF. USING e WITH CHECK identicos (padrao da migration 013).';

-- Fatura e ledger imutavel: garraia_app fica so com SELECT/INSERT. A ADP da
-- migration 007 ja concedeu os quatro privilegios (006:31-33), entao sao os
-- REVOKE UPDATE/DELETE abaixo que impoem a imutabilidade: uma fatura emitida
-- nunca muda pelo pool do app. O cascade de group delete (FK ON DELETE CASCADE)
-- remove faturas independentemente deste grant (acoes referenciais nao passam
-- pelo check de privilegio de tabela). #1653 adiciona o caminho controlado de
-- transicao de pagamento.
GRANT SELECT, INSERT ON invoices TO garraia_app;
REVOKE UPDATE, DELETE ON invoices FROM garraia_app;

-- ─── 035.4 garraia_effective_plan_id — plano efetivo (ausencia = Free) ─────
--
-- Todo grupo tem um plano EFETIVO mesmo sem linha em subscriptions: a ausencia
-- de assinatura viva significa Free. Isto evita trigger e backfill e mantem a
-- regra "a linha de subscription so nasce com pagamento" (#1653) — um grupo
-- novo, e todo grupo que ja existia antes do billing (upgrade da 0.4.9),
-- resolve para 'free' sem ninguem intervir.
--
-- SECURITY INVOKER (default do Postgres, explicito aqui por ser load-bearing):
-- a funcao roda com o privilegio e o contexto RLS do CHAMADOR. Sob `garraia_app`
-- com a GUC app.current_group_id setada, a consulta interna a `subscriptions`
-- respeita a policy group_isolation — logo chamar a funcao com o id de OUTRO
-- grupo devolve 'free' (sem visibilidade, cai no default), nunca vaza o plano
-- alheio. SECURITY DEFINER aqui seria um vazamento: rodaria como o dono e
-- furaria a RLS.
--
-- STABLE: so le, sem efeito colateral; resultado estavel dentro da query.
-- Ha no maximo uma assinatura nao-cancelada por grupo (indice parcial), entao
-- o LIMIT 1 e cinto-e-suspensorio.
CREATE FUNCTION garraia_effective_plan_id(p_group_id uuid)
RETURNS text
LANGUAGE sql
STABLE
SECURITY INVOKER
SET search_path = public, pg_temp
AS $$
    SELECT COALESCE(
        (SELECT s.plan_id
         FROM subscriptions s
         WHERE s.group_id = p_group_id
           AND s.status <> 'canceled'
         LIMIT 1),
        'free'
    );
$$;

COMMENT ON FUNCTION garraia_effective_plan_id(uuid) IS
    'Plano efetivo do grupo: plan_id da assinatura nao-cancelada, ou ''free'' na ausencia. SECURITY INVOKER (respeita a RLS da sessao): chamar com o id de outro grupo devolve ''free'' porque a RLS esconde a assinatura alheia. STABLE, read-only. Ausencia de assinatura = Free e por desenho (ADR 0026): a linha de subscription so nasce com pagamento (#1653), sem trigger nem backfill.';

-- EXECUTE so ao garraia_app (Postgres concede a PUBLIC por default — o REVOKE
-- e obrigatorio). Mesma higiene das funcoes da migration 034.
REVOKE EXECUTE ON FUNCTION garraia_effective_plan_id(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION garraia_effective_plan_id(uuid) TO garraia_app;
