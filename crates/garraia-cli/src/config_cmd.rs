//! `garraia config check` — validate the effective configuration and report.
//!
//! Plan 0035 (GAR-379 slice 1). Exit codes follow sysexits:
//! - `0` — OK (no errors; warnings allowed unless `--strict`).
//! - `2` — validation errors (or warnings under `--strict`).
//! - `65` — `EX_DATAERR`, file exists but parses as invalid.
//!
//! SEC-L-01 (plan 0035 security audit): the parse-error string is truncated
//! to 256 characters before being emitted so that a pathological YAML/TOML
//! file (e.g. one that smuggles a large payload into the error chain) cannot
//! dominate the output. The bounded message preserves line/column context
//! from `serde_yaml`/`toml` but never prints the full failing file.

use anyhow::Result;
use garraia_config::{AppConfig, ConfigCheck, ConfigLoader, LlmProviderConfig, Severity};

/// Maximum length of the `format!("{e}")` snippet emitted on parse failure.
/// Keeps error output bounded without losing the leading line/column context.
const PARSE_ERROR_MAX_LEN: usize = 256;

pub(crate) fn truncate_error(raw: String) -> String {
    if raw.len() <= PARSE_ERROR_MAX_LEN {
        return raw;
    }
    let mut truncated: String = raw.chars().take(PARSE_ERROR_MAX_LEN).collect();
    truncated.push_str("... [truncated]");
    truncated
}

