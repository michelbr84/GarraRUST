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
pub fn merge_update(existing: &mut AppConfig, outcome: &WizardOutcome) {
    existing.gateway.host = outcome.host.clone();
    existing.gateway.port = outcome.port;

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
}

// ---------- Top-level write --------------------------------------------------

/// Write `<config_dir>/config.yml` according to `strategy`. Returns the
/// path that was written.
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
) -> Result<PathBuf> {
    let config_path = config_dir.join("config.yml");
    match strategy {
        ExistingConfigStrategy::FirstWrite => {
            let cfg = build_app_config(outcome);
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
            let yaml = serde_yaml::to_string(&cfg).context("serialize AppConfig")?;
            std::fs::write(&config_path, yaml)
                .with_context(|| format!("write {}", config_path.display()))?;
        }
        ExistingConfigStrategy::MergeUpdate => {
            let raw = std::fs::read_to_string(&config_path)
                .with_context(|| format!("read {}", config_path.display()))?;
            let mut existing: AppConfig =
                serde_yaml::from_str(&raw).context("parse existing config.yml")?;
            merge_update(&mut existing, outcome);
            let yaml = serde_yaml::to_string(&existing).context("serialize merged AppConfig")?;
            std::fs::write(&config_path, yaml)
                .with_context(|| format!("write {}", config_path.display()))?;
        }
    }
    // The wizard now writes `llm.*.api_key` into this file by default, so it
    // must not be left at the umask default (commonly 0644). Applies to all
    // three strategies — they converge on the same `config_path`.
    garraia_config::harden_secret_file(&config_path)
        .with_context(|| format!("restrict permissions on {}", config_path.display()))?;
    Ok(config_path)
}

// ---------- Tests --------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn outcome_cloud_only() -> WizardOutcome {
        WizardOutcome {
            host: "0.0.0.0".into(),
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
        }
    }

    /// Same shape as `outcome_cloud_only` but carrying a cleartext key, i.e.
    /// the operator picked `SecretStorage::Config` (the v0.3.0 default).
    fn outcome_cloud_with_key(key: &str) -> WizardOutcome {
        let mut out = outcome_cloud_only();
        out.cloud = Some(CloudLlmChoice {
            key: "openrouter".into(),
            provider: "openrouter".into(),
            model: "openrouter/auto".into(),
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

    #[test]
    fn config_storage_writes_api_key_into_llm_entry() {
        let dir = tempdir().unwrap();
        let out = outcome_cloud_with_key("test-key-abc");
        let path = write_config(dir.path(), &out, ExistingConfigStrategy::FirstWrite).unwrap();
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
        let path = write_config(dir.path(), &out, ExistingConfigStrategy::FirstWrite).unwrap();
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
        let path = write_config(dir.path(), &out, ExistingConfigStrategy::FirstWrite).unwrap();

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
        let path = write_config(dir.path(), &out, ExistingConfigStrategy::FirstWrite).unwrap();
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
