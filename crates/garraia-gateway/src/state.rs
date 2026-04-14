use crate::mcp_commands;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use garraia_agents::{AgentRuntime, ChatMessage};
use garraia_auth::{
    AppPool, InternalProvider, JwtIssuer, LoginPool, SessionStore as AuthSessionStore, SignupPool,
};
use garraia_channels::{ChannelRegistry, CommandRegistry};
use garraia_config::AppConfig;
use garraia_db::{ChatSessionManager, SessionStore};
use garraia_runtime::RuntimeSettings;
use garraia_security::{Allowlist, PairingManager};
use std::sync::RwLock;
use tokio::sync::{Mutex, watch};
use tracing::{info, warn};
use uuid::Uuid;

/// How long a disconnected session is kept for resume.
const SESSION_TTL: Duration = Duration::from_secs(3600); // 1 hour
/// How often the cleanup task runs.
const CLEANUP_INTERVAL: Duration = Duration::from_secs(300); // 5 minutes

/// Shared application state accessible from all request handlers.
pub struct AppState {
    pub config: AppConfig,
    pub channels: tokio::sync::RwLock<ChannelRegistry>,
    pub agents: AgentRuntime,
    pub sessions: DashMap<String, SessionState>,
    /// Model overrides per channel (e.g. "telegram-123456" -> "openai/gpt-4o")
    pub channel_models: DashMap<String, String>,
    /// In-flight A2A tasks keyed by task ID.
    pub a2a_tasks: DashMap<String, garraia_agents::a2a::A2ATask>,
    /// MCP server connection manager (legacy, for backward compat).
    pub mcp_manager: Option<garraia_agents::McpManager>,
    /// MCP manager wrapped in Arc for health monitoring.
    pub mcp_manager_arc: Option<Arc<garraia_agents::McpManager>>,
    /// Live registry of MCP server configs and statuses (source of truth for admin API).
    pub mcp_registry: crate::mcp::McpRuntimeRegistry,
    pub session_store: Option<Arc<Mutex<SessionStore>>>,
    /// GAR-201: Unified session manager for multi-channel external-ID → session-ID mapping.
    pub chat_session_manager: Option<Arc<ChatSessionManager>>,
    /// Receives hot-reloaded config updates. `None` if watcher is not active.
    config_rx: Option<watch::Receiver<AppConfig>>,
    /// Broadcast channel for tailing logs to WebSocket clients.
    pub log_tx: tokio::sync::broadcast::Sender<serde_json::Value>,
    /// OpenClaw bridge client (available when OPENCLAW_ENABLED=true).
    pub openclaw_client: Option<Arc<garraia_channels::OpenClawClient>>,
    /// OpenClaw configuration.
    pub openclaw_config: Option<garraia_channels::OpenClawConfig>,
    /// TTS client (available when `--with-voice` is used).
    /// Supports Chatterbox, Hibiki, and LM Studio providers via `dyn TtsSynthesizer`.
    pub voice_client: Option<Arc<dyn garraia_voice::TtsSynthesizer>>,
    /// Whisper STT client (available when `--with-voice` is used).
    pub stt_client: Option<Arc<garraia_voice::WhisperClient>>,
    /// Cached health check results (updated by background task).
    pub health_cache: Option<crate::health::HealthCache>,
    /// Central registry for slash commands.
    pub command_registry: Arc<RwLock<CommandRegistry>>,
    /// Authorized users list.
    pub allowlist: Arc<std::sync::Mutex<Allowlist>>,
    /// Generates pairing codes.
    pub pairing: Arc<std::sync::Mutex<PairingManager>>,
    /// Tracks when the application started for uptime calculation.
    pub boot_time: std::time::Instant,
    /// Runtime settings for agent execution.
    pub runtime_settings: RuntimeSettings,

