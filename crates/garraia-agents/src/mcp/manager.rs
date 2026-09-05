use std::collections::HashMap;
use std::path::Path;
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

/// Vet an MCP server URL before dialling it.
///
/// `IpScope::AllowPrivate` is deliberate and load-bearing: MCP servers are
/// routinely self-hosted on `http://127.0.0.1:3000` or on the LAN, so blocking
/// private ranges would break the ordinary case. What this does remove is the
/// part that is never a legitimate MCP endpoint — every non-HTTP scheme
/// (`file:`, `gopher:`), link-local (`169.254.169.254`, cloud instance
/// metadata), CGNAT, multicast and the unspecified address.
/// Fetch policy for an MCP server URL.
///
/// `AllowPrivate` is deliberate and load-bearing: MCP servers are routinely
/// self-hosted on `http://127.0.0.1:3000` or on the LAN, so blocking private
/// ranges would break the ordinary case. What it does remove is what is never a
/// legitimate MCP endpoint — every non-HTTP scheme (`file:`, `gopher:`),
/// link-local (`169.254.169.254`, cloud instance metadata), NAT64-embedded
/// versions of those, CGNAT, multicast and the unspecified address.
///
fn mcp_url_policy() -> garraia_common::ssrf::UrlPolicy {
    garraia_common::ssrf::UrlPolicy::http_public(
        Duration::from_secs(30),
        concat!("GarraIA/", env!("CARGO_PKG_VERSION"), " mcp-client"),
    )
    .with_ip_scope(garraia_common::ssrf::IpScope::AllowPrivate)
}

/// Vet an MCP server URL.
///
/// # Known gap: no DNS pinning here
///
/// Every other call site of the shared guard connects through
/// [`garraia_common::ssrf::pinned_client`], which fixes the vetted addresses
/// into the client so `.send()` cannot re-resolve the host — closing the
/// DNS-rebinding window. This one cannot, today: `rmcp` resolves to
/// **reqwest 0.13** while the workspace is on **0.12**, so the two
/// `reqwest::Client` types are unrelated and `StreamableHttpClientTransport::
/// with_client` will not accept ours. The scheme gate and the IP block below
/// still apply; what remains open is an attacker who controls the resolver for
/// a registered MCP host and can flip the answer between this check and the
/// transport\'s own connect.
///
/// Registering an MCP server is an operator action behind admin auth, so the
/// practical exposure is small — but the asymmetry is real and should close
/// when the workspace moves to reqwest 0.13.
// Always compiled, so the tests below run in the default feature set even
// though the only production caller is behind `mcp-http`.
#[cfg_attr(not(feature = "mcp-http"), allow(dead_code))]
pub fn validate_mcp_url(
    url: &str,
) -> std::result::Result<garraia_common::ssrf::VettedUrl, garraia_common::ssrf::SsrfRejection> {
    garraia_common::ssrf::vet_url(url, &mcp_url_policy())
}

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

/// How long a connection must stay alive before its restart counter is
/// cleared. Without this, resetting on every successful handshake let a
/// crash-looping server restart forever.
const STABILITY_WINDOW: Duration = Duration::from_secs(60);

impl ConnectionParams {
    fn timeout_secs(&self) -> u64 {
        match self {
            ConnectionParams::Stdio { timeout_secs, .. } => *timeout_secs,
            #[cfg(feature = "mcp-http")]
            ConnectionParams::Http { timeout_secs, .. } => *timeout_secs,
        }
    }
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
    /// When this connection was established. Used to decide whether it lived
    /// long enough to count as stable (see `STABILITY_WINDOW`).
    connected_at: Instant,
}

impl McpConnection {
    /// Liveness of the child/transport.
    ///
    /// `RunningService::is_closed()` is NOT usable here: rmcp only flips it in
    /// `close()`/`cancel()`/`waiting()`, so it stays `false` forever when the
    /// child process dies on its own (the serve loop exits with
    /// `QuitReason::Closed` without cancelling the token). That made
    /// `check_and_reconnect` a no-op and reported dead servers as `Running`.
    /// The peer's transport channel is the signal that actually closes when
    /// the serve loop ends.
    fn is_alive(&self) -> bool {
        !self.service.peer().is_transport_closed()
    }
}