pub fn run_config_check(json: bool, strict: bool) -> Result<i32> {
    let loader = ConfigLoader::new()?;
    loader.ensure_dirs()?;

    let config = match loader.load() {
        Ok(c) => c,
        Err(e) => {
            let parse_error = truncate_error(format!("{e}"));
            if json {
                let payload = serde_json::json!({
                    "ok": false,
                    "exit_code": 65,
                    "error": parse_error,
                    "config_dir": loader.config_dir().display().to_string(),
                });
                println!("{}", serde_json::to_string_pretty(&payload)?);
            } else {
                eprintln!(
                    "error: failed to load config from {}: {parse_error}",
                    loader.config_dir().display()
                );
                eprintln!("hint: the file exists but does not parse; fix YAML/TOML syntax.");
            }
            return Ok(65);
        }
    };

    let check = garraia_config::run_check(&loader, &config);

    let exit_code = compute_exit_code(&check, strict);

    if json {
        let payload = serde_json::json!({
            "ok": exit_code == 0,
            "exit_code": exit_code,
            "source": check.source,
            "summary": check.summary,
            "findings": check.findings,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        print_human(&check, strict);
    }

    Ok(exit_code)
}

fn compute_exit_code(check: &ConfigCheck, strict: bool) -> i32 {
    if check.has_errors() || (strict && check.has_warnings()) {
        2
    } else {
        0
    }
}

// `mod tests` lives mid-file here because the public entry (`run_config_check`)
// and its `compute_exit_code` helper precede it, while the large `print_*`
// helpers come after. Split 8+ large fn moves would bloat this PR — allow
// locally.
#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::*;
    use garraia_config::{ConfigSummary, Finding, SourceReport};
    use std::path::PathBuf;

    fn empty_check(findings: Vec<Finding>) -> ConfigCheck {
        ConfigCheck {
            source: SourceReport {
                config_dir: PathBuf::from("/tmp"),
                file_used: None,
                used_defaults: true,
                env_vars_detected: vec![],
                mcp_json_present: false,
            },
            findings,
            summary: ConfigSummary {
                gateway_host: "127.0.0.1".into(),
                gateway_port: 3888,
                gateway_api_key_set: false,
                tls_enabled: false,
                channels_count: 0,
                llm_providers: vec![],
                llm_providers_api_key_set: vec![],
                default_provider: None,
                fallback_providers: vec![],
                llm_models: Default::default(),
                embeddings_providers: vec![],
                mcp_servers_count: 0,
                log_level: None,
            },
        }
    }

    #[test]
    fn compute_exit_code_zero_when_clean_non_strict() {
        let check = empty_check(vec![]);
        assert_eq!(compute_exit_code(&check, false), 0);
        assert_eq!(compute_exit_code(&check, true), 0);
    }

    #[test]
    fn compute_exit_code_two_on_error_regardless_of_strict() {
        let check = empty_check(vec![Finding {
            severity: Severity::Error,
            field: "gateway.port".into(),
            message: "zero".into(),
        }]);
        assert_eq!(compute_exit_code(&check, false), 2);
        assert_eq!(compute_exit_code(&check, true), 2);
    }

    #[test]
    fn compute_exit_code_promotes_warning_only_under_strict() {
        let check = empty_check(vec![Finding {
            severity: Severity::Warning,
            field: "gateway.rate_limit.burst_size".into(),
            message: "zero disables".into(),
        }]);
        assert_eq!(compute_exit_code(&check, false), 0);
        assert_eq!(compute_exit_code(&check, true), 2);
    }

    #[test]
    fn truncate_error_leaves_short_strings_alone() {
        let short = "invalid at line 4: unexpected character".to_string();
        assert_eq!(truncate_error(short.clone()), short);
    }

    #[test]
    fn truncate_error_bounds_pathological_input() {
        let giant = "x".repeat(10_000);
        let truncated = truncate_error(giant);
        assert!(truncated.ends_with("... [truncated]"));
        assert!(truncated.len() <= PARSE_ERROR_MAX_LEN + "... [truncated]".len());
    }
}

fn print_human(check: &ConfigCheck, strict: bool) {
    println!("GarraIA config check");
    println!("====================");

    println!();
    println!("Sources");
    println!("-------");
    println!(
        "  config_dir         : {}",
        check.source.config_dir.display()
    );
    match &check.source.file_used {
        Some(path) => println!("  file               : {}", path.display()),
        None => println!("  file               : (none — using defaults)"),
    }
    println!(
        "  defaults_only      : {}",
        if check.source.used_defaults {
            "yes"
        } else {
            "no"
        }
    );
    println!(
        "  mcp.json present   : {}",
        if check.source.mcp_json_present {
            "yes"
        } else {
            "no"
        }
    );
    println!(
        "  env vars detected  : {} [{}]",
        check.source.env_vars_detected.len(),
        check.source.env_vars_detected.join(", ")
    );

    println!();
    println!("Summary (redacted)");
    println!("------------------");
    println!(
        "  gateway            : {}:{}",
        check.summary.gateway_host, check.summary.gateway_port
    );
    println!(
        "  gateway_api_key    : {}",
        if check.summary.gateway_api_key_set {
            "set"
        } else {
            "not set"
        }
    );
    println!(
        "  tls_enabled        : {}",
        if check.summary.tls_enabled {
            "yes"
        } else {
            "no"
        }
    );
    println!("  channels           : {}", check.summary.channels_count);
    println!(
        "  llm providers      : {} [{}]",
        check.summary.llm_providers.len(),
        check.summary.llm_providers.join(", ")
    );
    if !check.summary.llm_providers_api_key_set.is_empty() {
        println!(
            "  llm api_key set    : [{}]",
            check.summary.llm_providers_api_key_set.join(", ")
        );
    }
    println!(
        "  embeddings         : {} [{}]",
        check.summary.embeddings_providers.len(),
        check.summary.embeddings_providers.join(", ")
    );
    println!("  mcp servers        : {}", check.summary.mcp_servers_count);
    if let Some(lvl) = &check.summary.log_level {
        println!("  log_level          : {lvl}");
    }

    println!();
    println!("Findings");
    println!("--------");
    if check.findings.is_empty() {
        println!("  (none)");
    } else {
        for f in &check.findings {
            let tag = match f.severity {
                Severity::Error => "ERROR  ",
                Severity::Warning => "WARNING",
            };
            println!("  [{tag}] {}: {}", f.field, f.message);
        }
    }

    println!();
    let errors = check
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .count();
    let warnings = check
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Warning)
        .count();
    println!(
        "Result: {} error(s), {} warning(s){}",
        errors,
        warnings,
        if strict { " [strict]" } else { "" }
    );
}

/// Where a `set-model` write should land, and what it should contain.
///
/// Split out from [`run_set_model`] so the merge semantics are unit-testable
/// without touching the real config directory or the filesystem.
pub(crate) struct SetModelRequest<'a> {
    pub provider_key: &'a str,
    pub provider: &'a str,
    pub model: &'a str,
    pub base_url: &'a str,
}

