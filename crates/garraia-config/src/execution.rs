//! Secao `execution` (ADR 0024, #1329): o perfil de execucao do processo.
//!
//! Dois perfis. `standard` (default, secao ausente) e a postura de hoje:
//! `ToolGate` fail-closed por modo, jail de filesystem, piso `search` no
//! WhatsApp pessoal. `isolated-pod` e a declaracao do operador de que este
//! processo roda num pod/container **descartavel** e que o pod — nao o
//! Garra — e a fronteira de seguranca; a politica derivada disso mora em
//! `garraia_gateway::bootstrap::execution`, e nao aqui.
//!
//! # O que este modulo NAO faz, de proposito
//!
//! O perfil so muda por **escolha explicita** do operador: chave
//! `execution.profile` no arquivo ou a env [`PROFILE_ENV`], que vence o
//! arquivo. Nenhuma linha aqui (nem em lugar nenhum do workspace) le
//! marcadores de runtime de container para decidir o perfil — um container
//! com o socket do Docker montado ou com o namespace de PID do host e
//! indistinguivel de um pod descartavel visto de dentro, e "parece isolado"
//! nao e isolamento. Um teste varre este arquivo e proibe esses literais.
//!
//! Valor invalido (arquivo ou env) e **erro de carga**, nunca "cai em
//! `standard` em silencio": a env e aplicada pelo `ConfigLoader`, que
//! devolve `Err`, e o gateway nao sobe. `AppConfig::default()` nunca le a
//! env — quem quer a env aplicada passa pelo loader.
//!
//! # Por que a env nao e gravada em `profile`
//!
//! O valor vindo da env vive num campo `#[serde(skip)]` proprio, em vez de
//! sobrescrever `profile`. `ConfigLoader::save` serializa o `AppConfig`
//! inteiro, e ha caminhos load-modify-save (`set_channel_enabled`, `garra
//! config set`, `garra whatsapp link`) que rodam com a env presente. Se a env
//! fosse copiada para `profile`, um unico `save` promoveria um override
//! efemero de ambiente a config persistida: tirar a env deixaria o arquivo
//! dizendo `isolated-pod`. Poder total que sobrevive a remocao da chave que o
//! ligou e exatamente o que este perfil nao pode fazer.

use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Env que vence `execution.profile` (`standard` | `isolated-pod`).
pub const PROFILE_ENV: &str = "GARRAIA_EXECUTION_PROFILE";

/// O perfil de execucao (`execution.profile`, ADR 0024).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutionProfile {
    /// Postura de hoje: instalacao direta em maquina compartilhada.
    #[default]
    Standard,
    /// O processo roda num pod descartavel; o pod e a fronteira.
    IsolatedPod,
}

impl ExecutionProfile {
    /// Os valores aceitos, na grafia da config e da env.
    pub const VALORES_ACEITOS: &'static [&'static str] = &["standard", "isolated-pod"];

    /// A grafia canonica (`standard` | `isolated-pod`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::IsolatedPod => "isolated-pod",
        }
    }

    /// `true` so em `isolated-pod`.
    pub fn is_isolated_pod(self) -> bool {
        matches!(self, Self::IsolatedPod)
    }
}

impl fmt::Display for ExecutionProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ExecutionProfile {
    type Err = ExecutionProfileError;

    /// Aceita exatamente `standard` e `isolated-pod` (apos `trim`, sem
    /// distinguir caixa). Qualquer outra coisa e erro com o valor ofensor —
    /// nunca um default silencioso.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let limpo = s.trim();
        if limpo.eq_ignore_ascii_case("standard") {
            Ok(Self::Standard)
        } else if limpo.eq_ignore_ascii_case("isolated-pod") {
            Ok(Self::IsolatedPod)
        } else {
            Err(ExecutionProfileError {
                valor: limpo.to_string(),
            })
        }
    }
}

/// Valor de perfil que nao e nenhum dos aceitos.
///
/// A mensagem nomeia a env e os valores aceitos porque e o que o operador
/// precisa para consertar — o caminho mais comum ate aqui e um typo em
/// `GARRAIA_EXECUTION_PROFILE` num manifest de pod.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionProfileError {
    valor: String,
}

impl ExecutionProfileError {
    /// O valor recusado, como veio (apos `trim`).
    pub fn valor(&self) -> &str {
        &self.valor
    }
}

