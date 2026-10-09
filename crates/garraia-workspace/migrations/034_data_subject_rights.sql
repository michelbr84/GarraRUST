-- 034_data_subject_rights.sql
--
-- Direitos dos titulares (LGPD arts. 18/20, GDPR arts. 15-20): o apoio de
-- schema para (a) o pedido de correcao de e-mail com estado pendente e
-- (b) o apagamento definitivo rastreavel com periodo de carencia.
--
-- Depends:  001 (users/sessions/api_keys/group_members/group_invites),
--           003 (files/folders/file_versions), 004 (chats/messages/threads),
--           005 (memory_items/memory_embeddings), 006 (tasks*),
--           014 (tus_uploads), 026-029 (doc_*), 030 (users.status
--           'anonymized'), 032/033 (padrao SECURITY DEFINER + grant).
-- Forward-only per CLAUDE.md regra 9: so adiciona tabelas/colunas/funcoes e
-- re-adiciona um CHECK nomeado ampliando o dominio (nunca estreitando).
--
-- ============================================================================
-- Por que DUAS tabelas de pedido e nao colunas em `users`
-- ============================================================================
--
-- Um pedido tem ciclo de vida proprio (aberto -> resolvido), autor, carimbo e
-- resultado. Em coluna de `users` isso vira um punhado de campos nullable sem
-- historico; em tabela, cada pedido e uma linha auditavel e o indice parcial
-- UNIQUE expressa "no maximo um pedido aberto por titular" no schema, em vez
-- de no app. Mesmo desenho do ledger `tus_uploads` (migration 014).
--
-- ============================================================================
-- Por que o apagamento NAO remove a linha de `users`
-- ============================================================================
--
-- `DELETE FROM users` e estruturalmente impossivel aqui, e nao por descuido:
-- quatro FKs apontam para `users(id)` SEM clausula ON DELETE, logo com a
-- semantica default NO ACTION, o que faz o DELETE ser recusado enquanto
-- existir uma linha referenciando:
--
--   messages.sender_user_id       -> users(id)   (migration 004)
--   chats.created_by              -> users(id)   (migration 004)
--   message_threads.created_by    -> users(id)   (migration 004)
--   group_invites.created_by      -> users(id)   (migration 001)
--   group_members.invited_by      -> users(id)   (migration 001)
--
-- Essas linhas vivem em ESPACO COMPARTILHADO: a mensagem de um titular e a
-- raiz da thread onde outras pessoas responderam, o chat que ele criou e onde
-- a equipe conversa. Remove-las para atender um pedido individual apagaria
-- contexto de terceiros — exatamente o que a protecao de terceiros proibe.
--
-- Portanto o apagamento definitivo aqui significa, e isto e a definicao
-- operacional usada por `purge_account_data`:
--
--   1. CONTEUDO pessoal do titular e destruido (corpo de mensagem, comentario,
--      memoria, arquivo + versoes + blob);
--   2. IDENTIFICADORES pessoais sao substituidos por token nao-identificavel
--      (e-mail, display_name, provider_sub, todos os `*_label`
--      desnormalizados, ip e user_agent do audit);
--   3. CREDENCIAIS sao destruidas (password_hash, sessoes, chaves de API);
--   4. a linha de `users` PERMANECE como lapide sem dado pessoal, com
--      `status = 'purged'`, so para manter a integridade referencial do
--      espaco compartilhado.
--
-- O resultado nao e dado pessoal (LGPD art. 12 / GDPR art. 4(5) e recital 26):
-- o UUID remanescente nao e reversivel a uma pessoa sem a linha de `users`,
-- que ja nao guarda nenhum identificador.
--
-- ============================================================================
-- Por que `audit_events` SOBREVIVE ao apagamento
-- ============================================================================
--
-- Decisao consciente e conservadora (GDPR art. 17(3)(b) e (e), LGPD art. 16,
-- I e IV): o audit e (i) obrigacao legal/registro de seguranca e (ii) a UNICA
-- prova de que o apagamento foi pedido e executado. Apaga-lo junto destruiria
-- a evidencia do proprio direito exercido.
--
-- O meio-termo implementado: a LINHA fica (action, resource, created_at), mas
-- os identificadores pessoais DENTRO dela sao apagados — `actor_label` (copia
-- do display_name), `ip` e `user_agent` viram NULL. `actor_user_id` fica: e
-- pseudonimo e aponta para uma lapide sem dado pessoal. O `metadata` dos
-- eventos de workspace e estrutural por contrato do repo (`audit_workspace_event`
-- nunca escreve PII), por isso nao e tocado.