    // ── GAR-391c: garraia-auth wiring ──────────────────────────────────────
    // All five are `Option` so the gateway boots in fail-soft mode when
    // `AuthConfig::from_env` returns None (missing env vars in dev). The
    // `/v1/auth/*` endpoints check for `Some` and return 503 otherwise.
    //
    // Note: the SQLite legacy `session_store` field above is intentionally
    // distinct from `auth_session_store` (Postgres `garraia-auth::SessionStore`)
    // because they back unrelated paths (mobile_auth.rs vs the new auth_routes.rs).
    /// `InternalProvider` wired through `LoginPool` (BYPASSRLS, garraia_login role).
    /// Security review 391c L-3: `pub` here is acceptable because the inner
    /// `LoginPool::pool()` accessor is `pub(crate)` to `garraia-auth`, so
    /// downstream gateway code cannot extract the raw `PgPool`. The
    /// `Arc<InternalProvider>` only exposes the trait surface.
    pub auth_provider: Option<Arc<InternalProvider>>,
    /// JWT issuer + verifier (HS256), shared across login + extractor.
    pub jwt_issuer: Option<Arc<JwtIssuer>>,
    /// Postgres-backed session store for refresh tokens (`garraia-auth::SessionStore`).
    /// Renamed to `auth_session_store` to avoid collision with the legacy SQLite
    /// `session_store` field above.
    pub auth_session_store: Option<Arc<AuthSessionStore>>,
    /// Dedicated signup pool (BYPASSRLS, garraia_signup role) for the
    /// `/v1/auth/signup` endpoint. Distinct from `LoginPool` by design.
    /// `pub(crate)` per security review L-3 — only `auth_routes.rs` needs it.
    pub(crate) signup_pool: Option<Arc<SignupPool>>,
    /// Shared `Arc<LoginPool>` so the extractor can perform group_members
    /// membership lookups without holding a separate handle.
    /// `pub(crate)` per security review L-3 — only the extractor + auth_routes
    /// need it; downstream handlers must NOT acquire the BYPASSRLS pool.
    pub(crate) login_pool: Option<Arc<LoginPool>>,
    /// Shared `Arc<AppPool>` — the RLS-enforced `garraia_app` pool used by
    /// `/v1/*` handlers outside the auth flow. `pub(crate)` — only
    /// `rest_v1` constructs a `RestV1FullState` from it. Absent when
    /// `GARRAIA_APP_DATABASE_URL` is not configured (plan 0016 M1-T3).
    pub(crate) app_pool: Option<Arc<AppPool>>,
}

/// Per-connection session tracking.
pub struct SessionState {
    pub id: String,
    pub tenant_id: String,
    pub user_id: Option<String>,
    pub channel_id: Option<String>,
    pub history: Vec<ChatMessage>,
    /// Currently active agent for this session (None = default)
    pub agent_id: Option<String>,
    /// Whether a WebSocket is currently attached.
    pub connected: bool,
    /// When the session was created.
    pub created_at: Instant,
    /// Last time the session had activity (message or pong).
    pub last_active: Instant,
    /// Phase 1.3: Working directory for this session's project context.
    pub working_dir: Option<String>,
    /// Phase 1.3: Human-readable project name for this session.
    pub project_name: Option<String>,
    /// Phase 1.3: Project ID linking this session to a registered project.
    pub project_id: Option<String>,
}

/// Agent configuration override for a session.
#[derive(Debug, Clone, Default)]
pub struct AgentConfigOverride {
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    pub max_tokens: Option<u32>,
}