impl fmt::Display for ExecutionProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "perfil de execucao invalido {:?} — {} e execution.profile aceitam apenas {}",
            self.valor,
            PROFILE_ENV,
            ExecutionProfile::VALORES_ACEITOS.join(" | ")
        )
    }
}

impl std::error::Error for ExecutionProfileError {}

/// De onde veio o perfil efetivo — para `config check`, `/api/diagnostics`
/// e o log de boot dizerem "isolated-pod (fonte: env)" em vez de so o valor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProfileSource {
    /// Secao ausente ou sem `profile`: o default `standard`.
    #[default]
    Default,
    /// `execution.profile` no `config.yml`/`config.toml`.
    File,
    /// [`PROFILE_ENV`] presente (e valida) no ambiente do processo.
    Env,
}

impl ProfileSource {
    /// `default` | `file` | `env`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::File => "file",
            Self::Env => "env",
        }
    }
}

impl fmt::Display for ProfileSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Secao `execution` (ADR 0024).
///
/// Os campos publicos sao o que esta no arquivo. O perfil **efetivo** — com a
/// env aplicada — sai de [`ExecutionConfig::perfil`], e a origem de
/// [`ExecutionConfig::origem`]; ler `profile` direto ignora a env.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionConfig {
    /// `standard` (default) | `isolated-pod`.
    #[serde(default)]
    pub profile: Option<ExecutionProfile>,
    /// Raiz do MCP `filesystem` autoprovisionado em `isolated-pod`. Ausente
    /// => `<data_dir>/workspace`. Ignorado (com Warning no `config check`)
    /// em `standard`.
    #[serde(default)]
    pub pod_root: Option<PathBuf>,
    /// O que [`PROFILE_ENV`] disse, quando o loader a aplicou. Nunca vai
    /// para o disco — ver o docblock do modulo.
    #[serde(skip)]
    do_env: Option<ExecutionProfile>,
}

impl ExecutionConfig {
    /// A secao como o arquivo a declararia — sem env aplicada. E o unico
    /// construtor fora deste modulo (o campo da env e privado de proposito,
    /// para so o loader e `com_env_aplicada` preenche-lo).
    pub fn new(profile: Option<ExecutionProfile>, pod_root: Option<PathBuf>) -> Self {
        Self {
            profile,
            pod_root,
            do_env: None,
        }
    }

    /// O perfil efetivo: env (se aplicada) > arquivo > `standard`.
    pub fn perfil(&self) -> ExecutionProfile {
        self.do_env.or(self.profile).unwrap_or_default()
    }

    /// De onde [`Self::perfil`] tirou a resposta.
    pub fn origem(&self) -> ProfileSource {
        if self.do_env.is_some() {
            ProfileSource::Env
        } else if self.profile.is_some() {
            ProfileSource::File
        } else {
            ProfileSource::Default
        }
    }

    /// `execution.pod_root`, como declarado (sem resolver).
    pub fn pod_root(&self) -> Option<&Path> {
        self.pod_root.as_deref()
    }

    /// Aplica [`PROFILE_ENV`] por cima do arquivo. Env ausente ou vazia nao
    /// muda nada; env invalida e `Err` — e o chamador (o `ConfigLoader`)
    /// transforma isso em erro de carga.
    pub fn aplicar_env(&mut self) -> Result<(), ExecutionProfileError> {
        if let Some(perfil) = perfil_do_env()? {
            self.do_env = Some(perfil);
        }
        Ok(())
    }

    /// Constroi a secao com um perfil vindo "da env", sem ler o ambiente.
    /// Para testes de quem consome a secao (gateway, check) sem `set_var`.
    #[doc(hidden)]
    pub fn com_env_aplicada(mut self, perfil: ExecutionProfile) -> Self {
        self.do_env = Some(perfil);
        self
    }
}