/// Manages the lifecycle of MCP server connections.
pub struct McpManager {
    connections: Arc<RwLock<HashMap<String, McpConnection>>>,
    /// GAR-293: per-server restart state (survives connection removal).
    restart_states: Arc<RwLock<HashMap<String, RestartState>>>,
    /// Servers that failed to connect at boot. They never entered
    /// `connections`, so `check_and_reconnect` (which iterates connections)
    /// could never see them and only a manual admin restart recovered them.
    pending: Arc<RwLock<HashMap<String, PendingServer>>>,
}

/// `(name, params, allowed_tools)` for one server needing a (re)connect.
type ReconnectTarget = (String, ConnectionParams, Vec<String>);

/// A configured-but-not-connected server awaiting a retry.
#[derive(Clone)]
struct PendingServer {
    params: ConnectionParams,
    allowed_tools: Vec<String>,
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
            pending: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Record a stdio server that failed to connect at boot so the health
    /// monitor retries it with the same backoff as a crashed connection.
    ///
    /// Note: a server that never completed a handshake has no tool schemas,
    /// so a later successful retry restores slash-commands and the admin API
    /// but not the LLM tool list — `AgentRuntime` is immutable after boot.
    #[allow(clippy::too_many_arguments)]
    pub async fn register_pending_stdio(
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
    ) {
        self.pending.write().await.insert(
            name.to_string(),
            PendingServer {
                params: ConnectionParams::Stdio {
                    command: command.to_string(),
                    args: args.to_vec(),
                    env: env.clone(),
                    timeout_secs,
                    memory_limit_mb,
                },
                allowed_tools,
            },
        );
        // The retry loop reads max_restarts/backoff from here.
        self.restart_states
            .write()
            .await
            .entry(name.to_string())
            .or_insert_with(|| RestartState::new(max_restarts, restart_delay_secs));
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
        // On Windows, script wrappers like `npx`, `uvx`, `yarn`, etc. are `.cmd`
        // files that cannot be spawned directly by CreateProcess. We wrap them in
        // `cmd /c <command> [args...]` so the shell resolves the extension.
        #[cfg(windows)]
        let mut cmd = {
            let needs_shell =
                !command.ends_with(".exe") && !std::path::Path::new(command).is_absolute();
            if needs_shell {
                let mut c = Command::new("cmd");
                c.args(["/c", command]).args(args);
                c
            } else {
                let mut c = Command::new(command);
                c.args(args);
                c
            }
        };
        #[cfg(not(windows))]
        let mut cmd = {
            let mut c = Command::new(command);
            c.args(args);
            c
        };

        // Termux (issues #909/#913): on Android an ELF exec goes through the
        // termux-exec shim, and a host that spawns the gateway with a filtered
        // environment strips `LD_PRELOAD` — after which every MCP child that is
        // an npm/pip script dies on its `/usr/bin/...` shebang. Injected before
        // the config overlay below so an explicit `env.LD_PRELOAD` still wins.
        #[cfg(target_os = "android")]
        if let Some(preload) = termux_ld_preload(
            env,
            std::env::var("PREFIX").ok().as_deref(),
            std::env::var("LD_PRELOAD").ok().as_deref(),
            |path| path.exists(),
        ) {
            tracing::debug!(server = %name, "Termux: injecting LD_PRELOAD (termux-exec) into MCP child");
            cmd.env("LD_PRELOAD", preload);
        }

        for (k, v) in env {
            cmd.env(k, v);
        }

        // GAR-293: apply memory limit on Unix via setrlimit(RLIMIT_AS).
        #[cfg(unix)]
        if let Some(limit_mb) = memory_limit_mb {
            apply_memory_limit(&mut cmd, limit_mb);
        }

        // Containment: children must not outlive an abruptly-killed gateway.
        #[cfg(any(target_os = "linux", target_os = "android"))]
        apply_parent_death_signal(&mut cmd);

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

        // Discover tools. Timeout mirrors the handshake above: a child that
        // spawns but never answers tools/list must not block gateway startup.
        let mcp_tools =
            tokio::time::timeout(Duration::from_secs(timeout_secs), service.list_all_tools())
                .await
                .map_err(|_| {
                    Error::Mcp(format!(
                        "MCP server '{name}' tools/list timed out after {timeout_secs}s"
                    ))
                })?
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
            connected_at: Instant::now(),
        };

        self.connections
            .write()
            .await
            .insert(name.to_string(), conn);
        self.pending.write().await.remove(name);

