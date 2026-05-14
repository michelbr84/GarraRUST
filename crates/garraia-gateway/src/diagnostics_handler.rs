//! Diagnostics endpoint (plan 0122 / PR-9).
//!
//! Surfaces a single read-only `GET /api/diagnostics` endpoint that runs
//! the per-subsystem health checks the Web Console renders in the
//! Diagnostics page. Each check has the same shape so the UI can render
//! a uniform checklist.
//!
//! Secret-free: when reporting on a secret (e.g. JWT_SECRET) we only
//! emit `configured: true|false`, never the value.

use std::time::SystemTime;

use axum::Json;
use axum::extract::State;
use serde::Serialize;

use crate::state::SharedState;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum CheckStatus {
    /// All good.
    Ok,
    /// Functional but with a caveat (Ollama optional, etc.).
    Warning,
    /// Broken — needs the user's attention.
    Error,
    /// Not applicable (the subsystem isn't enabled in this build).
    Skipped,
}

#[derive(Debug, Clone, Serialize)]
struct DiagnosticCheck {
    /// Stable id ("gateway.responds", "secrets.jwt", ...).
    id: &'static str,
    /// Human label rendered in the UI.
    label: &'static str,
    status: CheckStatus,
    /// Short evidence string. Never contains secret values.
    detail: String,
    /// Suggested next step when status != Ok. Empty when not applicable.
    next_step: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticsReport {
    /// Aggregate worst-case status across all checks.
    status: &'static str,
    /// Process version + build info.
    version: &'static str,
    /// Seconds since boot.
    uptime_secs: u64,
    /// Wall-clock timestamp at report generation (server's clock, UTC).
    generated_at: String,
    /// Each per-subsystem check.
    checks: Vec<DiagnosticCheck>,
}

fn now_iso8601() -> String {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Simple ISO-8601 formatter (UTC). Pulling chrono would be one more
    // dep; this is good enough for a diagnostic timestamp.
    let secs = now % 60;
    let mins = (now / 60) % 60;
    let hours = (now / 3600) % 24;
    let days = now / 86400;
    // Days since 1970-01-01.
    let (year, month, day) = days_to_ymd(days as i64);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, mins, secs
    )
}

/// Convert "days since 1970-01-01" to (year, month, day). No external dep.
fn days_to_ymd(days: i64) -> (i32, u32, u32) {
    // Algorithm from Howard Hinnant's date library, public domain.
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let year = (y + if m <= 2 { 1 } else { 0 }) as i32;
    (year, m, d)
}

