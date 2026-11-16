//! L2 do smoke de instalacao limpa da v0.4.4, contra o binario de verdade:
//! `garra status | head` entrava em panico com "failed printing to stdout:
//! Broken pipe" e exit 101.
//!
//! Cada caso roda o comando com stdout ligado a um pipe cujo leitor ja foi
//! fechado — o estado em que o `head` deixa o pipe depois de ler o que queria.
//! Os comandos que so leem e imprimem tem de sair em silencio, mortos por
//! SIGPIPE como `ls | head`; o `mcp-server`, que roda por tempo indeterminado,
//! tem de continuar vivo diante do mesmo pipe (o sinal segue ignorado nele) e
//! sair pelas proprias pernas.
#![cfg(unix)]

use std::io::Write as _;
use std::net::TcpListener;
use std::os::unix::process::ExitStatusExt as _;
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use tempfile::tempdir;

fn garra_bin() -> &'static str {
    env!("CARGO_BIN_EXE_garra")
}

fn porta_livre() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("porta efemera")
        .local_addr()
        .expect("local_addr")
        .port()
}

/// Roda `garra <args>` com stdout num pipe sem leitor e devolve o status e o
/// stderr. `stdin` vazio vira `/dev/null`.
fn roda_com_stdout_fechado(args: &[&str], stdin: &[u8]) -> (ExitStatus, String) {
    let dir = tempdir().expect("tempdir");
    roda_com_stdout_fechado_em(dir.path(), args, stdin)
}

