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

/// Nome único por design, para não colidir com nada que o runner exporte.
const PLANTADA: &str = "GARRAIA_TEST_PLANTED_SECRET";
const EXPLICITA: &str = "GARRAIA_TEST_EXPLICIT";

/// Planta o segredo no ambiente DESTE processo de teste.
///
/// SAFETY: `set_var` é `unsafe` na edition 2024 porque escrever no bloco de
/// ambiente enquanto outra thread o lê é UB — e ler é exatamente o que
/// `std::process::Command` faz ao copiar o ambiente para um filho.
///
/// A garantia aqui NÃO é o `Once`: é que este binário de teste tem um único
/// `#[tokio::test]` (ver `ambiente_do_filho_por_politica` abaixo), e a
/// chamada acontece antes de qualquer spawn. Um `Once` não serviria — ele
/// garante que a escrita ocorre uma vez, não que ninguém esteja lendo o
/// ambiente naquele instante. A versão anterior deste arquivo tinha três
/// `#[tokio::test]` paralelos, cada um iterando `vars_os()` para montar o
/// ambiente do filho: uma corrida real na glibc.
///
/// Se um teste novo for adicionado a este arquivo, ele PRECISA rodar dentro
/// do mesmo `#[tokio::test]`, ou a corrida volta.
fn plantar_segredo() {
    unsafe { std::env::set_var(PLANTADA, "hunter2") };
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

/// Os três cenários do isolamento de ambiente, num ÚNICO teste e em
/// sequência.
///
/// Sequencial de propósito, e não por estilo: `plantar_segredo` escreve no
/// bloco de ambiente do processo e cada spawn o lê inteiro (`vars_os`). Com
/// `#[tokio::test]` separados o cargo roda os três em paralelo, e escrita
/// concorrente com leitura do `environ` é UB na glibc — o teste passaria
/// quase sempre e falharia sem explicação de vez em quando, que é o pior
/// resultado possível para um teste de segurança.
///
/// O plantio acontece uma vez, aqui, antes de qualquer `connect`.
#[tokio::test]
async fn ambiente_do_filho_por_politica() {
    plantar_segredo();

    let body = async {
        // 1. O defeito. Falha antes da correção: a variável plantada no
        //    gateway chegava inteira ao filho, junto de todo o resto do cofre.
        let padrao = ambiente_do_filho("env-default", HashMap::new(), false).await;
        assert!(
            !tem(&padrao, PLANTADA),
            "o filho MCP recebeu '{PLANTADA}', plantada no ambiente do gateway"
        );
        // A allowlist continua entregando o mínimo para o filho sequer subir.
        assert!(tem(&padrao, "PATH"), "o filho precisa de PATH");

        // 2. O mapa `env` do servidor é onde o operador coloca de propósito o
        //    que aquele servidor precisa (`GITHUB_TOKEN` e afins) — e ele
        //    passa, sem reabrir a herança.
        let mut explicito = HashMap::new();
        explicito.insert(EXPLICITA.to_string(), "do-operador".to_string());
        let com_mapa = ambiente_do_filho("env-explicit", explicito, false).await;
        assert_eq!(
            com_mapa["values"][EXPLICITA].as_str(),
            Some("do-operador"),
            "o mapa 'env' do servidor não chegou ao filho"
        );
        assert!(
            !tem(&com_mapa, PLANTADA),
            "o mapa explícito não pode reabrir a herança do ambiente"
        );

        // 3. A válvula de escape faz exatamente o que promete — é por isso
        //    que o default é `false` e a conexão emite `warn!`.
        let herdado = ambiente_do_filho("env-inherit", HashMap::new(), true).await;
        assert_eq!(
            herdado["values"][PLANTADA].as_str(),
            Some("hunter2"),
            "inherit_env=true deve entregar o ambiente inteiro do gateway"
        );
    };

    tokio::time::timeout(Duration::from_secs(90), body)
        .await
        .expect("test must not hang");
}
