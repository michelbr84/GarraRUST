//! #1075 (continuação): o ambiente de um filho MCP.
//!
//! Até esta correção o processo de um servidor MCP stdio herdava o ambiente
//! INTEIRO do gateway — `GARRAIA_JWT_SECRET`, chaves de provider, passphrase
//! do cofre, tudo que o `dotenvy` tivesse carregado. Como servidores MCP são
//! rotineiramente binários de terceiro baixados na hora por `npx`, isso
//! entregava o cofre completo a código que o operador nunca auditou.
//!
//! Estes testes sobem um filho de verdade pelo MESMO caminho de spawn do
//! gateway (`McpManager::connect`) e leem o ambiente que ele de fato recebeu.
//! Unix-only: a allowlist tem complemento por plataforma e o fixture depende
//! de `python3` no PATH, como o resto da suíte de MCP.
#![cfg(all(feature = "mcp", unix))]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use garraia_agents::McpManager;
use garraia_agents::tools::ToolContext;

/// Nome único por design: os testes rodam em paralelo dentro do mesmo
/// binário, então a variável é só PLANTADA (nunca removida nem alterada) e
/// não colide com nada que outro teste leia.
const PLANTADA: &str = "GARRAIA_TEST_PLANTED_SECRET";
const EXPLICITA: &str = "GARRAIA_TEST_EXPLICIT";

fn plantar_segredo() {
    static UMA_VEZ: std::sync::Once = std::sync::Once::new();
    UMA_VEZ.call_once(|| {
        // SAFETY: edition 2024 exige `unsafe` para `set_var`. A escrita
        // acontece uma única vez, antes de qualquer spawn deste binário de
        // teste, e a variável não é lida por nenhuma outra thread.
        unsafe { std::env::set_var(PLANTADA, "hunter2") };
    });
}

fn fixture_args() -> Vec<String> {
    vec![
        format!(
            "{}/tests/fixtures/fake_mcp_server.py",
            env!("CARGO_MANIFEST_DIR")
        ),
        "--expose-env".to_string(),
    ]
}

fn ctx() -> ToolContext {
    ToolContext {
        session_id: "mcp-child-env-test".to_string(),
        user_id: None,
        is_heartbeat: false,
        approval: garraia_agents::tools::approval::ToolApproval::None,
        working_dir: None,
        project_id: None,
    }
}

/// Sobe o fixture e devolve o ambiente que o filho realmente recebeu.
async fn ambiente_do_filho(
    nome: &str,
    explicito: HashMap<String, String>,
    inherit_env: bool,
) -> serde_json::Value {
    plantar_segredo();

    let manager = Arc::new(McpManager::new());
    manager
        .connect(
            nome,
            "python3",
            &fixture_args(),
            &explicito,
            10,
            vec![],
            None,
            5,
            1,
            inherit_env,
        )
        .await
        .expect("fixture server should connect");

    let tools = manager.take_tools(nome, Duration::from_secs(10)).await;
    let tool = tools
        .iter()
        .find(|t| t.name().ends_with("env_report"))
        .expect("fixture must expose env_report");

    let saida = tool
        .execute(&ctx(), serde_json::json!({}))
        .await
        .expect("env_report call should succeed");
    manager.disconnect_all().await;

    serde_json::from_str(&saida.content).expect("env_report must return JSON")
}

fn tem(relatorio: &serde_json::Value, chave: &str) -> bool {
    relatorio["names"]
        .as_array()
        .expect("names must be an array")
        .iter()
        .any(|n| n.as_str() == Some(chave))
}

/// O defeito. Falha antes da correção: a variável plantada no gateway
/// chegava inteira ao filho, junto de todo o resto do cofre.
#[tokio::test]
async fn filho_mcp_nao_recebe_a_variavel_plantada_no_gateway() {
    let body = async {
        let relatorio = ambiente_do_filho("env-default", HashMap::new(), false).await;

        assert!(
            !tem(&relatorio, PLANTADA),
            "o filho MCP recebeu '{PLANTADA}', plantada no ambiente do gateway"
        );
        // A allowlist continua entregando o mínimo para o filho sequer subir.
        assert!(tem(&relatorio, "PATH"), "o filho precisa de PATH");
    };
    tokio::time::timeout(Duration::from_secs(30), body)
        .await
        .expect("test must not hang");
}

/// O mapa `env` do servidor é onde o operador coloca de propósito o que
/// aquele servidor precisa (`GITHUB_TOKEN` e afins) — e ele passa.
#[tokio::test]
async fn mapa_env_explicito_do_servidor_chega_ao_filho() {
    let body = async {
        let mut explicito = HashMap::new();
        explicito.insert(EXPLICITA.to_string(), "do-operador".to_string());

        let relatorio = ambiente_do_filho("env-explicit", explicito, false).await;

        assert_eq!(
            relatorio["values"][EXPLICITA].as_str(),
            Some("do-operador"),
            "o mapa 'env' do servidor não chegou ao filho"
        );
        assert!(
            !tem(&relatorio, PLANTADA),
            "o mapa explícito não pode reabrir a herança do ambiente"
        );
    };
    tokio::time::timeout(Duration::from_secs(30), body)
        .await
        .expect("test must not hang");
}

/// A válvula de escape faz exatamente o que promete — é por isso que o
/// default é `false` e a conexão emite `warn!`.
#[tokio::test]
async fn inherit_env_true_restaura_a_heranca_completa() {
    let body = async {
        let relatorio = ambiente_do_filho("env-inherit", HashMap::new(), true).await;

        assert_eq!(
            relatorio["values"][PLANTADA].as_str(),
            Some("hunter2"),
            "inherit_env=true deve entregar o ambiente inteiro do gateway"
        );
    };
    tokio::time::timeout(Duration::from_secs(30), body)
        .await
        .expect("test must not hang");
}
