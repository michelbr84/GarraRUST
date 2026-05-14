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
//!     - `llm.*` — only **adds** missing keys; never replaces an
//!       existing user-customized provider.
//!     - `agent.default_provider` — set only when currently `None`.
//!     - `agent.fallback_providers` — set only when currently empty.
//!     - `voice.*` — replaced when the wizard just opted into voice;
//!       otherwise untouched.
//!     - `channels.telegram` — only added when missing.
//!
//! Secret-redaction invariant: API keys never appear in the YAML written
//! by this module unless the user chose plaintext storage (option 1 in
//! the existing vault flow). The vault path is handled by the
//! orchestrator (`mod.rs`); this module only knows about the cleartext
//! key when explicitly handed one and writes it to `llm.<name>.api_key`.

#![allow(dead_code)] // M1.7 orchestrator wires these in.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use garraia_config::{
    AgentConfig, AppConfig, ChannelConfig, GatewayConfig, LlmProviderConfig, VoiceConfig,
};

use super::local_stack::{
    OLLAMA_API_KEY, OLLAMA_OPENAI_BASE_URL, OLLAMA_PROVIDER_KEY, QWEN3_MODEL_TAG,
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

    /// OpenRouter cloud entry — populated for cloud-only or cloud-first
    /// modes. `Some` even when the api_key field is `None` (env-var
    /// users).
    pub openrouter: Option<CloudLlmChoice>,

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
    /// Key used in the `llm:` map. Defaults to `"openrouter"`.
    pub key: String,
    pub model: String,
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
            model: QWEN3_MODEL_TAG.to_string(),
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
    if let Some(cloud) = &outcome.openrouter {
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
        provider: "openrouter".to_string(),
        model: Some(cloud.model.clone()),
        api_key: cloud.api_key_plaintext.clone(),
        base_url: Some("https://openrouter.ai/api/v1".to_string()),
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

/// Patch `existing` in place with the additive `MergeUpdate` rules.
/// See module docs for which fields are wizard-owned vs. user-owned.
pub fn merge_update(existing: &mut AppConfig, outcome: &WizardOutcome) {
    existing.gateway.host = outcome.host.clone();
    existing.gateway.port = outcome.port;

    if let Some(cloud) = &outcome.openrouter {
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
            openrouter: Some(CloudLlmChoice {
                key: "openrouter".into(),
                model: "deepseek/deepseek-chat-v3.5".into(),
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
            openrouter: Some(CloudLlmChoice {
                key: "openrouter".into(),
                model: "deepseek/deepseek-chat-v3.5".into(),
                api_key_plaintext: None,
            }),
            local_llm: Some(LocalLlmChoice::default()),
            voice_enabled: true,
            system_prompt: None,
            telegram: None,
        }
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
        assert!(raw.contains("hf.co/MaziyarPanahi/Qwen3-14B-GGUF:Q4_K_M"));
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
