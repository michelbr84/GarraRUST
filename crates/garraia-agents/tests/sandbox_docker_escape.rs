//! #1272 S2: o sandbox docker como FRONTEIRA, contra um Docker real.
//!
//! O `tests/sandbox_docker.rs` prova que o comando roda dentro do container.
//! Este prova o que a #1272 precisa para deixar o `bash` ligado em
//! `execution.profile = standard`: de dentro do container, nenhuma forma de
//! shell alcanca um arquivo do host fora do diretorio montado — nem por
//! redirecao, nem por subshell, `sh -c`, `tee`, pipe, symlink ou filho em
//! background — e nada que o comando deixa no mount pertence a root.
//!
//! O tier arriscado do `safety_gate` e contornado DE PROPOSITO: cada comando
//! vai com a aprovacao do proprio comando (`ToolApproval::granted`). E o pior
//! caso — o gate textual deixou passar — e o que sobra tem de ser o
//! isolamento de processo, porque uma lista negra de texto nao e fronteira.
//!
//! Anti-vacuidade: todo comando imprime `IN_CONTAINER`, e cada leitura
//! negada tem um gemeo que le o mesmo conteudo DENTRO do mount e acha. O
//! padrao de skip/`GARRAIA_REQUIRE_DOCKER` e o do `sandbox_docker.rs`.
#![cfg(unix)]

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use garraia_agents::tools::approval::ToolApproval;
use garraia_agents::tools::{BashTool, Tool, ToolContext, ToolOutput};
use garraia_agents::{SandboxBackend, SandboxMode, SandboxPolicy};

const TIMEOUT_TOOL_SEGS: u64 = 120;
const TIMEOUT_TESTE: Duration = Duration::from_secs(150);
const TIMEOUT_SONDA: Duration = Duration::from_secs(20);
const PROVA_DE_CONTAINER: &str = "test -f /.dockerenv && echo IN_CONTAINER || echo ON_HOST";

async fn docker_disponivel() -> Result<(), String> {
    let mut sonda = tokio::process::Command::new("docker");
    sonda
        .arg("info")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .env_clear();
    for (chave, valor) in garraia_common::safety_gate::allowed_child_env() {
        sonda.env(chave, valor);
    }
    match tokio::time::timeout(TIMEOUT_SONDA, sonda.output()).await {
        Ok(Ok(saida)) if saida.status.success() => Ok(()),
        Ok(Ok(saida)) => Err(format!("`docker info` saiu com {}", saida.status)),
        Ok(Err(e)) => Err(format!("nao foi possivel executar `docker`: {e}")),
        Err(_) => Err("`docker info` nao respondeu".to_string()),
    }
}

async fn tool_docker() -> Option<BashTool> {
    if let Err(motivo) = docker_disponivel().await {
        assert!(
            std::env::var_os("GARRAIA_REQUIRE_DOCKER").is_none(),
            "GARRAIA_REQUIRE_DOCKER esta setado, mas o Docker nao respondeu: {motivo}"
        );
        eprintln!("[skip] Docker indisponivel ({motivo}); escape do sandbox (#1272) pulado");
        return None;
    }
    let mut tool = BashTool::new_with_confirmation(Some(TIMEOUT_TOOL_SEGS));
    tool.set_sandbox_policy(SandboxPolicy {
        mode: SandboxMode::All,
        backend: Some(SandboxBackend::Docker),
        ..SandboxPolicy::default()
    });
    Some(tool)
}

/// Um workdir montado e, IRMAO dele (fora do mount), um diretorio do host
/// com um segredo.
struct Cenario {
    _base: tempfile::TempDir,
    workdir: PathBuf,
    fora: PathBuf,
    segredo: PathBuf,
    conteudo: String,
}

fn cenario() -> Cenario {
    let base = tempfile::tempdir().expect("tempdir");
    let raiz = base.path().canonicalize().expect("canon");
    let workdir = raiz.join("workdir");
    let fora = raiz.join("fora");
    std::fs::create_dir_all(&workdir).expect("mkdir workdir");
    std::fs::create_dir_all(&fora).expect("mkdir fora");
    let conteudo = format!("segredo-do-host-{}", std::process::id());
    let segredo = fora.join("segredo.txt");
    std::fs::write(&segredo, format!("{conteudo}\n")).expect("write segredo");
    Cenario {
        _base: base,
        workdir,
        fora,
        segredo,
        conteudo,
    }
}

async fn executar(tool: &BashTool, workdir: &Path, comando: &str) -> ToolOutput {
    let contexto = ToolContext {
        session_id: "sandbox-escape-1272".to_string(),
        user_id: None,
        is_heartbeat: false,
        // O pior caso: o gate textual aprovou este comando.
        approval: ToolApproval::granted("bash", comando),
        working_dir: Some(workdir.to_string_lossy().into_owned()),
        project_id: None,
    };
    let chamada = tool.execute(&contexto, serde_json::json!({ "command": comando }));
    match tokio::time::timeout(TIMEOUT_TESTE, chamada).await {
        Ok(Ok(saida)) => saida,
        Ok(Err(e)) => panic!("execute devolveu Err para `{comando}`: {e}"),
        Err(_) => panic!("`{comando}` travou"),
    }
}

