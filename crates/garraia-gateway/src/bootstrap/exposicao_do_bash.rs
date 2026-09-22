//! #1272: quando a tool `bash` existe numa superficie **sem humano no laco**.
//!
//! O `garraia mcp-server` (tool `garra_agent`) e o runtime do gateway (canais
//! remotos, `/mode code`) registravam um `bash` irrestrito em qualquer perfil.
//! O tier arriscado do `safety_gate` so pega o que parece perigoso: um
//! `cat /etc/shadow` ou um `echo x > /qualquer/lugar` passava e rodava no
//! host. Uma lista negra textual de comandos nao e fronteira — redirecao,
//! subshell, `tee`, pipe, filho em background e symlink contornam qualquer
//! lista — entao a regra aqui e de **isolamento de processo**, nao de texto:
//!
//! 1. [`ExposicaoDoBash::Sandbox`]: a policy `agent.sandbox` exige sandbox
//!    para `bash` (modo `all` sem `bash` em `elevated`, ou `allowlist` com
//!    `bash`), o backend e `docker` ou `podman`, a plataforma e unix e o
//!    binario existe. O `bash` e registrado e TODO comando passa por
//!    `wrap_command`, que continua fail-closed em runtime (daemon parado =
//!    comando recusado). Nunca ha fallback para o host.
//! 2. [`ExposicaoDoBash::HostDoPod`]: o operador declarou
//!    `execution.profile = isolated-pod` (arquivo ou
//!    `GARRAIA_EXECUTION_PROFILE`) e o caso 1 nao se aplica. O `bash` roda no
//!    host **do pod**, com a denylist do `safety_gate` e o tier arriscado
//!    intactos — o perfil libera tools, nao desliga protecoes (ADR 0024).
//! 3. [`ExposicaoDoBash::Desligado`]: todo o resto em `standard`. O `bash`
//!    NAO e registrado. Nao registrar (em vez de registrar e negar) e a menor
//!    superficie possivel, e como o system prompt e gerado das tools
//!    registradas, o modelo nunca ouve que tem um shell.
//!
//! `ssh` nunca conta como sandbox: e execucao remota, sem isolamento de rede
//! nem de mount (#1225 S3).
//!
//! # O que este modulo nunca faz
//!
//! Decidir o perfil sozinho. O perfil vem de `execution.profile` ja resolvido
//! pelo loader; nada aqui procura marcador de runtime de container. Um teste
//! varre este arquivo atras desses literais, como o de `execution.rs`.
//!
//! `garraia chat` fica de fora de proposito: la o `BashTool` tem canal de
//! confirmacao e o principal e o humano no terminal.

use garraia_agents::sandbox::{SandboxBackend, SandboxPolicy};
use garraia_config::ExecutionProfile;
use tracing::{info, warn};

/// Nome da tool no registry.
const BASH: &str = "bash";

/// Por que o `bash` ficou fora do registry em `standard`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotivoDoBashDesligado {
    /// `agent.sandbox.mode = off` (o default).
    SandboxDesligado,
    /// `mode = all` com `bash` em `agent.sandbox.elevated`.
    BashElevado,
    /// `mode = allowlist` sem `bash` em `sandboxed_tools`.
    BashForaDaAllowlist,
    /// Sandbox ligado sem `agent.sandbox.backend`.
    SemBackend,
    /// `backend = ssh`: execucao remota, nao isolamento.
    BackendSsh,
    /// `docker`/`podman` configurado e o binario nao existe no host.
    BackendIndisponivel,
    /// O wrap so e suportado em unix (#1225 F2).
    PlataformaNaoUnix,
}