-- ─── users.status — novo estado terminal 'purged' ────────────────────────
--
-- Amplia o CHECK nomeado (padrao da migration 030). 'deleted' passa a
-- significar "pedido feito, carencia correndo" e 'purged' "apagamento
-- executado" — sem isso o worker nao consegue distinguir o que ja fez, e o
-- procedimento manual nao consegue auditar o estado de uma conta.
ALTER TABLE users
    DROP CONSTRAINT IF EXISTS users_status_check;

ALTER TABLE users
    ADD CONSTRAINT users_status_check
        CHECK (status IN ('active', 'suspended', 'deleted', 'anonymized', 'purged'));

COMMENT ON COLUMN users.status IS
    'active -> normal; suspended -> blocked login; deleted -> apagamento pedido, carencia correndo (account_deletion_requests); anonymized -> PII substituida, conta anonimizada (LGPD art. 12 / GDPR art. 4(5)); purged -> apagamento definitivo executado, linha e lapide sem dado pessoal';

-- ─── user_email_change_requests — correcao de e-mail (LGPD art. 18, III) ──
--
-- ATENCAO, invariante de seguranca: esta tabela NAO troca e-mail. Ela REGISTRA
-- o pedido. `users.email` e a chave de login (`user_identities.provider_sub`
-- carrega o mesmo e-mail para provider='internal') e trocar sem provar posse
-- da caixa nova e um take-over: quem tiver um token de acesso roubado aponta a
-- conta para o proprio e-mail e captura o fluxo de recuperacao.
--
-- Este repositorio NAO tem infraestrutura de e-mail (nenhum crate depende de
-- smtp/lettre/provedor — verificado por varredura no momento desta migration),
-- logo nao ha como enviar o link de confirmacao. Em vez de inventar o envio ou
-- trocar as cegas, o pedido fica `pending` e a aplicacao e passo MANUAL do
-- operador, com verificacao de posse fora de banda — procedimento em
-- `docs/legal/data-subject-requests.md`.
CREATE TABLE user_email_change_requests (
    id              uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         uuid        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- citext: mesma classe de `users.email`, para o operador comparar
    -- case-insensitive na hora de aplicar.
    requested_email citext      NOT NULL CHECK (length(requested_email) BETWEEN 3 AND 320),
    status          text        NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending', 'applied', 'rejected', 'superseded', 'canceled')),
    requested_at    timestamptz NOT NULL DEFAULT now(),
    resolved_at     timestamptz,
    resolved_by     uuid        REFERENCES users(id) ON DELETE SET NULL,
    -- Coerencia de estado no schema: pendente nao tem resolucao, resolvido tem.
    CONSTRAINT user_email_change_resolution_consistent
        CHECK ((status = 'pending'  AND resolved_at IS NULL)
            OR (status <> 'pending' AND resolved_at IS NOT NULL))
);

-- No maximo UM pedido aberto por titular. Um segundo PATCH supersede o
-- anterior (status='superseded') em vez de empilhar pedidos concorrentes que
-- o operador teria de desempatar.
CREATE UNIQUE INDEX user_email_change_one_pending_idx
    ON user_email_change_requests(user_id)
    WHERE status = 'pending';

CREATE INDEX user_email_change_user_idx
    ON user_email_change_requests(user_id, requested_at DESC);

COMMENT ON TABLE user_email_change_requests IS
    'Pedidos de correcao de e-mail (LGPD art. 18, III / GDPR art. 16). REGISTRO apenas: nenhuma linha aqui altera users.email. A aplicacao exige verificacao de posse da caixa nova e hoje e passo manual do operador (docs/legal/data-subject-requests.md) porque o repo nao tem infraestrutura de envio de e-mail.';
