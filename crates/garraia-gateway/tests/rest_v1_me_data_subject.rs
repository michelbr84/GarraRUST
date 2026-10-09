//! Direitos dos titulares — testes de integracao contra Postgres real.
//!
//! Cobre o incremento que tornou `GET /v1/me/export` completo por titular,
//! deu ao `PATCH /v1/me` um fluxo seguro de correcao de e-mail e ligou o
//! worker de apagamento definitivo ao `DELETE /v1/me`.
//!
//! ## Por que uma funcao `#[tokio::test]` por cenario grande, e nao vinte
//!
//! Mesmo motivo registrado em `rest_v1_me_authed.rs`: cada `#[tokio::test]`
//! sobe o proprio runtime tokio e o derruba ao retornar, e conexoes do
//! `PgPool` compartilhado adquiridas dentro de um runtime morto produzem
//! `pool timed out` de forma nao-determinista. Os cenarios aqui correm em
//! sequencia dentro de poucas funcoes; a falha ainda aponta o cenario pela
//! mensagem de assert.
//!
//! ## O teste que mais importa
//!
//! `purge_nao_toca_dado_de_terceiro`: Alice e Bob conversam no MESMO chat;
//! Alice pede o apagamento. Depois da purga, o corpo da mensagem da Alice
//! tem de estar destruido e o corpo da mensagem do Bob tem de estar
//! **intacto**. Um pedido individual nao esvazia espaco compartilhado — e
//! essa a invariante que separa este apagamento de um `DELETE CASCADE`.

mod common;

use std::sync::Arc;
use std::time::Duration;

use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use garraia_gateway::account_purge_worker::{
    AccountPurgeWorkerConfig, DEFAULT_GRACE_PERIOD_DAYS, run_purge_tick,
};
use garraia_storage::{LocalFs, ObjectStore, PutOptions};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

use common::fixtures::seed_user_with_group;
use common::{Harness, harness_get};

// ─── helpers de requisicao ───────────────────────────────────────────────

fn bearer(mut req: Request<axum::body::Body>, token: &str) -> Request<axum::body::Body> {
    req.headers_mut().insert(
        HeaderName::from_static("authorization"),
        HeaderValue::from_str(&format!("Bearer {token}")).expect("valid bearer header"),
    );
    req
}

fn get_authed(path: &str, token: &str) -> Request<axum::body::Body> {
    bearer(harness_get(path), token)
}

/// `PATCH`/`DELETE` com `ConnectInfo` — o governor da cadeia de producao le o
/// IP de par dessa extensao e sem ela responde 500 antes do handler.
fn json_authed(
    method: &str,
    path: &str,
    token: &str,
    body: Option<Value>,
) -> Request<axum::body::Body> {
    let payload = match &body {
        Some(v) => axum::body::Body::from(serde_json::to_vec(v).expect("serialize body")),
        None => axum::body::Body::empty(),
    };
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .body(payload)
        .expect("request builder");
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo::<std::net::SocketAddr>(
            "127.0.0.1:1".parse().expect("fixed test peer"),
        ));
    bearer(req, token)
}

async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("body is JSON")
}

// ─── helpers de seed (admin_pool: superusuario, bypassa RLS) ────────────

/// Um chat de grupo. Devolve o `chat_id`.
async fn seed_chat(h: &Harness, group_id: Uuid, creator: Uuid, name: &str) -> Uuid {
    let chat_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO chats (id, group_id, type, name, created_by) \
         VALUES ($1, $2, 'channel', $3, $4)",
    )
    .bind(chat_id)
    .bind(group_id)
    .bind(name)
    .bind(creator)
    .execute(&h.admin_pool)
    .await
    .expect("seed chat");
    chat_id
}

/// Uma mensagem. Devolve o `message_id`.
async fn seed_message(
    h: &Harness,
    chat_id: Uuid,
    group_id: Uuid,
    sender: Uuid,
    label: &str,
    body: &str,
) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO messages (id, chat_id, group_id, sender_user_id, sender_label, body) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(chat_id)
    .bind(group_id)
    .bind(sender)
    .bind(label)
    .bind(body)
    .execute(&h.admin_pool)
    .await
    .expect("seed message");
    id
}

/// Um arquivo + uma versao apontando para `object_key`. Devolve o `file_id`.
async fn seed_file_with_version(
    h: &Harness,
    group_id: Uuid,
    owner: Uuid,
    name: &str,
    object_key: &str,
) -> Uuid {
    let file_id = Uuid::new_v4();
    let mut tx = h.admin_pool.begin().await.expect("seed file tx");
    sqlx::query(
        "INSERT INTO files (id, group_id, name, size_bytes, mime_type, created_by, created_by_label) \
         VALUES ($1, $2, $3, 11, 'text/plain', $4, 'Seed Owner')",
    )
    .bind(file_id)
    .bind(group_id)
    .bind(name)
    .bind(owner)
    .execute(&mut *tx)
    .await
    .expect("seed files row");

    // checksum/hmac: hex de 64 caracteres, exigido pelos CHECKs da migration 003.
    let hex64 = "a".repeat(64);
    sqlx::query(
        "INSERT INTO file_versions \
             (file_id, group_id, version, object_key, etag, checksum_sha256, \
              integrity_hmac, size_bytes, mime_type, created_by, created_by_label) \
         VALUES ($1, $2, 1, $3, 'etag-seed', $4, $4, 11, 'text/plain', $5, 'Seed Owner')",
    )
    .bind(file_id)
    .bind(group_id)
    .bind(object_key)
    .bind(&hex64)
    .bind(owner)
    .execute(&mut *tx)
    .await
    .expect("seed file_versions row");

    tx.commit().await.expect("seed file commit");
    file_id
}

/// Uma memoria pessoal (`scope_type='user'`, `group_id IS NULL`).
async fn seed_personal_memory(h: &Harness, owner: Uuid, content: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO memory_items \
             (id, scope_type, scope_id, group_id, created_by, created_by_label, kind, content) \
         VALUES ($1, 'user', $2, NULL, $2, 'Seed Owner', 'note', $3)",
    )
    .bind(id)
    .bind(owner)
    .bind(content)
    .execute(&h.admin_pool)
    .await
    .expect("seed memory");
    id
}

