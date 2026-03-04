use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(unix)]
use libc;

use garraia_common::{Error, Result};
use rmcp::ServiceExt;
use rmcp::service::{Peer, RoleClient, RunningService};
use rmcp::transport::TokioChildProcess;
use tokio::process::Command;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use super::tool_bridge::McpTool;
use crate::tools::Tool;

/// Cached info about a tool discovered from an MCP server.
#[derive(Debug, Clone)]
pub struct McpToolInfo {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
}

/// Cached info about a resource from an MCP server.
#[derive(Debug, Clone)]
pub struct McpResourceInfo {
    pub uri: String,
    pub name: String,
    pub description: Option<String>,
    pub mime_type: Option<String>,
}

/// Cached info about a prompt from an MCP server.
#[derive(Debug, Clone)]
pub struct McpPromptInfo {
    pub name: String,
    pub description: Option<String>,
    pub arguments: Vec<McpPromptArgument>,
}

/// A prompt argument definition.
#[derive(Debug, Clone)]
pub struct McpPromptArgument {
    pub name: String,
    pub description: Option<String>,
    pub required: bool,
}

/// Connection parameters for reconnection.
#[derive(Clone)]
enum ConnectionParams {
    Stdio {
        command: String,
        args: Vec<String>,
        env: HashMap<String, String>,
        timeout_secs: u64,
        /// GAR-293: virtual memory cap in MB (Unix only).
        memory_limit_mb: Option<u64>,
    },
    #[cfg(feature = "mcp-http")]
    Http { url: String, timeout_secs: u64 },
}

/// GAR-293: Tracks auto-restart history for one MCP server.
#[derive(Clone, Debug)]
struct RestartState {
    /// How many automatic restarts have been attempted since last successful connect.
    count: u32,
    /// When the last restart was attempted.
    last_attempt: Option<Instant>,
    /// Maximum number of restarts before giving up.
    max_restarts: u32,
    /// Base delay (seconds). Actual delay = base * 2^count, capped at 300s.
    base_delay_secs: u64,
}

impl RestartState {
    fn new(max_restarts: u32, base_delay_secs: u64) -> Self {
        Self {
            count: 0,
            last_attempt: None,
            max_restarts,
            base_delay_secs,
        }
    }

    /// Returns `true` when the backoff delay has elapsed and we should retry.
    fn should_retry_now(&self) -> bool {
        if self.count >= self.max_restarts {
            return false;
        }
        match self.last_attempt {
            None => true,
            Some(t) => {
                let delay = self.current_delay_secs();
                t.elapsed() >= Duration::from_secs(delay)
            }
        }
    }

    /// `base * 2^count`, capped at 300s.
    fn current_delay_secs(&self) -> u64 {
        let shift = self.count.min(8); // 2^8 = 256, × 5 = 1280 > 300 → will be capped
        (self.base_delay_secs << shift).min(300)
    }

    fn record_attempt(&mut self) {
        self.last_attempt = Some(Instant::now());
        self.count += 1;
    }

    fn reset(&mut self) {
        self.count = 0;
        self.last_attempt = None;
    }

    fn is_exhausted(&self) -> bool {
        self.count >= self.max_restarts
    }
}

/// A live connection to one MCP server.
struct McpConnection {
    server_name: String,
    service: RunningService<RoleClient, ()>,
    tools: Vec<McpToolInfo>,
    params: ConnectionParams,
    /// GAR-190: tool allowlist — empty means all tools are permitted.
    allowed_tools: Vec<String>,
}

/// Manages the lifecycle of MCP server connections.
pub struct McpManager {
    connections: Arc<RwLock<HashMap<String, McpConnection>>>,
    /// GAR-293: per-server restart state (survives connection removal).
    restart_states: Arc<RwLock<HashMap<String, RestartState>>>,
}

impl Default for McpManager {
    fn default() -> Self {
        Self::new()
    }
}