/// Le [`PROFILE_ENV`]: ausente ou vazia => `Ok(None)`; valida => `Ok(Some)`;
/// qualquer outra coisa => `Err` com o valor ofensor.
pub fn perfil_do_env() -> Result<Option<ExecutionProfile>, ExecutionProfileError> {
    match std::env::var(PROFILE_ENV) {
        Ok(raw) if raw.trim().is_empty() => Ok(None),
        Ok(raw) => raw.parse().map(Some),
        Err(_) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AppConfig;

    /// Serializa o `AppConfig` com a secao e le de volta, nos dois formatos
    /// que o loader aceita.
    #[test]
    fn isolated_pod_faz_round_trip_em_yaml_e_toml() {
        let yaml = "execution:\n  profile: isolated-pod\n  pod_root: /workspace\n";
        let cfg: AppConfig = serde_yaml::from_str(yaml).expect("yaml parseia");
        assert_eq!(cfg.execution.profile, Some(ExecutionProfile::IsolatedPod));
        assert_eq!(cfg.execution.pod_root(), Some(Path::new("/workspace")));
        assert_eq!(cfg.execution.perfil(), ExecutionProfile::IsolatedPod);

        let de_volta = serde_yaml::to_string(&cfg).expect("serializa");
        assert!(de_volta.contains("profile: isolated-pod"), "{de_volta}");
        let relido: AppConfig = serde_yaml::from_str(&de_volta).expect("relê");
        assert_eq!(relido.execution, cfg.execution);

        let toml = "[execution]\nprofile = \"isolated-pod\"\n";
        let cfg: AppConfig = toml::from_str(toml).expect("toml parseia");
        assert_eq!(cfg.execution.perfil(), ExecutionProfile::IsolatedPod);
    }

    /// Secao ausente = comportamento de hoje: `standard`, origem `default`.
    #[test]
    fn secao_ausente_e_standard_de_origem_default() {
        let cfg: AppConfig = serde_yaml::from_str("gateway:\n  port: 3888\n").expect("parseia");
        assert_eq!(cfg.execution.profile, None);
        assert_eq!(cfg.execution.perfil(), ExecutionProfile::Standard);
        assert_eq!(cfg.execution.origem(), ProfileSource::Default);
        assert_eq!(
            AppConfig::default().execution.origem(),
            ProfileSource::Default
        );
    }

    /// Valor desconhecido no arquivo e erro de parse — nunca `standard` em
    /// silencio (ADR 0024, "fail-closed em toda duvida").
    #[test]
    fn valor_desconhecido_no_arquivo_e_recusado() {
        for ruim in ["isolated_pod", "pod", "IsolatedPod", "trusted", ""] {
            let yaml = format!("execution:\n  profile: {ruim:?}\n");
            let r = serde_yaml::from_str::<AppConfig>(&yaml);
            assert!(r.is_err(), "{ruim:?} deveria ser recusado");
        }
    }

    #[test]
    fn from_str_aceita_so_os_dois_valores() {
        assert_eq!(
            "standard".parse::<ExecutionProfile>(),
            Ok(ExecutionProfile::Standard)
        );
        assert_eq!(
            " Isolated-Pod \n".parse::<ExecutionProfile>(),
            Ok(ExecutionProfile::IsolatedPod)
        );
        let err = "pod".parse::<ExecutionProfile>().expect_err("invalido");
        assert_eq!(err.valor(), "pod");
        let msg = err.to_string();
        assert!(msg.contains(PROFILE_ENV), "{msg}");
        assert!(
            msg.contains("standard") && msg.contains("isolated-pod"),
            "{msg}"
        );
        assert!("isolated_pod".parse::<ExecutionProfile>().is_err());
        assert!("".parse::<ExecutionProfile>().is_err());
    }

    #[test]
    fn as_str_e_display_batem_com_a_grafia_da_config() {
        assert_eq!(ExecutionProfile::Standard.as_str(), "standard");
        assert_eq!(ExecutionProfile::IsolatedPod.to_string(), "isolated-pod");
        assert_eq!(ProfileSource::Default.as_str(), "default");
        assert_eq!(ProfileSource::File.as_str(), "file");
        assert_eq!(ProfileSource::Env.to_string(), "env");
        for v in ExecutionProfile::VALORES_ACEITOS {
            assert!(v.parse::<ExecutionProfile>().is_ok(), "{v}");
        }
    }

    #[test]
    fn origem_e_file_quando_o_arquivo_declara() {
        let cfg = ExecutionConfig {
            profile: Some(ExecutionProfile::Standard),
            ..Default::default()
        };
        assert_eq!(cfg.origem(), ProfileSource::File);
        assert_eq!(cfg.perfil(), ExecutionProfile::Standard);
    }

    /// O que a env aplicada faz com a secao, sem tocar no ambiente.
    #[test]
    fn env_aplicada_vence_o_arquivo_e_nao_vai_para_o_disco() {
        let cfg = ExecutionConfig {
            profile: Some(ExecutionProfile::IsolatedPod),
            ..Default::default()
        }
        .com_env_aplicada(ExecutionProfile::Standard);
        assert_eq!(cfg.perfil(), ExecutionProfile::Standard);
        assert_eq!(cfg.origem(), ProfileSource::Env);
        // O arquivo continua dizendo o que dizia: o `save` nao promove a env.
        assert_eq!(cfg.profile, Some(ExecutionProfile::IsolatedPod));
        let yaml = serde_yaml::to_string(&cfg).expect("serializa");
        assert!(yaml.contains("profile: isolated-pod"), "{yaml}");
        assert!(!yaml.contains("do_env"), "{yaml}");
    }

    fn com_env<T>(valor: Option<&str>, f: impl FnOnce() -> T) -> T {
        let _guard = crate::ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let anterior = std::env::var_os(PROFILE_ENV);
        // SAFETY: ENV_TEST_LOCK held.
        unsafe {
            match valor {
                Some(v) => std::env::set_var(PROFILE_ENV, v),
                None => std::env::remove_var(PROFILE_ENV),
            }
        }
        let r = f();
        // SAFETY: ENV_TEST_LOCK held.
        unsafe {
            match anterior {
                Some(v) => std::env::set_var(PROFILE_ENV, v),
                None => std::env::remove_var(PROFILE_ENV),
            }
        }
        r
    }

    /// Precedencia real, lendo o ambiente: env > arquivo > default.
    #[test]
    fn aplicar_env_da_precedencia_a_env_sobre_o_arquivo() {
        com_env(Some("isolated-pod"), || {
            let mut cfg = ExecutionConfig {
                profile: Some(ExecutionProfile::Standard),
                ..Default::default()
            };
            cfg.aplicar_env().expect("env valida");
            assert_eq!(cfg.perfil(), ExecutionProfile::IsolatedPod);
            assert_eq!(cfg.origem(), ProfileSource::Env);
            assert_eq!(cfg.profile, Some(ExecutionProfile::Standard));
        });
        com_env(None, || {
            let mut cfg = ExecutionConfig {
                profile: Some(ExecutionProfile::IsolatedPod),
                ..Default::default()
            };
            cfg.aplicar_env().expect("sem env");
            assert_eq!(cfg.perfil(), ExecutionProfile::IsolatedPod);
            assert_eq!(cfg.origem(), ProfileSource::File);
            assert_eq!(perfil_do_env(), Ok(None));
        });
        // Env presente mas vazia conta como ausente — e o que um
        // `env GARRAIA_EXECUTION_PROFILE= garraia start` produz.
        com_env(Some("   "), || {
            let mut cfg = ExecutionConfig::default();
            cfg.aplicar_env().expect("env vazia e ausente");
            assert_eq!(cfg.origem(), ProfileSource::Default);
        });
    }

    /// Env invalida e `Err`, com o valor e a env na mensagem — e a secao
    /// fica como estava (o loader descarta tudo, mas a funcao nao pode ter
    /// meio-aplicado nada).
    #[test]
    fn env_invalida_e_erro_e_nao_cai_em_standard() {
        com_env(Some("pod"), || {
            let mut cfg = ExecutionConfig {
                profile: Some(ExecutionProfile::IsolatedPod),
                ..Default::default()
            };
            let err = cfg.aplicar_env().expect_err("env invalida");
            assert_eq!(err.valor(), "pod");
            assert!(err.to_string().contains(PROFILE_ENV));
            assert_eq!(cfg.origem(), ProfileSource::File);
            assert!(perfil_do_env().is_err());
        });
    }

    /// ADR 0024, driver 1: o perfil e explicito, nunca inferido. Verificado
    /// no fonte, como `detect.rs::o_modulo_de_deteccao_nunca_executa_binario`:
    /// revisao humana nao pega um `if Path::new("/.dockerenv").exists()`
    /// num diff futuro tao bem quanto um teste.
    #[test]
    fn o_modulo_do_perfil_nunca_detecta_container() {
        let fonte = include_str!("execution.rs");

        // Ignora este proprio teste, que precisa citar os literais proibidos.
        let ate_o_teste = fonte
            .split("fn o_modulo_do_perfil_nunca_detecta_container")
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
                "execution.rs nao pode conter `{proibido}`: o perfil e declarado, nunca detectado"
            );
        }
    }
}
