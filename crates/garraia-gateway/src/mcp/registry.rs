//! Runtime registry that tracks the live state of every configured MCP server.
//!
//! [`McpRuntimeRegistry`] bridges two worlds:
//!
//! - **Config layer** ([`McpConfig`]) — the static list of servers loaded from
//!   `mcp.json` or written through the admin API.
//! - **Agent layer** ([`garraia_agents::McpManager`]) — the process/network
//!   connections that do the actual work.
//!
//! The registry is the authoritative source for the admin API: it combines the
//! static config with live status so that `/admin/api/mcp` can return a full
//! picture in one shot.
//!
//! # Thread safety
//!
//! [`McpRuntimeRegistry`] is cheaply cloneable — clones share the same inner
//! state via `Arc`. All mutations go through a `tokio::sync::RwLock`.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use tracing::{debug, info};

use super::{McpConfig, McpServer, McpServerConfig, McpStatus};

// ── Inner state ───────────────────────────────────────────────────────────────

struct Inner {
    /// Server name → combined runtime view.
    servers: HashMap<String, McpServer>,
}

impl Inner {
    fn from_config(config: &McpConfig) -> Self {
        let servers = config
            .mcp_servers
            .iter()
            .map(|(name, cfg)| {
                let server = McpServer::stopped(name.clone(), cfg.clone());
                (name.clone(), server)
            })
            .collect();
        Self { servers }
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Tracks the live runtime state of all configured MCP servers.
///
/// Construct one with [`McpRuntimeRegistry::new`] (pass an initial
/// [`McpConfig`]) then call [`McpRuntimeRegistry::sync_from_manager`] after
/// the agent-layer connections are established to populate live statuses.
#[derive(Clone)]
pub struct McpRuntimeRegistry {
    inner: Arc<RwLock<Inner>>,
}

impl McpRuntimeRegistry {
    /// Create a registry pre-populated with all servers from `config`, each
    /// starting in [`McpStatus::Stopped`].
    pub fn new(config: &McpConfig) -> Self {
        Self {
            inner: Arc::new(RwLock::new(Inner::from_config(config))),
        }
    }

    // ── Queries ──────────────────────────────────────────────────────────────

    /// Return a snapshot of all servers (sorted by name for stable output).
    pub async fn list(&self) -> Vec<McpServer> {
        let guard = self.inner.read().await;
        let mut servers: Vec<McpServer> = guard.servers.values().cloned().collect();
        servers.sort_by(|a, b| a.name.cmp(&b.name));
        servers
    }

    /// Return the runtime view of a single server, or `None` if unknown.
    pub async fn get(&self, name: &str) -> Option<McpServer> {
        self.inner.read().await.servers.get(name).cloned()
    }

    /// Return `true` if a server with the given name is registered.
    pub async fn contains(&self, name: &str) -> bool {
        self.inner.read().await.servers.contains_key(name)
    }

    /// Snapshot the current config (all server configs, without live status).
    ///
    /// Used by the persistence service to write `mcp.json`.
    pub async fn config_snapshot(&self) -> McpConfig {
        let guard = self.inner.read().await;
        let mcp_servers = guard
            .servers
            .iter()
            .map(|(name, srv)| (name.clone(), srv.config.clone()))
            .collect();
        McpConfig { mcp_servers }
    }

    // ── Mutations ─────────────────────────────────────────────────────────────

    /// Add or replace a server entry (config only; status resets to Stopped).
    pub async fn add_server(&self, name: impl Into<String>, config: McpServerConfig) {
        let name = name.into();
        info!("mcp registry: adding server '{name}'");
        let server = McpServer::stopped(name.clone(), config);
        self.inner.write().await.servers.insert(name, server);
    }

    /// Remove a server entry. Returns `true` if it existed.
    pub async fn remove_server(&self, name: &str) -> bool {
        let removed = self.inner.write().await.servers.remove(name).is_some();
        if removed {
            info!("mcp registry: removed server '{name}'");
        }
        removed
    }

    /// Update the live status and tool count for a specific server.
    ///
    /// If the server is not in the registry, the call is a no-op.
    pub async fn set_status(&self, name: &str, status: McpStatus, tool_count: usize) {
        let mut guard = self.inner.write().await;
        if let Some(server) = guard.servers.get_mut(name) {
            debug!(
                "mcp registry: '{name}' status {:?} -> {:?} ({tool_count} tools)",
                server.status, status
            );
            server.status = status;
            server.tool_count = tool_count;
        }
    }

    // ── Agent-layer sync ──────────────────────────────────────────────────────

    /// Sync live statuses from the agent-layer [`McpManager`].
    ///
    /// For each server reported by the manager:
    /// - connected → [`McpStatus::Running`]
    /// - not connected but known to registry → keeps its current status
    ///   (preserves an earlier [`McpStatus::Error`] if that's what caused the
    ///   disconnect; otherwise stays [`McpStatus::Stopped`])
    ///
    /// Servers present in the registry but *absent* from the manager are left
    /// at their current status (they may never have connected yet).
    pub async fn sync_from_manager(&self, manager: &garraia_agents::McpManager) {
        let live_servers = manager.list_servers().await;

        let mut guard = self.inner.write().await;
        for (name, tool_count, is_connected) in live_servers {
            if let Some(server) = guard.servers.get_mut(&name) {
                if is_connected {
                    server.status = McpStatus::Running;
                    server.tool_count = tool_count;
                }
                // disconnected: keep existing status as-is
            } else {
                // Server is alive in the manager but not in our registry — register it
                // as an "unmanaged" entry so it shows up in the admin API.
                debug!("mcp registry: discovered unmanaged server '{name}' from manager");
                // We have no static config for it; fabricate a minimal one.
                let config = McpServerConfig {
                    command: None,
                    args: vec![],
                    env: Default::default(),
                    url: None,
                    transport: None,
                    timeout_secs: 30,
                };
                let mut server = McpServer::stopped(name.clone(), config);
                if is_connected {
                    server.status = McpStatus::Running;
                    server.tool_count = tool_count;
                }
                guard.servers.insert(name, server);
            }
        }
    }

    /// Mark a server as errored (e.g. failed to connect at startup).
    pub async fn mark_error(&self, name: &str, message: impl Into<String>) {
        let message = message.into();
        let mut guard = self.inner.write().await;
        if let Some(server) = guard.servers.get_mut(name) {
            server.status = McpStatus::Error { message };
            server.tool_count = 0;
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::{McpConfig, McpServerConfig, McpStatus, McpTransportType};

    fn make_stdio_config(cmd: &str) -> McpServerConfig {
        McpServerConfig {
            command: Some(cmd.into()),
            args: vec![],
            env: Default::default(),
            url: None,
            transport: None,
            timeout_secs: 30,
        }
    }

    fn make_config_with(names: &[&str]) -> McpConfig {
        let mut mcp_servers = std::collections::HashMap::new();
        for name in names {
            mcp_servers.insert(name.to_string(), make_stdio_config("npx"));
        }
        McpConfig { mcp_servers }
    }

    #[tokio::test]
    async fn new_starts_all_stopped() {
        let config = make_config_with(&["alpha", "beta"]);
        let reg = McpRuntimeRegistry::new(&config);
        let servers = reg.list().await;
        assert_eq!(servers.len(), 2);
        for s in &servers {
            assert_eq!(s.status, McpStatus::Stopped);
            assert_eq!(s.tool_count, 0);
        }
    }

    #[tokio::test]
    async fn list_returns_sorted() {
        let config = make_config_with(&["zeta", "alpha", "mu"]);
        let reg = McpRuntimeRegistry::new(&config);
        let names: Vec<_> = reg.list().await.into_iter().map(|s| s.name).collect();
        assert_eq!(names, vec!["alpha", "mu", "zeta"]);
    }

    #[tokio::test]
    async fn get_returns_none_for_unknown() {
        let config = make_config_with(&["alpha"]);
        let reg = McpRuntimeRegistry::new(&config);
        assert!(reg.get("unknown").await.is_none());
        assert!(reg.get("alpha").await.is_some());
    }

    #[tokio::test]
    async fn set_status_updates_entry() {
        let config = make_config_with(&["alpha"]);
        let reg = McpRuntimeRegistry::new(&config);
        reg.set_status("alpha", McpStatus::Running, 5).await;
        let s = reg.get("alpha").await.unwrap();
        assert_eq!(s.status, McpStatus::Running);
        assert_eq!(s.tool_count, 5);
    }

    #[tokio::test]
    async fn set_status_noop_for_unknown() {
        let config = make_config_with(&["alpha"]);
        let reg = McpRuntimeRegistry::new(&config);
        // Should not panic
        reg.set_status("ghost", McpStatus::Running, 3).await;
        assert!(reg.get("ghost").await.is_none());
    }

    #[tokio::test]
    async fn add_and_remove_server() {
        let config = McpConfig::default();
        let reg = McpRuntimeRegistry::new(&config);

        reg.add_server("new-server", make_stdio_config("uvx")).await;
        assert!(reg.contains("new-server").await);
        let s = reg.get("new-server").await.unwrap();
        assert_eq!(s.status, McpStatus::Stopped);

        let removed = reg.remove_server("new-server").await;
        assert!(removed);
        assert!(!reg.contains("new-server").await);

        let not_removed = reg.remove_server("new-server").await;
        assert!(!not_removed);
    }

    #[tokio::test]
    async fn mark_error_sets_error_status() {
        let config = make_config_with(&["alpha"]);
        let reg = McpRuntimeRegistry::new(&config);
        reg.mark_error("alpha", "connection refused").await;
        let s = reg.get("alpha").await.unwrap();
        assert!(matches!(s.status, McpStatus::Error { ref message } if message == "connection refused"));
        assert_eq!(s.tool_count, 0);
    }

    #[tokio::test]
    async fn config_snapshot_reflects_servers() {
        let config = make_config_with(&["alpha", "beta"]);
        let reg = McpRuntimeRegistry::new(&config);
        // Add status changes — these must NOT appear in the snapshot (only config)
        reg.set_status("alpha", McpStatus::Running, 3).await;

        let snapshot = reg.config_snapshot().await;
        assert_eq!(snapshot.mcp_servers.len(), 2);
        assert!(snapshot.mcp_servers.contains_key("alpha"));
        assert!(snapshot.mcp_servers.contains_key("beta"));
    }

    #[tokio::test]
    async fn clone_shares_state() {
        let config = make_config_with(&["alpha"]);
        let reg = McpRuntimeRegistry::new(&config);
        let clone = reg.clone();

        reg.set_status("alpha", McpStatus::Running, 7).await;
        // The clone should see the same update.
        let s = clone.get("alpha").await.unwrap();
        assert_eq!(s.status, McpStatus::Running);
        assert_eq!(s.tool_count, 7);
    }

    #[tokio::test]
    async fn infer_transport_stored_correctly() {
        let config = McpConfig::default();
        let reg = McpRuntimeRegistry::new(&config);

        let cfg = McpServerConfig {
            command: Some("npx".into()),
            args: vec!["-y".into(), "mcp-server".into()],
            env: Default::default(),
            url: None,
            transport: Some(McpTransportType::Sse),
            timeout_secs: 60,
        };
        reg.add_server("sse-server", cfg).await;

        let s = reg.get("sse-server").await.unwrap();
        assert_eq!(s.config.infer_transport(), McpTransportType::Sse);
        assert_eq!(s.config.timeout_secs, 60);
    }
}