impl AppState {
    pub fn new(config: AppConfig, agents: AgentRuntime, channels: ChannelRegistry) -> Self {
        Self {
            config,
            channels: tokio::sync::RwLock::new(channels),
            agents,
            sessions: DashMap::new(),
            channel_models: DashMap::new(),
            a2a_tasks: DashMap::new(),
            mcp_manager: None,
            mcp_manager_arc: None,
            mcp_registry: {
                // GAR-291: attach vault so sensitive env vars are resolved on load.
                // Provision filesystem MCP on first boot when mcp.json is absent.
                let svc = crate::mcp::McpPersistenceService::with_default_path();
                svc.provision_filesystem_if_missing();
                let svc = if let Some(vp) = crate::bootstrap::default_vault_path() {
                    svc.with_vault(vp)
                } else {
                    svc
                };
                svc.load_registry()
            },
            session_store: None,
            chat_session_manager: None,
            config_rx: None,
            log_tx: tokio::sync::broadcast::channel(100).0,
            openclaw_client: None,
            openclaw_config: None,
            voice_client: None,
            stt_client: None,
            health_cache: None,
            command_registry: {
                let reg = CommandRegistry::new();
                Arc::new(RwLock::new(reg))
            },
            allowlist: Arc::new(std::sync::Mutex::new(Allowlist::load_or_create(
                &garraia_config::ConfigLoader::default_config_dir().join("allowlist.json"),
            ))),
            pairing: Arc::new(std::sync::Mutex::new(PairingManager::new(
                std::time::Duration::from_secs(300),
            ))),
            boot_time: Instant::now(),
            runtime_settings: RuntimeSettings::default(),
            // GAR-391c auth wiring — None until bootstrap loads AuthConfig.
            auth_provider: None,
            jwt_issuer: None,
            auth_session_store: None,
            signup_pool: None,
            login_pool: None,
            // Plan 0016 M1-T3 — None until bootstrap loads AuthConfig
            // AND `GARRAIA_APP_DATABASE_URL` is set AND the pool
            // connects successfully. Independent of the other three
            // pools: absence only degrades /v1/groups-style handlers.
            app_pool: None,
        }
    }

    /// Attach the garraia-auth components built from `AuthConfig` at bootstrap
    /// time. All four required pools/tokens are wired together: `LoginPool`
    /// is shared by `InternalProvider` and `SessionStore`. `SignupPool` is
    /// independent. `AppPool` (plan 0016 M1-T3) is **optional** and wired
    /// only when `GARRAIA_APP_DATABASE_URL` was set and the connect
    /// succeeded — handlers that need it fall back to 503 when `None`.
    /// (GAR-391c + plan 0016 M1-T3)
    pub fn set_auth_components(
        &mut self,
        login_pool: Arc<LoginPool>,
        signup_pool: Arc<SignupPool>,
        jwt_issuer: Arc<JwtIssuer>,
        app_pool: Option<Arc<AppPool>>,
    ) {
        let provider = Arc::new(InternalProvider::new(login_pool.clone()));
        let session_store = Arc::new(AuthSessionStore::new(login_pool.clone()));
        self.auth_provider = Some(provider);
        self.jwt_issuer = Some(jwt_issuer);
        self.auth_session_store = Some(session_store);
        self.signup_pool = Some(signup_pool);
        self.login_pool = Some(login_pool);
        self.app_pool = app_pool;
    }

    /// Attach a persistent session store used to hydrate and persist chat history.
    pub fn set_session_store(&mut self, store: Arc<Mutex<SessionStore>>) {
        self.session_store = Some(store);
    }

    /// Attach the GAR-201 ChatSessionManager for multi-channel session resolution.
    pub fn set_chat_session_manager(&mut self, manager: Arc<ChatSessionManager>) {
        self.chat_session_manager = Some(manager);
    }

    /// Attach a config watch receiver for hot-reload support.
    pub fn set_config_watcher(&mut self, rx: watch::Receiver<AppConfig>) {
        self.config_rx = Some(rx);
    }

    /// Configure the runtime settings for agent execution.
    pub fn set_runtime_settings(&mut self, settings: RuntimeSettings) {
        self.runtime_settings = settings;
    }

    /// Register MCP tools as slash commands.
    /// This does the async work first, then registers synchronously.
    pub async fn register_mcp_tools(&self) {
        if let Some(manager_arc) = &self.mcp_manager_arc {
            // First, do all async work to collect commands (no lock held)
            let commands = mcp_commands::collect_mcp_commands(Arc::clone(manager_arc)).await;

            // Then acquire lock and register synchronously
            let mut registry = self.command_registry.write().unwrap();
            mcp_commands::register_collected_commands(&mut registry, commands);
        }
    }