COMMENT ON COLUMN user_email_change_requests.requested_email IS
    'E-mail NOVO pedido pelo titular. E PII: nunca logar inline, nunca expor a outro usuario. Visivel so ao proprio titular (RLS owner-only) e ao operador.';

ALTER TABLE user_email_change_requests ENABLE ROW LEVEL SECURITY;
ALTER TABLE user_email_change_requests FORCE ROW LEVEL SECURITY;

CREATE POLICY user_email_change_owner_only ON user_email_change_requests
    AS PERMISSIVE
    FOR ALL
    USING (user_id = NULLIF(current_setting('app.current_user_id', true), '')::uuid)
    WITH CHECK (user_id = NULLIF(current_setting('app.current_user_id', true), '')::uuid);

COMMENT ON POLICY user_email_change_owner_only ON user_email_change_requests IS
    'Class: user. Mesmo desenho de sessions_owner_only (migration 007). Fail-closed via NULLIF: GUC vazia -> NULL -> zero linhas. USING e WITH CHECK identicos (padrao da migration 013).';

GRANT SELECT, INSERT, UPDATE ON user_email_change_requests TO garraia_app;
-- Sem DELETE de proposito: o historico de pedidos e trilha de conformidade.

-- ─── account_deletion_requests — fila do apagamento definitivo ────────────
--
-- `DELETE /v1/me` ja fazia o soft-delete e revogava sessoes; o apagamento
-- definitivo era "deferido a um worker futuro". Esta e a fila desse worker.
--
-- `purge_after` (carencia) existe por duas razoes, e a conservadora vem
-- primeiro: apagamento imediato transforma um token roubado em arma de
-- destruicao irreversivel. A carencia da ao titular legitimo tempo de
-- reverter e ao operador tempo de aplicar retencao legal (hold) antes de o
-- dado virar irrecuperavel.
CREATE TABLE account_deletion_requests (
    id            uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id       uuid        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    status        text        NOT NULL DEFAULT 'pending'
                  CHECK (status IN ('pending', 'in_progress', 'completed', 'failed', 'canceled')),
    requested_at  timestamptz NOT NULL DEFAULT now(),
    -- Momento a partir do qual o worker pode executar. Nunca no passado em
    -- relacao ao pedido.
    purge_after   timestamptz NOT NULL,
    started_at    timestamptz,
    completed_at  timestamptz,
    attempts      int         NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    -- Mensagem tecnica do ultimo erro. NUNCA conteudo do titular: o worker
    -- grava so o texto do erro de SQL/IO.
    last_error    text,
    -- Relatorio estrutural: contagem por tabela, chaves de blob pendentes e
    -- marcas de progresso. NUNCA conteudo apagado.
    purge_report  jsonb       NOT NULL DEFAULT '{}'::jsonb,
    CONSTRAINT account_deletion_purge_after_not_before_request
        CHECK (purge_after >= requested_at)
);

-- No maximo UM pedido aberto por titular (idempotencia do DELETE /v1/me).
CREATE UNIQUE INDEX account_deletion_one_open_idx
    ON account_deletion_requests(user_id)
    WHERE status IN ('pending', 'in_progress');

-- Indice parcial da varredura do worker (mesmo papel do
-- tus_uploads_expires_in_progress_idx da migration 014).
CREATE INDEX account_deletion_due_idx
    ON account_deletion_requests(purge_after)
    WHERE status = 'pending';

CREATE INDEX account_deletion_stale_idx
    ON account_deletion_requests(started_at)
    WHERE status = 'in_progress';

COMMENT ON TABLE account_deletion_requests IS
    'Fila do apagamento definitivo de conta (LGPD art. 18, VI / GDPR art. 17). Uma linha por pedido; `purge_after` e a carencia antes de o apagamento ficar irreversivel. O worker (`account_purge_worker.rs`) reclama por claim_account_purge() e executa purge_account_data().';
COMMENT ON COLUMN account_deletion_requests.purge_report IS
    'Relatorio ESTRUTURAL do apagamento: contagem por tabela, object_keys pendentes de remocao no ObjectStore e marcas de progresso (db_purged). Nunca guarda conteudo apagado.';