/// Apply a `set-model` request to `config` in place.
///
/// Deliberately narrow: it replaces exactly one `llm:` entry and repoints
/// `agent.default_provider`. Everything else — other providers, channels,
/// voice, the operator's system prompt — is left byte-for-byte alone, because
/// this runs unattended (unpacked installs, `ollama launch`) where clobbering
/// an existing config would be silent data loss.
///
/// The previous `agent.default_provider`, when it named a different key that
/// still exists, is demoted to the front of `fallback_providers` rather than
/// dropped, so switching to a local model does not disable a configured cloud
/// provider outright.
pub(crate) fn apply_set_model(config: &mut AppConfig, req: &SetModelRequest<'_>) {
    let api_key = if req.provider == "ollama" || req.provider == "llamacpp" {
        // Keyless local daemons: ollama e llama-server não exigem credencial.
        None
    } else {
        // Ollama's OpenAI-compatible endpoint ignores the key but the client
        // requires a non-empty one; "ollama" is the conventional placeholder.
        Some("ollama".to_string())
    };

    config.llm.insert(
        req.provider_key.to_string(),
        LlmProviderConfig {
            provider: req.provider.to_string(),
            model: Some(req.model.to_string()),
            api_key,
            base_url: Some(req.base_url.to_string()),
            extra: Default::default(),
        },
    );

    let previous = config
        .agent
        .default_provider
        .replace(req.provider_key.to_string());
    if let Some(prev) = previous
        && prev != req.provider_key
        && config.llm.contains_key(&prev)
        && !config.agent.fallback_providers.contains(&prev)
    {
        config.agent.fallback_providers.insert(0, prev);
    }
}

/// `garraia config set-model` — write one provider entry and make it default.
pub fn run_set_model(
    model: &str,
    provider_key: &str,
    provider: &str,
    base_url: &str,
) -> Result<i32> {
    if model.trim().is_empty() {
        eprintln!("error: --model must not be empty");
        return Ok(2);
    }
    if provider_key.trim().is_empty() {
        eprintln!("error: --provider-key must not be empty");
        return Ok(2);
    }

    let loader = ConfigLoader::new()?;
    loader.ensure_dirs()?;

    // An unparseable existing config is EX_DATAERR, same as `config check` —
    // never silently overwrite a file we could not read.
    let mut config = match loader.load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "error: existing config could not be parsed: {}",
                truncate_error(format!("{e}"))
            );
            eprintln!("       fix it (or move it aside) before running `config set-model`.");
            return Ok(65);
        }
    };

    apply_set_model(
        &mut config,
        &SetModelRequest {
            provider_key,
            provider,
            model,
            base_url,
        },
    );

    // `save` serializes and clamps the file to 0600 (it can hold credentials).
    loader.save(&config)?;

    println!(
        "Modelo definido: {model} (provider '{provider}' sob a chave '{provider_key}')\n\
         Config: {}/config.yml",
        loader.config_dir().display()
    );
    Ok(0)
}

#[cfg(test)]
mod set_model_tests {
    use super::*;

    fn req<'a>(key: &'a str, model: &'a str) -> SetModelRequest<'a> {
        SetModelRequest {
            provider_key: key,
            provider: "openai",
            model,
            base_url: "http://127.0.0.1:11434/v1",
        }
    }

    #[test]
    fn writes_entry_and_makes_it_default() {
        let mut cfg = AppConfig::default();
        apply_set_model(&mut cfg, &req("ollama-launch", "qwen3.8:latest"));

        let entry = cfg.llm.get("ollama-launch").expect("entry written");
        assert_eq!(entry.provider, "openai");
        assert_eq!(entry.model.as_deref(), Some("qwen3.8:latest"));
        assert_eq!(entry.base_url.as_deref(), Some("http://127.0.0.1:11434/v1"));
        // Ollama's /v1 ignores the key but the OpenAI client demands one.
        assert_eq!(entry.api_key.as_deref(), Some("ollama"));
        assert_eq!(cfg.agent.default_provider.as_deref(), Some("ollama-launch"));
    }

    #[test]
    fn native_ollama_provider_stays_keyless() {
        let mut cfg = AppConfig::default();
        let mut r = req("local", "qwen3.8:latest");
        r.provider = "ollama";
        apply_set_model(&mut cfg, &r);
        assert!(cfg.llm["local"].api_key.is_none());
    }

