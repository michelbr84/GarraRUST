//! `GET|POST /admin/api/whatsapp/access` e `GET .../access/audit` (#1402,
//! #1412, #1413, #1414): a API admin e o segundo consumidor do caminho
//! unico de mutacao — le pelo mesmo `visao::documento` da CLI, muda pelo
//! mesmo `mutacao::aplicar`, audita pelo mesmo `auditoria`, com `origem:
//! admin_api` e o username do admin como ator.
//!
//! Um so `#[tokio::test]` por arquivo: `GARRAIA_CONFIG_DIR` e processo-wide.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
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
    AccessMutationRequest, AuditQuery, admin_whatsapp_access, admin_whatsapp_access_audit,
    admin_whatsapp_access_mutate,
};
use garraia_gateway::state::AppState;
use tokio::sync::Mutex;

const NUMERO: &str = "5511999998888";
const DONO: &str = "5511977776666";
const ESTRANHO: &str = "5521955554444";

fn admin(role: Role) -> axum::Extension<AuthenticatedAdmin> {
    axum::Extension(AuthenticatedAdmin {
        user_id: "u-1402".to_string(),
        username: "operadora".to_string(),
        role,
        csrf_token: "csrf".to_string(),
        session_token: "sess".to_string(),
    })
}

fn pedido(action: &str) -> AccessMutationRequest {
    AccessMutationRequest {
        action: action.to_string(),
        identity: None,
        jid: None,
        level: None,
        write: None,
        enabled: None,
        dry_run: false,
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

#[tokio::test]
async fn a_api_admin_le_muda_e_audita_pelo_mesmo_motor_da_cli() {
    let dir = tempfile::tempdir().expect("temp config dir");
    unsafe {
        std::env::set_var("GARRAIA_CONFIG_DIR", dir.path());
    }
    let loader = ConfigLoader::with_dir(dir.path());
    loader.ensure_dirs().expect("dirs");

    let mut config = AppConfig::default();
    let mut settings = std::collections::HashMap::new();
    settings.insert("allow".to_string(), serde_json::json!([NUMERO]));
    settings.insert("owners".to_string(), serde_json::json!([DONO]));
    settings.insert("default_mode".to_string(), serde_json::json!("code"));
    config.channels.insert(
        "whatsapp_linked".to_string(),
        ChannelConfig {
            channel_type: "whatsapp_linked".to_string(),
            enabled: Some(true),
            settings,
        },
    );
    // O `data_dir` do audit e o da config; aponta para dentro do tempdir.
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

    // ── GET: o documento da politica, sem identidade ──
    let (status, doc) = corpo(
        admin_whatsapp_access(State(admin_state.clone()), admin(Role::Viewer))
            .await
            .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    assert_eq!(doc["policy"]["admission"], serde_json::json!("restricted"));
    assert_eq!(
        doc["policy"]["execution_profile"],
        serde_json::json!("standard")
    );
    assert!(
        doc.get("hot_reload").is_some(),
        "diz se o gateway rele a quente: {doc}"
    );
    let texto = doc.to_string();
    for numero in [NUMERO, DONO] {
        assert!(
            !texto.contains(numero),
            "a API nunca revela identidade: {texto}"
        );
    }
    assert!(
        !texto.contains("\"identity\""),
        "sem `identity` na API: {texto}"
    );
    let principais = doc["policy"]["principals"].as_array().expect("lista");
    assert!(principais.iter().any(|p| p["principal"] == "dono"));
    assert!(principais.iter().any(|p| p["principal"] == "usuario"));

    // ── Viewer nao muda nada (403) ──
    let (status, _) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Viewer),
            Json(pedido("open")),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // ── POST dry_run: impacto pelo motor real, nada gravado nem auditado ──
    let mut req = pedido("write");
    req.identity = Some(format!("+{NUMERO}"));
    req.write = Some(false);
    req.dry_run = true;
    let (status, doc) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(req),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    assert_eq!(doc["dry_run"], serde_json::json!(true));
    assert_eq!(doc["changed"], serde_json::json!(true));
    assert_eq!(doc["written"], serde_json::json!(false));
    let impacto = doc["impact"].as_array().expect("impact");
    assert!(
        impacto
            .iter()
            .any(|d| d["loses"].as_array().is_some_and(|l| l
                .iter()
                .any(|c| c.as_str().unwrap_or("").contains("escrita")))),
        "{doc}"
    );
    assert!(!doc.to_string().contains(NUMERO));
    let relido = ConfigLoader::with_dir(dir.path())
        .load_sem_env()
        .expect("load");
    // `access.users` traz o legado (`allow`/`owners`) por construcao; o que
    // prova que nada foi gravado e a secao `access` nao existir no arquivo e
    // o usuario continuar como o legado o descreve (sem teto).
    assert!(
        !relido.channels["whatsapp_linked"]
            .settings
            .contains_key("access"),
        "dry_run nao grava: {:?}",
        relido.channels["whatsapp_linked"].settings.get("access")
    );
    let s = garraia_gateway::bootstrap::whatsapp_linked_settings(&relido);
    assert_eq!(
        garraia_gateway::bootstrap::whatsapp_linked_politica::principal_do_turno(
            &s, NUMERO, "x", false, false,
        ),
        garraia_gateway::bootstrap::whatsapp_linked_politica::Principal::Usuario(
            garraia_gateway::bootstrap::whatsapp_linked_politica::Alcance::COMPLETO
        )
    );
    let data_dir = dir.path().join("data");
    assert!(
        garraia_gateway::bootstrap::whatsapp_linked_politica::auditoria::ler(&data_dir, 10)
            .expect("ler")
            .is_empty(),
        "dry_run nao audita"
    );

    // ── POST real: grava, audita com origem admin_api e ator = username ──
    let mut req = pedido("level");
    req.identity = Some(NUMERO.to_string());
    req.level = Some("read".to_string());
    let (status, doc) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(req),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    assert_eq!(doc["written"], serde_json::json!(true));
    assert_eq!(doc["audit"]["written"], serde_json::json!(true), "{doc}");
    assert_eq!(doc["policy"]["admission"], serde_json::json!("restricted"));
    let relido = ConfigLoader::with_dir(dir.path())
        .load_sem_env()
        .expect("load");
    let s = garraia_gateway::bootstrap::whatsapp_linked_settings(&relido);
    let p = garraia_gateway::bootstrap::whatsapp_linked_politica::principal_do_turno(
        &s, NUMERO, "x", false, false,
    );
    assert_eq!(
        p,
        garraia_gateway::bootstrap::whatsapp_linked_politica::Principal::Usuario(
            garraia_gateway::bootstrap::whatsapp_linked_politica::Alcance::LEITURA
        )
    );
    let eventos =
        garraia_gateway::bootstrap::whatsapp_linked_politica::auditoria::ler(&data_dir, 10)
            .expect("ler");
    assert_eq!(eventos.len(), 1);
    assert_eq!(eventos[0].origem, "admin_api");
    assert_eq!(eventos[0].ator, "operadora");
    assert_eq!(eventos[0].acao, "level");
    assert_eq!(eventos[0].alvo.as_deref(), Some("…8888"));

    // ── POST invalido: 400 com a mensagem acionavel, nada muda ──
    let mut req = pedido("default");
    req.level = Some("full".to_string());
    let (status, doc) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(req),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{doc}");
    assert!(
        doc["error"].as_str().unwrap_or("").contains("read"),
        "{doc}"
    );
    let (status, doc) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(pedido("explodir")),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "acao desconhecida: {doc}");

    // ── open + block + reset pela API, e o audit lista mais recente primeiro ──
    let (status, _) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(pedido("open")),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let mut req = pedido("block");
    req.identity = Some(ESTRANHO.to_string());
    let (status, _) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(req),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, doc) = corpo(
        admin_whatsapp_access_audit(
            State(admin_state.clone()),
            admin(Role::Viewer),
            Query(AuditQuery { limit: Some(2) }),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    let eventos = doc["events"].as_array().expect("events");
    assert_eq!(eventos.len(), 2, "limit");
    assert_eq!(eventos[0]["action"], serde_json::json!("block"));
    assert_eq!(eventos[1]["action"], serde_json::json!("open"));
    assert!(!doc.to_string().contains(ESTRANHO));

    let (status, doc) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(pedido("reset")),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    assert_eq!(doc["policy"]["admission"], serde_json::json!("restricted"));
    // Segundo reset: nada muda, nada e auditado, 200.
    let antes = garraia_gateway::bootstrap::whatsapp_linked_politica::auditoria::ler(&data_dir, 50)
        .expect("ler")
        .len();
    let (status, doc) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(pedido("reset")),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    assert_eq!(doc["changed"], serde_json::json!(false));
    let depois =
        garraia_gateway::bootstrap::whatsapp_linked_politica::auditoria::ler(&data_dir, 50)
            .expect("ler")
            .len();
    assert_eq!(antes, depois);

    unsafe {
        std::env::remove_var("GARRAIA_CONFIG_DIR");
    }
}