ALTER TABLE account_deletion_requests ENABLE ROW LEVEL SECURITY;
ALTER TABLE account_deletion_requests FORCE ROW LEVEL SECURITY;

CREATE POLICY account_deletion_owner_only ON account_deletion_requests
    AS PERMISSIVE
    FOR ALL
    USING (user_id = NULLIF(current_setting('app.current_user_id', true), '')::uuid)
    WITH CHECK (user_id = NULLIF(current_setting('app.current_user_id', true), '')::uuid);

COMMENT ON POLICY account_deletion_owner_only ON account_deletion_requests IS
    'Class: user. O titular ve e cria o proprio pedido; ninguem ve o de outro. O worker e cross-tenant e por isso NAO passa por aqui: usa claim_account_purge/purge_account_data (SECURITY DEFINER), como o uploads_worker faz com expire_tus_uploads_sweep (migration 032).';

GRANT SELECT, INSERT ON account_deletion_requests TO garraia_app;
-- Sem UPDATE/DELETE para garraia_app: a maquina de estados do pedido e
-- exclusiva das funcoes SECURITY DEFINER abaixo. Assim um handler de request
-- nao consegue, nem por bug, marcar o proprio pedido como 'completed' sem o
-- apagamento ter acontecido, nem cancelar o de ninguem.

-- ─── claim_account_purge — reivindica lote devido (SECURITY DEFINER) ──────
--
-- Mesmo motivo da expire_tus_uploads_sweep (migration 032): o worker roda em
-- `garraia_app` sem GUC de tenant, e a policy owner-only desta tabela e
-- fail-closed — consulta direta devolveria zero linhas. A funcao roda com o
-- privilegio do criador, encapsula o bypass e e granted a garraia_app.
--
-- Reivindica dois conjuntos:
--   (a) 'pending' com a carencia vencida;
--   (b) 'in_progress' parado ha mais de p_stale (worker morto no meio).
-- `FOR UPDATE SKIP LOCKED` deixa replicas concorrentes dividirem o lote sem
-- bloquear uma a outra.
CREATE OR REPLACE FUNCTION claim_account_purge(
    p_now   timestamptz,
    p_limit int,
    p_stale interval DEFAULT interval '1 hour'
)
RETURNS TABLE (
    request_id   uuid,
    user_id      uuid,
    attempts     int,
    db_purged    boolean,
    object_keys  text[]
)
LANGUAGE sql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
    WITH victim AS (
        SELECT r.id
        FROM account_deletion_requests r
        WHERE (r.status = 'pending' AND r.purge_after < p_now)
           OR (r.status = 'in_progress' AND r.started_at < p_now - p_stale)
        ORDER BY r.purge_after ASC
        LIMIT p_limit
        FOR UPDATE SKIP LOCKED
    )
    UPDATE account_deletion_requests AS r
    SET status     = 'in_progress',
        started_at = now(),
        attempts   = r.attempts + 1
    FROM victim
    WHERE r.id = victim.id
    RETURNING r.id,
              r.user_id,
              r.attempts,
              COALESCE((r.purge_report->>'db_purged')::boolean, false),
              -- Chaves de blob persistidas por uma passada anterior que morreu
              -- depois do commit do SQL e antes de limpar o ObjectStore.
              COALESCE(
                  ARRAY(SELECT jsonb_array_elements_text(r.purge_report->'object_keys')),
                  ARRAY[]::text[]
              );
$$;

COMMENT ON FUNCTION claim_account_purge(timestamptz, int, interval) IS
    'SECURITY DEFINER: reivindica ate p_limit pedidos de apagamento devidos (carencia vencida) ou travados em in_progress ha mais de p_stale, marca in_progress e devolve o progresso ja feito (db_purged + object_keys pendentes) para o worker retomar sem repetir o trabalho de banco.';

GRANT EXECUTE ON FUNCTION claim_account_purge(timestamptz, int, interval) TO garraia_app;

