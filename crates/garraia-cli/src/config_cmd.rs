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
use garraia_config::{ConfigCheck, ConfigLoader, Severity};

/// Maximum length of the `format!("{e}")` snippet emitted on parse failure.
/// Keeps error output bounded without losing the leading line/column context.
const PARSE_ERROR_MAX_LEN: usize = 256;

fn truncate_error(raw: String) -> String {
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
    if check.has_errors() {
        2
    } else if strict && check.has_warnings() {
        2
    } else {
        0
    }
}

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
