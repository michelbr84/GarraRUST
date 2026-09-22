//! #1247 contra o binario de verdade: `start`, `restart` e `start -d` rodam o
//! `config check` no boot; so a allowlist fechada (hoje: TLS pela metade)
//! recusa, com exit 78 e a escotilha `GARRAIA_ALLOW_INVALID_CONFIG=1`.
//!
//! Todos os casos ligam em loopback (sem `HOST`), para que a recusa do #1261
//! nao se misture com a do boot gate.

use std::net::TcpListener;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

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

fn comando(dir: &std::path::Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(garra_bin());
    cmd.args(args)
        .env("XDG_CONFIG_HOME", dir)
        .env("GARRAIA_CONFIG_DIR", dir)
        .env("HOME", dir)
        .env("GARRAIA_NO_SPINNER", "1")
        .env_remove("GARRAIA_ALLOW_INVALID_CONFIG")
        .env_remove("GARRAIA_GATEWAY_API_KEY")
        .env_remove("HOST")
        .env_remove("PORT")
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

/// Espera o processo sair (a recusa e imediata); se ele subir, mata e falha.
fn roda_com_teto(mut cmd: Command) -> Output {
    let mut filho = cmd.spawn().expect("spawn garra");
    let inicio = std::time::Instant::now();
    loop {
        if filho.try_wait().expect("try_wait").is_some() {
            return filho.wait_with_output().expect("output");
        }
        if inicio.elapsed() > Duration::from_secs(20) {
            let _ = filho.kill();
            let out = filho.wait_with_output().expect("output");
            panic!(
                "o processo nao saiu — subiu em vez de recusar.\nstderr:\n{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Deixa o processo rodar um pouco, mata, e devolve a saida + se ele saiu
/// sozinho (e com que codigo).
fn roda_um_pouco(mut cmd: Command) -> (Option<i32>, String) {
    let mut filho = cmd.spawn().expect("spawn garra");
    std::thread::sleep(Duration::from_secs(3));
    let saiu = filho.try_wait().expect("try_wait").map(|s| s.code());
    let _ = filho.kill();
    let out = filho.wait_with_output().expect("output");
    let texto = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (saiu.flatten(), texto)
}

fn log_do_gateway(dir: &std::path::Path) -> String {
    std::fs::read_to_string(dir.join("garraia.log")).unwrap_or_default()
}

fn config_tls_pela_metade(dir: &std::path::Path) {
    std::fs::write(
        dir.join("config.yml"),
        "gateway:\n  tls_cert_path: \"/nao/existe/cert.pem\"\n",
    )
    .expect("config");
}

fn assert_recusou(out: &Output, contexto: &str) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(78),
        "{contexto}: exit 78 esperado\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("gateway.tls_key_path"),
        "{contexto}: {stderr}"
    );
    assert!(
        stderr.contains("GARRAIA_ALLOW_INVALID_CONFIG"),
        "{contexto}: {stderr}"
    );
    assert!(stderr.contains(" config check`"), "{contexto}: {stderr}");
}

#[test]
fn tls_pela_metade_recusa_o_start_e_nada_escuta() {
    let dir = tempdir().expect("tempdir");
    config_tls_pela_metade(dir.path());
    let porta = porta_livre();
    let p = porta.to_string();
    let out = roda_com_teto(comando(dir.path(), &["start", "--port", &p]));
    assert_recusou(&out, "start");
    TcpListener::bind(("127.0.0.1", porta))
        .unwrap_or_else(|e| panic!("ninguem podia ter escutado em {porta}: {e}"));
}

/// Qualquer valor que nao seja exatamente "1" nao abre a escotilha.
#[test]
fn escotilha_com_valor_errado_continua_recusando() {
    let dir = tempdir().expect("tempdir");
    config_tls_pela_metade(dir.path());
    let p = porta_livre().to_string();
    for valor in ["true", "0", " 1", "yes"] {
        let mut cmd = comando(dir.path(), &["start", "--port", &p]);
        cmd.env("GARRAIA_ALLOW_INVALID_CONFIG", valor);
        let out = roda_com_teto(cmd);
        assert_recusou(&out, &format!("GARRAIA_ALLOW_INVALID_CONFIG={valor:?}"));
    }
}

#[test]
fn escotilha_um_passa_do_gate_e_segue_dizendo_o_erro() {
    let dir = tempdir().expect("tempdir");
    config_tls_pela_metade(dir.path());
    let p = porta_livre().to_string();
    let mut cmd = comando(dir.path(), &["start", "--port", &p]);
    cmd.env("GARRAIA_ALLOW_INVALID_CONFIG", "1");
    let (saiu, saida) = roda_um_pouco(cmd);
    assert_ne!(
        saiu,
        Some(78),
        "a escotilha tinha de passar do gate:\n{saida}"
    );
    let tudo = format!("{saida}{}", log_do_gateway(dir.path()));
    assert!(
        tudo.contains("boot allowed anyway by GARRAIA_ALLOW_INVALID_CONFIG=1"),
        "o achado bloqueante segue logado como erro:\n{tudo}"
    );
}

/// Error fora da allowlist (uma entrada `llm` sem chave) NAO bloqueia — e e
/// dito uma vez.
#[test]
fn error_fora_da_allowlist_sobe_e_e_dito_uma_vez() {
    let dir = tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("config.yml"),
        "llm:\n  semchave1247:\n    provider: anthropic\n    model: claude-x\n",
    )
    .expect("config");
    let p = porta_livre().to_string();
    let mut cmd = comando(dir.path(), &["start", "--port", &p]);
    cmd.env_remove("ANTHROPIC_API_KEY")
        .env_remove("GARRAIA_ANTHROPIC_API_KEY")
        .env_remove("GARRAIA_VAULT_PASSPHRASE");
    let (saiu, saida) = roda_um_pouco(cmd);
    assert_ne!(saiu, Some(78), "fora da allowlist nao recusa:\n{saida}");
    let log = log_do_gateway(dir.path());
    let tudo = format!("{saida}{log}");
    let vezes_no_log = log.matches("config error [llm.semchave1247").count();
    let vezes_na_saida = saida.matches("config error [llm.semchave1247").count();
    assert!(
        vezes_no_log.max(vezes_na_saida) == 1,
        "o Error aparece uma vez (log={vezes_no_log}, saida={vezes_na_saida}):\n{tudo}"
    );
    assert!(tudo.contains(" config check` for details"), "{tudo}");
}

/// `start -d`: a recusa sai no stderr do pai, antes do fork; sem PID file.
#[test]
fn start_daemon_recusa_antes_do_fork() {
    let dir = tempdir().expect("tempdir");
    config_tls_pela_metade(dir.path());
    let p = porta_livre().to_string();
    let out = roda_com_teto(comando(dir.path(), &["start", "-d", "--port", &p]));
    assert_recusou(&out, "start -d");
    assert!(!dir.path().join("garraia.pid").exists(), "sem PID file");
}

#[test]
fn restart_passa_pelo_mesmo_gate() {
    let dir = tempdir().expect("tempdir");
    config_tls_pela_metade(dir.path());
    let p = porta_livre().to_string();
    let out = roda_com_teto(comando(dir.path(), &["restart", "--port", &p]));
    assert_recusou(&out, "restart");
}

/// O gate roda antes do `try_stop_daemon`: um `restart` recusado nao pode
/// deixar o operador sem gateway. Um sentinela vivo no `garraia.pid` faz o
/// papel do daemon atual e tem de sobreviver, no foreground e no `-d` —
/// mover o gate para depois do stop mata o sentinela e deixa isto vermelho.
#[cfg(unix)]
#[test]
fn restart_recusado_nao_derruba_o_daemon_atual() {
    for args in [&["restart"][..], &["restart", "-d"][..]] {
        let dir = tempdir().expect("tempdir");
        config_tls_pela_metade(dir.path());
        let mut sentinela = Command::new("sleep")
            .arg("60")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn sentinela");
        std::fs::write(dir.path().join("garraia.pid"), sentinela.id().to_string())
            .expect("pid file");
        let p = porta_livre().to_string();
        let mut todos: Vec<&str> = args.to_vec();
        todos.extend(["--port", &p]);
        let out = roda_com_teto(comando(dir.path(), &todos));
        let vivo = sentinela.try_wait().expect("try_wait").is_none();
        let _ = sentinela.kill();
        let _ = sentinela.wait();
        assert_recusou(&out, &args.join(" "));
        assert!(
            vivo,
            "`garra {}` recusado derrubou o daemon atual",
            args.join(" ")
        );
    }
}
