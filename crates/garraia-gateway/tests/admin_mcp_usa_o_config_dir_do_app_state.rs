//! #2033: os handlers admin do MCP gravam e leem no config dir do `AppState`.
//!
//! O create e o delete persistiam pelo `McpPersistenceService::with_default_path()`
//! e pelo cofre padrao (`<config_dir padrao>/credentials/vault.json`), e o
//! restart lia a allowlist declarada pelo `ConfigLoader::new()`, tambem no
//! diretorio padrao. Um estado montado sobre outro diretorio (o
//! `AppState::with_config_dir`, publico desde o #2031) carregava o registry de
//! la, mas:
//!
//! - o create e o delete gravavam o registry inteiro no `mcp.json` padrao;
//! - os segredos do `env` iam para o cofre padrao;
//! - o delete apagava `mcp.<nome>.*` do cofre padrao;
//! - o restart aplicava a allowlist do `mcp.json` padrao.
//!
//! Aqui o estado mora num diretorio e o `GARRAIA_CONFIG_DIR` aponta para uma
//! isca, com outro `mcp.json` e outro cofre. Tudo tem de acontecer no
//! diretorio do estado, e a isca tem de terminar byte a byte como comecou.
//!
//! Um teste so neste binario, porque ele escreve no ambiente do processo.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path as Caminho, State};
use axum::response::IntoResponse;
use garraia_agents::{AgentRuntime, McpManager};
use garraia_channels::ChannelRegistry;
use garraia_config::AppConfig;
use garraia_gateway::admin::mcp::{
    CreateMcpRequest, admin_create_mcp, admin_delete_mcp, admin_restart_mcp,
};
use garraia_gateway::admin::middleware::AuthenticatedAdmin;
use garraia_gateway::admin::rbac::Role;
use garraia_gateway::admin::shared::AdminState;
use garraia_gateway::admin::store::AdminStore;
use garraia_gateway::state::AppState;
use garraia_security::CredentialVault;
use tokio::sync::Mutex;

const PASSPHRASE: &str = "passphrase-de-teste-2033";

/// Servidor que o teste apaga; o `TOKEN` dele mora no cofre.
const APAGADO: &str = "apagado";
/// Servidor que o teste cria; o `API_TOKEN` dele tem de ir para o cofre.
const CRIADO: &str = "criado";
/// Servidor que o teste reinicia; so o `mcp.json` do estado o restringe.
const RESTRITO: &str = "restrito";

fn fixture_args() -> Vec<String> {
    vec![
        format!(
            "{}/../garraia-agents/tests/fixtures/fake_mcp_server.py",
            env!("CARGO_MANIFEST_DIR")
        ),
        "--tools".to_string(),
        "read_file,write_file".to_string(),
    ]
}

fn cofre_em(dir: &Path) -> PathBuf {
    dir.join("credentials").join("vault.json")
}

/// Um config dir com um cofre que guarda `segredo` em `mcp.apagado.TOKEN` e um
/// `mcp.json` com os servidores [`APAGADO`] e [`RESTRITO`]. A allowlist do
/// `RESTRITO` e a que o restart tem de aplicar quando o diretorio e o do
/// estado; `None` deixa o servidor sem restricao.
fn config_dir(segredo: &str, allowlist_do_restrito: Option<&[&str]>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let cofre = cofre_em(dir.path());
    std::fs::create_dir_all(cofre.parent().expect("credentials/")).expect("criar credentials/");
    let mut vault = CredentialVault::create(&cofre, PASSPHRASE).expect("criar o cofre");
    vault.set(format!("mcp.{APAGADO}.TOKEN"), segredo);
    vault.save().expect("gravar o cofre");

    let mut restrito = serde_json::json!({
        "command": "python3",
        "args": fixture_args(),
        "transport": "stdio",
        "timeout": 10,
    });
    if let Some(ferramentas) = allowlist_do_restrito {
        restrito["allowed_tools"] = serde_json::json!(ferramentas);
    }
    let mcp = serde_json::json!({
        "mcpServers": {
            APAGADO: {
                "command": "true",
                "env": { "TOKEN": format!("vault:mcp.{APAGADO}.TOKEN") }
            },
            RESTRITO: restrito,
        }
    });
    std::fs::write(
        dir.path().join("mcp.json"),
        serde_json::to_vec_pretty(&mcp).expect("serializar o mcp.json"),
    )
    .expect("gravar o mcp.json");
    dir
}

/// O `mcp.json` e o cofre de um diretorio, como bytes.
fn retrato(dir: &Path) -> (Vec<u8>, Vec<u8>) {
    (
        std::fs::read(dir.join("mcp.json")).expect("ler o mcp.json"),
        std::fs::read(cofre_em(dir)).expect("ler o cofre"),
    )
}

fn mcp_json(dir: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(dir.join("mcp.json")).expect("ler o mcp.json"))
        .expect("parsear o mcp.json")
}

fn segredo(dir: &Path, chave: &str) -> Option<String> {
    CredentialVault::open(&cofre_em(dir), PASSPHRASE)
        .expect("abrir o cofre")
        .get(chave)
        .map(str::to_string)
}

fn admin() -> axum::Extension<AuthenticatedAdmin> {
    axum::Extension(AuthenticatedAdmin {
        user_id: "u-2033".to_string(),
        username: "operador".to_string(),
        role: Role::Admin,
        csrf_token: "csrf".to_string(),
        session_token: "sess".to_string(),
    })
}

async fn corpo(resposta: impl IntoResponse) -> (axum::http::StatusCode, serde_json::Value) {
    let resposta = resposta.into_response();
    let status = resposta.status();
    let bytes = axum::body::to_bytes(resposta.into_body(), 64 * 1024)
        .await
        .expect("corpo da resposta");
    (
        status,
        serde_json::from_slice(&bytes).expect("corpo em JSON"),
    )
}

