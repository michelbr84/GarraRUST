//! #1225 S2: o spawn das tools que executam um PROGRAMA (`run_tests`,
//! `git_diff`, `code_review`, `repo_search`) passa por aqui, e aqui a
//! `SandboxPolicy` e consultada — no host quando ela nao se aplica, dentro
//! do container quando se aplica, e nunca no host quando o sandbox foi
//! pedido e nao pode ser aplicado.
//!
//! Fica fora de `src/tools/` de proposito: o teste que varre as tools (em
//! `sandbox.rs`) classifica cada tool pelo que ELA chama, e este modulo nao
//! e uma tool.
//!
//! O que vale nos dois caminhos: `stdin` nulo (#1269/#1270), env do filho
//! reduzido a allowlist (#1075 R3), `kill_on_drop`, e o timeout da tool. No
//! caminho do container, o timeout tambem roda `<runtime> rm -f <nome>`:
//! matar o cliente do docker nao mata o container.

use std::path::Path;
use std::process::{Output, Stdio};
use std::time::Duration;

use tokio::process::Command;

use crate::sandbox::{SandboxPolicy, SandboxedArgv};

/// Tempo para o `rm -f` do container que estourou o timeout.
const TIMEOUT_DO_RM: Duration = Duration::from_secs(15);

/// O runtime (docker/podman) devolve 127 quando o programa pedido nao
/// existe na imagem.
const PROGRAMA_NAO_ENCONTRADO: i32 = 127;

/// O pedido de spawn de uma tool.
pub(crate) struct Pedido<'a> {
    /// Nome registrado da tool (a chave da policy).
    pub tool: &'a str,
    /// Programa (`git`, `cargo`, `rg`...). Constante do codigo.
    pub programa: &'a str,
    /// Argumentos, ja validados pela tool.
    pub args: &'a [String],
    /// Diretorio de trabalho; `None` herda o do processo.
    pub cwd: Option<&'a Path>,
    /// Variaveis extras (constantes do codigo, nunca valor do modelo).
    pub env: &'a [(&'a str, &'a str)],
    /// Timeout da tool.
    pub timeout: Duration,
}

/// O desfecho, com o motivo legivel para a tool devolver ao modelo.
#[derive(Debug)]
pub(crate) enum Desfecho {
    /// O programa rodou (no host ou no container) e terminou.
    Saida(Output),
    /// O programa nao pode ser executado: nao existe no host (erro de spawn)
    /// ou nao existe na imagem do sandbox (127 do runtime).
    NaoExecutou(String),
    /// O timeout da tool estourou (o container, se houver, foi removido).
    Timeout,
    /// O sandbox foi pedido e nao pode ser aplicado: nada rodou.
    Recusado(String),
}

/// Executa o pedido segundo a `policy`. Ver o doc do modulo.
pub(crate) async fn executar(policy: &SandboxPolicy, pedido: Pedido<'_>) -> Desfecho {
    match policy.wrap_argv(
        pedido.tool,
        pedido.programa,
        pedido.args,
        pedido.cwd,
        pedido.env,
    ) {
        Ok(None) => no_host(&pedido).await,
        Ok(Some(sb)) => no_container(&pedido, sb).await,
        Err(e) => {
            tracing::error!(tool = pedido.tool, "sandbox fail-closed: {e}");
            Desfecho::Recusado(format!(
                "`{}` bloqueado: sandbox obrigatorio nao pode ser aplicado. {e}",
                pedido.tool
            ))
        }
    }
}

fn filho(programa: &str) -> Command {
    let mut cmd = Command::new(programa);
    cmd.stdin(Stdio::null()).kill_on_drop(true);
    #[cfg(unix)]
    {
        cmd.env_clear();
        for (chave, valor) in garraia_common::safety_gate::allowed_child_env() {
            cmd.env(chave, valor);
        }
    }
    cmd
}