impl McpManager {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            restart_states: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Connect to an MCP server by spawning a child process.
    ///
    /// `allowed_tools`: GAR-190 tool allowlist. Pass an empty `Vec` to allow all tools.
    /// `memory_limit_mb`: GAR-293 — max virtual memory in MB (Unix only). `None` = no limit.
    /// `max_restarts` / `restart_delay_secs`: GAR-293 backoff config.
    #[allow(clippy::too_many_arguments)]
    pub async fn connect(
        &self,
        name: &str,
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
        timeout_secs: u64,
        allowed_tools: Vec<String>,
        memory_limit_mb: Option<u64>,
        max_restarts: u32,
        restart_delay_secs: u64,
    ) -> Result<()> {
        let mut cmd = Command::new(command);
        cmd.args(args);
        for (k, v) in env {
            cmd.env(k, v);
        }

        // GAR-293: apply memory limit on Unix via setrlimit(RLIMIT_AS).
        #[cfg(unix)]
        if let Some(limit_mb) = memory_limit_mb {
            apply_memory_limit(&mut cmd, limit_mb);
        }

        let transport = TokioChildProcess::new(cmd)
            .map_err(|e| Error::Mcp(format!("failed to spawn MCP server '{name}': {e}")))?;

        let service = tokio::time::timeout(Duration::from_secs(timeout_secs), ().serve(transport))
            .await
            .map_err(|_| {
                Error::Mcp(format!(
                    "MCP server '{name}' handshake timed out after {timeout_secs}s"
                ))
            })?
            .map_err(|e| Error::Mcp(format!("MCP server '{name}' handshake failed: {e}")))?;

        // Discover tools
        let mcp_tools = service
            .list_all_tools()
            .await
            .map_err(|e| Error::Mcp(format!("failed to list tools from '{name}': {e}")))?;

        let tools: Vec<McpToolInfo> = mcp_tools
            .into_iter()
            .map(|t| McpToolInfo {
                name: t.name.to_string(),
                description: t.description.map(|d| d.to_string()),
                input_schema: serde_json::to_value(&*t.input_schema).unwrap_or_default(),
            })
            .collect();

        info!(
            "MCP server '{name}' connected: {} tool(s) discovered",
            tools.len()
        );
        for tool in &tools {
            info!("  -> {name}.{}", tool.name);
        }

        // GAR-190: log which tools are blocked by the allowlist
        if !allowed_tools.is_empty() {
            let blocked: Vec<&str> = tools
                .iter()
                .filter(|t| !allowed_tools.contains(&t.name))
                .map(|t| t.name.as_str())
                .collect();
            if !blocked.is_empty() {
                info!(
                    "MCP server '{name}': allowlist active — {} tool(s) blocked: {:?}",
                    blocked.len(),
                    blocked
                );
            }
        }

        let conn = McpConnection {
            server_name: name.to_string(),
            service,
            tools,
            params: ConnectionParams::Stdio {
                command: command.to_string(),
                args: args.to_vec(),
                env: env.clone(),
                timeout_secs,
                memory_limit_mb,
            },
            allowed_tools,
        };

        self.connections
            .write()
            .await
            .insert(name.to_string(), conn);

        // GAR-293: reset restart state on successful connect.
        self.restart_states
            .write()
            .await
            .entry(name.to_string())
            .and_modify(|s| s.reset())
            .or_insert_with(|| RestartState::new(max_restarts, restart_delay_secs));

        Ok(())
    }

    /// Connect to an MCP server via HTTP (Streamable HTTP transport).
    #[cfg(feature = "mcp-http")]
    pub async fn connect_http(&self, name: &str, url: &str, timeout_secs: u64, allowed_tools: Vec<String>, max_restarts: u32, restart_delay_secs: u64) -> Result<()> {
        use rmcp::transport::StreamableHttpClientTransport;

        let transport = StreamableHttpClientTransport::from_uri(url);

        let service = tokio::time::timeout(Duration::from_secs(timeout_secs), ().serve(transport))
            .await
            .map_err(|_| {
                Error::Mcp(format!(
                    "MCP server '{name}' HTTP handshake timed out after {timeout_secs}s"
                ))
            })?
            .map_err(|e| Error::Mcp(format!("MCP server '{name}' HTTP handshake failed: {e}")))?;

        let mcp_tools = service
            .list_all_tools()
            .await
            .map_err(|e| Error::Mcp(format!("failed to list tools from '{name}': {e}")))?;

        let tools: Vec<McpToolInfo> = mcp_tools
            .into_iter()
            .map(|t| McpToolInfo {
                name: t.name.to_string(),
                description: t.description.map(|d| d.to_string()),
                input_schema: serde_json::to_value(&*t.input_schema).unwrap_or_default(),
            })
            .collect();

        info!(
            "MCP server '{name}' connected via HTTP: {} tool(s) discovered",
            tools.len()
        );

        let conn = McpConnection {
            server_name: name.to_string(),
            service,
            tools,
            params: ConnectionParams::Http {
                url: url.to_string(),
                timeout_secs,
            },
            allowed_tools,
        };

        self.connections
            .write()
            .await
            .insert(name.to_string(), conn);

        // GAR-293: reset restart state on successful HTTP connect.
        self.restart_states
            .write()
            .await
            .entry(name.to_string())
            .and_modify(|s| s.reset())
            .or_insert_with(|| RestartState::new(max_restarts, restart_delay_secs));

        Ok(())
    }