/// Uma lista + uma tarefa criada por `owner`. Devolve o `task_id`.
async fn seed_task(h: &Harness, group_id: Uuid, owner: Uuid, title: &str) -> Uuid {
    let list_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let mut tx = h.admin_pool.begin().await.expect("seed task tx");
    sqlx::query(
        "INSERT INTO task_lists (id, group_id, name, type, created_by, created_by_label) \
         VALUES ($1, $2, 'Seed list', 'list', $3, 'Seed Owner')",
    )
    .bind(list_id)
    .bind(group_id)
    .bind(owner)
    .execute(&mut *tx)
    .await
    .expect("seed task_lists");
    sqlx::query(
        "INSERT INTO tasks (id, list_id, group_id, title, created_by, created_by_label) \
         VALUES ($1, $2, $3, $4, $5, 'Seed Owner')",
    )
    .bind(task_id)
    .bind(list_id)
    .bind(group_id)
    .bind(title)
    .bind(owner)
    .execute(&mut *tx)
    .await
    .expect("seed tasks");
    tx.commit().await.expect("seed task commit");
    task_id
}

/// Um thread de mensagem com título, ancorado em `root_message` (migration
/// 004: `UNIQUE (root_message_id)`). Devolve o `thread_id`. O título é o
/// dado pessoal aqui — a migration 034 o zera no apagamento.
async fn seed_thread(
    h: &Harness,
    chat_id: Uuid,
    root_message: Uuid,
    creator: Uuid,
    title: &str,
) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO message_threads (id, chat_id, root_message_id, title, created_by) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(chat_id)
    .bind(root_message)
    .bind(title)
    .bind(creator)
    .execute(&h.admin_pool)
    .await
    .expect("seed message thread");
    id
}

/// Uma página de doc + uma versão snapshot criados por `creator` (migrations
/// 026/028). Devolve o `version_id`. `created_by` da versão é UUID plain
/// NOT NULL sem FK — sobrevive ao dono; a 034 redige só o label.
async fn seed_doc_version(h: &Harness, group_id: Uuid, creator: Uuid) -> Uuid {
    let page_id = Uuid::new_v4();
    let version_id = Uuid::new_v4();
    let mut tx = h.admin_pool.begin().await.expect("seed doc version tx");
    sqlx::query(
        "INSERT INTO doc_pages (id, group_id, title, created_by, created_by_label) \
         VALUES ($1, $2, 'Pagina seed', $3, 'Seed Owner')",
    )
    .bind(page_id)
    .bind(group_id)
    .bind(creator)
    .execute(&mut *tx)
    .await
    .expect("seed doc_pages row");
    sqlx::query(
        "INSERT INTO doc_page_versions \
             (id, page_id, group_id, snapshot_jsonb, created_by, created_by_label) \
         VALUES ($1, $2, $3, '{}'::jsonb, $4, 'Seed Owner')",
    )
    .bind(version_id)
    .bind(page_id)
    .bind(group_id)
    .bind(creator)
    .execute(&mut *tx)
    .await
    .expect("seed doc_page_versions row");
    tx.commit().await.expect("seed doc version commit");
    version_id
}

/// Ações de audit de um titular (fatia de conta, `group_id = nil`).
async fn audit_actions_for_user(h: &Harness, user_id: Uuid) -> Vec<String> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT action FROM audit_events \
         WHERE actor_user_id = $1 AND group_id = $2 \
         ORDER BY created_at DESC, id DESC",
    )
    .bind(user_id)
    .bind(Uuid::nil())
    .fetch_all(&h.admin_pool)
    .await
    .expect("read audit");
    rows.into_iter().map(|(a,)| a).collect()
}

/// Antecipa a carencia para o passado — e o que torna o pedido elegivel sem
/// o teste esperar 30 dias.
async fn make_purge_due(h: &Harness, user_id: Uuid) {
    let n = sqlx::query(
        "UPDATE account_deletion_requests SET purge_after = now() - interval '1 minute', \
             requested_at = now() - interval '31 days' \
         WHERE user_id = $1 AND status = 'pending'",
    )
    .bind(user_id)
    .execute(&h.admin_pool)
    .await
    .expect("backdate purge_after")
    .rows_affected();
    assert_eq!(
        n, 1,
        "esperava exatamente um pedido pendente para antecipar"
    );
}

// ─── Cenario 1: exportacao com conteudo + delimitacao cross-tenant ──────

