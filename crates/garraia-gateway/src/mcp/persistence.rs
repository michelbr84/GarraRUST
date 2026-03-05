//! Persistence for MCP server configuration.
//!
//! [`McpPersistenceService`] is the single place where `mcp.json` is read and
//! written. It uses the gateway's [`McpConfig`] format (compatible with Claude
//! Desktop) and bridges it to the in-memory [`McpRuntimeRegistry`].
//!
//! # File location
//!
//! By default the file lives at `<config_dir>/mcp.json` where `config_dir` is
//! resolved by [`garraia_config::ConfigLoader::default_config_dir`]
//! (usually `~/.garraia/` or `$XDG_CONFIG_HOME/garraia/`).

use std::path::{Path, PathBuf};

use tracing::{info, warn};

use super::{McpConfig, McpRuntimeRegistry, is_sensitive_key};

/// Sentinel prefix for vault-referenced env values in `mcp.json`.
const VAULT_REF_PREFIX: &str = "vault:";

/// Returns the vault key used to store `env_key` for `server_name`.
fn vault_key(server_name: &str, env_key: &str) -> String {
    format!("mcp.{server_name}.{env_key}")
}

/// Loads and saves `mcp.json`, and builds [`McpRuntimeRegistry`] from it.
///
/// When a `vault_path` is configured (via [`with_vault`](Self::with_vault)),
/// sensitive env vars (API keys, tokens, etc.) are stored encrypted in the
/// vault and replaced by `vault:<key>` references in `mcp.json`. On load,
/// vault references are resolved back to their plaintext values for use at
/// runtime. If the vault is unavailable, plaintext values are used as-is.
#[derive(Clone)]
pub struct McpPersistenceService {
    path: PathBuf,
    /// Path to the AES-256-GCM credential vault (see `garraia-security`).
    /// When `None`, env vars are saved as plaintext (with a warning).
    vault_path: Option<PathBuf>,
}