    #[test]
    fn native_llamacpp_provider_stays_keyless() {
        let mut cfg = AppConfig::default();
        let mut r = req("local-llama", "qwen3-8b");
        r.provider = "llamacpp";
        apply_set_model(&mut cfg, &r);
        assert!(cfg.llm["local-llama"].api_key.is_none());
        assert_eq!(cfg.llm["local-llama"].provider, "llamacpp");
    }

    #[test]
    fn rerunning_updates_in_place_without_duplicating() {
        let mut cfg = AppConfig::default();
        apply_set_model(&mut cfg, &req("ollama-launch", "qwen3.8:latest"));
        apply_set_model(&mut cfg, &req("ollama-launch", "qwen3:8b"));

        assert_eq!(cfg.llm.len(), 1);
        assert_eq!(cfg.llm["ollama-launch"].model.as_deref(), Some("qwen3:8b"));
        assert!(
            cfg.agent.fallback_providers.is_empty(),
            "a key must never become its own fallback"
        );
    }

    #[test]
    fn previous_default_is_demoted_to_fallback_not_dropped() {
        // The unattended-install case: a user already configured OpenRouter,
        // then a launcher points GarraIA at a local model. Their cloud
        // provider must survive as a fallback.
        let mut cfg = AppConfig::default();
        cfg.llm.insert(
            "openrouter".to_string(),
            LlmProviderConfig {
                provider: "openrouter".to_string(),
                model: Some("openrouter/auto".to_string()),
                api_key: Some("k".to_string()),
                base_url: None,
                extra: Default::default(),
            },
        );
        cfg.agent.default_provider = Some("openrouter".to_string());

        apply_set_model(&mut cfg, &req("ollama-launch", "qwen3.8:latest"));

        assert_eq!(cfg.agent.default_provider.as_deref(), Some("ollama-launch"));
        assert_eq!(cfg.agent.fallback_providers, vec!["openrouter".to_string()]);
        // And the original entry is untouched.
        assert_eq!(cfg.llm["openrouter"].api_key.as_deref(), Some("k"));
    }

    #[test]
    fn dangling_previous_default_is_not_promoted_to_fallback() {
        let mut cfg = AppConfig::default();
        cfg.agent.default_provider = Some("provider-that-was-deleted".to_string());
        apply_set_model(&mut cfg, &req("ollama-launch", "qwen3.8:latest"));
        assert!(cfg.agent.fallback_providers.is_empty());
    }

    #[test]
    fn unrelated_config_is_left_alone() {
        let mut cfg = AppConfig::default();
        cfg.agent.system_prompt = Some("persona do operador".to_string());
        cfg.gateway.port = 4242;
        apply_set_model(&mut cfg, &req("ollama-launch", "qwen3.8:latest"));
        assert_eq!(
            cfg.agent.system_prompt.as_deref(),
            Some("persona do operador")
        );
        assert_eq!(cfg.gateway.port, 4242);
    }
}

// =============================================================================
// `garraia config set-routing` — primary + backup provider in one write.
// =============================================================================

/// One side of a routing: which provider type, which model, which endpoint.
pub(crate) struct RoutingSide<'a> {
    pub provider: &'a str,
    pub model: &'a str,
    pub base_url: Option<&'a str>,
}

pub(crate) struct SetRoutingRequest<'a> {
    pub primary: RoutingSide<'a>,
    pub backup: Option<RoutingSide<'a>>,
    /// Credential for the *primary* provider, when it needs one. Never sourced
    /// from argv — see `run_set_routing`.
    pub api_key: Option<&'a str>,
}

/// Default endpoint per provider type, mirroring the gateway's boot arms in
/// `garraia-gateway/src/bootstrap/mod.rs`.
fn default_base_url(provider: &str) -> Option<&'static str> {
    match provider {
        "openrouter" => Some("https://openrouter.ai/api/v1"),
        "ollama" => Some("http://localhost:11434"),
        // llama-server's documented default HTTP port.
        "llamacpp" => Some("http://localhost:8080"),
        "openai" | "anthropic" => None, // provider clients carry their own default
        _ => None,
    }
}

/// The `llm:` map key a routing side occupies.
///
/// Keyed by provider type rather than by role, so re-running with the two sides
/// swapped updates the same two entries instead of accumulating orphans.
fn routing_key(provider: &str) -> String {
    provider.to_string()
}

