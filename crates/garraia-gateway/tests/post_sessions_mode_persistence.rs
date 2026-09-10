//! #1102: `POST /api/sessions` com `mode` tem de gravar
//! `agent_mode`/`agent_mode_source` no metadado persistente — a mesma leitura
//! que o executor faz para aplicar a `ToolPolicy` (#988).
//!
//! O 201 ecoava o modo e a linha nascia sem ele: `create_session` grava o modo
//! e **depois** emite o token de sessao, e
//! `ChatSessionManager::create_token` chama `upsert_session(..., Value::Null)`
//! para garantir a linha antes do FK. `json_patch(T, P)` devolve `P` quando `P`
//! nao e objeto, entao aquele "no-op" reescrevia o metadado inteiro com `null`
//! por cima do modo recem-gravado. O mesmo `upsert` explica por que
//! `POST /api/mode/select` sempre funcionou: aquele caminho nao emite token.

use std::net::TcpListener;

use garraia_config::AppConfig;
use garraia_gateway::GatewayServer;
use serde_json::json;
use serial_test::serial;

fn random_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind to random port");
    listener.local_addr().unwrap().port()
}

#[tokio::test]
#[serial]
async fn post_sessions_applies_requested_mode_to_persistent_metadata() {
    let port = random_port();
    let mut config = AppConfig::default();
    config.gateway.port = port;
    config.memory.enabled = false;
    config.mcp.clear();

    let tmp = tempfile::tempdir().expect("create temp config dir");
    // SAFETY: teste single-thread (serial) sem outros leitores destas env vars.
    unsafe {
        std::env::set_var("GARRAIA_CONFIG_DIR", tmp.path().to_str().unwrap());
        std::env::set_var(
            garraia_gateway::mcp::McpPersistenceService::DISABLE_AUTOPROVISION_ENV,
            "1",
        );
        // O data_dir default e ~/.garraia/data; aponta para o tempdir para o
        // teste nao tocar o sessions.db do usuario.
        config.data_dir = Some(tmp.path().join("data"));
    }

    tokio::spawn(async move {
        let server = GatewayServer::new(config);
        let _ = server.run().await;
    });

    // Espera o gateway aceitar conexoes.
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .expect("build client");
    let base = format!("http://127.0.0.1:{port}");
    let mut up = false;
    for _ in 0..60 {
        if reqwest::get(format!("{base}/ping")).await.is_ok() {
            up = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    assert!(up, "gateway nao subiu");

    // 1. Criar sessao JA COM modo.
    let created: serde_json::Value = client
        .post(format!("{base}/api/sessions"))
        .json(&json!({ "agent_id": "reachy_voice", "mode": "search" }))
        .send()
        .await
        .expect("POST /api/sessions")
        .json()
        .await
        .expect("JSON da resposta");
    assert_eq!(created["mode"], "search", "resposta deve ecoar o modo");
    let session_id = created["session_id"].as_str().unwrap().to_string();

    // 2. O modo tem de valer para a propria sessao — era aqui que o 201
    //    mentia: o metadado voltava `null` e o modo nunca chegava ao executor.
    let current: serde_json::Value = client
        .get(format!("{base}/api/mode/current"))
        .header("x-session-id", &session_id)
        .send()
        .await
        .expect("GET /api/mode/current")
        .json()
        .await
        .unwrap();
    assert_eq!(
        current["mode"], "search",
        "o modo pedido na criacao tem de valer para a sessao"
    );

    // 3. E tem de ser um modo ESCOLHIDO, nao deduzido: e o que liga a
    //    ToolPolicy. `get_chosen_agent_mode` so aceita `source == "user"`.
    let db = tmp.path().join("data").join("sessions.db");
    assert!(db.exists(), "o sessions.db do tempdir deveria existir");
    let conn = rusqlite::Connection::open(&db).expect("abrir sessions.db");
    let metadata: Option<String> = conn
        .query_row(
            "SELECT metadata FROM sessions WHERE id = ?1",
            rusqlite::params![session_id],
            |row| row.get(0),
        )
        .expect("linha da sessao");
    let metadata: serde_json::Value =
        serde_json::from_str(metadata.as_deref().unwrap_or("null")).expect("metadado e JSON");
    assert_eq!(
        metadata["agent_mode"], "search",
        "o metadado tem de carregar o modo"
    );
    assert_eq!(
        metadata["agent_mode_source"], "user",
        "modo pedido na criacao e escolha do usuario, nao deducao do auto-router"
    );
}