    /// GAR-293: Reset the restart counter for a server (called on manual admin restart).
    pub async fn reset_restart_state(&self, name: &str) {
        if let Some(state) = self.restart_states.write().await.get_mut(name) {
            state.reset();
            info!("MCP server '{name}' restart counter reset (manual restart)");
        }
    }

    /// Disconnect a specific MCP server.
    pub async fn disconnect(&self, name: &str) {
        if let Some(conn) = self.connections.write().await.remove(name) {
            info!("disconnecting MCP server '{name}'");
            if let Err(e) = conn.service.cancel().await {
                warn!("error cancelling MCP server '{name}': {e}");
            }
        }
    }

    /// Disconnect all MCP servers.
    pub async fn disconnect_all(&self) {
        let conns: HashMap<String, McpConnection> =
            std::mem::take(&mut *self.connections.write().await);
        for (name, conn) in conns {
            info!("disconnecting MCP server '{name}'");
            if let Err(e) = conn.service.cancel().await {
                warn!("error cancelling MCP server '{name}': {e}");
            }
        }
    }

    /// Create `Tool` trait objects for all tools from a specific server.
    ///
    /// GAR-190: If the connection has a non-empty `allowed_tools` list, only tools
    /// whose names appear in that list are returned. Unknown names in the allowlist
    /// are silently ignored (the tool simply wasn't discovered by this server).
    pub async fn take_tools(&self, name: &str, timeout: Duration) -> Vec<Box<dyn Tool>> {
        let conns = self.connections.read().await;
        let Some(conn) = conns.get(name) else {
            return Vec::new();
        };

        let peer: Arc<Peer<RoleClient>> = Arc::new(conn.service.peer().clone());

        conn.tools
            .iter()
            .filter(|t| {
                conn.allowed_tools.is_empty() || conn.allowed_tools.contains(&t.name)
            })
            .map(|t| {
                Box::new(McpTool::new(
                    &conn.server_name,
                    t.name.clone(),
                    t.description.clone(),
                    t.input_schema.clone(),
                    Arc::clone(&peer),
                    timeout,
                )) as Box<dyn Tool>
            })
            .collect()
    }

    /// List all connected servers with their tool counts.
    pub async fn list_servers(&self) -> Vec<(String, usize, bool)> {
        let conns = self.connections.read().await;
        conns
            .iter()
            .map(|(name, conn)| (name.clone(), conn.tools.len(), !conn.service.is_closed()))
            .collect()
    }

    /// Get tool info for a specific server.
    pub async fn tool_info(&self, name: &str) -> Vec<McpToolInfo> {
        let conns = self.connections.read().await;
        conns.get(name).map(|c| c.tools.clone()).unwrap_or_default()
    }

    /// List resources from a specific MCP server.
    pub async fn list_resources(&self, name: &str) -> Result<Vec<McpResourceInfo>> {
        let conns = self.connections.read().await;
        let conn = conns
            .get(name)
            .ok_or_else(|| Error::Mcp(format!("MCP server '{name}' not connected")))?;

        let resources = conn
            .service
            .list_all_resources()
            .await
            .map_err(|e| Error::Mcp(format!("failed to list resources from '{name}': {e}")))?;

        Ok(resources
            .into_iter()
            .map(|r| McpResourceInfo {
                uri: r.uri.to_string(),
                name: r.name.to_string(),
                description: r.description.as_deref().map(|d| d.to_string()),
                mime_type: r.mime_type.as_deref().map(|m| m.to_string()),
            })
            .collect())
    }

    /// Read a specific resource from an MCP server.
    pub async fn read_resource(&self, name: &str, uri: &str) -> Result<String> {
        let conns = self.connections.read().await;
        let conn = conns
            .get(name)
            .ok_or_else(|| Error::Mcp(format!("MCP server '{name}' not connected")))?;

        let params = rmcp::model::ReadResourceRequestParams {
            meta: None,
            uri: uri.to_string(),
        };

        let result = conn.service.read_resource(params).await.map_err(|e| {
            Error::Mcp(format!(
                "failed to read resource '{uri}' from '{name}': {e}"
            ))
        })?;

        let text_parts: Vec<String> = result
            .contents
            .into_iter()
            .filter_map(|c| match c {
                rmcp::model::ResourceContents::TextResourceContents { text, .. } => Some(text),
                _ => None,
            })
            .collect();

        Ok(text_parts.join("\n"))
    }

