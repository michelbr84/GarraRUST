//! #2033: `admin_create_mcp` e `admin_delete_mcp` gravam o `mcp.json` e o
//! cofre do config dir sobre o qual o `AppState` foi montado, e nao do config
//! dir padrao.
//!
//! O `AppState::with_config_dir` le o `mcp.json` e o cofre do diretorio que
//! recebe (#2027), mas os dois handlers montavam a persistencia com
//! `McpPersistenceService::with_default_path()` e o cofre padrao. Um estado
//! sobre um diretorio X gravava o retrato do registry de X no `mcp.json`
//! padrao, mandava os segredos para o cofre padrao e, no delete, apagava
//! `mcp.<nome>.*` do cofre padrao — o de quem roda o processo.
//!
//! Aqui o `GARRAIA_CONFIG_DIR` aponta para um diretorio isca, com um
//! `mcp.json` e um cofre plantados. O estado e montado sobre outro diretorio.
//! A isca tem de sair byte a byte igual, e o diretorio do estado tem de
//! receber as gravacoes.
//!
//! Um teste so neste binario, porque ele escreve no ambiente do processo.

use std::collections::HashMap;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use garraia_agents::{AgentRuntime, McpManager};
use garraia_channels::ChannelRegistry;
use garraia_config::AppConfig;
use garraia_gateway::admin::mcp::{CreateMcpRequest, admin_create_mcp, admin_delete_mcp};
use garraia_gateway::admin::middleware::AuthenticatedAdmin;
use garraia_gateway::admin::rbac::Role;
use garraia_gateway::admin::shared::AdminState;
use garraia_gateway::admin::store::AdminStore;
use garraia_gateway::state::AppState;
use garraia_security::CredentialVault;
use tokio::sync::Mutex;

const PASSPHRASE: &str = "passphrase-de-teste-2033";
const NOME: &str = "novo";
const SEGREDO: &str = "segredo-do-servidor-novo";
/// A chave que o cofre da isca guarda com o mesmo nome do servidor criado e
/// apagado: um delete no cofre errado a apagaria.
const CHAVE_DA_ISCA: &str = "mcp.novo.API_TOKEN";

fn admin() -> AuthenticatedAdmin {
    AuthenticatedAdmin {
        user_id: "u-2033".to_string(),
        username: "operador".to_string(),
        role: Role::Admin,
        csrf_token: "csrf".to_string(),
        session_token: "sess".to_string(),
    }
}

fn ler(p: &std::path::Path) -> Vec<u8> {
    std::fs::read(p).unwrap_or_else(|e| panic!("ler {}: {e}", p.display()))
}