-- ─── purge_account_data — o apagamento em si (SECURITY DEFINER) ───────────
--
-- Uma transacao, todas as tabelas. SECURITY DEFINER porque 32 tabelas estao
-- sob FORCE RLS e o apagamento e cross-tenant por natureza: o titular pode ter
-- dado em varios grupos e nao existe uma GUC de grupo que cubra todos.
--
-- Sobre a regra 12 do CLAUDE.md (`password_hash` nunca pelo pool `garraia_app`):
-- esta funcao ESCREVE NULL em `user_identities.password_hash` e nunca o LE nem
-- o devolve. O retorno e so contagem. Nenhum caminho de leitura do hash e
-- criado aqui, e o `garraia_app` continua sem ver a coluna por RLS — ele so
-- pode chamar a funcao, cujo corpo e fixo.
--
-- Idempotente: todo UPDATE de redacao tem guarda contra o valor-lapide, todo
-- DELETE e por predicado. Rodar duas vezes devolve contagem zero na segunda.
CREATE OR REPLACE FUNCTION purge_account_data(p_user_id uuid)
RETURNS TABLE (report jsonb, object_keys text[])
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
DECLARE
    -- Lapide de conteudo. Precisa ser NAO-VAZIA: messages.body tem
    -- CHECK (length(body) BETWEEN 1 AND 100000) e task_comments.body_md tem
    -- CHECK (length(body_md) BETWEEN 1 AND 50000) — string vazia seria
    -- recusada pelo schema.
    c_body_tombstone constant text := '[conteudo removido a pedido do titular]';
    -- Lapide de rotulo. Os `*_label` desnormalizados sao COPIA do display_name,
    -- logo PII, e precisam cair junto.
    c_label_tombstone constant text := 'Usuario Removido';
    -- Mesma formula de garraia_auth::anon_token (UUID completo, 32 hex): o
    -- prefixo curto colidiria entre usuarios criados na mesma janela sob
    -- UUIDv7 e os UNIQUE de users.email e (provider, provider_sub)
    -- transformariam a colisao em erro.
    v_anon_token text := 'anon-' || replace(p_user_id::text, '-', '') || '@garraanon.local';
    v_old_email  citext;
    v_keys       text[] := ARRAY[]::text[];
    v_report     jsonb  := '{}'::jsonb;
    v_n          bigint;
