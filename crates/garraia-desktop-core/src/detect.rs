//! Detecção de agentes externos — **leitura, nunca execução**.
//!
//! # As três regras fail-closed (issue #1181)
//!
//! 1. **Detecção é leitura.** Varre `PATH` e configs conhecidas e **nunca
//!    executa** o binário para "ver se é ele".
//! 2. **Adicionar é decisão do usuário.** Detectado ≠ adicionado. Este módulo
//!    não escreve nada, em lugar nenhum.
//! 3. **Executar é ação explícita.** Não é assunto daqui — é de
//!    [`crate::supervise`], e com o comando visível antes de rodar.
//!
//! Este módulo implementa a regra 1 e torna a 2 estrutural: a única coisa que
//! ele sabe fazer é perguntar a um [`Filesystem`] se um caminho existe. Não há
//! método de escrita, não há `std::process`, e um teste varre o próprio fonte
//! para garantir que continua assim.
//!
//! # Por que um nome na `PATH` não basta
//!
//! Lição já paga em `crates/garraia-cli/src/agents.rs`: o nome `agentdeck` no
//! registro do npm pertence a um **projeto diferente e sem relação**. Um
//! executável com o nome certo na `PATH` não prova identidade nenhuma — e o
//! jeito que a CLI resolve isso (rodar `agentdeck agents --help`) não está
//! disponível aqui, porque executar é exatamente o que a regra 1 proíbe.
//!
//! A saída é classificar em vez de adivinhar. Achou só o nome:
//! [`Confidence::Ambiguous`] — aparece na UI como "encontrado, não
//! confirmado", e nada acontece com ele sem o usuário mandar. Achou o nome
//! **mais** uma marca que só o agente de verdade deixa (seu diretório de
//! config, seu arquivo de marca): [`Confidence::Confirmed`].
//!
//! Fail-closed quer dizer que a dúvida **nunca** vira confirmação.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// agentes que o control center sabe procurar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    GarraIa,
    Hermes,
    OpenClaw,
    ClaudeCode,
    /// O motor por trás de `garra agents` — é dele que a aba Agents é cliente.
    AgentDeck,
}

impl AgentKind {
    pub const ALL: [AgentKind; 5] = [
        AgentKind::GarraIa,
        AgentKind::Hermes,
        AgentKind::OpenClaw,
        AgentKind::ClaudeCode,
        AgentKind::AgentDeck,
    ];

    /// Nome do executável procurado na `PATH`.
    pub fn binary_name(self) -> &'static str {
        match self {
            AgentKind::GarraIa => "garraia",
            AgentKind::Hermes => "hermes",
            AgentKind::OpenClaw => "openclaw",
            AgentKind::ClaudeCode => "claude",
            AgentKind::AgentDeck => "agentdeck",
        }
    }

    /// Diretório de config sob o `$HOME` do usuário, relativo.
    ///
    /// Encontrar o binário **e** este diretório é o que separa o agente de um
    /// homônimo: quem instala o pacote npm errado não ganha o diretório de
    /// config do projeto certo.
    pub fn config_dir(self) -> &'static str {
        match self {
            AgentKind::GarraIa => ".garraia",
            AgentKind::Hermes => ".hermes",
            AgentKind::OpenClaw => ".openclaw",
            AgentKind::ClaudeCode => ".claude",
            AgentKind::AgentDeck => ".agentdeck",
        }
    }

    /// Identificador estável para config, log e JSON.
    pub fn as_str(self) -> &'static str {
        match self {
            AgentKind::GarraIa => "garraia",
            AgentKind::Hermes => "hermes",
            AgentKind::OpenClaw => "open_claw",
            AgentKind::ClaudeCode => "claude_code",
            AgentKind::AgentDeck => "agent_deck",
        }
    }
}

/// O que foi encontrado, e onde. Cada variante é um fato observado por
/// leitura — nenhuma delas envolveu rodar coisa alguma.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Evidence {
    /// Um executável com o nome esperado apareceu na `PATH`.
    ///
    /// **Sozinha, esta evidência não confirma nada** — ver a nota sobre o
    /// homônimo npm no topo do módulo.
    ExecutableOnPath { path: PathBuf },
    /// O diretório de config do agente existe no `$HOME`.
    ConfigDirectory { path: PathBuf },
}