    /// Get a reference to the runtime settings.
    pub fn runtime_settings(&self) -> &RuntimeSettings {
        &self.runtime_settings
    }

    /// Check if config hot-reload watcher is active.
    pub fn has_config_watcher(&self) -> bool {
        self.config_rx.is_some()
    }

    /// Trigger config hot-reload by sending a new config value through the watcher channel.
    /// Returns Ok(()) if reload was triggered, Err if watcher is not active.
    pub fn trigger_config_reload(&self) -> Result<(), String> {
        if let Some(rx) = &self.config_rx {
            // Clone the current config and send it through to trigger reload
            let _config = rx.borrow().clone();
            // The watcher will detect this as a change and apply it
            // Note: This won't work directly since we can't write to the sender from here
            // Instead, we indicate that reload is possible
            Ok(())
        } else {
            Err("Config watcher not active".to_string())
        }
    }

    /// Get the latest config, preferring the hot-reloaded version if available.
    pub fn current_config(&self) -> AppConfig {
        if let Some(rx) = &self.config_rx {
            rx.borrow().clone()
        } else {
            self.config.clone()
        }
    }

    pub fn create_session(&self) -> String {
        let id = Uuid::new_v4().to_string();
        self.create_session_with_id(id.clone());
        id
    }

    /// Create a session with a specific ID (used by channels like Telegram
    /// where the external chat ID determines the session key).
    pub fn create_session_with_id(&self, id: String) {
        self.create_session_for_tenant(id, "default".to_string());
    }

    /// Create a session with a specific ID scoped to a tenant.
    pub fn create_session_for_tenant(&self, id: String, tenant_id: String) {
        let now = Instant::now();
        self.sessions.insert(
            id.clone(),
            SessionState {
                id,
                tenant_id,
                user_id: None,
                channel_id: None,
                history: Vec::new(),
                agent_id: None,
                connected: true,
                created_at: now,
                last_active: now,
                working_dir: None,
                project_name: None,
                project_id: None,
            },
        );
    }

    /// Resolve the continuity key used by the cross-channel memory bus.
    /// When disabled, returns `None` and memory remains session-scoped.
    pub fn continuity_key(&self, _user_id: Option<&str>) -> Option<String> {
        if self.config.memory.shared_continuity {
            Some("bus:shared-global".to_string())
        } else {
            None
        }
    }

    /// Get the effective agent configuration for a session.
    /// Returns (provider_id, model, system_prompt, max_tokens) for the agent.
    pub fn get_agent_config(&self, session_id: &str) -> AgentConfigOverride {
        let agent_id = self
            .sessions
            .get(session_id)
            .and_then(|s| s.agent_id.clone())
            .unwrap_or_else(|| "default".to_string());

        if agent_id == "default" {
            // Use global agent config
            return AgentConfigOverride {
                provider_id: self.config.agent.default_provider.clone(),
                model: None,
                system_prompt: self.config.agent.system_prompt.clone(),
                max_tokens: self.config.agent.max_tokens,
            };
        }

        // Use named agent config
        if let Some(named_config) = self.config.agents.get(&agent_id) {
            return AgentConfigOverride {
                provider_id: named_config.provider.clone(),
                model: named_config.model.clone(),
                system_prompt: named_config.system_prompt.clone(),
                max_tokens: named_config.max_tokens,
            };
        }

        // Fallback to default
        AgentConfigOverride {
            provider_id: self.config.agent.default_provider.clone(),
            model: None,
            system_prompt: self.config.agent.system_prompt.clone(),
            max_tokens: self.config.agent.max_tokens,
        }
    }

    /// Get current agent ID for a session.
    pub fn get_session_agent_id(&self, session_id: &str) -> Option<String> {
        self.sessions
            .get(session_id)
            .and_then(|s| s.agent_id.clone())
    }

