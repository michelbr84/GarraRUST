//! Config validation and precedence reporting.
//!
//! Plan 0035 (GAR-379 slice 1): produces a structured diagnostic of the
//! configuration currently in effect without revealing secrets. Callers
//! (the CLI `garraia config check` command) print a human-readable or
//! JSON summary and translate [`Severity`] into a sysexits exit code:
//!
//! - `0` — no errors, no warnings (or warnings without `--strict`).
//! - `2` — at least one [`Severity::Error`] (or a [`Severity::Warning`]
//!   in `--strict` mode).
//! - `65` (sysexits `EX_DATAERR`) — handled by the CLI when
//!   [`ConfigLoader::load`] itself returns a parse error (not this module's
//!   responsibility).
//!
//! # Redaction invariant
//!
//! This module MUST NOT serialize secret material. Specifically it never
//! emits values of `gateway.api_key`, `llm.*.api_key`, or
//! `embeddings.*.api_key`; it only reports presence via a boolean field
//! (`"api_key_set": true`). The JSON output should be safe to paste into
//! a bug report.

use std::path::PathBuf;

use serde::Serialize;

use crate::loader::ConfigLoader;
use crate::model::AppConfig;

/// Severity of a single validation finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Violates a hard invariant. Always produces a non-zero exit.
    Error,
    /// Heuristic smell — likely misconfiguration but not invalid.
    /// Exits non-zero only under `--strict`.
    Warning,
}

/// A single diagnostic entry.
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub severity: Severity,
    /// Human-readable field path, e.g. `gateway.session_ttl_secs`.
    pub field: String,
    pub message: String,
}

/// What sources the [`ConfigLoader`] actually consulted.
#[derive(Debug, Clone, Serialize)]
pub struct SourceReport {
    /// Config directory inspected.
    pub config_dir: PathBuf,
    /// `Some(path)` when a YAML or TOML config file was read.
    pub file_used: Option<PathBuf>,
    /// `true` when neither file existed and defaults were used.
    pub used_defaults: bool,
    /// Names (not values) of `GARRAIA_*` env vars detected in the process.
    pub env_vars_detected: Vec<String>,
    /// Whether `mcp.json` was present (not its contents).
    pub mcp_json_present: bool,
}

/// Aggregate result returned to the CLI.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigCheck {
    pub source: SourceReport,
    pub findings: Vec<Finding>,
    /// Redacted summary of key fields (presence-only for secrets).
    pub summary: ConfigSummary,
}

/// Non-secret summary of the effective config.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigSummary {
    pub gateway_host: String,
    pub gateway_port: u16,
    pub gateway_api_key_set: bool,
    pub tls_enabled: bool,
    pub channels_count: usize,
    pub llm_providers: Vec<String>,
    pub llm_providers_api_key_set: Vec<String>,
    pub embeddings_providers: Vec<String>,
    pub mcp_servers_count: usize,
    pub log_level: Option<String>,
}

impl ConfigCheck {
    /// Highest severity observed, if any.
    pub fn max_severity(&self) -> Option<Severity> {
        let mut max = None;
        for f in &self.findings {
            match (max, f.severity) {
                (None, s) => max = Some(s),
                (Some(Severity::Warning), Severity::Error) => max = Some(Severity::Error),
                _ => {}
            }
        }
        max
    }

    /// Whether any [`Severity::Error`] was found.
    pub fn has_errors(&self) -> bool {
        self.findings.iter().any(|f| f.severity == Severity::Error)
    }

    /// Whether any [`Severity::Warning`] was found.
    pub fn has_warnings(&self) -> bool {
        self.findings
            .iter()
            .any(|f| f.severity == Severity::Warning)
    }
}