async fn export_inclui_conteudo_e_nunca_alcanca_outro_tenant() {
    let h = Harness::get().await;

    let (alice, alice_group, alice_token) = seed_user_with_group(&h, "alice@dsr-export.test")
        .await
        .expect("seed alice");
    let (bob, bob_group, bob_token) = seed_user_with_group(&h, "bob@dsr-export.test")
        .await
        .expect("seed bob");

    // Conteudo da Alice no grupo dela.
    let alice_chat = seed_chat(&h, alice_group, alice, "alice-canal").await;
    let alice_msg = seed_message(
        &h,
        alice_chat,
        alice_group,
        alice,
        "Alice",
        "SEGREDO-DA-ALICE",
    )
    .await;
    let alice_file = seed_file_with_version(
        &h,
        alice_group,
        alice,
        "alice-doc.txt",
        &format!("{alice_group}/files/alice/v1"),
    )
    .await;
    let alice_mem = seed_personal_memory(&h, alice, "MEMORIA-DA-ALICE").await;
    let alice_task = seed_task(&h, alice_group, alice, "TAREFA-DA-ALICE").await;

    // Conteudo do Bob, em outro grupo.
    let bob_chat = seed_chat(&h, bob_group, bob, "bob-canal").await;
    let bob_msg = seed_message(&h, bob_chat, bob_group, bob, "Bob", "SEGREDO-DO-BOB").await;
    seed_file_with_version(
        &h,
        bob_group,
        bob,
        "bob-doc.txt",
        &format!("{bob_group}/files/bob/v1"),
    )
    .await;
    seed_personal_memory(&h, bob, "MEMORIA-DO-BOB").await;
    seed_task(&h, bob_group, bob, "TAREFA-DO-BOB").await;

    // ── Exportacao da Alice ────────────────────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(get_authed("/v1/me/export", &alice_token))
        .await
        .expect("oneshot export");
    assert_eq!(resp.status(), StatusCode::OK, "export deve responder 200");

    let disposition = resp
        .headers()
        .get(axum::http::header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(
        disposition.starts_with("attachment;"),
        "export deve vir como anexo, veio {disposition:?}"
    );

    let v = body_json(resp).await;
    let raw = serde_json::to_string(&v).expect("reserialize export");

    assert_eq!(v["schema_version"], "2", "schema do export com conteudo");

    // O conteudo da Alice tem de estar la, e por id.
    let msg_ids: Vec<&str> = v["messages"]
        .as_array()
        .expect("messages array")
        .iter()
        .filter_map(|m| m["message_id"].as_str())
        .collect();
    assert!(
        msg_ids.contains(&alice_msg.to_string().as_str()),
        "export deve conter a mensagem escrita pela Alice; veio {msg_ids:?}"
    );
    assert!(
        raw.contains("SEGREDO-DA-ALICE"),
        "mensagem propria sai com corpo completo"
    );
    assert!(
        raw.contains("alice-canal"),
        "o nome do chat acompanha a mensagem"
    );

    let file_ids: Vec<&str> = v["files"]
        .as_array()
        .expect("files array")
        .iter()
        .filter_map(|f| f["file_id"].as_str())
        .collect();
    assert!(
        file_ids.contains(&alice_file.to_string().as_str()),
        "export deve listar o arquivo enviado pela Alice"
    );

    let mem_ids: Vec<&str> = v["memory_items"]
        .as_array()
        .expect("memory array")
        .iter()
        .filter_map(|m| m["memory_item_id"].as_str())
        .collect();
    assert!(
        mem_ids.contains(&alice_mem.to_string().as_str()),
        "export deve conter a memoria pessoal da Alice"
    );
    assert!(
        raw.contains("MEMORIA-DA-ALICE"),
        "memoria propria sai com conteudo completo"
    );

    let task_ids: Vec<&str> = v["tasks"]
        .as_array()
        .expect("tasks array")
        .iter()
        .filter_map(|t| t["task_id"].as_str())
        .collect();
    assert!(
        task_ids.contains(&alice_task.to_string().as_str()),
        "export deve conter a tarefa criada pela Alice"
    );
    let alice_task_entry = v["tasks"]
        .as_array()
        .expect("tasks array")
        .iter()
        .find(|t| t["task_id"].as_str() == Some(alice_task.to_string().as_str()))
        .expect("entrada da tarefa da Alice");
    assert_eq!(
        alice_task_entry["authored"], true,
        "tarefa criada pela titular vem marcada authored"
    );

    // Transparencia do escopo: as duas listas tem de existir.
    assert!(
        !v["exclusions"]
            .as_array()
            .expect("exclusions array")
            .is_empty(),
        "o export tem de declarar o que ficou de fora"
    );
    assert!(
        v["truncated_sections"].is_array(),
        "truncated_sections tem de existir mesmo vazio"
    );
    assert_eq!(
        v["truncated_sections"]
            .as_array()
            .expect("truncated array")
            .len(),
        0,
        "nada deveria ter batido no teto neste cenario pequeno"
    );

    // ── A delimitacao: NADA do Bob pode aparecer ───────────────────────
    for needle in [
        "SEGREDO-DO-BOB",
        "MEMORIA-DO-BOB",
        "TAREFA-DO-BOB",
        "bob-doc.txt",
        "bob-canal",
        "bob@dsr-export.test",
    ] {
        assert!(
            !raw.contains(needle),
            "export da Alice vazou dado do Bob: {needle:?} apareceu no corpo"
        );
    }
    for needle in [bob_group.to_string(), bob.to_string(), bob_msg.to_string()] {
        assert!(
            !raw.contains(&needle),
            "export da Alice vazou identificador do Bob: {needle}"
        );
    }

    // ── E o espelho: a exportacao do Bob nao ve a Alice ────────────────
    let resp = h
        .router
        .clone()
        .oneshot(get_authed("/v1/me/export", &bob_token))
        .await
        .expect("oneshot export bob");
    assert_eq!(resp.status(), StatusCode::OK);
    let raw_bob = serde_json::to_string(&body_json(resp).await).expect("reserialize");
    for needle in [
        "SEGREDO-DA-ALICE",
        "MEMORIA-DA-ALICE",
        "TAREFA-DA-ALICE",
        "alice-doc.txt",
    ] {
        assert!(
            !raw_bob.contains(needle),
            "export do Bob vazou dado da Alice: {needle:?}"
        );
    }

    // Audit do proprio pedido de exportacao.
    let actions = audit_actions_for_user(&h, alice).await;
    assert!(
        actions.contains(&"account.data_exported".to_string()),
        "exportacao tem de deixar rastro no audit; veio {actions:?}"
    );
}

// ─── Cenario 2: mencao recebida sai como trecho, nao como corpo ─────────

async fn mencao_recebida_sai_como_trecho_e_nao_como_corpo_alheio() {
    let h = Harness::get().await;

    let (alice, group, alice_token) = seed_user_with_group(&h, "alice@dsr-mention.test")
        .await
        .expect("seed alice");
    let (bob, bob_token) =
        common::fixtures::seed_member_via_admin(&h, group, "member", "bob@dsr-mention.test")
            .await
            .expect("seed bob no mesmo grupo");

    let chat = seed_chat(&h, group, alice, "canal-compartilhado").await;

    // O Bob escreve uma mensagem longa que cita a Alice. O corpo e dele.
    let cauda = "X".repeat(400);
    let corpo = format!("Oi @alice, veja isto: INICIO-DO-BOB {cauda} FIM-DO-BOB");
    let bob_msg = seed_message(&h, chat, group, bob, "Bob", &corpo).await;
    sqlx::query(
        "INSERT INTO message_mentions (message_id, mentioned_user_id, group_id) \
         VALUES ($1, $2, $3)",
    )
    .bind(bob_msg)
    .bind(alice)
    .bind(group)
    .execute(&h.admin_pool)
    .await
    .expect("seed mention");

    let resp = h
        .router
        .clone()
        .oneshot(get_authed("/v1/me/export", &alice_token))
        .await
        .expect("oneshot");
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;
    let raw = serde_json::to_string(&v).expect("reserialize");

    let mention = v["mentions_received"]
        .as_array()
        .expect("mentions array")
        .iter()
        .find(|m| m["message_id"].as_str() == Some(bob_msg.to_string().as_str()))
        .expect("a mencao recebida tem de estar no export");

    let excerpt = mention["body_excerpt"].as_str().expect("excerpt string");
    assert_eq!(
        excerpt.chars().count(),
        200,
        "o trecho tem de ter exatamente 200 caracteres, veio {}",
        excerpt.chars().count()
    );
    assert!(
        excerpt.contains("INICIO-DO-BOB"),
        "o trecho tem de comecar no inicio da mensagem"
    );
    assert!(
        !raw.contains("FIM-DO-BOB"),
        "o FIM da mensagem do Bob esta alem dos 200 caracteres e NAO pode sair \
         no export da Alice — o corpo pertence a quem escreveu"
    );

    // A mensagem do Bob nao e dela: nao entra em `messages`.
    let own: Vec<&str> = v["messages"]
        .as_array()
        .expect("messages array")
        .iter()
        .filter_map(|m| m["message_id"].as_str())
        .collect();
    assert!(
        !own.contains(&bob_msg.to_string().as_str()),
        "mensagem escrita pelo Bob nunca entra em `messages` da Alice"
    );

    // E o Bob, do lado dele, segue com a mensagem dele por inteiro.
    let resp = h
        .router
        .clone()
        .oneshot(get_authed("/v1/me/export", &bob_token))
        .await
        .expect("oneshot bob");
    let raw_bob = serde_json::to_string(&body_json(resp).await).expect("reserialize");
    assert!(
        raw_bob.contains("FIM-DO-BOB"),
        "o autor exporta o proprio corpo completo"
    );
}

// ─── Cenario 3: PATCH /v1/me — correcao de e-mail ───────────────────────

async fn patch_me_registra_pedido_de_email_e_nunca_troca_as_cegas() {
    let h = Harness::get().await;
    let (user, _group, token) = seed_user_with_group(&h, "rect@dsr-patch.test")
        .await
        .expect("seed user");

    // ── display_name segue aplicando na hora ───────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(json_authed(
            "PATCH",
            "/v1/me",
            &token,
            Some(json!({ "display_name": "Nome Novo" })),
        ))
        .await
        .expect("oneshot patch name");
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;
    assert_eq!(v["display_name"], "Nome Novo");
    assert!(
        v.get("email_change_pending").is_none(),
        "sem `email` no corpo, nao ha pedido pendente na resposta"
    );

    // ── e-mail: registra pedido, NAO troca ─────────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(json_authed(
            "PATCH",
            "/v1/me",
            &token,
            Some(json!({ "email": "novo@dsr-patch.test" })),
        ))
        .await
        .expect("oneshot patch email");
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;

    assert_eq!(
        v["email"], "rect@dsr-patch.test",
        "o e-mail devolvido tem de continuar sendo o ATUAL — a troca nao acontece aqui"
    );
    let pending = &v["email_change_pending"];
    assert_eq!(pending["requested_email"], "novo@dsr-patch.test");
    assert_eq!(pending["status"], "pending");
    let first_request_id = pending["request_id"]
        .as_str()
        .expect("request_id")
        .to_string();

    // E no banco: `users.email` intacto.
    let (db_email,): (String,) = sqlx::query_as("SELECT email::text FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&h.admin_pool)
        .await
        .expect("read users.email");
    assert_eq!(
        db_email, "rect@dsr-patch.test",
        "users.email NAO pode ter mudado"
    );

    // E a chave de login tambem: trocar so uma das duas quebraria o login.
    let identity: Option<(String,)> = sqlx::query_as(
        "SELECT provider_sub FROM user_identities WHERE user_id = $1 AND provider = 'internal'",
    )
    .bind(user)
    .fetch_optional(&h.admin_pool)
    .await
    .expect("read identity");
    if let Some((sub,)) = identity {
        assert_ne!(
            sub, "novo@dsr-patch.test",
            "provider_sub (chave de login) NAO pode ter mudado"
        );
    }

    // ── audit: so o tamanho, nunca o endereco ──────────────────────────
    let rows: Vec<(String, Value)> = sqlx::query_as(
        "SELECT action, metadata FROM audit_events \
         WHERE actor_user_id = $1 AND action = 'account.email_change_requested'",
    )
    .bind(user)
    .fetch_all(&h.admin_pool)
    .await
    .expect("read audit");
    assert_eq!(rows.len(), 1, "um pedido, um evento");
    let (_, meta) = &rows[0];
    assert_eq!(meta["requested_email_len"], "novo@dsr-patch.test".len());
    let meta_raw = serde_json::to_string(meta).expect("serialize metadata");
    assert!(
        !meta_raw.contains("novo@dsr-patch.test"),
        "o endereco pedido e PII e NAO pode aparecer no metadata do audit: {meta_raw}"
    );

    // ── um segundo pedido supersede o primeiro ─────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(json_authed(
            "PATCH",
            "/v1/me",
            &token,
            Some(json!({ "email": "terceiro@dsr-patch.test" })),
        ))
        .await
        .expect("oneshot patch email 2");
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;
    assert_eq!(
        v["email_change_pending"]["requested_email"],
        "terceiro@dsr-patch.test"
    );

    let statuses: Vec<(String, String)> = sqlx::query_as(
        "SELECT id::text, status FROM user_email_change_requests WHERE user_id = $1",
    )
    .bind(user)
    .fetch_all(&h.admin_pool)
    .await
    .expect("read requests");
    assert_eq!(statuses.len(), 2, "dois pedidos registrados");
    let first = statuses
        .iter()
        .find(|(id, _)| *id == first_request_id)
        .expect("o primeiro pedido");
    assert_eq!(
        first.1, "superseded",
        "o pedido anterior tem de ficar superseded, nao pendente em duplicidade"
    );
    let pendentes = statuses.iter().filter(|(_, s)| s == "pending").count();
    assert_eq!(pendentes, 1, "no maximo um pedido aberto por titular");

    // ── pedir o e-mail que ja e o atual: 400 ───────────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(json_authed(
            "PATCH",
            "/v1/me",
            &token,
            Some(json!({ "email": "RECT@dsr-patch.test" })),
        ))
        .await
        .expect("oneshot same email");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "o endereco atual (comparado sem caixa, como citext) e no-op -> 400"
    );

    // ── forma invalida: 400, sem gravar nada ───────────────────────────
    for bad in ["sem-arroba", "a@b@c.test", "  ", "a@"] {
        let resp = h
            .router
            .clone()
            .oneshot(json_authed(
                "PATCH",
                "/v1/me",
                &token,
                Some(json!({ "email": bad })),
            ))
            .await
            .expect("oneshot bad email");
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "e-mail {bad:?} deveria ser recusado"
        );
    }

    let total: (i64,) =
        sqlx::query_as("SELECT count(*) FROM user_email_change_requests WHERE user_id = $1")
            .bind(user)
            .fetch_one(&h.admin_pool)
            .await
            .expect("count requests");
    assert_eq!(
        total.0, 2,
        "pedido recusado na validacao nao pode gravar linha"
    );

    // ── o pedido aparece na exportacao do titular ──────────────────────
    let resp = h
        .router
        .clone()
        .oneshot(get_authed("/v1/me/export", &token))
        .await
        .expect("oneshot export");
    let v = body_json(resp).await;
    assert_eq!(
        v["email_change_requests"]
            .as_array()
            .expect("email_change_requests array")
            .len(),
        2,
        "a exportacao mostra ao titular os pedidos que ele fez"
    );
}