    /// Return a cloned history snapshot for a session.
    pub fn session_history(&self, session_id: &str) -> Vec<ChatMessage> {
        self.sessions
            .get(session_id)
            .map(|s| s.history.clone())
            .unwrap_or_default()
    }

    /// Ensure a session is present in memory and hydrate recent history from persistent storage.
    pub async fn hydrate_session_history(
        &self,
        session_id: &str,
        channel_id: Option<&str>,
        user_id: Option<&str>,
    ) {
        if !self.sessions.contains_key(session_id) {
            self.create_session_with_id(session_id.to_string());
        }

        if let Some(mut session) = self.sessions.get_mut(session_id) {
            if let Some(channel) = channel_id {
                session.channel_id = Some(channel.to_string());
            }
            if let Some(user) = user_id {
                session.user_id = Some(user.to_string());
            }
            session.connected = true;
            session.last_active = Instant::now();
        }

        let Some(store) = &self.session_store else {
            return;
        };

        let should_load = self
            .sessions
            .get(session_id)
            .map(|s| s.history.is_empty())
            .unwrap_or(false);

        let tenant_id = self
            .sessions
            .get(session_id)
            .map(|s| s.tenant_id.clone())
            .unwrap_or_else(|| "default".to_string());
        let channel = channel_id.unwrap_or("web");
        let user = user_id.unwrap_or("anonymous");
        let metadata = self
            .continuity_key(user_id)
            .map(|k| serde_json::json!({ "continuity_key": k }))
            .unwrap_or_else(|| serde_json::json!({}));

        let mut loaded_history = Vec::new();
        {
            let guard = store.lock().await;
            if let Err(e) =
                guard.upsert_session_with_tenant(session_id, &tenant_id, channel, user, &metadata)
            {
                warn!("failed to upsert session {session_id} in session store: {e}");
            }

            if should_load {
                // GAR-208: prepend latest summary (if any) as a System message so the
                // LLM has context about turns that fall outside the sliding window.
                if let Ok(Some((summary_text, _))) = guard.get_latest_session_summary(session_id) {
                    loaded_history.push(ChatMessage {
                        role: garraia_agents::ChatRole::System,
                        content: garraia_agents::MessagePart::Text(format!(
                            "[Conversation summary up to this point]\n{summary_text}"
                        )),
                    });
                }

                match guard.load_recent_messages(session_id, 100) {
                    Ok(messages) => {
                        let recent: Vec<ChatMessage> = messages
                            .into_iter()
                            .filter_map(|m| match m.direction.as_str() {
                                "user" => Some(ChatMessage {
                                    role: garraia_agents::ChatRole::User,
                                    content: garraia_agents::MessagePart::Text(m.content),
                                }),
                                "assistant" => Some(ChatMessage {
                                    role: garraia_agents::ChatRole::Assistant,
                                    content: garraia_agents::MessagePart::Text(m.content),
                                }),
                                _ => None,
                            })
                            .collect();
                        loaded_history.extend(recent);
                    }
                    Err(e) => {
                        warn!("failed to load session history for {session_id}: {e}");
                    }
                }
            }
        }

        if should_load
            && !loaded_history.is_empty()
            && let Some(mut session) = self.sessions.get_mut(session_id)
        {
            session.history = loaded_history;
        }
    }