impl McpPersistenceService {
    /// Create a service that reads/writes the file at `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into(), vault_path: None }
    }

    /// Create a service pointing to the default `mcp.json` location
    /// (`<garraia_config_dir>/mcp.json`).
    pub fn with_default_path() -> Self {
        Self::new(garraia_config::ConfigLoader::default_config_dir().join("mcp.json"))
    }

    /// Attach an encrypted vault for sensitive env var storage (GAR-291).
    ///
    /// When set, [`load_registry`](Self::load_registry) resolves `vault:` refs
    /// from the vault and [`save_from_registry`](Self::save_from_registry)
    /// encrypts sensitive values into the vault.
    pub fn with_vault(mut self, vault_path: impl Into<PathBuf>) -> Self {
        self.vault_path = Some(vault_path.into());
        self
    }

    /// The path this service manages.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Seed `mcp.json` with a Filesystem MCP entry when the file does not exist yet.
    ///
    /// This is a first-run convenience: new installations get local filesystem
    /// access immediately without requiring manual admin-UI configuration.
    /// Existing installations (file already present) are **never** modified.
    ///
    /// The allowed root is the user's home directory (`$HOME` / `%USERPROFILE%`),
    /// falling back to the parent of `~/.garraia/` if the env var is absent.
    pub fn provision_filesystem_if_missing(&self) {
        if self.path.exists() {
            return;
        }

        // Resolve the user's home directory in a cross-platform way.
        let home_dir = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                // Fallback: parent of ~/.garraia/ → ~
                self.path
                    .parent()
                    .and_then(|p| p.parent())
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| PathBuf::from("."))
            });

        let mut config = McpConfig::default();
        config.mcp_servers.insert(
            "filesystem".to_string(),
            super::McpServerConfig {
                command: Some("npx".to_string()),
                args: vec![
                    "-y".to_string(),
                    "@modelcontextprotocol/server-filesystem".to_string(),
                    home_dir.to_string_lossy().into_owned(),
                ],
                env: Default::default(),
                url: None,
                transport: None,
                timeout_secs: 30,
                memory_limit_mb: None,
                max_restarts: None,
                restart_delay_secs: None,
            },
        );

        match self.save(&config) {
            Ok(()) => info!(
                "mcp: provisioned default mcp.json with filesystem MCP at {}",
                home_dir.display()
            ),
            Err(e) => warn!("mcp: failed to provision default mcp.json: {e}"),
        }
    }

    /// Load `mcp.json` from disk.
    ///
    /// Returns an empty [`McpConfig`] when the file does not exist yet.
    pub fn load(&self) -> anyhow::Result<McpConfig> {
        let config = McpConfig::load(&self.path)?;
        info!(
            "mcp persistence: loaded {} server(s) from {}",
            config.mcp_servers.len(),
            self.path.display()
        );
        Ok(config)
    }

    /// Save a [`McpConfig`] snapshot to `mcp.json` (pretty-printed JSON).
    pub fn save(&self, config: &McpConfig) -> anyhow::Result<()> {
        config.save(&self.path)?;
        info!(
            "mcp persistence: saved {} server(s) to {}",
            config.mcp_servers.len(),
            self.path.display()
        );
        Ok(())
    }

    /// Load the file and build a registry with all servers in [`McpStatus::Stopped`].
    ///
    /// Call [`McpRuntimeRegistry::sync_from_manager`] after the agent-layer
    /// connections are established to update the live statuses.
    ///
    /// **GAR-291**: If a vault is configured, `vault:` references in env values
    /// are resolved to their plaintext secrets before the registry is built.
    /// Servers are never exposed to unresolved `vault:` strings at runtime.
    pub fn load_registry(&self) -> McpRuntimeRegistry {
        match self.load() {
            Ok(mut config) => {
                self.resolve_vault_refs(&mut config);
                McpRuntimeRegistry::new(&config)
            }
            Err(e) => {
                warn!(
                    "mcp persistence: failed to load mcp.json ({}), starting with empty registry: {e}",
                    self.path.display()
                );
                McpRuntimeRegistry::new(&McpConfig::default())
            }
        }
    }

    /// Snapshot the registry's current config and write it to `mcp.json`.
    ///
    /// **GAR-291**: If a vault is configured and `GARRAIA_VAULT_PASSPHRASE` is
    /// set, sensitive env values are moved to the vault and replaced by
    /// `vault:<key>` references. If the vault is unavailable, plaintext is
    /// written with a warning (graceful degradation — never blocks operation).
    pub async fn save_from_registry(&self, registry: &McpRuntimeRegistry) -> anyhow::Result<()> {
        let mut config = registry.config_snapshot().await;
        self.encrypt_to_vault(&mut config);
        self.save(&config)
    }

    /// Remove all vault credentials whose key starts with `mcp.<server_name>.`.
    ///
    /// Called by the DELETE endpoint (GAR-286) so that removing a server also
    /// cleans up its encrypted credentials. No-op if no vault is configured or
    /// `GARRAIA_VAULT_PASSPHRASE` is not set.
    pub fn delete_server_vault_entries(&self, server_name: &str) {
        if let Some(vault_path) = &self.vault_path {
            let prefix = format!("mcp.{server_name}.");
            let removed = garraia_security::try_vault_delete_prefix(vault_path, &prefix);
            if removed > 0 {
                info!(server = %server_name, "mcp: removed {removed} vault credential(s)");
            }
        }
    }

    // ── Vault helpers (GAR-291) ───────────────────────────────────────────────

    /// Resolve `vault:` references in env values using the configured vault.
    ///
    /// Values that are already plaintext (no prefix) are left untouched.
    /// Unresolvable `vault:` refs emit a warning and remain as-is so the
    /// server config is still visible for debugging.
    fn resolve_vault_refs(&self, config: &mut McpConfig) {
        let vault_path = match &self.vault_path {
            Some(p) => p.as_path(),
            None => return, // no vault configured — nothing to resolve
        };

        for (server_name, server_cfg) in config.mcp_servers.iter_mut() {
            for (env_key, env_val) in server_cfg.env.iter_mut() {
                if let Some(vk) = env_val.strip_prefix(VAULT_REF_PREFIX) {
                    match garraia_security::try_vault_get(vault_path, vk) {
                        Some(resolved) => *env_val = resolved,
                        None => warn!(
                            server = %server_name,
                            env_key = %env_key,
                            vault_ref = %vk,
                            "mcp: vault ref unresolvable — vault missing or GARRAIA_VAULT_PASSPHRASE not set"
                        ),
                    }
                }
            }
        }
    }

    /// Move sensitive plaintext env values into the vault and replace them
    /// with `vault:` references in `config`.
    ///
    /// No-op if no vault is configured or `GARRAIA_VAULT_PASSPHRASE` is absent.
    fn encrypt_to_vault(&self, config: &mut McpConfig) {
        let vault_path = match &self.vault_path {
            Some(p) => p.as_path(),
            None => {
                // Check if any server has sensitive keys and warn if so.
                let has_secrets = config.mcp_servers.values()
                    .any(|s| s.env.keys().any(|k| is_sensitive_key(k)));
                if has_secrets {
                    warn!("mcp: vault not configured — saving sensitive env vars as plaintext; set GARRAIA_VAULT_PASSPHRASE to enable encryption");
                }
                return;
            }
        };

        if std::env::var("GARRAIA_VAULT_PASSPHRASE").unwrap_or_default().is_empty() {
            let has_secrets = config.mcp_servers.values()
                .any(|s| s.env.keys().any(|k| is_sensitive_key(k)));
            if has_secrets {
                warn!("mcp: GARRAIA_VAULT_PASSPHRASE not set — sensitive env vars saved as plaintext");
            }
            return;
        }

        for (server_name, server_cfg) in config.mcp_servers.iter_mut() {
            for (env_key, env_val) in server_cfg.env.iter_mut() {
                // Skip values already stored as vault references.
                if env_val.starts_with(VAULT_REF_PREFIX) {
                    continue;
                }
                if !is_sensitive_key(env_key) {
                    continue;
                }
                let vk = vault_key(server_name, env_key);
                if garraia_security::try_vault_set(vault_path, &vk, env_val) {
                    info!(
                        server = %server_name,
                        env_key = %env_key,
                        "mcp: stored credential in vault"
                    );
                    *env_val = format!("{VAULT_REF_PREFIX}{vk}");
                } else {
                    warn!(
                        server = %server_name,
                        env_key = %env_key,
                        "mcp: failed to store credential in vault — saving as plaintext"
                    );
                }
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::mcp::{McpServerConfig, McpStatus};

    fn temp_mcp_json(content: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mcp.json");
        std::fs::write(&path, content).expect("write");
        (dir, path)
    }

    fn make_config(names: &[&str]) -> McpConfig {
        let mcp_servers = names
            .iter()
            .map(|&n| {
                (
                    n.to_string(),
                    McpServerConfig {
                        command: Some("npx".into()),
                        args: vec![n.into()],
                        ..Default::default()
                    },
                )
            })
            .collect();
        McpConfig { mcp_servers }
    }

    #[test]
    fn load_parses_valid_mcp_json() {
        let json = r#"{"mcpServers":{"gradio":{"command":"npx","args":["mcp-remote"]}}}"#;
        let (_dir, path) = temp_mcp_json(json);
        let svc = McpPersistenceService::new(&path);
        let cfg = svc.load().expect("load");
        assert_eq!(cfg.mcp_servers.len(), 1);
        assert!(cfg.mcp_servers.contains_key("gradio"));
    }

    #[test]
    fn load_returns_empty_when_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nonexistent.json");
        let svc = McpPersistenceService::new(&path);
        let cfg = svc.load().expect("load returns default");
        assert!(cfg.mcp_servers.is_empty());
    }

    #[test]
    fn save_and_reload_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mcp.json");
        let svc = McpPersistenceService::new(&path);

        let original = make_config(&["alpha", "beta"]);
        svc.save(&original).expect("save");

        let loaded = svc.load().expect("reload");
        assert_eq!(loaded.mcp_servers.len(), 2);
        assert!(loaded.mcp_servers.contains_key("alpha"));
        assert!(loaded.mcp_servers.contains_key("beta"));
    }

    #[test]
    fn save_creates_parent_dirs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nested").join("dir").join("mcp.json");
        let svc = McpPersistenceService::new(&path);
        let cfg = make_config(&["server1"]);
        svc.save(&cfg).expect("save with nested dirs");
        assert!(path.exists());
    }

    #[test]
    fn load_registry_builds_all_stopped() {
        let json = r#"{"mcpServers":{"s1":{"command":"cmd1"},"s2":{"command":"cmd2"}}}"#;
        let (_dir, path) = temp_mcp_json(json);
        let svc = McpPersistenceService::new(&path);
        let reg = svc.load_registry();

        // We can't await in sync tests easily, so use block_on
        let rt = tokio::runtime::Runtime::new().unwrap();
        let servers = rt.block_on(reg.list());
        assert_eq!(servers.len(), 2);
        for s in &servers {
            assert_eq!(s.status, McpStatus::Stopped);
        }
    }

    #[tokio::test]
    async fn save_from_registry_persists_config() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mcp.json");
        let svc = McpPersistenceService::new(&path);

        let cfg = make_config(&["my-server"]);
        let reg = McpRuntimeRegistry::new(&cfg);
        // Change live status — should NOT appear in saved file (only config)
        reg.set_status("my-server", McpStatus::Running, 5).await;

        svc.save_from_registry(&reg).await.expect("save");

        // Reload and check
        let loaded = svc.load().expect("reload");
        assert_eq!(loaded.mcp_servers.len(), 1);
        assert!(loaded.mcp_servers.contains_key("my-server"));
    }
}