    /// List prompts from a specific MCP server.
    pub async fn list_prompts(&self, name: &str) -> Result<Vec<McpPromptInfo>> {
        let conns = self.connections.read().await;
        let conn = conns
            .get(name)
            .ok_or_else(|| Error::Mcp(format!("MCP server '{name}' not connected")))?;

        let prompts = conn
            .service
            .list_all_prompts()
            .await
            .map_err(|e| Error::Mcp(format!("failed to list prompts from '{name}': {e}")))?;

        Ok(prompts
            .into_iter()
            .map(|p| McpPromptInfo {
                name: p.name.to_string(),
                description: p.description.map(|d| d.to_string()),
                arguments: p
                    .arguments
                    .unwrap_or_default()
                    .into_iter()
                    .map(|a| McpPromptArgument {
                        name: a.name.to_string(),
                        description: a.description.map(|d| d.to_string()),
                        required: a.required.unwrap_or(false),
                    })
                    .collect(),
            })
            .collect())
    }

    /// Get a specific prompt with arguments from an MCP server.
    pub async fn get_prompt(
        &self,
        name: &str,
        prompt_name: &str,
        args: Option<serde_json::Map<String, serde_json::Value>>,
    ) -> Result<Vec<String>> {
        let conns = self.connections.read().await;
        let conn = conns
            .get(name)
            .ok_or_else(|| Error::Mcp(format!("MCP server '{name}' not connected")))?;

        let params = rmcp::model::GetPromptRequestParams {
            meta: None,
            name: prompt_name.to_string(),
            arguments: args,
        };

        let result = conn.service.get_prompt(params).await.map_err(|e| {
            Error::Mcp(format!(
                "failed to get prompt '{prompt_name}' from '{name}': {e}"
            ))
        })?;

        let messages: Vec<String> = result
            .messages
            .into_iter()
            .map(|m| {
                let role = match m.role {
                    rmcp::model::PromptMessageRole::User => "user",
                    rmcp::model::PromptMessageRole::Assistant => "assistant",
                };
                let text = match m.content {
                    rmcp::model::PromptMessageContent::Text { text } => text,
                    _ => "(non-text content)".to_string(),
                };
                format!("[{role}] {text}")
            })
            .collect();

        Ok(messages)
    }

    /// List all prompts from all connected MCP servers.
    ///
    /// Silently skips servers that don't support the prompts capability or return errors.
    /// Returns `(server_name, prompts)` pairs, omitting servers with no prompts.
    pub async fn list_all_prompts(&self) -> Vec<(String, Vec<McpPromptInfo>)> {
        let server_names: Vec<String> = {
            let conns = self.connections.read().await;
            conns.keys().cloned().collect()
        };

        let mut result = Vec::new();
        for name in server_names {
            match self.list_prompts(&name).await {
                Ok(prompts) if !prompts.is_empty() => result.push((name, prompts)),
                _ => {}
            }
        }
        result
    }

