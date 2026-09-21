//! Schema-driven Settings Registry (plan 0121 / PR-8).
//!
//! Three endpoints surfaced under `/api/settings/*`:
//! - `GET  /api/settings/schema`    — full schema (all known settings + meta).
//! - `GET  /api/settings/effective` — live values, secrets MASKED.
//! - `PATCH /api/settings`          — validate + audit; **dry-run only** for
//!   now. Full persistence (TOML backup + atomic write + hot-reload) lands
//!   in plan 0121a.
//!
//! ## Secret invariant
//!
//! Settings carrying `secret: true` are write-only. The
//! `/effective` endpoint emits `{configured: true|false}` instead of the
//! value. The audit log records that a write happened, never the value.
//!
//! ## Why dry-run
//!
//! Persisting a settings change correctly means: (1) validate against the
//! schema, (2) load the current TOML file, (3) write a `.bak` copy, (4)
//! merge the change atomically, (5) trigger `config_rx` watcher reload,
//! (6) decide if a process restart is needed. That stack is plan 0121a.
//! For PR-8 we validate and audit the request, return the right
//! `requires_restart` signal, and the UI shows "applied for this
//! session" without actually mutating disk. Net: zero risk of corrupting
//! `garraia.toml` from a half-finished UI today.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::state::SharedState;

