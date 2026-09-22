//! #1225 S2c: as tools de programa (`repo_search`, `run_tests`, `git_diff`)
//! contra um Docker REAL, com a imagem default da policy
//! (`debian:bookworm-slim`, que tem `grep` e nao tem `rg`, `cargo` nem `git`).
//!
//! Os testes unitarios (`sandbox_spawn.rs`) provam o argv com um runtime
//! falso. Este prova o efeito: a busca acontece DENTRO do container, sobre o
//! diretorio montado, e as tools cujo programa nao existe na imagem falham
//! com a mensagem acionavel em vez de cair no programa do host.
//!
//! Anti-vacuidade: a busca tem contraprova de mount (sem `mount_workdir` o
//! marcador nao aparece) e gemeo no host (com `elevated` o mesmo marcador e
//! achado pelo programa do host). Skip/`GARRAIA_REQUIRE_DOCKER` no padrao do
//! `sandbox_docker.rs`.
#![cfg(unix)]

use std::process::Stdio;
use std::time::Duration;

use garraia_agents::tools::approval::ToolApproval;
use garraia_agents::tools::git_diff_tool::GitDiffTool;
use garraia_agents::tools::{RepoSearchTool, RunTestsTool, Tool, ToolContext, ToolOutput};
use garraia_agents::{SandboxBackend, SandboxMode, SandboxPolicy};

const TIMEOUT_TOOL_SEGS: u64 = 90;
const TIMEOUT_TESTE: Duration = Duration::from_secs(120);

async fn docker_ok() -> bool {
    let mut sonda = tokio::process::Command::new("docker");
    sonda
        .arg("info")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env_clear();
    for (chave, valor) in garraia_common::safety_gate::allowed_child_env() {
        sonda.env(chave, valor);
    }
    let ok = matches!(
        tokio::time::timeout(Duration::from_secs(20), sonda.status()).await,
        Ok(Ok(s)) if s.success()
    );
    if !ok {
        assert!(
            std::env::var_os("GARRAIA_REQUIRE_DOCKER").is_none(),
            "GARRAIA_REQUIRE_DOCKER esta setado, mas o Docker nao respondeu"
        );
        eprintln!("[skip] Docker indisponivel; smoke das tools sandboxadas (#1225 S2c) pulado");
    }
    ok
}

fn policy(mount_workdir: bool, elevated: &[&str]) -> SandboxPolicy {
    SandboxPolicy {
        mode: SandboxMode::All,
        backend: Some(SandboxBackend::Docker),
        mount_workdir,
        elevated: elevated.iter().map(|s| s.to_string()).collect(),
        ..SandboxPolicy::default()
    }
}

fn ctx(dir: &std::path::Path) -> ToolContext {
    ToolContext {
        session_id: "sandbox-docker-tools".into(),
        user_id: None,
        is_heartbeat: false,
        approval: ToolApproval::None,
        working_dir: Some(dir.to_string_lossy().into_owned()),
        project_id: None,
    }
}

async fn executar(tool: &dyn Tool, dir: &std::path::Path, input: serde_json::Value) -> ToolOutput {
    match tokio::time::timeout(TIMEOUT_TESTE, tool.execute(&ctx(dir), input)).await {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => ToolOutput::error(e.to_string()),
        Err(_) => panic!("{} travou", tool.name()),
    }
}

/// A busca roda no container (rg ausente -> grep no container) e acha o
/// marcador no diretorio montado; sem o mount, nao acha; elevada, o host acha.
#[tokio::test(flavor = "multi_thread")]
async fn repo_search_busca_dentro_do_container_sobre_o_mount() {
    if !docker_ok().await {
        return;
    }
    let base = tempfile::tempdir().expect("tmp");
    let dir = base.path().canonicalize().expect("canon");
    let marcador = format!("agulha-sandbox-{}", std::process::id());
    std::fs::write(dir.join("palheiro.txt"), format!("{marcador}\n")).expect("write");
    let input = serde_json::json!({"query": marcador});

    let com_mount =
        RepoSearchTool::new(Some(TIMEOUT_TOOL_SEGS), None).com_sandbox(policy(true, &[]));
    let out = executar(&com_mount, &dir, input.clone()).await;
    assert!(
        out.content.contains("palheiro.txt"),
        "o grep no container nao achou o marcador montado: {}",
        out.content
    );

    // Contraprova: sem mount o mesmo pedido nao ve o arquivo do host.
    let sem_mount =
        RepoSearchTool::new(Some(TIMEOUT_TOOL_SEGS), None).com_sandbox(policy(false, &[]));
    let out = executar(&sem_mount, &dir, input.clone()).await;
    assert!(
        !out.content.contains("palheiro.txt"),
        "sem mount_workdir o marcador apareceu — a busca rodou no host: {}",
        out.content
    );

    // Gemeo: elevada, roda no host e acha.
    let elevada = RepoSearchTool::new(Some(TIMEOUT_TOOL_SEGS), None)
        .com_sandbox(policy(true, &["repo_search"]));
    let out = executar(&elevada, &dir, input).await;
    assert!(out.content.contains("palheiro.txt"), "{}", out.content);
}

/// `cargo` nao existe na imagem default: a tool diz isso, em vez de rodar o
/// `cargo` do host (que existe neste runner).
#[tokio::test(flavor = "multi_thread")]
async fn run_tests_com_imagem_sem_cargo_nao_cai_no_cargo_do_host() {
    if !docker_ok().await {
        return;
    }
    let base = tempfile::tempdir().expect("tmp");
    let dir = base.path().canonicalize().expect("canon");
    let tool = RunTestsTool::new(Some(TIMEOUT_TOOL_SEGS)).com_sandbox(policy(true, &[]));
    let out = executar(&tool, &dir, serde_json::json!({"framework": "cargo"})).await;
    assert!(out.is_error, "{}", out.content);
    assert!(
        out.content.contains("nao existe na imagem"),
        "esperava a mensagem de programa ausente na imagem: {}",
        out.content
    );
    assert!(
        out.content.contains("agent.sandbox.elevated"),
        "{}",
        out.content
    );
}

/// Mesmo contrato para o `git` do `git_diff`.
#[tokio::test(flavor = "multi_thread")]
async fn git_diff_com_imagem_sem_git_nao_cai_no_git_do_host() {
    if !docker_ok().await {
        return;
    }
    let base = tempfile::tempdir().expect("tmp");
    let dir = base.path().canonicalize().expect("canon");
    let tool = GitDiffTool::new(Some(TIMEOUT_TOOL_SEGS), None).com_sandbox(policy(true, &[]));
    let out = executar(&tool, &dir, serde_json::json!({"operation": "status"})).await;
    assert!(
        out.content.contains("nao existe na imagem"),
        "esperava a mensagem de programa ausente na imagem: {}",
        out.content
    );
}
