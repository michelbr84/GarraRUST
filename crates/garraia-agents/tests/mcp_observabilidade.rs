//! #1438: o health monitor do MCP alimenta o registro de confiabilidade — uma
//! QUEDA por transporte que morreu (nao uma por tick) e cada reconexao com o
//! desfecho. Processo filho de verdade (a fixture Python dos testes de ciclo
//! de vida): so um transporte que morre de fato exercita a deteccao.
#![cfg(feature = "mcp")]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use garraia_agents::McpManager;
use garraia_agents::observabilidade::{McpSnapshot, Observabilidade};
use garraia_agents::tools::ToolContext;

fn fixture_args(extra: &[&str]) -> Vec<String> {
    let script = format!(
        "{}/tests/fixtures/fake_mcp_server.py",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut args = vec![script];
    args.extend(extra.iter().map(|s| s.to_string()));
    args
}

fn ctx() -> ToolContext {
    ToolContext {
        session_id: "mcp-observabilidade-1438".to_string(),
        user_id: None,
        is_heartbeat: false,
        approval: garraia_agents::tools::approval::ToolApproval::None,
        working_dir: None,
        project_id: None,
    }
}

fn servidor(obs: &Observabilidade, nome: &str) -> McpSnapshot {
    obs.snapshot(Instant::now())
        .mcp
        .into_iter()
        .find(|m| m.server == nome)
        .unwrap_or_else(|| panic!("o registro nao viu o servidor {nome}"))
}

#[tokio::test]
async fn queda_e_reconexao_chegam_ao_registro() {
    let body = async {
        let obs = Arc::new(Observabilidade::new());
        let manager = Arc::new(McpManager::new());
        manager.set_observabilidade(Arc::clone(&obs));
        manager
            .connect(
                "fake",
                "python3",
                &fixture_args(&["--crash-after-calls", "1"]),
                &HashMap::new(),
                10,
                vec![],
                None,
                5,
                1,
                false,
            )
            .await
            .expect("a fixture conecta");

        // Vivo: o tick nao conta queda nenhuma.
        manager.health_tick().await;
        assert_eq!(servidor(&obs, "fake").drops, 0);

        let tools = manager.take_tools("fake", Duration::from_secs(10)).await;
        let _ = tools[0].execute(&ctx(), serde_json::json!({})).await;
        for _ in 0..40 {
            if !manager.is_connected("fake").await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(
            !manager.is_connected("fake").await,
            "premissa: o filho morreu"
        );

        manager.health_tick().await;
        assert!(manager.is_connected("fake").await, "premissa: reconectou");
        let fake = servidor(&obs, "fake");
        assert_eq!(fake.drops, 1, "{fake:?}");
        assert_eq!(fake.reconnect_attempts, 1, "{fake:?}");
        assert_eq!(fake.reconnects_ok, 1, "{fake:?}");
        assert_eq!(fake.reconnects_failed, 0, "{fake:?}");
        assert!(fake.last_reconnect_ago_s.is_some());

        // Vivo de novo: nenhuma queda nova.
        manager.health_tick().await;
        assert_eq!(servidor(&obs, "fake").drops, 1);
        manager.disconnect_all().await;
    };
    tokio::time::timeout(Duration::from_secs(30), body)
        .await
        .expect("o teste nao pode pendurar");
}

#[tokio::test]
async fn reconexao_que_falha_conta_como_falha_e_nao_como_queda() {
    let body = async {
        let obs = Arc::new(Observabilidade::new());
        let manager = Arc::new(McpManager::new());
        manager.set_observabilidade(Arc::clone(&obs));
        manager
            .register_pending_stdio(
                "quebrado",
                "garraia-comando-que-nao-existe-1438",
                &[],
                &HashMap::new(),
                5,
                vec![],
                None,
                5,
                1,
                false,
            )
            .await;

        manager.health_tick().await;

        let q = servidor(&obs, "quebrado");
        assert_eq!(q.reconnect_attempts, 1, "{q:?}");
        assert_eq!(q.reconnects_failed, 1, "{q:?}");
        assert_eq!(q.reconnects_ok, 0, "{q:?}");
        assert_eq!(q.drops, 0, "nunca conectou: nao e queda");
    };
    tokio::time::timeout(Duration::from_secs(30), body)
        .await
        .expect("o teste nao pode pendurar");
}
