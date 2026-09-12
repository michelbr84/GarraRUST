use std::path::Path;

use garraia_config::AppConfig;

/// Maximum on-screen width for `Config:` / `CWD:` paths in the banner.
/// Keeps the right-hand column readable on a default 80-col terminal.
const MAX_PATH_DISPLAY_WIDTH: usize = 32;

/// Render a path for display in the banner:
/// - Collapse the user's home dir to `~` on Windows and Unix (the old
///   implementation only honoured POSIX `$HOME` and broke on Windows).
/// - If the result is longer than [`MAX_PATH_DISPLAY_WIDTH`], replace the
///   middle with `…` so the head and tail stay visible.
pub(crate) fn shorten_path(p: &Path) -> String {
    let raw = p.to_string_lossy().into_owned();

    // Home truncation. `dirs::home_dir()` returns the correct value on
    // both Windows (USERPROFILE) and Unix (HOME).
    let collapsed = match dirs::home_dir() {
        Some(home) if !home.as_os_str().is_empty() => {
            let home_str = home.to_string_lossy();
            // Only collapse when the path actually *starts* with home — avoid
            // accidentally rewriting something like `/foo/<home>/bar`.
            if !home_str.is_empty() && raw.starts_with(home_str.as_ref()) {
                let tail = &raw[home_str.len()..];
                if tail.is_empty() {
                    "~".to_string()
                } else {
                    format!("~{tail}")
                }
            } else {
                raw
            }
        }
        _ => raw,
    };

    if collapsed.chars().count() <= MAX_PATH_DISPLAY_WIDTH {
        return collapsed;
    }

    // Middle-ellipsis. Keep head ~ 12 chars and tail ~ 16 chars so the
    // last segment (usually the leaf folder) stays visible.
    let head_n = 12;
    let tail_n = MAX_PATH_DISPLAY_WIDTH.saturating_sub(head_n + 1); // 1 char for '…'
    let chars: Vec<char> = collapsed.chars().collect();
    let head: String = chars.iter().take(head_n).collect();
    let tail: String = chars
        .iter()
        .skip(chars.len().saturating_sub(tail_n))
        .collect();
    format!("{head}…{tail}")
}

/// Renderiza a tela `garra about` a partir das capacidades do terminal.
///
/// Pura de proposito, no mesmo molde do `ui/conversation` (#942): nada aqui
/// escreve no terminal, e cada linha e afirmavel contra literal em teste —
/// inclusive o caminho plain, que ninguem exercita a mao e por isso quebrava
/// em silencio: a versao anterior escrevia ANSI incondicional, e um
/// `garra about > arquivo` (ou pipe, `NO_COLOR`, `TERM=dumb`) recebia
/// sequencias de escape cruas.
pub fn about_text(style: crate::ui::Style) -> String {
    let version = env!("CARGO_PKG_VERSION");

    // Cores e desenho derivam do Style — o dono unico da decisao e
    // `ui::Capabilities::detect()`, nao este modulo.
    let (cyan, yellow, green, bold, dim, reset) = if style.color {
        (
            "\x1b[36m", "\x1b[33m", "\x1b[32m", "\x1b[1m", "\x1b[2m", "\x1b[0m",
        )
    } else {
        ("", "", "", "", "", "")
    };
    let (tl, tr, bl, br, h, v) = if style.unicode {
        ('╭', '╮', '╰', '╯', '─', '│')
    } else {
        ('+', '+', '+', '+', '-', '|')
    };
    // Largura interna do quadro (46 colunas entre as bordas), herdada da
    // versao anterior; `edge` monta as bordas e `pad` as linhas vazias.
    let bar = h.to_string().repeat(46);
    let edge = |l: char, r: char| format!("{cyan}{bold}{l}{bar}{r}{reset}\n");
    let pad = format!(
        "{cyan}{bold}{v}{reset}                                              {cyan}{bold}{v}{reset}"
    );

    let mut out = String::new();
    out.push('\n');
    out.push_str(&edge(tl, tr));
    out.push_str(&format!("{pad}\n"));
    out.push_str(&format!("{cyan}{bold}{v}{reset}      {yellow}{bold}_~^~^~_{reset}                                {cyan}{bold}{v}{reset}\n"));
    out.push_str(&format!("{cyan}{bold}{v}{reset}   {yellow}{bold}\\) /  o o  \\ (/{reset}   {green}{bold}GarraIA v{version}{reset}         {cyan}{bold}{v}{reset}\n"));
    out.push_str(&format!("{cyan}{bold}{v}{reset}     {yellow}{bold}'_   -   _'{reset}    Personal AI Assistant   {cyan}{bold}{v}{reset}\n"));
    out.push_str(&format!("{cyan}{bold}{v}{reset}     {yellow}{bold}/ '-----' \\{reset}                            {cyan}{bold}{v}{reset}\n"));
    out.push_str(&format!("{pad}\n"));
    out.push_str(&edge(bl, br));
    out.push('\n');
    out.push_str(&format!(
        "  {dim}Assistente de IA pessoal, escrito em Rust.{reset}\n"
    ));
    out.push_str(&format!(
        "  {dim}Tudo local: conversas, memoria, config e credenciais.{reset}\n"
    ));
    out.push('\n');
    out.push_str(&format!("  {bold}garra{reset}          conversar\n"));
    out.push_str(&format!("  {bold}garra start{reset}    subir o gateway\n"));
    out.push_str(&format!(
        "  {bold}garra doctor{reset}   diagnosticar a instalacao\n"
    ));
    out.push_str(&format!(
        "  {bold}garra --help{reset}   todos os comandos\n"
    ));
    out.push('\n');
    out
}