/// Leituras do host fora do mount: todas as formas de shell falham, e o
/// gemeo (o mesmo conteudo copiado para DENTRO do workdir) e lido.
#[tokio::test(flavor = "multi_thread")]
async fn nenhuma_forma_de_shell_le_arquivo_do_host_fora_do_mount() {
    let Some(tool) = tool_docker().await else {
        return;
    };
    let c = cenario();
    let s = c.segredo.display();
    for forma in [
        format!("cat {s}"),
        format!("ln -s {s} link; cat link"),
        format!("echo $(cat {s})"),
        format!("sh -c 'cat {s}'"),
        format!("cat {s} | tee copia.txt"),
        format!("(cat {s}) > sub.txt; cat sub.txt"),
        format!("nohup cat {s} > bg.txt 2>&1 & wait; cat bg.txt"),
        "cat /etc/shadow".to_string(),
    ] {
        let comando = format!("{PROVA_DE_CONTAINER}; {forma}");
        let saida = executar(&tool, &c.workdir, &comando).await;
        assert!(
            saida.content.contains("IN_CONTAINER"),
            "`{forma}` rodou fora do container: {}",
            saida.content
        );
        assert!(
            !saida.content.contains(&c.conteudo),
            "`{forma}` leu o arquivo do host: {}",
            saida.content
        );
    }
    // /etc/shadow do host nao e o do container: o do container nem e legivel
    // pelo uid do operador. Nenhuma linha `root:` do host pode aparecer.
    let shadow = executar(&tool, &c.workdir, "cat /etc/shadow").await;
    assert!(
        !shadow.content.contains("root:$"),
        "hash de senha apareceu: {}",
        shadow.content
    );

    // Gemeo positivo: o mesmo conteudo dentro do mount e lido.
    std::fs::write(c.workdir.join("dentro.txt"), &c.conteudo).expect("write");
    let dentro = executar(&tool, &c.workdir, "cat dentro.txt").await;
    assert!(
        dentro.content.contains(&c.conteudo),
        "o mount nao funcionou (teste vacuo): {}",
        dentro.content
    );
}

/// Escritas fora do mount: nenhum arquivo aparece no host, por nenhuma forma.
#[tokio::test(flavor = "multi_thread")]
async fn nenhuma_forma_de_shell_escreve_fora_do_mount() {
    let Some(tool) = tool_docker().await else {
        return;
    };
    let c = cenario();
    let f = c.fora.display();
    let formas = [
        (format!("echo x > {f}/w1"), "w1"),
        (format!("sh -c 'echo x > {f}/w2'"), "w2"),
        (format!("echo x | tee {f}/w3"), "w3"),
        (format!("(echo x > {f}/w4) & wait"), "w4"),
        (format!("ln -s {f} saida; echo x > saida/w5"), "w5"),
        (format!("mkdir -p {f} && touch {f}/w6"), "w6"),
    ];
    for (forma, nome) in &formas {
        let saida = executar(&tool, &c.workdir, &format!("{PROVA_DE_CONTAINER}; {forma}")).await;
        assert!(saida.content.contains("IN_CONTAINER"), "{}", saida.content);
        assert!(
            !c.fora.join(nome).exists(),
            "`{forma}` escreveu no host fora do mount"
        );
    }
    // Gemeo positivo: escrever no workdir funciona e aparece no host.
    let saida = executar(&tool, &c.workdir, "echo ok > dentro.txt").await;
    assert!(!saida.is_error, "{}", saida.content);
    assert!(c.workdir.join("dentro.txt").exists());
}

/// Nada que o comando deixa no mount pertence a root, e root nao existe la
/// dentro: `id -u` e o uid do operador, e um `chmod u+s` so marca um arquivo
/// DO OPERADOR (que roda como ele mesmo).
#[tokio::test(flavor = "multi_thread")]
async fn artefato_no_mount_e_do_operador_e_nunca_de_root() {
    let Some(tool) = tool_docker().await else {
        return;
    };
    let c = cenario();
    let meu_uid = std::fs::metadata(&c.workdir).expect("meta").uid();
    let saida = executar(
        &tool,
        &c.workdir,
        &format!("{PROVA_DE_CONTAINER}; id -u; cp /bin/sh ./s && chmod u+s ./s; mknod dev c 1 3"),
    )
    .await;
    assert!(saida.content.contains("IN_CONTAINER"), "{}", saida.content);
    assert!(
        saida.content.contains(&format!("\n{meu_uid}\n")),
        "o container nao roda com o uid do operador ({meu_uid}): {}",
        saida.content
    );
    let s = c.workdir.join("s");
    assert!(s.exists(), "cp falhou: {}", saida.content);
    let dono = std::fs::metadata(&s).expect("meta s").uid();
    assert_eq!(dono, meu_uid, "artefato no mount pertence a {dono}");
    assert_ne!(dono, 0, "artefato de root no mount");
    assert!(
        !c.workdir.join("dev").exists(),
        "mknod criou device no host (cap-drop ausente)"
    );
}
