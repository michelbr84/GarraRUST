//! `config.yml` emission with three strategies — plan 0126 §M1.5.
//!
//! * `FirstWrite` — no existing config; serialize and write.
//! * `Backup { path }` — rename existing `config.yml` to
//!   `config.yml.bak-YYYYMMDD-HHMMSS` (UTC, deterministic), then write
//!   the new file. The rename is atomic on POSIX so the user is never
//!   left without a config.
//! * `MergeUpdate` — load existing config, patch only the fields the
//!   wizard owns:
//!     - `gateway.host`, `gateway.port` — replaced (wizard owns).
//!     - `llm.*` — **adds** missing keys; never replaces an existing
//!       user-customized provider. One exception: when the operator supplies
//!       a cleartext OpenRouter key, it is backfilled into pre-existing
//!       `openrouter` entries that have **no** key, so re-running the wizard
//!       repairs a config left keyless by the old vault-by-default flow. A
//!       key that is already set is never overwritten.
//!     - `agent.default_provider` — set only when currently `None`.
//!     - `agent.fallback_providers` — set only when currently empty.
//!     - `voice.*` — replaced when the wizard just opted into voice;
//!       otherwise untouched.
//!     - `channels.telegram` — only added when missing.
//!     - `gateway.api_key` — set **only** when the existing value is absent
//!       or blank. A key the operator already chose is never overwritten
//!       (#1241).
//!
//! Gateway credential (#1241): when the host the wizard resolved is **not**
//! loopback, `garraia init` used to emit `gateway.api_key: None` — a gateway
//! on the whole internet with no credential at all. [`gateway_api_key_for_host`]
//! now mints 32 CSPRNG bytes for that case, and only that case; a loopback
//! bind keeps emitting no key.
//!
//! Secret invariant: API keys appear in the YAML written by this module only
//! when the operator chose config storage (`SecretStorage::Config`, the
//! default since v0.3.0). The vault path is handled by the orchestrator
//! (`mod.rs`); this module only knows about the cleartext key when explicitly
//! handed one, and writes it to `llm.<name>.api_key`. Because that makes
//! `config.yml` credential-bearing, [`write_config`] clamps the file to mode
//! `0600` via `garraia_config::harden_secret_file` on every strategy.

#![allow(dead_code)] // M1.7 orchestrator wires these in.

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use garraia_config::{
    AgentConfig, AppConfig, ChannelConfig, GatewayConfig, LlmProviderConfig, VoiceConfig,
};

use super::local_stack::{
    DEFAULT_OLLAMA_MODEL_TAG, OLLAMA_API_KEY, OLLAMA_OPENAI_BASE_URL, OLLAMA_PROVIDER_KEY,
};

// ---------- Public types -----------------------------------------------------

/// Everything the wizard collected during the interactive flow. Passed
/// to [`write_config`] which translates it into the on-disk
/// [`AppConfig`].
#[derive(Debug, Clone)]
pub struct WizardOutcome {
    /// "0.0.0.0" on RunPod/root, "127.0.0.1" otherwise.
    pub host: String,
    pub port: u16,

    /// First provider tried by the agent runtime.
    pub default_provider: String,
    /// Ordered fallbacks. Empty when only one provider was configured.
    pub fallback_providers: Vec<String>,

    /// Cloud provider entry (OpenRouter, OpenAI, Anthropic, …) — populated
    /// for cloud-only or cloud-first modes. `Some` even when the api_key
    /// field is `None` (env-var users).
    pub cloud: Option<CloudLlmChoice>,

    /// Local LLM — populated only when the user opted in (GPU detected,
    /// `GARRAIA_BOOTSTRAP_LOCAL != 0`, user confirmed).
    pub local_llm: Option<LocalLlmChoice>,

    /// `true` when the user opted into voice on a GPU machine. Causes
    /// the wizard to emit a `voice:` section with Chatterbox + Whisper
    /// endpoints.
    pub voice_enabled: bool,

    /// User-supplied system prompt. `None` keeps the schema's default.
    pub system_prompt: Option<String>,

    /// Optional Telegram channel — same shape as before the rewrite.
    pub telegram: Option<TelegramChoice>,