/// A marca inteira, sob demanda (#935).
///
/// O mascote saiu da abertura do `garra chat`, que passou a ser um cabecalho
/// de tres linhas — ele ocupava doze linhas de terminal em toda sessao para
/// dizer o que cabe em duas. Continua aqui, atras de um comando explicito,
/// porque identidade de produto nao se joga fora: so deixa de ser cobrada de
/// quem so quer conversar.
pub fn print_about() {
    // O dono unico da decisao (#942): pipe/arquivo, `NO_COLOR` e `TERM=dumb`
    // recebem o caminho plain (ASCII, sem escape nenhum).
    print!("{}", about_text(crate::ui::Capabilities::detect().style()));
}

/// Print the startup banner with Ferris and config summary.
pub fn print_banner(host: &str, port: u16, config: &AppConfig, config_dir: &Path) {
    let version = env!("CARGO_PKG_VERSION");

    // Gather info.
    //
    // Name the provider *and* say whether it will actually come up. The banner
    // used to print the configured name unconditionally, so an operator whose
    // key resolved nowhere saw a confident "Provider   main" immediately above
    // a boot log line saying that very provider had been skipped.
    let provider = match config
        .agent
        .default_provider
        .as_deref()
        .or_else(|| config.llm.keys().next().map(|s| s.as_str()))
    {
        None => "none".to_string(),
        Some(key) => match config.llm.get(key) {
            None => format!("{key} ⚠ not in llm:"),
            Some(entry) => {
                if garraia_config::resolve_provider_key_source(
                    &entry.provider,
                    entry.api_key.as_deref(),
                )
                .is_resolved()
                {
                    key.to_string()
                } else {
                    format!("{key} ⚠ no API key")
                }
            }
        },
    };

    // Which config file is actually in force. `ConfigLoader::load` prefers
    // `config.yml` and silently ignores `config.toml` when both exist, so
    // showing only the directory left operators editing a file the gateway
    // never reads.
    let config_file = if config_dir.join("config.yml").exists() {
        "config.yml"
    } else if config_dir.join("config.toml").exists() {
        "config.toml"
    } else {
        "none (using defaults)"
    };

    let channels = if config.channels.is_empty() {
        "none".to_string()
    } else {
        let mut names: Vec<_> = config.channels.keys().cloned().collect();
        names.sort();
        names.join(", ")
    };

    let skill_count = config_dir
        .join("skills")
        .read_dir()
        .map(|rd| {
            rd.filter(|e| {
                e.as_ref()
                    .map(|e| e.path().extension().is_some_and(|x| x == "md"))
                    .unwrap_or(false)
            })
            .count()
        })
        .unwrap_or(0);
    let skills = if skill_count == 0 {
        "none".to_string()
    } else {
        format!("{skill_count} loaded")
    };

    let mcp_count = config.mcp.len();
    let mcp = if mcp_count == 0 {
        "none".to_string()
    } else {
        format!(
            "{mcp_count} server{}",
            if mcp_count == 1 { "" } else { "s" }
        )
    };

    let url = format!("http://{host}:{port}");
    let config_display = shorten_path(config_dir);
    let cwd_display = std::env::current_dir()
        .as_deref()
        .map(shorten_path)
        .unwrap_or_else(|_| "?".to_string());

    // Layout
    let width = 70;
    let left_w = 33;
    let right_w = width - left_w - 3; // 3 for "│ " + "│"

    let title = format!("GarraIA v{version}");
    let title_dashes = width - 2 - title.len() - 5; // 2 for ╭╮, 5 for "─── " + " "
    let top = format!("╭─── {title} {}╮", "─".repeat(title_dashes));
    let bottom = format!("╰{}╯", "─".repeat(width - 2));

    let row = |l: &str, r: &str| format!("│ {:<left_w$}│  {:<right_w$}│", l, r);

    println!("{top}");
    println!("{}", row("", ""));
    println!("{}", row("  Oi! Eu sou o Garra 🐾", "Gateway"));
    println!("{}", row("", &url));
    println!("{}", row("      _~^~^~_", &"─".repeat(right_w - 2)));
    println!(
        "{}",
        row("  \\) /  o o  \\ (/", &format!("Provider    {provider}"))
    );
    println!(
        "{}",
        row("    '_   -   _'", &format!("Channels    {channels}"))
    );
    println!(
        "{}",
        row("    / '-----' \\", &format!("Skills      {skills}"))
    );
    println!("{}", row("", &format!("MCP         {mcp}")));
    println!("{}", row("  Seu assistente pessoal", ""));
    println!("{}", row("", &format!("Config      {config_display}")));
    println!("{}", row("", &format!("File        {config_file}")));
    println!("{}", row("", &format!("CWD         {cwd_display}")));
    println!("{}", row("", "Press Ctrl+C to stop"));
    println!("{}", row("", ""));
    println!("{bottom}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn shorten_path_no_op_for_short_paths() {
        let p = PathBuf::from("/tmp/x");
        let s = shorten_path(&p);
        assert_eq!(s, "/tmp/x");
    }

    #[test]
    fn shorten_path_collapses_home() {
        let home = dirs::home_dir().expect("test env should have a home dir");
        let inside = home.join("garraia");
        let s = shorten_path(&inside);
        assert!(
            s.starts_with('~'),
            "expected leading '~', got `{s}` (home was `{}`)",
            home.display()
        );
        assert!(s.ends_with("garraia"), "expected trailing leaf, got `{s}`");
    }

    #[test]
    fn shorten_path_truncates_long_paths_with_ellipsis() {
        // Path that's definitely longer than MAX_PATH_DISPLAY_WIDTH = 32 chars
        // and does not start with the user's home (so the home-collapse branch
        // does not trigger).
        let p =
            PathBuf::from("/var/lib/some/deeply/nested/garraia-config-directory-that-is-very-long");
        let s = shorten_path(&p);
        assert!(
            s.chars().count() <= MAX_PATH_DISPLAY_WIDTH,
            "shortened path too long: `{s}` ({} chars)",
            s.chars().count()
        );
        assert!(
            s.contains('…'),
            "expected ellipsis in shortened path, got `{s}`"
        );
    }

    /// O débito que motivou o `about_text` (#942, TODO.md): a versão anterior
    /// escrevia ANSI incondicional, e `garra about > arquivo` recebia escapes
    /// crus. O caminho plain é ASCII puro, sem uma única sequência de escape.
    #[test]
    fn about_plain_has_no_ansi_and_ascii_box() {
        let s = about_text(crate::ui::Style::PLAIN);
        assert!(
            !s.contains('\x1b'),
            "plain path must not emit escape sequences, got `{s}`"
        );
        assert!(s.contains('+'), "plain box uses ASCII corners, got `{s}`");
        assert!(s.contains('-'), "plain box uses ASCII rules, got `{s}`");
        assert!(s.contains("GarraIA v"), "version line missing");
    }

    /// O terminal interativo comum continua recebendo cor e o quadro unicode.
    #[test]
    fn about_rich_uses_color_and_unicode_box() {
        let s = about_text(crate::ui::Style::RICH);
        assert!(s.contains("\x1b[36m"), "rich path colors the frame cyan");
        assert!(s.contains('╭'), "rich box uses unicode corners, got `{s}`");
        assert!(s.contains("GarraIA v"), "version line missing");
    }
}