impl Evidence {
    /// `true` quando a evidência é específica do agente de verdade, e não
    /// apenas de um nome que qualquer um pode ocupar.
    fn is_corroborating(&self) -> bool {
        match self {
            // Qualquer pacote pode se chamar como quiser.
            Evidence::ExecutableOnPath { .. } => false,
            // O diretório de config é deixado pelo agente ao rodar.
            Evidence::ConfigDirectory { .. } => true,
        }
    }
}

/// Quão certa é a identificação.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Achou o binário e nada mais. Pode ser um homônimo. A UI mostra, o
    /// usuário decide; nada é feito automaticamente.
    Ambiguous,
    /// Binário **e** marca própria do agente. Identidade corroborada.
    Confirmed,
}

/// Um agente encontrado no sistema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectedAgent {
    pub kind: AgentKind,
    pub confidence: Confidence,
    /// Em ordem de descoberta. Nunca vazia — sem evidência não há detecção.
    pub evidence: Vec<Evidence>,
}

impl DetectedAgent {
    /// Caminho do executável, quando foi um deles que apareceu.
    pub fn executable(&self) -> Option<&Path> {
        self.evidence.iter().find_map(|e| match e {
            Evidence::ExecutableOnPath { path } => Some(path.as_path()),
            _ => None,
        })
    }

    /// `true` só quando a identidade foi **corroborada**.
    ///
    /// É a pergunta que o chamador deve fazer antes de tratar o agente como
    /// real. Fail-closed: dúvida responde `false`.
    pub fn is_identified(&self) -> bool {
        self.confidence == Confidence::Confirmed
    }
}

/// Acesso somente-leitura ao sistema de arquivos.
///
/// A superfície é mínima de propósito: duas perguntas, nenhuma resposta que
/// envolva abrir, escrever ou executar. É o que permite testar a detecção sem
/// depender do que está instalado na máquina — e é o que torna
/// *estruturalmente* impossível este módulo executar um binário.
pub trait Filesystem {
    /// O caminho é um arquivo?
    fn is_file(&self, path: &Path) -> bool;
    /// O caminho é um diretório?
    fn is_dir(&self, path: &Path) -> bool;
}

/// Implementação real, sobre `std::fs`.
#[derive(Debug, Clone, Copy, Default)]
pub struct RealFs;

impl Filesystem for RealFs {
    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }
}

/// Varredura de agentes.
///
/// Os diretórios de `PATH` e o `$HOME` são **injetados**, não lidos do
/// ambiente por dentro: teste não deveria depender de variável de ambiente
/// global, e a casca é quem sabe em que usuário está rodando.
/// [`Detector::from_env`] é o atalho para o caso real.
pub struct Detector<F: Filesystem> {
    fs: F,
    path_dirs: Vec<PathBuf>,
    home: Option<PathBuf>,
}

impl<F: Filesystem> Detector<F> {
    pub fn new(fs: F, path_dirs: Vec<PathBuf>, home: Option<PathBuf>) -> Self {
        Self {
            fs,
            path_dirs,
            home,
        }
    }

    /// Varre todos os agentes conhecidos, em ordem estável.
    ///
    /// Só aparecem no resultado os que deixaram alguma evidência. Ausência de
    /// evidência é ausência de agente — não é erro.
    pub fn scan(&self) -> Vec<DetectedAgent> {
        AgentKind::ALL
            .into_iter()
            .filter_map(|kind| self.detect(kind))
            .collect()
    }

    /// Procura um agente específico.
    pub fn detect(&self, kind: AgentKind) -> Option<DetectedAgent> {
        let mut evidence = Vec::new();

        if let Some(path) = self.find_on_path(kind.binary_name()) {
            evidence.push(Evidence::ExecutableOnPath { path });
        }

        if let Some(home) = &self.home {
            let dir = home.join(kind.config_dir());
            if self.fs.is_dir(&dir) {
                evidence.push(Evidence::ConfigDirectory { path: dir });
            }
        }

        if evidence.is_empty() {
            return None;
        }

        // Fail-closed: só sobe para `Confirmed` com evidência que um homônimo
        // não conseguiria forjar por acidente. Na dúvida, `Ambiguous`.
        let confidence = if evidence.iter().any(Evidence::is_corroborating) {
            Confidence::Confirmed
        } else {
            Confidence::Ambiguous
        };

        if confidence == Confidence::Ambiguous {
            tracing::debug!(
                agent = kind.as_str(),
                "binario encontrado na PATH sem marca propria: identidade nao confirmada"
            );
        }

        Some(DetectedAgent {
            kind,
            confidence,
            evidence,
        })
    }