BEGIN
    -- E-mail atual, para limpar `group_invites.invited_email` (que guarda o
    -- e-mail por valor, nao por FK). Se a conta ja passou por anonymize, ja
    -- vem como token — o UPDATE abaixo fica no-op, que e o correto.
    SELECT u.email INTO v_old_email FROM users u WHERE u.id = p_user_id;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'purge_account_data: user % not found', p_user_id;
    END IF;

    -- ── 1. Chaves de blob, ANTES de apagar as linhas que as guardam ───────
    --
    -- Depois do DELETE a chave nao existe mais em lugar nenhum; se o worker
    -- morresse entre o commit e a limpeza do ObjectStore o blob ficaria
    -- orfao para sempre. Por isso as chaves sao coletadas aqui e persistidas
    -- no pedido no passo 2, antes de qualquer DELETE.
    SELECT COALESCE(array_agg(k), ARRAY[]::text[]) INTO v_keys FROM (
        SELECT fv.object_key AS k
        FROM file_versions fv
        JOIN files f ON f.id = fv.file_id
        WHERE f.created_by = p_user_id
        UNION
        SELECT t.object_key AS k
        FROM tus_uploads t
        WHERE t.created_by = p_user_id
    ) AS keys;

    -- ── 2. Persistir as chaves no pedido (ponto de retomada) ──────────────
    UPDATE account_deletion_requests
    SET purge_report = purge_report
                       || jsonb_build_object('object_keys', to_jsonb(v_keys))
    WHERE user_id = p_user_id
      AND status = 'in_progress';

    -- ── 3. Linhas puramente pessoais: DELETE ──────────────────────────────
    --
    -- Nenhuma delas e conteudo de terceiro nem estrutura compartilhada: sao
    -- ligacoes do titular (reacao, mencao recebida, atribuicao, assinatura,
    -- participacao) ou credencial dele.
    DELETE FROM message_reactions   WHERE user_id = p_user_id;
    GET DIAGNOSTICS v_n = ROW_COUNT; v_report := v_report || jsonb_build_object('message_reactions', v_n);

    DELETE FROM message_mentions    WHERE mentioned_user_id = p_user_id;
    GET DIAGNOSTICS v_n = ROW_COUNT; v_report := v_report || jsonb_build_object('message_mentions', v_n);

    DELETE FROM doc_page_mentions   WHERE mentioned_user_id = p_user_id;
    GET DIAGNOSTICS v_n = ROW_COUNT; v_report := v_report || jsonb_build_object('doc_page_mentions', v_n);

    DELETE FROM task_assignees      WHERE user_id = p_user_id;
    GET DIAGNOSTICS v_n = ROW_COUNT; v_report := v_report || jsonb_build_object('task_assignees', v_n);

    DELETE FROM task_subscriptions  WHERE user_id = p_user_id;
    GET DIAGNOSTICS v_n = ROW_COUNT; v_report := v_report || jsonb_build_object('task_subscriptions', v_n);

    DELETE FROM chat_members        WHERE user_id = p_user_id;
    GET DIAGNOSTICS v_n = ROW_COUNT; v_report := v_report || jsonb_build_object('chat_members', v_n);

    DELETE FROM group_members       WHERE user_id = p_user_id;
    GET DIAGNOSTICS v_n = ROW_COUNT; v_report := v_report || jsonb_build_object('group_members', v_n);

    DELETE FROM sessions            WHERE user_id = p_user_id;
    GET DIAGNOSTICS v_n = ROW_COUNT; v_report := v_report || jsonb_build_object('sessions', v_n);

    DELETE FROM api_keys            WHERE user_id = p_user_id;
    GET DIAGNOSTICS v_n = ROW_COUNT; v_report := v_report || jsonb_build_object('api_keys', v_n);

    -- Uploads em voo do titular. Blobs correspondentes saem no passo 1/worker.
    DELETE FROM tus_uploads         WHERE created_by = p_user_id;
    GET DIAGNOSTICS v_n = ROW_COUNT; v_report := v_report || jsonb_build_object('tus_uploads', v_n);

    -- Memoria do assistente SOBRE o titular, criada por ele, em qualquer
    -- escopo. Pessoal ('user') e obvio; 'group'/'chat' tambem sai porque
    -- memory_items e estado derivado do assistente, nao conhecimento
    -- autoral da organizacao como uma pagina de wiki. memory_embeddings
    -- cai por ON DELETE CASCADE (migration 005).
    DELETE FROM memory_items        WHERE created_by = p_user_id;
    GET DIAGNOSTICS v_n = ROW_COUNT; v_report := v_report || jsonb_build_object('memory_items', v_n);

    -- Arquivos enviados pelo titular. O blob e dado pessoal dele e o
    -- apagamento e a obrigacao legal, entao a linha sai e `file_versions`,
    -- `message_attachments` e `task_attachments` caem por ON DELETE CASCADE
    -- (migrations 003/017/020). Consequencia assumida e documentada: um
    -- anexo que o titular enviou desaparece da mensagem de outra pessoa —
    -- o que sai e o ARQUIVO dele, nao a mensagem dela.
    DELETE FROM files               WHERE created_by = p_user_id;
    GET DIAGNOSTICS v_n = ROW_COUNT; v_report := v_report || jsonb_build_object('files', v_n);

    -- ── 4. Conteudo autoral em espaco compartilhado: redigir em lugar ─────
    --
    -- A linha fica (outras pessoas responderam a ela, threads a referenciam
    -- por FK), o CONTEUDO vai embora. `messages.body_tsv` e coluna GENERATED
    -- a partir de `body` (migration 004), logo o indice de busca full-text
    -- e reescrito pelo proprio UPDATE — o texto antigo nao sobrevive no FTS.
    UPDATE messages
    SET body         = c_body_tombstone,
        sender_label = c_label_tombstone,
        deleted_at   = COALESCE(deleted_at, now()),
        edited_at    = now()
    WHERE sender_user_id = p_user_id
      AND body <> c_body_tombstone;
    GET DIAGNOSTICS v_n = ROW_COUNT; v_report := v_report || jsonb_build_object('messages_redacted', v_n);

    UPDATE task_comments
    SET body_md      = c_body_tombstone,
        author_label = c_label_tombstone,
        deleted_at   = COALESCE(deleted_at, now()),
        edited_at    = now()
    WHERE author_user_id = p_user_id
      AND body_md <> c_body_tombstone;
    GET DIAGNOSTICS v_n = ROW_COUNT; v_report := v_report || jsonb_build_object('task_comments_redacted', v_n);

    -- ── 5. Rotulos desnormalizados: copia do display_name = PII ───────────
    --
    -- Sem este passo o nome do titular sobrevive espalhado por sete tabelas
    -- mesmo com `users.display_name` ja anonimizado. `created_by` vira NULL
    -- onde a FK permite (ON DELETE SET NULL ja declara essa intencao no
    -- schema), desligando a linha da pessoa.
    UPDATE files      SET created_by = NULL, created_by_label = c_label_tombstone
        WHERE created_by = p_user_id;
    UPDATE folders    SET created_by = NULL, created_by_label = c_label_tombstone
        WHERE created_by = p_user_id;
    UPDATE file_versions SET created_by = NULL, created_by_label = c_label_tombstone
        WHERE created_by = p_user_id;
    UPDATE tasks      SET created_by = NULL, created_by_label = c_label_tombstone
        WHERE created_by = p_user_id;
    UPDATE task_lists SET created_by = NULL, created_by_label = c_label_tombstone
        WHERE created_by = p_user_id;
    UPDATE task_labels SET created_by = NULL, created_by_label = c_label_tombstone
        WHERE created_by = p_user_id;
    UPDATE doc_pages  SET created_by = NULL, created_by_label = c_label_tombstone
        WHERE created_by = p_user_id;
    UPDATE message_attachments SET attached_by = NULL, attached_by_label = c_label_tombstone
        WHERE attached_by = p_user_id;
    UPDATE task_attachments    SET attached_by = NULL, attached_by_label = c_label_tombstone
        WHERE attached_by = p_user_id;
    -- task_activity.actor_user_id nao tem FK (migration 006); zerar o id
    -- aqui e seguro e desliga a trilha da pessoa.
    UPDATE task_activity SET actor_user_id = NULL, actor_label = c_label_tombstone
        WHERE actor_user_id = p_user_id;

    -- ── 6. audit_events: a linha fica, os identificadores saem ────────────
    --
    -- Justificativa completa no cabecalho desta migration (GDPR art. 17(3)(b)
    -- e (e) / LGPD art. 16). `ip` e `user_agent` sao PII direta e saem;
    -- `actor_label` e copia do display_name e sai; `actor_user_id` fica,
    -- pseudonimo, apontando para a lapide.
    UPDATE audit_events
    SET actor_label = NULL,
        ip          = NULL,
        user_agent  = NULL
    WHERE actor_user_id = p_user_id
      AND (actor_label IS NOT NULL OR ip IS NOT NULL OR user_agent IS NOT NULL);
    GET DIAGNOSTICS v_n = ROW_COUNT; v_report := v_report || jsonb_build_object('audit_events_deidentified', v_n);

    -- ── 7. Convites que carregam o e-mail por valor ───────────────────────
    UPDATE group_invites SET invited_email = v_anon_token
        WHERE invited_email = v_old_email;
    GET DIAGNOSTICS v_n = ROW_COUNT; v_report := v_report || jsonb_build_object('group_invites_anonymized', v_n);

    -- ── 8. Pedidos de correcao de e-mail: o e-mail PEDIDO tambem e PII ────
    UPDATE user_email_change_requests
    SET requested_email = v_anon_token,
        status          = CASE WHEN status = 'pending' THEN 'canceled' ELSE status END,
        resolved_at     = COALESCE(resolved_at, now())
    WHERE user_id = p_user_id
      AND requested_email <> v_anon_token;
    GET DIAGNOSTICS v_n = ROW_COUNT; v_report := v_report || jsonb_build_object('email_change_requests_anonymized', v_n);

    -- ── 9. Credencial e identidade ────────────────────────────────────────
    --
    -- `password_hash` vira NULL (escrita, nunca leitura — ver nota da regra 12
    -- no cabecalho da funcao). `provider_sub` guarda o e-mail de login para
    -- provider='internal' (migration 001) e vira o token.
    UPDATE user_identities
    SET provider_sub  = v_anon_token,
        password_hash = NULL
    WHERE user_id = p_user_id;
    GET DIAGNOSTICS v_n = ROW_COUNT; v_report := v_report || jsonb_build_object('user_identities_anonymized', v_n);

    -- ── 10. A lapide ──────────────────────────────────────────────────────
    UPDATE users
    SET email            = v_anon_token,
        display_name     = c_label_tombstone,
        status           = 'purged',
        legacy_sqlite_id = NULL,
        updated_at       = now()
    WHERE id = p_user_id;

    v_report := v_report || jsonb_build_object('db_purged', true);

    RETURN QUERY SELECT v_report, v_keys;
