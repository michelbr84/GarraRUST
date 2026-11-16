//! #1913: quando o gateway e o pai adotivo dos descendentes de um servidor
//! MCP (PID 1 de um container, ou subreaper), o encerramento tem de COLHER os
//! que matou — senao cada desconexao deixa um `<defunct>` para sempre.
//!
//! Binario de teste proprio de proposito: `PR_SET_CHILD_SUBREAPER` vale para
//! o processo inteiro, e nao pode vazar para as outras suites.
#![cfg(all(feature = "mcp", target_os = "linux"))]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use garraia_agents::McpManager;

/// `/proc/<pid>` ainda existe? Zumbi conta como existente — e exatamente o
/// que este teste procura.
fn existe(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}/stat")).exists()
}

#[tokio::test]
async fn orfaos_adotados_pelo_gateway_sao_colhidos() {
    // SAFETY: `prctl` so muda um atributo deste processo.
    let rc = unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1) };
    assert_eq!(rc, 0, "PR_SET_CHILD_SUBREAPER");

    let corpo = async {
        let dir = tempfile::tempdir().expect("tempdir");
        let log = dir.path().join("eventos.log");
        let args = vec![
            format!(
                "{}/tests/fixtures/fake_mcp_tree_server.py",
                env!("CARGO_MANIFEST_DIR")
            ),
            "--log".to_string(),
            log.display().to_string(),
            "--descendentes".to_string(),
            "filho,neto,sessao".to_string(),
        ];
        let manager = Arc::new(McpManager::new());
        manager
            .connect(
                "adotados",
                "python3",
                &args,
                &HashMap::new(),
                10,
                vec![],
                None,
                5,
                1,
                false,
            )
            .await
            .expect("conectar");

        let pids: Vec<(String, u32)> = std::fs::read_to_string(&log)
            .expect("log")
            .lines()
            .filter_map(|l| {
                let mut campos = l.split_whitespace();
                (campos.next()? == "descendente")
                    .then(|| Some((campos.next()?.to_string(), campos.next()?.parse().ok()?)))?
            })
            .collect();
        assert_eq!(pids.len(), 3, "descendentes: {pids:?}");

        manager.disconnect("adotados").await;

        // Mortos E colhidos: nem zumbi pode restar. Um segundo encerramento
        // qualquer (aqui, uma conexao nova e a saida dela) nao e necessario:
        // a colheita e do proprio `disconnect`.
        let comeco = Instant::now();
        loop {
            let restam: Vec<_> = pids.iter().filter(|(_, pid)| existe(*pid)).collect();
            if restam.is_empty() {
                break;
            }
            assert!(
                comeco.elapsed() < Duration::from_secs(10),
                "descendentes nao colhidos (zumbis adotados pelo gateway): {restam:?}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(60), corpo)
        .await
        .expect("o teste nao pode pendurar");
}
