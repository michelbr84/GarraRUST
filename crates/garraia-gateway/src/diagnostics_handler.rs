//! Diagnostics endpoint (plan 0122 / PR-9).
//!
//! Surfaces a single read-only `GET /api/diagnostics` endpoint that runs
//! the per-subsystem health checks the Web Console renders in the
//! Diagnostics page. Each check has the same shape so the UI can render
//! a uniform checklist.
//!
//! Secret-free: when reporting on a secret (e.g. JWT_SECRET) we only
//! emit `configured: true|false`, never the value.
//!
//! The two voice checks (#1098) are the only ones that touch the network, and
//! only when voice mode is on: with TTS/STT down the gateway used to log a
//! warning nobody read, so `GET /api/tts` answered with a silent text fallback
//! and the operator never learned the server was gone. Surfacing the same
//! fact as an `error` row here is what makes it visible in the console.

use std::time::{Duration, SystemTime};

use axum::Json;
use axum::extract::State;
use garraia_common::ssrf::{self, IpScope, UrlPolicy};
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

/// Next step offered when the TTS server is unreachable. Mirrors the command
/// in `docs/voice.md` so the console points at the same thing the doc does.
const TTS_NEXT_STEP: &str =
    "Start the TTS server: `chatterbox-tts serve --host 127.0.0.1 --port 7860` (docs/voice.md).";

/// Same, for the STT server.
const STT_NEXT_STEP: &str =
    "Start the STT server: `fwsh serve --host 127.0.0.1 --port 9090` (docs/voice.md).";

/// Budget for one voice probe. Two of them run per report, and only when
/// voice mode is on — the Diagnostics page must not hang on a dead port.
const VOICE_PROBE_TIMEOUT: Duration = Duration::from_millis(1500);

/// Voice servers are local by design (`docs/voice.md`), so this call site
/// opts into [`IpScope::AllowPrivate`] — the same reason `Ollama` does. The
/// URL comes from config, but it still goes through `vet_url`: scheme,
/// host and blocked ranges (link-local metadata, CGNAT) are checked, and
/// `pinned_client` pins the resolved address so it cannot be swapped
/// between the check and the connect.
fn voice_policy() -> UrlPolicy {
    UrlPolicy::http_public(VOICE_PROBE_TIMEOUT, "garraia-gateway/diagnostics")
        .with_ip_scope(IpScope::AllowPrivate)
}