    /// `which` sem crate externo, e sem executar nada.
    ///
    /// Mesma varredura do `which_in_path` da CLI, inclusive as extensões do
    /// Windows — o executável lá carrega sufixo, e procurar só o nome cru não
    /// acharia nada.
    fn find_on_path(&self, binary: &str) -> Option<PathBuf> {
        for dir in &self.path_dirs {
            let candidate = dir.join(binary);
            if self.fs.is_file(&candidate) {
                return Some(candidate);
            }
            for ext in ["exe", "cmd", "bat"] {
                let with_ext = dir.join(format!("{binary}.{ext}"));
                if self.fs.is_file(&with_ext) {
                    return Some(with_ext);
                }
            }
        }
        None
    }
}

impl Detector<RealFs> {
    /// Detector sobre o ambiente real: `PATH` e `$HOME` do processo.
    ///
    /// `PATH` ausente vira lista vazia e `HOME` ausente vira `None` — os dois
    /// degradam para "não achei nada", nunca para panic.
    pub fn from_env() -> Self {
        let path_dirs = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_default();
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from);
        Self::new(RealFs, path_dirs, home)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// Filesystem de mentira: um conjunto de caminhos que "existem".
    #[derive(Default)]
    struct FakeFs {
        files: BTreeSet<PathBuf>,
        dirs: BTreeSet<PathBuf>,
    }

    impl FakeFs {
        fn with_file(mut self, p: &str) -> Self {
            self.files.insert(PathBuf::from(p));
            self
        }
        fn with_dir(mut self, p: &str) -> Self {
            self.dirs.insert(PathBuf::from(p));
            self
        }
    }

    impl Filesystem for FakeFs {
        fn is_file(&self, path: &Path) -> bool {
            self.files.contains(path)
        }
        fn is_dir(&self, path: &Path) -> bool {
            self.dirs.contains(path)
        }
    }

    fn detector(fs: FakeFs) -> Detector<FakeFs> {
        Detector::new(
            fs,
            vec![PathBuf::from("/usr/bin"), PathBuf::from("/opt/bin")],
            Some(PathBuf::from("/home/u")),
        )
    }

    #[test]
    fn nada_instalado_nao_detecta_nada() {
        let d = detector(FakeFs::default());
        assert!(d.scan().is_empty());
        assert_eq!(d.detect(AgentKind::AgentDeck), None);
    }

    /// O invariante central: um nome na `PATH` **não** prova identidade.
    ///
    /// É exatamente o caso do homônimo npm do `agentdeck`, que a CLI recusa em
    /// `agents.rs`. Aqui ele é detectado, mas não confirmado — e
    /// `is_identified()` responde `false`.
    #[test]
    fn binario_na_path_sem_marca_propria_nao_confirma_identidade() {
        let d = detector(FakeFs::default().with_file("/usr/bin/agentdeck"));
        let found = d.detect(AgentKind::AgentDeck).expect("detecta o binario");

        assert_eq!(found.confidence, Confidence::Ambiguous);
        assert!(
            !found.is_identified(),
            "homonimo nao pode passar por agente de verdade"
        );
        assert_eq!(found.executable(), Some(Path::new("/usr/bin/agentdeck")));
    }

    #[test]
    fn binario_mais_config_propria_confirma() {
        let d = detector(
            FakeFs::default()
                .with_file("/usr/bin/agentdeck")
                .with_dir("/home/u/.agentdeck"),
        );
        let found = d.detect(AgentKind::AgentDeck).expect("detecta o agente");

        assert_eq!(found.confidence, Confidence::Confirmed);
        assert!(found.is_identified());
        assert_eq!(found.evidence.len(), 2);
    }

