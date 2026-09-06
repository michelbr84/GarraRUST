//! Apresentacao da conversa no terminal (#934) e cabecalho compacto de
//! abertura (#935).
//!
//! Puro de proposito, no mesmo molde do `spinner`: nada aqui escreve no
//! terminal nem le o relogio. As funcoes recebem o que precisam e devolvem
//! `String`, o que torna cada linha afirmavel contra um literal em teste —
//! inclusive os caminhos ASCII e sem cor, que ninguem exercita a mao e por
//! isso sao justamente os que quebram sem ninguem ver.

use std::path::Path;

const CYAN: &str = "\x1b[36m";
const GREEN: &str = "\x1b[32m";
const BOLD: &str = "\x1b[1m";
pub(crate) const DIM: &str = "\x1b[2m";
/// Avisos e erros dirigidos ao usuario (o renderer os usa).
pub(crate) const YELLOW: &str = "\x1b[33m";
pub(crate) const RESET: &str = "\x1b[0m";

/// Largura maxima do cabecalho quando o terminal nao diz a dele.
pub const DEFAULT_WIDTH: usize = 72;

/// O que este terminal aguenta.
///
/// **Quem decide** e o [`Capabilities::detect`](super::Capabilities::detect):
/// ate o #942 este tipo tinha um `detect()` proprio que olhava TTY, `NO_COLOR`
/// e `TERM=dumb` mas **nao** o locale, enquanto o spinner olhava o locale — e
/// num terminal com `LANG=C` o usuario via `❯` em UTF-8 ao lado de uma
/// animacao ASCII. Um dono so acabou com o desencontro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    pub unicode: bool,
    pub color: bool,
}

impl Style {
    /// ASCII puro, sem uma unica sequencia de escape. E o que sai quando a
    /// saida esta redirecionada para arquivo ou pipe.
    pub const PLAIN: Self = Self {
        unicode: false,
        color: false,
    };

    /// Unicode com cor — o terminal interativo comum.
    pub const RICH: Self = Self {
        unicode: true,
        color: true,
    };

    /// Marcador do input do usuario. Substitui o antigo `voce >`.
    pub fn user_prompt(&self) -> String {
        let marker = if self.unicode { "❯" } else { ">" };
        if self.color {
            format!("{GREEN}{BOLD}{marker}{RESET} ")
        } else {
            format!("{marker} ")
        }
    }

    /// Rotulo da resposta, escrito uma unica vez imediatamente antes do
    /// primeiro delta (o contrato do `stream_turn`).
    ///
    /// Vai numa linha propria, com uma linha em branco antes: o antigo
    /// `garra > ` prefixava a resposta na mesma linha, e quando a resposta
    /// tinha varios paragrafos so a primeira linha ficava identificada. A
    /// linha em branco tambem absorve a linha que o spinner acabou de limpar.
    pub fn assistant_prefix(&self) -> String {
        if self.color {
            format!("\n{CYAN}{BOLD}Garra{RESET}\n")
        } else {
            "\nGarra\n".to_string()
        }
    }

    fn separator(&self) -> &'static str {
        if self.unicode { " · " } else { " | " }
    }

    fn rule_char(&self) -> char {
        if self.unicode { '─' } else { '-' }
    }

    fn ellipsis(&self) -> &'static str {
        if self.unicode { "…" } else { "..." }
    }
}