/// O pedido de correcao de um titular nao pode ser visivel a outro.
async fn pedido_de_email_nao_vaza_para_outro_titular() {
    let h = Harness::get().await;
    let (_alice, _ag, alice_token) = seed_user_with_group(&h, "alice@dsr-isol.test")
        .await
        .expect("seed alice");
    let (_bob, _bg, bob_token) = seed_user_with_group(&h, "bob@dsr-isol.test")
        .await
        .expect("seed bob");

    let resp = h
        .router
        .clone()
        .oneshot(json_authed(
            "PATCH",
            "/v1/me",
            &alice_token,
            Some(json!({ "email": "alice-secreto@dsr-isol.test" })),
        ))
        .await
        .expect("oneshot alice patch");
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = h
        .router
        .clone()
        .oneshot(get_authed("/v1/me/export", &bob_token))
        .await
        .expect("oneshot bob export");
    let raw = serde_json::to_string(&body_json(resp).await).expect("reserialize");
    assert!(
        !raw.contains("alice-secreto@dsr-isol.test"),
        "o e-mail pedido pela Alice nao pode aparecer na exportacao do Bob"
    );
}

// ─── Cenario 4: DELETE /v1/me enfileira o apagamento ────────────────────

async fn delete_me_enfileira_apagamento_com_carencia_e_e_idempotente() {
    let h = Harness::get().await;
    let (user, _group, token) = seed_user_with_group(&h, "del@dsr-delete.test")
        .await
        .expect("seed user");

    let resp = h
        .router
        .clone()
        .oneshot(json_authed("DELETE", "/v1/me", &token, None))
        .await
        .expect("oneshot delete");
    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "DELETE /v1/me mantem o contrato 204 sem corpo"
    );

    // Lapide + pedido na MESMA transacao: as duas coisas ou nenhuma.
    let (status,): (String,) = sqlx::query_as("SELECT status FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&h.admin_pool)
        .await
        .expect("read status");
    assert_eq!(status, "deleted");

    type ReqRow = (String, DateTime<Utc>, DateTime<Utc>);
    let (req_status, requested_at, purge_after): ReqRow = sqlx::query_as(
        "SELECT status, requested_at, purge_after FROM account_deletion_requests \
         WHERE user_id = $1",
    )
    .bind(user)
    .fetch_one(&h.admin_pool)
    .await
    .expect("pedido de apagamento tem de existir");
    assert_eq!(req_status, "pending");

    let grace = purge_after - requested_at;
    assert_eq!(
        grace.num_days(),
        DEFAULT_GRACE_PERIOD_DAYS,
        "a carencia gravada tem de ser a constante documentada"
    );

    // Sessoes revogadas.
    let (ativas,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM sessions \
         WHERE user_id = $1 AND revoked_at IS NULL AND expires_at > now()",
    )
    .bind(user)
    .fetch_one(&h.admin_pool)
    .await
    .expect("count sessions");
    assert_eq!(ativas, 0, "nenhuma sessao ativa deve sobrar");

    // Os DOIS eventos de audit.
    let actions = audit_actions_for_user(&h, user).await;
    assert!(
        actions.contains(&"account.self_deleted".to_string()),
        "falta account.self_deleted em {actions:?}"
    );
    assert!(
        actions.contains(&"account.purge_scheduled".to_string()),
        "falta account.purge_scheduled em {actions:?}"
    );

    // Repetir: 409 e nenhum pedido novo.
    let resp = h
        .router
        .clone()
        .oneshot(json_authed("DELETE", "/v1/me", &token, None))
        .await
        .expect("oneshot delete 2");
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    let (n,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM account_deletion_requests WHERE user_id = $1")
            .bind(user)
            .fetch_one(&h.admin_pool)
            .await
            .expect("count requests");
    assert_eq!(n, 1, "o segundo DELETE nao pode enfileirar outro pedido");

    // Conta sob apagamento nao aceita correcao de perfil.
    let resp = h
        .router
        .clone()
        .oneshot(json_authed(
            "PATCH",
            "/v1/me",
            &token,
            Some(json!({ "display_name": "Tentativa" })),
        ))
        .await
        .expect("oneshot patch after delete");
    assert_eq!(
        resp.status(),
        StatusCode::CONFLICT,
        "perfil de conta em apagamento nao aceita escrita"
    );
}

// ─── Cenario 5: o worker, a carencia e a protecao de terceiros ──────────

async fn purge_nao_toca_dado_de_terceiro() {
    let h = Harness::get().await;
    let tmp = tempfile::tempdir().expect("tempdir");
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFs::new(tmp.path()).expect("LocalFs para o teste"));
    let config = AccountPurgeWorkerConfig::default();

    // Alice e Bob no MESMO grupo e no MESMO chat.
    let (alice, group, alice_token) = seed_user_with_group(&h, "alice@dsr-purge.test")
        .await
        .expect("seed alice");
    let (bob, _bob_token) =
        common::fixtures::seed_member_via_admin(&h, group, "member", "bob@dsr-purge.test")
            .await
            .expect("seed bob");

    let chat = seed_chat(&h, group, alice, "canal-do-time").await;
    let alice_msg = seed_message(&h, chat, group, alice, "Alice", "CORPO-DA-ALICE").await;
    let bob_msg = seed_message(&h, chat, group, bob, "Bob", "CORPO-DO-BOB").await;

    // Um arquivo da Alice, com o blob de verdade no store.
    let alice_key = format!("{group}/files/alice-purge/v1");
    let alice_file = seed_file_with_version(&h, group, alice, "alice-purge.txt", &alice_key).await;
    store
        .put(
            &alice_key,
            Bytes::from_static(b"bytes-alice"),
            PutOptions::default(),
        )
        .await
        .expect("put blob da alice");

    // Um arquivo do Bob, idem — tem de sobreviver.
    let bob_key = format!("{group}/files/bob-purge/v1");
    let bob_file = seed_file_with_version(&h, group, bob, "bob-purge.txt", &bob_key).await;
    store
        .put(
            &bob_key,
            Bytes::from_static(b"bytes-bob"),
            PutOptions::default(),
        )
        .await
        .expect("put blob do bob");

    let alice_mem = seed_personal_memory(&h, alice, "MEMORIA-PURGE-ALICE").await;
    let bob_mem = seed_personal_memory(&h, bob, "MEMORIA-PURGE-BOB").await;

    // Um thread com título e uma versão de doc por cada um: os dois caminhos
    // de redação (não de remoção) da migration 034.
    let alice_thread = seed_thread(&h, chat, alice_msg, alice, "TITULO-DA-ALICE").await;
    let bob_thread = seed_thread(&h, chat, bob_msg, bob, "TITULO-DO-BOB").await;
    let alice_version = seed_doc_version(&h, group, alice).await;
    let bob_version = seed_doc_version(&h, group, bob).await;

    // ── 1. Pedido feito, carencia CORRENDO: o worker nao toca em nada ──
    let resp = h
        .router
        .clone()
        .oneshot(json_authed("DELETE", "/v1/me", &alice_token, None))
        .await
        .expect("oneshot delete");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let report = run_purge_tick(
        h.purge_pool.clone(),
        h.app_pool.clone(),
        Some(store.clone()),
        &config,
    )
    .await
    .expect("tick durante a carencia");
    assert_eq!(
        report.claimed, 0,
        "pedido dentro da carencia NAO pode ser executado; veio {report:?}"
    );

    let (body,): (String,) = sqlx::query_as("SELECT body FROM messages WHERE id = $1")
        .bind(alice_msg)
        .fetch_one(&h.admin_pool)
        .await
        .expect("read alice body");
    assert_eq!(
        body, "CORPO-DA-ALICE",
        "durante a carencia o conteudo tem de continuar intacto"
    );

    // ── 2. Carencia vencida: o worker executa ──────────────────────────
    make_purge_due(&h, alice).await;

    let report = run_purge_tick(
        h.purge_pool.clone(),
        h.app_pool.clone(),
        Some(store.clone()),
        &config,
    )
    .await
    .expect("tick depois da carencia");
    assert_eq!(report.claimed, 1, "o pedido devido tem de ser reivindicado");
    assert_eq!(report.purged, 1, "e concluido; veio {report:?}");
    assert_eq!(report.failed, 0, "sem falhas; veio {report:?}");
    assert_eq!(report.blobs_deleted, 1, "o blob da Alice tem de sair");

    // ── 3. O dado da Alice foi destruido ───────────────────────────────
    let (body, label, deleted_at): (String, String, Option<DateTime<Utc>>) =
        sqlx::query_as("SELECT body, sender_label, deleted_at FROM messages WHERE id = $1")
            .bind(alice_msg)
            .fetch_one(&h.admin_pool)
            .await
            .expect("read alice msg");
    assert!(
        !body.contains("CORPO-DA-ALICE"),
        "o corpo da mensagem da Alice tem de estar destruido, veio {body:?}"
    );
    assert!(
        !label.contains("Alice"),
        "o sender_label e copia do display_name e tem de cair, veio {label:?}"
    );
    assert!(deleted_at.is_some(), "a mensagem fica marcada como apagada");

    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM files WHERE id = $1")
        .bind(alice_file)
        .fetch_one(&h.admin_pool)
        .await
        .expect("count alice file");
    assert_eq!(n, 0, "o arquivo da Alice tem de sair do banco");
    assert!(
        !store.exists(&alice_key).await.expect("exists alice key"),
        "o BLOB da Alice tem de sair do object store, nao so a linha"
    );

    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM memory_items WHERE id = $1")
        .bind(alice_mem)
        .fetch_one(&h.admin_pool)
        .await
        .expect("count alice mem");
    assert_eq!(n, 0, "a memoria da Alice tem de sair");

    // Redação, não remoção: o título do thread da Alice (dado pessoal) cai,
    // mas a linha fica — o thread é espaço compartilhado do chat.
    let (titulo,): (Option<String>,) =
        sqlx::query_as("SELECT title FROM message_threads WHERE id = $1")
            .bind(alice_thread)
            .fetch_one(&h.admin_pool)
            .await
            .expect("read alice thread");
    assert_eq!(
        titulo, None,
        "o titulo do thread criado pela Alice tem de ser zerado"
    );

    // Idem para a versão de doc: `created_by` é UUID NOT NULL sem FK e
    // sobrevive como pseudônimo; o label vira lápide.
    let (label,): (String,) =
        sqlx::query_as("SELECT created_by_label FROM doc_page_versions WHERE id = $1")
            .bind(alice_version)
            .fetch_one(&h.admin_pool)
            .await
            .expect("read alice doc version");
    assert_eq!(
        label, "Usuario Removido",
        "o label da versao de doc da Alice tem de virar lapide"
    );

    // Identificadores: lapide sem dado pessoal.
    let (email, display_name, status): (String, String, String) =
        sqlx::query_as("SELECT email::text, display_name, status FROM users WHERE id = $1")
            .bind(alice)
            .fetch_one(&h.admin_pool)
            .await
            .expect("read alice user row");
    assert_eq!(status, "purged");
    assert!(
        !email.contains("alice@dsr-purge.test"),
        "o e-mail tem de virar token, veio {email:?}"
    );
    assert_eq!(email, format!("anon-{}@garraanon.local", alice.simple()));
    assert!(!display_name.contains("alice"), "display_name tem de cair");

    // Credencial destruida.
    let hash: Option<(Option<String>,)> =
        sqlx::query_as("SELECT password_hash FROM user_identities WHERE user_id = $1")
            .bind(alice)
            .fetch_optional(&h.admin_pool)
            .await
            .expect("read identity");
    if let Some((h_opt,)) = hash {
        assert!(h_opt.is_none(), "password_hash tem de ficar NULL");
    }

    // Audit desidentificado, mas presente — e a prova do apagamento.
    let (restantes,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM audit_events \
         WHERE actor_user_id = $1 AND (ip IS NOT NULL OR user_agent IS NOT NULL \
               OR actor_label IS NOT NULL)",
    )
    .bind(alice)
    .fetch_one(&h.admin_pool)
    .await
    .expect("count identified audit");
    assert_eq!(
        restantes, 0,
        "nenhum evento de audit pode continuar carregando ip/user_agent/actor_label"
    );

    let actions = audit_actions_for_user(&h, alice).await;
    assert!(
        actions.contains(&"account.purged".to_string()),
        "o apagamento tem de ser rastreavel por audit; veio {actions:?}"
    );

    // O relatorio e estrutural: nunca o conteudo apagado.
    let (relatorio,): (Value,) =
        sqlx::query_as("SELECT purge_report FROM account_deletion_requests WHERE user_id = $1")
            .bind(alice)
            .fetch_one(&h.admin_pool)
            .await
            .expect("read purge_report");
    let relatorio_raw = serde_json::to_string(&relatorio).expect("serialize report");
    assert!(
        !relatorio_raw.contains("CORPO-DA-ALICE") && !relatorio_raw.contains("MEMORIA-PURGE-ALICE"),
        "o relatorio de apagamento nao pode guardar o conteudo apagado: {relatorio_raw}"
    );

    // ── 4. O QUE MAIS IMPORTA: o dado do Bob esta intacto ──────────────
    let (bob_body, bob_label): (String, String) =
        sqlx::query_as("SELECT body, sender_label FROM messages WHERE id = $1")
            .bind(bob_msg)
            .fetch_one(&h.admin_pool)
            .await
            .expect("read bob msg");
    assert_eq!(
        bob_body, "CORPO-DO-BOB",
        "a mensagem do Bob, no MESMO chat, nao pode ser tocada pelo pedido da Alice"
    );
    assert_eq!(bob_label, "Bob", "nem o rotulo dele");

    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM files WHERE id = $1")
        .bind(bob_file)
        .fetch_one(&h.admin_pool)
        .await
        .expect("count bob file");
    assert_eq!(n, 1, "o arquivo do Bob tem de sobreviver");
    assert!(
        store.exists(&bob_key).await.expect("exists bob key"),
        "o blob do Bob tem de sobreviver"
    );

    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM memory_items WHERE id = $1")
        .bind(bob_mem)
        .fetch_one(&h.admin_pool)
        .await
        .expect("count bob mem");
    assert_eq!(n, 1, "a memoria do Bob tem de sobreviver");

    // Redações do apagamento são por `created_by`: nada do Bob pode mudar.
    let (titulo,): (Option<String>,) =
        sqlx::query_as("SELECT title FROM message_threads WHERE id = $1")
            .bind(bob_thread)
            .fetch_one(&h.admin_pool)
            .await
            .expect("read bob thread");
    assert_eq!(
        titulo.as_deref(),
        Some("TITULO-DO-BOB"),
        "o titulo do thread do Bob nao pode ser tocado pelo pedido da Alice"
    );

    let (label,): (String,) =
        sqlx::query_as("SELECT created_by_label FROM doc_page_versions WHERE id = $1")
            .bind(bob_version)
            .fetch_one(&h.admin_pool)
            .await
            .expect("read bob doc version");
    assert_eq!(
        label, "Seed Owner",
        "o label da versao de doc do Bob nao pode ser tocado"
    );

    let (bob_email,): (String,) = sqlx::query_as("SELECT email::text FROM users WHERE id = $1")
        .bind(bob)
        .fetch_one(&h.admin_pool)
        .await
        .expect("read bob email");
    assert_eq!(
        bob_email, "bob@dsr-purge.test",
        "a identidade do Bob nao pode ser tocada"
    );

    // O chat que a Alice criou fica: a equipe depende dele.
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM chats WHERE id = $1")
        .bind(chat)
        .fetch_one(&h.admin_pool)
        .await
        .expect("count chat");
    assert_eq!(
        n, 1,
        "o chat criado pela Alice e espaco compartilhado e tem de sobreviver"
    );

    // ── 5. Idempotencia: um segundo tick nao acha mais nada ────────────
    let report = run_purge_tick(
        h.purge_pool.clone(),
        h.app_pool.clone(),
        Some(store.clone()),
        &config,
    )
    .await
    .expect("tick idempotente");
    assert_eq!(
        report.claimed, 0,
        "pedido concluido nao pode ser reivindicado de novo; veio {report:?}"
    );
}

