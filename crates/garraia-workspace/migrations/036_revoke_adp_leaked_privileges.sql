-- 036_revoke_adp_leaked_privileges.sql
--
-- Fecha o excesso de privilegio que a ALTER DEFAULT PRIVILEGES (ADP) da
-- migration 007 concedeu silenciosamente a `garraia_app`.
--
-- ============================================================================
-- Ordem de merge / numeracao (CLAUDE.md regra 18)
-- ============================================================================
--
-- O slot 035 esta RESERVADO pela branch `feat/billing-schema-1652` (ainda nao
-- mergeada no momento desta migration). Esta migration usa 036 e assume que a
-- 035 entra ANTES dela — o merge sera rodado nessa ordem. A dependencia e de
-- NUMERACAO (manter a serie estritamente crescente e linear, regra 18), nao de
-- schema: todo REVOKE abaixo toca apenas tabelas criadas em migrations <= 034,
-- logo 036 nao le nem altera nada que a 035 cria. Se a 035 criar tabelas novas
-- com privilegio menor que CRUD, e responsabilidade DELA revogar o proprio
-- excesso (mesmo padrao documentado abaixo).
--
-- Forward-only (regra 9): so REVOKE de privilegio, nenhum DROP, nenhum ALTER
-- destrutivo de schema. REVOKE de um privilegio ausente e no-op no Postgres,
-- entao esta migration e idempotente em qualquer ordem de aplicacao.
--
-- ============================================================================
-- Causa raiz
-- ============================================================================
--
-- A migration 007 (linhas 76-77) declara:
--
--     ALTER DEFAULT PRIVILEGES IN SCHEMA public
--         GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO garraia_app;
--
-- A ADP vale para TODA tabela criada DEPOIS dela pelo mesmo role (o proprio
-- repo documenta isso em 003:40-42 e 006:31-33). Varias migrations posteriores
-- declararam, no seu GRANT seletivo, um conjunto MENOR de privilegios — mas
-- GRANT e ADITIVO: ele concede, nunca restringe. O privilegio extra concedido
-- pela ADP sobrevivia, divergindo da intencao declarada. Em producao isso dava
-- a `garraia_app` UPDATE/DELETE onde o autor da tabela pretendia nega-los.
--
-- Cada bloco abaixo revoga SOMENTE o excesso. SELECT e INSERT nunca sao
-- tocados; o privilegio de mutacao legitimo de cada tabela (o que o GRANT
-- original listou) permanece. A prova por tabela de que NENHUM caminho do pool
-- `garraia_app` usa o privilegio revogado esta no fragmento de changelog e no
-- plano; as rotas de mutacao que existem usam so privilegios mantidos, e os
-- UPDATE/DELETE cross-tenant do apagamento LGPD rodam em funcoes
-- SECURITY DEFINER como `garraia_purge`/owner, nunca como `garraia_app`.
--
-- ============================================================================
-- Padrao a partir daqui (replicado no CLAUDE.md e no threat-model)
-- ============================================================================
--
-- Toda tabela criada APOS a migration 007 recebe SELECT/INSERT/UPDATE/DELETE
-- para `garraia_app` automaticamente via ADP. Um GRANT seletivo mais estreito
-- NAO reduz isso. Para privilegio menor que CRUD completo e OBRIGATORIO um
-- REVOKE explicito na mesma migration que cria a tabela.

-- ─── task_attachments (migration 017) — intencao: sem UPDATE ──────────────
-- Anexo e imutavel: o handler faz INSERT/DELETE (tasks/attachments.rs),
-- nunca UPDATE. A redacao de rotulo no apagamento roda em purge_account_data
-- (SECURITY DEFINER, 034).
REVOKE UPDATE ON task_attachments FROM garraia_app;

-- ─── message_attachments (migration 020) — intencao: sem UPDATE ───────────
-- Mesmo desenho de task_attachments: INSERT/DELETE (messages.rs), sem UPDATE.
REVOKE UPDATE ON message_attachments FROM garraia_app;

-- ─── message_reactions (migration 021) — intencao: sem UPDATE ─────────────
-- Reacao e INSERT (adiciona) / DELETE (remove) (chats.rs); nunca se atualiza
-- uma reacao in-place.
REVOKE UPDATE ON message_reactions FROM garraia_app;

-- ─── message_mentions (migration 022) — intencao: sem UPDATE ──────────────
-- Mencao e derivada do corpo da mensagem: INSERT/DELETE, nunca UPDATE. Sai
-- por ON DELETE CASCADE quando a mensagem morre.
REVOKE UPDATE ON message_mentions FROM garraia_app;

-- ─── doc_pages (migration 026) — intencao: sem DELETE ─────────────────────
-- Apagamento de pagina e SOFT (UPDATE archived_at, docs.rs); nunca DELETE
-- fisico pelo pool do app. UPDATE permanece.
REVOKE DELETE ON doc_pages FROM garraia_app;

-- ─── doc_page_versions (migration 028) — intencao: so SELECT/INSERT ───────
-- Versao e snapshot imutavel: o app so faz INSERT (doc_versions.rs) e SELECT.
-- A redacao de rotulo no apagamento roda em purge_account_data (SECURITY
-- DEFINER, 034), nao pelo app.
REVOKE UPDATE, DELETE ON doc_page_versions FROM garraia_app;

-- ─── doc_page_mentions (migration 029) — intencao: sem UPDATE ─────────────
-- Igual a message_mentions: INSERT/DELETE (doc_mentions.rs), nunca UPDATE.
REVOKE UPDATE ON doc_page_mentions FROM garraia_app;

-- ─── user_email_change_requests (migration 034) — intencao: sem DELETE ────
-- O historico de pedidos e trilha de conformidade (034:153). O app faz
-- INSERT/UPDATE (me.rs: registra e supersede); nunca apaga a trilha. UPDATE
-- permanece.
REVOKE DELETE ON user_email_change_requests FROM garraia_app;

-- ─── account_deletion_requests (migration 034) — intencao: so SELECT/INSERT ─
-- O caso com peso de seguranca (034:219-223): a maquina de estados do pedido
-- e EXCLUSIVA das funcoes SECURITY DEFINER (claim/finish/fail_account_purge),
-- executadas pelo worker como `garraia_purge`. O app so faz INSERT (enfileira
-- o pedido em DELETE /v1/me) e SELECT (le o estado). Com UPDATE/DELETE reais,
-- um handler no escopo do proprio titular (RLS owner-only satisfeita) poderia
-- marcar o pedido como 'completed' SEM o purge, ou cancela-lo fora do fluxo.
-- O modulo account_purge_worker.rs ja documenta este invariante como se ele
-- valesse — esta migration torna a realidade igual ao contrato.
REVOKE UPDATE, DELETE ON account_deletion_requests FROM garraia_app;