/// Outcome of reaching (or not) one configured voice endpoint. Kept as an
/// enum rather than a `Result` so [`voice_check`] stays a pure function —
/// that is what lets the mapping be tested without a network.
#[derive(Debug, Clone, PartialEq, Eq)]
enum VoiceProbe {
    /// Voice mode is off in this process: nothing to reach, nothing wrong.
    Disabled,
    /// Something answered; any HTTP status counts as up.
    Reachable,
    /// Vetted and dialled, but no answer. Carries a short, stable reason.
    Unreachable(&'static str),
    /// Not a URL this gateway may call: bad scheme, no host, blocked range.
    Invalid(String),
}

async fn probe_voice_endpoint(endpoint: &str) -> VoiceProbe {
    let policy = voice_policy();
    let vetted = match ssrf::vet_url(endpoint, &policy) {
        Ok(v) => v,
        Err(e) => return VoiceProbe::Invalid(e.to_string()),
    };
    let client = match ssrf::pinned_client(&vetted, &policy) {
        Ok(c) => c,
        Err(e) => return VoiceProbe::Invalid(e.to_string()),
    };
    match client.get(vetted.url.clone()).send().await {
        Ok(_) => VoiceProbe::Reachable,
        Err(e) => VoiceProbe::Unreachable(if e.is_connect() {
            "nothing listening (connection refused)"
        } else if e.is_timeout() {
            "no answer within 1.5s"
        } else {
            "request failed"
        }),
    }
}

/// Build the diagnostic row for one voice endpoint.
///
/// `Error` (not `Warning`) when a configured server is down: a warning is
/// exactly what #1098 reports as too easy to miss.
fn voice_check(
    id: &'static str,
    label: &'static str,
    endpoint: &str,
    next_step: &'static str,
    probe: VoiceProbe,
) -> DiagnosticCheck {
    let (status, detail, next_step) = match probe {
        VoiceProbe::Disabled => (
            CheckStatus::Skipped,
            "voice mode not enabled (start the gateway with --with-voice)".to_string(),
            None,
        ),
        VoiceProbe::Reachable => (CheckStatus::Ok, format!("reachable at {endpoint}"), None),
        VoiceProbe::Unreachable(reason) => (
            CheckStatus::Error,
            format!("{endpoint} unreachable: {reason}"),
            Some(next_step),
        ),
        VoiceProbe::Invalid(reason) => (
            CheckStatus::Error,
            format!("{endpoint}: {reason}"),
            Some("Set the voice endpoint to an http(s) URL on a host this gateway may reach."),
        ),
    };
    DiagnosticCheck {
        id,
        label,
        status,
        detail,
        next_step,
    }
}

/// Which TTS endpoint is actually in play — `hibiki_endpoint` only when the
/// configured provider is `hibiki`, otherwise the shared default.
fn active_tts_endpoint(config: &garraia_config::VoiceConfig) -> &str {
    if config.tts_provider.eq_ignore_ascii_case("hibiki") {
        &config.hibiki_endpoint
    } else {
        &config.tts_endpoint
    }
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

    // 8. JWT secret configured (presence only). Issue #824: the canonical
    // all-caps vault passphrase is the last fallback in AuthConfig::from_env,
    // so its presence also unlocks the auth flow.
    let jwt_configured = std::env::var("GARRAIA_JWT_SECRET").is_ok()
        || std::env::var("GarraIA_VAULT_PASSPHRASE").is_ok()
        || std::env::var("GARRAIA_VAULT_PASSPHRASE").is_ok();
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
            Some(
                "Optional on a local single-user gateway. To enable auth you need all \
                 four auth env vars plus Postgres, not this one alone; start with \
                 export GARRAIA_JWT_SECRET=$(openssl rand -hex 32). See docs/auth-config.md.",
            )
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

    // 13. TTS server reachable (#1098). Skipped when voice mode is off —
    //     there is nothing to reach, and nothing wrong, in that case.
    let voice_on = state.config.voice.enabled;
    let tts_endpoint = active_tts_endpoint(&state.config.voice).to_string();
    let tts_probe = if voice_on {
        probe_voice_endpoint(&tts_endpoint).await
    } else {
        VoiceProbe::Disabled
    };
    checks.push(voice_check(
        "voice.tts",
        "TTS server",
        &tts_endpoint,
        TTS_NEXT_STEP,
        tts_probe,
    ));

    // 14. STT server reachable (#1098).
    let stt_endpoint = state.config.voice.stt_endpoint.clone();
    let stt_probe = if voice_on {
        probe_voice_endpoint(&stt_endpoint).await
    } else {
        VoiceProbe::Disabled
    };
    checks.push(voice_check(
        "voice.stt",
        "STT server",
        &stt_endpoint,
        STT_NEXT_STEP,
        stt_probe,
    ));

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

#[cfg(test)]
mod tests {
    use super::*;

    const ENDPOINT: &str = "http://127.0.0.1:7860";

    /// #1098: com o modo voz desligado nao ha servidor para alcancar, e isso
    /// nao e defeito — a linha e `skipped`, nao `error`.
    #[test]
    fn voz_desligada_e_skipped_e_nao_erro() {
        let c = voice_check(
            "voice.tts",
            "TTS server",
            ENDPOINT,
            TTS_NEXT_STEP,
            VoiceProbe::Disabled,
        );
        assert!(matches!(c.status, CheckStatus::Skipped));
        assert!(
            c.next_step.is_none(),
            "skipped nao sugere proximo passo: nao ha nada a consertar"
        );
    }

    #[test]
    fn servidor_alcancavel_e_ok_sem_proximo_passo() {
        let c = voice_check(
            "voice.tts",
            "TTS server",
            ENDPOINT,
            TTS_NEXT_STEP,
            VoiceProbe::Reachable,
        );
        assert!(matches!(c.status, CheckStatus::Ok));
        assert_eq!(c.detail, format!("reachable at {ENDPOINT}"));
        assert!(c.next_step.is_none());
    }

    /// O ponto da issue: servidor fora tem de ser `error` com o comando que
    /// levanta o servico — um `warning` era exatamente o que passava batido.
    #[test]
    fn servidor_fora_e_error_com_o_comando_de_subida() {
        let c = voice_check(
            "voice.tts",
            "TTS server",
            ENDPOINT,
            TTS_NEXT_STEP,
            VoiceProbe::Unreachable("nothing listening (connection refused)"),
        );
        assert!(matches!(c.status, CheckStatus::Error));
        assert!(c.detail.contains(ENDPOINT));
        assert!(c.detail.contains("nothing listening"));
        assert_eq!(c.next_step, Some(TTS_NEXT_STEP));
        assert!(
            TTS_NEXT_STEP.contains("chatterbox-tts serve"),
            "o proximo passo tem de citar o comando real da docs"
        );
        assert!(STT_NEXT_STEP.contains("9090"), "e o STT a porta certa");
    }

    /// Endpoint que nao e URL chamavel tambem e `error` — falha fechada, nunca
    /// se finge que esta tudo bem.
    #[test]
    fn endpoint_invalido_e_error() {
        let c = voice_check(
            "voice.stt",
            "STT server",
            "file:///etc/passwd",
            STT_NEXT_STEP,
            VoiceProbe::Invalid("scheme not allowed".to_string()),
        );
        assert!(matches!(c.status, CheckStatus::Error));
        assert!(c.next_step.is_some());
        assert!(
            c.detail.contains("file:///etc/passwd"),
            "detalhe nomeia o endpoint rejeitado: {}",
            c.detail
        );
    }

    /// `hibiki` tem endpoint proprio; o default e o do chatterbox.
    #[test]
    fn provider_escolhe_o_endpoint() {
        let mut cfg = garraia_config::VoiceConfig::default();
        assert_eq!(active_tts_endpoint(&cfg), "http://127.0.0.1:7860");
        cfg.hibiki_endpoint = "http://127.0.0.1:8912".to_string();
        assert_eq!(
            active_tts_endpoint(&cfg),
            "http://127.0.0.1:7860",
            "sem trocar o provider, o endpoint do hibiki nao vale"
        );
        cfg.tts_provider = "hibiki".to_string();
        assert_eq!(active_tts_endpoint(&cfg), "http://127.0.0.1:8912");
    }

    /// O escopo privado e o que faz os servicos locais de voz alcancaveis; sem
    /// ele o proprio 127.0.0.1 do default seria barrado pelo guarda.
    #[test]
    fn a_policy_de_voz_permite_loopback() {
        assert_eq!(voice_policy().ip_scope, IpScope::AllowPrivate);
        assert!(
            ssrf::vet_url(ENDPOINT, &voice_policy()).is_ok(),
            "o endpoint default de voz tem de passar pelo vet_url"
        );
        assert!(
            ssrf::vet_url("http://169.254.169.254/", &voice_policy()).is_err(),
            "metadata de cloud continua barrado mesmo no escopo privado"
        );
    }
}