END;
$$;

COMMENT ON FUNCTION purge_account_data(uuid) IS
    'SECURITY DEFINER: apagamento definitivo do titular em UMA transacao, cross-tenant (32 tabelas sob FORCE RLS). Destroi conteudo pessoal (mensagem, comentario, memoria, arquivo+versoes), apaga ligacoes e credenciais, substitui identificadores por token nao-identificavel e deixa users como lapide status=purged — a linha de users NAO e removida porque cinco FKs NO ACTION a protegem e remove-la apagaria contexto de terceiros. Devolve contagem por tabela e as object_keys cujos blobs o worker deve remover do ObjectStore. Idempotente.';

GRANT EXECUTE ON FUNCTION purge_account_data(uuid) TO garraia_app;

-- ─── finish_account_purge / fail_account_purge — fim da maquina de estados ─
--
-- Separadas da purge porque a remocao dos blobs acontece FORA do banco: so o
-- worker sabe se o ObjectStore aceitou. `garraia_app` nao tem UPDATE nesta
-- tabela (ver grant acima), logo este e o unico caminho para fechar o pedido.
CREATE OR REPLACE FUNCTION finish_account_purge(
    p_request_id uuid,
    p_report     jsonb
)
RETURNS boolean
LANGUAGE sql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
    WITH done AS (
        UPDATE account_deletion_requests
        SET status       = 'completed',
            completed_at = now(),
            last_error   = NULL,
            purge_report = purge_report || p_report
        WHERE id = p_request_id
          AND status = 'in_progress'
        RETURNING 1
    )
    SELECT EXISTS (SELECT 1 FROM done);
