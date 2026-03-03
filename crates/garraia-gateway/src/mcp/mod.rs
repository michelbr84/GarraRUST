//! MCP (Model Context Protocol) domain types for the gateway.
//!
//! Defines the configuration and runtime models for MCP servers managed
//! by the gateway. These types are compatible with the `mcp.json` file
//! format used by Claude Desktop and similar tools.
//!
//! # Overview
//!
//! - [`McpTransportType`] — how a server communicates (stdio, HTTP, SSE, etc.)
//! - [`McpServerConfig`] — static configuration loaded from `mcp.json`
//! - [`McpStatus`] — live runtime state of a connection
//! - [`McpServer`] — combined view (config + status) for the admin API
//! - [`McpConfig`] — top-level `mcp.json` wrapper (`{"mcpServers": {...}}`)
//! - [`McpRuntimeRegistry`] — thread-safe registry of live server states

pub mod persistence;
pub mod registry;

pub use persistence::McpPersistenceService;
pub use registry::McpRuntimeRegistry;

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

// ── Transport ────────────────────────────────────────────────────────────────

/// The communication transport used to connect to an MCP server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum McpTransportType {
    /// Spawn a child process and communicate over stdin/stdout.
    Stdio,
    /// Plain HTTP request/response.
    Http,
    /// Server-Sent Events (SSE) stream.
    Sse,
    /// MCP Streamable HTTP transport (bidirectional over HTTP).
    StreamableHttp,
}

// ── Config ───────────────────────────────────────────────────────────────────

/// Configuration for a single MCP server entry.
///
/// This matches the per-server object inside `mcp.json`:
/// ```json
/// {
///   "command": "npx",
///   "args": ["-y", "some-mcp-server"],
///   "env": { "SOME_VAR": "value" }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfig {
    /// Shell command to run (stdio transport).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,

    /// Arguments to pass to `command`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,

    /// Extra environment variables for the spawned process.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub env: HashMap<String, String>,

    /// Base URL for HTTP / SSE / StreamableHttp transports.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// Explicit transport override. Inferred from `command`/`url` when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<McpTransportType>,

    /// Seconds to wait for the initial handshake (default: 30).
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

fn default_timeout_secs() -> u64 {
    30
}

impl McpServerConfig {
    /// Infer the transport from the fields present in the config.
    ///
    /// - Explicit `transport` field wins.
    /// - `command` present → [`McpTransportType::Stdio`].
    /// - `url` present → [`McpTransportType::StreamableHttp`] (most common URL-based type).
    /// - Otherwise defaults to [`McpTransportType::Stdio`].
    pub fn infer_transport(&self) -> McpTransportType {
        if let Some(ref t) = self.transport {
            return t.clone();
        }
        if self.command.is_some() {
            return McpTransportType::Stdio;
        }
        if self.url.is_some() {
            return McpTransportType::StreamableHttp;
        }
        McpTransportType::Stdio
    }
}

// ── Status ───────────────────────────────────────────────────────────────────

/// Runtime connection status for an MCP server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum McpStatus {
    /// Successfully connected and ready.
    Running,
    /// Not yet started or explicitly stopped.
    Stopped,
    /// Connection attempt failed or connection was lost.
    Error {
        /// Human-readable error description.
        message: String,
    },
}

impl McpStatus {
    /// Returns `true` when the server is in the [`McpStatus::Running`] state.
    pub fn is_running(&self) -> bool {
        matches!(self, McpStatus::Running)
    }
}

// ── Runtime view ─────────────────────────────────────────────────────────────

/// Combined runtime view of an MCP server, suitable for the admin API.
///
/// Merges static [`McpServerConfig`] with live status information reported
/// by [`garraia_agents::McpManager`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServer {
    /// Unique name (key from `mcp.json` / admin API).
    pub name: String,

    /// Static configuration.
    pub config: McpServerConfig,

    /// Current runtime status.
    pub status: McpStatus,

    /// Number of tools discovered during the last successful handshake.
    pub tool_count: usize,
}

impl McpServer {
    /// Convenience constructor for a stopped server (not yet connected).
    pub fn stopped(name: impl Into<String>, config: McpServerConfig) -> Self {
        Self {
            name: name.into(),
            config,
            status: McpStatus::Stopped,
            tool_count: 0,
        }
    }
}

// ── Top-level file format ────────────────────────────────────────────────────

/// Top-level `mcp.json` wrapper.
///
/// Matches the format used by Claude Desktop:
/// ```json
/// {
///   "mcpServers": {
///     "my-server": { "command": "npx", "args": ["my-mcp"] }
///   }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct McpConfig {
    /// Map of server name → configuration.
    #[serde(default)]
    pub mcp_servers: HashMap<String, McpServerConfig>,
}