// ─── Schema ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SettingType {
    String,
    Integer,
    Boolean,
    Enum,
    Secret,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SettingCategory {
    General,
    Gateway,
    Providers,
    Channels,
    Secrets,
    Logs,
    Security,
    Appearance,
    Experimental,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SettingSource {
    /// Compiled-in default.
    Default,
    /// Read from `garraia.toml`.
    File,
    /// Read from a `GARRAIA_*` env var.
    Env,
    /// Set at runtime (admin API, CLI, etc.).
    Runtime,
}

/// One row of the settings registry.
#[derive(Debug, Clone, Serialize)]
pub struct SettingSchema {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub category: SettingCategory,
    #[serde(rename = "type")]
    pub type_: SettingType,
    pub default: serde_json::Value,
    pub editable: bool,
    pub secret: bool,
    pub requires_restart: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub choices: Option<Vec<&'static str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validation: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<&'static str>,
}

/// Static enumeration of every setting the Web Console knows about.
/// **Source of truth.** Adding a row here is the canonical way to expose a
/// new knob through the Settings Registry. Returned as a fresh `Vec` per
/// call (rare, low-frequency endpoint — allocation cost is negligible and
/// keeps the schema free to embed `serde_json::Value` defaults and
/// `Vec<&str>` choice lists that can't live in a `const`).
fn settings() -> Vec<SettingSchema> {
    vec![
        // — General —
        SettingSchema {
            id: "general.version",
            label: "Gateway version",
            description: "Compiled binary version. Read-only.",
            category: SettingCategory::General,
            type_: SettingType::String,
            default: serde_json::Value::Null,
            editable: false,
            secret: false,
            requires_restart: false,
            choices: None,
            validation: None,
            warning: None,
        },
        // — Gateway —
        SettingSchema {
            id: "gateway.host",
            label: "Listener host",
            description: "Bind address for the HTTP/WS listener.",
            category: SettingCategory::Gateway,
            type_: SettingType::String,
            default: serde_json::Value::Null,
            editable: true,
            secret: false,
            requires_restart: true,
            choices: None,
            validation: Some("non-empty IPv4/IPv6/hostname"),
            warning: Some(
                "Binding to 0.0.0.0 exposes the gateway to the network — confirm with a firewall.",
            ),
        },
        SettingSchema {
            id: "gateway.port",
            label: "Listener port",
            description: "TCP port for the HTTP/WS listener.",
            category: SettingCategory::Gateway,
            type_: SettingType::Integer,
            default: serde_json::json!(3888),
            editable: true,
            secret: false,
            requires_restart: true,
            choices: None,
            validation: Some("1..=65535"),
            warning: None,
        },
        SettingSchema {
            id: "gateway.tls_enabled",
            label: "TLS enabled",
            description: "Serve HTTPS via the configured cert + key.",
            category: SettingCategory::Gateway,
            type_: SettingType::Boolean,
            default: serde_json::json!(false),
            editable: false,
            secret: false,
            requires_restart: true,
            choices: None,
            validation: None,
            warning: Some("Toggle via gateway.tls_cert_path / tls_key_path — derived flag."),
        },
        // — Providers —
        SettingSchema {
            id: "providers.default",
            label: "Default LLM provider",
            description: "Provider id used when a session doesn't pin one.",
            category: SettingCategory::Providers,
            type_: SettingType::String,
            default: serde_json::Value::Null,
            editable: true,
            secret: false,
            requires_restart: false,
            choices: None,
            validation: Some("must match a registered provider id"),
            warning: None,
        },
        // — Channels —
        SettingSchema {
            id: "channels.active",
            label: "Active channels",
            description: "Live channel registry list. Read-only here — toggle via the Channels page.",
            category: SettingCategory::Channels,
            type_: SettingType::String,
            default: serde_json::Value::Null,
            editable: false,
            secret: false,
            requires_restart: false,
            choices: None,
            validation: None,
            warning: None,
        },
        // — Secrets — (write-only; .effective never returns the value)
        SettingSchema {
            id: "secrets.gateway_api_key",
            label: "Gateway API key",
            description: "Required for /admin endpoints. Write-only.",
            category: SettingCategory::Secrets,
            type_: SettingType::Secret,
            default: serde_json::Value::Null,
            editable: true,
            secret: true,
            requires_restart: true,
            choices: None,
            validation: Some("min 16 chars"),
            warning: Some(
                "Losing this key locks you out of /admin. Save it externally before applying.",
            ),
        },
        SettingSchema {
            id: "secrets.jwt_secret",
            label: "JWT signing secret",
            description: "HS256 secret for /v1/auth/* and /auth/* endpoints.",
            category: SettingCategory::Secrets,
            type_: SettingType::Secret,
            default: serde_json::Value::Null,
            editable: true,
            secret: true,
            requires_restart: true,
            choices: None,
            validation: Some(">= 32 bytes"),
            warning: Some("Rotating invalidates all existing JWTs — every client must re-login."),
        },
        SettingSchema {
            id: "secrets.refresh_hmac_secret",
            label: "Refresh-token HMAC secret",
            description: "Separate HMAC-SHA256 secret for refresh-token verification.",
            category: SettingCategory::Secrets,
            type_: SettingType::Secret,
            default: serde_json::Value::Null,
            editable: true,
            secret: true,
            requires_restart: true,
            choices: None,
            validation: Some(">= 32 bytes"),
            warning: None,
        },
        // — Logs —
        SettingSchema {
            id: "logs.level",
            label: "Log level",
            description: "Tracing subscriber filter directive.",
            category: SettingCategory::Logs,
            type_: SettingType::Enum,
            default: serde_json::json!("info"),
            editable: true,
            secret: false,
            requires_restart: true,
            choices: Some(vec!["trace", "debug", "info", "warn", "error"]),
            validation: None,
            warning: None,
        },
        // — Security —
        SettingSchema {
            id: "security.cors_origins",
            label: "CORS origins",
            description: "Comma-separated allow-list. Empty = same-origin only.",
            category: SettingCategory::Security,
            type_: SettingType::String,
            default: serde_json::Value::Null,
            editable: true,
            secret: false,
            requires_restart: true,
            choices: None,
            validation: Some("comma-separated absolute origins"),
            warning: Some("Adding `*` disables origin checks — never do this in production."),
        },
        SettingSchema {
            id: "security.rate_limit_rpm",
            label: "Rate limit (req/min)",
            description: "Per-IP requests-per-minute on /api/*.",
            category: SettingCategory::Security,
            type_: SettingType::Integer,
            default: serde_json::json!(120),
            editable: true,
            secret: false,
            requires_restart: true,
            choices: None,
            validation: Some("1..=10000"),
            warning: None,
        },
        // — Security: sandbox por tool (#1222 / #1225) —
        //
        // Read-only aqui de proposito. O PATCH desta rota e dry-run (plan
        // 0121a persiste): uma chave editavel faria a UI dizer "aplicado" para
        // um controle de contencao que continuaria desligado no proximo boot.
        // Ate la o registro serve para o operador VER o estado, que era o que
        // faltava — a #1225 existe porque ninguem conseguia nem ligar nem ver.
        SettingSchema {
            id: "security.sandbox_mode",
            label: "Tool sandbox mode",
            description: "agent.sandbox.mode — off | all | allowlist. Read-only here; edit garraia.toml.",
            category: SettingCategory::Security,
            type_: SettingType::Enum,
            default: serde_json::json!("off"),
            editable: false,
            secret: false,
            requires_restart: true,
            choices: Some(vec!["off", "all", "allowlist"]),
            validation: None,
            warning: Some(
                "Only the `bash` tool is wrapped today; run_tests/git_diff/repo_search still spawn on the host. `ssh` is remote execution, not a sandbox. Unix only.",
            ),
        },
        SettingSchema {
            id: "security.sandbox_backend",
            label: "Tool sandbox backend",
            description: "agent.sandbox.backend — docker | podman | ssh. The SSH host is never reported here.",
            category: SettingCategory::Security,
            type_: SettingType::String,
            default: serde_json::Value::Null,
            editable: false,
            secret: false,
            requires_restart: true,
            choices: None,
            validation: None,
            warning: Some(
                "Empty while the mode is not `off` means every sandboxed command fails closed. `ssh` also fails closed until agent.sandbox.network_disabled and agent.sandbox.mount_workdir are an explicit false.",
            ),
        },
        // — Appearance —
        SettingSchema {
            id: "appearance.default_theme",
            label: "Default theme",
            description: "Theme used by first-time visitors before a localStorage preference exists.",
            category: SettingCategory::Appearance,
            type_: SettingType::Enum,
            default: serde_json::json!("dark"),
            editable: true,
            secret: false,
            requires_restart: false,
            choices: Some(vec!["light", "dark"]),
            validation: None,
            warning: None,
        },
        SettingSchema {
            id: "appearance.default_skin",
            label: "Default skin",
            description: "Skin preset used by first-time visitors.",
            category: SettingCategory::Appearance,
            type_: SettingType::Enum,
            default: serde_json::json!("garra-blue"),
            editable: true,
            secret: false,
            requires_restart: false,
            choices: Some(vec![
                "garra-blue",
                "aurora-admin",
                "editorial",
                "cyber-garra",
            ]),
            validation: None,
            warning: None,
        },
        // — Experimental —
        SettingSchema {
            id: "experimental.streaming",
            label: "Streaming responses",
            description: "Enable Server-Sent-Events / chunked streaming on /api/sessions/*/messages.",
            category: SettingCategory::Experimental,
            type_: SettingType::Boolean,
            default: serde_json::json!(false),
            editable: true,
            secret: false,
            requires_restart: true,
            choices: None,
            validation: None,
            warning: Some("Experimental — may interact badly with existing channels."),
        },
    ]
}

/// GET /api/settings/schema — full schema listing.
pub async fn schema_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "settings": settings() }))
}

