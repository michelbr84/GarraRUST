//! Issue #1386 (P0): uma ferramenta MCP nunca vira slash command.
//!
//! O boot registrava um comando `/mcp_<tool>` por ferramenta MCP conectada, e
//! o fechamento desse comando chamava `McpManager::call_tool` direto. Esse
//! caminho nao passava por `ToolGate`, nem pelo modo da sessao, nem por
//! `ToolApproval`, nem por `HardwareGate` — o unico controle era o `Role::User`
//! do proprio comando, que na pratica todo mundo tem. Resultado: uma sessao
//! criada em modo `search` (read-only) executava ferramenta de mutacao por
//! `POST /api/sessions/{id}/messages`, e `GET /api/slash-commands` listava os
//! nomes de graca.
//!
//! O caminho legitimo e outro e continua intacto: as ferramentas MCP entram no
//! `AgentRuntime` (`sync_mcp_tools`/`replace_mcp_tools`) e sao despachadas pelo
//! laco de tool-calling, atras do `ToolGate`. O ponte que as executa mora em
//! `garraia-agents`, nao aqui.
//!
//! # Por que um teste de fonte
//!
//! A registracao vivia dentro de `GatewayServer::run`, que abre socket e sobe
//! canal — nao ha seam barato para exercitar o boot de verdade, e um teste que
//! so monta `AppState` a mao nao executa o sitio do defeito (ele passaria
//! identico ANTES da correcao, o que o tornaria decoracao). Entao o guarda
//! principal e estatico, no mesmo espirito do
//! `approval_scope_coverage.rs` e do teste que varre o `detect.rs`: nenhum
//! fonte de producao do gateway pode chamar `call_tool`. Esse e o primitivo de
//! execucao do bypass — qualquer reintroducao dele, no boot, no reconnect do
//! health monitor ou no restart do admin, tem de passar por ali.

mod varredura_de_escopo;

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, LazyLock};

use garraia_agents::{AgentRuntime, McpManager};
use garraia_channels::ChannelRegistry;
use garraia_channels::commands::Role;
use garraia_config::AppConfig;
use garraia_gateway::state::AppState;
use varredura_de_escopo::varrer;

const SERVER: &str = "fixture-1386";

/// O guarda que reprova se alguem devolver o caminho paralelo.
///
/// `Fonte::codigo` vem com comentario e conteudo de literal apagados, entao a
/// mencao em doc comment de `admin/mcp.rs` ("reachable through
/// `McpManager::call_tool`") nao conta — o teste fala do que o gateway *faz*.
///
/// Se algum dia o gateway precisar mesmo executar ferramenta MCP fora do
/// `AgentRuntime`, o conserto NAO e afrouxar este teste: e fazer a chamada
/// atras do `ToolGate`, com o modo da sessao e o `ToolApproval` no caminho,
/// e entao decidir aqui, por escrito, por que aquele sitio e seguro.
#[test]
fn nenhum_fonte_de_producao_do_gateway_executa_ferramenta_mcp() {
    let raiz = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let arquivos = varrer(&raiz);
    assert!(
        arquivos.iter().any(|(a, _)| a == "server.rs"),
        "a varredura nao achou src/server.rs; ela quebrou?"
    );

    let culpados: Vec<String> = arquivos
        .iter()
        // Sem o ponto de proposito: pega tanto `mgr.call_tool(...)` quanto a
        // sintaxe UFCS `McpManager::call_tool(&mgr, ...)`, que a versao
        // anterior deste guarda deixava passar (revisao de seguranca do
        // #1386). `Fonte::codigo` ja vem sem comentario, entao a mencao em
        // doc comment de `src/admin/mcp.rs` nao conta.
        .filter(|(_, fonte)| fonte.codigo.contains("call_tool("))
        .map(|(arq, _)| arq.clone())
        .collect();

    assert!(
        culpados.is_empty(),
        "ferramenta MCP so pode ser executada pelo laco de tool-calling, atras do \
         `ToolGate` (#1386); chamam `call_tool` direto: {culpados:?}"
    );
}

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

/// O registro de comandos como o gateway o entrega: `AppState::new` mais os
/// embutidos, que e tudo que `GatewayServer::run` registra hoje.
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

/// Complemento do guarda estatico, pelo lado do resultado: com um servidor MCP
/// de verdade conectado e com ferramentas de verdade, o registro entregue nao
/// tem comando `mcp_*` e `/mcp_<tool>` nao resolve para nada — cai em comando
/// desconhecido, como qualquer texto solto.
///
/// Sozinho este teste NAO pegaria a regressao (antes da correcao quem
/// registrava era `register_mcp_tools()`, chamado do `run()`, que ele nao
/// executa). Quem pega e o teste de fonte la em cima.
#[tokio::test(flavor = "multi_thread")]
async fn registro_entregue_nao_tem_comando_mcp() {
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

        // E a superficie listada nao vaza nem o nome: `GET /api/slash-commands`
        // e o menu do Telegram saem daqui, e serviam de indice do bypass.
        for (name, _) in registry.list_for_role(Role::User) {
            assert!(!name.starts_with("mcp_"), "`/{name}` exposto a Role::User");
        }
        for (name, _) in registry.telegram_commands() {
            assert!(!name.starts_with("mcp_"), "`/{name}` no menu do Telegram");
        }
    }

    manager.disconnect(SERVER).await;
}