/// Sem `ObjectStore` ligado e com blob para remover, o pedido NAO fecha.
/// Fechar como `completed` com blob vivo seria declarar no audit um
/// apagamento que nao ocorreu.
async fn purge_sem_object_store_nao_fecha_o_pedido() {
    let h = Harness::get().await;
    let config = AccountPurgeWorkerConfig::default();

    let (user, group, token) = seed_user_with_group(&h, "nostore@dsr-purge.test")
        .await
        .expect("seed user");
    let key = format!("{group}/files/nostore/v1");
    seed_file_with_version(&h, group, user, "nostore.txt", &key).await;

    let resp = h
        .router
        .clone()
        .oneshot(json_authed("DELETE", "/v1/me", &token, None))
        .await
        .expect("oneshot delete");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    make_purge_due(&h, user).await;

    let report = run_purge_tick(h.purge_pool.clone(), h.app_pool.clone(), None, &config)
        .await
        .expect("tick sem store");
    assert_eq!(report.claimed, 1);
    assert_eq!(
        report.purged, 0,
        "sem backend de storage o pedido NAO pode ser fechado; veio {report:?}"
    );
    assert_eq!(report.failed, 1);

    let (status, attempts, last_error): (String, i32, Option<String>) = sqlx::query_as(
        "SELECT status, attempts, last_error FROM account_deletion_requests WHERE user_id = $1",
    )
    .bind(user)
    .fetch_one(&h.admin_pool)
    .await
    .expect("read request");
    assert_eq!(
        status, "pending",
        "com tentativa sobrando o pedido volta para a fila, nao morre"
    );
    assert_eq!(attempts, 1);
    assert!(
        last_error.unwrap_or_default().contains("ObjectStore"),
        "o motivo tecnico tem de ficar registrado para o operador"
    );

    let actions = audit_actions_for_user(&h, user).await;
    assert!(
        actions.contains(&"account.purge_failed".to_string()),
        "a falha tem de ser visivel no audit, nao so no log; veio {actions:?}"
    );

    // As chaves ficaram persistidas para a retomada nao perder o blob.
    let (relatorio,): (Value,) =
        sqlx::query_as("SELECT purge_report FROM account_deletion_requests WHERE user_id = $1")
            .bind(user)
            .fetch_one(&h.admin_pool)
            .await
            .expect("read report");
    let keys = relatorio["object_keys"]
        .as_array()
        .expect("object_keys tem de estar persistido");
    assert!(
        keys.iter().any(|k| k.as_str() == Some(key.as_str())),
        "a object_key tem de sobreviver ao DELETE das linhas, senao o blob fica orfao"
    );

    // ── Retomada end-to-end: o store aparece e o proximo tick conclui ──
    // O pedido esta `pending` com `db_purged` e as chaves persistidas. O
    // blob, que sempre existiu so no limbo entre banco e storage, agora e
    // colocado num store de verdade: o proximo tick tem de retomar pelas
    // chaves do relatorio (sem reexecutar o passo de banco) e fechar.
    let tmp = tempfile::tempdir().expect("tempdir da retomada");
    let store: Arc<dyn ObjectStore> =
        Arc::new(LocalFs::new(tmp.path()).expect("LocalFs da retomada"));
    store
        .put(
            &key,
            Bytes::from_static(b"bytes-nostore"),
            PutOptions::default(),
        )
        .await
        .expect("put blob na retomada");

    let report = run_purge_tick(
        h.purge_pool.clone(),
        h.app_pool.clone(),
        Some(store.clone()),
        &config,
    )
    .await
    .expect("tick de retomada");
    assert_eq!(
        report.claimed, 1,
        "o pedido voltou para a fila e tem de ser reivindicado de novo; veio {report:?}"
    );
    assert_eq!(
        report.purged, 1,
        "com store disponivel a retomada tem de concluir o apagamento; veio {report:?}"
    );
    assert_eq!(
        report.blobs_deleted, 1,
        "o blob pendente tem de sair na retomada; veio {report:?}"
    );
    assert!(
        !store.exists(&key).await.expect("exists key na retomada"),
        "o blob tem de sumir do store na retomada"
    );

    let (status,): (String,) =
        sqlx::query_as("SELECT status FROM account_deletion_requests WHERE user_id = $1")
            .bind(user)
            .fetch_one(&h.admin_pool)
            .await
            .expect("read status pos-retomada");
    assert_eq!(
        status, "completed",
        "a retomada tem de fechar o pedido como completed"
    );

    let actions = audit_actions_for_user(&h, user).await;
    assert!(
        actions.contains(&"account.purged".to_string()),
        "a retomada concluida tem de emitir o audit de apagamento; veio {actions:?}"
    );
}