// ─── Effective ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
struct EffectiveValue {
    id: &'static str,
    /// For non-secret settings, this is the live value. For secrets, this is
    /// always `null` — see `configured` instead.
    value: serde_json::Value,
    /// True iff a secret is currently configured (i.e. non-empty), without
    /// leaking the value itself. Always `null` for non-secret settings.
    configured: Option<bool>,
    source: SettingSource,
}

/// Valor e origem das duas linhas de sandbox (#1225) em
/// `/api/settings/effective`.
///
/// Funcao **pura**, separada do handler de proposito. Duas razoes:
///
/// 1. Ela decide o que o operador LE sobre um controle de contencao. Dizer
///    `Default` para uma secao que ele escreveu — ou `File` para uma que ele
///    nunca tocou — e a interface mentindo sobre onde o sandbox esta ligado,
///    que e a classe de bug que a #1225 existe para fechar.
/// 2. `effective_value_for` so e alcancavel montando um `AppState` inteiro, e
///    e por isso que este arquivo esta em 0% de cobertura. Extrair a decisao
///    e o que a torna exercitavel sem subir o mundo.
///
/// O `ssh_host` **nunca** sai daqui: nao e segredo, mas nomeia
/// infraestrutura, e a rota e auth-free. O backend sai so como discriminante.
fn sandbox_effective(
    id: &str,
    sb: &garraia_config::SandboxConfig,
) -> (serde_json::Value, SettingSource) {
    use serde_json::{Value, json};

    // Secao ausente => os dois campos estao no default compilado. Reportar
    // `File` faria a UI afirmar que alguem escolheu `off`.
    let source = if sb.mode == garraia_config::SandboxMode::Off && sb.backend.is_none() {
        SettingSource::Default
    } else {
        SettingSource::File
    };
    let value = if id == "security.sandbox_mode" {
        json!(match sb.mode {
            garraia_config::SandboxMode::Off => "off",
            garraia_config::SandboxMode::All => "all",
            garraia_config::SandboxMode::Allowlist => "allowlist",
        })
    } else {
        match sb.backend {
            None => Value::Null,
            Some(garraia_config::SandboxBackendKind::Docker) => json!("docker"),
            Some(garraia_config::SandboxBackendKind::Podman) => json!("podman"),
            Some(garraia_config::SandboxBackendKind::Ssh) => json!("ssh"),
        }
    };
    (value, source)
}

