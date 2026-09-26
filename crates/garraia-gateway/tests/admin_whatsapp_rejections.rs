//! #1422: `GET /admin/api/whatsapp/access` traz as mensagens recusadas pelo
//! portao — contagens por motivo, recentes mascaradas (`…1234`), retencao — e
//! `POST /admin/api/whatsapp/access/rejections/reset` zera com audit. Nunca a
//! identidade inteira; leitura com Channels/Read, reset com Channels/Update.
//!
//! Um so `#[tokio::test]` por arquivo: `GARRAIA_CONFIG_DIR` e processo-wide.

use std::sync::Arc;

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
    admin_whatsapp_access, admin_whatsapp_access_rejections_reset,
};
use garraia_gateway::bootstrap::whatsapp_linked_rejeicoes::Motivo;
use garraia_gateway::state::AppState;
use tokio::sync::Mutex;

const ESTRANHO: &str = "5521955554444@s.whatsapp.net";
const BLOQUEADO: &str = "5511966665555";
const LID: &str = "87654321098765@lid";

fn admin(role: Role) -> axum::Extension<AuthenticatedAdmin> {
    axum::Extension(AuthenticatedAdmin {
        user_id: "u-1422".to_string(),
        username: "operadora".to_string(),
        role,
        csrf_token: "csrf".to_string(),
        session_token: "sess".to_string(),
    })
}

async fn corpo(resp: axum::response::Response) -> (StatusCode, serde_json::Value) {
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .expect("body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

#[tokio::test]
async fn a_api_admin_mostra_rejeicoes_mascaradas_e_zera_com_audit() {
    let dir = tempfile::tempdir().expect("tempdir");
    // SAFETY: teste isolado; o loader le `GARRAIA_CONFIG_DIR` no `new()`.
    unsafe { std::env::set_var("GARRAIA_CONFIG_DIR", dir.path()) };
    let mut config = AppConfig::default();
    let mut settings = std::collections::HashMap::new();
    settings.insert("allow".to_string(), serde_json::json!(["5511999998888"]));
    config.channels.insert(
        "whatsapp_linked".to_string(),
        ChannelConfig {
            channel_type: "whatsapp_linked".to_string(),
            enabled: Some(true),
            settings,
        },
    );
    ConfigLoader::new()
        .expect("loader")
        .save(&config)
        .expect("config no disco");

    let mut state = AppState::new(
        config,
        Arc::new(AgentRuntime::new()),
        ChannelRegistry::new(),
    );
    state.mcp_manager_arc = Some(Arc::new(McpManager::new()));
    let state = Arc::new(state);
    // O que o turno registraria: um estranho, um bloqueado, um LID sem numero.
    state
        .whatsapp_linked
        .registrar_rejeicao(Motivo::Restrita, ESTRANHO, false);
    state
        .whatsapp_linked
        .registrar_rejeicao(Motivo::Restrita, ESTRANHO, false);
    state
        .whatsapp_linked
        .registrar_rejeicao(Motivo::Bloqueado, BLOQUEADO, true);
    state.whatsapp_linked.registrar_recusa_lid(LID);

    let admin_state = AdminState {
        store: Arc::new(Mutex::new(
            AdminStore::in_memory().expect("in-memory admin store"),
        )),
        app_state: Arc::clone(&state),
        encryption_key: Arc::new(vec![0u8; 32]),
    };

    // GET (Viewer basta): contagens, recentes mascaradas, retencao, remediacao.
    let (status, doc) = corpo(
        admin_whatsapp_access(State(admin_state.clone()), admin(Role::Viewer))
            .await
            .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    let rej = &doc["rejections"];
    assert_eq!(rej["total"], 4, "{rej}");
    assert_eq!(rej["by_reason"]["restricted_policy"], 2);
    assert_eq!(rej["by_reason"]["blocked_user"], 1);
    assert_eq!(rej["by_reason"]["unresolved_lid"], 1);
    assert_eq!(
        rej["by_reason"]["channel_disabled"], 0,
        "todo motivo, mesmo zerado"
    );
    let recent = rej["recent"].as_array().expect("recent");
    assert_eq!(recent.len(), 4);
    assert_eq!(recent[0]["reason"], "unresolved_lid", "mais nova primeiro");
    assert_eq!(recent[0]["identity_last4"], "…8765");
    assert_eq!(recent[1]["reason"], "blocked_user");
    assert_eq!(recent[1]["identity_last4"], "…5555");
    assert_eq!(recent[1]["group"], true);
    assert!(recent[0]["at"].as_str().is_some_and(|s| s.ends_with('Z')));
    assert_eq!(rej["retention"]["recent_max"], 50);
    assert_eq!(rej["retention"]["scope"], "since_boot");
    assert!(
        rej["remediation"]["unresolved_lid"]
            .as_str()
            .is_some_and(|s| s.contains("/pair"))
    );
    let texto = doc.to_string();
    for inteiro in ["5521955554444", "5511966665555", "87654321098765"] {
        assert!(
            !texto.contains(inteiro),
            "identidade inteira vazou: {texto}"
        );
    }

    // Reset: Viewer nao pode; Admin zera, audita e devolve o resumo novo.
    let (status, _) = corpo(
        admin_whatsapp_access_rejections_reset(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Viewer),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(state.whatsapp_linked.rejeicoes().total, 4, "403 nao zera");

    let (status, doc) = corpo(
        admin_whatsapp_access_rejections_reset(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    assert_eq!(doc["cleared"], 4);
    assert_eq!(doc["rejections"]["total"], 0);
    assert!(doc["rejections"]["last_reset"].as_str().is_some());
    assert_eq!(state.whatsapp_linked.rejeicoes().total, 0);
    assert_eq!(
        state.whatsapp_linked.recusas_lid(),
        0,
        "o contador legado de @lid zera junto"
    );
    let audit = admin_state
        .store
        .lock()
        .await
        .list_audit_log(10, 0, Some("whatsapp_access"), None);
    assert!(
        audit.iter().any(|e| e.action == "rejections_reset"),
        "o reset e auditado: {audit:?}"
    );
}