/// Env vars the gateway/tools read. Names only — never values.
///
/// Kept alphabetised to make diffs obvious when new secrets are introduced;
/// the full set from `CLAUDE.md` rule #6 plus auth/db/metrics/telemetry
/// plumbing. See [`detect_env_vars`] for the read-semantics contract.
const KNOWN_GARRAIA_ENV_VARS: &[&str] = &[
    "GARRAIA_APP_DATABASE_URL",
    "GARRAIA_CONFIG_DIR",
    "GARRAIA_DATA_DIR",
    "GARRAIA_JWT_SECRET",
    "GARRAIA_LOGIN_DATABASE_URL",
    "GARRAIA_LOG_FORMAT",
    "GARRAIA_METRICS_ALLOW",
    "GARRAIA_METRICS_BIND",
    "GARRAIA_METRICS_TOKEN",
    "GARRAIA_REFRESH_HMAC_SECRET",
    "GARRAIA_SIGNUP_DATABASE_URL",
    "GARRAIA_TRUSTED_PROXIES",
    "GARRAIA_VAULT_PASSPHRASE",
];

fn detect_env_vars() -> Vec<String> {
    let mut found: Vec<String> = KNOWN_GARRAIA_ENV_VARS
        .iter()
        .filter(|name| std::env::var_os(name).is_some())
        .map(|name| (*name).to_owned())
        .collect();
    found.sort();
    found
}

/// Mask the userinfo portion of a URL-like string so credentials
/// (`http://user:secret@host/`) do not leak through error messages.
/// SEC-M-02 (plan 0035 security audit).
fn sanitise_for_display(value: &str) -> String {
    let trimmed = value.trim();
    if let Some(scheme_end) = trimmed.find("://") {
        let (scheme, rest) = trimmed.split_at(scheme_end + 3);
        if let Some(at_pos) = rest.find('@') {
            let after_at = &rest[at_pos + 1..];
            return format!("{scheme}***REDACTED***@{after_at}");
        }
    }
    trimmed.to_owned()
}

fn source_report(loader: &ConfigLoader) -> SourceReport {
    let dir = loader.config_dir().to_path_buf();
    let yaml_path = dir.join("config.yml");
    let toml_path = dir.join("config.toml");
    let mcp_path = dir.join("mcp.json");

    let (file_used, used_defaults) = if yaml_path.exists() {
        (Some(yaml_path), false)
    } else if toml_path.exists() {
        (Some(toml_path), false)
    } else {
        (None, true)
    };

    SourceReport {
        config_dir: dir,
        file_used,
        used_defaults,
        env_vars_detected: detect_env_vars(),
        mcp_json_present: mcp_path.exists(),
    }
}

fn summarise(config: &AppConfig) -> ConfigSummary {
    let mut llm_providers: Vec<String> = config.llm.keys().cloned().collect();
    llm_providers.sort();

    let mut llm_providers_api_key_set: Vec<String> = config
        .llm
        .iter()
        .filter(|(_, cfg)| cfg.api_key.is_some())
        .map(|(name, _)| name.clone())
        .collect();
    llm_providers_api_key_set.sort();

    let mut embeddings_providers: Vec<String> = config.embeddings.keys().cloned().collect();
    embeddings_providers.sort();

    let tls_enabled =
        config.gateway.tls_cert_path.is_some() && config.gateway.tls_key_path.is_some();

    ConfigSummary {
        gateway_host: config.gateway.host.clone(),
        gateway_port: config.gateway.port,
        gateway_api_key_set: config.gateway.api_key.is_some(),
        tls_enabled,
        channels_count: config.channels.len(),
        llm_providers,
        llm_providers_api_key_set,
        embeddings_providers,
        mcp_servers_count: config.mcp.len(),
        log_level: config.log_level.clone(),
    }
}

