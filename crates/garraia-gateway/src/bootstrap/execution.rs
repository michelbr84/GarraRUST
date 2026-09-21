//! ADR 0024 (#1329): a **politica** derivada do perfil de execucao.
//!
//! `garraia-config` guarda o que o operador declarou (`execution.profile`,
//! `execution.pod_root`, a env `GARRAIA_EXECUTION_PROFILE`); este modulo
//! traduz isso nas decisoes que o gateway toma na subida — a raiz do MCP
//! `filesystem` autoprovisionado e o aviso de boot — do mesmo jeito que
//! `sandbox_policy_from` traduz `agent.sandbox`. Tudo aqui e puro: nada le
//! disco, nada cria diretorio; quem provisiona e quem cria.
//!
//! # O que este modulo nunca faz
//!
//! Decidir o perfil sozinho. Nao existe leitura de marcador de runtime de
//! container aqui — um container com o socket do Docker montado ou com o
//! namespace de PID do host e indistinguivel de um pod descartavel visto de
//! dentro. O perfil vem do `AppConfig` ja carregado (arquivo ou env, ambos
//! escolha explicita do operador), e um teste varre este arquivo atras dos
//! literais de deteccao e falha se algum aparecer.
//!
//! # `$HOME` nunca e raiz implicita
//!
//! Em nenhum perfil. Em `standard` a raiz do `filesystem` e
//! `agent.file_roots` (se houver) ou `<data_dir>/workspace`; em
//! `isolated-pod` e `execution.pod_root` (se houver) ou o mesmo workspace. Um
//! caminho pod-local mais amplo (`/workspace`, `/`) e escolha explicita em
//! `pod_root`, nunca inferida.

use std::path::PathBuf;

use garraia_config::{AppConfig, ExecutionProfile, ProfileSource};
use tracing::{info, warn};

/// A politica de execucao efetiva, resolvida uma vez na subida.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoliticaDeExecucao {
    /// `standard` | `isolated-pod`, com a env ja aplicada pelo loader.
    pub perfil: ExecutionProfile,
    /// `default` | `file` | `env`.
    pub origem: ProfileSource,
    /// `execution.pod_root`, como declarado. So tem efeito em `isolated-pod`.
    pub pod_root: Option<PathBuf>,
}

impl PoliticaDeExecucao {
    /// `true` so em `isolated-pod`.
    pub fn is_isolated_pod(&self) -> bool {
        self.perfil.is_isolated_pod()
    }
}

/// Le a politica do `AppConfig`. Pura.
pub fn politica_de_execucao(config: &AppConfig) -> PoliticaDeExecucao {
    PoliticaDeExecucao {
        perfil: config.execution.perfil(),
        origem: config.execution.origem(),
        pod_root: config.execution.pod_root().map(PathBuf::from),
    }
}

/// As raizes que o MCP `filesystem` autoprovisionado recebe, por perfil.
///
/// - `standard`: `agent.file_roots` quando nao esta vazio (o mesmo jail das
///   file tools nativas, #1244); senao `<data_dir>/workspace`.
/// - `isolated-pod`: `execution.pod_root` quando declarado; senao o mesmo
///   `<data_dir>/workspace`.
///
/// **Nunca** `$HOME` — era o `$HOME` implicito que a #1329 apontou como o
/// contorno do jail. Pura: nao toca o disco; quem chama cria o diretorio.
pub fn raizes_do_mcp_filesystem(config: &AppConfig) -> Vec<PathBuf> {
    let workspace = || vec![config.resolved_data_dir().join("workspace")];
    match config.execution.perfil() {
        ExecutionProfile::Standard => {
            let declaradas: Vec<PathBuf> = config
                .agent
                .file_roots
                .iter()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .collect();
            if declaradas.is_empty() {
                workspace()
            } else {
                declaradas
            }
        }
        ExecutionProfile::IsolatedPod => match config.execution.pod_root() {
            Some(root) => vec![root.to_path_buf()],
            None => workspace(),
        },
    }
}

