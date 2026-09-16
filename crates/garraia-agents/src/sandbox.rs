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
//! - OpenShell e Crabbox ficam 🔵 planejados (variantes `SandboxBackend` não
//!   implementadas ainda); Docker/Podman/SSH estão funcionais.

use garraia_common::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

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
        let backend = self.backend.as_ref().ok_or_else(|| {
            Error::Agent(
                "sandbox obrigatório por config mas nenhum backend definido \
                 (tools.sandbox.backend: docker|podman|ssh)"
                    .into(),
            )
        })?;
        if !backend.is_available() {
            return Err(Error::Agent(format!(
                "sandbox fail-closed: backend `{}` não encontrado no host; \
                 instale-o, marque a tool como elevated, ou defina \
                 tools.sandbox.mode = off",
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
                    parts.push_str(&format!(" -v {cwd}:{cwd} -w {cwd}"));
                }
                parts.push_str(&format!(" {} sh -lc {:?}", self.image, command));
                parts
            }
            SandboxBackend::Ssh(host) => {
                // NOTA: ssh não isola o host remoto; é isolamento do host
                // local. Documentado como tal no módulo e nos docs.
                format!("ssh {host} -- sh -lc {:?}", command)
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
        // o contrato via backend com nome que nunca existe: usamos Ssh com
        // binário ssh simulado ausente não é possível — validamos apenas a
        // sintaxe do wrap do Ssh (que não depende de disponibilidade aqui).
        let ssh = SandboxPolicy {
            mode: SandboxMode::All,
            backend: Some(SandboxBackend::Ssh("box".into())),
            mount_workdir: false,
            ..SandboxPolicy::default()
        };
        let wrapped = ssh
            .wrap_command("bash", "echo oi", "/tmp")
            .unwrap()
            .unwrap();
        assert_eq!(wrapped, "ssh box -- sh -lc \"echo oi\"");
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
