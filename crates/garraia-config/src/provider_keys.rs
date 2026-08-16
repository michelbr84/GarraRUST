//! The single source of truth for "does this LLM provider have a usable API key?"
//!
//! Before this module the question had **three** different answers depending on
//! which surface you asked:
//!
//! * `garraia-gateway::bootstrap` walked vault → config → env (the only one that
//!   decided whether a provider actually came up),
//! * `/health` checked config `||` env and ignored the vault entirely, so it
//!   reported `"no API key configured"` for providers the gateway had
//!   successfully loaded from the vault,
//! * the admin providers list reported `has_secret` from an AES-GCM SQLite store
//!   that the boot path never reads at all.
//!
//! They could disagree on the same machine at the same moment, which is how a
//! provider could look configured everywhere and still be skipped at startup.
//! Everything now routes through [`resolve_api_key`] / [`resolve_api_key_source`].
//!
//! This lives in `garraia-config` rather than in the gateway because
//! `garraia-config::check` (backing `garraia config check`) needs the same
//! answer, and the gateway is a *consumer* of this crate, not a provider to it.

use std::path::{Path, PathBuf};

use crate::loader::ConfigLoader;

/// Environment variable carrying the credential-vault passphrase.
///
/// Note the casing: this is **not** the same variable as the mixed-case
/// `GarraIA_VAULT_PASSPHRASE` consulted by [`crate::auth`] as a JWT-secret
/// fallback. The two are genuinely distinct and easy to confuse.
pub const VAULT_PASSPHRASE_ENV: &str = "GARRAIA_VAULT_PASSPHRASE";

/// Where a resolved API key came from, or why none was found.
///
/// Callers that only need the key itself should use [`resolve_api_key`]; the
/// source matters for diagnostics (`garraia config check`, `/api/diagnostics`,
/// the startup banner) where "the key is in the vault but the vault is locked"
/// must read differently from "there is no key anywhere".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    /// Decrypted out of `credentials/vault.json`.
    Vault,
    /// Read from `llm.<name>.api_key` in `config.yml`.
    Config,
    /// Read from the provider's environment variable.
    Env,
    /// Nothing resolved — the provider will be skipped at startup.
    Missing,
}

impl KeySource {
    /// Whether a provider with this source will actually come up.
    pub fn is_resolved(self) -> bool {
        !matches!(self, KeySource::Missing)
    }

    /// Short human-readable label for reports and log lines.
    pub fn label(self) -> &'static str {
        match self {
            KeySource::Vault => "credential vault",
            KeySource::Config => "config.yml",
            KeySource::Env => "environment variable",
            KeySource::Missing => "not configured",
        }
    }
}

/// The environment variable (and credential-vault entry name — they are the
/// same string by convention) that holds the API key for `provider`.
///
/// Returns `None` for providers that need no key: `ollama` talks to a local
/// daemon, and `echo` is the feature-gated dev provider. An unknown provider
/// string also yields `None`, matching the gateway's
/// `warn!("unknown LLM provider type")` arm.
///
/// Keep in lockstep with the `match llm_config.provider.as_str()` arms in
/// `garraia-gateway/src/bootstrap/mod.rs`. The gateway test
/// `bootstrap::config::tests::provider_key_table_covers_every_arm_of_the_boot_loop`
/// asserts the two agree, including that `ollama` and `echo` stay keyless.
pub fn provider_key_env(provider: &str) -> Option<&'static str> {
    Some(match provider {
        "anthropic" => "ANTHROPIC_API_KEY",
        "openai" => "OPENAI_API_KEY",
        "sansa" => "SANSA_API_KEY",
        "deepseek" => "DEEPSEEK_API_KEY",
        "mistral" => "MISTRAL_API_KEY",
        "gemini" => "GEMINI_API_KEY",
        "falcon" => "FALCON_API_KEY",
        "jais" => "JAIS_API_KEY",
        "qwen" => "QWEN_API_KEY",
        "yi" => "YI_API_KEY",
        "cohere" => "COHERE_API_KEY",
        "minimax" => "MINIMAX_API_KEY",
        "moonshot" => "MOONSHOT_API_KEY",
        "openrouter" => "OPENROUTER_API_KEY",
        // `ollama` needs no key; `echo` is the dev provider; anything else is
        // unknown to the runtime and gets skipped regardless.
        _ => return None,
    })
}

/// Default credential-vault path under the resolved config directory.
pub fn default_vault_path() -> PathBuf {
    ConfigLoader::default_config_dir()
        .join("credentials")
        .join("vault.json")
}

/// Whether a vault file exists on disk but no passphrase is available to open
/// it — the state in which secrets are present yet unreadable, and providers go
/// silently offline.
pub fn vault_present_but_locked(vault_path: &Path) -> bool {
    vault_path.exists() && !vault_passphrase_available()
}

fn vault_passphrase_available() -> bool {
    std::env::var(VAULT_PASSPHRASE_ENV)
        .map(|v| !v.is_empty())
        .unwrap_or(false)
}