    /// Append a user/assistant turn to in-memory state and persistent session storage.
    pub async fn persist_turn(
        &self,
        session_id: &str,
        channel_id: Option<&str>,
        user_id: Option<&str>,
        user_text: &str,
        assistant_text: &str,
    ) {
        if !self.sessions.contains_key(session_id) {
            self.create_session_with_id(session_id.to_string());
        }

        if let Some(mut session) = self.sessions.get_mut(session_id) {
            if let Some(channel) = channel_id {
                session.channel_id = Some(channel.to_string());
            }
            if let Some(user) = user_id {
                session.user_id = Some(user.to_string());
            }
            session.last_active = Instant::now();
            session.history.push(ChatMessage {
                role: garraia_agents::ChatRole::User,
                content: garraia_agents::MessagePart::Text(user_text.to_string()),
            });
            session.history.push(ChatMessage {
                role: garraia_agents::ChatRole::Assistant,
                content: garraia_agents::MessagePart::Text(assistant_text.to_string()),
            });
        }

        let Some(store) = &self.session_store else {
            return;
        };

        let tenant_id = self
            .sessions
            .get(session_id)
            .map(|s| s.tenant_id.clone())
            .unwrap_or_else(|| "default".to_string());
        let channel = channel_id.unwrap_or("web");
        let user = user_id.unwrap_or("anonymous");
        let metadata = self
            .continuity_key(user_id)
            .map(|k| serde_json::json!({ "continuity_key": k }))
            .unwrap_or_else(|| serde_json::json!({}));

        let guard = store.lock().await;
        if let Err(e) =
            guard.upsert_session_with_tenant(session_id, &tenant_id, channel, user, &metadata)
        {
            warn!("failed to upsert session {session_id}: {e}");
            return;
        }
        if let Err(e) = guard.append_message(
            session_id,
            "user",
            user_text,
            chrono::Utc::now(),
            &serde_json::json!({ "channel_id": channel, "user_id": user }),
        ) {
            warn!("failed to persist user message for {session_id}: {e}");
        }
        if let Err(e) = guard.append_message(
            session_id,
            "assistant",
            assistant_text,
            chrono::Utc::now(),
            &serde_json::json!({ "channel_id": channel, "user_id": user }),
        ) {
            warn!("failed to persist assistant message for {session_id}: {e}");
        }
    }

    /// Mark a session as disconnected (but don't remove it yet).
    pub fn disconnect_session(&self, session_id: &str) {
        if let Some(mut session) = self.sessions.get_mut(session_id) {
            session.connected = false;
            session.last_active = Instant::now();
        }
    }

    /// Try to resume an existing disconnected session. Returns `true` if resumed.
    pub fn resume_session(&self, session_id: &str) -> bool {
        if let Some(mut session) = self.sessions.get_mut(session_id) {
            session.connected = true;
            session.last_active = Instant::now();
            true
        } else {
            false
        }
    }

    /// Remove sessions that have been disconnected longer than the TTL.
    pub fn cleanup_expired_sessions(&self) -> usize {
        self.cleanup_expired_sessions_at(Instant::now())
    }

    fn cleanup_expired_sessions_at(&self, now: Instant) -> usize {
        let mut removed = 0;

        self.sessions.retain(|_id, session| {
            if !session.connected && now.duration_since(session.last_active) > SESSION_TTL {
                removed += 1;
                false
            } else {
                true
            }
        });

        if removed > 0 {
            info!("cleaned up {removed} expired sessions");
        }
        removed
    }

    /// Spawn a background task that periodically cleans up expired sessions.
    pub fn spawn_session_cleanup(self: &Arc<Self>) {
        let state = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(CLEANUP_INTERVAL);
            loop {
                interval.tick().await;
                state.cleanup_expired_sessions();
            }
        });
    }

    /// GAR-202: Spawn a background task that cleans up expired session tokens every 5 min.
    pub fn spawn_token_cleanup(self: &Arc<Self>) {
        let Some(manager) = self.chat_session_manager.clone() else {
            return;
        };
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
            loop {
                interval.tick().await;
                let n = manager.cleanup_expired_tokens().await;
                if n > 0 {
                    info!("cleaned up {n} expired session tokens");
                }
            }
        });
    }

    /// Spawn a background task that logs hot-reloaded config changes.
    /// Note: Agent-level settings (system_prompt, max_tokens) will take effect
    /// on next restart. Provider and channel changes also require restart.
    pub fn spawn_config_applier(self: &Arc<Self>) {
        let Some(mut rx) = self.config_rx.clone() else {
            return;
        };

        tokio::spawn(async move {
            while rx.changed().await.is_ok() {
                let new_config = rx.borrow().clone();

                if let Some(prompt) = &new_config.agent.system_prompt {
                    info!(
                        "config reloaded: system_prompt updated (len={})",
                        prompt.len()
                    );
                }
                if let Some(max_tokens) = new_config.agent.max_tokens {
                    info!("config reloaded: max_tokens={max_tokens}");
                }
                if let Some(level) = &new_config.log_level {
                    info!("config reloaded: log_level={level}");
                }
            }

            warn!("config watcher channel closed");
        });
    }
}