/// Apply a `set-routing` request to `config` in place.
///
/// Writes both `llm:` entries, points `agent.default_provider` at the primary,
/// and makes the backup the head of `agent.fallback_providers`. Everything else
/// in the config is left alone — this runs unattended from AgentDeck, where
/// clobbering a hand-written channel or persona would be silent data loss.
///
/// Note the asymmetry with `apply_set_model`: that one *demotes* whatever was
/// default before, because it only knows about one provider. Here the caller
/// states both roles explicitly, so the previous default is simply replaced —
/// keeping it would contradict the routing the user just chose.
pub(crate) fn apply_set_routing(config: &mut AppConfig, req: &SetRoutingRequest<'_>) {
    let primary_key = routing_key(req.primary.provider);

    let primary_base = req
        .primary
        .base_url
        .map(str::to_string)
        .or_else(|| default_base_url(req.primary.provider).map(str::to_string));

    // Preserve an existing key when the caller did not supply one: rotating the
    // model must not silently de-authenticate the provider.
    let existing_key = config
        .llm
        .get(&primary_key)
        .and_then(|entry| entry.api_key.clone());

    config.llm.insert(
        primary_key.clone(),
        LlmProviderConfig {
            provider: req.primary.provider.to_string(),
            model: Some(req.primary.model.to_string()),
            api_key: req
                .api_key
                .map(str::to_string)
                .or(existing_key)
                .or_else(|| placeholder_key(req.primary.provider)),
            base_url: primary_base,
            extra: Default::default(),
        },
    );

    config.agent.default_provider = Some(primary_key.clone());

    match &req.backup {
        Some(backup) => {
            let backup_key = routing_key(backup.provider);
            let backup_base = backup
                .base_url
                .map(str::to_string)
                .or_else(|| default_base_url(backup.provider).map(str::to_string));
            let backup_existing = config
                .llm
                .get(&backup_key)
                .and_then(|entry| entry.api_key.clone());

            config.llm.insert(
                backup_key.clone(),
                LlmProviderConfig {
                    provider: backup.provider.to_string(),
                    model: Some(backup.model.to_string()),
                    api_key: backup_existing.or_else(|| placeholder_key(backup.provider)),
                    base_url: backup_base,
                    extra: Default::default(),
                },
            );

            // The backup leads the fallback chain; anything else the operator
            // configured stays behind it rather than being discarded.
            config
                .agent
                .fallback_providers
                .retain(|k| k != &backup_key && k != &primary_key);
            config.agent.fallback_providers.insert(0, backup_key);
        }
        None => {
            // No backup requested: the primary must not shadow itself.
            config
                .agent
                .fallback_providers
                .retain(|k| k != &primary_key);
        }
    }
}

/// Placeholder credential for providers whose client requires a non-empty key
/// but whose endpoint ignores it. `None` means "genuinely needs a real key".
fn placeholder_key(provider: &str) -> Option<String> {
    match provider {
        "ollama" => Some("ollama".to_string()),
        _ => None,
    }
}

