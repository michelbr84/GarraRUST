//! #1409 / #1415: `GET /admin/api/sessions` diz, para cada sessao, o modo
//! escolhido, o projeto ativo e — para o WhatsApp pessoal — o principal e o
//! modo EFETIVO do turno (piso do canal ∧ escolha da sessao), sem nunca
//! expor caminho; e `GET /admin/api/capabilities?session_id=` da o painel de
//! capacidades daquela conversa.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use garraia_agents::{AgentRuntime, McpManager};
use garraia_channels::ChannelRegistry;
use garraia_config::{AppConfig, ChannelConfig};
use garraia_gateway::admin::capabilities::{CapabilitiesQuery, admin_capabilities};
use garraia_gateway::admin::handlers::admin_list_sessions;
use garraia_gateway::admin::middleware::AuthenticatedAdmin;
use garraia_gateway::admin::rbac::Role;
use garraia_gateway::admin::shared::AdminState;
use garraia_gateway::admin::store::AdminStore;
use garraia_gateway::state::AppState;
use tokio::sync::Mutex;

const DONO: &str = "5511977776666";
const USUARIO: &str = "5511999998888";

fn admin(role: Role) -> axum::Extension<AuthenticatedAdmin> {
    axum::Extension(AuthenticatedAdmin {
        user_id: "u-1409".to_string(),
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
async fn sessoes_trazem_principal_modo_efetivo_e_projeto_sem_caminho() {
    let mut config = AppConfig::default();
    let mut settings = std::collections::HashMap::new();
    settings.insert("allow".to_string(), serde_json::json!([USUARIO]));
    settings.insert("owners".to_string(), serde_json::json!([DONO]));
    settings.insert(
        "access".to_string(),
        serde_json::json!({ "users": { USUARIO: { "level": "read", "write": true } } }),
    );
    config.channels.insert(
        "whatsapp_linked".to_string(),
        ChannelConfig {
            channel_type: "whatsapp_linked".to_string(),
            enabled: Some(true),
            settings,
        },
    );
    let mut state = AppState::new(
        config,
        Arc::new(AgentRuntime::new()),
        ChannelRegistry::new(),
    );
    state.mcp_manager_arc = Some(Arc::new(McpManager::new()));
    let state = Arc::new(state);

    // Tres sessoes: dono 1:1, usuario read/write com projeto, e um grupo com o dono dentro.
    let sid_dono = format!("whatsapp-linked-{DONO}@s.whatsapp.net");
    let sid_usuario = format!("whatsapp-linked-{USUARIO}@s.whatsapp.net");
    let sid_grupo = "whatsapp-linked-120363000000000000@g.us".to_string();
    state
        .hydrate_session_history(&sid_dono, Some("whatsapp_linked"), Some(DONO))
        .await;
    state
        .hydrate_session_history(&sid_usuario, Some("whatsapp_linked"), Some(USUARIO))
        .await;
    state
        .hydrate_session_history(&sid_grupo, Some("whatsapp_linked"), Some(DONO))
        .await;
    if let Some(mut s) = state.sessions.get_mut(&sid_usuario) {
        s.project_id = Some("p-1".to_string());
        s.project_name = Some("Garra".to_string());
        s.working_dir = Some("/srv/projetos/garra-secreto".to_string());
    }
    // E uma sessao web sem nada.
    state
        .hydrate_session_history("web-1", Some("web"), None)
        .await;

    let admin_state = AdminState {
        store: Arc::new(Mutex::new(
            AdminStore::in_memory().expect("in-memory admin store"),
        )),
        app_state: Arc::clone(&state),
        encryption_key: Arc::new(vec![0u8; 32]),
    };

    let (status, doc) = corpo(
        admin_list_sessions(State(admin_state.clone()), admin(Role::Viewer))
            .await
            .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    let sessoes = doc["sessions"].as_array().expect("lista");
    let de = |id: &str| {
        sessoes
            .iter()
            .find(|s| s["id"] == id)
            .unwrap_or_else(|| panic!("{id} ausente: {sessoes:?}"))
            .clone()
    };
    let dono = de(&sid_dono);
    assert_eq!(dono["principal"], serde_json::json!("dono"), "{dono}");
    assert_eq!(
        dono["effective_mode"],
        serde_json::json!("search"),
        "standard: dono no piso search"
    );
    assert_eq!(dono["level"], serde_json::Value::Null, "dono nao tem teto");
    let usuario = de(&sid_usuario);
    assert_eq!(usuario["principal"], serde_json::json!("usuario"));
    assert_eq!(usuario["level"], serde_json::json!("read"));
    assert_eq!(usuario["write"], serde_json::json!(true));
    assert_eq!(usuario["effective_mode"], serde_json::json!("search"));
    assert_eq!(usuario["project_name"], serde_json::json!("Garra"));
    assert_eq!(usuario["has_workspace"], serde_json::json!(true));
    let grupo = de(&sid_grupo);
    assert_eq!(
        grupo["principal"],
        serde_json::json!("grupo"),
        "dono no grupo e o grupo"
    );
    let web = de("web-1");
    assert_eq!(
        web["principal"],
        serde_json::Value::Null,
        "principal so no WhatsApp pessoal"
    );
    assert_eq!(web["has_workspace"], serde_json::json!(false));
    assert_eq!(web["chosen_mode"], serde_json::Value::Null);
    let texto = doc.to_string();
    assert!(
        !texto.contains("garra-secreto"),
        "caminho do workspace nao sai: {texto}"
    );
    assert!(!texto.contains("\"working_dir\""), "{texto}");

    // O painel de capacidades da conversa do usuario read: file_write negada,
    // file_read visivel, nada de identidade alem do que a lista de sessoes ja tem.
    state
        .agents
        .register_tool(Box::new(garraia_agents::tools::FileReadTool::new(
            garraia_agents::tools::FileJail::sessions_only(),
        )));
    state
        .agents
        .register_tool(Box::new(garraia_agents::tools::FileWriteTool::new(
            garraia_agents::tools::FileJail::sessions_only(),
        )));
    let (status, doc) = corpo(
        admin_capabilities(
            State(admin_state.clone()),
            admin(Role::Viewer),
            Query(CapabilitiesQuery {
                session_id: Some(sid_usuario.clone()),
            }),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    let caps = doc["capabilities"].as_array().expect("lista");
    let cap = |n: &str| {
        caps.iter()
            .find(|c| c["name"] == n)
            .unwrap_or_else(|| panic!("{n} ausente: {caps:?}"))
            .clone()
    };
    assert_eq!(cap("file_read")["state"], serde_json::json!("visible"));
    assert_eq!(
        cap("file_write")["state"],
        serde_json::json!("denied"),
        "o painel da conversa aplica o piso do canal e o teto do principal: {caps:?}"
    );
    assert_eq!(
        doc["session"]["principal"],
        serde_json::json!("usuario"),
        "{doc}"
    );
    assert_eq!(
        doc["session"]["effective_mode"],
        serde_json::json!("search")
    );
}