async fn no_host(pedido: &Pedido<'_>) -> Desfecho {
    let mut cmd = filho(pedido.programa);
    if let Some(dir) = pedido.cwd {
        cmd.current_dir(dir);
    }
    cmd.args(pedido.args);
    for (chave, valor) in pedido.env {
        cmd.env(chave, valor);
    }
    match tokio::time::timeout(pedido.timeout, cmd.output()).await {
        Ok(Ok(saida)) => Desfecho::Saida(saida),
        Ok(Err(e)) => Desfecho::NaoExecutou(format!("`{}`: {e}", pedido.programa)),
        Err(_) => Desfecho::Timeout,
    }
}

async fn no_container(pedido: &Pedido<'_>, sb: SandboxedArgv) -> Desfecho {
    let mut cmd = filho(&sb.runtime);
    cmd.args(&sb.argv);
    match tokio::time::timeout(pedido.timeout, cmd.output()).await {
        Ok(Ok(saida)) if saida.status.code() == Some(PROGRAMA_NAO_ENCONTRADO) => {
            Desfecho::NaoExecutou(format!(
                "o programa `{}` nao existe na imagem do sandbox (agent.sandbox.image) — use \
                 uma imagem com a toolchain, ou agent.sandbox.elevated = [\"{}\"] para rodar \
                 no host",
                pedido.programa, pedido.tool
            ))
        }
        Ok(Ok(saida)) => Desfecho::Saida(saida),
        Ok(Err(e)) => Desfecho::NaoExecutou(format!("`{}`: {e}", sb.runtime)),
        Err(_) => {
            remove_container(&sb).await;
            Desfecho::Timeout
        }
    }
}

/// `<runtime> rm -f <nome>`, em argv, com o mesmo env reduzido. Melhor
/// esforco: se falhar, o `--rm` do proprio container ainda o recolhe quando
/// o processo terminar.
async fn remove_container(sb: &SandboxedArgv) {
    remove_container_por_nome(&sb.runtime, &sb.nome_do_container).await;
}

/// [`remove_container`] a partir do par `(runtime, nome)` — o que o `bash`
/// recebe de `SandboxPolicy::wrap_command_nomeado` (review da #1272,
/// SANDBOX-6).
pub(crate) async fn remove_container_por_nome(runtime: &str, nome: &str) {
    let mut rm = filho(runtime);
    rm.args(["rm", "-f", nome])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    match tokio::time::timeout(TIMEOUT_DO_RM, rm.status()).await {
        Ok(Ok(_)) => {}
        _ => tracing::warn!(
            container = %nome,
            "sandbox: nao foi possivel remover o container depois do timeout"
        ),
    }
}

#[cfg(all(test, unix))]
pub(crate) mod testes {
    //! Runtime FALSO: um `docker` num PATH temporario que registra o argv
    //! que recebeu e se comporta como o teste manda. Serializado por um
    //! mutex porque mexe no `PATH` do processo.

    use super::*;
    use crate::{SandboxBackend, SandboxMode};
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    pub(crate) static TRAVA_DO_PATH: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    /// `docker` falso: grava cada invocacao (uma linha por argumento, e
    /// `---` no fim) em `<dir>/argv.log` e executa `corpo`.
    pub(crate) struct RuntimeFalso {
        pub dir: tempfile::TempDir,
        path_antigo: Option<std::ffi::OsString>,
    }

    impl RuntimeFalso {
        pub(crate) fn novo(corpo: &str) -> Self {
            let dir = tempfile::tempdir().expect("tmp");
            let log = dir.path().join("argv.log");
            let script = format!(
                "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\" >> '{}'; done\necho --- >> '{}'\n{corpo}\n",
                log.display(),
                log.display()
            );
            let p = dir.path().join("docker");
            std::fs::write(&p, script).expect("write");
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).expect("chmod");
            let path_antigo = std::env::var_os("PATH");
            let mut novo = std::ffi::OsString::from(dir.path());
            if let Some(antigo) = &path_antigo {
                novo.push(":");
                novo.push(antigo);
            }
            // SAFETY: serializado por TRAVA_DO_PATH em todo teste que usa isto.
            unsafe { std::env::set_var("PATH", novo) };
            Self { dir, path_antigo }
        }