    #[test]
    fn config_sozinha_confirma_mesmo_sem_binario_na_path() {
        // Instalado fora da PATH: existe, está identificado, mas não há
        // caminho de execução conhecido — e isso aparece no resultado.
        let d = detector(FakeFs::default().with_dir("/home/u/.hermes"));
        let found = d.detect(AgentKind::Hermes).expect("detecta pela config");

        assert_eq!(found.confidence, Confidence::Confirmed);
        assert_eq!(found.executable(), None);
    }

    #[test]
    fn sem_home_so_resta_a_evidencia_ambigua() {
        let d = Detector::new(
            FakeFs::default()
                .with_file("/usr/bin/agentdeck")
                .with_dir("/home/u/.agentdeck"),
            vec![PathBuf::from("/usr/bin")],
            None,
        );
        let found = d.detect(AgentKind::AgentDeck).expect("detecta o binario");

        // Sem `$HOME` não há como corroborar — fail-closed.
        assert_eq!(found.confidence, Confidence::Ambiguous);
    }

    #[test]
    fn primeiro_diretorio_da_path_vence() {
        let d = detector(
            FakeFs::default()
                .with_file("/usr/bin/hermes")
                .with_file("/opt/bin/hermes"),
        );
        let found = d.detect(AgentKind::Hermes).expect("detecta o binario");
        assert_eq!(found.executable(), Some(Path::new("/usr/bin/hermes")));
    }

    #[test]
    fn acha_executavel_com_extensao_do_windows() {
        let d = detector(FakeFs::default().with_file("/usr/bin/claude.cmd"));
        let found = d.detect(AgentKind::ClaudeCode).expect("detecta o binario");
        assert_eq!(found.executable(), Some(Path::new("/usr/bin/claude.cmd")));
    }

    #[test]
    fn scan_devolve_em_ordem_estavel_e_so_o_que_existe() {
        let d = detector(
            FakeFs::default()
                .with_file("/usr/bin/garraia")
                .with_dir("/home/u/.garraia")
                .with_file("/usr/bin/agentdeck"),
        );
        let found = d.scan();
        let kinds: Vec<_> = found.iter().map(|a| a.kind).collect();
        assert_eq!(kinds, vec![AgentKind::GarraIa, AgentKind::AgentDeck]);

        assert!(found[0].is_identified());
        assert!(!found[1].is_identified());
    }

    #[test]
    fn deteccao_serializa_com_nomes_estaveis() {
        let d = detector(FakeFs::default().with_file("/usr/bin/openclaw"));
        let found = d.detect(AgentKind::OpenClaw).expect("detecta o binario");
        let json = serde_json::to_value(&found).expect("DetectedAgent e serializavel");

        assert_eq!(json["kind"], "open_claw");
        assert_eq!(json["confidence"], "ambiguous");
        assert_eq!(json["evidence"][0]["kind"], "executable_on_path");
    }

    /// Regra 1 do #1181, verificada no fonte e não na intenção.
    ///
    /// Mesmo espírito do teste que varre o `spinner.rs` atrás de código que
    /// esconde o cursor: a garantia interessante aqui é que **nunca** apareça
    /// um caminho de execução de binário neste módulo, e revisão humana não
    /// pega isso num diff futuro tão bem quanto um teste.
    #[test]
    fn o_modulo_de_deteccao_nunca_executa_binario() {
        let fonte = include_str!("detect.rs");

        // Ignora este próprio teste, que precisa citar os nomes proibidos.
        let ate_o_teste = fonte
            .split("fn o_modulo_de_deteccao_nunca_executa_binario")
            .next()
            .unwrap_or(fonte);

        // Ignora comentário e doc comment: a regra é sobre o que o módulo
        // *faz*, não sobre o que ele explica. Sem isto, documentar a própria
        // proibição quebraria o teste que a defende.
        let corpo: String = ate_o_teste
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");

        for proibido in [
            "std::process",
            "Command::new",
            "process::Command",
            "Stdio",
            ".status()",
            ".output()",
            ".spawn()",
        ] {
            assert!(
                !corpo.contains(proibido),
                "detect.rs nao pode conter `{proibido}`: deteccao e leitura, nunca execucao"
            );
        }
    }
}