/// `garraia config set-routing` — configure primary + backup in one shot.
///
/// The credential is read from **stdin**, never from a flag: argv is readable
/// by any process on the machine (`/proc/<pid>/cmdline` on Linux, `ps` almost
/// everywhere), so an `--api-key sk-...` would leak the key to every local user
/// for the lifetime of the process.
#[allow(clippy::too_many_arguments)]
pub fn run_set_routing(
    primary_provider: &str,
    primary_model: &str,
    primary_base_url: Option<&str>,
    backup_provider: Option<&str>,
    backup_model: Option<&str>,
    backup_base_url: Option<&str>,
    api_key_stdin: bool,
    dry_run: bool,
) -> Result<i32> {
    if primary_provider.trim().is_empty() || primary_model.trim().is_empty() {
        eprintln!("error: --primary-provider and --primary-model must not be empty");
        return Ok(2);
    }

    let backup = match (backup_provider, backup_model) {
        (Some(p), Some(m)) if !p.trim().is_empty() && !m.trim().is_empty() => Some(RoutingSide {
            provider: p,
            model: m,
            base_url: backup_base_url,
        }),
        (None, None) => None,
        _ => {
            eprintln!("error: --backup-provider and --backup-model must be given together");
            return Ok(2);
        }
    };

    if let Some(b) = &backup
        && b.provider == primary_provider
    {
        eprintln!(
            "error: backup provider must differ from the primary ('{primary_provider}'); \
             a provider cannot fall back to itself"
        );
        return Ok(2);
    }

    let api_key = if api_key_stdin {
        let mut buf = String::new();
        std::io::stdin()
            .read_line(&mut buf)
            .map_err(|e| anyhow::anyhow!("failed to read API key from stdin: {e}"))?;
        let trimmed = buf.trim().to_string();
        if trimmed.is_empty() {
            eprintln!("error: --api-key-stdin was given but stdin was empty");
            return Ok(2);
        }
        Some(trimmed)
    } else {
        None
    };

    let loader = ConfigLoader::new()?;
    loader.ensure_dirs()?;

    let mut config = match loader.load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "error: existing config could not be parsed: {}",
                truncate_error(format!("{e}"))
            );
            eprintln!("       fix it (or move it aside) before running `config set-routing`.");
            return Ok(65);
        }
    };

    apply_set_routing(
        &mut config,
        &SetRoutingRequest {
            primary: RoutingSide {
                provider: primary_provider,
                model: primary_model,
                base_url: primary_base_url,
            },
            backup,
            api_key: api_key.as_deref(),
        },
    );

    if dry_run {
        // Report the decision without touching disk. Never echo the key —
        // only whether one was supplied.
        println!(
            "dry-run: primário {primary_provider} ({primary_model}), backup {}, api_key {}",
            backup_provider
                .zip(backup_model)
                .map(|(p, m)| format!("{p} ({m})"))
                .unwrap_or_else(|| "nenhum".to_string()),
            if api_key.is_some() {
                "fornecida"
            } else {
                "não fornecida"
            }
        );
        return Ok(0);
    }

    // `save` serializes and clamps the file to 0600 (it can hold credentials).
    loader.save(&config)?;

    println!(
        "Roteamento definido:\n  primário: {primary_provider} ({primary_model})\n  backup:   {}\n\
         Config: {}/config.yml",
        backup_provider
            .zip(backup_model)
            .map(|(p, m)| format!("{p} ({m})"))
            .unwrap_or_else(|| "nenhum".to_string()),
        loader.config_dir().display()
    );
    Ok(0)
}

#[cfg(test)]
mod set_routing_tests {
    use super::*;

    fn side<'a>(provider: &'a str, model: &'a str) -> RoutingSide<'a> {
        RoutingSide {
            provider,
            model,
            base_url: None,
        }
    }

    fn apply(config: &mut AppConfig, api_key: Option<&str>) {
        apply_set_routing(
            config,
            &SetRoutingRequest {
                primary: side("openrouter", "z-ai/glm-5.3-flash"),
                backup: Some(side("ollama", "qwen3.5:2b")),
                api_key,
            },
        );
    }

    #[test]
    fn writes_both_sides_and_points_the_agent_at_them() {
        let mut config = AppConfig::default();
        apply(&mut config, Some("sk-or-v1-test"));

        assert_eq!(config.agent.default_provider.as_deref(), Some("openrouter"));
        assert_eq!(config.agent.fallback_providers, vec!["ollama".to_string()]);

        let primary = config.llm.get("openrouter").expect("primary entry");
        assert_eq!(primary.provider, "openrouter");
        assert_eq!(primary.model.as_deref(), Some("z-ai/glm-5.3-flash"));
        assert_eq!(primary.api_key.as_deref(), Some("sk-or-v1-test"));
        assert_eq!(
            primary.base_url.as_deref(),
            Some("https://openrouter.ai/api/v1")
        );

        let backup = config.llm.get("ollama").expect("backup entry");
        assert_eq!(backup.model.as_deref(), Some("qwen3.5:2b"));
        // Ollama needs no real credential, only a non-empty placeholder.
        assert_eq!(backup.api_key.as_deref(), Some("ollama"));
    }

