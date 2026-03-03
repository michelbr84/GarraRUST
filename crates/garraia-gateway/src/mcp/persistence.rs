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

use super::{McpConfig, McpRuntimeRegistry};

/// Loads and saves `mcp.json`, and builds [`McpRuntimeRegistry`] from it.
#[derive(Clone)]
pub struct McpPersistenceService {
    path: PathBuf,
}

impl McpPersistenceService {
    /// Create a service that reads/writes the file at `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Create a service pointing to the default `mcp.json` location
    /// (`<garraia_config_dir>/mcp.json`).
    pub fn with_default_path() -> Self {
        Self::new(garraia_config::ConfigLoader::default_config_dir().join("mcp.json"))
    }

    /// The path this service manages.
    pub fn path(&self) -> &Path {
        &self.path
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
    pub fn load_registry(&self) -> McpRuntimeRegistry {
        match self.load() {
            Ok(config) => McpRuntimeRegistry::new(&config),
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
    pub async fn save_from_registry(&self, registry: &McpRuntimeRegistry) -> anyhow::Result<()> {
        let config = registry.config_snapshot().await;
        self.save(&config)
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
                        env: HashMap::new(),
                        url: None,
                        transport: None,
                        timeout_secs: 30,
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