        pub(crate) fn invocacoes(&self) -> Vec<Vec<String>> {
            let log = std::fs::read_to_string(self.dir.path().join("argv.log")).unwrap_or_default();
            log.split("---\n")
                .filter(|b| !b.is_empty())
                .map(|b| b.lines().map(str::to_string).collect())
                .collect()
        }
    }

    impl Drop for RuntimeFalso {
        fn drop(&mut self) {
            // SAFETY: idem.
            unsafe {
                match &self.path_antigo {
                    Some(p) => std::env::set_var("PATH", p),
                    None => std::env::remove_var("PATH"),
                }
            }
        }
    }

    pub(crate) fn policy_docker() -> SandboxPolicy {
        SandboxPolicy {
            mode: SandboxMode::All,
            backend: Some(SandboxBackend::Docker),
            ..SandboxPolicy::default()
        }
    }

    fn pedido<'a>(args: &'a [String], cwd: &'a Path, timeout: Duration) -> Pedido<'a> {
        Pedido {
            tool: "repo_search",
            programa: "rg",
            args,
            cwd: Some(cwd),
            env: &[("GIT_CONFIG_NOSYSTEM", "1")],
            timeout,
        }
    }

    #[tokio::test]
    async fn no_container_o_argv_leva_o_programa_e_os_args_literais() {
        let _t = TRAVA_DO_PATH.lock().await;
        let falso = RuntimeFalso::novo("echo saida-do-container");
        let dir = tempfile::tempdir().expect("tmp");
        let cwd: PathBuf = dir.path().canonicalize().expect("canon");
        let args = vec!["$(id)".to_string(), "-x; rm".to_string()];
        let d = executar(
            &policy_docker(),
            pedido(&args, &cwd, Duration::from_secs(10)),
        )
        .await;
        let Desfecho::Saida(saida) = d else {
            panic!("esperava Saida: {d:?}");
        };
        assert!(String::from_utf8_lossy(&saida.stdout).contains("saida-do-container"));
        let inv = falso.invocacoes();
        let argv = &inv[0];
        let fim = &argv[argv.len() - 4..];
        assert_eq!(fim, ["debian:bookworm-slim", "rg", "$(id)", "-x; rm"]);
        assert!(
            argv.contains(&"GIT_CONFIG_NOSYSTEM=1".to_string()),
            "{argv:?}"
        );
        assert!(argv.contains(&"--cap-drop".to_string()), "{argv:?}");
    }

    #[tokio::test]
    async fn exit_127_do_runtime_vira_mensagem_acionavel() {
        let _t = TRAVA_DO_PATH.lock().await;
        let _falso = RuntimeFalso::novo("exit 127");
        let dir = tempfile::tempdir().expect("tmp");
        let cwd = dir.path().canonicalize().expect("canon");
        let d = executar(&policy_docker(), pedido(&[], &cwd, Duration::from_secs(10))).await;
        let Desfecho::NaoExecutou(msg) = d else {
            panic!("esperava NaoExecutou: {d:?}");
        };
        assert!(msg.contains("nao existe na imagem"), "{msg}");
        assert!(msg.contains("agent.sandbox.elevated"), "{msg}");
    }

    #[tokio::test]
    async fn timeout_remove_o_container_pelo_nome() {
        let _t = TRAVA_DO_PATH.lock().await;
        // `rm` volta na hora; `run` dorme ate o timeout.
        let falso = RuntimeFalso::novo("[ \"$1\" = run ] && sleep 5; exit 0");
        let dir = tempfile::tempdir().expect("tmp");
        let cwd = dir.path().canonicalize().expect("canon");
        let d = executar(
            &policy_docker(),
            pedido(&[], &cwd, Duration::from_millis(300)),
        )
        .await;
        assert!(matches!(d, Desfecho::Timeout), "{d:?}");
        let inv = falso.invocacoes();
        let nome = inv[0]
            .iter()
            .skip_while(|a| *a != "--name")
            .nth(1)
            .cloned()
            .expect("--name");
        assert!(nome.starts_with("garra-sbx-"), "{nome}");
        assert!(
            inv.iter().any(|a| a == &["rm", "-f", nome.as_str()]),
            "o container nao foi removido: {inv:?}"
        );
    }

    #[tokio::test]
    async fn policy_off_roda_no_host_sem_tocar_no_runtime() {
        let _t = TRAVA_DO_PATH.lock().await;
        let falso = RuntimeFalso::novo("exit 0");
        let dir = tempfile::tempdir().expect("tmp");
        let cwd = dir.path().canonicalize().expect("canon");
        let args = vec!["host-ok".to_string()];
        let d = executar(
            &SandboxPolicy::default(),
            Pedido {
                programa: "echo",
                ..pedido(&args, &cwd, Duration::from_secs(10))
            },
        )
        .await;
        let Desfecho::Saida(s) = d else {
            panic!("{d:?}")
        };
        assert!(String::from_utf8_lossy(&s.stdout).contains("host-ok"));
        assert!(falso.invocacoes().is_empty(), "o runtime foi chamado");
    }

    #[tokio::test]
    async fn ssh_e_recusado_para_tool_de_workdir() {
        let dir = tempfile::tempdir().expect("tmp");
        let cwd = dir.path().canonicalize().expect("canon");
        let p = SandboxPolicy {
            backend: Some(SandboxBackend::Ssh("box".into())),
            network_disabled: false,
            mount_workdir: false,
            ..policy_docker()
        };
        let d = executar(&p, pedido(&[], &cwd, Duration::from_secs(5))).await;
        let Desfecho::Recusado(msg) = d else {
            panic!("{d:?}")
        };
        assert!(msg.contains("ssh"), "{msg}");
        assert!(msg.contains("docker"), "{msg}");
    }
}

