//! #1433: a elevacao sensivel exige confirmacao explicita, ENFORCADA no
//! servidor. Um `POST /admin/api/whatsapp/access` que concederia a um
//! principal uma classe mutante que ele nao tinha e recusado com 409
//! `elevation_confirmation_required` sem `confirm_elevation: true` — e nada e
//! gravado. Com a confirmacao, aplica. Mudancas que so TIRAM nao precisam de
//! confirmacao.
//!
//! Um so `#[tokio::test]` por arquivo: `GARRAIA_CONFIG_DIR` e processo-wide.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use garraia_agents::{AgentRuntime, McpManager};
use garraia_channels::ChannelRegistry;
use garraia_config::{AppConfig, ChannelConfig, ConfigLoader};
use garraia_gateway::admin::middleware::AuthenticatedAdmin;
use garraia_gateway::admin::rbac::Role;
use garraia_gateway::admin::shared::AdminState;
use garraia_gateway::admin::store::AdminStore;
use garraia_gateway::admin::whatsapp_access::{
    AccessMutationRequest, admin_whatsapp_access_mutate,
};
use garraia_gateway::state::AppState;
use tokio::sync::Mutex;

const NUMERO: &str = "5511999998888";

fn admin() -> axum::Extension<AuthenticatedAdmin> {
    axum::Extension(AuthenticatedAdmin {
        user_id: "u-1433".to_string(),
        username: "operadora".to_string(),
        role: Role::Admin,
        csrf_token: "csrf".to_string(),
        session_token: "sess".to_string(),
    })
}

fn pedido(action: &str) -> AccessMutationRequest {
    AccessMutationRequest {
        action: action.to_string(),
        identity: Some(format!("+{NUMERO}")),
        identity_last4: None,
        jid: None,
        level: None,
        write: None,
        preset: None,
        enabled: None,
        dry_run: false,
        confirm_elevation: false,
    }
}

async fn corpo(resp: axum::response::Response) -> (StatusCode, serde_json::Value) {
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .expect("body");
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

fn write_do_usuario(dir: &std::path::Path) -> Option<bool> {
    let relido = ConfigLoader::with_dir(dir).load_sem_env().expect("load");
    relido.channels["whatsapp_linked"].settings["access"]["users"][NUMERO]
        .get("write")
        .and_then(serde_json::Value::as_bool)
}

#[tokio::test]
async fn elevacao_sensivel_exige_confirmacao_no_servidor() {
    let dir = tempfile::tempdir().expect("temp config dir");
    unsafe {
        std::env::set_var("GARRAIA_CONFIG_DIR", dir.path());
    }
    let loader = ConfigLoader::with_dir(dir.path());
    loader.ensure_dirs().expect("dirs");

    let mut config = AppConfig::default();
    let mut settings = std::collections::HashMap::new();
    settings.insert("default_mode".to_string(), serde_json::json!("code"));
    settings.insert(
        "access".to_string(),
        serde_json::json!({
            "users": { NUMERO: { "level": "read", "write": false } }
        }),
    );
    config.channels.insert(
        "whatsapp_linked".to_string(),
        ChannelConfig {
            channel_type: "whatsapp_linked".to_string(),
            enabled: Some(true),
            settings,
        },
    );
    config.data_dir = Some(dir.path().join("data"));
    loader.save(&config).expect("save");

    let mut state = AppState::new(
        config.clone(),
        Arc::new(AgentRuntime::new()),
        ChannelRegistry::new(),
    );
    state.mcp_manager_arc = Some(Arc::new(McpManager::new()));
    let admin_state = AdminState {
        store: Arc::new(Mutex::new(
            AdminStore::in_memory().expect("in-memory admin store"),
        )),
        app_state: Arc::new(state),
        encryption_key: Arc::new(vec![0u8; 32]),
    };

    assert_eq!(
        write_do_usuario(dir.path()),
        Some(false),
        "baseline: sem escrita"
    );

    // ── dry_run: MOSTRA a elevacao, sem gravar nem recusar ──
    let mut req = pedido("write");
    req.write = Some(true);
    req.dry_run = true;
    let (status, doc) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(),
            Json(req),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    let elevacao = doc["elevation"].as_array().expect("elevation");
    assert!(
        elevacao.iter().any(|e| e["classes"]
            .as_array()
            .is_some_and(|cs| cs.iter().any(|c| c == "filesystem.write"))),
        "a elevacao nomeia a classe sensivel ganha: {doc}"
    );
    assert_eq!(
        write_do_usuario(dir.path()),
        Some(false),
        "dry_run nao grava"
    );

    // ── real SEM confirmacao: 409, nada gravado ──
    let mut req = pedido("write");
    req.write = Some(true);
    let (status, doc) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(),
            Json(req),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{doc}");
    assert_eq!(
        doc["error_code"],
        serde_json::json!("elevation_confirmation_required"),
        "{doc}"
    );
    assert_eq!(doc["written"], serde_json::json!(false));
    assert!(!doc["elevation"].as_array().expect("elevation").is_empty());
    assert_eq!(
        write_do_usuario(dir.path()),
        Some(false),
        "a recusa vem antes de qualquer escrita"
    );

    // ── real COM confirmacao: aplica ──
    let mut req = pedido("write");
    req.write = Some(true);
    req.confirm_elevation = true;
    let (status, doc) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(),
            Json(req),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    assert_eq!(doc["written"], serde_json::json!(true), "{doc}");
    assert_eq!(
        write_do_usuario(dir.path()),
        Some(true),
        "confirmada, gravou"
    );

    // ── tirar nao e elevacao: write off sem confirmacao aplica ──
    let mut req = pedido("write");
    req.write = Some(false);
    let (status, doc) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(),
            Json(req),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "tirar nunca pede confirmacao: {doc}"
    );
    assert_eq!(doc["written"], serde_json::json!(true));
    assert_eq!(write_do_usuario(dir.path()), Some(false));

    unsafe {
        std::env::remove_var("GARRAIA_CONFIG_DIR");
    }
}