/// GET /api/diagnostics — full diagnostic report.
pub async fn diagnostics_handler(State(state): State<SharedState>) -> Json<DiagnosticsReport> {
    let mut checks: Vec<DiagnosticCheck> = Vec::new();

    // 1. Gateway responds — we are responding right now, so this is OK.
    checks.push(DiagnosticCheck {
        id: "gateway.responds",
        label: "Gateway responds",
        status: CheckStatus::Ok,
        detail: format!(
            "Live at {}:{}",
            state.config.gateway.host, state.config.gateway.port
        ),
        next_step: None,
    });

    // 2. Listener port valid range.
    let port = state.config.gateway.port;
    checks.push(DiagnosticCheck {
        id: "gateway.port",
        label: "Listener port valid",
        status: if (1..=65535).contains(&port) {
            CheckStatus::Ok
        } else {
            CheckStatus::Error
        },
        detail: format!("port={}", port),
        next_step: if (1..=65535).contains(&port) {
            None
        } else {
            Some("Set gateway.port in garraia.toml to a value 1..=65535.")
        },
    });

    // 3. Config dir exists.
    let cfg_dir = garraia_config::ConfigLoader::default_config_dir();
    let cfg_exists = cfg_dir.exists();
    checks.push(DiagnosticCheck {
        id: "config.dir",
        label: "Config directory",
        status: if cfg_exists {
            CheckStatus::Ok
        } else {
            CheckStatus::Warning
        },
        detail: format!("{}", cfg_dir.display()),
        next_step: if cfg_exists {
            None
        } else {
            Some("Run `garraia init` to scaffold ~/.garraia.")
        },
    });

    // 4. .env presence (best-effort — env vars are loaded by the host shell,
    // but a `.env` file in CWD is the most common dev setup).
    let dotenv = std::path::Path::new(".env").exists();
    checks.push(DiagnosticCheck {
        id: "env.dotenv",
        label: ".env file in CWD",
        status: if dotenv {
            CheckStatus::Ok
        } else {
            CheckStatus::Warning
        },
        detail: if dotenv {
            ".env loaded".to_string()
        } else {
            "no .env in CWD (env vars must come from the parent shell)".to_string()
        },
        next_step: if dotenv {
            None
        } else {
            Some("Copy .env.example to .env and fill in the values you need.")
        },
    });

    // 5. Default provider active.
    let default_provider = state.agents.default_provider_id();
    checks.push(DiagnosticCheck {
        id: "provider.default",
        label: "Default LLM provider",
        status: if default_provider.is_some() {
            CheckStatus::Ok
        } else {
            CheckStatus::Error
        },
        detail: default_provider
            .clone()
            .unwrap_or_else(|| "none registered".to_string()),
        next_step: if default_provider.is_some() {
            None
        } else {
            Some(
                "Register at least one LLM provider via /api/providers POST or seed an API key in .env.",
            )
        },
    });

    // 6. Telegram configured (env var presence — never the value).
    let tg_configured =
        std::env::var("TELOXIDE_TOKEN").is_ok() || std::env::var("TELEGRAM_BOT_TOKEN").is_ok();
    checks.push(DiagnosticCheck {
        id: "channel.telegram",
        label: "Telegram channel",
        status: if tg_configured {
            CheckStatus::Ok
        } else {
            CheckStatus::Skipped
        },
        detail: if tg_configured {
            "TELOXIDE_TOKEN configured".to_string()
        } else {
            "optional channel — set TELOXIDE_TOKEN to enable".to_string()
        },
        next_step: None,
    });

    // 7. Discord configured.
    let dc_configured = std::env::var("DISCORD_TOKEN").is_ok();
    checks.push(DiagnosticCheck {
        id: "channel.discord",
        label: "Discord channel",
        status: if dc_configured {
            CheckStatus::Ok
        } else {
            CheckStatus::Skipped
        },
        detail: if dc_configured {
            "DISCORD_TOKEN configured".to_string()
        } else {
            "optional channel — set DISCORD_TOKEN to enable".to_string()
        },
        next_step: None,
    });

    // 8. JWT secret configured (presence only).
    let jwt_configured = std::env::var("GARRAIA_JWT_SECRET").is_ok()
        || std::env::var("GarraIA_VAULT_PASSPHRASE").is_ok();
    checks.push(DiagnosticCheck {
        id: "secrets.jwt",
        label: "JWT signing secret",
        status: if jwt_configured {
            CheckStatus::Ok
        } else {
            CheckStatus::Warning
        },
        detail: if jwt_configured {
            "configured (value masked)".to_string()
        } else {
            "missing — /v1/auth/* and /auth/* will return 503".to_string()
        },
        next_step: if jwt_configured {
            None
        } else {
            Some("Set GARRAIA_JWT_SECRET to a >=32-byte random value.")
        },
    });

    // 9. Gateway exposed on 0.0.0.0 — security warning.
    let bind = state.config.gateway.host.as_str();
    let exposed = bind == "0.0.0.0" || bind == "::";
    checks.push(DiagnosticCheck {
        id: "security.bind",
        label: "Listener binding",
        status: if exposed {
            CheckStatus::Warning
        } else {
            CheckStatus::Ok
        },
        detail: format!("host={}", bind),
        next_step: if exposed {
            Some(
                "Binding to all interfaces — make sure a firewall protects the port or switch to 127.0.0.1.",
            )
        } else {
            None
        },
    });

    // 10. TLS — informational.
    let tls_on = state.config.gateway.tls_cert_path.is_some();
    checks.push(DiagnosticCheck {
        id: "security.tls",
        label: "TLS",
        status: if tls_on {
            CheckStatus::Ok
        } else {
            CheckStatus::Skipped
        },
        detail: if tls_on {
            "TLS enabled".to_string()
        } else {
            "plain HTTP — fine for localhost, enable TLS for prod".to_string()
        },
        next_step: None,
    });

    // 11. Active channels.
    let channels: Vec<String> = state
        .channels
        .read()
        .await
        .list()
        .into_iter()
        .map(|s| s.to_string())
        .collect();
    checks.push(DiagnosticCheck {
        id: "runtime.channels",
        label: "Active channels",
        status: if channels.is_empty() {
            CheckStatus::Warning
        } else {
            CheckStatus::Ok
        },
        detail: if channels.is_empty() {
            "none".to_string()
        } else {
            channels.join(", ")
        },
        next_step: if channels.is_empty() {
            Some("At least 'web' is expected. Check the bootstrap log for channel registration errors.")
        } else {
            None
        },
    });

    // 12. Active sessions count.
    checks.push(DiagnosticCheck {
        id: "runtime.sessions",
        label: "Active sessions",
        status: CheckStatus::Ok,
        detail: format!("{} in-memory", state.sessions.len()),
        next_step: None,
    });

    // Aggregate status: error > warning > ok (skipped is neutral).
    let status = if checks
        .iter()
        .any(|c| matches!(c.status, CheckStatus::Error))
    {
        "error"
    } else if checks
        .iter()
        .any(|c| matches!(c.status, CheckStatus::Warning))
    {
        "warning"
    } else {
        "ok"
    };

    Json(DiagnosticsReport {
        status,
        version: env!("CARGO_PKG_VERSION"),
        uptime_secs: state.boot_time.elapsed().as_secs(),
        generated_at: now_iso8601(),
        checks,
    })
}
