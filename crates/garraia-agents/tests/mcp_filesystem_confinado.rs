//! Prova ao vivo do #1482 contra o `@modelcontextprotocol/server-filesystem`
//! de verdade (o que o gateway autoprovisiona), com a raiz do servidor no PAI
//! dos diretorios de sessao — exatamente a topologia de `standard`.
//!
//! Opt-in (`GARRAIA_E2E_MCP_FILESYSTEM=1`): precisa de `npx` e, na primeira
//! vez, de rede para baixar o pacote fixado. Os testes unitarios de
//! `mcp::confinamento` sao a prova de CI; este e a evidencia de que o servidor
//! real serviria o arquivo da outra sessao sem o jail, e nao o serve com ele.
#![cfg(feature = "mcp")]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use garraia_agents::McpManager;
use garraia_agents::tools::{FileJail, FileJailDenial, Tool, ToolContext};

/// A mesma versao que `garraia-gateway::mcp::persistence` fixa.
const PACOTE: &str = "@modelcontextprotocol/server-filesystem@2026.8.31";

fn ctx(sessao: &str, working_dir: &std::path::Path) -> ToolContext {
    ToolContext {
        session_id: sessao.to_string(),
        user_id: None,
        is_heartbeat: false,
        approval: garraia_agents::tools::approval::ToolApproval::None,
        working_dir: Some(working_dir.to_string_lossy().into_owned()),
        project_id: None,
    }
}

async fn servidor(raiz: &std::path::Path) -> Arc<McpManager> {
    let manager = Arc::new(McpManager::new());
    manager
        .connect(
            "filesystem",
            "npx",
            &[
                "-y".to_string(),
                PACOTE.to_string(),
                raiz.to_string_lossy().into_owned(),
            ],
            &HashMap::new(),
            60,
            vec![],
            None,
            0,
            1,
            true,
        )
        .await
        .expect("o server-filesystem real sobe (npx no PATH, pacote em cache ou rede)");
    manager
}

fn tool<'a>(tools: &'a [Box<dyn Tool>], nome: &str) -> &'a dyn Tool {
    tools
        .iter()
        .find(|t| t.name() == nome)
        .unwrap_or_else(|| panic!("sem a tool {nome}"))
        .as_ref()
}

#[tokio::test]
async fn sem_jail_o_servidor_serve_a_outra_sessao_e_com_jail_nao() {
    if std::env::var_os("GARRAIA_E2E_MCP_FILESYSTEM").is_none() {
        eprintln!("pulado: exporte GARRAIA_E2E_MCP_FILESYSTEM=1 (precisa de npx)");
        return;
    }
    let tmp = tempfile::tempdir().expect("tempdir");
    let workspace = std::fs::canonicalize(tmp.path())
        .expect("canon")
        .join("workspace");
    let a = workspace.join("sessao-a");
    let b = workspace.join("sessao-b");
    std::fs::create_dir_all(&a).expect("a");
    std::fs::create_dir_all(&b).expect("b");
    std::fs::write(a.join("meu.txt"), "conteudo de A").expect("meu");
    std::fs::write(b.join("segredo.txt"), "segredo de B").expect("segredo");

    let manager = servidor(&workspace).await;
    let tools = manager
        .take_tools("filesystem", Duration::from_secs(60))
        .await;
    assert!(!tools.is_empty(), "o servidor expoe tools");
    let ler = tool(&tools, "filesystem__read_text_file");
    let listar = tool(&tools, "filesystem__list_directory");
    let raizes = tool(&tools, "filesystem__list_allowed_directories");
    let sessao_a = ctx("sessao-a", &a);
    let pedido_b = serde_json::json!({ "path": b.join("segredo.txt") });

    // 1. O achado: SEM jail, o servidor real serve o arquivo da sessao B.
    let vazou = ler
        .execute(&sessao_a, pedido_b.clone())
        .await
        .expect("executa");
    assert!(
        !vazou.is_error && vazou.content.contains("segredo de B"),
        "premissa do achado: sem jail o servidor serve a outra sessao: {}",
        vazou.content
    );
    let pai = raizes
        .execute(&sessao_a, serde_json::json!({}))
        .await
        .expect("executa");
    assert!(
        pai.content
            .contains(&workspace.to_string_lossy().into_owned()),
        "premissa: sem jail o servidor entrega o pai: {}",
        pai.content
    );

    // 2. Com o jail que o gateway entrega no boot, a mesma chamada e recusada
    //    com a frase unica das tools nativas.
    manager.set_jail_das_file_tools(FileJail::sessions_only());
    let recusa = ler.execute(&sessao_a, pedido_b).await.expect("executa");
    assert!(recusa.is_error, "{}", recusa.content);
    assert_eq!(recusa.content, FileJailDenial::Outside.message());
    assert!(!recusa.content.contains("sessao-b"));

    let pai_negado = listar
        .execute(&sessao_a, serde_json::json!({ "path": workspace }))
        .await
        .expect("executa");
    assert!(pai_negado.is_error, "{}", pai_negado.content);

    let so_a = raizes
        .execute(&sessao_a, serde_json::json!({}))
        .await
        .expect("executa");
    assert!(!so_a.is_error);
    assert!(
        so_a.content.contains(&a.to_string_lossy().into_owned()),
        "{}",
        so_a.content
    );
    assert!(!so_a.content.contains("sessao-b"), "{}", so_a.content);
    assert_ne!(
        so_a.content.trim(),
        workspace.to_string_lossy(),
        "{}",
        so_a.content
    );

    // 3. O proprio diretorio continua servido — por caminho absoluto e relativo.
    let meu = ler
        .execute(&sessao_a, serde_json::json!({ "path": a.join("meu.txt") }))
        .await
        .expect("executa");
    assert!(
        !meu.is_error && meu.content.contains("conteudo de A"),
        "{}",
        meu.content
    );
    let relativo = ler
        .execute(&sessao_a, serde_json::json!({ "path": "meu.txt" }))
        .await
        .expect("executa");
    assert!(
        !relativo.is_error && relativo.content.contains("conteudo de A"),
        "{}",
        relativo.content
    );

    manager.disconnect_all().await;
}