/// #1225 S2b: as quatro tools, dirigidas pelo caminho do agente (`execute`),
/// contra o runtime falso. Com `mode = all` + docker o programa do HOST nao
/// roda — o runtime recebe o programa real no argv; com `elevated` ou `off`
/// o runtime nem e chamado; com `ssh` a tool recusa; e os gates proprios de
/// cada tool rodam ANTES do sandbox.
#[cfg(all(test, unix))]
mod testes_das_tools {
    use super::testes::{RuntimeFalso, TRAVA_DO_PATH, policy_docker};
    use crate::SandboxBackend;
    use crate::providers::{ContentBlock, LlmProvider, LlmRequest, LlmResponse, MessagePart};
    use crate::sandbox::SandboxPolicy;
    use crate::tools::repo_dir::contexto_de_teste as ctx;
    use crate::tools::{CodeReviewTool, GitDiffTool, RepoSearchTool, RunTestsTool, Tool};
    use std::sync::Arc;

    const SAIDA: &str = "echo saida-do-container";

    fn dir() -> (tempfile::TempDir, String) {
        let d = tempfile::tempdir().expect("tmp");
        let s = d
            .path()
            .canonicalize()
            .expect("canon")
            .to_string_lossy()
            .into_owned();
        (d, s)
    }

    /// O programa real chegou ao argv do runtime, depois da imagem.
    fn rodou_no_container(falso: &RuntimeFalso, programa: &str) {
        let inv = falso.invocacoes();
        assert!(!inv.is_empty(), "o runtime nao foi chamado");
        let argv = &inv[inv.len() - 1];
        let i = argv
            .iter()
            .position(|a| a == "debian:bookworm-slim")
            .expect("imagem no argv");
        assert_eq!(argv[i + 1], programa, "{argv:?}");
        assert_eq!(argv[0], "run");
    }

    fn elevada(tool: &str) -> SandboxPolicy {
        SandboxPolicy {
            elevated: vec![tool.to_string()],
            ..policy_docker()
        }
    }

    fn ssh() -> SandboxPolicy {
        SandboxPolicy {
            backend: Some(SandboxBackend::Ssh("box".into())),
            network_disabled: false,
            mount_workdir: false,
            ..policy_docker()
        }
    }