/// Largura util do terminal, best-effort — **fonte unica** desde o #942.
///
/// Ate aqui havia duas: esta, que lia `COLUMNS`, e a do spinner, que
/// perguntava ao `console::Term`. A do spinner era a boa (funciona sem o
/// shell exportar nada), entao ela vem primeiro e o `COLUMNS` fica como
/// segunda opiniao — util quando a saida nao e um terminal mas o usuario
/// quer controlar a largura.
///
/// Nao e exato, e nem precisa ser: a regua do cabecalho nunca passa do
/// conteudo, e o `shorten_path` ja limita o caminho em 32 chars.
pub fn terminal_width() -> usize {
    if let Some((_, cols)) = console::Term::stdout().size_checked()
        && cols as usize >= 20
    {
        return cols as usize;
    }
    std::env::var("COLUMNS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|w| *w >= 20)
        .unwrap_or(DEFAULT_WIDTH)
}

/// Cabecalho compacto de abertura (#935).
///
/// Tres linhas no lugar do quadro de doze com o mascote — que nao sumiu, so
/// deixou de ser cobrado em toda abertura (`garra about` mostra ele inteiro).
#[derive(Debug, Clone)]
pub struct Header {
    pub version: String,
    pub model: String,
    pub mode: String,
    /// Ja encurtado (ver `banner::shorten_path`).
    pub cwd: String,
    pub branch: Option<String>,
    pub project: Option<String>,
}

impl Header {
    pub fn render(&self, style: Style, width: usize) -> String {
        let sep = style.separator();

        let linha1 = format!(
            "GarraIA {}{sep}{}{sep}{}",
            self.version, self.model, self.mode
        );

        let mut partes2 = vec![self.cwd.clone()];
        if let Some(branch) = &self.branch {
            partes2.push(branch.clone());
        }
        if let Some(project) = &self.project {
            partes2.push(project.clone());
        }
        let linha2 = partes2.join(sep);

        let linha1 = truncate(&linha1, width, style.ellipsis());
        let linha2 = truncate(&linha2, width, style.ellipsis());

        // A regua acompanha o conteudo e nunca passa da largura pedida: e o
        // que impede a borda quebrada em terminal estreito.
        let largura_regua = linha1
            .chars()
            .count()
            .max(linha2.chars().count())
            .min(width);
        let regua: String = std::iter::repeat_n(style.rule_char(), largura_regua).collect();

        if style.color {
            format!("{BOLD}{linha1}{RESET}\n{DIM}{linha2}{RESET}\n{DIM}{regua}{RESET}\n")
        } else {
            format!("{linha1}\n{linha2}\n{regua}\n")
        }
    }
}

/// Corta em `width` caracteres, sinalizando o corte.
fn truncate(s: &str, width: usize, ellipsis: &str) -> String {
    if s.chars().count() <= width {
        return s.to_string();
    }
    let reservado = ellipsis.chars().count();
    let manter = width.saturating_sub(reservado);
    let cabeca: String = s.chars().take(manter).collect();
    format!("{cabeca}{ellipsis}")
}

/// Ramo do git lido direto do `.git/HEAD` — sem subprocesso.
///
/// Chamar `git` no boot custaria um fork por abertura de chat, e o dado esta
/// num arquivo de uma linha. `HEAD` desanexado devolve o sha curto, que e o
/// que o usuario precisa ver para saber onde esta.
pub fn git_branch(dir: &Path) -> Option<String> {
    let git = dir.join(".git");

    // Em worktree e submodulo, `.git` e um arquivo com `gitdir: <caminho>`.
    let git_dir = if git.is_file() {
        let conteudo = std::fs::read_to_string(&git).ok()?;
        let apontado = conteudo.strip_prefix("gitdir:")?.trim();
        let caminho = Path::new(apontado);
        if caminho.is_absolute() {
            caminho.to_path_buf()
        } else {
            dir.join(caminho)
        }
    } else if git.is_dir() {
        git
    } else {
        return None;
    };

    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();

    if let Some(referencia) = head.strip_prefix("ref: refs/heads/") {
        return Some(referencia.to_string());
    }

    // HEAD desanexado: o proprio sha.
    if head.len() >= 7 && head.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(head[..7].to_string());
    }

    None
}

