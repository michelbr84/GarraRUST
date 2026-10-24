//! Achado da validacao do v0.4.10-rc1: numa maquina sem Node.js (CLI num
//! Linux limpo, desktop num Windows sem Node global — o Node privado do
//! instalador e so da ponte do WhatsApp), o boot provisionava um `filesystem`
//! MCP via `npx` que nunca subia, e o `/api/diagnostics` de uma instalacao
//! nova ficava amarelo.
//!
//! Os testes unitarios de `mcp::persistence` injetam a busca do `npx`. Este
//! binario prova o mesmo contrato com a busca DE VERDADE
//! (`McpPersistenceService::provision_filesystem_if_missing`, que procura na
//! `PATH` do processo): ausencia real, arquivo sem permissao de execucao e um
//! executavel real. Mexe na `PATH`, por isso vive num binario de teste
//! proprio (processo separado) e num unico `#[test]` sequencial — nenhum
//! outro teste deste processo le a `PATH` em paralelo.

use std::path::{Path, PathBuf};

use garraia_channels::whatsapp_linked::bridge::find_executable;
use garraia_gateway::bootstrap::RaizesDoMcpFilesystem;
use garraia_gateway::mcp::McpPersistenceService;

/// Um `npx` executavel de verdade (nunca e chamado: a provisao so grava a
/// config). No Windows a busca aceita `.cmd`, como o `npx.cmd` do Node.
fn npx_falso_executavel(dir: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let p = dir.join("npx.cmd");
        std::fs::write(&p, "@echo off\r\nexit /b 0\r\n").expect("write npx.cmd");
        p
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join("npx");
        std::fs::write(&p, "#!/bin/sh\nexit 0\n").expect("write npx");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        p
    }
}

fn com_path<T>(path: &std::ffi::OsStr, f: impl FnOnce() -> T) -> T {
    // SAFETY: unico teste deste binario; nada mais le a env em paralelo.
    unsafe { std::env::set_var("PATH", path) };
    f()
}

#[test]
fn sem_npx_real_na_path_a_provisao_nao_grava_nada_e_com_npx_grava() {
    let path_original = std::env::var_os("PATH");
    // SAFETY: idem.
    unsafe { std::env::remove_var(McpPersistenceService::DISABLE_AUTOPROVISION_ENV) };

    // --- 1. Ausencia real: a PATH so tem um diretorio vazio. ---------------
    let vazio = tempfile::tempdir().expect("tempdir");
    let base = tempfile::tempdir().expect("tempdir");
    let mcp_json = base.path().join("mcp.json");
    let workspace = base.path().join("data").join("workspace");
    com_path(vazio.path().as_os_str(), || {
        // Sanidade: o teste so vale se a busca real de fato nao acha `npx`.
        assert_eq!(
            find_executable("npx"),
            None,
            "a PATH do teste nao pode ter npx"
        );
        McpPersistenceService::new(&mcp_json)
            .provision_filesystem_if_missing(&RaizesDoMcpFilesystem::Workspace(workspace.clone()));
    });
    assert!(!mcp_json.exists(), "sem npx real => nenhum mcp.json");
    assert!(
        !workspace.exists(),
        "sem npx real => o workspace default nao e criado"
    );

    // --- 2. Arquivo `npx` SEM permissao de execucao (Unix) continua sendo
    //        ausencia: a busca nao pode tratar "existe" como "executa". ------
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        let sem_x = tempfile::tempdir().expect("tempdir");
        let p = sem_x.path().join("npx");
        std::fs::write(&p, "#!/bin/sh\nexit 0\n").expect("write");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).expect("chmod");
        com_path(sem_x.path().as_os_str(), || {
            assert_eq!(find_executable("npx"), None, "npx sem +x nao conta");
            McpPersistenceService::new(&mcp_json).provision_filesystem_if_missing(
                &RaizesDoMcpFilesystem::Workspace(workspace.clone()),
            );
        });
        assert!(!mcp_json.exists(), "npx sem +x => nenhum mcp.json");
        assert!(!workspace.exists());
    }

    // --- 3. Um executavel `npx` real na PATH: provisiona como sempre. ------
    let bin = tempfile::tempdir().expect("tempdir");
    let npx = npx_falso_executavel(bin.path());
    com_path(bin.path().as_os_str(), || {
        assert_eq!(find_executable("npx"), Some(npx.clone()));
        McpPersistenceService::new(&mcp_json)
            .provision_filesystem_if_missing(&RaizesDoMcpFilesystem::Workspace(workspace.clone()));
    });

    // SAFETY: idem — restaura antes das asserções finais.
    unsafe {
        match path_original {
            Some(p) => std::env::set_var("PATH", p),
            None => std::env::remove_var("PATH"),
        }
    }

    let cru = std::fs::read_to_string(&mcp_json).expect("com npx o mcp.json e gravado");
    let json: serde_json::Value = serde_json::from_str(&cru).expect("json");
    let fs = &json["mcpServers"]["filesystem"];
    assert_eq!(fs["command"], "npx", "{cru}");
    assert!(workspace.is_dir(), "com npx o workspace default e criado");
}
