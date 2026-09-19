//! Em que diretório o `git` de uma tool roda — e como esse diretório foi
//! escolhido (issue #1258).
//!
//! ## O defeito
//!
//! `GitDiffTool::run_git_command` e `CodeReviewTool::get_diff` montavam o
//! `Command` **sem `current_dir`**. O git herdava então o diretório de
//! trabalho do *processo do gateway*, nunca o `working_dir` da sessão que
//! pediu a tool. Duas consequências, e a segunda é a que dificulta o
//! diagnóstico:
//!
//! 1. A tool responde sobre o **repositório errado**: uma sessão com
//!    `working_dir` apontando para o projeto A recebia o diff do repositório
//!    onde o gateway subiu — ou "not a git repository", quando aquele CWD não
//!    era repositório nenhum.
//! 2. O resultado **não é reproduzível entre instalações**: o CWD depende de
//!    como o processo subiu — `garra start` num terminal, unidade systemd com
//!    `WorkingDirectory=`, sidecar do desktop, container. O mesmo prompt, na
//!    mesma sessão, dava resposta diferente conforme a forma de iniciar o
//!    processo, e nada no pedido do usuário explicava a diferença.
//!
//! Não é vazamento de disco: `file_path` nesta família vira **pathspec do
//! git**, que só enxerga o repositório em que está. Por isso a issue é P2 e
//! não P1 — o risco é resposta errada e confusão de operador.
//!
//! ## A decisão (#1258)
//!
//! - **Com `working_dir` da sessão**: `Command::current_dir(working_dir)`,
//!   exato.
//! - **Sem `working_dir`** — que no gateway é o caso comum, porque um prompt
//!   de Telegram não traz projeto: mantém o CWD do processo, como hoje, **mas
//!   a resposta passa a dizer de qual repositório ela falou**. Recusar (o
//!   fail-closed que a #1244 adotou para as file tools) quebraria o uso
//!   existente; a resposta errada *silenciosa* era o defeito real.
//!
//! A procedência fica presa ao valor, em vez de num `bool` ou num
//! `Option<PathBuf>` solto: quem formata a resposta não tem como esquecer de
//! dizer de onde o diff veio, porque a própria variante carrega a frase.

use std::path::{Path, PathBuf};

/// O diretório em que o `git` desta chamada vai rodar, com a procedência da
/// escolha — e é a procedência que entra na resposta da tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoDir {
    /// O `working_dir` da sessão. Vira `Command::current_dir`.
    Sessao(PathBuf),
    /// A sessão não tem `working_dir`: o git herda o CWD do processo, como
    /// antes da #1258. O caminho vem resolvido aqui só para poder ser
    /// **nomeado** na resposta; `None` quando o próprio CWD não é legível
    /// (diretório removido debaixo de um processo em execução).
    ProcessoCwd(Option<PathBuf>),
}

impl RepoDir {
    /// Decide a partir do `working_dir` do [`ToolContext`](super::ToolContext).
    ///
    /// `working_dir` vazio ou só com espaço conta como ausente, pelo mesmo
    /// motivo do `FileJail::effective_roots`: `current_dir("")` falharia no
    /// spawn com um erro que não explica nada, e um working_dir em branco não
    /// é um projeto.
    ///
    /// Residual conhecido e deliberado: um `working_dir` **relativo** segue
    /// resolvendo contra o CWD do processo (é o que `Command::current_dir`
    /// faz, e o que `repo_search`, `run_tests` e `bash` já fazem), e a linha
    /// de contexto o mostra como a sessão o declarou. Absolutizar aqui
    /// mudaria o comportamento de três outras tools sem que a issue peça.
    pub fn decidir(working_dir: Option<&str>) -> Self {
        match working_dir.map(str::trim).filter(|w| !w.is_empty()) {
            Some(wd) => Self::Sessao(PathBuf::from(wd)),
            None => Self::ProcessoCwd(std::env::current_dir().ok()),
        }
    }

    /// O que passar para `Command::current_dir`, ou `None` para herdar o CWD
    /// do processo — que é o caso sem `working_dir`, preservado de propósito.
    pub fn cwd_do_git(&self) -> Option<&Path> {
        match self {
            Self::Sessao(dir) => Some(dir.as_path()),
            Self::ProcessoCwd(_) => None,
        }
    }

    /// O diretório e a procedência, sem rótulo: `"/x/y (working_dir da
    /// sessão)"`. Quem formata escolhe o rótulo — texto puro no `git_diff`,
    /// markdown no `code_review`.
    pub fn descricao(&self) -> String {
        match self {
            Self::Sessao(dir) => format!("{} (working_dir da sessão)", dir.display()),
            Self::ProcessoCwd(Some(cwd)) => format!(
                "{} (CWD do processo do gateway — a sessão não tem working_dir)",
                cwd.display()
            ),
            Self::ProcessoCwd(None) => "indeterminado (CWD do processo do gateway ilegível — \
                 a sessão não tem working_dir)"
                .to_string(),
        }
    }

