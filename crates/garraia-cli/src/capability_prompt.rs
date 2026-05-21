//! GAR-495 — Capability prompt nativo.
//!
//! Builds a provider-agnostic description of what the Garra runtime exposes —
//! LLM providers, built-in tools, channels, MCP servers — from the loaded
//! `AppConfig`. Used by `garra max-power` to give situational awareness before
//! routing a goal to the appropriate pipeline stage.

use garraia_config::AppConfig;

/// Static built-in tool registry: (name, one-line description).
/// Mirrors the tool modules in `garraia-agents/src/tools/`.
const BUILTIN_TOOLS: &[(&str, &str)] = &[
    (
        "bash",
        "Execute shell commands with safety-gate enforcement",
    ),
    ("file_read", "Read files from the local filesystem"),
    ("file_write", "Write or overwrite files in the workspace"),
    ("git_diff", "Show git diff for staged/unstaged changes"),
    (
        "list_dir",
        "List directory contents with optional glob filter",
    ),
    ("repo_search", "Full-text search across the repository"),
    (
        "run_tests",
        "Run `cargo test` (or `flutter test`) and return output",
    ),
    ("web_fetch", "Fetch a URL and return its text content"),
    ("web_search", "Web search via configured search provider"),
    (
        "code_review",
        "Automated code review with style/correctness checks",
    ),
];

/// LLM provider entry derived from `AppConfig.llm`.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderInfo {
    /// Alias key from the config (e.g. `"anthropic"`, `"openai"`, `"local"`).
    pub name: String,
    /// Provider type string from the config (e.g. `"anthropic"`, `"openai"`, `"ollama"`).
    pub provider_type: String,
    /// Optional model override.
    pub model: Option<String>,
}

/// Snapshot of everything the Garra runtime can do, derived from `AppConfig`.
#[derive(Debug, Clone)]
pub struct CapabilitySnapshot {
    pub providers: Vec<ProviderInfo>,
    pub builtin_tools: Vec<(&'static str, &'static str)>,
    pub channels: Vec<String>,
    pub mcp_servers: Vec<String>,
}

/// Build a capability snapshot from the loaded config. Pure function, no I/O.
pub fn build_snapshot(config: &AppConfig) -> CapabilitySnapshot {
    let mut providers: Vec<ProviderInfo> = config
        .llm
        .iter()
        .map(|(name, cfg)| ProviderInfo {
            name: name.clone(),
            provider_type: cfg.provider.clone(),
            model: cfg.model.clone(),
        })
        .collect();
    providers.sort_by(|a, b| a.name.cmp(&b.name));

    let mut channels: Vec<String> = config.channels.keys().cloned().collect();
    channels.sort();

    let mut mcp_servers: Vec<String> = config.mcp.keys().cloned().collect();
    mcp_servers.sort();

    CapabilitySnapshot {
        providers,
        builtin_tools: BUILTIN_TOOLS.to_vec(),
        channels,
        mcp_servers,
    }
}

/// Render the snapshot into a provider-agnostic, human-readable prompt string.
pub fn render_prompt(snap: &CapabilitySnapshot) -> String {
    let mut out = String::new();

    out.push_str("=== Garra Capability Snapshot ===\n\n");

    // LLM providers
    if snap.providers.is_empty() {
        out.push_str("LLM Providers: none configured\n");
    } else {
        out.push_str(&format!("LLM Providers ({}):\n", snap.providers.len()));
        for p in &snap.providers {
            let model_str = p
                .model
                .as_deref()
                .map(|m| format!(" [model: {m}]"))
                .unwrap_or_default();
            out.push_str(&format!(
                "  - {} (type: {}){}\n",
                p.name, p.provider_type, model_str
            ));
        }
    }
    out.push('\n');

    // Built-in tools
    out.push_str(&format!("Built-in Tools ({}):\n", snap.builtin_tools.len()));
    for (name, desc) in &snap.builtin_tools {
        out.push_str(&format!("  - {name}: {desc}\n"));
    }
    out.push('\n');

    // Channels
    if snap.channels.is_empty() {
        out.push_str("Channels: none configured\n");
    } else {
        out.push_str(&format!("Channels ({}):\n", snap.channels.len()));
        for ch in &snap.channels {
            out.push_str(&format!("  - {ch}\n"));
        }
    }
    out.push('\n');

    // MCP servers
    if snap.mcp_servers.is_empty() {
        out.push_str("MCP Servers: none configured\n");
    } else {
        out.push_str(&format!("MCP Servers ({}):\n", snap.mcp_servers.len()));
        for srv in &snap.mcp_servers {
            out.push_str(&format!("  - {srv}\n"));
        }
    }

    out
}