    #[tokio::test]
    async fn repo_search_roda_no_container_e_o_fallback_grep_tambem() {
        let _t = TRAVA_DO_PATH.lock().await;
        let (_g, wd) = dir();
        let falso = RuntimeFalso::novo(SAIDA);
        let tool = RepoSearchTool::new(Some(10), None).com_sandbox(policy_docker());
        let out = tool
            .execute(&ctx(Some(&wd)), serde_json::json!({"query": "x"}))
            .await
            .expect("execute");
        assert!(
            out.content.contains("saida-do-container"),
            "{}",
            out.content
        );
        rodou_no_container(&falso, "rg");
        drop(falso);

        // rg ausente na imagem (127): o grep roda NO CONTAINER, nunca no host.
        let falso = RuntimeFalso::novo(
            "for a in \"$@\"; do [ \"$a\" = rg ] && exit 127; done; echo achou-com-grep",
        );
        let out = tool
            .execute(&ctx(Some(&wd)), serde_json::json!({"query": "x"}))
            .await
            .expect("execute");
        assert!(out.content.contains("achou-com-grep"), "{}", out.content);
        rodou_no_container(&falso, "grep");
    }

    #[tokio::test]
    async fn run_tests_roda_no_container_depois_dos_gates() {
        let _t = TRAVA_DO_PATH.lock().await;
        let (_g, wd) = dir();
        let falso = RuntimeFalso::novo(SAIDA);
        let tool = RunTestsTool::new(Some(10)).com_sandbox(policy_docker());

        // Gate proprio primeiro: `-x` e recusado e o runtime nem e chamado.
        let out = tool
            .execute(
                &ctx(Some(&wd)),
                serde_json::json!({"framework": "cargo", "test_name": "--x"}),
            )
            .await
            .expect("execute");
        assert!(
            out.is_error && out.content.contains("test_name"),
            "{}",
            out.content
        );
        assert!(falso.invocacoes().is_empty(), "o gate nao veio antes");

        let out = tool
            .execute(&ctx(Some(&wd)), serde_json::json!({"framework": "cargo"}))
            .await
            .expect("execute");
        assert!(
            out.content.contains("saida-do-container"),
            "{}",
            out.content
        );
        rodou_no_container(&falso, "cargo");
    }

    #[tokio::test]
    async fn run_tests_com_confirmacao_pede_antes_de_qualquer_spawn() {
        let _t = TRAVA_DO_PATH.lock().await;
        let (_g, wd) = dir();
        let falso = RuntimeFalso::novo(SAIDA);
        let tool = RunTestsTool::new_with_confirmation(Some(10)).com_sandbox(policy_docker());
        let out = tool
            .execute(&ctx(Some(&wd)), serde_json::json!({"framework": "cargo"}))
            .await
            .expect("execute");
        assert!(out.requires_confirmation, "{out:?}");
        assert!(
            falso.invocacoes().is_empty(),
            "spawnou antes da confirmacao"
        );
    }

    #[tokio::test]
    async fn git_diff_roda_no_container_e_range_hostil_e_recusado_antes() {
        let _t = TRAVA_DO_PATH.lock().await;
        let (_g, wd) = dir();
        let falso = RuntimeFalso::novo(SAIDA);
        let tool = GitDiffTool::new(Some(10), None).com_sandbox(policy_docker());
        let out = tool
            .execute(
                &ctx(Some(&wd)),
                serde_json::json!({"operation": "diff", "from_commit": "--output=/x", "to_commit": "HEAD"}),
            )
            .await;
        // O range hostil sai como erro da tool, antes do runtime.
        match out {
            Ok(o) => assert!(o.is_error, "{o:?}"),
            Err(e) => assert!(e.to_string().contains("recusada"), "{e}"),
        }
        assert!(falso.invocacoes().is_empty(), "o gate nao veio antes");

        let out = tool
            .execute(&ctx(Some(&wd)), serde_json::json!({"operation": "diff"}))
            .await
            .expect("execute");
        assert!(
            out.content.contains("saida-do-container"),
            "{}",
            out.content
        );
        rodou_no_container(&falso, "git");
        let argv = falso.invocacoes().pop().expect("argv");
        assert!(
            argv.contains(&"core.fsmonitor=false".to_string()),
            "{argv:?}"
        );
        assert!(
            argv.contains(&"GIT_CONFIG_NOSYSTEM=1".to_string()),
            "{argv:?}"
        );
    }

    struct Eco;