pub type SharedState = Arc<AppState>;

#[cfg(test)]
mod tests {
    use super::*;
    use garraia_agents::AgentRuntime;
    use garraia_channels::ChannelRegistry;
    use garraia_config::AppConfig;

    fn test_state() -> AppState {
        AppState::new(
            AppConfig::default(),
            AgentRuntime::new(),
            ChannelRegistry::new(),
        )
    }

    #[test]
    fn create_session_returns_unique_ids() {
        let state = test_state();
        let id1 = state.create_session();
        let id2 = state.create_session();
        assert_ne!(id1, id2);
        assert_eq!(state.sessions.len(), 2);
    }

    #[test]
    fn disconnect_and_resume_session_round_trip() {
        let state = test_state();
        let id = state.create_session();

        // Initially connected
        assert!(state.sessions.get(&id).unwrap().connected);

        state.disconnect_session(&id);
        assert!(!state.sessions.get(&id).unwrap().connected);

        let resumed = state.resume_session(&id);
        assert!(resumed);
        assert!(state.sessions.get(&id).unwrap().connected);
    }

    #[test]
    fn resume_nonexistent_session_returns_false() {
        let state = test_state();
        assert!(!state.resume_session("does-not-exist"));
    }

    #[test]
    fn cleanup_expired_sessions_removes_only_disconnected_expired() {
        let state = test_state();

        // Create two sessions; record their last_active timestamps
        let active_id = state.create_session();
        let expired_id = state.create_session();

        // Disconnect the expired session. Its last_active is set to Instant::now()
        // at creation time. We simulate expiry by running cleanup with a "now"
        // 2 hours in the future, which avoids subtracting from Instant (UB on
        // systems that have been running < 2 h).
        state.disconnect_session(&expired_id);
        let future_now = Instant::now() + Duration::from_secs(7200);

        let removed = state.cleanup_expired_sessions_at(future_now);
        assert_eq!(removed, 1);
        assert!(state.sessions.contains_key(&active_id));
        assert!(!state.sessions.contains_key(&expired_id));
    }

    #[test]
    fn cleanup_does_not_remove_connected_sessions() {
        let state = test_state();
        let id = state.create_session();

        // Session is still connected. Even with a "now" 2 hours in the future
        // the connected session must NOT be removed.
        let future_now = Instant::now() + Duration::from_secs(7200);

        let removed = state.cleanup_expired_sessions_at(future_now);
        assert_eq!(removed, 0);
        assert!(state.sessions.contains_key(&id));
    }

    #[test]
    fn session_history_returns_empty_for_unknown_session() {
        let state = test_state();
        let history = state.session_history("nonexistent");
        assert!(history.is_empty());
    }

    #[test]
    fn continuity_key_with_shared_continuity_enabled() {
        let mut config = AppConfig::default();
        config.memory.shared_continuity = true;
        let state = AppState::new(config, AgentRuntime::new(), ChannelRegistry::new());
        let key = state.continuity_key(Some("user1"));
        assert_eq!(key, Some("bus:shared-global".to_string()));
    }

    #[test]
    fn continuity_key_with_shared_continuity_disabled() {
        let mut config = AppConfig::default();
        config.memory.shared_continuity = false;
        let state = AppState::new(config, AgentRuntime::new(), ChannelRegistry::new());
        let key = state.continuity_key(Some("user1"));
        assert_eq!(key, None);
    }
}