fn effective_value_for(s: &SettingSchema, state: &SharedState) -> EffectiveValue {
    use serde_json::{Value, json};
    let (value, configured, source) = match s.id {
        "general.version" => (
            Value::String(env!("CARGO_PKG_VERSION").to_string()),
            None,
            SettingSource::Default,
        ),
        "gateway.host" => (
            Value::String(state.config.gateway.host.clone()),
            None,
            SettingSource::File,
        ),
        "gateway.port" => (json!(state.config.gateway.port), None, SettingSource::File),
        "gateway.tls_enabled" => (
            json!(state.config.gateway.tls_cert_path.is_some()),
            None,
            SettingSource::File,
        ),
        "providers.default" => (
            state
                .agents
                .default_provider_id()
                .map(Value::String)
                .unwrap_or(Value::Null),
            None,
            SettingSource::Runtime,
        ),
        "channels.active" => {
            // Async access deferred — read the list outside this fn.
            (Value::Null, None, SettingSource::Runtime)
        }
        // #1241: `is_some()` reportava `configured: true` para um
        // `api_key: "  "` que deixa o gate de `/api/*` e `/ws` DESLIGADO.
        // `api_key_configurada` e a mesma regra que o `ApiKeyGate` aplica.
        "secrets.gateway_api_key" => (
            Value::Null,
            Some(state.config.gateway.api_key_configurada()),
            SettingSource::File,
        ),
        "secrets.jwt_secret" => (
            Value::Null,
            Some(std::env::var("GARRAIA_JWT_SECRET").is_ok()),
            SettingSource::Env,
        ),
        "secrets.refresh_hmac_secret" => (
            Value::Null,
            Some(std::env::var("GARRAIA_REFRESH_HMAC_SECRET").is_ok()),
            SettingSource::Env,
        ),
        "logs.level" => (
            Value::String(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string())),
            None,
            SettingSource::Env,
        ),
        "security.cors_origins" => (
            // CORS origin string isn't a single field — emit "—" placeholder.
            Value::String("(see gateway.cors)".into()),
            None,
            SettingSource::File,
        ),
        "security.rate_limit_rpm" => (json!(120), None, SettingSource::Default),
        // #1225: so o discriminante. `ssh_host` nao e segredo, mas nomeia
        // infraestrutura e nao acrescenta nada ao diagnostico — a rota e
        // auth-free, entao o que nao precisa sair nao sai.
        "security.sandbox_mode" | "security.sandbox_backend" => {
            let (value, source) = sandbox_effective(s.id, &state.config.agent.sandbox);
            (value, None, source)
        }
        "appearance.default_theme" => (json!("dark"), None, SettingSource::Default),
        "appearance.default_skin" => (json!("garra-blue"), None, SettingSource::Default),
        "experimental.streaming" => (json!(false), None, SettingSource::Default),
        _ => (Value::Null, None, SettingSource::Default),
    };
    EffectiveValue {
        id: s.id,
        value,
        configured,
        source,
    }
}