fn validate(config: &AppConfig) -> Vec<Finding> {
    let mut findings: Vec<Finding> = Vec::new();
    let push_err = |findings: &mut Vec<Finding>, field: &str, message: String| {
        findings.push(Finding {
            severity: Severity::Error,
            field: field.to_owned(),
            message,
        });
    };
    let push_warn = |findings: &mut Vec<Finding>, field: &str, message: String| {
        findings.push(Finding {
            severity: Severity::Warning,
            field: field.to_owned(),
            message,
        });
    };

    // gateway.port: u16 so 0..=65535. 0 is invalid for a listener.
    if config.gateway.port == 0 {
        push_err(
            &mut findings,
            "gateway.port",
            "gateway.port must be 1..=65535 (got 0)".into(),
        );
    }

    // session_ttl_secs: must be positive (JWT-like absolute TTL).
    if config.gateway.session_ttl_secs <= 0 {
        push_err(
            &mut findings,
            "gateway.session_ttl_secs",
            format!(
                "gateway.session_ttl_secs must be a positive integer (got {}; 0 or negative disable token rotation)",
                config.gateway.session_ttl_secs
            ),
        );
    }

    // session_idle_secs: >= 0 (0 means disabled). Idle > TTL is nonsense.
    if config.gateway.session_idle_secs < 0 {
        push_err(
            &mut findings,
            "gateway.session_idle_secs",
            format!(
                "gateway.session_idle_secs must be >= 0 (got {}); 0 disables idle cutoff",
                config.gateway.session_idle_secs
            ),
        );
    } else if config.gateway.session_idle_secs > 0
        && config.gateway.session_ttl_secs > 0
        && config.gateway.session_idle_secs > config.gateway.session_ttl_secs
    {
        push_warn(
            &mut findings,
            "gateway.session_idle_secs",
            format!(
                "gateway.session_idle_secs ({}) is larger than gateway.session_ttl_secs ({}); the absolute TTL will close the session first",
                config.gateway.session_idle_secs, config.gateway.session_ttl_secs
            ),
        );
    }

    // rate_limit.burst_size: 0 effectively disables the bucket — warn.
    if config.gateway.rate_limit.burst_size == 0 {
        push_warn(
            &mut findings,
            "gateway.rate_limit.burst_size",
            "gateway.rate_limit.burst_size == 0 disables rate limiting entirely".into(),
        );
    }
    if config.gateway.rate_limit.per_second == 0 {
        push_warn(
            &mut findings,
            "gateway.rate_limit.per_second",
            "gateway.rate_limit.per_second == 0 disables sustained rate limiting".into(),
        );
    }

    // TLS: cert + key must be set together or both absent.
    match (
        config.gateway.tls_cert_path.as_deref(),
        config.gateway.tls_key_path.as_deref(),
    ) {
        (Some(_), None) => push_err(
            &mut findings,
            "gateway.tls_key_path",
            "gateway.tls_cert_path is set but gateway.tls_key_path is missing".into(),
        ),
        (None, Some(_)) => push_err(
            &mut findings,
            "gateway.tls_cert_path",
            "gateway.tls_key_path is set but gateway.tls_cert_path is missing".into(),
        ),
        _ => {}
    }

    // Timeouts: 0 means "no timeout" for reqwest/tokio — warn (user probably meant something else).
    if config.timeouts.llm.default_secs == 0 {
        push_warn(
            &mut findings,
            "timeouts.llm.default_secs",
            "timeouts.llm.default_secs == 0 disables the LLM timeout (hung calls will block forever)".into(),
        );
    }
    if config.timeouts.mcp.default_secs == 0 {
        push_warn(
            &mut findings,
            "timeouts.mcp.default_secs",
            "timeouts.mcp.default_secs == 0 disables the MCP timeout".into(),
        );
    }

    // Voice: if enabled, endpoints must look like URLs (trivial check — not full parse).
    // SEC-M-02 (security audit): URLs may contain userinfo credentials
    // (`http://user:pw@host/`). The `sanitise_for_display` helper masks the
    // userinfo portion so misconfigured endpoints never leak creds into
    // findings' messages nor into the JSON output.
    if config.voice.enabled {
        if config.voice.tts_endpoint.trim().is_empty() {
            push_err(
                &mut findings,
                "voice.tts_endpoint",
                "voice.enabled but voice.tts_endpoint is empty".into(),
            );
        } else if !config.voice.tts_endpoint.starts_with("http://")
            && !config.voice.tts_endpoint.starts_with("https://")
        {
            push_warn(
                &mut findings,
                "voice.tts_endpoint",
                format!(
                    "voice.tts_endpoint should be an http:// or https:// URL (got `{}`)",
                    sanitise_for_display(&config.voice.tts_endpoint)
                ),
            );
        }
        if config.voice.stt_endpoint.trim().is_empty() {
            push_err(
                &mut findings,
                "voice.stt_endpoint",
                "voice.enabled but voice.stt_endpoint is empty".into(),
            );
        } else if !config.voice.stt_endpoint.starts_with("http://")
            && !config.voice.stt_endpoint.starts_with("https://")
        {
            push_warn(
                &mut findings,
                "voice.stt_endpoint",
                format!(
                    "voice.stt_endpoint should be an http:// or https:// URL (got `{}`)",
                    sanitise_for_display(&config.voice.stt_endpoint)
                ),
            );
        }
    }

    // MCP allowlist sanity: transport must be known.
    for (name, server) in &config.mcp {
        if server.transport != "stdio" && server.transport != "http" {
            push_warn(
                &mut findings,
                &format!("mcp.{name}.transport"),
                format!(
                    "mcp.{name}.transport=`{}` is not a recognised transport; expected `stdio` or `http`",
                    server.transport
                ),
            );
        }
        if server.command.trim().is_empty() && server.url.is_none() {
            push_err(
                &mut findings,
                &format!("mcp.{name}"),
                "mcp server entry must have either `command` or `url`".into(),
            );
        }
    }

    // FS glob mode: only "picomatch" or "bash" are supported.
    match config.fs.glob.mode.as_str() {
        "picomatch" | "bash" => {}
        other => push_err(
            &mut findings,
            "fs.glob.mode",
            format!("fs.glob.mode must be `picomatch` or `bash` (got `{other}`)"),
        ),
    }

    // memory.embedding_provider, when set, should match an embeddings entry.
    if let Some(ep) = config.memory.embedding_provider.as_deref()
        && !config.embeddings.contains_key(ep)
    {
        push_err(
            &mut findings,
            "memory.embedding_provider",
            format!("memory.embedding_provider=`{ep}` does not match any entry in `embeddings:`"),
        );
    }

    // agent.default_provider, when set, should match an llm entry.
    if let Some(dp) = config.agent.default_provider.as_deref()
        && !config.llm.contains_key(dp)
    {
        push_err(
            &mut findings,
            "agent.default_provider",
            format!("agent.default_provider=`{dp}` does not match any entry in `llm:`"),
        );
    }

    // agent.fallback_providers entries should also exist in `llm:`.
    for fb in &config.agent.fallback_providers {
        if !config.llm.contains_key(fb) {
            push_err(
                &mut findings,
                "agent.fallback_providers",
                format!("agent.fallback_providers entry `{fb}` does not match any entry in `llm:`"),
            );
        }
    }

    // mobile.persona: plan 0042 §5.3 — suspiciously short strings are
    // flagged as Warning (valid shape, unusual content). Threshold of
    // 10 chars picks up accidental "hi" / "test" without slapping
    // every lean prompt.
    if let Some(p) = &config.mobile.persona
        && p.len() < 10
    {
        push_warn(
            &mut findings,
            "mobile.persona",
            format!(
                "mobile.persona is only {} chars; typical personas are multi-sentence — likely a stub or typo",
                p.len()
            ),
        );
    }

    findings
}

