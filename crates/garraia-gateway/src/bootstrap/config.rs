//! Bootstrap configuration helpers.
//!
//! Slice 10.a of GAR-440 (Q10 of EPIC GAR-430 Quality Gates Phase 3.6).
//! Extracted from `bootstrap.rs` without behavior change — holds the path
//! resolvers and the API-key precedence chain (vault → config → env) that
//! every downstream pipeline (agents, channels, state, router, admin)
//! consumes via `crate::bootstrap::{default_vault_path, resolve_api_key}`.

use std::path::PathBuf;

/// Default vault path under the user's home directory.
pub(crate) fn default_vault_path() -> Option<PathBuf> {
    Some(
        garraia_config::ConfigLoader::default_config_dir()
            .join("credentials")
            .join("vault.json"),
    )
}

pub(super) fn default_allowlist_path() -> PathBuf {
    garraia_config::ConfigLoader::default_config_dir().join("allowlist.json")
}

/// Resolve an API key using the priority chain: vault -> config -> env var.
pub(crate) fn resolve_api_key(
    config_key: Option<&str>,
    vault_credential_key: &str,
    env_var: &str,
) -> Option<String> {
    // 1. Try credential vault (only works when GARRAIA_VAULT_PASSPHRASE is set)
    if let Some(vault_path) = default_vault_path()
        && let Some(val) = garraia_security::try_vault_get(&vault_path, vault_credential_key)
    {
        return Some(val);
    }

    // 2. Config file value
    if let Some(key) = config_key
        && !key.is_empty()
    {
        return Some(key.to_string());
    }

    // 3. Environment variable
    std::env::var(env_var).ok()
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
}