/// O anuncio de boot. `standard` e um `info!` com perfil e origem;
/// `isolated-pod` e um unico `warn!` que diz o que foi liberado, o que o
/// perfil NAO isola por conta propria e como reverter — porque o risco
/// numero um do ADR e o operador ligar o perfil fora de um pod.
pub fn anunciar_no_boot(p: &PoliticaDeExecucao) {
    match p.perfil {
        ExecutionProfile::Standard => {
            info!(
                "execution profile = standard (fonte: {}) — postura padrao: ToolGate por modo, \
                 jail de filesystem, piso `search` no WhatsApp pessoal",
                p.origem
            );
        }
        ExecutionProfile::IsolatedPod => {
            let raiz = p
                .pod_root
                .as_ref()
                .map(|r| r.display().to_string())
                .unwrap_or_else(|| "<data_dir>/workspace (execution.pod_root ausente)".into());
            warn!(
                "execution profile = isolated-pod (fonte: {}, pod_root: {raiz}). O agente tem \
                 PODER TOTAL dentro deste pod para o dono do WhatsApp em conversa 1:1: \
                 filesystem, shell, servidores MCP e subagentes. O POD e a fronteira de \
                 seguranca, nao o Garra. O perfil NAO isola por conta propria: filesystem do \
                 host montado, socket do Docker/Podman, namespace de PID/rede do host, mounts \
                 nao declarados e segredos do host ficam ao alcance do agente se o pod os \
                 expoe. Confirme que este processo roda num pod descartavel. Para reverter: \
                 `execution.profile = standard` no config.yml ou remova a env \
                 GARRAIA_EXECUTION_PROFILE (ADR 0024, #1329)",
                p.origem
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use garraia_config::{AgentConfig, ExecutionConfig};
    use std::path::Path;

    fn config(
        profile: Option<ExecutionProfile>,
        pod_root: Option<&str>,
        file_roots: &[&str],
        data_dir: Option<&str>,
    ) -> AppConfig {
        AppConfig {
            execution: ExecutionConfig::new(profile, pod_root.map(PathBuf::from)),
            agent: AgentConfig {
                file_roots: file_roots.iter().map(|s| s.to_string()).collect(),
                ..AgentConfig::default()
            },
            data_dir: data_dir.map(PathBuf::from),
            ..AppConfig::default()
        }
    }

    #[test]
    fn politica_reflete_perfil_origem_e_pod_root() {
        let ausente = politica_de_execucao(&AppConfig::default());
        assert_eq!(
            ausente,
            PoliticaDeExecucao {
                perfil: ExecutionProfile::Standard,
                origem: ProfileSource::Default,
                pod_root: None,
            }
        );
        assert!(!ausente.is_isolated_pod());

        let por_arquivo = politica_de_execucao(&config(
            Some(ExecutionProfile::IsolatedPod),
            Some("/workspace"),
            &[],
            None,
        ));
        assert_eq!(por_arquivo.perfil, ExecutionProfile::IsolatedPod);
        assert_eq!(por_arquivo.origem, ProfileSource::File);
        assert_eq!(
            por_arquivo.pod_root.as_deref(),
            Some(Path::new("/workspace"))
        );
        assert!(por_arquivo.is_isolated_pod());

        // A env (ja aplicada pelo loader) vence o arquivo, e a origem diz isso.
        let mut cfg = config(Some(ExecutionProfile::IsolatedPod), None, &[], None);
        cfg.execution = cfg.execution.com_env_aplicada(ExecutionProfile::Standard);
        let por_env = politica_de_execucao(&cfg);
        assert_eq!(por_env.perfil, ExecutionProfile::Standard);
        assert_eq!(por_env.origem, ProfileSource::Env);
    }

    /// `standard` sem `agent.file_roots`: o workspace do Garra, nunca `$HOME`.
    #[test]
    fn standard_sem_file_roots_usa_o_workspace_do_data_dir() {
        let raizes = raizes_do_mcp_filesystem(&config(None, None, &[], Some("/tmp/garra-data")));
        assert_eq!(raizes, vec![PathBuf::from("/tmp/garra-data/workspace")]);

        // `pod_root` declarado em standard e ignorado (o check avisa).
        let raizes = raizes_do_mcp_filesystem(&config(
            None,
            Some("/workspace"),
            &[],
            Some("/tmp/garra-data"),
        ));
        assert_eq!(raizes, vec![PathBuf::from("/tmp/garra-data/workspace")]);
    }

    /// `standard` com `agent.file_roots`: o mesmo jail das file tools nativas.
    #[test]
    fn standard_com_file_roots_usa_as_raizes_do_jail() {
        let raizes = raizes_do_mcp_filesystem(&config(
            Some(ExecutionProfile::Standard),
            None,
            &["/srv/projeto", "  ", "/srv/outro"],
            Some("/tmp/garra-data"),
        ));
        assert_eq!(
            raizes,
            vec![PathBuf::from("/srv/projeto"), PathBuf::from("/srv/outro")]
        );
    }

    /// `isolated-pod` com `pod_root`: e ele, e so ele — `agent.file_roots`
    /// nao entra (o jail das file tools nativas continua valendo la).
    #[test]
    fn isolated_pod_com_pod_root_usa_o_pod_root() {
        let raizes = raizes_do_mcp_filesystem(&config(
            Some(ExecutionProfile::IsolatedPod),
            Some("/workspace"),
            &["/srv/projeto"],
            Some("/tmp/garra-data"),
        ));
        assert_eq!(raizes, vec![PathBuf::from("/workspace")]);
    }

    /// `isolated-pod` sem `pod_root`: o workspace do Garra — a fronteira
    /// default dentro do pod e o workspace, nao o pod inteiro.
    #[test]
    fn isolated_pod_sem_pod_root_usa_o_workspace_do_data_dir() {
        let raizes = raizes_do_mcp_filesystem(&config(
            Some(ExecutionProfile::IsolatedPod),
            None,
            &["/srv/projeto"],
            Some("/tmp/garra-data"),
        ));
        assert_eq!(raizes, vec![PathBuf::from("/tmp/garra-data/workspace")]);
    }

    /// Sem `data_dir` a raiz cai em `<config_dir>/data/workspace` — e em
    /// nenhum perfil ela e o `$HOME` nu.
    #[test]
    fn nenhum_perfil_usa_home_como_raiz() {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .expect("HOME no ambiente de teste");
        for profile in [None, Some(ExecutionProfile::IsolatedPod)] {
            let raizes = raizes_do_mcp_filesystem(&config(profile, None, &[], None));
            assert_eq!(raizes.len(), 1, "{raizes:?}");
            assert_ne!(raizes[0], home, "{profile:?}: $HOME nunca e raiz implicita");
            assert!(
                raizes[0].ends_with("workspace"),
                "{profile:?}: {}",
                raizes[0].display()
            );
        }
    }

    /// Os dois ramos do anuncio rodam sem panic (o conteudo e log, nao
    /// contrato; o que se prende aqui e que nenhum ramo `unwrap`a).
    #[test]
    fn anunciar_no_boot_cobre_os_dois_perfis() {
        anunciar_no_boot(&politica_de_execucao(&AppConfig::default()));
        anunciar_no_boot(&politica_de_execucao(&config(
            Some(ExecutionProfile::IsolatedPod),
            None,
            &[],
            None,
        )));
        anunciar_no_boot(&politica_de_execucao(&config(
            Some(ExecutionProfile::IsolatedPod),
            Some("/workspace"),
            &[],
            None,
        )));
    }

    /// ADR 0024, driver 1: o perfil e explicito, nunca inferido. Mesmo molde
    /// de `detect.rs::o_modulo_de_deteccao_nunca_executa_binario` e do teste
    /// gemeo em `garraia_config::execution`.
    #[test]
    fn o_modulo_de_politica_nunca_detecta_container() {
        let fonte = include_str!("execution.rs");

        // Ignora este proprio teste, que precisa citar os literais proibidos.
        let ate_o_teste = fonte
            .split("fn o_modulo_de_politica_nunca_detecta_container")
            .next()
            .unwrap_or(fonte);

        // Ignora comentario e doc comment: a regra e sobre o que o modulo
        // *faz*, nao sobre o que ele explica.
        let corpo: String = ate_o_teste
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");

        for proibido in [
            "/.dockerenv",
            "cgroup",
            "/proc/1",
            "/proc/self",
            "KUBERNETES_SERVICE_HOST",
            "container=",
        ] {
            assert!(
                !corpo.contains(proibido),
                "bootstrap/execution.rs nao pode conter `{proibido}`: o perfil e declarado, \
                 nunca detectado"
            );
        }
    }
}
