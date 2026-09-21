//! Sandbox por tool contra um Docker REAL (#1225 S4).
//!
//! Os testes unitarios de `sandbox.rs` e `bash_tool.rs` provam a STRING que
//! `wrap_command` monta (`--network none`, `no-new-privileges`, o mount
//! `-v cwd:cwd -w cwd`) e os caminhos fail-closed. Nenhum deles sobe um
//! container — entao um Docker que ignorasse uma flag, ou um `wrap_command`
//! que devolvesse `Ok(None)` por engano e deixasse o comando rodar no host,
//! manteria a suite inteira verde. Este arquivo fecha esse buraco: dirige a
//! `BashTool` com uma policy Docker de verdade e afirma o que acontece
//! DENTRO do container.
//!
//! Anti-vacuidade, porque e o defeito que este teste existe para pegar:
//!
//! * Todo comando imprime um marcador que so existe dentro do container
//!   (`/.dockerenv`). Um `echo` que rodasse no host tambem imprimiria
//!   `garra-sandbox-ok` — sem o marcador o teste passaria pelo motivo errado.
//! * A sonda de rede distingue "sem `getent` na imagem" de "sem rede": se a
//!   imagem default mudasse para uma sem `getent`, `getent ... || echo
//!   NO_NETWORK` daria NO_NETWORK em qualquer container, com ou sem rede.
//! * O controle com `network_disabled = false` prova que a MESMA sonda
//!   resolve quando a rede e permitida — ou seja, que o `NO_NETWORK` do
//!   teste principal veio da flag, e nao de um `getent` quebrado.
//! * O mount tem contraprova: com `mount_workdir = false` o arquivo do cwd
//!   deixa de ser visivel.
//!
//! Padrao do `crates/garraia-storage/tests/s3_integration.rs`: sem daemon
//! alcancavel os testes imprimem `[skip]` e voltam; `GARRAIA_REQUIRE_DOCKER=1`
//! (que o CI liga no runner Linux) transforma esse skip em falha, para que um
//! verde signifique que o container rodou mesmo. A sonda de disponibilidade
//! roda com o MESMO ambiente esvaziado que a tool aplica ao filho (#1075 R3:
//! so `PATH`/`HOME`/`LANG`/`LC_ALL`/`TERM`/`USER`), porque e assim que o
//! `docker` vai ser chamado de verdade — um daemon so alcancavel via
//! `DOCKER_HOST` no ambiente do pai nao conta como disponivel para a tool.
//!
//! A imagem e a default da policy (`debian:bookworm-slim`) e o pull e
//! implicito na primeira execucao, por isso o timeout generoso.
#![cfg(unix)]

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use garraia_agents::tools::approval::ToolApproval;
use garraia_agents::tools::{BashTool, Tool, ToolContext, ToolOutput};
use garraia_agents::{SandboxBackend, SandboxMode, SandboxPolicy};

/// Timeout da propria `BashTool`: a primeira execucao pode puxar a imagem.
const TIMEOUT_TOOL_SEGS: u64 = 120;
/// Cerca externa do teste, maior que a da tool de proposito: se a tool
/// estourar, e a mensagem DELA que aparece, nao um panic de timeout opaco.
const TIMEOUT_TESTE: Duration = Duration::from_secs(150);
/// `docker info` e barato; se nao responder nisto o daemon nao esta de pe.
const TIMEOUT_SONDA: Duration = Duration::from_secs(20);

/// Marcador que so existe dentro de um container Docker. Todo comando o
/// imprime, para que um comando que rodasse no host nao passasse.
const PROVA_DE_CONTAINER: &str = "test -f /.dockerenv && echo IN_CONTAINER || echo ON_HOST";

/// Sonda de rede com tres saidas distintas — ver o comentario do modulo.
const SONDA_DE_REDE: &str = "command -v getent >/dev/null 2>&1 || { echo NO_GETENT; exit 0; }; \
                             getent hosts example.com >/dev/null 2>&1 && echo RESOLVED || echo NO_NETWORK";

fn policy(network_disabled: bool, mount_workdir: bool) -> SandboxPolicy {
    SandboxPolicy {
        mode: SandboxMode::All,
        backend: Some(SandboxBackend::Docker),
        network_disabled,
        mount_workdir,
        // `image` fica no default da policy de proposito: e a imagem que um
        // operador recebe ao ligar o sandbox sem configurar nada.
        ..SandboxPolicy::default()
    }
}

/// O daemon responde ao `docker` chamado como a tool o chama?
///
/// Mesmo `env_clear` + allowlist R3 do `bash_tool.rs`, e nao o ambiente do
/// processo de teste: a pergunta e se a TOOL alcanca o daemon.
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
        Ok(Ok(saida)) => Err(format!(
            "`docker info` saiu com {}: {}",
            saida.status,
            String::from_utf8_lossy(&saida.stderr).trim()
        )),
        Ok(Err(e)) => Err(format!("nao foi possivel executar `docker`: {e}")),
        Err(_) => Err(format!(
            "`docker info` nao respondeu em {}s",
            TIMEOUT_SONDA.as_secs()
        )),
    }
}