/// One-line summary suitable for inline display (e.g. in the `max-power` banner).
pub fn render_summary(snap: &CapabilitySnapshot) -> String {
    format!(
        "{} provider(s) | {} tools | {} channel(s) | {} MCP server(s)",
        snap.providers.len(),
        snap.builtin_tools.len(),
        snap.channels.len(),
        snap.mcp_servers.len(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use garraia_config::{AppConfig, LlmProviderConfig};
    use std::collections::HashMap;

    fn provider(provider_type: &str, model: Option<&str>) -> LlmProviderConfig {
        LlmProviderConfig {
            provider: provider_type.to_string(),
            model: model.map(str::to_string),
            api_key: None,
            base_url: None,
            extra: HashMap::new(),
        }
    }

    #[test]
    fn empty_config_renders_without_panic() {
        let config = AppConfig::default();
        let snap = build_snapshot(&config);
        assert!(snap.providers.is_empty());
        assert!(!snap.builtin_tools.is_empty());
        assert!(snap.channels.is_empty());
        assert!(snap.mcp_servers.is_empty());

        let prompt = render_prompt(&snap);
        assert!(prompt.contains("none configured"));
        assert!(prompt.contains("Built-in Tools"));
    }

    #[test]
    fn anthropic_only_config() {
        let mut config = AppConfig::default();
        config.llm.insert(
            "anthropic".to_string(),
            provider("anthropic", Some("claude-sonnet-4-6")),
        );

        let snap = build_snapshot(&config);
        assert_eq!(snap.providers.len(), 1);
        assert_eq!(snap.providers[0].name, "anthropic");
        assert_eq!(snap.providers[0].provider_type, "anthropic");
        assert_eq!(
            snap.providers[0].model.as_deref(),
            Some("claude-sonnet-4-6")
        );

        let prompt = render_prompt(&snap);
        assert!(prompt.contains("anthropic"));
        assert!(prompt.contains("claude-sonnet-4-6"));

        let summary = render_summary(&snap);
        assert!(summary.starts_with("1 provider(s)"));
    }

    #[test]
    fn openai_plus_ollama_config() {
        let mut config = AppConfig::default();
        config
            .llm
            .insert("openai".to_string(), provider("openai", Some("gpt-4o")));
        config
            .llm
            .insert("local".to_string(), provider("ollama", Some("llama3.2")));

        let snap = build_snapshot(&config);
        assert_eq!(snap.providers.len(), 2);

        // Providers sorted alphabetically by name
        assert_eq!(snap.providers[0].name, "local");
        assert_eq!(snap.providers[1].name, "openai");

        let prompt = render_prompt(&snap);
        assert!(prompt.contains("ollama"));
        assert!(prompt.contains("gpt-4o"));

        let summary = render_summary(&snap);
        assert!(summary.starts_with("2 provider(s)"));
    }

    #[test]
    fn all_three_providers_config() {
        let mut config = AppConfig::default();
        config
            .llm
            .insert("anthropic".to_string(), provider("anthropic", None));
        config
            .llm
            .insert("openai".to_string(), provider("openai", None));
        config
            .llm
            .insert("ollama".to_string(), provider("ollama", Some("mistral")));

        let snap = build_snapshot(&config);
        assert_eq!(snap.providers.len(), 3);
        let summary = render_summary(&snap);
        assert!(summary.starts_with("3 provider(s)"));
    }

    #[test]
    fn channels_and_mcp_appear_in_snapshot() {
        let config: AppConfig = serde_yaml::from_str(
            r#"
channels:
  telegram:
    type: telegram
    enabled: true
  discord:
    type: discord
    enabled: true
mcp:
  filesystem:
    command: npx
"#,
        )
        .expect("parse test config");

        let snap = build_snapshot(&config);
        assert_eq!(snap.channels.len(), 2);
        assert_eq!(snap.mcp_servers.len(), 1);
        // Channels sorted alphabetically
        assert_eq!(snap.channels[0], "discord");
        assert_eq!(snap.channels[1], "telegram");

        let prompt = render_prompt(&snap);
        assert!(prompt.contains("telegram"));
        assert!(prompt.contains("filesystem"));
    }

    #[test]
    fn builtin_tools_count_is_stable() {
        let config = AppConfig::default();
        let snap = build_snapshot(&config);
        assert_eq!(snap.builtin_tools.len(), BUILTIN_TOOLS.len());
        // All tool names must be non-empty
        for (name, desc) in &snap.builtin_tools {
            assert!(!name.is_empty());
            assert!(!desc.is_empty());
        }
    }

    #[test]
    fn render_summary_format() {
        let config = AppConfig::default();
        let snap = build_snapshot(&config);
        let s = render_summary(&snap);
        // Must contain all four segments
        assert!(s.contains("provider(s)"));
        assert!(s.contains("tools"));
        assert!(s.contains("channel(s)"));
        assert!(s.contains("MCP server(s)"));
    }
}