#[tokio::test]
async fn create_e_delete_gravam_no_config_dir_do_estado() {
    // A isca: o que seria o config dir real do operador.
    let isca = tempfile::tempdir().expect("tempdir da isca");
    let cofre_da_isca = garraia_config::vault_path_in(isca.path());
    std::fs::create_dir_all(cofre_da_isca.parent().expect("credentials/")).expect("credentials/");
    let mut vault = CredentialVault::create(&cofre_da_isca, PASSPHRASE).expect("cofre da isca");
    vault.set(CHAVE_DA_ISCA, "segredo-real-do-operador");
    vault.save().expect("gravar o cofre da isca");
    let mcp_da_isca = isca.path().join("mcp.json");
    std::fs::write(
        &mcp_da_isca,
        br#"{"mcpServers":{"do-operador":{"command":"true"}}}"#,
    )
    .expect("mcp.json da isca");
    let antes_mcp = ler(&mcp_da_isca);
    let antes_cofre = ler(&cofre_da_isca);

    // O diretorio do estado, com um `mcp.json` vazio para nao provisionar.
    let dir = tempfile::tempdir().expect("tempdir do estado");
    let mcp_do_estado = dir.path().join("mcp.json");
    std::fs::write(&mcp_do_estado, br#"{"mcpServers":{}}"#).expect("mcp.json do estado");
    let cofre_do_estado = garraia_config::vault_path_in(dir.path());

    // SAFETY: `set_var` so e sound sem outra thread lendo ou escrevendo o
    // environ ao mesmo tempo. Este e o unico teste do binario e o runtime do
    // `#[tokio::test]` e de uma thread so, entao ninguem mais toca o environ.
    unsafe {
        std::env::set_var("GARRAIA_VAULT_PASSPHRASE", PASSPHRASE);
        std::env::set_var("GARRAIA_CONFIG_DIR", isca.path());
    }

    let body = async {
        let manager = Arc::new(McpManager::new());
        let config = AppConfig {
            data_dir: Some(dir.path().join("data")),
            ..AppConfig::default()
        };
        let mut state = AppState::with_config_dir(
            config,
            Arc::new(AgentRuntime::new()),
            ChannelRegistry::new(),
            dir.path(),
        );
        assert_eq!(state.config_dir(), dir.path());
        state.mcp_manager_arc = Some(Arc::clone(&manager));
        let admin_state = AdminState {
            store: Arc::new(Mutex::new(
                AdminStore::in_memory().expect("in-memory admin store"),
            )),
            app_state: Arc::new(state),
            encryption_key: Arc::new(vec![0u8; 32]),
        };

        // ── create ──────────────────────────────────────────────────────
        let response = admin_create_mcp(
            State(admin_state.clone()),
            axum::Extension(admin()),
            Json(CreateMcpRequest {
                name: NOME.to_string(),
                command: Some("echo".to_string()),
                args: vec!["oi".to_string()],
                env: HashMap::from([("API_TOKEN".to_string(), SEGREDO.to_string())]),
                url: None,
                transport: None,
                timeout_secs: None,
                allowed_tools: Vec::new(),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), axum::http::StatusCode::CREATED);

        let raw: serde_json::Value =
            serde_json::from_slice(&ler(&mcp_do_estado)).expect("mcp.json do estado");
        assert_eq!(
            raw["mcpServers"][NOME]["env"]["API_TOKEN"],
            serde_json::json!(format!("vault:{CHAVE_DA_ISCA}")),
            "o create grava o servidor no mcp.json do estado, com o segredo no cofre: {raw}"
        );
        let cofre = CredentialVault::open(&cofre_do_estado, PASSPHRASE).expect("cofre do estado");
        assert_eq!(
            cofre.get(CHAVE_DA_ISCA),
            Some(SEGREDO),
            "o segredo vai para o cofre do config dir do estado"
        );
        assert_eq!(
            ler(&mcp_da_isca),
            antes_mcp,
            "o create nao toca o mcp.json padrao"
        );
        assert_eq!(
            ler(&cofre_da_isca),
            antes_cofre,
            "o create nao toca o cofre padrao"
        );

        // ── delete ──────────────────────────────────────────────────────
        let response = admin_delete_mcp(
            State(admin_state.clone()),
            axum::Extension(admin()),
            Path(NOME.to_string()),
        )
        .await
        .into_response();
        assert_eq!(response.status(), axum::http::StatusCode::OK);

        let raw: serde_json::Value =
            serde_json::from_slice(&ler(&mcp_do_estado)).expect("mcp.json do estado");
        assert!(
            raw["mcpServers"].get(NOME).is_none(),
            "o delete tira o servidor do mcp.json do estado: {raw}"
        );
        let cofre = CredentialVault::open(&cofre_do_estado, PASSPHRASE).expect("cofre do estado");
        assert_eq!(
            cofre.get(CHAVE_DA_ISCA),
            None,
            "o delete apaga mcp.novo.* do cofre do estado"
        );
        assert_eq!(
            ler(&mcp_da_isca),
            antes_mcp,
            "o delete nao toca o mcp.json padrao"
        );
        assert_eq!(
            ler(&cofre_da_isca),
            antes_cofre,
            "o delete nao apaga mcp.novo.* do cofre padrao"
        );

        manager.disconnect_all().await;
    };
    tokio::time::timeout(std::time::Duration::from_secs(60), body)
        .await
        .expect("o teste nao pode travar");
}
