//! #1237: `vault:` no `env` de um servidor MCP declarado em `config.yml`
//! chega RESOLVIDO ao filho no boot — e referencia que não resolve impede
//! o servidor de subir.
//!
//! Até esta correção a resolução de `vault:` vivia só no registry
//! (`McpPersistenceService::load_registry`, GAR-291), nunca no caminho de
//! boot (`ConfigLoader::merged_mcp_config` → `build_mcp_tools`): o filho
//! recebia a string literal como valor da variável. O operador acreditava
//! que o segredo estava cifrado; o servidor falhava de forma enganosa.
//!
//! Os dois cenários num ÚNICO teste e em sequência, pela mesma razão de
//! `mcp_child_env.rs` (agents) e `mcp_boot_pending_allowlist.rs`: o teste
//! escreve no bloco de ambiente do processo (`GARRAIA_CONFIG_DIR`,
//! `GARRAIA_VAULT_PASSPHRASE`) e cada spawn o lê inteiro — testes paralelos
//! escrevendo enquanto `Command` lê é UB na glibc.
//!
//! O caminho é o REAL de boot: `build_mcp_tools`, com o fixture
//! `fake_mcp_server.py --expose-env` reportando o ambiente que o filho de
//! fato recebeu. Unix-only pelo fixture (`python3` no PATH), como o resto
//! da suíte de MCP.

use std::collections::HashMap;

use garraia_config::AppConfig;

const SERVER_OK: &str = "vault-ok";
const SERVER_QUEBRADO: &str = "vault-broken";
const ENV_KEY: &str = "GARRAIA_TEST_TOKEN";
const VALOR_RESOLVIDO: &str = "token-do-cofre-1237";

fn server_stdio(env: HashMap<String, String>) -> garraia_config::McpServerConfig {
    garraia_config::McpServerConfig {
        command: "python3".to_string(),
        args: vec![
            format!(
                "{}/../garraia-agents/tests/fixtures/fake_mcp_server.py",
                env!("CARGO_MANIFEST_DIR")
            ),
            "--expose-env".to_string(),
        ],
        env,
        transport: "stdio".to_string(),
        url: None,
        enabled: Some(true),
        timeout: Some(10),
        allowed_tools: vec![],
        memory_limit_mb: None,
        max_restarts: Some(1),
        restart_delay_secs: Some(1),
        inherit_env: false,
    }
}

#[tokio::test]
async fn boot_resolve_vault_no_env_e_falha_fechada_no_nao_resolvido() {
    // Config dir isolado: o vault de boot (`default_vault_path`) cai dentro
    // dele — nenhum vault real do desenvolvedor participa.
    let dir = tempfile::tempdir().expect("temp config dir");
    // SAFETY: binário de teste com um único teste; nenhum outro thread lê o
    // ambiente concorrentemente neste momento (o fixture só sobe depois).
    unsafe {
        std::env::set_var("GARRAIA_CONFIG_DIR", dir.path());
        std::env::set_var("GARRAIA_VAULT_PASSPHRASE", "passphrase-de-teste-1237");
    }
    let vault_path = garraia_config::default_vault_path();
    assert!(
        !vault_path.exists(),
        "vault pré-existente no config dir de teste"
    );
    assert!(
        garraia_security::try_vault_set(
            &vault_path,
            &format!("mcp.{SERVER_OK}.{ENV_KEY}"),
            VALOR_RESOLVIDO
        ),
        "setup: o vault precisa ser criado com a passphrase de teste"
    );

    let mut config = AppConfig::default();
    config.mcp.insert(
        SERVER_OK.to_string(),
        server_stdio(HashMap::from([(
            ENV_KEY.to_string(),
            format!("vault:mcp.{SERVER_OK}.{ENV_KEY}"),
        )])),
    );
    config.mcp.insert(
        SERVER_QUEBRADO.to_string(),
        server_stdio(HashMap::from([(
            ENV_KEY.to_string(),
            format!("vault:mcp.{SERVER_QUEBRADO}.{ENV_KEY}"),
        )])),
    );

    let (manager, tools, failures) = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        garraia_gateway::bootstrap::build_mcp_tools(&config),
    )
    .await
    .expect("build_mcp_tools within timeout");

    // 1. A referência RESOLVÍVEL: o servidor sobe e o filho recebe o VALOR.
    let tool = tools
        .iter()
        .find(|t| t.name().contains(SERVER_OK) && t.name().ends_with("env_report"))
        .unwrap_or_else(|| {
            panic!(
                "o servidor com ref resolvível sobe e registra env_report; \
                 tools={:?} failures={failures:?}",
                tools
                    .iter()
                    .map(|t| t.name().to_string())
                    .collect::<Vec<_>>()
            )
        });
    let saida = tool
        .execute(
            &garraia_agents::tools::ToolContext {
                session_id: "mcp-boot-vault-env".to_string(),
                user_id: None,
                is_heartbeat: false,
                approval: garraia_agents::tools::approval::ToolApproval::None,
                working_dir: None,
                project_id: None,
            },
            serde_json::json!({}),
        )
        .await
        .expect("env_report call should succeed");
    let relatorio: serde_json::Value =
        serde_json::from_str(&saida.content).expect("env_report must return JSON");
    assert_eq!(
        relatorio["values"][ENV_KEY].as_str(),
        Some(VALOR_RESOLVIDO),
        "o `vault:` do config.yml tem que chegar RESOLVIDO ao filho, não como literal"
    );
    let literal_no_filho = relatorio.to_string().contains("vault:");
    assert!(
        !literal_no_filho,
        "nenhum literal `vault:` pode chegar ao filho"
    );

    // 2. A referência NÃO resolvível: o servidor NÃO sobe (fail-closed) e o
    //    motivo aparece em `failures` — sem valor de segredo no texto.
    assert!(
        !tools.iter().any(|t| t.name().contains(SERVER_QUEBRADO)),
        "servidor com vault: que não resolve NÃO pode subir"
    );
    assert!(
        failures
            .iter()
            .any(|(nome, motivo)| nome == SERVER_QUEBRADO && motivo.contains("vault")),
        "a falha fechada precisa registrar o motivo: {failures:?}"
    );
    assert!(
        failures
            .iter()
            .all(|(_, motivo)| !motivo.contains(VALOR_RESOLVIDO)),
        "o texto da falha não pode conter o valor resolvido: {failures:?}"
    );

    manager.disconnect_all().await;
}
