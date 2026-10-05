//! `GET /admin/api/permissions/overview` (#1433): a visao global de agentes e
//! permissoes. Prova que a rota NAO tem logica propria — o efetivo de cada
//! principal e identico ao que o motor (`impacto`) calcula —, que lista os
//! canais com honestidade (o que nao tem motor de politica diz isso), que
//! marca o que e sensivel e que nunca revela identidade.
//!
//! Um so `#[tokio::test]` por arquivo: `GARRAIA_CONFIG_DIR` e processo-wide.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use garraia_agents::{AgentRuntime, McpManager};
use garraia_channels::ChannelRegistry;
use garraia_config::{AppConfig, ChannelConfig, ConfigLoader, ExecutionProfile};
use garraia_gateway::admin::middleware::AuthenticatedAdmin;
use garraia_gateway::admin::permissions_overview::admin_permissions_overview;
use garraia_gateway::admin::rbac::Role;
use garraia_gateway::admin::shared::AdminState;
use garraia_gateway::admin::store::AdminStore;
use garraia_gateway::state::AppState;
use tokio::sync::Mutex;

const NUMERO: &str = "5511999998888";
const DONO: &str = "5511977776666";

fn admin(role: Role) -> axum::Extension<AuthenticatedAdmin> {
    axum::Extension(AuthenticatedAdmin {
        user_id: "u-1433".to_string(),
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
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

#[tokio::test]
async fn a_visao_global_usa_o_mesmo_motor_e_lista_canais_com_honestidade() {
    let dir = tempfile::tempdir().expect("temp config dir");
    unsafe {
        std::env::set_var("GARRAIA_CONFIG_DIR", dir.path());
    }
    let loader = ConfigLoader::with_dir(dir.path());
    loader.ensure_dirs().expect("dirs");

    let mut config = AppConfig::default();
    let mut settings = std::collections::HashMap::new();
    settings.insert("owners".to_string(), serde_json::json!([DONO]));
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

    // Viewer le (Channels/Read, a mesma guarda da pagina WhatsApp Access).
    let (status, doc) = corpo(
        admin_permissions_overview(State(admin_state.clone()), admin(Role::Viewer))
            .await
            .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");

    // ── global: o perfil de execucao do processo ──
    assert_eq!(
        doc["global"]["execution_profile"],
        serde_json::json!("standard")
    );

    // ── legenda: toda classe, com a marca de sensivel ──
    let classes = doc["capability_classes"].as_array().expect("classes");
    let sensivel = |nome: &str| -> bool {
        classes
            .iter()
            .find(|c| c["class"] == serde_json::json!(nome))
            .and_then(|c| c["sensitive"].as_bool())
            .unwrap_or_else(|| panic!("classe {nome} ausente: {doc}"))
    };
    assert!(!sensivel("filesystem.read"), "leitura nao e sensivel");
    assert!(sensivel("filesystem.write"), "escrita e sensivel");
    assert!(sensivel("process.execute"), "shell e sensivel");
    assert!(!sensivel("network.read"), "web e leitura");

    // ── canais: whatsapp_linked tem motor; os demais sao honestos ──
    let canais = doc["channels"].as_array().expect("channels");
    let wa = canais
        .iter()
        .find(|c| c["id"] == serde_json::json!("whatsapp_linked"))
        .expect("whatsapp_linked listado");
    assert_eq!(wa["policy_engine"], serde_json::json!(true));
    assert_eq!(
        wa["policy_source"],
        serde_json::json!("channels.whatsapp_linked.access")
    );
    assert_eq!(wa["edit_page"], serde_json::json!("whatsapp_access"));
    let telegram = canais
        .iter()
        .find(|c| c["id"] == serde_json::json!("telegram"))
        .expect("telegram listado");
    assert_eq!(
        telegram["policy_engine"],
        serde_json::json!(false),
        "canal sem motor nao finge ter politica por principal"
    );
    assert!(
        telegram["note"]
            .as_str()
            .unwrap_or("")
            .contains("not supported yet"),
        "diz a verdade: {telegram}"
    );
    assert!(
        telegram["principals"]
            .as_array()
            .is_some_and(|p| p.is_empty()),
        "sem principais inventados"
    );

    // ── efetivo == motor: a rota NAO reimplementa o calculo ──
    let principais = wa["principals"].as_array().expect("principals");
    let usuario = principais
        .iter()
        .find(|p| p["principal"] == serde_json::json!("usuario"))
        .expect("usuario");
    let settings = garraia_gateway::bootstrap::whatsapp_linked_settings(
        &ConfigLoader::with_dir(dir.path())
            .load_sem_env()
            .expect("load"),
    );
    let esperado: Vec<String> =
        garraia_gateway::bootstrap::whatsapp_linked_politica::impacto::efetivo(
            &settings,
            ExecutionProfile::Standard,
            garraia_gateway::bootstrap::whatsapp_linked_politica::Principal::Usuario(
                garraia_gateway::bootstrap::whatsapp_linked_politica::Alcance::LEITURA,
            ),
            false,
        )
        .capacidades
        .classes()
        .into_iter()
        .map(|c| c.as_str().to_string())
        .collect();
    let vistas: Vec<String> = usuario["classes"]
        .as_array()
        .expect("classes")
        .iter()
        .map(|v| v.as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(
        vistas, esperado,
        "o efetivo da visao e o do motor: {usuario}"
    );
    // Um `read` so lista classes de leitura; nenhuma sensivel.
    assert!(
        usuario["sensitive_classes"]
            .as_array()
            .is_some_and(|s| s.is_empty()),
        "read nao concede classe sensivel: {usuario}"
    );
    assert!(
        principais
            .iter()
            .any(|p| p["principal"] == serde_json::json!("dono")),
        "o dono aparece: {wa}"
    );

    // ── nunca revela identidade ──
    let texto = doc.to_string();
    for numero in [NUMERO, DONO] {
        assert!(!texto.contains(numero), "identidade crua na visao: {texto}");
    }
    assert!(!texto.contains("\"identity\""), "sem `identity`: {texto}");

    unsafe {
        std::env::remove_var("GARRAIA_CONFIG_DIR");
    }
}