/// `BashTool` com a policy Docker, ou `None` para o teste se pular.
///
/// O `None` e o skip do padrao `s3_integration.rs`; com
/// `GARRAIA_REQUIRE_DOCKER` setado ele vira falha, porque onde o Docker E
/// esperado um teste que se pula sozinho e um teste que mente.
async fn tool_docker(network_disabled: bool, mount_workdir: bool) -> Option<BashTool> {
    if let Err(motivo) = docker_disponivel().await {
        assert!(
            std::env::var_os("GARRAIA_REQUIRE_DOCKER").is_none(),
            "GARRAIA_REQUIRE_DOCKER esta setado, mas o daemon Docker nao respondeu \
             ao `docker` chamado como a BashTool o chama: {motivo}"
        );
        eprintln!(
            "[skip] Docker indisponivel ({motivo}); teste de integracao do sandbox \
             (#1225 S4) pulado"
        );
        return None;
    }
    let mut tool = BashTool::new(Some(TIMEOUT_TOOL_SEGS));
    tool.set_sandbox_policy(policy(network_disabled, mount_workdir));
    Some(tool)
}

/// Diretorio de trabalho descartavel, canonico e absoluto.
///
/// Sempre ha um `working_dir`: o `wrap_command` monta o que receber, e
/// `ToolContext { working_dir: None }` vira `-v .:. -w .`, que o Docker
/// recusa ("needs to be an absolute path"). O runtime real sempre passa a
/// raiz do projeto, entao o teste faz o mesmo. Canonico porque em alguns
/// hosts `/tmp` e symlink e o bind mount precisa do caminho resolvido.
fn workdir_de_teste() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let canonico = dir.path().canonicalize().expect("canonicalize tempdir");
    (dir, canonico)
}

fn ctx(working_dir: &std::path::Path) -> ToolContext {
    ToolContext {
        session_id: "sandbox-docker-test".to_string(),
        user_id: None,
        is_heartbeat: false,
        approval: ToolApproval::None,
        working_dir: Some(working_dir.to_string_lossy().into_owned()),
        project_id: None,
    }
}

async fn executar(tool: &BashTool, working_dir: &std::path::Path, comando: &str) -> ToolOutput {
    let contexto = ctx(working_dir);
    let chamada = tool.execute(&contexto, serde_json::json!({ "command": comando }));
    match tokio::time::timeout(TIMEOUT_TESTE, chamada).await {
        Ok(Ok(saida)) => saida,
        Ok(Err(e)) => panic!("BashTool::execute devolveu Err para `{comando}`: {e}"),
        Err(_) => panic!(
            "`{comando}` nao terminou em {}s (a tool tem timeout proprio de {}s, \
             entao isto e um travamento fora dela)",
            TIMEOUT_TESTE.as_secs(),
            TIMEOUT_TOOL_SEGS
        ),
    }
}

/// O caminho feliz: o comando roda, a saida volta, e veio de DENTRO do
/// container. A primeira execucao pode trazer o log do pull no `STDERR:`,
/// por isso `contains` e nao igualdade.
#[tokio::test(flavor = "multi_thread")]
async fn echo_sai_de_dentro_do_container() {
    let Some(tool) = tool_docker(true, true).await else {
        return;
    };
    let (_guarda, dir) = workdir_de_teste();
    let saida = executar(
        &tool,
        &dir,
        &format!("echo garra-sandbox-ok; {PROVA_DE_CONTAINER}"),
    )
    .await;
    assert!(
        !saida.is_error,
        "comando deveria ter sucesso: {}",
        saida.content
    );
    assert!(
        saida.content.contains("garra-sandbox-ok"),
        "saida do echo nao chegou: {}",
        saida.content
    );
    assert!(
        saida.content.contains("IN_CONTAINER") && !saida.content.contains("ON_HOST"),
        "o comando rodou no HOST, nao no container: {}",
        saida.content
    );
}

/// `--network none` EFETIVO: dentro do container `getent hosts example.com`
/// falha. `NO_GETENT` e `RESOLVED` sao afirmados ausentes de proposito —
/// ver o comentario do modulo sobre vacuidade.
#[tokio::test(flavor = "multi_thread")]
async fn network_disabled_deixa_o_container_sem_rede() {
    let Some(tool) = tool_docker(true, true).await else {
        return;
    };
    let (_guarda, dir) = workdir_de_teste();
    let saida = executar(
        &tool,
        &dir,
        &format!("{SONDA_DE_REDE}; {PROVA_DE_CONTAINER}"),
    )
    .await;
    assert!(
        !saida.is_error,
        "sonda deveria ter sucesso: {}",
        saida.content
    );
    assert!(
        saida.content.contains("IN_CONTAINER"),
        "a sonda rodou no HOST: {}",
        saida.content
    );
    assert!(
        !saida.content.contains("NO_GETENT"),
        "a imagem default nao tem `getent`; a sonda de rede ficou vazia: {}",
        saida.content
    );
    assert!(
        !saida.content.contains("RESOLVED"),
        "com network_disabled = true o container RESOLVEU example.com — \
         `--network none` nao esta sendo aplicado: {}",
        saida.content
    );
    assert!(
        saida.content.contains("NO_NETWORK"),
        "esperava NO_NETWORK da sonda: {}",
        saida.content
    );
}