/// Run the full validation + reporting pipeline.
pub fn run_check(loader: &ConfigLoader, config: &AppConfig) -> ConfigCheck {
    ConfigCheck {
        source: source_report(loader),
        findings: validate(config),
        summary: summarise(config),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AppConfig, GatewayConfig, LlmProviderConfig, McpServerConfig, VoiceConfig};
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "garraia-config-check-{}-{}-{}",
            label,
            std::process::id(),
            nanos
        ))
    }

    #[test]
    fn source_report_detects_yaml() {
        let dir = temp_dir("yaml");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("config.yml"), "gateway:\n  host: \"127.0.0.1\"\n").unwrap();
        let loader = ConfigLoader::with_dir(&dir);
        let report = source_report(&loader);
        assert_eq!(report.file_used, Some(dir.join("config.yml")));
        assert!(!report.used_defaults);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn source_report_falls_back_to_toml_then_defaults() {
        let dir = temp_dir("toml");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("config.toml"), "[gateway]\nhost = \"0.0.0.0\"\n").unwrap();
        let loader = ConfigLoader::with_dir(&dir);
        let report = source_report(&loader);
        assert_eq!(report.file_used, Some(dir.join("config.toml")));
        assert!(!report.used_defaults);
        let _ = fs::remove_dir_all(&dir);

        let dir = temp_dir("defaults");
        fs::create_dir_all(&dir).unwrap();
        let loader = ConfigLoader::with_dir(&dir);
        let report = source_report(&loader);
        assert_eq!(report.file_used, None);
        assert!(report.used_defaults);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn valid_default_config_has_no_errors() {
        let cfg = AppConfig::default();
        let findings = validate(&cfg);
        assert!(
            !findings.iter().any(|f| f.severity == Severity::Error),
            "default config produced errors: {findings:?}"
        );
    }

    #[test]
    fn zero_session_ttl_is_error() {
        let mut cfg = AppConfig::default();
        cfg.gateway.session_ttl_secs = 0;
        let findings = validate(&cfg);
        assert!(
            findings
                .iter()
                .any(|f| f.severity == Severity::Error && f.field == "gateway.session_ttl_secs"),
            "findings = {findings:?}"
        );
    }

    #[test]
    fn negative_session_idle_is_error() {
        let mut cfg = AppConfig::default();
        cfg.gateway.session_idle_secs = -1;
        let findings = validate(&cfg);
        assert!(
            findings
                .iter()
                .any(|f| f.severity == Severity::Error && f.field == "gateway.session_idle_secs"),
            "findings = {findings:?}"
        );
    }

    #[test]
    fn idle_greater_than_ttl_is_warning() {
        let mut cfg = AppConfig::default();
        cfg.gateway.session_ttl_secs = 60;
        cfg.gateway.session_idle_secs = 600;
        let findings = validate(&cfg);
        assert!(
            findings
                .iter()
                .any(|f| f.severity == Severity::Warning && f.field == "gateway.session_idle_secs"),
            "findings = {findings:?}"
        );
    }

    #[test]
    fn partial_tls_is_error() {
        let mut cfg = AppConfig::default();
        cfg.gateway.tls_cert_path = Some("/tmp/cert.pem".to_string());
        cfg.gateway.tls_key_path = None;
        let findings = validate(&cfg);
        assert!(
            findings
                .iter()
                .any(|f| f.severity == Severity::Error && f.field == "gateway.tls_key_path"),
            "findings = {findings:?}"
        );
    }

    #[test]
    fn zero_burst_is_warning() {
        let mut cfg = AppConfig::default();
        cfg.gateway.rate_limit.burst_size = 0;
        let findings = validate(&cfg);
        assert!(
            findings
                .iter()
                .any(|f| f.severity == Severity::Warning
                    && f.field == "gateway.rate_limit.burst_size"),
            "findings = {findings:?}"
        );
    }

    #[test]
    fn unknown_fs_glob_mode_is_error() {
        let mut cfg = AppConfig::default();
        cfg.fs.glob.mode = "regex".into();
        let findings = validate(&cfg);
        assert!(
            findings
                .iter()
                .any(|f| f.severity == Severity::Error && f.field == "fs.glob.mode"),
            "findings = {findings:?}"
        );
    }

    #[test]
    fn voice_enabled_with_empty_endpoint_is_error() {
        let cfg = AppConfig {
            voice: VoiceConfig {
                enabled: true,
                tts_endpoint: "".into(),
                stt_endpoint: "http://stt".into(),
                ..VoiceConfig::default()
            },
            ..AppConfig::default()
        };
        let findings = validate(&cfg);
        assert!(
            findings
                .iter()
                .any(|f| f.severity == Severity::Error && f.field == "voice.tts_endpoint"),
            "findings = {findings:?}"
        );
    }

    #[test]
    fn memory_embedding_provider_must_match_entry() {
        let mut cfg = AppConfig::default();
        cfg.memory.embedding_provider = Some("missing-one".into());
        let findings = validate(&cfg);
        assert!(
            findings
                .iter()
                .any(|f| f.severity == Severity::Error && f.field == "memory.embedding_provider"),
            "findings = {findings:?}"
        );
    }

    #[test]
    fn agent_default_provider_must_match_llm_entry() {
        let mut cfg = AppConfig::default();
        cfg.agent.default_provider = Some("ghost".into());
        let findings = validate(&cfg);
        assert!(
            findings
                .iter()
                .any(|f| f.severity == Severity::Error && f.field == "agent.default_provider"),
            "findings = {findings:?}"
        );
    }

    #[test]
    fn agent_default_provider_matches_when_llm_has_entry() {
        let mut cfg = AppConfig::default();
        cfg.llm.insert(
            "primary".into(),
            LlmProviderConfig {
                provider: "openrouter".into(),
                model: None,
                api_key: Some("sk-secret".into()),
                base_url: None,
                extra: HashMap::new(),
            },
        );
        cfg.agent.default_provider = Some("primary".into());
        let findings = validate(&cfg);
        assert!(
            !findings.iter().any(|f| f.field == "agent.default_provider"),
            "findings = {findings:?}"
        );
    }

    #[test]
    fn summary_redacts_api_keys() {
        let mut cfg = AppConfig {
            gateway: GatewayConfig {
                api_key: Some("sk-live-supersecret".into()),
                ..GatewayConfig::default()
            },
            ..AppConfig::default()
        };
        cfg.llm.insert(
            "openrouter".into(),
            LlmProviderConfig {
                provider: "openrouter".into(),
                model: None,
                api_key: Some("sk-or-supersecret".into()),
                base_url: None,
                extra: HashMap::new(),
            },
        );
        let summary = summarise(&cfg);
        assert!(summary.gateway_api_key_set);
        assert_eq!(summary.llm_providers, vec!["openrouter".to_string()]);
        assert_eq!(
            summary.llm_providers_api_key_set,
            vec!["openrouter".to_string()]
        );
        // The serialised JSON must NOT contain the secret values.
        let json = serde_json::to_string(&summary).unwrap();
        assert!(
            !json.contains("supersecret"),
            "summary leaked secret: {json}"
        );
    }

    #[test]
    fn mcp_entry_without_command_or_url_is_error() {
        let mut cfg = AppConfig::default();
        cfg.mcp.insert(
            "broken".into(),
            McpServerConfig {
                command: "".into(),
                args: vec![],
                env: HashMap::new(),
                transport: "stdio".into(),
                url: None,
                enabled: None,
                timeout: None,
                allowed_tools: vec![],
                memory_limit_mb: None,
                max_restarts: None,
                restart_delay_secs: None,
            },
        );
        let findings = validate(&cfg);
        assert!(
            findings
                .iter()
                .any(|f| f.severity == Severity::Error && f.field == "mcp.broken"),
            "findings = {findings:?}"
        );
    }

    #[test]
    fn sanitise_for_display_masks_userinfo() {
        assert_eq!(
            sanitise_for_display("http://user:sk-secret@host/path"),
            "http://***REDACTED***@host/path"
        );
        assert_eq!(
            sanitise_for_display("https://token@internal.api/v1"),
            "https://***REDACTED***@internal.api/v1"
        );
        assert_eq!(
            sanitise_for_display("http://plain.host/path"),
            "http://plain.host/path"
        );
        // Non-URL input is passed through trimmed (not a secret either way).
        assert_eq!(sanitise_for_display("  ftp://x.y  "), "ftp://x.y");
    }

    #[test]
    fn voice_endpoint_error_never_leaks_userinfo() {
        let cfg = AppConfig {
            voice: VoiceConfig {
                enabled: true,
                tts_endpoint: "notaurl://user:sk-secret@host".into(),
                stt_endpoint: "http://garraia-stt:9090".into(),
                ..VoiceConfig::default()
            },
            ..AppConfig::default()
        };
        let findings = validate(&cfg);
        let tts = findings
            .iter()
            .find(|f| f.field == "voice.tts_endpoint")
            .expect("expected tts finding");
        assert!(
            !tts.message.contains("sk-secret"),
            "finding leaked credential: {}",
            tts.message
        );
        assert!(tts.message.contains("***REDACTED***"));
    }

    #[test]
    fn full_check_serialisation_never_leaks_api_keys() {
        let dir = temp_dir("full-redaction");
        fs::create_dir_all(&dir).unwrap();
        let loader = ConfigLoader::with_dir(&dir);

        let mut cfg = AppConfig {
            gateway: GatewayConfig {
                api_key: Some("sk-gateway-supersecret".into()),
                ..GatewayConfig::default()
            },
            ..AppConfig::default()
        };
        cfg.llm.insert(
            "primary".into(),
            LlmProviderConfig {
                provider: "openrouter".into(),
                model: None,
                api_key: Some("sk-llm-supersecret".into()),
                base_url: None,
                extra: HashMap::new(),
            },
        );
        cfg.embeddings.insert(
            "cohere".into(),
            crate::model::EmbeddingProviderConfig {
                provider: "cohere".into(),
                model: None,
                api_key: Some("sk-embed-supersecret".into()),
                base_url: None,
                dimensions: None,
                extra: HashMap::new(),
            },
        );

        let check = run_check(&loader, &cfg);
        let full_json = serde_json::to_string(&check).expect("serialise check");
        for needle in [
            "sk-gateway-supersecret",
            "sk-llm-supersecret",
            "sk-embed-supersecret",
        ] {
            assert!(
                !full_json.contains(needle),
                "full ConfigCheck serialisation leaked {needle}: {full_json}"
            );
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_returns_source_findings_and_summary() {
        let dir = temp_dir("full");
        fs::create_dir_all(&dir).unwrap();
        let loader = ConfigLoader::with_dir(&dir);
        let mut cfg = AppConfig::default();
        cfg.gateway.session_ttl_secs = 0;
        let check = run_check(&loader, &cfg);
        assert!(check.source.used_defaults);
        assert!(check.has_errors());
        assert_eq!(check.max_severity(), Some(Severity::Error));
        assert_eq!(check.summary.gateway_port, 3888);
        let _ = fs::remove_dir_all(&dir);
    }

    // Plan 0042 §6.1 — mobile.persona validation tests.

    #[test]
    fn mobile_persona_none_produces_no_finding() {
        let cfg = AppConfig::default();
        let findings = validate(&cfg);
        assert!(
            findings.iter().all(|f| f.field != "mobile.persona"),
            "persona absent must not flag: {findings:?}"
        );
    }

    #[test]
    fn mobile_persona_short_string_emits_warn() {
        let mut cfg = AppConfig::default();
        cfg.mobile.persona = Some("hi".into());
        let findings = validate(&cfg);
        let f = findings
            .iter()
            .find(|f| f.field == "mobile.persona")
            .expect("persona finding");
        assert_eq!(f.severity, Severity::Warning);
        assert!(f.message.contains("2 chars"));
    }

    #[test]
    fn mobile_persona_long_enough_is_clean() {
        let mut cfg = AppConfig::default();
        cfg.mobile.persona = Some("Você é um assistente útil e direto.".into());
        let findings = validate(&cfg);
        assert!(
            findings.iter().all(|f| f.field != "mobile.persona"),
            "long persona must not flag: {findings:?}"
        );
    }
}
