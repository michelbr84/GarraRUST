//! Smoke de `garra whatsapp allow|users|remove|owner|unowner` sem terminal
//! (#1345), contra o binario de verdade — a metade de gestao de acesso do
//! `whatsapp_smoke.rs`, em arquivo proprio pelo teto de 700 linhas do Quality
//! Ratchet (`.quality/`, plan 0064). O molde e o mesmo: o que aparece na tela
//! e o exit code.

use std::process::{Command, Stdio};

use tempfile::tempdir;

fn garra_bin() -> &'static str {
    env!("CARGO_BIN_EXE_garra")
}

// ---------------------------------------------------------------------------
// #1345: `whatsapp allow` sem terminal
// ---------------------------------------------------------------------------

/// O mesmo `garra()`, com env extra (o perfil de execucao, sobretudo).
fn garra_env(dir: &std::path::Path, args: &[&str], env: &[(&str, &str)]) -> std::process::Output {
    let mut cmd = Command::new(garra_bin());
    cmd.args(args)
        .env("XDG_CONFIG_HOME", dir)
        .env("GARRAIA_CONFIG_DIR", dir)
        .env("HOME", dir)
        .env("GARRAIA_LANG", "pt_BR.UTF-8")
        .env_remove("GARRAIA_EXECUTION_PROFILE");
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn garra")
}

fn config_yml(dir: &std::path::Path) -> String {
    std::fs::read_to_string(dir.join("config.yml")).unwrap_or_default()
}

/// Num pipe, `allow` grava o numero normalizado e sai 0 — e na tela so
/// aparecem os quatro ultimos digitos.
#[test]
fn allow_in_a_pipe_writes_the_config_and_exits_zero() {
    let dir = tempdir().expect("tempdir");
    let out = garra_env(dir.path(), &["whatsapp", "allow", "+55 11 99999-8888"], &[]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        config_yml(dir.path()).contains("5511999998888"),
        "{}",
        config_yml(dir.path())
    );
    assert!(stdout.contains("8888"), "{stdout}");
    assert!(
        !stdout.contains("5511999998888") && !stderr.contains("5511999998888"),
        "numero inteiro nunca vai para a tela:\n{stdout}\n{stderr}"
    );
}

#[test]
fn allow_with_an_invalid_number_exits_65() {
    let dir = tempdir().expect("tempdir");
    // Sem `+`, o DDD passaria por codigo do pais (#1345, review WHATSAPP-11).
    for numero in [
        "abc",
        "011999998888",
        "5511999998888@s.whatsapp.net",
        "11 99999-8888",
        "(11) 99999-8888",
        "5511999998888",
    ] {
        let out = garra_env(dir.path(), &["whatsapp", "allow", numero], &[]);
        assert_eq!(out.status.code(), Some(65), "{numero}");
    }
    assert!(!config_yml(dir.path()).contains("allow"));
}