/// CONTROLE: a mesma sonda, mesma imagem, mesma policy exceto
/// `network_disabled = false`, resolve. Prova que o `NO_NETWORK` do teste
/// acima veio da flag e nao de um `getent` quebrado ou de uma imagem sem
/// resolver.
///
/// Depende de o RUNNER ter rede. Sem rede no host o teste se pula com aviso
/// — nunca passa em silencio, nunca falha por um problema que nao e do
/// sandbox. `GARRAIA_REQUIRE_DOCKER` fala do Docker, nao da rede, entao nao
/// converte este skip em falha.
#[tokio::test(flavor = "multi_thread")]
async fn controle_com_rede_ligada_resolve() {
    let Some(tool) = tool_docker(false, true).await else {
        return;
    };
    let host_resolve = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::net::lookup_host("example.com:80"),
    )
    .await;
    let host_tem_rede = match host_resolve {
        Ok(Ok(mut enderecos)) => enderecos.next().is_some(),
        _ => false,
    };
    if !host_tem_rede {
        eprintln!(
            "[skip] o host nao resolve example.com; controle de rede do sandbox \
             (#1225 S4) pulado — o teste principal (`--network none`) continua valendo"
        );
        return;
    }
    let (_guarda, dir) = workdir_de_teste();
    let saida = executar(
        &tool,
        &dir,
        &format!("{SONDA_DE_REDE}; {PROVA_DE_CONTAINER}"),
    )
    .await;
    assert!(
        !saida.is_error,
        "sonda deveria ter sucesso: {}",
        saida.content
    );
    assert!(
        saida.content.contains("IN_CONTAINER"),
        "a sonda rodou no HOST: {}",
        saida.content
    );
    assert!(
        !saida.content.contains("NO_GETENT"),
        "a imagem default nao tem `getent`: {}",
        saida.content
    );
    assert!(
        saida.content.contains("RESOLVED"),
        "com network_disabled = false e host com rede, o container deveria resolver \
         example.com; se isto falhar, o teste `--network none` pode estar passando \
         por vacuidade: {}",
        saida.content
    );
}

/// `mount_workdir`: um arquivo criado no cwd do HOST e lido por `cat` dentro
/// do container, por caminho RELATIVO — o que prova o `-v cwd:cwd` e o
/// `-w cwd` juntos. O `pwd` confirma que o cwd do container e o mesmo path
/// do host, que e o que mantem caminhos relativos do modelo funcionando.
#[tokio::test(flavor = "multi_thread")]
async fn mount_workdir_expoe_o_cwd_do_host_dentro_do_container() {
    let Some(tool) = tool_docker(true, true).await else {
        return;
    };
    let (_guarda, dir) = workdir_de_teste();
    let conteudo = format!("garra-mount-probe-{}", std::process::id());
    std::fs::write(dir.join("probe.txt"), format!("{conteudo}\n")).expect("escreve probe.txt");

    let saida = executar(
        &tool,
        &dir,
        &format!("pwd; cat probe.txt; {PROVA_DE_CONTAINER}"),
    )
    .await;
    assert!(
        !saida.is_error,
        "cat deveria ter sucesso: {}",
        saida.content
    );
    assert!(
        saida.content.contains("IN_CONTAINER"),
        "o cat rodou no HOST: {}",
        saida.content
    );
    assert!(
        saida.content.contains(&conteudo),
        "o arquivo do cwd do host nao apareceu dentro do container: {}",
        saida.content
    );
    assert!(
        saida.content.contains(&dir.to_string_lossy().into_owned()),
        "o cwd dentro do container deveria ser o mesmo path do host ({}): {}",
        dir.display(),
        saida.content
    );
}

/// Contraprova do mount: com `mount_workdir = false` o mesmo `cat` nao acha
/// o arquivo. Sem isto, o teste acima passaria se o comando rodasse no host
/// por engano — o marcador `/.dockerenv` ja cobre esse caso, mas esta e a
/// prova de que e a FLAG que traz o arquivo, nao outra coisa.
#[tokio::test(flavor = "multi_thread")]
async fn sem_mount_workdir_o_cwd_do_host_nao_aparece() {
    let Some(tool) = tool_docker(true, false).await else {
        return;
    };
    let (_guarda, dir) = workdir_de_teste();
    let conteudo = format!("garra-no-mount-probe-{}", std::process::id());
    std::fs::write(dir.join("probe.txt"), format!("{conteudo}\n")).expect("escreve probe.txt");

    let saida = executar(&tool, &dir, &format!("{PROVA_DE_CONTAINER}; cat probe.txt")).await;
    assert!(
        saida.content.contains("IN_CONTAINER"),
        "o comando rodou no HOST: {}",
        saida.content
    );
    assert!(
        saida.is_error,
        "sem mount o `cat probe.txt` deveria falhar dentro do container: {}",
        saida.content
    );
    assert!(
        !saida.content.contains(&conteudo),
        "o arquivo do host vazou para o container sem mount_workdir: {}",
        saida.content
    );
}