impl MotivoDoBashDesligado {
    /// Frase curta, sem valor de config nenhum (nada de host, imagem, path).
    pub fn descricao(self) -> &'static str {
        match self {
            Self::SandboxDesligado => "agent.sandbox.mode = off",
            Self::BashElevado => "bash esta em agent.sandbox.elevated",
            Self::BashForaDaAllowlist => {
                "agent.sandbox.mode = allowlist sem bash em sandboxed_tools"
            }
            Self::SemBackend => "agent.sandbox sem backend",
            Self::BackendSsh => "agent.sandbox.backend = ssh e execucao remota, nao isolamento",
            Self::BackendIndisponivel => "o binario do backend (docker/podman) nao existe no host",
            Self::PlataformaNaoUnix => "sandbox so e suportado em unix",
        }
    }
}

/// O desfecho para a tool `bash` numa superficie sem humano no laco.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExposicaoDoBash {
    /// Registrada; todo comando dentro do container do `backend`.
    Sandbox {
        /// `Docker` ou `Podman` — nunca `Ssh`.
        backend: SandboxBackend,
    },
    /// Registrada no host do pod (`execution.profile = isolated-pod`).
    HostDoPod,
    /// Nao registrada.
    Desligado {
        /// O primeiro motivo que impediu o caso `Sandbox`.
        motivo: MotivoDoBashDesligado,
    },
}

/// O passo acionavel quando o `bash` esta desligado. Sem valor de config.
pub const COMO_LIGAR_O_BASH: &str = "para ter bash: agent.sandbox { mode: all, backend: docker } \
     (ou podman) com o binario instalado; ou execution.profile = isolated-pod se este processo \
     roda mesmo num pod descartavel (#1272)";

impl ExposicaoDoBash {
    /// A tool `bash` entra no registry?
    pub fn registra_bash(&self) -> bool {
        !matches!(self, Self::Desligado { .. })
    }

    /// Uma linha para humano (log, diagnostico, system prompt). Sem valor de
    /// config: nem imagem, nem host, nem caminho.
    pub fn descricao(&self) -> String {
        match self {
            Self::Sandbox { backend } => format!(
                "bash ligado, cada comando dentro de um container {} (agent.sandbox)",
                backend.binary()
            ),
            Self::HostDoPod => {
                "bash ligado no host do pod (execution.profile = isolated-pod); denylist e \
                 tier arriscado continuam valendo"
                    .to_string()
            }
            Self::Desligado { motivo } => format!(
                "bash DESLIGADO em execution.profile = standard: {}",
                motivo.descricao()
            ),
        }
    }
}

/// A decisao, pura: perfil, policy, plataforma e disponibilidade injetados.
pub fn decidir_exposicao_do_bash(
    perfil: ExecutionProfile,
    policy: &SandboxPolicy,
    alvo_unix: bool,
    disponivel: impl Fn(&SandboxBackend) -> bool,
) -> ExposicaoDoBash {
    let sandbox = motivo_sem_sandbox(policy, alvo_unix, &disponivel);
    match sandbox {
        Ok(backend) => ExposicaoDoBash::Sandbox { backend },
        Err(_) if perfil.is_isolated_pod() => ExposicaoDoBash::HostDoPod,
        Err(motivo) => ExposicaoDoBash::Desligado { motivo },
    }
}

/// `Ok(backend)` quando o sandbox do `bash` e utilizavel; senao o motivo.
fn motivo_sem_sandbox(
    policy: &SandboxPolicy,
    alvo_unix: bool,
    disponivel: &impl Fn(&SandboxBackend) -> bool,
) -> Result<SandboxBackend, MotivoDoBashDesligado> {
    use garraia_agents::SandboxMode;
    if !policy.requires_sandbox(BASH) {
        return Err(match policy.mode {
            SandboxMode::Off => MotivoDoBashDesligado::SandboxDesligado,
            SandboxMode::All => MotivoDoBashDesligado::BashElevado,
            SandboxMode::Allowlist => MotivoDoBashDesligado::BashForaDaAllowlist,
        });
    }
    let backend = policy
        .backend
        .as_ref()
        .ok_or(MotivoDoBashDesligado::SemBackend)?;
    if matches!(backend, SandboxBackend::Ssh(_)) {
        return Err(MotivoDoBashDesligado::BackendSsh);
    }
    if !alvo_unix {
        return Err(MotivoDoBashDesligado::PlataformaNaoUnix);
    }
    if !disponivel(backend) {
        return Err(MotivoDoBashDesligado::BackendIndisponivel);
    }
    Ok(backend.clone())
}