/// Igual a [`roda_com_stdout_fechado`], mas com `HOME` (e o diretorio de
/// trabalho) no caminho dado — para testes que precisam inspecionar o que o
/// comando gravou sob `$HOME`.
fn roda_com_stdout_fechado_em(
    home: &std::path::Path,
    args: &[&str],
    stdin: &[u8],
) -> (ExitStatus, String) {
    let (leitor, escritor) = std::io::pipe().expect("pipe");
    drop(leitor);

    // Uma porta sem ninguem, para que um comando que consulte o gateway
    // (`status`, `doctor`) nunca fale com um daemon real desta maquina.
    let porta = porta_livre().to_string();
    let mut cmd = Command::new(garra_bin());
    cmd.args(args)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home)
        .env("GARRAIA_CONFIG_DIR", home)
        .env("GARRAIA_NO_SPINNER", "1")
        .env("PORT", &porta)
        .env_remove("HOST")
        .env_remove("RUST_BACKTRACE")
        .current_dir(home)
        .stdout(Stdio::from(escritor))
        .stderr(Stdio::piped());
    cmd.stdin(if stdin.is_empty() {
        Stdio::null()
    } else {
        Stdio::piped()
    });

    let mut filho = cmd.spawn().expect("spawn garra");
    if let Some(mut entrada) = filho.stdin.take() {
        // Um processo que ja morreu fecha o stdin; o erro aqui nao importa.
        let _ = entrada.write_all(stdin);
    }
    let inicio = Instant::now();
    loop {
        if filho.try_wait().expect("try_wait").is_some() {
            break;
        }
        if inicio.elapsed() > Duration::from_secs(60) {
            let _ = filho.kill();
            let saida = filho.wait_with_output().expect("output");
            panic!(
                "`garra {}` nao saiu em 60s.\nstderr:\n{}",
                args.join(" "),
                String::from_utf8_lossy(&saida.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let saida = filho.wait_with_output().expect("output");
    (
        saida.status,
        String::from_utf8_lossy(&saida.stderr).into_owned(),
    )
}

#[test]
fn comandos_de_leitura_saem_em_silencio_com_stdout_fechado() {
    let casos: &[&[&str]] = &[
        &["status"],
        &["about"],
        &["doctor"],
        &["doctor", "--json"],
        &["logs"],
        &["logs", "--path"],
        &["runs", "list"],
        &["config", "check"],
        &["memory", "stats"],
        &["mcp", "list"],
        &["channel", "list"],
        &["skill", "list"],
        &["whatsapp", "status"],
    ];
    let mut falhas = Vec::new();
    for args in casos {
        let (status, stderr) = roda_com_stdout_fechado(args, b"");
        let rotulo = format!("garra {}", args.join(" "));
        if stderr.contains("panicked") || stderr.contains("Broken pipe") {
            falhas.push(format!("{rotulo}: panico/EPIPE no stderr:\n{stderr}"));
        }
        if status.code() == Some(101) {
            falhas.push(format!("{rotulo}: exit 101 (panico)"));
        }
        // Todos estes escrevem em stdout, entao todos morrem no primeiro
        // write — pelo sinal, como `ls | head`, nao por erro.
        if status.signal() != Some(libc::SIGPIPE) {
            falhas.push(format!(
                "{rotulo}: esperado morrer por SIGPIPE, veio {status:?}\nstderr:\n{stderr}"
            ));
        }
    }
    assert!(falhas.is_empty(), "{}", falhas.join("\n\n"));
}

/// `garra skills mine | head -1` (#1903): o minerador termina as escritas
/// antes de imprimir, entao a morte por SIGPIPE no primeiro write sem leitor
/// nao pode deixar candidato pela metade — nem reescrever o que ja existe na
/// segunda passada.
#[test]
fn skills_mine_grava_completo_antes_de_morrer_por_sigpipe() {
    let dir = tempdir().expect("tempdir");
    let home = dir.path().join("home");
    let sessoes = dir.path().join("sessoes");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&sessoes).unwrap();
    for id in ["a", "b"] {
        let corpo = format!(
            r#"{{"session_id":"{id}","intent":"release","task_family":"release","commands":[{{"cmd":"git status --porcelain","exit_code":0}},{{"cmd":"cargo test -p garraia --lib","exit_code":0}}],"created_at":"2026-11-15T00:00:00Z"}}"#
        );
        std::fs::write(sessoes.join(format!("{id}.json")), corpo).unwrap();
    }

    let args = [
        "skills",
        "mine",
        "--sessions-dir",
        sessoes.to_str().unwrap(),
        "--threshold",
        "2",
    ];
    let (status, stderr) = roda_com_stdout_fechado_em(&home, &args, b"");
    assert!(
        !stderr.contains("panicked"),
        "mine nao pode entrar em panico com stdout fechado:\n{stderr}"
    );
    assert_ne!(status.code(), Some(101), "exit 101 = panico de println");

    let candidatos: Vec<_> = std::fs::read_dir(home.join(".garra/skills/_candidates"))
        .expect("diretorio de candidatos criado antes de imprimir")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("mined-") && n.ends_with(".md"))
        })
        .collect();
    assert_eq!(candidatos.len(), 1, "um padrao repetido = um candidato");
    let conteudo = std::fs::read_to_string(&candidatos[0]).expect("candidato legivel");
    assert!(
        conteudo.contains("## Commands") && conteudo.contains("Detected in 2 session(s)"),
        "candidato incompleto apos o SIGPIPE: {conteudo}"
    );

    // Segunda passada, tambem com stdout fechado: idempotencia preservada.
    let (status2, _) = roda_com_stdout_fechado_em(&home, &args, b"");
    assert!(
        !matches!(status2.code(), Some(101)),
        "segunda passada entrou em panico"
    );
    assert_eq!(
        std::fs::read_to_string(&candidatos[0]).unwrap(),
        conteudo,
        "candidato reescrito na segunda mineracao"
    );
}

/// O outro lado da mesma decisao: no `mcp-server` o SIGPIPE continua
/// ignorado. Com a resposta do `initialize` sem ter para onde ir, o processo
/// recebe `EPIPE` como erro e sai pelas proprias pernas — nunca morto pelo
/// sinal. Se a troca vazasse para ele, isto veria `signal == SIGPIPE`.
#[test]
fn mcp_server_nao_morre_por_sigpipe_com_stdout_fechado() {
    let initialize = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"#,
        r#""protocolVersion":"2025-06-18","capabilities":{},"#,
        r#""clientInfo":{"name":"teste","version":"0"}}}"#,
        "\n"
    );
    let (status, stderr) = roda_com_stdout_fechado(&["mcp-server"], initialize.as_bytes());
    assert_ne!(
        status.signal(),
        Some(libc::SIGPIPE),
        "o mcp-server morreu por SIGPIPE — a troca vazou para um processo de longa duracao.\nstderr:\n{stderr}"
    );
    // Saiu pelas proprias pernas (exit code, nenhum sinal), sem panico: o
    // `EPIPE` virou o erro de transporte que o rmcp ja trata.
    assert!(
        status.code().is_some_and(|c| c != 101),
        "o mcp-server tinha de sair com um exit code proprio, veio {status:?}.\nstderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("panicked"),
        "o mcp-server entrou em panico:\n{stderr}"
    );
}
