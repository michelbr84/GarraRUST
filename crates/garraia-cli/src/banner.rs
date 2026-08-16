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
}