/// [`decidir_exposicao_do_bash`] com a plataforma real e a sonda real do
/// binario (`SandboxBackend::is_available`).
pub fn exposicao_do_bash(perfil: ExecutionProfile, policy: &SandboxPolicy) -> ExposicaoDoBash {
    decidir_exposicao_do_bash(perfil, policy, cfg!(unix), SandboxBackend::is_available)
}

/// Anuncia a decisao uma vez por subida. `Desligado` e o unico `warn!`, e ele
/// diz por que e como ligar. `superficie` e um nome fixo do codigo
/// (`"gateway"`, `"mcp-server"`), nunca entrada de usuario.
pub fn anuncia_exposicao_do_bash(superficie: &'static str, exposicao: &ExposicaoDoBash) {
    match exposicao {
        ExposicaoDoBash::Desligado { .. } => warn!(
            superficie,
            "{}; a tool bash NAO foi registrada. {COMO_LIGAR_O_BASH}",
            exposicao.descricao()
        ),
        _ => info!(superficie, "{}", exposicao.descricao()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use garraia_agents::SandboxMode;

    fn policy(mode: SandboxMode, backend: Option<SandboxBackend>) -> SandboxPolicy {
        SandboxPolicy {
            mode,
            backend,
            ..SandboxPolicy::default()
        }
    }

    const STD: ExecutionProfile = ExecutionProfile::Standard;
    const POD: ExecutionProfile = ExecutionProfile::IsolatedPod;

    fn sim(_: &SandboxBackend) -> bool {
        true
    }
    fn nao(_: &SandboxBackend) -> bool {
        false
    }

    fn desligado(m: MotivoDoBashDesligado) -> ExposicaoDoBash {
        ExposicaoDoBash::Desligado { motivo: m }
    }

    #[test]
    fn standard_sem_sandbox_desliga_o_bash() {
        let e = decidir_exposicao_do_bash(STD, &SandboxPolicy::default(), true, sim);
        assert_eq!(e, desligado(MotivoDoBashDesligado::SandboxDesligado));
        assert!(!e.registra_bash());
    }

    #[test]
    fn standard_com_ssh_desliga_mesmo_com_as_flags_reconhecidas() {
        let mut p = policy(SandboxMode::All, Some(SandboxBackend::Ssh("box".into())));
        p.network_disabled = false;
        p.mount_workdir = false;
        assert_eq!(
            decidir_exposicao_do_bash(STD, &p, true, sim),
            desligado(MotivoDoBashDesligado::BackendSsh)
        );
    }

    #[test]
    fn standard_com_bash_elevado_desliga() {
        let mut p = policy(SandboxMode::All, Some(SandboxBackend::Docker));
        p.elevated = vec!["bash".into()];
        assert_eq!(
            decidir_exposicao_do_bash(STD, &p, true, sim),
            desligado(MotivoDoBashDesligado::BashElevado)
        );
    }

    #[test]
    fn standard_com_allowlist_sem_bash_desliga() {
        let mut p = policy(SandboxMode::Allowlist, Some(SandboxBackend::Docker));
        p.sandboxed_tools = vec!["web_fetch".into()];
        assert_eq!(
            decidir_exposicao_do_bash(STD, &p, true, sim),
            desligado(MotivoDoBashDesligado::BashForaDaAllowlist)
        );
        p.sandboxed_tools = vec!["bash".into()];
        assert_eq!(
            decidir_exposicao_do_bash(STD, &p, true, sim),
            ExposicaoDoBash::Sandbox {
                backend: SandboxBackend::Docker
            }
        );
    }

    #[test]
    fn standard_sem_backend_desliga() {
        let p = policy(SandboxMode::All, None);
        assert_eq!(
            decidir_exposicao_do_bash(STD, &p, true, sim),
            desligado(MotivoDoBashDesligado::SemBackend)
        );
    }

    #[test]
    fn standard_com_binario_ausente_desliga() {
        let p = policy(SandboxMode::All, Some(SandboxBackend::Docker));
        assert_eq!(
            decidir_exposicao_do_bash(STD, &p, true, nao),
            desligado(MotivoDoBashDesligado::BackendIndisponivel)
        );
    }

    #[test]
    fn standard_fora_de_unix_desliga() {
        let p = policy(SandboxMode::All, Some(SandboxBackend::Docker));
        assert_eq!(
            decidir_exposicao_do_bash(STD, &p, false, sim),
            desligado(MotivoDoBashDesligado::PlataformaNaoUnix)
        );
    }

    #[test]
    fn standard_com_docker_ou_podman_disponivel_sandboxa() {
        for b in [SandboxBackend::Docker, SandboxBackend::Podman] {
            let p = policy(SandboxMode::All, Some(b.clone()));
            let e = decidir_exposicao_do_bash(STD, &p, true, sim);
            assert_eq!(e, ExposicaoDoBash::Sandbox { backend: b });
            assert!(e.registra_bash());
        }
    }

    #[test]
    fn isolated_pod_sem_sandbox_roda_no_host_do_pod() {
        let e = decidir_exposicao_do_bash(POD, &SandboxPolicy::default(), true, sim);
        assert_eq!(e, ExposicaoDoBash::HostDoPod);
        assert!(e.registra_bash());
        // ssh e binario ausente tambem caem no host do pod, nao em Desligado.
        let ssh = policy(SandboxMode::All, Some(SandboxBackend::Ssh("b".into())));
        assert_eq!(
            decidir_exposicao_do_bash(POD, &ssh, true, sim),
            ExposicaoDoBash::HostDoPod
        );
    }

    #[test]
    fn isolated_pod_com_docker_disponivel_continua_sandboxado() {
        let p = policy(SandboxMode::All, Some(SandboxBackend::Docker));
        assert_eq!(
            decidir_exposicao_do_bash(POD, &p, true, sim),
            ExposicaoDoBash::Sandbox {
                backend: SandboxBackend::Docker
            }
        );
    }

    #[test]
    fn descricao_e_passo_nao_carregam_valor_de_config() {
        let mut p = policy(
            SandboxMode::All,
            Some(SandboxBackend::Ssh("host-secreto".into())),
        );
        p.image = "imagem-secreta".into();
        let e = decidir_exposicao_do_bash(STD, &p, true, sim);
        let texto = format!("{} {COMO_LIGAR_O_BASH}", e.descricao());
        assert!(!texto.contains("host-secreto") && !texto.contains("imagem-secreta"));
        assert!(COMO_LIGAR_O_BASH.contains("agent.sandbox"));
        assert!(COMO_LIGAR_O_BASH.contains("execution.profile = isolated-pod"));
    }

    /// ADR 0024: o perfil nunca e inferido de marcador de container. A
    /// varredura roda so sobre a metade de producao deste arquivo.
    #[test]
    fn modulo_nao_infere_nada_de_container() {
        let fonte = include_str!("exposicao_do_bash.rs");
        let producao = fonte.split("#[cfg(test)]").next().unwrap_or(fonte);
        for proibido in [
            concat!("/.docker", "env"),
            concat!("cgr", "oup"),
            concat!("KUBERNETES_", "SERVICE_HOST"),
            concat!("/run/.contain", "erenv"),
            concat!("contain", "er="),
        ] {
            assert!(
                !producao.contains(proibido),
                "exposicao_do_bash.rs nao pode olhar `{proibido}`"
            );
        }
    }
}
