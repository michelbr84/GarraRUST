//! Bootstrap configuration helpers.
//!
//! Slice 10.a of GAR-440 (Q10 of EPIC GAR-430 Quality Gates Phase 3.6)
//! extracted the path resolvers and the API-key precedence chain out of
//! `bootstrap.rs`. The precedence chain itself has since moved down into
//! [`garraia_config::provider_keys`], so that `garraia config check`, `/health`,
//! and the admin providers list can answer "does this provider have a usable
//! key?" the same way the boot path does — they previously disagreed, which is
//! how a provider could look configured on every surface and still be skipped
//! at startup.
//!
//! What remains here is the gateway-facing surface: the re-exports consumed via
//! `crate::bootstrap::{default_vault_path, resolve_api_key}` (used by
//! `admin::handlers`, `admin::mcp`, `router`, `state`) and the locked-vault
//! diagnostic.

use std::path::PathBuf;

/// Default vault path under the user's config directory.
///
/// Returns `Option` purely to preserve the signature its four call sites
/// (`router`, `state`, `admin::mcp` ×2) already branch on; the path is always
/// resolvable.
pub(crate) fn default_vault_path() -> Option<PathBuf> {
    Some(garraia_config::default_vault_path())
}

pub(super) fn default_allowlist_path() -> PathBuf {
    garraia_config::ConfigLoader::default_config_dir().join("allowlist.json")
}

/// Plan 0250 (GAR-771): emit one friendly, actionable warning when a credential
/// vault exists on disk but `GARRAIA_VAULT_PASSPHRASE` is not set — the exact
/// situation that silently disables providers/channels (the keys are encrypted
/// and we can't open the vault). Without this, the operator only sees a cryptic
/// "no API key" and has no idea their secrets are right there, locked.
///
/// This is the state the pre-v0.3.0 onboarding wizard produced by default: it
/// encrypted the key into the vault, then `garraia start` had no passphrase to
/// open it with. The wizard now writes to `config.yml` instead, so this warning
/// should only fire for operators who deliberately opted into the vault.
pub(crate) fn warn_if_vault_locked() {
    let Some(vault_path) = default_vault_path() else {
        return;
    };
    if !garraia_config::vault_present_but_locked(&vault_path) {
        return;
    }
    tracing::warn!(
        "🔒 Encontrei seu cofre de credenciais em {}, mas preciso da senha pra \
         abri-lo. Suas chaves estão guardadas e seguras — só defina a variável \
         GARRAIA_VAULT_PASSPHRASE (a mesma senha que você criou no wizard) e me \
         reinicie. Sem ela, eu subo, mas os provedores e canais ficam desligados.",
        vault_path.display()
    );
}

/// Resolve an API key using the priority chain vault -> config -> env var.
///
/// Delegates to [`garraia_config::resolve_api_key`]. Also used for channel
/// tokens (`TELEGRAM_BOT_TOKEN`, `SLACK_BOT_TOKEN`, `WHATSAPP_ACCESS_TOKEN`),
/// which follow the identical chain.
pub(crate) fn resolve_api_key(
    config_key: Option<&str>,
    vault_credential_key: &str,
    env_var: &str,
) -> Option<String> {
    garraia_config::resolve_api_key(config_key, vault_credential_key, env_var)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_api_key_prefers_config_over_env() {
        // Config value should win when present
        let result = resolve_api_key(
            Some("from-config"),
            "NONEXISTENT_VAULT_KEY",
            "NONEXISTENT_ENV_VAR_12345",
        );
        assert_eq!(result, Some("from-config".to_string()));
    }

    #[test]
    fn resolve_api_key_falls_back_to_env() {
        // Set a unique env var for this test
        let var_name = "GARRAIA_TEST_API_KEY_BOOTSTRAP_72";
        // SAFETY: this test is single-threaded and uses a unique env var name.
        unsafe { std::env::set_var(var_name, "from-env") };
        let result = resolve_api_key(None, "NONEXISTENT_VAULT_KEY", var_name);
        assert_eq!(result, Some("from-env".to_string()));
        unsafe { std::env::remove_var(var_name) };
    }

    #[test]
    fn resolve_api_key_returns_none_when_all_missing() {
        let result = resolve_api_key(None, "NONEXISTENT_VAULT_KEY", "NONEXISTENT_ENV_VAR_99999");
        assert_eq!(result, None);
    }

    /// Guards the refactor that removed fifteen hardcoded `("X_API_KEY",
    /// "X_API_KEY")` pairs from `build_agent_runtime`: every provider string the
    /// boot loop matches on must still resolve to an env var through the shared
    /// table, and the keyless ones must still report `None`.
    #[test]
    fn provider_key_table_covers_every_arm_of_the_boot_loop() {
        use garraia_config::provider_key_env;

        for (provider, expected) in [
            ("anthropic", "ANTHROPIC_API_KEY"),
            ("openai", "OPENAI_API_KEY"),
            ("sansa", "SANSA_API_KEY"),
            ("deepseek", "DEEPSEEK_API_KEY"),
            ("mistral", "MISTRAL_API_KEY"),
            ("gemini", "GEMINI_API_KEY"),
            ("falcon", "FALCON_API_KEY"),
            ("jais", "JAIS_API_KEY"),
            ("qwen", "QWEN_API_KEY"),
            ("yi", "YI_API_KEY"),
            ("cohere", "COHERE_API_KEY"),
            ("minimax", "MINIMAX_API_KEY"),
            ("moonshot", "MOONSHOT_API_KEY"),
            ("openrouter", "OPENROUTER_API_KEY"),
        ] {
            assert_eq!(
                provider_key_env(provider),
                Some(expected),
                "boot loop matches on `{provider}` but the shared table disagrees"
            );
        }

        // Keyless arms: these must never be reported as missing a key.
        assert_eq!(provider_key_env("ollama"), None);
        assert_eq!(provider_key_env("echo"), None);
    }
}