    /// Credential for `gateway.api_key`, minted by [`gateway_api_key_for_host`]
    /// when [`host`](Self::host) is **not** loopback and left `None` when it
    /// is (#1241). On `MergeUpdate` it is written only into a config whose
    /// `gateway.api_key` is absent or blank — an operator key is never
    /// overwritten.
    pub gateway_api_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CloudLlmChoice {
    /// Key used in the `llm:` map (e.g. `"openrouter"`, `"openai"`).
    pub key: String,
    /// Provider type string consumed by `build_agent_runtime` — for the
    /// wizard presets this always equals `key`.
    pub provider: String,
    pub model: String,
    /// `None` lets the provider client use its own default endpoint.
    pub base_url: Option<String>,
    pub api_key_plaintext: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LocalLlmChoice {
    /// Key in the `llm:` map. Defaults to [`OLLAMA_PROVIDER_KEY`].
    pub key: String,
    pub base_url: String,
    pub model: String,
}

impl Default for LocalLlmChoice {
    fn default() -> Self {
        Self {
            key: OLLAMA_PROVIDER_KEY.to_string(),
            base_url: OLLAMA_OPENAI_BASE_URL.to_string(),
            model: DEFAULT_OLLAMA_MODEL_TAG.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TelegramChoice {
    pub plaintext_token: Option<String>,
}

/// Strategy passed to [`write_config`] — chosen by the orchestrator
/// after inspecting whether `config.yml` already exists.
#[derive(Debug, Clone)]
pub enum ExistingConfigStrategy {
    FirstWrite,
    /// The wizard will rename the existing file to `backup_path` before
    /// writing the new one. The orchestrator computes the backup path
    /// via [`backup_path_for`].
    Backup {
        backup_path: PathBuf,
    },
    /// Load the existing config and patch wizard-owned fields only.
    MergeUpdate,
}

// ---------- Backup-path helper -----------------------------------------------

/// Returns `<config_dir>/config.yml.bak-YYYYMMDD-HHMMSS` using a UTC
/// timestamp. Deterministic given a fixed clock — tests inject the
/// timestamp via [`backup_path_for_with`].
pub fn backup_path_for(config_dir: &Path) -> PathBuf {
    backup_path_for_with(config_dir, Utc::now())
}

pub fn backup_path_for_with(config_dir: &Path, when: chrono::DateTime<Utc>) -> PathBuf {
    let stamp = when.format("%Y%m%d-%H%M%S").to_string();
    config_dir.join(format!("config.yml.bak-{stamp}"))
}

// ---------- Gateway credential (#1241) ---------------------------------------

/// Length of the generated `gateway.api_key`, in raw CSPRNG bytes. Rendered
/// as lowercase hex, so the key the operator sees is twice this many chars.
pub const GATEWAY_API_KEY_BYTES: usize = 32;

/// `true` when `host` only reaches the machine the gateway runs on.
///
/// Fail-closed on anything it cannot parse: a name that is not `localhost`
/// counts as exposed, so the wizard errs towards minting a credential rather
/// than towards leaving a reachable gateway open.
pub fn host_is_loopback(host: &str) -> bool {
    let host = host.trim();
    // `[::1]` — the way an IPv6 literal is written in a URL authority.
    let host = host
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(host);
    match host.parse::<IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => host.eq_ignore_ascii_case("localhost"),
    }
}

/// The `gateway.api_key` a config emitted for `host` must carry.
///
/// `None` for a loopback bind — nothing changes for the laptop case. For any
/// other host (the `0.0.0.0` that [`super::pick_host_port`] picks on a
/// root/RunPod box) this returns a fresh credential: `garra init` on a cloud
/// VM used to leave `/api/*` open to the whole internet (#1241).
///
/// Each call draws new bytes from the system CSPRNG — never a time-seeded
/// PRNG.
pub fn gateway_api_key_for_host(host: &str) -> Result<Option<String>> {
    if host_is_loopback(host) {
        return Ok(None);
    }
    Ok(Some(generate_gateway_api_key()?))
}

/// [`GATEWAY_API_KEY_BYTES`] bytes from the system CSPRNG, lowercase hex.
///
/// `garraia_security::random_bytes` is the shared helper — it hands back an
/// array that `ring` already filled, so no zeroed buffer exists in between.
fn generate_gateway_api_key() -> Result<String> {
    let bytes: [u8; GATEWAY_API_KEY_BYTES] = garraia_security::random_bytes()
        .context("system CSPRNG refused to produce a gateway.api_key")?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// `true` when `api_key` is a credential rather than an absent/blank field.
///
/// Mirrors `garraia_gateway::gateway_auth::ApiKeyGate::from_config`: there,
/// absent, empty and whitespace-only all mean "gate off". A config in that
/// state has no operator key to protect, so the wizard may fill it.
fn gateway_api_key_is_set(api_key: Option<&str>) -> bool {
    !api_key.unwrap_or_default().trim().is_empty()
}

// ---------- Build / merge ----------------------------------------------------

/// Translate a [`WizardOutcome`] into a fresh [`AppConfig`] — used by
/// `FirstWrite` and `Backup` paths.
pub fn build_app_config(outcome: &WizardOutcome) -> AppConfig {
    let mut llm: HashMap<String, LlmProviderConfig> = HashMap::new();
    if let Some(cloud) = &outcome.cloud {
        llm.insert(cloud.key.clone(), cloud_llm_provider(cloud));
    }
    if let Some(local) = &outcome.local_llm {
        llm.insert(local.key.clone(), local_llm_provider(local));
    }

    let mut channels: HashMap<String, ChannelConfig> = HashMap::new();
    if let Some(tg) = &outcome.telegram {
        channels.insert("telegram".to_string(), telegram_channel(tg));
    }

    let mut voice = VoiceConfig::default();
    if outcome.voice_enabled {
        voice.enabled = true;
        // Defaults already align with plan 0126 — provider/endpoint/lang.
    }

    AppConfig {
        gateway: GatewayConfig {
            host: outcome.host.clone(),
            port: outcome.port,
            // #1241: `Some` exactly when the bind is not loopback.
            api_key: outcome.gateway_api_key.clone(),
            ..GatewayConfig::default()
        },
        llm,
        channels,
        agent: AgentConfig {
            system_prompt: outcome.system_prompt.clone(),
            default_provider: Some(outcome.default_provider.clone()),
            fallback_providers: outcome.fallback_providers.clone(),
            ..Default::default()
        },
        voice,
        ..Default::default()
    }
}

fn cloud_llm_provider(cloud: &CloudLlmChoice) -> LlmProviderConfig {
    LlmProviderConfig {
        provider: cloud.provider.clone(),
        model: Some(cloud.model.clone()),
        api_key: cloud.api_key_plaintext.clone(),
        base_url: cloud.base_url.clone(),
        extra: Default::default(),
    }
}

fn local_llm_provider(local: &LocalLlmChoice) -> LlmProviderConfig {
    LlmProviderConfig {
        // Ollama exposes an OpenAI-compatible endpoint — provider key
        // points the agent runtime at the OpenAI client.
        provider: "openai".to_string(),
        model: Some(local.model.clone()),
        api_key: Some(OLLAMA_API_KEY.to_string()),
        base_url: Some(local.base_url.clone()),
        extra: Default::default(),
    }
}

fn telegram_channel(tg: &TelegramChoice) -> ChannelConfig {
    let mut settings = HashMap::new();
    if let Some(token) = &tg.plaintext_token {
        settings.insert(
            "bot_token".to_string(),
            serde_json::Value::String(token.clone()),
        );
    }
    ChannelConfig {
        channel_type: "telegram".to_string(),
        enabled: Some(true),
        settings,
    }
}

/// Backfill a freshly-collected cleartext key into pre-existing `llm:` entries
/// of the same provider type that have no key of their own.
///
/// Without this, `merge_update` was purely additive, and re-running
/// `garraia init` could not repair a broken config. An operator whose config
/// already had a keyless `llm.main` — exactly what the pre-0.3.0 wizard wrote
/// when it defaulted to the credential vault — would get a *second* entry
/// `llm.openrouter` while `llm.main` stayed keyless, and `build_agent_runtime`
/// kept skipping `main` with "no API key" on every boot.
///
/// Only entries whose `api_key` is absent or empty are filled; a key the
/// operator already set is never touched. Deliberately **not** applied to the
/// local-Ollama provider, which registers under `provider: "openai"` with a
/// placeholder key — backfilling that would overwrite a real, intentionally
/// env-var-backed OpenAI entry with the Ollama placeholder.
fn backfill_missing_api_key(existing: &mut AppConfig, provider_type: &str, api_key: &str) {
    for entry in existing.llm.values_mut() {
        if entry.provider == provider_type
            && entry.api_key.as_deref().unwrap_or_default().is_empty()
        {
            entry.api_key = Some(api_key.to_string());
        }
    }
}

/// Patch `existing` in place with the additive `MergeUpdate` rules.
/// See module docs for which fields are wizard-owned vs. user-owned.
///
/// Returns `true` when the wizard's generated `gateway.api_key` was installed
/// — i.e. only when the existing config had none. The caller needs that
/// answer to decide whether to print the key: printing a key that was **not**
/// written would be worse than printing nothing (#1241).
pub fn merge_update(existing: &mut AppConfig, outcome: &WizardOutcome) -> bool {
    existing.gateway.host = outcome.host.clone();
    existing.gateway.port = outcome.port;

    // The one rule that must never regress: an operator who already set
    // `gateway.api_key` and re-runs `garra init` keeps their key. Only an
    // absent/blank field — which the gateway treats as "no gate at all" —
    // gets filled.
    let mut gateway_key_written = false;
    if let Some(generated) = &outcome.gateway_api_key
        && !gateway_api_key_is_set(existing.gateway.api_key.as_deref())
    {
        existing.gateway.api_key = Some(generated.clone());
        gateway_key_written = true;
    }

    if let Some(cloud) = &outcome.cloud {
        if let Some(key) = cloud.api_key_plaintext.as_deref() {
            backfill_missing_api_key(existing, &cloud.provider, key);
        }
        existing
            .llm
            .entry(cloud.key.clone())
            .or_insert_with(|| cloud_llm_provider(cloud));
    }
    if let Some(local) = &outcome.local_llm {
        existing
            .llm
            .entry(local.key.clone())
            .or_insert_with(|| local_llm_provider(local));
    }

    if existing.agent.default_provider.is_none() {
        existing.agent.default_provider = Some(outcome.default_provider.clone());
    }
    if existing.agent.fallback_providers.is_empty() {
        existing.agent.fallback_providers = outcome.fallback_providers.clone();
    }
    if outcome.system_prompt.is_some() && existing.agent.system_prompt.is_none() {
        existing.agent.system_prompt = outcome.system_prompt.clone();
    }

    if outcome.voice_enabled {
        existing.voice.enabled = true;
    }

    if let Some(tg) = &outcome.telegram
        && !existing.channels.contains_key("telegram")
    {
        existing
            .channels
            .insert("telegram".to_string(), telegram_channel(tg));
    }

    gateway_key_written
}

// ---------- Top-level write --------------------------------------------------

/// What [`write_config`] left on disk.
#[derive(Debug, Clone)]
pub struct WrittenConfig {
    /// The `config.yml` that was written.
    pub path: PathBuf,
    /// `Some` only when **this run** wrote the generated `gateway.api_key`
    /// (#1241) — so the summary prints a key that is really in the file, and
    /// stays quiet when an operator key was preserved instead.
    pub gateway_api_key_written: Option<String>,
}

/// Write `<config_dir>/config.yml` according to `strategy`. Returns the
/// path that was written plus the gateway key, if any, that this run put
/// there.
///
/// * `FirstWrite` and `Backup` build a fresh `AppConfig` from `outcome`
///   and serialize it.
/// * `Backup { backup_path }` first renames the existing
///   `config.yml` to `backup_path`. The rename is atomic on POSIX —
///   the user is never left without a config.
/// * `MergeUpdate` loads the existing `config.yml` via `serde_yaml`,
///   patches it via [`merge_update`], and rewrites the file in place.
///
/// On `MergeUpdate` failure to parse the existing YAML, the function
/// returns an error — the orchestrator must surface this to the
/// operator (who can then choose the `Backup` strategy instead).
pub fn write_config(
    config_dir: &Path,
    outcome: &WizardOutcome,
    strategy: ExistingConfigStrategy,
) -> Result<WrittenConfig> {
    let config_path = config_dir.join("config.yml");
    // Set by the branch that actually wrote it; `MergeUpdate` may decline.
    let mut gateway_api_key_written = None;
    match strategy {
        ExistingConfigStrategy::FirstWrite => {
            let cfg = build_app_config(outcome);
            gateway_api_key_written = cfg.gateway.api_key.clone();
            let yaml = serde_yaml::to_string(&cfg).context("serialize AppConfig")?;
            std::fs::write(&config_path, yaml)
                .with_context(|| format!("write {}", config_path.display()))?;
        }
        ExistingConfigStrategy::Backup { backup_path } => {
            if config_path.exists() {
                std::fs::rename(&config_path, &backup_path).with_context(|| {
                    format!(
                        "rename {} → {}",
                        config_path.display(),
                        backup_path.display()
                    )
                })?;
            }
            let cfg = build_app_config(outcome);
            gateway_api_key_written = cfg.gateway.api_key.clone();
            let yaml = serde_yaml::to_string(&cfg).context("serialize AppConfig")?;
            std::fs::write(&config_path, yaml)
                .with_context(|| format!("write {}", config_path.display()))?;
        }
        ExistingConfigStrategy::MergeUpdate => {
            let raw = std::fs::read_to_string(&config_path)
                .with_context(|| format!("read {}", config_path.display()))?;
            let mut existing: AppConfig =
                serde_yaml::from_str(&raw).context("parse existing config.yml")?;
            if merge_update(&mut existing, outcome) {
                gateway_api_key_written = outcome.gateway_api_key.clone();
            }
            let yaml = serde_yaml::to_string(&existing).context("serialize merged AppConfig")?;
            std::fs::write(&config_path, yaml)
                .with_context(|| format!("write {}", config_path.display()))?;
        }
    }
    // The wizard now writes `llm.*.api_key` into this file by default — and,
    // on an exposed bind, the gateway credential (#1241) — so it must not be
    // left at the umask default (commonly 0644). Applies to all three
    // strategies: they converge on the same `config_path`. On Windows
    // `harden_secret_file` is a no-op; that predates this change.
    garraia_config::harden_secret_file(&config_path)
        .with_context(|| format!("restrict permissions on {}", config_path.display()))?;
    Ok(WrittenConfig {
        path: config_path,
        gateway_api_key_written,
    })
}

// ---------- Tests --------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn outcome_cloud_only() -> WizardOutcome {
        outcome_cloud_only_on_host("0.0.0.0")
    }

    /// Same fixture parametrized by host, so the gateway-credential tests run
    /// through the very policy `run_wizard` uses (#1241) instead of restating
    /// it.
    fn outcome_cloud_only_on_host(host: &str) -> WizardOutcome {
        WizardOutcome {
            host: host.into(),
            port: 3888,
            default_provider: "openrouter".into(),
            fallback_providers: vec![],
            cloud: Some(CloudLlmChoice {
                key: "openrouter".into(),
                provider: "openrouter".into(),
                model: "deepseek/deepseek-chat-v3.5".into(),
                base_url: Some("https://openrouter.ai/api/v1".into()),
                api_key_plaintext: None,
            }),
            local_llm: None,
            voice_enabled: false,
            system_prompt: Some("You are a helpful personal AI assistant.".into()),
            telegram: None,
            gateway_api_key: gateway_api_key_for_host(host).expect("csprng"),
        }
    }

    fn outcome_local_first() -> WizardOutcome {
        WizardOutcome {
            host: "0.0.0.0".into(),
            port: 3888,
            default_provider: OLLAMA_PROVIDER_KEY.into(),
            fallback_providers: vec!["openrouter".into()],
            cloud: Some(CloudLlmChoice {
                key: "openrouter".into(),
                provider: "openrouter".into(),
                model: "deepseek/deepseek-chat-v3.5".into(),
                base_url: Some("https://openrouter.ai/api/v1".into()),
                api_key_plaintext: None,
            }),
            local_llm: Some(LocalLlmChoice::default()),
            voice_enabled: true,
            system_prompt: None,
            telegram: None,
            gateway_api_key: gateway_api_key_for_host("0.0.0.0").expect("csprng"),
        }
    }

    /// Same shape as `outcome_cloud_only` but carrying a cleartext key, i.e.
    /// the operator picked `SecretStorage::Config` (the v0.3.0 default).
    fn outcome_cloud_with_key(key: &str) -> WizardOutcome {
        let mut out = outcome_cloud_only();
        out.cloud = Some(CloudLlmChoice {
            key: "openrouter".into(),
            provider: "openrouter".into(),
            model: crate::defaults::DEFAULT_CLOUD_MODEL.into(),
            base_url: Some("https://openrouter.ai/api/v1".into()),
            api_key_plaintext: Some(key.into()),
        });
        out
    }

    /// The config the pre-0.3.0 wizard left behind when it defaulted to the
    /// vault: a structurally perfect provider entry named `main` with no key.
    fn legacy_keyless_main() -> AppConfig {
        let mut cfg = AppConfig::default();
        cfg.llm.insert(
            "main".into(),
            LlmProviderConfig {
                provider: "openrouter".into(),
                model: Some("openrouter/auto".into()),
                api_key: None,
                base_url: Some("https://openrouter.ai/api/v1".into()),
                extra: Default::default(),
            },
        );
        cfg
    }

    // ---- #1241: credencial do gateway em bind exposto --------------------

    /// O caso da issue: `garra init` como root escolhe `0.0.0.0`, e ate
    /// #1241 saia dali com `gateway.api_key: None` — gateway na internet
    /// inteira sem credencial nenhuma.
    #[test]
    fn bind_exposto_gera_chave_de_gateway() {
        let out = outcome_cloud_only(); // host = "0.0.0.0"
        let cfg = build_app_config(&out);
        assert!(
            cfg.gateway.api_key.is_some(),
            "bind nao-loopback tem que sair com gateway.api_key gravada"
        );
    }

    /// O espelho, e o limite do blast radius: no laptop nada muda.
    #[test]
    fn bind_loopback_nao_gera_chave_de_gateway() {
        let out = outcome_cloud_only_on_host("127.0.0.1");
        let cfg = build_app_config(&out);
        assert!(
            cfg.gateway.api_key.is_none(),
            "bind loopback nao pode ganhar chave — o wizard nao muda nada nesse caminho"
        );
    }

    /// A tabela de `host_is_loopback`, incluindo o fail-closed: um nome que
    /// nao seja `localhost` conta como exposto.
    #[test]
    fn classificacao_de_host_loopback() {
        for host in [
            "127.0.0.1",
            "127.0.0.53",
            "::1",
            "[::1]",
            "localhost",
            "LocalHost",
        ] {
            assert!(host_is_loopback(host), "{host} devia contar como loopback");
        }
        for host in [
            "0.0.0.0",
            "::",
            "192.168.1.10",
            "10.0.0.2",
            "garra.example.com",
        ] {
            assert!(!host_is_loopback(host), "{host} devia contar como exposto");
        }
    }

    /// A chave e CSPRNG de verdade: 32 bytes em hex, e duas execucoes nunca
    /// dao a mesma coisa. Um PRNG semeado por relogio passaria no primeiro
    /// assert e falharia neste.
    #[test]
    fn chave_gerada_tem_entropia_e_nao_se_repete() {
        let a = gateway_api_key_for_host("0.0.0.0").unwrap().unwrap();
        let b = gateway_api_key_for_host("0.0.0.0").unwrap().unwrap();
        assert_eq!(
            a.len(),
            GATEWAY_API_KEY_BYTES * 2,
            "32 bytes em hex sao 64 caracteres"
        );
        assert!(
            a.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "a chave tem que ser hex minusculo: {a}"
        );
        assert_ne!(a, b, "duas execucoes nao podem devolver a mesma chave");
    }

    /// `AppConfig` de teste com a credencial de gateway ja no lugar (ou
    /// ausente/em branco), sem `field_reassign_with_default`.
    fn config_com_gateway_key(valor: Option<&str>) -> AppConfig {
        AppConfig {
            gateway: GatewayConfig {
                api_key: valor.map(str::to_string),
                ..GatewayConfig::default()
            },
            ..AppConfig::default()
        }
    }

    /// **A regressao mais cara**: quem ja tem chave e roda `garra init` de
    /// novo nao pode perde-la. Um merge que sobrescreve derruba todo cliente
    /// ja configurado — celular, script, reverse proxy.
    #[test]
    fn merge_nao_sobrescreve_chave_de_operador() {
        let mut existing = config_com_gateway_key(Some("chave-do-operador"));

        let out = outcome_cloud_only(); // host exposto => outcome traz chave nova
        assert!(
            out.gateway_api_key.is_some(),
            "fixture precisa trazer chave"
        );
        let gravou = merge_update(&mut existing, &out);

        assert_eq!(
            existing.gateway.api_key.as_deref(),
            Some("chave-do-operador"),
            "a chave do operador tem que sobreviver ao re-run do wizard"
        );
        assert!(
            !gravou,
            "o merge nao gravou chave nenhuma, e precisa dizer isso"
        );
    }

    /// O outro lado do merge: config sem chave (ou com a chave em branco, que
    /// o `ApiKeyGate` trata como gate desligado) recebe a chave gerada.
    #[test]
    fn merge_preenche_chave_ausente_ou_em_branco() {
        for existente in [None, Some(""), Some("   ")] {
            let mut existing = config_com_gateway_key(existente);

            let out = outcome_cloud_only();
            let gravou = merge_update(&mut existing, &out);

            assert!(
                gravou,
                "com {existente:?} em disco o merge tinha que gravar"
            );
            assert_eq!(
                existing.gateway.api_key, out.gateway_api_key,
                "a chave gravada tem que ser a do outcome"
            );
        }
    }

    /// `write_config` so devolve a chave que ele **de fato** colocou no
    /// arquivo — imprimir uma chave que nao esta em disco seria pior do que
    /// nao imprimir nada.
    #[test]
    fn write_config_reporta_so_a_chave_que_gravou() {
        // FirstWrite num host exposto: grava e reporta.
        let dir = tempdir().unwrap();
        let out = outcome_cloud_only();
        let escrito = write_config(dir.path(), &out, ExistingConfigStrategy::FirstWrite).unwrap();
        assert_eq!(escrito.gateway_api_key_written, out.gateway_api_key);
        let cfg: AppConfig =
            serde_yaml::from_str(&std::fs::read_to_string(&escrito.path).unwrap()).unwrap();
        assert_eq!(cfg.gateway.api_key, out.gateway_api_key);

        // MergeUpdate sobre config com chave de operador: nao grava, nao reporta.
        let dir = tempdir().unwrap();
        let existing = config_com_gateway_key(Some("chave-do-operador"));
        std::fs::write(
            dir.path().join("config.yml"),
            serde_yaml::to_string(&existing).unwrap(),
        )
        .unwrap();
        let escrito = write_config(dir.path(), &out, ExistingConfigStrategy::MergeUpdate).unwrap();
        assert!(
            escrito.gateway_api_key_written.is_none(),
            "nada foi gravado, entao nada pode ser impresso"
        );
        let cfg: AppConfig =
            serde_yaml::from_str(&std::fs::read_to_string(&escrito.path).unwrap()).unwrap();
        assert_eq!(cfg.gateway.api_key.as_deref(), Some("chave-do-operador"));

        // FirstWrite em loopback: nada a gravar, nada a reportar.
        let dir = tempdir().unwrap();
        let out = outcome_cloud_only_on_host("127.0.0.1");
        let escrito = write_config(dir.path(), &out, ExistingConfigStrategy::FirstWrite).unwrap();
        assert!(escrito.gateway_api_key_written.is_none());
    }

    #[test]
    fn config_storage_writes_api_key_into_llm_entry() {
        let dir = tempdir().unwrap();
        let out = outcome_cloud_with_key("test-key-abc");
        let path = write_config(dir.path(), &out, ExistingConfigStrategy::FirstWrite)
            .unwrap()
            .path;
        let cfg: AppConfig = serde_yaml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(
            cfg.llm.get("openrouter").unwrap().api_key.as_deref(),
            Some("test-key-abc"),
            "a key collected under SecretStorage::Config must reach llm.*.api_key"
        );
    }

    /// The wizard's cloud step is no longer OpenRouter-only: a non-OpenRouter
    /// preset must flow through untouched — its provider type in `provider:`,
    /// and `base_url` left absent so the client uses its own default endpoint.
    #[test]
    fn non_openrouter_preset_writes_generic_provider_entry() {
        let mut out = outcome_cloud_only();
        out.default_provider = "openai".into();
        out.cloud = Some(CloudLlmChoice {
            key: "openai".into(),
            provider: "openai".into(),
            model: "gpt-4o".into(),
            base_url: None,
            api_key_plaintext: Some("sk-test".into()),
        });

        let dir = tempdir().unwrap();
        let path = write_config(dir.path(), &out, ExistingConfigStrategy::FirstWrite)
            .unwrap()
            .path;
        let cfg: AppConfig = serde_yaml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let entry = cfg.llm.get("openai").unwrap();
        assert_eq!(entry.provider, "openai");
        assert_eq!(entry.model.as_deref(), Some("gpt-4o"));
        assert_eq!(
            entry.base_url, None,
            "no hardcoded OpenRouter base_url may leak in"
        );
        assert_eq!(entry.api_key.as_deref(), Some("sk-test"));
    }

    /// The regression that made `garraia init` unable to repair itself: a second
    /// run added `llm.openrouter` and left `llm.main` keyless, so the gateway
    /// kept logging `skipping openrouter provider main: no API key`.
    #[test]
    fn merge_update_backfills_key_into_existing_keyless_entry() {
        let mut existing = legacy_keyless_main();
        merge_update(&mut existing, &outcome_cloud_with_key("recovered-key"));

        assert_eq!(
            existing.llm.get("main").unwrap().api_key.as_deref(),
            Some("recovered-key"),
            "the pre-existing keyless `main` entry must be repaired, not orphaned"
        );
    }

    #[test]
    fn merge_update_never_overwrites_a_key_the_operator_already_set() {
        let mut existing = legacy_keyless_main();
        existing.llm.get_mut("main").unwrap().api_key = Some("operator-owned".into());

        merge_update(&mut existing, &outcome_cloud_with_key("wizard-key"));

        assert_eq!(
            existing.llm.get("main").unwrap().api_key.as_deref(),
            Some("operator-owned"),
            "an already-configured key is user-owned and must survive the wizard"
        );
    }

    /// When the operator chose vault or env storage there is no cleartext to
    /// backfill, so the old additive behaviour must be preserved exactly.
    #[test]
    fn merge_update_without_cleartext_leaves_existing_entry_keyless() {
        let mut existing = legacy_keyless_main();
        merge_update(&mut existing, &outcome_cloud_only());

        assert!(
            existing.llm.get("main").unwrap().api_key.is_none(),
            "no cleartext was collected, so nothing may be invented"
        );
    }

    /// The local-Ollama provider registers as `provider: "openai"` with a
    /// placeholder key. Backfill must not leak that placeholder into a real,
    /// intentionally env-var-backed OpenAI entry.
    #[test]
    fn merge_update_does_not_touch_unrelated_provider_types() {
        let mut existing = legacy_keyless_main();
        existing.llm.insert(
            "my-openai".into(),
            LlmProviderConfig {
                provider: "openai".into(),
                model: Some("gpt-4o".into()),
                api_key: None,
                base_url: None,
                extra: Default::default(),
            },
        );

        merge_update(&mut existing, &outcome_cloud_with_key("openrouter-only"));

        assert!(
            existing.llm.get("my-openai").unwrap().api_key.is_none(),
            "an openai entry must not receive the openrouter key"
        );
    }

    #[cfg(unix)]
    #[test]
    fn write_config_clamps_permissions_to_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let out = outcome_cloud_with_key("secret-in-file");
        let path = write_config(dir.path(), &out, ExistingConfigStrategy::FirstWrite)
            .unwrap()
            .path;

        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "config.yml now carries the API key; got {:o}",
            mode & 0o777
        );
    }

    #[test]
    fn first_write_emits_complete_config() {
        let dir = tempdir().unwrap();
        let out = outcome_local_first();
        let path = write_config(dir.path(), &out, ExistingConfigStrategy::FirstWrite)
            .unwrap()
            .path;
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("host: 0.0.0.0"));
        assert!(raw.contains("port: 3888"));
        assert!(raw.contains("openrouter:"));
        assert!(raw.contains("ollama-qwen3:"));
        assert!(raw.contains("default_provider: ollama-qwen3"));
        assert!(raw.contains("fallback_providers"));
        assert!(raw.contains("enabled: true")); // voice
        // The wizard's local entry carries whatever tag the picker produced;
        // `LocalLlmChoice::default()` is the `qwen3.8:latest` row.
        assert!(raw.contains(DEFAULT_OLLAMA_MODEL_TAG));
    }

