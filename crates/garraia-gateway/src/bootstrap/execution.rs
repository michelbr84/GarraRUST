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

/// As raizes que o MCP `filesystem` autoprovisionado recebe — e de onde
/// vieram, porque o provisionamento trata as duas origens de modo diferente.
///
/// `Workspace` e o default `<data_dir>/workspace`, um diretorio do proprio
/// Garra que o primeiro boot **cria** (ele nao existe ainda). `Declaradas`
/// sao caminhos que o operador escreveu (`agent.file_roots` em `standard`,
/// `execution.pod_root` em `isolated-pod`): o provisionamento **nao os
/// cria** — um `pod_root` com typo, ou relativo, viraria um diretorio novo
/// no host (ou no cwd de quem subiu o processo) por efeito colateral do
/// boot (F-3 da auditoria da #1329). Raiz declarada que nao existe e "nao
/// provisiona", com aviso.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RaizesDoMcpFilesystem {
    /// `<data_dir>/workspace`: a unica raiz que o provisionamento cria.
    Workspace(PathBuf),
    /// `agent.file_roots` ou `execution.pod_root`, como declarados. Nunca
    /// vazio quando sai de [`raizes_do_mcp_filesystem`].
    Declaradas(Vec<PathBuf>),
}

impl RaizesDoMcpFilesystem {
    /// Os caminhos, na ordem em que viram argumentos do servidor.
    pub fn caminhos(&self) -> &[PathBuf] {
        match self {
            Self::Workspace(raiz) => std::slice::from_ref(raiz),
            Self::Declaradas(raizes) => raizes,
        }
    }
}

/// As raizes que o MCP `filesystem` autoprovisionado recebe, por perfil.
///
/// - `standard`: `agent.file_roots` quando nao esta vazio; senao
///   `<data_dir>/workspace`.
/// - `isolated-pod`: `execution.pod_root` quando declarado; senao o mesmo
///   `<data_dir>/workspace`.
///
/// So a **config** entra aqui. O jail das file tools nativas (#1244) e mais
/// largo — `FileJail::from_config_roots` soma a env `GARRAIA_FILE_ROOTS`, e
/// cada chamada soma o `working_dir` da sessao — e nada disso chega ao
/// servidor MCP nem ao diagnostico `mcp.filesystem_root`, que compara contra
/// estas raizes declaradas (ADR 0024, tabela "O que cada perfil significa").
/// E deliberado: a raiz do MCP e a mais estreita das duas, e uma
/// `GARRAIA_FILE_ROOTS=/` no ambiente nao pode calar o aviso sobre um
/// `mcp.json` legado apontando para `$HOME`.
///
/// **Nunca** `$HOME` — era o `$HOME` implicito que a #1329 apontou como o
/// contorno do jail. Pura: nao toca o disco; quem provisiona cria (so) o
/// workspace.
pub fn raizes_do_mcp_filesystem(config: &AppConfig) -> RaizesDoMcpFilesystem {
    let workspace =
        || RaizesDoMcpFilesystem::Workspace(config.resolved_data_dir().join("workspace"));
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
                RaizesDoMcpFilesystem::Declaradas(declaradas)
            }
        }
        ExecutionProfile::IsolatedPod => match config.execution.pod_root() {
            Some(root) => RaizesDoMcpFilesystem::Declaradas(vec![root.to_path_buf()]),
            None => workspace(),
        },
    }
}

/// De onde sairam as raizes efetivas das file tools nativas (#1378).
///
/// O boot decide isto uma vez e o `/api/diagnostics` reporta a MESMA decisao,
/// porque os dois passam por [`crate::bootstrap::raizes_das_file_tools`]. Uma
/// linha de console que descrevesse um jail diferente do que o turno usa
/// seria pior que nenhuma linha.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FonteDasRaizesDasFileTools {
    /// O operador declarou raiz (`agent.file_roots` e/ou a env
    /// `GARRAIA_FILE_ROOTS`) e ela resolveu. Comportamento anterior a #1378,
    /// preservado byte a byte: o default nem e consultado.
    Declaradas,
    /// Nada declarado: vale o workspace default do perfil — o mesmo conjunto
    /// que o MCP `filesystem` recebe (ADR 0024).
    WorkspacePadrao,
    /// Nada declarado e o default tambem nao resolveu (o diretorio nao existe
    /// e nao pode ser criado, ou o `execution.pod_root` declarado tem typo).
    /// Fail-closed: so o `working_dir` da sessao autoriza algo, que e
    /// exatamente o estado que a #1378 descreve como defeito.
    SomenteSessao,
}