    /// A linha que abre a resposta, no formato `Chave: valor` que o
    /// `format_status` do `git_diff` já usa.
    pub fn linha_de_contexto(&self) -> String {
        format!("Repositório: {}", self.descricao())
    }

    /// A resposta da tool com a linha de contexto na frente.
    ///
    /// Vale também para erro, de propósito: um "No such file or directory"
    /// que não diz **qual** diretório é exatamente o diagnóstico impossível
    /// que a #1258 descreve.
    pub fn com_contexto(&self, corpo: &str) -> String {
        format!("{}\n\n{corpo}", self.linha_de_contexto())
    }
}

/// Um repositório git temporário com um commit inicial e, por cima, uma
/// modificação **não commitada** num arquivo cujo nome é único — é esse nome
/// que prova de qual repositório um diff veio.
///
/// Compartilhada pelos testes do `git_diff` e do `code_review`, que provam a
/// mesma coisa sobre o mesmo defeito raiz (#1258).
///
/// A identidade vai por `-c` na linha de comando em vez de `git config`: a
/// suite não pode depender do `user.name`/`user.email` da máquina que a roda,
/// e em CI não há nenhum. `commit.gpgsign=false` é o mesmo motivo ao
/// contrário — uma máquina que assina por padrão não commitaria sem chave.
#[cfg(test)]
pub(crate) fn repo_git_temporario(marcador: &str, ramo: &str) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().to_path_buf();

    let git = |args: &[&str]| {
        let saida = std::process::Command::new("git")
            .current_dir(&dir)
            .args(args)
            .output()
            .expect("git precisa estar no PATH para este teste");
        assert!(
            saida.status.success(),
            "git {args:?} falhou: {}",
            String::from_utf8_lossy(&saida.stderr)
        );
    };

    git(&["init", "-b", ramo]);
    let alvo = dir.join(format!("{marcador}.txt"));
    std::fs::write(&alvo, "linha original\n").expect("write");
    git(&["add", "."]);
    git(&[
        "-c",
        "user.email=test@test",
        "-c",
        "user.name=t",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-m",
        "inicial",
    ]);
    // A modificação que o `git diff` tem de mostrar.
    std::fs::write(&alvo, "linha alterada\n").expect("write");

    tmp
}

/// `ToolContext` mínimo com o `working_dir` que o teste quiser — o caminho do
/// agente, que é por onde a #1258 pede que a prova passe.
#[cfg(test)]
pub(crate) fn contexto_de_teste(working_dir: Option<&str>) -> super::ToolContext {
    super::ToolContext {
        session_id: "test-1258".into(),
        user_id: None,
        is_heartbeat: false,
        approval: crate::tools::approval::ToolApproval::None,
        working_dir: working_dir.map(str::to_string),
        project_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn working_dir_da_sessao_vence() {
        let repo = RepoDir::decidir(Some("/projetos/a"));
        assert_eq!(repo, RepoDir::Sessao(PathBuf::from("/projetos/a")));
        assert_eq!(repo.cwd_do_git(), Some(Path::new("/projetos/a")));
        assert!(repo.descricao().contains("/projetos/a"));
        assert!(repo.descricao().contains("working_dir da sessão"));
    }

    /// Sem `working_dir` o git continua herdando o CWD (`cwd_do_git` é `None`),
    /// e é justamente por isso que a resposta tem de nomear o diretório.
    #[test]
    fn sem_working_dir_herda_o_cwd_mas_nomeia_ele() {
        let repo = RepoDir::decidir(None);
        assert_eq!(repo.cwd_do_git(), None);
        let cwd = std::env::current_dir().expect("CWD do processo de teste");
        assert!(
            repo.descricao().contains(&cwd.display().to_string()),
            "{}",
            repo.descricao()
        );
        assert!(repo.descricao().contains("não tem working_dir"));
    }

    /// `Some("")` / `Some("  ")` é o mesmo que ausente: `current_dir("")`
    /// falharia no spawn com um erro que não explica nada.
    #[test]
    fn working_dir_vazio_conta_como_ausente() {
        for vazio in ["", "   ", "\t"] {
            let repo = RepoDir::decidir(Some(vazio));
            assert_eq!(
                repo.cwd_do_git(),
                None,
                "{vazio:?} deveria contar como ausente"
            );
        }
    }

    #[test]
    fn com_contexto_prefixa_o_corpo_sem_perder_nada() {
        let repo = RepoDir::Sessao(PathBuf::from("/p/a"));
        let saida = repo.com_contexto("diff --git a/x b/x");
        assert!(saida.starts_with("Repositório: /p/a (working_dir da sessão)"));
        assert!(saida.ends_with("diff --git a/x b/x"));
    }

    /// CWD ilegível não pode virar pânico nem string vazia: a frase diz que
    /// não sabe, o que é informação, e não um caminho inventado.
    #[test]
    fn cwd_ilegivel_ainda_produz_frase_util() {
        let repo = RepoDir::ProcessoCwd(None);
        assert!(repo.descricao().contains("indeterminado"));
        assert!(repo.com_contexto("x").contains("Repositório:"));
    }
}