$$;

COMMENT ON FUNCTION finish_account_purge(uuid, jsonb) IS
    'SECURITY DEFINER: fecha um pedido in_progress como completed e mescla o relatorio final (inclui o resultado da remocao de blobs, que acontece fora do banco). Devolve false se o pedido nao estava in_progress.';

GRANT EXECUTE ON FUNCTION finish_account_purge(uuid, jsonb) TO garraia_app;

-- Devolve o pedido para a fila enquanto houver tentativa sobrando; esgotadas,
-- marca 'failed' para o operador olhar. Falha transitoria de banco ou de
-- ObjectStore nao deve condenar o pedido na primeira tentativa, e tambem nao
-- deve girar para sempre em silencio.
CREATE OR REPLACE FUNCTION fail_account_purge(
    p_request_id   uuid,
    p_error        text,
    p_max_attempts int DEFAULT 5
)
RETURNS text
LANGUAGE sql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
    WITH updated AS (
        UPDATE account_deletion_requests
        SET status     = CASE WHEN attempts >= p_max_attempts THEN 'failed' ELSE 'pending' END,
            last_error = left(p_error, 2000)
        WHERE id = p_request_id
          AND status = 'in_progress'
        RETURNING status
    )
    SELECT status FROM updated;
$$;

COMMENT ON FUNCTION fail_account_purge(uuid, text, int) IS
    'SECURITY DEFINER: devolve o pedido para pending enquanto houver tentativa sobrando (falha transitoria), ou marca failed quando esgotam. last_error guarda so texto tecnico, nunca conteudo do titular. Devolve o status resultante, ou NULL se o pedido nao estava in_progress.';

GRANT EXECUTE ON FUNCTION fail_account_purge(uuid, text, int) TO garraia_app;