    #[async_trait::async_trait]
    impl LlmProvider for Eco {
        fn provider_id(&self) -> &str {
            "eco"
        }
        async fn complete(&self, request: &LlmRequest) -> garraia_common::Result<LlmResponse> {
            let texto = request
                .messages
                .iter()
                .map(|m| match &m.content {
                    MessagePart::Text(t) => t.clone(),
                    MessagePart::Parts(_) => String::new(),
                })
                .collect::<Vec<_>>()
                .join("\n");
            Ok(LlmResponse {
                content: vec![ContentBlock::Text { text: texto }],
                model: "eco".into(),
                stop_reason: None,
                usage: None,
            })
        }
        async fn health_check(&self) -> garraia_common::Result<bool> {
            Ok(true)
        }
    }

    #[tokio::test]
    async fn code_review_roda_o_git_no_container() {
        let _t = TRAVA_DO_PATH.lock().await;
        let (_g, wd) = dir();
        let falso = RuntimeFalso::novo(SAIDA);
        let tool = CodeReviewTool::new(Arc::new(Eco), "m", Some(10)).com_sandbox(policy_docker());
        let out = tool
            .execute(&ctx(Some(&wd)), serde_json::json!({}))
            .await
            .expect("execute");
        assert!(
            out.content.contains("saida-do-container"),
            "{}",
            out.content
        );
        rodou_no_container(&falso, "git");
    }

    /// Review da #1272 (SANDBOX-6): o `bash` sandboxado que estoura o
    /// timeout remove o container pelo nome — como as tools de argv —, em vez
    /// de deixa-lo rodando com o workdir montado rw.
    #[tokio::test]
    async fn bash_no_timeout_remove_o_container_pelo_nome() {
        let _t = TRAVA_DO_PATH.lock().await;
        let (_g, wd) = dir();
        let falso = RuntimeFalso::novo("[ \"$1\" = run ] && sleep 30; exit 0");
        let mut tool = crate::tools::BashTool::new(Some(1));
        tool.set_sandbox_policy(policy_docker());
        let out = tool
            .execute(&ctx(Some(&wd)), serde_json::json!({"command": "echo oi"}))
            .await
            .expect("execute");
        assert!(
            out.is_error && out.content.contains("tempo limite"),
            "{}",
            out.content
        );
        let inv = falso.invocacoes();
        let nome = inv
            .first()
            .and_then(|a| a.iter().skip_while(|x| *x != "--name").nth(1).cloned())
            .expect("--name no docker run");
        assert!(nome.starts_with("garra-sbx-"), "{nome}");
        assert!(
            inv.iter().any(|a| a == &["rm", "-f", nome.as_str()]),
            "o container nao foi removido: {inv:?}"
        );
    }

    /// `elevated` e `off` nao tocam no runtime (regressao zero para quem
    /// nao liga o sandbox); `ssh` recusa sem rodar nada.
    #[tokio::test]
    async fn elevated_off_e_ssh() {
        let _t = TRAVA_DO_PATH.lock().await;
        let (_g, wd) = dir();
        std::fs::write(std::path::Path::new(&wd).join("a.txt"), "agulha-no-host\n").expect("w");
        let falso = RuntimeFalso::novo("exit 0");
        for policy in [elevada("repo_search"), SandboxPolicy::default()] {
            let out = RepoSearchTool::new(Some(10), None)
                .com_sandbox(policy)
                .execute(
                    &ctx(Some(&wd)),
                    serde_json::json!({"query": "agulha-no-host"}),
                )
                .await
                .expect("execute");
            assert!(out.content.contains("a.txt"), "{}", out.content);
        }
        assert!(
            falso.invocacoes().is_empty(),
            "elevated/off chamou o runtime"
        );

        let out = RepoSearchTool::new(Some(10), None)
            .com_sandbox(ssh())
            .execute(
                &ctx(Some(&wd)),
                serde_json::json!({"query": "agulha-no-host"}),
            )
            .await
            .expect("execute");
        assert!(out.is_error, "{}", out.content);
        assert!(out.content.contains("ssh"), "{}", out.content);
        assert!(
            !out.content.contains("a.txt"),
            "rodou no host: {}",
            out.content
        );
    }
}