    /// Spawn a background health monitor that pings servers and reconnects on failure.
    pub fn spawn_health_monitor(self: &Arc<Self>) {
        let manager = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            loop {
                interval.tick().await;
                manager.check_and_reconnect().await;
            }
        });
    }

    /// Call a specific tool on a specific MCP server.
    ///
    /// # Arguments
    /// * `server_name` - Name of the MCP server
    /// * `tool_name` - Name of the tool to call
    /// * `arguments` - Arguments to pass to the tool
    ///
    /// # Returns
    /// The tool's output as a string, or an error
    pub async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: std::collections::HashMap<String, serde_json::Value>,
    ) -> std::result::Result<String, String> {
        use crate::tools::{Tool, ToolContext};

        let conns = self.connections.read().await;
        let conn = match conns.get(server_name) {
            Some(c) => c,
            None => return Err(format!("MCP server '{}' not found", server_name)),
        };

        // Find the tool in the connection's tool list
        let tool_info = match conn
            .tools
            .iter()
            .find(|t| t.name == tool_name || t.name == format!("{}.{}", server_name, tool_name))
        {
            Some(t) => t,
            None => {
                return Err(format!(
                    "Tool '{}' not found on server '{}'",
                    tool_name, server_name
                ));
            }
        };

        // Create a peer reference
        let peer: Arc<Peer<RoleClient>> = Arc::new(conn.service.peer().clone());

        // Create the tool
        let tool = McpTool::new(
            server_name,
            tool_info.name.clone(),
            tool_info.description.clone(),
            tool_info.input_schema.clone(),
            peer,
            Duration::from_secs(60),
        );

        // Execute the tool
        let context = ToolContext {
            session_id: "mcp_command".to_string(),
            user_id: None,
            is_heartbeat: false,
            is_confirmation_approved: false,
        };

        let input = serde_json::Value::Object(arguments.into_iter().collect());
        let output = tool
            .execute(&context, input)
            .await
            .map_err(|e| e.to_string())?;

        Ok(output.content)
    }

    /// GAR-293: Check all connections and attempt reconnect with exponential backoff.
    async fn check_and_reconnect(&self) {
        let to_reconnect: Vec<(String, ConnectionParams, Vec<String>)> = {
            let conns = self.connections.read().await;
            conns
                .iter()
                .filter(|(_, conn)| conn.service.is_closed())
                .map(|(name, conn)| (name.clone(), conn.params.clone(), conn.allowed_tools.clone()))
                .collect()
        };

        for (name, params, allowed_tools) in to_reconnect {
            // Check restart state before attempting reconnect.
            let (should_retry, attempt_num, max_restarts) = {
                let mut states = self.restart_states.write().await;
                let state = states.entry(name.clone()).or_insert_with(|| {
                    RestartState::new(5, 5) // safe defaults if missing
                });

                if state.is_exhausted() {
                    error!(
                        "MCP server '{name}' has crashed {} time(s) — max restarts ({}) reached. \
                         Use the admin API to restart manually.",
                        state.count, state.max_restarts
                    );
                    (false, state.count, state.max_restarts)
                } else if !state.should_retry_now() {
                    let delay = state.current_delay_secs();
                    info!(
                        "MCP server '{name}' waiting for backoff delay ({delay}s) before retry \
                         (attempt {}/{})",
                        state.count + 1, state.max_restarts
                    );
                    (false, state.count, state.max_restarts)
                } else {
                    let attempt = state.count + 1;
                    let max = state.max_restarts;
                    state.record_attempt();
                    (true, attempt, max)
                }
            };

            if !should_retry {
                continue;
            }

            info!(
                "MCP server '{name}' connection lost — restart attempt {attempt_num}/{max_restarts}"
            );

            // Remove stale connection before reconnecting.
            self.connections.write().await.remove(&name);

            let result = match &params {
                ConnectionParams::Stdio {
                    command,
                    args,
                    env,
                    timeout_secs,
                    memory_limit_mb,
                } => {
                    // Fetch max_restarts / restart_delay from saved state.
                    let (mr, rd) = {
                        let states = self.restart_states.read().await;
                        states.get(&name).map(|s| (s.max_restarts, s.base_delay_secs)).unwrap_or((5, 5))
                    };
                    self.connect(&name, command, args, env, *timeout_secs, allowed_tools, *memory_limit_mb, mr, rd).await
                }
                #[cfg(feature = "mcp-http")]
                ConnectionParams::Http { url, timeout_secs } => {
                    let (mr, rd) = {
                        let states = self.restart_states.read().await;
                        states.get(&name).map(|s| (s.max_restarts, s.base_delay_secs)).unwrap_or((5, 5))
                    };
                    self.connect_http(&name, url, *timeout_secs, allowed_tools, mr, rd).await
                }
            };

            match result {
                Ok(()) => {
                    info!("MCP server '{name}' reconnected successfully (attempt {attempt_num})");
                    // reset() is called inside connect() on success.
                }
                Err(e) => {
                    warn!("MCP server '{name}' reconnect attempt {attempt_num} failed: {e}");
                }
            }
        }
    }
}

/// GAR-293: Apply a virtual-memory limit to a child process (Unix only).
///
/// Uses `setrlimit(RLIMIT_AS, limit_mb * 1024 * 1024)` before exec.
/// If the process exceeds the limit the kernel delivers SIGSEGV / ENOMEM.
#[cfg(unix)]
fn apply_memory_limit(cmd: &mut Command, limit_mb: u64) {
    let limit_bytes = limit_mb.saturating_mul(1024 * 1024);
    // SAFETY: `setrlimit` is async-signal-safe and only affects the child.
    unsafe {
        cmd.pre_exec(move || {
            let rlim = libc::rlimit {
                rlim_cur: limit_bytes,
                rlim_max: limit_bytes,
            };
            // Ignore errors — we don't want the spawn to fail just because
            // the limit couldn't be set (e.g. already above hard limit).
            let _ = libc::setrlimit(libc::RLIMIT_AS, &rlim);
            Ok(())
        });
    }
}