impl McpConfig {
    /// Load an `McpConfig` from a JSON file.
    ///
    /// Returns an empty config if the file does not exist.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(path)?;
        let config: Self = serde_json::from_str(&raw)?;
        Ok(config)
    }

    /// Persist the config to a JSON file (pretty-printed).
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Iterator over `(name, config)` pairs.
    pub fn servers(&self) -> impl Iterator<Item = (&str, &McpServerConfig)> {
        self.mcp_servers.iter().map(|(k, v)| (k.as_str(), v))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_transport_stdio_from_command() {
        let cfg = McpServerConfig {
            command: Some("npx".into()),
            args: vec!["-y".into(), "mcp-server".into()],
            env: Default::default(),
            url: None,
            transport: None,
            timeout_secs: 30,
        };
        assert_eq!(cfg.infer_transport(), McpTransportType::Stdio);
    }

    #[test]
    fn infer_transport_explicit_wins() {
        let cfg = McpServerConfig {
            command: Some("npx".into()),
            args: vec![],
            env: Default::default(),
            url: None,
            transport: Some(McpTransportType::Sse),
            timeout_secs: 30,
        };
        assert_eq!(cfg.infer_transport(), McpTransportType::Sse);
    }

    #[test]
    fn infer_transport_url_defaults_to_streamable_http() {
        let cfg = McpServerConfig {
            command: None,
            args: vec![],
            env: Default::default(),
            url: Some("https://example.com/mcp".into()),
            transport: None,
            timeout_secs: 30,
        };
        assert_eq!(cfg.infer_transport(), McpTransportType::StreamableHttp);
    }

    #[test]
    fn mcp_status_is_running() {
        assert!(McpStatus::Running.is_running());
        assert!(!McpStatus::Stopped.is_running());
        assert!(!McpStatus::Error { message: "oops".into() }.is_running());
    }

    #[test]
    fn mcp_config_roundtrip_json() {
        let json = r#"{
            "mcpServers": {
                "gradio": {
                    "command": "npx",
                    "args": ["mcp-remote", "https://example.com/mcp/sse", "--transport", "sse-only"]
                },
                "n8n-mcp": {
                    "command": "npx",
                    "args": ["-y", "supergateway", "--streamableHttp", "https://n8n.example.com/mcp"]
                }
            }
        }"#;

        let cfg: McpConfig = serde_json::from_str(json).expect("parse");
        assert_eq!(cfg.mcp_servers.len(), 2);
        assert!(cfg.mcp_servers.contains_key("gradio"));
        assert!(cfg.mcp_servers.contains_key("n8n-mcp"));

        let gradio = &cfg.mcp_servers["gradio"];
        assert_eq!(gradio.command.as_deref(), Some("npx"));
        assert_eq!(gradio.infer_transport(), McpTransportType::Stdio);

        // Re-serialise and parse again to ensure lossless roundtrip.
        let serialised = serde_json::to_string(&cfg).expect("serialise");
        let cfg2: McpConfig = serde_json::from_str(&serialised).expect("re-parse");
        assert_eq!(cfg2.mcp_servers.len(), 2);
    }

    #[test]
    fn mcp_server_stopped_constructor() {
        let config = McpServerConfig {
            command: Some("uvx".into()),
            args: vec!["my-tool".into()],
            env: Default::default(),
            url: None,
            transport: None,
            timeout_secs: 30,
        };
        let server = McpServer::stopped("my-server", config);
        assert_eq!(server.name, "my-server");
        assert_eq!(server.status, McpStatus::Stopped);
        assert!(!server.status.is_running());
        assert_eq!(server.tool_count, 0);
    }

    #[test]
    fn mcp_status_serde_tagged() {
        let running = McpStatus::Running;
        let stopped = McpStatus::Stopped;
        let error = McpStatus::Error { message: "timeout".into() };

        let r = serde_json::to_value(&running).unwrap();
        assert_eq!(r["state"], "running");

        let s = serde_json::to_value(&stopped).unwrap();
        assert_eq!(s["state"], "stopped");

        let e = serde_json::to_value(&error).unwrap();
        assert_eq!(e["state"], "error");
        assert_eq!(e["message"], "timeout");
    }

    #[test]
    fn load_returns_default_when_file_missing() {
        let path = std::path::Path::new("/nonexistent/path/mcp.json");
        let cfg = McpConfig::load(path).expect("should not error for missing file");
        assert!(cfg.mcp_servers.is_empty());
    }
}