/// Resolve an API key, reporting where it came from.
///
/// Precedence is **vault → config → env**, as in the original implementation in
/// `garraia-gateway::bootstrap::config`, with one deliberate change: an
/// **empty** environment variable is now treated as absent rather than as a
/// valid empty key. Previously `OPENROUTER_API_KEY=""` registered a provider
/// with an empty credential that then failed on the first HTTP call with an
/// opaque upstream 401; it now reports [`KeySource::Missing`] and is skipped
/// with the actionable "no API key" warning. This also makes the env tier
/// consistent with the config tier, which already ignored empty strings.
///
/// `vault_key` and `env_var` are the same string for every provider today; both
/// are taken separately so a future rename of one cannot silently retarget the
/// other.
pub fn resolve_api_key_source(
    config_key: Option<&str>,
    vault_key: &str,
    env_var: &str,
) -> (KeySource, Option<String>) {
    // 1. Credential vault — only readable when the passphrase is in the env.
    if let Some(val) = garraia_security::try_vault_get(&default_vault_path(), vault_key) {
        return (KeySource::Vault, Some(val));
    }

    // 2. Config file value.
    if let Some(key) = config_key.filter(|k| !k.is_empty()) {
        return (KeySource::Config, Some(key.to_string()));
    }

    // 3. Environment variable.
    if let Some(val) = std::env::var(env_var).ok().filter(|v| !v.is_empty()) {
        return (KeySource::Env, Some(val));
    }

    (KeySource::Missing, None)
}

/// Resolve an API key using the precedence chain vault → config → env.
///
/// Thin wrapper over [`resolve_api_key_source`] for call sites that do not care
/// where the key came from.
pub fn resolve_api_key(config_key: Option<&str>, vault_key: &str, env_var: &str) -> Option<String> {
    resolve_api_key_source(config_key, vault_key, env_var).1
}

/// Resolve the key for a configured `llm:` entry, keyed off its `provider`
/// field. Providers that need no key (`ollama`, `echo`) report
/// [`KeySource::Config`] with no value, since absence of a key is not a fault
/// for them.
pub fn resolve_provider_key_source(provider: &str, config_key: Option<&str>) -> KeySource {
    match provider_key_env(provider) {
        Some(var) => resolve_api_key_source(config_key, var, var).0,
        // Keyless provider — nothing to resolve, nothing missing.
        None => KeySource::Config,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Env-var mutation is process-global, so the precedence tests share one
    /// serialized test to avoid cross-talk with the rest of the suite.
    #[test]
    fn precedence_is_config_then_env_when_vault_is_absent() {
        let var = "GARRAIA_TEST_PROVIDER_KEYS_PRECEDENCE";
        // SAFETY: unique variable name, mutated and removed within this test.
        unsafe { std::env::remove_var(var) };

        // Nothing anywhere.
        let (src, val) = resolve_api_key_source(None, "NONEXISTENT_VAULT_KEY", var);
        assert_eq!(src, KeySource::Missing);
        assert!(val.is_none());

        // Env only.
        unsafe { std::env::set_var(var, "from-env") };
        let (src, val) = resolve_api_key_source(None, "NONEXISTENT_VAULT_KEY", var);
        assert_eq!(src, KeySource::Env);
        assert_eq!(val.as_deref(), Some("from-env"));

        // Config beats env.
        let (src, val) = resolve_api_key_source(Some("from-config"), "NONEXISTENT_VAULT_KEY", var);
        assert_eq!(src, KeySource::Config);
        assert_eq!(val.as_deref(), Some("from-config"));

        // An empty config value is not a value — must fall through to env.
        let (src, val) = resolve_api_key_source(Some(""), "NONEXISTENT_VAULT_KEY", var);
        assert_eq!(src, KeySource::Env);
        assert_eq!(val.as_deref(), Some("from-env"));

        // An empty env var is likewise not a value.
        unsafe { std::env::set_var(var, "") };
        let (src, _) = resolve_api_key_source(None, "NONEXISTENT_VAULT_KEY", var);
        assert_eq!(src, KeySource::Missing);

        unsafe { std::env::remove_var(var) };
    }

    #[test]
    fn keyless_providers_are_never_reported_missing() {
        assert!(provider_key_env("ollama").is_none());
        assert!(provider_key_env("echo").is_none());
        assert!(provider_key_env("totally-unknown").is_none());

        assert_eq!(
            resolve_provider_key_source("ollama", None),
            KeySource::Config
        );
        assert!(resolve_provider_key_source("ollama", None).is_resolved());
    }

    #[test]
    fn every_known_provider_maps_to_an_api_key_var() {
        for p in [
            "anthropic",
            "openai",
            "sansa",
            "deepseek",
            "mistral",
            "gemini",
            "falcon",
            "jais",
            "qwen",
            "yi",
            "cohere",
            "minimax",
            "moonshot",
            "openrouter",
        ] {
            let var = provider_key_env(p).unwrap_or_else(|| panic!("{p} must map to a var"));
            assert!(
                var.ends_with("_API_KEY"),
                "{p} maps to {var}, which breaks the *_API_KEY convention"
            );
        }
    }

    #[test]
    fn key_source_labels_distinguish_resolved_from_missing() {
        assert!(KeySource::Vault.is_resolved());
        assert!(KeySource::Config.is_resolved());
        assert!(KeySource::Env.is_resolved());
        assert!(!KeySource::Missing.is_resolved());
        assert_eq!(KeySource::Missing.label(), "not configured");
    }
}
