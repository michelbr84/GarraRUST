//! Issue #1386 (P0): uma ferramenta MCP nunca vira slash command.
//!
//! O boot registrava um comando `/mcp_<tool>` por ferramenta MCP conectada,
//! e o fechamento desse comando chamava `McpManager::call_tool` direto. Esse
//! caminho nao passava por `ToolGate`, nem pelo modo da sessao, nem por
//! `ToolApproval`, nem por `HardwareGate` — o unico controle era o `Role::User`
//! do proprio comando, que na pratica todo mundo tem. Resultado: uma sessao
//! criada em modo `search` (read-only) executava ferramenta de mutacao por
//! `POST /api/sessions/{id}/messages`, e `GET /api/slash-commands` ainda
//! listava os nomes de graca.
//!
//! O caminho legitimo e outro e continua intacto: as ferramentas MCP entram no
//! `AgentRuntime` (`sync_mcp_tools`/`replace_mcp_tools`) e sao despachadas pelo
//! laco de tool-calling, atras do `ToolGate`. Este teste prova so a ausencia do
//! caminho paralelo.
//!
//! O teste monta o registro do mesmo jeito que `GatewayServer::run` monta
//! (`AppState::new` + `commands::register_commands`) e com um servidor MCP de
//! verdade conectado, com ferramentas de verdade. Se alguem reintroduzir a
//! auto-registracao — no boot, no reconnect do health monitor ou no restart do
//! admin — e aqui que estoura.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use garraia_agents::{AgentRuntime, McpManager};
use garraia_channels::ChannelRegistry;
use garraia_channels::commands::Role;
use garraia_config::AppConfig;
use garraia_gateway::state::AppState;

const SERVER: &str = "fixture-1386";

/// Aponta o binario de teste inteiro para um diretorio de config descartavel
/// antes que qualquer teste construa um `AppState`.
///
/// `AppState::new` le `ConfigLoader::default_config_dir()/allowlist.json` e
/// chama `provision_filesystem_if_missing()`, que escreve `mcp.json` no mesmo
/// diretorio. Sem isto, o teste mexeria no `~/.config/garraia` real da maquina.
/// Mesmo motivo (e mesma forma) do `admin_mcp_restart_allowlist.rs`.
static TEST_ENV: LazyLock<tempfile::TempDir> = LazyLock::new(|| {
    let dir = tempfile::tempdir().expect("temp config dir");
    // SAFETY: o `LazyLock` e o que serializa isto contra as outras threads de
    // teste deste binario — toda thread forca o lock antes de tocar em config.
    unsafe {
        std::env::set_var("GARRAIA_CONFIG_DIR", dir.path());
        std::env::set_var(
            garraia_gateway::mcp::McpPersistenceService::DISABLE_AUTOPROVISION_ENV,
            "1",
        );
    }
    dir
});

fn test_env() {
    LazyLock::force(&TEST_ENV);
}

/// O mesmo servidor stdio falso que os testes de ciclo de vida do MCP dirigem.
/// `write_file` e de proposito: era exatamente a ferramenta de mutacao que o
/// comando `/mcp_write_file` expunha sem gate nenhum.
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

async fn connected_manager() -> Arc<McpManager> {
    let manager = Arc::new(McpManager::new());
    manager
        .connect(
            SERVER,
            "python3",
            &fixture_args(),
            &HashMap::new(),
            10,
            Vec::new(),
            None,
            5,
            1,
            false,
        )
        .await
        .expect("fixture server should connect");
    manager
}

/// Monta o estado como o boot monta: `AppState::new`, o `Arc` do manager
/// instalado, e os comandos embutidos registrados.
async fn booted_state(manager: &Arc<McpManager>) -> AppState {
    let mut state = AppState::new(
        AppConfig::default(),
        Arc::new(AgentRuntime::new()),
        ChannelRegistry::new(),
    );
    state.mcp_manager_arc = Some(Arc::clone(manager));
    garraia_gateway::commands::register_commands(&mut state.command_registry.write().unwrap());
    state
}

#[tokio::test(flavor = "multi_thread")]
async fn boot_nao_registra_nenhum_comando_mcp() {
    test_env();
    let manager = connected_manager().await;

    // Sem esta guarda o teste passaria de gracinha, com um servidor que nao
    // conectou e zero ferramenta para registrar.
    let tools = manager.tool_info(SERVER).await;
    assert!(
        tools.iter().any(|t| t.name == "write_file"),
        "fixture precisa expor `write_file` para o teste valer algo; expos: {:?}",
        tools.iter().map(|t| &t.name).collect::<Vec<_>>()
    );

    let state = booted_state(&manager).await;
    {
        let registry = state.command_registry.read().unwrap();

        let vazados: Vec<String> = registry
            .list()
            .into_iter()
            .map(|(name, _)| name.to_string())
            .filter(|name| name.starts_with("mcp_"))
            .collect();
        assert!(
            vazados.is_empty(),
            "ferramenta MCP nao pode virar slash command (#1386); vazaram: {vazados:?}"
        );

        // O mesmo fato pelo lado do despacho: e assim que Telegram/Discord e
        // `POST /api/sessions/{id}/messages` chegam num comando.
        for tool in &tools {
            let texto = format!("/mcp_{}", tool.name);
            assert!(
                registry.resolve(&texto).is_none(),
                "`{texto}` nao pode resolver para comando nenhum (#1386)"
            );
        }
        assert!(registry.resolve("/mcp_write_file").is_none());
        assert!(registry.resolve("/mcp_read_file arg").is_none());
    }

    manager.disconnect(SERVER).await;
}

/// A superficie listada tambem nao pode vazar o nome: `GET /api/slash-commands`
/// e o menu do Telegram saem daqui, e ambos serviam de indice do bypass.
#[tokio::test(flavor = "multi_thread")]
async fn listagem_para_usuario_comum_nao_mostra_ferramenta_mcp() {
    test_env();
    let manager = connected_manager().await;
    let state = booted_state(&manager).await;
    {
        let registry = state.command_registry.read().unwrap();

        for (name, _) in registry.list_for_role(Role::User) {
            assert!(
                !name.starts_with("mcp_"),
                "`/{name}` exposto a Role::User sem passar pelo ToolGate (#1386)"
            );
        }
        for (name, _) in registry.telegram_commands() {
            assert!(
                !name.starts_with("mcp_"),
                "`/{name}` no menu do Telegram sem passar pelo ToolGate (#1386)"
            );
        }
    }

    manager.disconnect(SERVER).await;
}