/// A carencia nao e so configuracao: um pedido com `purge_after` no futuro
/// nunca e reivindicado, por mais ticks que rodem.
async fn carencia_bloqueia_o_worker_ate_vencer() {
    let h = Harness::get().await;
    let config = AccountPurgeWorkerConfig {
        // Janela de stale curta para provar que o bloqueio vem da carencia,
        // nao de o pedido estar "fresco".
        stale_after: Duration::from_secs(1),
        ..AccountPurgeWorkerConfig::default()
    };

    let (user, _group, token) = seed_user_with_group(&h, "grace@dsr-purge.test")
        .await
        .expect("seed user");
    let resp = h
        .router
        .clone()
        .oneshot(json_authed("DELETE", "/v1/me", &token, None))
        .await
        .expect("oneshot delete");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    for _ in 0..3 {
        run_purge_tick(h.purge_pool.clone(), h.app_pool.clone(), None, &config)
            .await
            .expect("tick dentro da carencia");
    }

    // `attempts` sobe a cada reivindicacao, entao zero prova que ESTE pedido
    // nunca foi tocado. Asserir sobre o contador global do tick seria fragil:
    // um cenario vizinho pode ter deixado outro pedido devido na fila, que o
    // tick desta carencia reivindicaria e concluaria — o report global diria
    // "1" sem provar nada sobre o pedido deste cenario.
    let (status, attempts): (String, i32) =
        sqlx::query_as("SELECT status, attempts FROM account_deletion_requests WHERE user_id = $1")
            .bind(user)
            .fetch_one(&h.admin_pool)
            .await
            .expect("read status");
    assert_eq!(status, "pending", "o pedido continua apenas enfileirado");
    assert_eq!(
        attempts, 0,
        "pedido dentro da carencia NUNCA pode ser reivindicado"
    );

    let (user_status,): (String,) = sqlx::query_as("SELECT status FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&h.admin_pool)
        .await
        .expect("read user status");
    assert_eq!(
        user_status, "deleted",
        "dentro da carencia a conta e lapide, ainda nao purgada"
    );
}

// ─── Entrypoint unico ────────────────────────────────────────────────────
//
// UMA funcao `#[tokio::test]`, nao oito. Cada `#[tokio::test]` sobe o proprio
// runtime tokio e o derruba ao retornar; os pools do `Harness` sao
// compartilhados pelo processo, e uma conexao ociosa criada dentro de um
// runtime ja encerrado faz o `acquire` seguinte estourar com
// `pool timed out while waiting for an open connection`. Rodar os oito como
// testes separados reproduziu exatamente isso (7 de 8 falharam no
// `fixture tx begin`), que e a mesma armadilha registrada no docblock de
// `rest_v1_me_authed.rs`.
//
// Um runtime, uma sequencia linear de acquire/release: deterministico. A
// falha continua apontando o cenario — o nome aparece no panic e cada assert
// carrega a sua mensagem.
#[tokio::test]
async fn direitos_dos_titulares_cenarios() {
    export_inclui_conteudo_e_nunca_alcanca_outro_tenant().await;
    mencao_recebida_sai_como_trecho_e_nao_como_corpo_alheio().await;
    patch_me_registra_pedido_de_email_e_nunca_troca_as_cegas().await;
    pedido_de_email_nao_vaza_para_outro_titular().await;
    delete_me_enfileira_apagamento_com_carencia_e_e_idempotente().await;
    purge_nao_toca_dado_de_terceiro().await;
    purge_sem_object_store_nao_fecha_o_pedido().await;
    carencia_bloqueia_o_worker_ate_vencer().await;
}
