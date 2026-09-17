//! Issue #1273: uma escrita de admin não pode apagar o que os outros
//! servidores declaram em `mcp.json`.
//!
//! `admin_create_mcp` constrói a config a partir do request e chama
//! `save_from_registry`, que serializa o snapshot INTEIRO do registry de
//! volta para o arquivo. O tipo do registry só carregava um subconjunto do
//! schema `garraia_config` — sem `allowed_tools`, sem `inherit_env`, sem
//! `enabled`, sem alias para o tuning snake_case — então um POST ou DELETE
//! reescrevia o arquivo sem esses campos para TODOS os servidores. Uma
//! allowlist declarada pelo operador era honrada no boot até a primeira
//! escrita de admin, e silenciosamente ausente no boot seguinte.
//!
//! Este teste dirige o handler real (`admin_create_mcp`) contra um
//! `mcp.json` real no disco e assevera que os campos declarados do OUTRO
//! servidor continuam no arquivo — e que o loader de boot ainda os lê.
//!
//! Vive em binário de teste próprio porque precisa de um config dir com um
//! `mcp.json` específico (os binários irmãos precisam dos seus).

use std::collections::HashMap;
use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::response::IntoResponse;
use garraia_agents::{AgentRuntime, McpManager};
use garraia_channels::ChannelRegistry;
use garraia_config::{AppConfig, ConfigLoader};
use garraia_gateway::admin::mcp::{CreateMcpRequest, admin_create_mcp};
use garraia_gateway::admin::middleware::AuthenticatedAdmin;
use garraia_gateway::admin::rbac::Role;
use garraia_gateway::admin::shared::AdminState;
use garraia_gateway::admin::store::AdminStore;
use garraia_gateway::state::AppState;
use tokio::sync::Mutex;

const KEEPER: &str = "keeper";

#[tokio::test]
async fn create_via_admin_api_preserves_declared_fields_of_other_servers() {
    let dir = tempfile::tempdir().expect("temp config dir");
    // SAFETY: binário de teste dedicado com um único teste, então nenhuma
    // outra thread lê o ambiente em paralelo.
    unsafe {
        std::env::set_var("GARRAIA_CONFIG_DIR", dir.path());
        std::env::set_var(
            garraia_gateway::mcp::McpPersistenceService::DISABLE_AUTOPROVISION_ENV,
            "1",
        );
    }

    // O arquivo exatamente como o operador (ou o wizard) o escreve — a
    // allowlist GAR-190, a válvula `inherit_env` (#1075), a chave de boot
    // `enabled` e o tuning.
    let mcp_json = serde_json::json!({
        "mcpServers": {
            KEEPER: {
                "command": "python3",
                "args": ["-m", "keeper"],
                "transport": "stdio",
                "allowed_tools": ["read_file", "write_file"],
                "inherit_env": true,
                "enabled": false,
                "memory_limit_mb": 512
            }
        }
    });
    std::fs::write(
        dir.path().join("mcp.json"),
        serde_json::to_vec_pretty(&mcp_json).expect("serialize mcp.json"),
    )
    .expect("write mcp.json");

    let body = async {
        // Um manager que não sabe nada: o servidor em criação nunca foi
        // conectado — nasce Stopped, por contrato.
        let manager = Arc::new(McpManager::new());

        // `AppState::new` carrega o registry do arquivo via
        // `McpPersistenceService::load_registry` — o caminho que a admin
        // API lê e escreve.
        let mut state = AppState::new(
            AppConfig::default(),
            Arc::new(AgentRuntime::new()),
            ChannelRegistry::new(),
        );
        assert!(
            state.config.mcp.is_empty(),
            "precondition: a seção 'mcp:' do config.yml deve estar vazia"
        );
        state.mcp_manager_arc = Some(Arc::clone(&manager));

        let admin_state = AdminState {
            store: Arc::new(Mutex::new(
                AdminStore::in_memory().expect("in-memory admin store"),
            )),
            app_state: Arc::new(state),
            encryption_key: Arc::new(vec![0u8; 32]),
        };

        let response = admin_create_mcp(
            State(admin_state),
            axum::Extension(AuthenticatedAdmin {
                user_id: "u-1273".to_string(),
                username: "operador".to_string(),
                role: Role::Admin,
                csrf_token: "csrf".to_string(),
                session_token: "sess".to_string(),
            }),
            Json(CreateMcpRequest {
                name: "novo".to_string(),
                command: Some("echo".to_string()),
                args: vec!["oi".to_string()],
                env: HashMap::new(),
                url: None,
                transport: None,
                timeout_secs: None,
                allowed_tools: Vec::new(),
            }),
        )
        .await
        .into_response();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("response body");
        assert_eq!(
            status,
            axum::http::StatusCode::CREATED,
            "create body: {}",
            String::from_utf8_lossy(&bytes)
        );

        // Sonda o arquivo — a sonda exata do corpo da issue.
        let raw: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("mcp.json")).expect("read back"),
        )
        .expect("parse back");
        let keeper = &raw["mcpServers"][KEEPER];
        assert_eq!(
            keeper["allowed_tools"],
            serde_json::json!(["read_file", "write_file"]),
            "uma escrita de admin não pode apagar a allowlist declarada para os OUTROS servidores: {raw}"
        );
        assert_eq!(keeper["inherit_env"], serde_json::json!(true));
        assert_eq!(keeper["enabled"], serde_json::json!(false));
        assert_eq!(
            keeper["memoryLimitMb"],
            serde_json::json!(512),
            "o tuning declarado também sobrevive (grafia canônica do writer)"
        );
        assert!(
            raw["mcpServers"]["novo"].is_object(),
            "o servidor criado deve ser persistido: {raw}"
        );

        // E o loader de boot ainda lê a allowlist preservada — o arquivo
        // continua sendo a fonte que `build_mcp_tools` honra no próximo
        // boot.
        let loader = ConfigLoader::with_dir(dir.path());
        let merged = loader.load_mcp_json();
        let keeper_cfg = merged.get(KEEPER).expect("keeper continua parseável");
        assert_eq!(
            keeper_cfg.allowed_tools,
            vec!["read_file".to_string(), "write_file".to_string()],
            "o loader de boot deve continuar vendo a allowlist depois da escrita de admin"
        );
        assert!(keeper_cfg.inherit_env);
        assert_eq!(keeper_cfg.enabled, Some(false));

        manager.disconnect_all().await;
    };
    tokio::time::timeout(std::time::Duration::from_secs(60), body)
        .await
        .expect("test must not hang");
}