#[tokio::test]
async fn create_delete_e_restart_usam_o_config_dir_do_app_state() {
    let estado = config_dir("segredo-do-estado", Some(&["read_file"]));
    let isca = config_dir("segredo-da-isca", None);

    // SAFETY: `set_var` so e sound sem outra thread lendo ou escrevendo o
    // environ ao mesmo tempo. Este e o unico teste do binario e o runtime do
    // `#[tokio::test]` e de uma thread so, entao ninguem mais toca o environ.
    unsafe {
        std::env::set_var("GARRAIA_VAULT_PASSPHRASE", PASSPHRASE);
        std::env::set_var("GARRAIA_CONFIG_DIR", isca.path());
    }
    let isca_antes = retrato(isca.path());

    let corpo_do_teste = async {
        let manager = Arc::new(McpManager::new());
        let config = AppConfig {
            data_dir: Some(estado.path().join("data")),
            ..AppConfig::default()
        };
        let mut state = AppState::with_config_dir(
            config,
            Arc::new(AgentRuntime::new()),
            ChannelRegistry::new(),
            estado.path(),
        );
        assert!(
            state.config.mcp.is_empty(),
            "precondicao: a secao 'mcp:' do config.yml tem de estar vazia, para \
             o restart so achar a allowlist no mcp.json"
        );
        state.mcp_manager_arc = Some(Arc::clone(&manager));
        let admin_state = AdminState {
            store: Arc::new(Mutex::new(
                AdminStore::in_memory().expect("admin store em memoria"),
            )),
            app_state: Arc::new(state),
            encryption_key: Arc::new(vec![0u8; 32]),
        };

        // 1. create: o servidor novo e o segredo dele vao para o diretorio do
        // estado.
        let (status, json) = corpo(
            admin_create_mcp(
                State(admin_state.clone()),
                admin(),
                Json(CreateMcpRequest {
                    name: CRIADO.to_string(),
                    command: Some("echo".to_string()),
                    args: vec!["oi".to_string()],
                    env: HashMap::from([("API_TOKEN".to_string(), "segredo-novo".to_string())]),
                    url: None,
                    transport: None,
                    timeout_secs: None,
                    allowed_tools: Vec::new(),
                }),
            )
            .await,
        )
        .await;
        assert_eq!(status, axum::http::StatusCode::CREATED, "create: {json}");
        let gravado = mcp_json(estado.path());
        assert_eq!(
            gravado["mcpServers"][CRIADO]["env"]["API_TOKEN"],
            serde_json::json!(format!("vault:mcp.{CRIADO}.API_TOKEN")),
            "o create tem de persistir no mcp.json do config dir do AppState, com o \
             segredo trocado pela referencia ao cofre: {gravado}"
        );
        assert_eq!(
            segredo(estado.path(), &format!("mcp.{CRIADO}.API_TOKEN")).as_deref(),
            Some("segredo-novo"),
            "o segredo do servidor criado tem de ir para o cofre do config dir do AppState"
        );

        // 2. delete: o servidor sai do mcp.json do estado, e o segredo dele,
        // do cofre do estado.
        let (status, json) = corpo(
            admin_delete_mcp(
                State(admin_state.clone()),
                admin(),
                Caminho(APAGADO.to_string()),
            )
            .await,
        )
        .await;
        assert_eq!(status, axum::http::StatusCode::OK, "delete: {json}");
        let gravado = mcp_json(estado.path());
        assert!(
            gravado["mcpServers"].get(APAGADO).is_none(),
            "o delete tem de tirar o servidor do mcp.json do config dir do AppState: {gravado}"
        );
        assert_eq!(
            segredo(estado.path(), &format!("mcp.{APAGADO}.TOKEN")),
            None,
            "o delete tem de apagar as credenciais do servidor do cofre do config dir \
             do AppState"
        );

        // 3. restart: a allowlist declarada vem do mcp.json do estado
        // (`read_file`), nao do da isca, que nao restringe o servidor.
        let (status, json) = corpo(
            admin_restart_mcp(
                State(admin_state.clone()),
                admin(),
                Caminho(RESTRITO.to_string()),
            )
            .await,
        )
        .await;
        assert_eq!(status, axum::http::StatusCode::OK, "restart: {json}");
        assert_eq!(
            json["tool_count"], 1,
            "o restart tem de aplicar a allowlist do mcp.json do config dir do AppState: {json}"
        );
        let erro = manager
            .call_tool(RESTRITO, "write_file", HashMap::new())
            .await
            .expect_err("write_file tem de ficar bloqueado pela allowlist do estado");
        assert!(
            erro.contains("blocked by the allowed_tools allowlist"),
            "esperava o erro da allowlist GAR-190, veio: {erro}"
        );
        manager.disconnect_all().await;
    };
    // Folga larga: cada abertura do cofre paga o KDF, que em build de debug
    // leva perto de um segundo, e o teste abre o cofre umas dez vezes.
    tokio::time::timeout(std::time::Duration::from_secs(120), corpo_do_teste)
        .await
        .expect("o teste nao pode travar");

    assert!(
        retrato(isca.path()) == isca_antes,
        "o mcp.json e o cofre do config dir padrao (GARRAIA_CONFIG_DIR) nao podem mudar: \
         os handlers admin do MCP tem de trabalhar no config dir do AppState"
    );
    assert_eq!(
        segredo(isca.path(), &format!("mcp.{APAGADO}.TOKEN")).as_deref(),
        Some("segredo-da-isca"),
        "o delete nao pode apagar as credenciais do cofre do config dir padrao"
    );
}