    #[test]
    fn re_applying_the_same_routing_is_idempotent() {
        let mut config = AppConfig::default();
        apply(&mut config, Some("sk-or-v1-test"));
        let first = (
            config.agent.default_provider.clone(),
            config.agent.fallback_providers.clone(),
            config.llm.len(),
        );

        apply(&mut config, Some("sk-or-v1-test"));

        assert_eq!(config.agent.default_provider, first.0);
        assert_eq!(config.agent.fallback_providers, first.1);
        assert_eq!(config.llm.len(), first.2, "must not accumulate llm entries");
    }

    #[test]
    fn rotating_the_model_preserves_an_existing_credential() {
        // Changing model must never silently de-authenticate the provider.
        let mut config = AppConfig::default();
        apply(&mut config, Some("sk-or-v1-original"));

        apply_set_routing(
            &mut config,
            &SetRoutingRequest {
                primary: side("openrouter", "openrouter/auto"),
                backup: Some(side("ollama", "qwen3.5:2b")),
                api_key: None,
            },
        );

        let primary = config.llm.get("openrouter").expect("primary entry");
        assert_eq!(primary.model.as_deref(), Some("openrouter/auto"));
        assert_eq!(primary.api_key.as_deref(), Some("sk-or-v1-original"));
    }

    #[test]
    fn swapping_the_two_sides_does_not_leave_orphans() {
        let mut config = AppConfig::default();
        apply(&mut config, Some("sk-or-v1-test"));

        apply_set_routing(
            &mut config,
            &SetRoutingRequest {
                primary: side("ollama", "qwen3.5:2b"),
                backup: Some(side("openrouter", "z-ai/glm-5.3-flash")),
                api_key: None,
            },
        );

        assert_eq!(config.agent.default_provider.as_deref(), Some("ollama"));
        assert_eq!(
            config.agent.fallback_providers,
            vec!["openrouter".to_string()]
        );
        assert_eq!(config.llm.len(), 2, "keys are per provider, not per role");
    }

    #[test]
    fn a_routing_without_a_backup_does_not_shadow_itself() {
        let mut config = AppConfig::default();
        config
            .agent
            .fallback_providers
            .push("openrouter".to_string());

        apply_set_routing(
            &mut config,
            &SetRoutingRequest {
                primary: side("openrouter", "z-ai/glm-5.3-flash"),
                backup: None,
                api_key: None,
            },
        );

        assert!(
            !config
                .agent
                .fallback_providers
                .contains(&"openrouter".to_string()),
            "the primary must not also be its own fallback"
        );
    }

    #[test]
    fn other_fallbacks_stay_behind_the_new_backup() {
        let mut config = AppConfig::default();
        config.agent.fallback_providers = vec!["anthropic".to_string()];

        apply(&mut config, None);

        assert_eq!(
            config.agent.fallback_providers,
            vec!["ollama".to_string(), "anthropic".to_string()],
            "an operator-configured fallback must be kept, just demoted"
        );
    }

    #[test]
    fn unrelated_config_is_left_alone() {
        let mut config = AppConfig::default();
        config.agent.system_prompt = Some("persona do operador".to_string());
        config.agent.max_tokens = Some(4096);
        config.llm.insert(
            "anthropic".to_string(),
            LlmProviderConfig {
                provider: "anthropic".to_string(),
                model: Some("claude-sonnet-4-5-20250929".to_string()),
                api_key: Some("sk-ant-existing".to_string()),
                base_url: None,
                extra: Default::default(),
            },
        );

        apply(&mut config, Some("sk-or-v1-test"));

        assert_eq!(
            config.agent.system_prompt.as_deref(),
            Some("persona do operador")
        );
        assert_eq!(config.agent.max_tokens, Some(4096));
        let untouched = config.llm.get("anthropic").expect("pre-existing entry");
        assert_eq!(untouched.api_key.as_deref(), Some("sk-ant-existing"));
    }

    #[test]
    fn explicit_base_urls_override_the_provider_defaults() {
        let mut config = AppConfig::default();
        apply_set_routing(
            &mut config,
            &SetRoutingRequest {
                primary: RoutingSide {
                    provider: "openrouter",
                    model: "z-ai/glm-5.3-flash",
                    base_url: Some("https://proxy.internal/v1"),
                },
                backup: None,
                api_key: None,
            },
        );
        assert_eq!(
            config
                .llm
                .get("openrouter")
                .and_then(|e| e.base_url.as_deref()),
            Some("https://proxy.internal/v1")
        );
    }
}