    #[test]
    fn backup_renames_existing_then_writes_new() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yml");
        std::fs::write(&path, "# legacy content marker\nllm: {}\n").unwrap();

        let when = chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 5, 14, 12, 34, 56).unwrap();
        let backup_path = backup_path_for_with(dir.path(), when);

        let out = outcome_cloud_only();
        write_config(
            dir.path(),
            &out,
            ExistingConfigStrategy::Backup {
                backup_path: backup_path.clone(),
            },
        )
        .unwrap();

        let backup_raw = std::fs::read_to_string(&backup_path).unwrap();
        assert!(backup_raw.contains("legacy content marker"));

        let new_raw = std::fs::read_to_string(&path).unwrap();
        assert!(new_raw.contains("openrouter:"));
        assert!(!new_raw.contains("legacy content marker"));
        assert!(
            backup_path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("config.yml.bak-20260514-")
        );
    }

    #[test]
    fn merge_update_preserves_existing_keys_and_only_adds_missing_ones() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yml");
        // Pre-existing config with a custom LLM provider key, a custom
        // agent.default_provider, and no openrouter entry.
        let original = r#"
gateway:
  host: 127.0.0.1
  port: 9999
llm:
  custom-anthropic:
    provider: anthropic
    model: claude-3-opus
agent:
  default_provider: custom-anthropic
  fallback_providers: ["custom-anthropic"]
  system_prompt: "Pre-existing prompt."