/// Marcadores de tipo de projeto no diretorio.
///
/// So os marcadores — a listagem de arquivos que acompanhava isso saiu do
/// cabecalho (#935) e continua indo para o prompt do sistema, onde ela
/// realmente serve para alguma coisa.
pub fn project_markers(dir: &Path) -> Vec<&'static str> {
    let mut markers = Vec::new();

    if dir.join("Cargo.toml").exists() {
        markers.push("Rust");
    }
    if dir.join("package.json").exists() {
        markers.push("Node.js");
    }
    if dir.join("pyproject.toml").exists() || dir.join("requirements.txt").exists() {
        markers.push("Python");
    }
    if dir.join("pubspec.yaml").exists() {
        markers.push("Flutter/Dart");
    }
    if dir.join("go.mod").exists() {
        markers.push("Go");
    }
    if dir.join("pom.xml").exists() || dir.join("build.gradle").exists() {
        markers.push("Java/Kotlin");
    }
    if dir.join("Dockerfile").exists() || dir.join("docker-compose.yml").exists() {
        markers.push("Docker");
    }

    markers
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> Header {
        Header {
            version: "0.3.9".to_string(),
            model: "openrouter/auto".to_string(),
            mode: "cloud".to_string(),
            cwd: "~/GarraRUST".to_string(),
            branch: Some("main".to_string()),
            project: Some("Rust".to_string()),
        }
    }

    #[test]
    fn rich_header_matches_the_mockup_of_the_epic() {
        let saida = header().render(Style::RICH, DEFAULT_WIDTH);
        let linhas: Vec<&str> = saida.lines().collect();

        assert_eq!(linhas.len(), 3, "o cabecalho tem tres linhas: {saida:?}");
        assert!(linhas[0].contains("GarraIA 0.3.9 · openrouter/auto · cloud"));
        assert!(linhas[1].contains("~/GarraRUST · main · Rust"));
        assert!(linhas[2].contains('─'));
    }

    #[test]
    fn plain_header_has_no_escape_sequence_and_no_unicode() {
        let saida = header().render(Style::PLAIN, DEFAULT_WIDTH);

        assert!(!saida.contains('\x1b'), "vazou escape ANSI: {saida:?}");
        assert!(saida.is_ascii(), "vazou caractere nao-ASCII: {saida:?}");
        assert!(saida.contains("GarraIA 0.3.9 | openrouter/auto | cloud"));
        assert!(saida.contains("---"));
    }

    /// Criterio de aceite do #935: terminal estreito nao pode quebrar a borda.
    #[test]
    fn narrow_terminal_never_exceeds_the_requested_width() {
        for largura in [20usize, 24, 32, 40] {
            let saida = header().render(Style::PLAIN, largura);
            for linha in saida.lines() {
                assert!(
                    linha.chars().count() <= largura,
                    "largura {largura}: linha com {} chars: {linha:?}",
                    linha.chars().count()
                );
            }
        }
    }

    #[test]
    fn header_omits_absent_branch_and_project() {
        let mut h = header();
        h.branch = None;
        h.project = None;
        let saida = h.render(Style::PLAIN, DEFAULT_WIDTH);

        let linha2 = saida.lines().nth(1).expect("segunda linha");
        assert_eq!(linha2, "~/GarraRUST", "sem separador solto: {linha2:?}");
    }

    /// #934: nada de `voce >`, e o fallback continua utilizavel.
    #[test]
    fn user_prompt_swaps_marker_but_keeps_ascii_usable() {
        assert!(Style::RICH.user_prompt().contains('❯'));
        assert!(!Style::RICH.user_prompt().contains("voce"));

        let plano = Style::PLAIN.user_prompt();
        assert_eq!(plano, "> ");
        assert!(!plano.contains('\x1b'));
    }

    /// O rotulo da resposta vai numa linha propria: resposta de varios
    /// paragrafos tinha so a primeira linha identificada com `garra > `.
    #[test]
    fn assistant_prefix_is_a_block_label_on_its_own_line() {
        let rico = Style::RICH.assistant_prefix();
        assert!(rico.starts_with('\n'), "linha em branco antes: {rico:?}");
        assert!(rico.ends_with('\n'), "resposta comeca na linha seguinte");
        assert!(rico.contains("Garra"));

        assert_eq!(Style::PLAIN.assistant_prefix(), "\nGarra\n");
    }

    #[test]
    fn truncation_marks_the_cut() {
        assert_eq!(truncate("abcdefgh", 5, "..."), "ab...");
        assert_eq!(truncate("abc", 5, "..."), "abc");
        assert_eq!(truncate("abcdefgh", 5, "…"), "abcd…");
    }

    #[test]
    fn reads_branch_from_a_plain_git_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let git = tmp.path().join(".git");
        std::fs::create_dir_all(&git).expect("mkdir");
        std::fs::write(git.join("HEAD"), "ref: refs/heads/claude/algo-longo\n").expect("write");

        assert_eq!(
            git_branch(tmp.path()),
            Some("claude/algo-longo".to_string())
        );
    }

    #[test]
    fn detached_head_reports_the_short_sha() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let git = tmp.path().join(".git");
        std::fs::create_dir_all(&git).expect("mkdir");
        std::fs::write(
            git.join("HEAD"),
            "9f39dfe1b65a1c2d3e4f5061728394a5b6c7d8e9\n",
        )
        .expect("write");

        assert_eq!(git_branch(tmp.path()), Some("9f39dfe".to_string()));
    }

    /// Worktree e submodulo tem `.git` como ARQUIVO apontando para outro
    /// lugar. Sem seguir o ponteiro, quem usa `git worktree` perderia o ramo
    /// no cabecalho sem entender por que.
    #[test]
    fn follows_the_gitdir_pointer_of_a_worktree() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let real = tmp.path().join("real-git-dir");
        std::fs::create_dir_all(&real).expect("mkdir");
        std::fs::write(real.join("HEAD"), "ref: refs/heads/feature\n").expect("write");

        let work = tmp.path().join("work");
        std::fs::create_dir_all(&work).expect("mkdir");
        std::fs::write(work.join(".git"), format!("gitdir: {}\n", real.display())).expect("write");

        assert_eq!(git_branch(&work), Some("feature".to_string()));
    }

    #[test]
    fn no_git_no_branch() {
        let tmp = tempfile::tempdir().expect("tempdir");
        assert_eq!(git_branch(tmp.path()), None);
    }

    #[test]
    fn detects_project_markers_without_listing_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("Cargo.toml"), "[package]").expect("write");
        std::fs::write(tmp.path().join("Dockerfile"), "FROM scratch").expect("write");
        std::fs::write(tmp.path().join("segredo.txt"), "nao deve aparecer").expect("write");

        let markers = project_markers(tmp.path());

        assert_eq!(markers, vec!["Rust", "Docker"]);
        assert!(
            !markers.iter().any(|m| m.contains("segredo")),
            "o cabecalho nao lista arquivos (#935)"
        );
    }
}