/// Um LID (`<id>@lid`) e gravado como veio, e na tela so o final (#1345).
#[test]
fn allow_accepts_a_lid_and_prints_only_its_last_digits() {
    let dir = tempdir().expect("tempdir");
    let out = garra_env(
        dir.path(),
        &["whatsapp", "allow", "87654321098765@lid"],
        &[],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{stdout}");
    assert!(config_yml(dir.path()).contains("87654321098765@lid"));
    assert!(
        stdout.contains("LID") && stdout.contains("8765"),
        "{stdout}"
    );
    assert!(!stdout.contains("87654321098765"), "{stdout}");
}

/// `--owner` em `standard` sai 64 (mesmo com `--yes`); em `isolated-pod`
/// sem `--yes` tambem, porque num pipe nao ha a quem perguntar.
#[test]
fn allow_owner_needs_isolated_pod_and_yes_in_a_pipe() {
    let dir = tempdir().expect("tempdir");
    let out = garra_env(
        dir.path(),
        &["whatsapp", "allow", "+5511999998888", "--owner", "--yes"],
        &[],
    );
    assert_eq!(out.status.code(), Some(64), "standard recusa --owner");
    assert!(!config_yml(dir.path()).contains("owners"));

    let pod = [("GARRAIA_EXECUTION_PROFILE", "isolated-pod")];
    let out = garra_env(
        dir.path(),
        &["whatsapp", "allow", "+5511999998888", "--owner"],
        &pod,
    );
    assert_eq!(out.status.code(), Some(64), "pod sem --yes num pipe");
    assert!(!config_yml(dir.path()).contains("owners"));

    let out = garra_env(
        dir.path(),
        &["whatsapp", "allow", "+5511999998888", "--owner", "--yes"],
        &pod,
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(config_yml(dir.path()).contains("owners"));
    assert!(
        !config_yml(dir.path()).contains("isolated-pod"),
        "o perfil da env nao vai para o arquivo"
    );
}

/// `link --allow` pre-responde a pergunta, mas o QR continua precisando de
/// terminal: num pipe e 69 com o texto do `ssh -t`, e nada e gravado.
#[test]
fn link_with_allow_in_a_pipe_still_needs_a_terminal() {
    let dir = tempdir().expect("tempdir");
    let out = garra_env(
        dir.path(),
        &["whatsapp", "link", "--allow", "+5511999998888"],
        &[],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(69), "{stdout}");
    assert!(stdout.contains("ssh -t"), "{stdout}");
    assert!(!config_yml(dir.path()).contains("5511999998888"));
}

// ---------------------------------------------------------------------------
// #1393/#1394/#1389: listar, remover e o curinga, no binario de verdade
// ---------------------------------------------------------------------------

/// `users` num pipe: papel e os quatro ultimos digitos, nunca o numero
/// inteiro — e o `--json` e um documento que um `jq` le direto.
#[test]
fn users_lists_the_gate_without_printing_a_full_number() {
    let dir = tempdir().expect("tempdir");
    let allow = garra_env(dir.path(), &["whatsapp", "allow", "+55 11 99999-8888"], &[]);
    assert_eq!(allow.status.code(), Some(0));

    let out = garra_env(dir.path(), &["whatsapp", "users"], &[]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "{stdout}\n{stderr}");
    assert!(stdout.contains("Autorizados: 1"), "{stdout}");
    assert!(stdout.contains("8888"), "{stdout}");
    assert!(
        !stdout.contains("5511999998888") && !stderr.contains("5511999998888"),
        "numero inteiro nunca vai para a tela:\n{stdout}\n{stderr}"
    );

    let out = garra_env(dir.path(), &["whatsapp", "users", "--json"], &[]);
    assert_eq!(out.status.code(), Some(0));
    let doc: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("o --json e um documento so");
    assert_eq!(doc["authorized"], 1);
    assert_eq!(doc["users"][0]["role"], "allow");
    assert_eq!(doc["users"][0]["last4"], "8888");
    assert!(
        !doc.to_string().contains("5511999998888"),
        "nem o JSON leva a identidade inteira"
    );
}

/// `remove` num pipe tira o numero e e idempotente; remover um DONO exige
/// `--yes`, e sem ele a config fica intacta.
#[test]
fn remove_revokes_a_number_and_never_drops_an_owner_silently() {
    let dir = tempdir().expect("tempdir");
    assert_eq!(
        garra_env(dir.path(), &["whatsapp", "allow", "+5511999998888"], &[])
            .status
            .code(),
        Some(0)
    );
    let out = garra_env(
        dir.path(),
        &["whatsapp", "remove", "+55 11 99999-8888"],
        &[],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{stdout}");
    assert!(stdout.contains("8888"), "{stdout}");
    assert!(!config_yml(dir.path()).contains("5511999998888"), "saiu");
    // De novo: quem nao estava na lista nao e erro.
    assert_eq!(
        garra_env(dir.path(), &["whatsapp", "remove", "+5511999998888"], &[])
            .status
            .code(),
        Some(0)
    );

    let pod = [("GARRAIA_EXECUTION_PROFILE", "isolated-pod")];
    assert_eq!(
        garra_env(
            dir.path(),
            &["whatsapp", "allow", "+5511999998888", "--owner", "--yes"],
            &pod,
        )
        .status
        .code(),
        Some(0)
    );
    let out = garra_env(dir.path(), &["whatsapp", "remove", "+5511999998888"], &pod);
    assert_eq!(
        out.status.code(),
        Some(64),
        "dono num pipe precisa de --yes"
    );
    assert!(config_yml(dir.path()).contains("5511999998888"), "intacto");

    let out = garra_env(
        dir.path(),
        &["whatsapp", "remove", "+5511999998888", "--yes"],
        &pod,
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!config_yml(dir.path()).contains("5511999998888"));
}

// ---------------------------------------------------------------------------
// #1395: promover e rebaixar, no binario de verdade
// ---------------------------------------------------------------------------

/// `owner` num pipe: recusado em `standard` (64), promove no pod com `--yes`
/// (0), e na tela so aparecem os quatro ultimos digitos.
#[test]
fn owner_is_refused_in_standard_and_promotes_inside_the_pod() {
    let dir = tempdir().expect("tempdir");
    let pod = [("GARRAIA_EXECUTION_PROFILE", "isolated-pod")];
    assert_eq!(
        garra_env(dir.path(), &["whatsapp", "allow", "+5511999998888"], &[])
            .status
            .code(),
        Some(0)
    );

    // `standard` (o default do `garra_env`): o perfil manda, mesmo com --yes.
    let out = garra_env(
        dir.path(),
        &["whatsapp", "owner", "+5511999998888", "--yes"],
        &[],
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(64), "{stderr}");
    assert!(stderr.contains("isolated-pod"), "{stderr}");
    assert!(!config_yml(dir.path()).contains("owners"), "nada gravado");

    // No pod, sem terminal e sem `--yes`: continua 64.
    let out = garra_env(dir.path(), &["whatsapp", "owner", "+5511999998888"], &pod);
    assert_eq!(
        out.status.code(),
        Some(64),
        "dono num pipe precisa de --yes"
    );
    assert!(!config_yml(dir.path()).contains("owners"));

    let out = garra_env(
        dir.path(),
        &["whatsapp", "owner", "+55 11 99999-8888", "--yes"],
        &pod,
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "{stdout}\n{stderr}");
    assert!(stdout.contains("8888"), "{stdout}");
    assert!(
        !stdout.contains("5511999998888") && !stderr.contains("5511999998888"),
        "numero inteiro nunca vai para a tela:\n{stdout}\n{stderr}"
    );
    assert!(config_yml(dir.path()).contains("owners"), "gravou");

    // O `users` reflete o papel novo, e promover de novo sai 0.
    let out = garra_env(dir.path(), &["whatsapp", "users", "--json"], &pod);
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(doc["owners"], 1);
    assert_eq!(doc["users"][0]["role"], "owners");
    assert_eq!(
        garra_env(
            dir.path(),
            &["whatsapp", "owner", "+5511999998888", "--yes"],
            &pod
        )
        .status
        .code(),
        Some(0),
        "promover duas vezes nao e erro"
    );
}

/// `unowner` tira o papel e **nunca** o acesso — e o ultimo dono exige
/// `--yes` num pipe. Rebaixar funciona tambem em `standard`, onde promover
/// nao funciona: um dono esquecido ali e justamente o que se quer limpar.
#[test]
fn unowner_demotes_without_ever_dropping_access() {
    let dir = tempdir().expect("tempdir");
    let pod = [("GARRAIA_EXECUTION_PROFILE", "isolated-pod")];
    for numero in ["+5511999998888", "+5511977776666"] {
        assert_eq!(
            garra_env(dir.path(), &["whatsapp", "owner", numero, "--yes"], &pod)
                .status
                .code(),
            Some(0),
            "{numero}"
        );
    }

    // Dois donos: rebaixar um nao precisa de confirmacao.
    let out = garra_env(dir.path(), &["whatsapp", "unowner", "+5511999998888"], &pod);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("8888"), "{stdout}");
    assert!(
        !stdout.contains("5511999998888"),
        "identidade inteira nunca vai para a tela:\n{stdout}"
    );

    // O acesso sobreviveu: ele e um `allow` agora, e o portao continua com 2.
    let out = garra_env(dir.path(), &["whatsapp", "users", "--json"], &pod);
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(doc["authorized"], 2, "ninguem perdeu acesso: {doc}");
    assert_eq!(doc["owners"], 1);
    assert!(
        !doc.to_string().contains("5511999998888"),
        "nem o JSON leva a identidade inteira"
    );

    // Sobrou um dono: num pipe, sem `--yes`, o comando para.
    let out = garra_env(dir.path(), &["whatsapp", "unowner", "+5511977776666"], &pod);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(64), "{stderr}");
    assert!(config_yml(dir.path()).contains("owners"), "intacto");

    // Com `--yes` sai — e rodando em `standard`, onde `owner` seria recusado.
    let out = garra_env(
        dir.path(),
        &["whatsapp", "unowner", "+5511977776666", "--yes"],
        &[],
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = garra_env(dir.path(), &["whatsapp", "users", "--json"], &[]);
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(doc["owners"], 0, "sem dono nenhum");
    assert_eq!(doc["authorized"], 2, "e com os dois ainda autorizados");
    for u in doc["users"].as_array().expect("array") {
        assert_eq!(u["role"], "allow", "{doc}");
    }

    // Idempotente: rebaixar quem nao e dono nao e erro.
    assert_eq!(
        garra_env(dir.path(), &["whatsapp", "unowner", "+5511977776666"], &[])
            .status
            .code(),
        Some(0)
    );
}

/// #1389: `allow '*'` continua recusado (65), mas a mensagem diz que "abrir
/// para todos" nao existe — e nao que o numero tem caractere invalido.
#[test]
fn allow_with_a_wildcard_says_the_feature_does_not_exist() {
    let dir = tempdir().expect("tempdir");
    let out = garra_env(dir.path(), &["whatsapp", "allow", "*"], &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(65), "{stderr}");
    assert!(stderr.contains("todo mundo"), "{stderr}");
    assert!(
        !stderr.contains("só pode ter dígitos"),
        "a frase generica de caractere nao serve aqui: {stderr}"
    );
    assert!(!config_yml(dir.path()).contains("allow"), "nada gravado");
}