impl FonteDasRaizesDasFileTools {
    /// Rotulo estavel para log e para o `/api/diagnostics`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Declaradas => "declaradas",
            Self::WorkspacePadrao => "workspace-padrao",
            Self::SomenteSessao => "somente-sessao",
        }
    }
}

impl std::fmt::Display for FonteDasRaizesDasFileTools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// As raizes que as file tools nativas recebem quando o operador nao declarou
/// nenhuma (#1378).
///
/// # Por que existe
///
/// Uma sessao do WhatsApp recem-vinculada nasce com `working_dir = null`. Com
/// `agent.file_roots` vazio — o default de toda instalacao limpa — o conjunto
/// de raizes efetivas de `FileJail::confine` ficava **vazio**, e vazio
/// significa negar tudo (#1244). O resultado e o que a #1378 relata:
/// `file_read`, `file_write` e `list_dir` registradas, anunciadas pelo modo, e
/// recusando toda chamada com `Denial::NoRoots`. A capability existe no papel
/// e nao existe na pratica.
///
/// # Por que e o workspace do Garra, e o mesmo nos dois perfis
///
/// O ADR 0024 ja nomeou o diretorio seguro que o Garra usa quando o operador
/// nao declarou nada: `<data_dir>/workspace`, o unico que o boot cria. Este
/// default reusa esse endereco, e **nao** o resultado inteiro de
/// [`raizes_do_mcp_filesystem`].
///
/// A diferenca esta no `isolated-pod` com `execution.pod_root` declarado. Ali
/// o MCP `filesystem` recebe o `pod_root`; se as tools nativas o recebessem
/// junto, esta correcao teria ampliado, de carona, o alcance das tools
/// nativas num perfil que nao e o assunto da #1378 — e a regra documentada
/// ("`execution.pod_root` muda so a raiz do MCP; para as nativas declare
/// `agent.file_roots`") teria mudado sem que ninguem pedisse. Um P0 de
/// usabilidade nao e lugar para alargar superficie de acesso a arquivo.
///
/// O preco aceito e o oposto do elegante: em `isolated-pod` com `pod_root`,
/// tools nativas e MCP ficam com raizes diferentes. Isso ja era verdade antes
/// da #1378 (as nativas ficavam com raiz NENHUMA), ja esta documentado como
/// pegadinha em `docs/execution-profiles.md`, e continua com a mesma saida:
/// declarar `agent.file_roots`.
///
/// **Nunca** `/` e **nunca** `$HOME`: o caminho e sempre um filho do
/// `<data_dir>`, e um alcance maior segue sendo escolha explicita do operador.
///
/// Pura: nao toca o disco. Quem cria o workspace e
/// [`crate::bootstrap::garantir_workspace_padrao`], na subida.
pub fn raizes_default_das_file_tools(config: &AppConfig) -> PathBuf {
    config.resolved_data_dir().join("workspace")
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
        assert_eq!(
            raizes,
            RaizesDoMcpFilesystem::Workspace(PathBuf::from("/tmp/garra-data/workspace"))
        );
        assert_eq!(
            raizes.caminhos(),
            &[PathBuf::from("/tmp/garra-data/workspace")]
        );

        // `pod_root` declarado em standard e ignorado (o check avisa).
        let raizes = raizes_do_mcp_filesystem(&config(
            None,
            Some("/workspace"),
            &[],
            Some("/tmp/garra-data"),
        ));
        assert_eq!(
            raizes,
            RaizesDoMcpFilesystem::Workspace(PathBuf::from("/tmp/garra-data/workspace"))
        );
    }

    /// `standard` com `agent.file_roots`: as raizes declaradas na config —
    /// e so elas (a env `GARRAIA_FILE_ROOTS` do jail nativo nao entra).
    #[test]
    fn standard_com_file_roots_usa_as_raizes_declaradas() {
        let raizes = raizes_do_mcp_filesystem(&config(
            Some(ExecutionProfile::Standard),
            None,
            &["/srv/projeto", "  ", "/srv/outro"],
            Some("/tmp/garra-data"),
        ));
        assert_eq!(
            raizes,
            RaizesDoMcpFilesystem::Declaradas(vec![
                PathBuf::from("/srv/projeto"),
                PathBuf::from("/srv/outro")
            ])
        );
        assert_eq!(
            raizes.caminhos(),
            &[PathBuf::from("/srv/projeto"), PathBuf::from("/srv/outro")]
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
        assert_eq!(
            raizes,
            RaizesDoMcpFilesystem::Declaradas(vec![PathBuf::from("/workspace")])
        );
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
        assert_eq!(
            raizes,
            RaizesDoMcpFilesystem::Workspace(PathBuf::from("/tmp/garra-data/workspace"))
        );
    }

    /// Sem `data_dir` a raiz cai em `<config_dir>/data/workspace` — e em
    /// nenhum perfil ela e o `$HOME` nu.
    #[test]
    #[serial_test::serial] // le `HOME`, que testes de `persistence` reescrevem
    fn nenhum_perfil_usa_home_como_raiz() {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .expect("HOME no ambiente de teste");
        for profile in [None, Some(ExecutionProfile::IsolatedPod)] {
            let raizes = raizes_do_mcp_filesystem(&config(profile, None, &[], None));
            let caminhos = raizes.caminhos();
            assert_eq!(caminhos.len(), 1, "{raizes:?}");
            assert_ne!(
                caminhos[0], home,
                "{profile:?}: $HOME nunca e raiz implicita"
            );
            assert!(
                caminhos[0].ends_with("workspace"),
                "{profile:?}: {}",
                caminhos[0].display()
            );
            assert!(
                matches!(raizes, RaizesDoMcpFilesystem::Workspace(_)),
                "{profile:?}: sem declaracao a raiz e o workspace, o unico que o boot cria"
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

    // ─── #1378: o default das file tools nativas ──────────────────────────

    /// **A garantia da #1378.** O default das file tools nativas e sempre o
    /// workspace do Garra — nunca `/`, nunca `$HOME` — e nao muda com perfil,
    /// `pod_root` nem `agent.file_roots`. E a raiz que a sessao do WhatsApp
    /// sem projeto passa a enxergar.
    #[test]
    fn default_das_file_tools_e_sempre_o_workspace_do_data_dir() {
        let casos = [
            config(None, None, &[], Some("/tmp/garra-data")),
            config(
                Some(ExecutionProfile::Standard),
                None,
                &["/srv/notas"],
                Some("/tmp/garra-data"),
            ),
            config(
                Some(ExecutionProfile::IsolatedPod),
                Some("/workspace"),
                &[],
                Some("/tmp/garra-data"),
            ),
        ];
        for cfg in casos {
            let caminho = raizes_default_das_file_tools(&cfg);
            assert_eq!(caminho, PathBuf::from("/tmp/garra-data/workspace"));
            assert!(
                caminho.parent().is_some(),
                "o default nunca pode ser a raiz do filesystem"
            );
            if let Some(home) = dirs::home_dir() {
                assert_ne!(caminho, home, "o default nunca pode ser o $HOME");
            }
        }
    }

    /// **A fronteira que esta correcao NAO cruza.** Em `isolated-pod` com
    /// `execution.pod_root`, o MCP `filesystem` recebe o `pod_root` e as tools
    /// nativas continuam sem receber: quem quiser as duas coisas declara
    /// `agent.file_roots`, como sempre foi. Se este teste ficar vermelho,
    /// alguem ampliou o alcance das tools nativas no perfil isolado de carona
    /// numa correcao de usabilidade.
    #[test]
    fn pod_root_nao_vira_raiz_das_file_tools_nativas() {
        let cfg = config(
            Some(ExecutionProfile::IsolatedPod),
            Some("/workspace"),
            &[],
            Some("/tmp/garra-data"),
        );
        assert_eq!(
            raizes_do_mcp_filesystem(&cfg),
            RaizesDoMcpFilesystem::Declaradas(vec![PathBuf::from("/workspace")]),
            "o MCP continua recebendo o pod_root (ADR 0024)"
        );
        assert_eq!(
            raizes_default_das_file_tools(&cfg),
            PathBuf::from("/tmp/garra-data/workspace"),
            "as tools nativas NAO herdam o pod_root (#1378)"
        );
    }

    /// Sem declaracao nenhuma os dois coincidem — e ali a #1378 nao inventou
    /// endereco novo: reusou o `<data_dir>/workspace` que o ADR 0024 ja
    /// designou como o diretorio seguro do Garra.
    #[test]
    fn sem_declaracao_o_default_coincide_com_o_do_mcp() {
        let cfg = config(None, None, &[], Some("/tmp/garra-data"));
        assert_eq!(
            raizes_do_mcp_filesystem(&cfg).caminhos(),
            [raizes_default_das_file_tools(&cfg)]
        );
    }

    /// O rotulo da fonte e estavel: ele sai no log de boot e no JSON do
    /// `/api/diagnostics`, que o console le.
    #[test]
    fn rotulos_da_fonte_sao_estaveis() {
        assert_eq!(
            FonteDasRaizesDasFileTools::Declaradas.as_str(),
            "declaradas"
        );
        assert_eq!(
            FonteDasRaizesDasFileTools::WorkspacePadrao.as_str(),
            "workspace-padrao"
        );
        assert_eq!(
            FonteDasRaizesDasFileTools::SomenteSessao.as_str(),
            "somente-sessao"
        );
    }
}