        // GAR-293: the restart counter is NOT reset on successful connect. A server
        // that handshakes and then dies seconds later would clear it every
        // cycle, making `max_restarts` unreachable and the crash loop
        // infinite. The reset happens in `check_and_reconnect` once the
        // connection survives `STABILITY_WINDOW`.
        self.restart_states
            .write()
            .await
            .entry(name.to_string())
            .or_insert_with(|| RestartState::new(max_restarts, restart_delay_secs));

        Ok(())
    }

    /// Connect to an MCP server via HTTP (Streamable HTTP transport).
    ///
    /// The URL is vetted here, at the terminal sink, rather than at each call
    /// site: three paths reach this function — `admin_create_mcp` +
    /// `/admin/api/mcp/{id}/restart`, the boot-time connect in
    /// `bootstrap::mod`, and the background reconnect loop below — and the URL
    /// is *stored* in `mcp.json` between them, so a value can be edited out of
    /// band after any front-door check. Validating here covers all three and
    /// re-validates on every reconnect. CodeQL: `rust/request-forgery` (9.1).
    #[cfg(feature = "mcp-http")]
    pub async fn connect_http(
        &self,
        name: &str,
        url: &str,
        timeout_secs: u64,
        allowed_tools: Vec<String>,
        max_restarts: u32,
        restart_delay_secs: u64,
    ) -> Result<()> {
        use rmcp::transport::StreamableHttpClientTransport;

        // Scheme gate + IP block. NOT pinned — see `validate_mcp_url` for why
        // the reqwest version split prevents it here.
        let vetted = validate_mcp_url(url)
            .map_err(|e| Error::Mcp(format!("MCP server '{name}' has an unusable url: {e}")))?;

        let transport = StreamableHttpClientTransport::from_uri(vetted.url.as_str());

        let service = tokio::time::timeout(Duration::from_secs(timeout_secs), ().serve(transport))
            .await
            .map_err(|_| {
                Error::Mcp(format!(
                    "MCP server '{name}' HTTP handshake timed out after {timeout_secs}s"
                ))
            })?
            .map_err(|e| Error::Mcp(format!("MCP server '{name}' HTTP handshake failed: {e}")))?;

        // Same tools/list timeout as the stdio path above.
        let mcp_tools =
            tokio::time::timeout(Duration::from_secs(timeout_secs), service.list_all_tools())
                .await
                .map_err(|_| {
                    Error::Mcp(format!(
                        "MCP server '{name}' tools/list timed out after {timeout_secs}s"
                    ))
                })?
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
            connected_at: Instant::now(),
        };

        self.connections
            .write()
            .await
            .insert(name.to_string(), conn);
        self.pending.write().await.remove(name);

        // GAR-293: the restart counter is NOT reset on successful HTTP connect. A server
        // that handshakes and then dies seconds later would clear it every
        // cycle, making `max_restarts` unreachable and the crash loop
        // infinite. The reset happens in `check_and_reconnect` once the
        // connection survives `STABILITY_WINDOW`.
        self.restart_states
            .write()
            .await
            .entry(name.to_string())
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

    /// Disconnect all MCP servers, concurrently.
    ///
    /// Sequential cancellation made shutdown cost the sum of every server's
    /// drain time; one slow server delayed all the others. The caller is
    /// still expected to bound this with a timeout.
    pub async fn disconnect_all(&self) {
        let conns: HashMap<String, McpConnection> =
            std::mem::take(&mut *self.connections.write().await);
        self.pending.write().await.clear();
        let cancels = conns.into_iter().map(|(name, conn)| async move {
            info!("disconnecting MCP server '{name}'");
            if let Err(e) = conn.service.cancel().await {
                warn!("error cancelling MCP server '{name}': {e}");
            }
        });
        futures::future::join_all(cancels).await;
    }

    /// Clone the current peer for a server, releasing the connections lock
    /// before the caller awaits anything on it.
    ///
    /// Every RPC path must resolve the peer through here: a reconnect swaps
    /// the whole `McpConnection`, so any `Peer` captured earlier talks to a
    /// dead transport.
    pub(crate) async fn peer_for(&self, name: &str) -> Option<Peer<RoleClient>> {
        let conns = self.connections.read().await;
        let conn = conns.get(name)?;
        if !conn.is_alive() {
            return None;
        }
        Some(conn.service.peer().clone())
    }

    /// Like [`Self::peer_for`], but also returns the server's configured
    /// timeout and reports "not connected" as an error. Used by the
    /// resource/prompt RPCs, which previously awaited with no timeout at all
    /// while holding the connections read lock.
    async fn peer_and_timeout(&self, name: &str) -> Result<(Peer<RoleClient>, Duration)> {
        let conns = self.connections.read().await;
        let conn = conns
            .get(name)
            .ok_or_else(|| Error::Mcp(format!("MCP server '{name}' not connected")))?;
        Ok((
            conn.service.peer().clone(),
            Duration::from_secs(conn.params.timeout_secs()),
        ))
    }

    /// Create `Tool` trait objects for all tools from a specific server.
    ///
    /// GAR-190: If the connection has a non-empty `allowed_tools` list, only tools
    /// whose names appear in that list are returned. Unknown names in the allowlist
    /// are silently ignored (the tool simply wasn't discovered by this server).
    pub async fn take_tools(self: &Arc<Self>, name: &str, timeout: Duration) -> Vec<Box<dyn Tool>> {
        let conns = self.connections.read().await;
        let Some(conn) = conns.get(name) else {
            return Vec::new();
        };

        conn.tools
            .iter()
            .filter(|t| is_tool_allowed(&conn.allowed_tools, &t.name))
            .map(|t| {
                Box::new(McpTool::new(
                    Arc::clone(self),
                    &conn.server_name,
                    t.name.clone(),
                    t.description.clone(),
                    t.input_schema.clone(),
                    timeout,
                )) as Box<dyn Tool>
            })
            .collect()
    }

    /// Build `Tool` trait objects for **every** connected server, each with its
    /// own configured timeout.
    ///
    /// Issue #924: `take_tools` needs a timeout the caller has to know, which
    /// is why the boot path was the only place that ever called it — nothing
    /// else had the per-server config at hand. This reads the timeout from the
    /// live connection, so any caller can resync without carrying config
    /// around. Together with `AgentRuntime::replace_mcp_tools` it makes the
    /// manager the single source of truth for the MCP half of the tool list.
    pub async fn tools_by_server(self: &Arc<Self>) -> Vec<(String, Vec<Box<dyn Tool>>)> {
        let names: Vec<(String, Duration)> = {
            let conns = self.connections.read().await;
            conns
                .iter()
                .map(|(name, conn)| {
                    (
                        name.clone(),
                        Duration::from_secs(conn.params.timeout_secs()),
                    )
                })
                .collect()
        };

        let mut out = Vec::with_capacity(names.len());
        for (name, timeout) in names {
            out.push((name.clone(), self.take_tools(&name, timeout).await));
        }
        out
    }

    /// List all connected servers with their tool counts.
    pub async fn list_servers(&self) -> Vec<(String, usize, bool)> {
        let conns = self.connections.read().await;
        conns
            .iter()
            .map(|(name, conn)| (name.clone(), conn.tools.len(), conn.is_alive()))
            .collect()
    }

    /// Get tool info for a specific server.
    pub async fn tool_info(&self, name: &str) -> Vec<McpToolInfo> {
        let conns = self.connections.read().await;
        conns.get(name).map(|c| c.tools.clone()).unwrap_or_default()
    }

    /// List resources from a specific MCP server.
    pub async fn list_resources(&self, name: &str) -> Result<Vec<McpResourceInfo>> {
        let (peer, timeout) = self.peer_and_timeout(name).await?;

        let resources = tokio::time::timeout(timeout, peer.list_all_resources())
            .await
            .map_err(|_| {
                Error::Mcp(format!(
                    "MCP server '{name}' resources/list timed out after {timeout:?}"
                ))
            })?
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
        let (peer, timeout) = self.peer_and_timeout(name).await?;

        let params = rmcp::model::ReadResourceRequestParams::new(uri.to_string());

        let result = tokio::time::timeout(timeout, peer.read_resource(params))
            .await
            .map_err(|_| {
                Error::Mcp(format!(
                    "MCP server '{name}' resources/read timed out after {timeout:?}"
                ))
            })?
            .map_err(|e| {
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
        let (peer, timeout) = self.peer_and_timeout(name).await?;

        let prompts = tokio::time::timeout(timeout, peer.list_all_prompts())
            .await
            .map_err(|_| {
                Error::Mcp(format!(
                    "MCP server '{name}' prompts/list timed out after {timeout:?}"
                ))
            })?
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
        let (peer, timeout) = self.peer_and_timeout(name).await?;

        let mut params = rmcp::model::GetPromptRequestParams::new(prompt_name.to_string());
        if let Some(a) = args {
            params = params.with_arguments(a);
        }

        let result = tokio::time::timeout(timeout, peer.get_prompt(params))
            .await
            .map_err(|_| {
                Error::Mcp(format!(
                    "MCP server '{name}' prompts/get timed out after {timeout:?}"
                ))
            })?
            .map_err(|e| {
                Error::Mcp(format!(
                    "failed to get prompt '{prompt_name}' from '{name}': {e}"
                ))
            })?;

        let messages: Vec<String> = result
            .messages
            .into_iter()
            .map(|m| {
                // rmcp 2.2 (spec MCP 2025-11-25) dissolveu os enums específicos
                // de prompt: `PromptMessageRole` virou o `Role` compartilhado e
                // `PromptMessageContent` virou o `ContentBlock` unificado.
                // `Role` é intencionalmente exaustivo (User/Assistant), então
                // não leva braço curinga; `ContentBlock` é `#[non_exhaustive]`
                // e leva.
                let role = match m.role {
                    rmcp::model::Role::User => "user",
                    rmcp::model::Role::Assistant => "assistant",
                };
                let text = match m.content {
                    rmcp::model::ContentBlock::Text(text_content) => text_content.text,
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

    /// Spawn a background health monitor that detects dead transports and
    /// reconnects with exponential backoff. It does not send MCP `ping`s —
    /// liveness comes from `McpConnection::is_alive`.
    pub fn spawn_health_monitor(self: &Arc<Self>) {
        self.spawn_health_monitor_with_runtime(None);
    }

    /// Health monitor that also keeps an `AgentRuntime`'s MCP tool inventory
    /// in sync after every reconnect pass (issue #924).
    ///
    /// Reconnecting repopulated `connections` and nothing else: the runtime's
    /// tool list was written once at boot and then frozen inside an `Arc`, so
    /// a server that came back had tools the LLM could not see. Passing the
    /// runtime here means recovery is complete instead of half-done.
    pub fn spawn_health_monitor_with_runtime(
        self: &Arc<Self>,
        runtime: Option<Arc<crate::AgentRuntime>>,
    ) {
        let manager = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            // Default Burst would fire catch-up ticks back-to-back if one
            // check ever ran long; we want steady spacing.
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                manager.check_and_reconnect().await;
                if let Some(rt) = runtime.as_ref() {
                    rt.sync_mcp_tools(&manager).await;
                }
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
        self: &Arc<Self>,
        server_name: &str,
        tool_name: &str,
        arguments: std::collections::HashMap<String, serde_json::Value>,
    ) -> std::result::Result<String, String> {
        use crate::tools::{Tool, ToolContext};

        // Resolve everything we need under the guard, then DROP it: the
        // execution below awaits an RPC, and holding `connections.read()`
        // across it lets one slow server block the write-fair lock for the
        // whole subsystem (reconnects included).
        let (tool_info, timeout_secs) = {
            let conns = self.connections.read().await;
            let conn = match conns.get(server_name) {
                Some(c) => c,
                None => return Err(format!("MCP server '{}' not found", server_name)),
            };

            // Find the tool in the connection's tool list
            let tool_info =
                match conn.tools.iter().find(|t| {
                    t.name == tool_name || t.name == format!("{}.{}", server_name, tool_name)
                }) {
                    Some(t) => t.clone(),
                    None => {
                        return Err(format!(
                            "Tool '{}' not found on server '{}'",
                            tool_name, server_name
                        ));
                    }
                };

            // GAR-190: the allowlist must bind this path too — `take_tools`
            // filters what the LLM sees, but call_tool (admin API / slash
            // commands) used to dispatch any discovered tool regardless.
            if !is_tool_allowed(&conn.allowed_tools, &tool_info.name) {
                return Err(format!(
                    "Tool '{}' on server '{}' is blocked by the allowed_tools allowlist",
                    tool_info.name, server_name
                ));
            }

            (tool_info, conn.params.timeout_secs())
        };

        // The tool resolves the current peer itself at execution time.
        let tool = McpTool::new(
            Arc::clone(self),
            server_name,
            tool_info.name.clone(),
            tool_info.description.clone(),
            tool_info.input_schema.clone(),
            Duration::from_secs(timeout_secs),
        );

        // Execute the tool
        let context = ToolContext {
            session_id: "mcp_command".to_string(),
            user_id: None,
            is_heartbeat: false,
            is_confirmation_approved: false,
            working_dir: None,
            project_id: None,
        };

        let input = serde_json::Value::Object(arguments.into_iter().collect());
        let output = tool
            .execute(&context, input)
            .await
            .map_err(|e| e.to_string())?;

        Ok(output.content)
    }

    /// Run one health-monitor pass immediately.
    ///
    /// Exposed so lifecycle tests can drive the sweep deterministically
    /// instead of sleeping through the 30s interval.
    pub async fn health_tick(&self) {
        self.check_and_reconnect().await;
    }

    /// Whether a server currently has a live transport.
    pub async fn is_connected(&self, name: &str) -> bool {
        self.connections
            .read()
            .await
            .get(name)
            .map(|c| c.is_alive())
            .unwrap_or(false)
    }

    /// GAR-293: Check all connections and attempt reconnect with exponential backoff.
    async fn check_and_reconnect(&self) {
        let (to_reconnect, stable): (Vec<ReconnectTarget>, Vec<String>) = {
            let conns = self.connections.read().await;
            let dead = conns
                .iter()
                .filter(|(_, conn)| !conn.is_alive())
                .map(|(name, conn)| {
                    (
                        name.clone(),
                        conn.params.clone(),
                        conn.allowed_tools.clone(),
                    )
                })
                .collect();
            let stable = conns
                .iter()
                .filter(|(_, conn)| {
                    conn.is_alive() && conn.connected_at.elapsed() >= STABILITY_WINDOW
                })
                .map(|(name, _)| name.clone())
                .collect();
            (dead, stable)
        };

        // Boot failures live in `pending`, never in `connections`.
        let mut to_reconnect: Vec<ReconnectTarget> = to_reconnect;
        {
            let pending = self.pending.read().await;
            for (name, p) in pending.iter() {
                to_reconnect.push((name.clone(), p.params.clone(), p.allowed_tools.clone()));
            }
        }

        // Only a connection that actually stayed up counts as a recovery.
        if !stable.is_empty() {
            let mut states = self.restart_states.write().await;
            for name in stable {
                if let Some(state) = states.get_mut(&name)
                    && state.count > 0
                {
                    info!(
                        "MCP server '{name}' stable for {}s — restart counter reset",
                        STABILITY_WINDOW.as_secs()
                    );
                    state.reset();
                }
            }
        }

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
                        state.count + 1,
                        state.max_restarts
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
                        states
                            .get(&name)
                            .map(|s| (s.max_restarts, s.base_delay_secs))
                            .unwrap_or((5, 5))
                    };
                    self.connect(
                        &name,
                        command,
                        args,
                        env,
                        *timeout_secs,
                        allowed_tools,
                        *memory_limit_mb,
                        mr,
                        rd,
                    )
                    .await
                }
                #[cfg(feature = "mcp-http")]
                ConnectionParams::Http { url, timeout_secs } => {
                    let (mr, rd) = {
                        let states = self.restart_states.read().await;
                        states
                            .get(&name)
                            .map(|s| (s.max_restarts, s.base_delay_secs))
                            .unwrap_or((5, 5))
                    };
                    self.connect_http(&name, url, *timeout_secs, allowed_tools, mr, rd)
                        .await
                }
            };

            match result {
                Ok(()) => {
                    info!("MCP server '{name}' reconnected successfully (attempt {attempt_num})");
                    // The counter is cleared only after STABILITY_WINDOW, at
                    // the top of a later tick — a handshake alone is not
                    // evidence that the server stopped crash-looping.
                }
                Err(e) => {
                    warn!("MCP server '{name}' reconnect attempt {attempt_num} failed: {e}");
                }
            }
        }
    }
}

/// GAR-190: single source of truth for the `allowed_tools` allowlist,
/// shared by `take_tools` (LLM registration) and `call_tool` (admin API /
/// slash commands). Empty allowlist = every discovered tool is allowed.
fn is_tool_allowed(allowed: &[String], tool_name: &str) -> bool {
    allowed.is_empty() || allowed.iter().any(|t| t == tool_name)
}

/// Path of the termux-exec shim, relative to `$PREFIX`.
///
/// Only ever read on Android; kept out of `cfg` so the decision function below
/// stays compilable — and therefore testable — on every host CI runs on.
const TERMUX_EXEC_LIB: &str = "lib/libtermux-exec.so";

/// Decide the `LD_PRELOAD` an MCP child needs to exec inside Termux.
///
/// On Android an ELF exec only resolves correctly through the termux-exec
/// shim. A host that spawns the gateway with a filtered environment (`env -i
/// PATH=… HOME=…`, which is good security hygiene) drops `LD_PRELOAD`, and
/// from there every MCP child that is an npm/pip script fails on its
/// `/usr/bin/env node` shebang — the path Termux does not have (issue #913).
///
/// Pure on purpose: every input is a parameter, including the existence probe,
/// so the whole decision table is asserted in unit tests on a Linux runner.
///
/// Returns `None` — i.e. changes nothing — whenever:
/// * the server's own config sets `LD_PRELOAD` (the operator always wins), or
/// * the gateway process already inherited a non-empty `LD_PRELOAD`, or
/// * `$PREFIX` does not look like a Termux sandbox, or
/// * the shim is not actually installed (`pkg install termux-exec`).
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn termux_ld_preload<F>(
    server_env: &HashMap<String, String>,
    prefix: Option<&str>,
    inherited_ld_preload: Option<&str>,
    lib_exists: F,
) -> Option<String>
where
    F: Fn(&Path) -> bool,
{
    if server_env.contains_key("LD_PRELOAD") {
        return None;
    }
    if inherited_ld_preload.is_some_and(|v| !v.is_empty()) {
        return None;
    }
    // Same heuristic as `install.sh:detect_platform` and
    // `garraia-cli::doctor::detect_termux` — kept in lockstep deliberately.
    let prefix = prefix.filter(|p| p.contains("com.termux"))?;
    let lib = Path::new(prefix).join(TERMUX_EXEC_LIB);
    lib_exists(&lib).then(|| lib.to_string_lossy().into_owned())
}

/// Ask the kernel to signal the child when its parent (the gateway) dies.
///
/// Without this, `kill -9` on the gateway — or any exit that skips the
/// graceful shutdown path — leaves every MCP child running with no parent to
/// reap it. Other platforms rely on the shutdown path.
///
/// Android is listed explicitly because `target_os = "android"` is NOT covered
/// by `target_os = "linux"` in Rust, even though bionic exposes the same
/// `prctl(PR_SET_PDEATHSIG)`. Without the extra arm every MCP child spawned
/// inside Termux was orphaned when the gateway died (issue #913).
#[cfg(any(target_os = "linux", target_os = "android"))]
fn apply_parent_death_signal(cmd: &mut Command) {
    // SAFETY: `prctl` is async-signal-safe and only affects the child.
    unsafe {
        cmd.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
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

#[cfg(test)]
mod tests {
    use super::{TERMUX_EXEC_LIB, termux_ld_preload};
    use std::collections::HashMap;

    const TERMUX_PREFIX: &str = "/data/data/com.termux/files/usr";

    fn server_env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    /// The whole point of the injection: inside Termux, with the shim
    /// installed and nothing else asking for a preload, MCP children get one.
    #[test]
    fn termux_ld_preload_injects_the_shim_when_nothing_else_set_it() {
        let got = termux_ld_preload(&server_env(&[]), Some(TERMUX_PREFIX), None, |_| true);
        assert_eq!(got, Some(format!("{TERMUX_PREFIX}/{TERMUX_EXEC_LIB}")));
    }

    /// An empty inherited value is not a preload — Termux hosts that export
    /// `LD_PRELOAD=` would otherwise silently opt out of the fix.
    #[test]
    fn termux_ld_preload_treats_an_empty_inherited_value_as_unset() {
        let got = termux_ld_preload(&server_env(&[]), Some(TERMUX_PREFIX), Some(""), |_| true);
        assert!(got.is_some());
    }

    /// The operator always wins: an explicit `env.LD_PRELOAD` on the server
    /// config is never second-guessed, even inside Termux.
    #[test]
    fn termux_ld_preload_never_overrides_the_server_config() {
        let env = server_env(&[("LD_PRELOAD", "/custom/lib.so")]);
        let got = termux_ld_preload(&env, Some(TERMUX_PREFIX), None, |_| true);
        assert_eq!(got, None);
    }

    /// Same for a preload the gateway itself was started with — appending to
    /// it is the caller's decision, not ours.
    #[test]
    fn termux_ld_preload_never_overrides_an_inherited_value() {
        let got = termux_ld_preload(
            &server_env(&[]),
            Some(TERMUX_PREFIX),
            Some("/other/lib.so"),
            |_| true,
        );
        assert_eq!(got, None);
    }

    /// Off Termux nothing is injected, whatever the platform reports.
    #[test]
    fn termux_ld_preload_is_inert_outside_termux() {
        for prefix in [None, Some("/usr"), Some("/home/me/.local")] {
            let got = termux_ld_preload(&server_env(&[]), prefix, None, |_| true);
            assert_eq!(got, None, "prefix {prefix:?} must not trigger injection");
        }
    }

    /// `pkg install termux-exec` is a prerequisite, not an assumption:
    /// pointing `LD_PRELOAD` at a file that is not there buys nothing and
    /// makes the child's failure harder to read.
    #[test]
    fn termux_ld_preload_requires_the_shim_to_exist() {
        let got = termux_ld_preload(&server_env(&[]), Some(TERMUX_PREFIX), None, |_| false);
        assert_eq!(got, None);
    }

    /// The probe is handed the full path so a caller cannot accidentally test
    /// `$PREFIX` itself for existence.
    #[test]
    fn termux_ld_preload_probes_the_full_shim_path() {
        let seen = std::cell::RefCell::new(Vec::new());
        let _ = termux_ld_preload(&server_env(&[]), Some(TERMUX_PREFIX), None, |p| {
            seen.borrow_mut().push(p.display().to_string());
            true
        });
        assert_eq!(
            seen.into_inner(),
            vec![format!("{TERMUX_PREFIX}/{TERMUX_EXEC_LIB}")]
        );
    }

    #[test]
    fn validate_mcp_url_allows_local_servers() {
        // Self-hosted MCP on loopback or the LAN is the ordinary case and must
        // keep working — this is what stops the guard from being a regression.
        //
        // Literal IPs only, on purpose: `vet_url` resolves the host, so a
        // hostname here would make the test depend on the runner having DNS.
        for url in [
            "http://127.0.0.1:3000/mcp",
            "http://192.168.1.10:8080/mcp",
            "https://10.1.2.3:8443/mcp",
            "http://[::1]:3000/mcp",
        ] {
            assert!(validate_mcp_url(url).is_ok(), "{url} should be allowed");
        }
    }

    #[test]
    fn validate_mcp_url_rejects_metadata_and_bad_schemes() {
        for url in [
            "http://169.254.169.254/latest/meta-data/",
            "http://[fe80::1]/mcp",
            "file:///etc/passwd",
            "gopher://evil.test/_x",
            "not a url",
            // NAT64-embedded metadata: only blocked once the guard knows the
            // 64:ff9b::/96 prefix.
            "http://[64:ff9b::169.254.169.254]/mcp",
        ] {
            assert!(validate_mcp_url(url).is_err(), "{url} should be rejected");
        }
    }

    use super::{RestartState, is_tool_allowed, validate_mcp_url};

    #[test]
    fn empty_allowlist_allows_everything() {
        assert!(is_tool_allowed(&[], "read_file"));
        assert!(is_tool_allowed(&[], "anything"));
    }

    #[test]
    fn restart_backoff_doubles_and_caps_at_300s() {
        let mut st = RestartState::new(10, 5);
        assert_eq!(st.current_delay_secs(), 5);
        st.record_attempt();
        assert_eq!(st.current_delay_secs(), 10);
        st.record_attempt();
        assert_eq!(st.current_delay_secs(), 20);
        for _ in 0..8 {
            st.record_attempt();
        }
        assert_eq!(st.current_delay_secs(), 300, "delay must saturate at 300s");
    }

    #[test]
    fn restart_state_exhausts_after_max_restarts() {
        let mut st = RestartState::new(3, 1);
        assert!(st.should_retry_now(), "first attempt is immediate");
        for _ in 0..3 {
            st.record_attempt();
        }
        assert!(st.is_exhausted());
        assert!(
            !st.should_retry_now(),
            "an exhausted server must not be retried automatically"
        );
    }

    /// The counter is only cleared explicitly (after STABILITY_WINDOW), never
    /// by a bare handshake — otherwise a server that connects and dies two
    /// seconds later restarts forever without ever reaching max_restarts.
    #[test]
    fn reset_clears_counter_so_flapping_needs_a_stability_window() {
        let mut st = RestartState::new(3, 1);
        st.record_attempt();
        st.record_attempt();
        assert_eq!(st.count, 2);
        st.reset();
        assert_eq!(st.count, 0);
        assert!(!st.is_exhausted());
        assert!(st.should_retry_now());
    }

    #[test]
    fn allowlist_matches_bare_tool_name_only() {
        let allowed = vec!["read_file".to_string()];
        assert!(is_tool_allowed(&allowed, "read_file"));
        assert!(!is_tool_allowed(&allowed, "write_file"));
        // Namespaced form is not what the allowlist stores.
        assert!(!is_tool_allowed(&allowed, "filesystem.read_file"));
    }
}