/// GET /api/settings/effective — live values, secrets masked.
pub async fn effective_handler(State(state): State<SharedState>) -> Json<serde_json::Value> {
    let mut rows: Vec<EffectiveValue> = settings()
        .iter()
        .map(|s| effective_value_for(s, &state))
        .collect();

    // Plug in async-required values that effective_value_for couldn't fetch.
    if let Some(row) = rows.iter_mut().find(|r| r.id == "channels.active") {
        let list: Vec<String> = state
            .channels
            .read()
            .await
            .list()
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        row.value = serde_json::Value::String(list.join(", "));
    }

    Json(serde_json::json!({ "settings": rows }))
}

// ─── PATCH ─────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PatchSettingsRequest {
    /// Map of `id -> new_value`. Unknown ids are rejected.
    pub patch: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct PatchSettingsResponse {
    pub ok: bool,
    pub applied: Vec<String>,
    pub rejected: Vec<RejectedEntry>,
    pub requires_restart: bool,
    pub dry_run: bool,
}

#[derive(Debug, Serialize)]
pub struct RejectedEntry {
    pub id: String,
    pub reason: String,
}

/// PATCH /api/settings — validate + audit. Dry-run for now (plan 0121a).
pub async fn patch_handler(
    Json(body): Json<PatchSettingsRequest>,
) -> (StatusCode, Json<PatchSettingsResponse>) {
    let mut applied: Vec<String> = Vec::new();
    let mut rejected: Vec<RejectedEntry> = Vec::new();
    let mut requires_restart = false;
    let all = settings();

    for (id, new_value) in body.patch.into_iter() {
        let Some(schema) = all.iter().find(|s| s.id == id) else {
            rejected.push(RejectedEntry {
                id: id.clone(),
                reason: "unknown setting id".into(),
            });
            continue;
        };
        if !schema.editable {
            rejected.push(RejectedEntry {
                id: id.clone(),
                reason: "setting is read-only".into(),
            });
            continue;
        }
        // Type validation
        let type_ok = match schema.type_ {
            SettingType::String | SettingType::Enum | SettingType::Secret => new_value.is_string(),
            SettingType::Integer => new_value.is_i64() || new_value.is_u64(),
            SettingType::Boolean => new_value.is_boolean(),
        };
        if !type_ok {
            rejected.push(RejectedEntry {
                id: id.clone(),
                reason: format!("type mismatch (expected {:?})", schema.type_),
            });
            continue;
        }
        // Enum validation
        if matches!(schema.type_, SettingType::Enum)
            && let (Some(s), Some(choices)) = (new_value.as_str(), schema.choices.as_ref())
            && !choices.contains(&s)
        {
            rejected.push(RejectedEntry {
                id: id.clone(),
                reason: format!("not in {:?}", choices),
            });
            continue;
        }
        applied.push(id.clone());
        if schema.requires_restart {
            requires_restart = true;
        }
        // AUDIT — log the WRITE event WITHOUT the value (secret-safe).
        info!(
            target: "garraia.settings.audit",
            setting_id = %id,
            secret = schema.secret,
            requires_restart = schema.requires_restart,
            "settings PATCH applied (dry-run, plan 0121a will persist)"
        );
    }

    let status = if rejected.is_empty() {
        StatusCode::OK
    } else {
        StatusCode::BAD_REQUEST
    };

    (
        status,
        Json(PatchSettingsResponse {
            ok: rejected.is_empty(),
            applied,
            rejected,
            requires_restart,
            dry_run: true,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use garraia_config::{SandboxBackendKind, SandboxConfig, SandboxMode};

    const MODO: &str = "security.sandbox_mode";
    const BACKEND: &str = "security.sandbox_backend";

    /// #1225 S3: secao ausente e config compilada, nao escolha do operador.
    #[test]
    fn sandbox_desligado_reporta_default() {
        let sb = SandboxConfig::default();
        assert_eq!(sb.mode, SandboxMode::Off);
        assert!(sb.backend.is_none());

        let (valor, origem) = sandbox_effective(MODO, &sb);
        assert_eq!(valor, serde_json::json!("off"));
        assert_eq!(origem, SettingSource::Default);

        let (valor, origem) = sandbox_effective(BACKEND, &sb);
        assert_eq!(valor, serde_json::Value::Null);
        assert_eq!(origem, SettingSource::Default);
    }

    /// Qualquer configuracao explicita vira `File` — inclusive `mode: off`
    /// com um backend escrito, que e o caso que a condicao composta existe
    /// para pegar: o operador desligou temporariamente, mas escreveu a secao.
    #[test]
    fn sandbox_configurado_reporta_file() {
        let casos = [
            SandboxConfig {
                mode: SandboxMode::All,
                backend: Some(SandboxBackendKind::Docker),
                ..SandboxConfig::default()
            },
            SandboxConfig {
                mode: SandboxMode::Allowlist,
                backend: Some(SandboxBackendKind::Podman),
                ..SandboxConfig::default()
            },
            // `off` COM backend: a secao existe no arquivo.
            SandboxConfig {
                mode: SandboxMode::Off,
                backend: Some(SandboxBackendKind::Ssh),
                ..SandboxConfig::default()
            },
            // `all` SEM backend: config invalida (o check recusa), mas a
            // origem continua sendo o arquivo — foi alguem que escreveu.
            SandboxConfig {
                mode: SandboxMode::All,
                backend: None,
                ..SandboxConfig::default()
            },
        ];
        for sb in casos {
            assert_eq!(
                sandbox_effective(MODO, &sb).1,
                SettingSource::File,
                "sb = {sb:?}"
            );
            assert_eq!(
                sandbox_effective(BACKEND, &sb).1,
                SettingSource::File,
                "sb = {sb:?}"
            );
        }
    }

    #[test]
    fn sandbox_mode_e_backend_saem_com_o_nome_certo() {
        for (modo, esperado) in [
            (SandboxMode::Off, "off"),
            (SandboxMode::All, "all"),
            (SandboxMode::Allowlist, "allowlist"),
        ] {
            let sb = SandboxConfig {
                mode: modo,
                ..SandboxConfig::default()
            };
            assert_eq!(sandbox_effective(MODO, &sb).0, serde_json::json!(esperado));
        }
        for (backend, esperado) in [
            (SandboxBackendKind::Docker, "docker"),
            (SandboxBackendKind::Podman, "podman"),
            (SandboxBackendKind::Ssh, "ssh"),
        ] {
            let sb = SandboxConfig {
                mode: SandboxMode::All,
                backend: Some(backend),
                ..SandboxConfig::default()
            };
            assert_eq!(
                sandbox_effective(BACKEND, &sb).0,
                serde_json::json!(esperado)
            );
        }
    }

    /// O `ssh_host` nao pode vazar por esta rota, que e auth-free. Nomear
    /// infraestrutura nao acrescenta nada ao diagnostico.
    #[test]
    fn sandbox_effective_nunca_devolve_o_ssh_host() {
        let sb = SandboxConfig {
            mode: SandboxMode::All,
            backend: Some(SandboxBackendKind::Ssh),
            ssh_host: Some("bastiao-interno.exemplo".into()),
            image: Some("registry.interno/imagem:1".into()),
            ..SandboxConfig::default()
        };
        for id in [MODO, BACKEND] {
            let (valor, _) = sandbox_effective(id, &sb);
            let texto = valor.to_string();
            assert!(!texto.contains("bastiao-interno"), "vazou host: {texto}");
            assert!(!texto.contains("registry.interno"), "vazou imagem: {texto}");
        }
    }

    /// As duas linhas existem no schema, sao read-only e ficam em Security.
    /// Read-only importa: o PATCH da rota e dry-run, entao uma chave editavel
    /// faria a UI dizer "aplicado" para um controle que voltaria desligado.
    #[test]
    fn sandbox_aparece_no_schema_como_somente_leitura() {
        let todas = settings();
        for id in [MODO, BACKEND] {
            let row = todas
                .iter()
                .find(|s| s.id == id)
                .unwrap_or_else(|| panic!("{id} deveria estar no schema"));
            assert!(!row.editable, "{id} nao pode ser editavel");
            assert!(!row.secret);
            assert!(matches!(row.category, SettingCategory::Security));
        }
    }
}