"#;
        std::fs::write(&path, original).unwrap();

        // Wizard is now run with local-first outcome — expect:
        //  - gateway host/port REPLACED (wizard owns these)
        //  - llm.custom-anthropic PRESERVED
        //  - llm.openrouter ADDED
        //  - llm.ollama-qwen3 ADDED
        //  - agent.default_provider PRESERVED (already set)
        //  - agent.fallback_providers PRESERVED (already non-empty)
        //  - agent.system_prompt PRESERVED
        let out = outcome_local_first();
        write_config(dir.path(), &out, ExistingConfigStrategy::MergeUpdate).unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        let merged: AppConfig = serde_yaml::from_str(&raw).unwrap();

        assert_eq!(merged.gateway.host, "0.0.0.0");
        assert_eq!(merged.gateway.port, 3888);
        assert!(merged.llm.contains_key("custom-anthropic"));
        assert!(merged.llm.contains_key("openrouter"));
        assert!(merged.llm.contains_key(OLLAMA_PROVIDER_KEY));
        assert_eq!(
            merged.agent.default_provider.as_deref(),
            Some("custom-anthropic")
        );
        assert_eq!(
            merged.agent.fallback_providers,
            vec!["custom-anthropic".to_string()]
        );
        assert_eq!(
            merged.agent.system_prompt.as_deref(),
            Some("Pre-existing prompt.")
        );
        // Voice enabled was toggled this run — must take effect.
        assert!(merged.voice.enabled);
    }

    #[test]
    fn merge_update_fills_empty_agent_fields_when_first_run_was_minimal() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.yml");
        // Pre-existing config that never set agent.default_provider /
        // fallback_providers — e.g. a hand-edited starter file.
        let original = r#"
gateway:
  host: 127.0.0.1
  port: 3888
llm: {}
"#;
        std::fs::write(&path, original).unwrap();
        let out = outcome_cloud_only();
        write_config(dir.path(), &out, ExistingConfigStrategy::MergeUpdate).unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        let merged: AppConfig = serde_yaml::from_str(&raw).unwrap();
        assert_eq!(merged.agent.default_provider.as_deref(), Some("openrouter"));
        assert!(merged.llm.contains_key("openrouter"));
    }
}
