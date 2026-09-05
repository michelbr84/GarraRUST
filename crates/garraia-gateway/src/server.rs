use std::sync::Arc;

use garraia_common::{
    ChannelId, Message, MessageContent, MessageDirection, Result, SessionId, UserId,
};
use garraia_config::{AppConfig, ConfigWatcher};
use garraia_db::{ChatSessionManager, SessionStore};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::admin;
#[cfg(target_os = "macos")]
use crate::bootstrap::build_imessage_channels;
use crate::bootstrap::{
    build_agent_runtime, build_channels, build_discord_channels, build_mcp_tools,
    build_slack_channels, build_telegram_channels, build_whatsapp_channels,
};
use crate::router::build_router;
use crate::state::AppState;

/// The main gateway server that binds to a port and serves the API + WebSocket.
pub struct GatewayServer {
    config: AppConfig,
    /// Plan 0024 (GAR-412): optional handle to the globally-installed
    /// Prometheus recorder. When `Some`, the server spawns the
    /// dedicated `/metrics` listener (fail-closed startup). When
    /// `None`, only the embedded main-listener route is active.
    #[cfg(feature = "telemetry")]
    metrics_handle: Option<garraia_telemetry::PrometheusHandle>,
    /// Plan 0024 (GAR-412): telemetry config loaded by the CLI
    /// alongside `garraia_telemetry::init()`. Passed in rather than
    /// re-read here (code review MEDIUM) so the auth config for
    /// `/metrics` and the tracing config come from one consistent
    /// snapshot of the environment.
    #[cfg(feature = "telemetry")]
    telemetry_config: Option<garraia_telemetry::TelemetryConfig>,
}

/// Refuse to boot while `gateway.session_tokens_required` is set.
///
/// Security finding from the #930 investigation: the middleware that was
/// supposed to implement this flag (`require_session_auth`,
/// `session_auth.rs`) has never been wired into any router layer — the flag
/// is a no-op for HTTP, while `docs/hardening-gateway.md` and both READMEs
/// told operators it protects the `/api/*` surface. An operator who followed
/// the hardening guide believed they were protected and were not.
///
/// Refusing beats the two alternatives. Booting with a warning keeps the lie
/// alive for anyone not reading logs; silently *enabling* the auth would
/// break every local integration that relies on today's open access — the
/// opposite surprise, delivered to different people. The refusal names the
/// one-line way out, so nobody is stranded: removing the flag changes
/// nothing about the gateway's actual behavior.
///
/// Free function rather than a method so the decision is testable without
/// constructing a `GatewayServer` (which wants a full `AppConfig` anyway,
/// but `run()` cannot be called twice and binds sockets).
fn refuse_inert_auth_flag(config: &AppConfig) -> Result<()> {
    if config.gateway.session_tokens_required {
        return Err(garraia_common::Error::Config(
            "gateway.session_tokens_required: true is set, but this flag is NOT implemented: \
             no HTTP route validates session tokens today, so it would protect nothing while \
             claiming to. Refusing to start rather than pretend. Fix: remove the flag (or set \
             it to false) in your config file — this changes nothing about actual behavior, \
             it only stops asserting a protection that does not exist. Track the real \
             implementation in the GarraRUST issue tracker."
                .into(),
        ));
    }
    Ok(())
}

impl GatewayServer {
    pub fn new(config: AppConfig) -> Self {
        Self {
            config,
            #[cfg(feature = "telemetry")]
            metrics_handle: None,
            #[cfg(feature = "telemetry")]
            telemetry_config: None,
        }
    }

    /// Plan 0024 (GAR-412): attach the `PrometheusHandle` produced by
    /// `garraia_telemetry::init()`. Builder-style so the existing CLI
    /// wiring only needs one extra call.
    #[cfg(feature = "telemetry")]
    pub fn with_metrics_handle(
        mut self,
        handle: Option<garraia_telemetry::PrometheusHandle>,
    ) -> Self {
        self.metrics_handle = handle;
        self
    }

    /// Plan 0024 (GAR-412): attach the `TelemetryConfig` the CLI
    /// already loaded. Avoids a second `TelemetryConfig::from_env()`
    /// call inside `run()` and the transient env-var inconsistency
    /// that pattern would introduce (code review MEDIUM).
    #[cfg(feature = "telemetry")]
    pub fn with_telemetry_config(
        mut self,
        cfg: Option<garraia_telemetry::TelemetryConfig>,
    ) -> Self {
        self.telemetry_config = cfg;
        self
    }

    pub async fn run(self) -> Result<()> {
        // Load `.env` before anything reads the environment. This must precede
        // `build_agent_runtime`, which resolves every provider's API key from
        // env vars: when the load lived in `build_channels` (called ~20 lines
        // below) `.env` provisioned channels but never providers, so an
        // embedder of `Server` got working Telegram and zero LLMs. Idempotent,
        // and it never overwrites an already-set variable.
        if let Err(e) = dotenvy::dotenv() {
            tracing::debug!("no .env file loaded: {e}");
        }

        // Security finding from the #930 investigation: refuse to boot with a
        // hardening flag that does not do what its documentation promised.
        refuse_inert_auth_flag(&self.config)?;

        let addr = format!("{}:{}", self.config.gateway.host, self.config.gateway.port);
        let tls_cert = self.config.gateway.tls_cert_path.clone();
        let tls_key = self.config.gateway.tls_key_path.clone();

        let agents = build_agent_runtime(&self.config);

        // Provision the default mcp.json BEFORE building tools. This used to
        // happen only inside AppState::new (below), i.e. after build_mcp_tools
        // had already read an absent file — so the very first boot always
        // came up with zero MCP tools.
        crate::mcp::McpPersistenceService::with_default_path().provision_filesystem_if_missing();

        // Connect MCP servers, then let the runtime pull their tools from the
        // manager. Issue #924: the boot path used to register the flat
        // `mcp_tools` vec directly, which left those tools indistinguishable
        // from native ones — so nothing could later replace them when a server
        // reconnected with a different inventory. Going through
        // `sync_mcp_tools` tags each tool with its server and makes boot use
        // the *same* code path as the health monitor and the admin handlers.
        let (mcp_manager, _mcp_tools, mcp_failures) = build_mcp_tools(&self.config).await;
        let mcp_tool_count = agents.sync_mcp_tools(&mcp_manager).await;
        if mcp_tool_count > 0 {
            info!(
                mcp_tools = mcp_tool_count,
                tools = ?agents.tool_names(),
                "MCP tools registered into AgentRuntime"
            );
        } else {
            info!("no MCP tools registered (no MCP servers configured or connected)");
        }

        let channels = build_channels(&self.config).await;
        let agents = Arc::new(agents);
        let mut state = AppState::new(self.config, agents, channels);

        // `build_mcp_tools` already returns the Arc: the bridged tools hold it
        // so they can resolve the CURRENT peer on every call (surviving
        // reconnects). register_mcp_tools() also reads mcp_manager_arc, and
        // used to be a silent no-op when the Arc was created ~300 lines below.
        let mcp_manager_arc = mcp_manager;
        state.mcp_manager_arc = Some(Arc::clone(&mcp_manager_arc));

        // Sync MCP registry with live manager state (populates Running/Stopped statuses).
        state.mcp_registry.sync_from_manager(&mcp_manager_arc).await;

        // Surface connect failures in the registry so `mcp list` / admin UI
        // show the actual error instead of a generic Stopped.
        for (name, message) in &mcp_failures {
            state
                .mcp_registry
                .set_status(
                    name,
                    crate::mcp::McpStatus::Error {
                        message: message.clone(),
                    },
                    0,
                )
                .await;
        }

        // Register MCP tools as slash commands (must be done before Arc-wrapping)
        state.register_mcp_tools().await;

        // Register built-in commands (must be done before Telegram channels are created)
        crate::commands::register_commands(&mut state.command_registry.write().unwrap());

        // Initialize TTS client if voice is enabled (supports multiple providers)
        if state.config.voice.enabled {
            let tts_provider = state.config.voice.tts_provider.as_str();
            let tts_endpoint = &state.config.voice.tts_endpoint;
            let language = &state.config.voice.language;

            let tts_client: Arc<dyn garraia_voice::TtsSynthesizer> = match tts_provider {
                "hibiki" => {
                    let client = garraia_voice::HibikiClient::new(
                        &state.config.voice.hibiki_endpoint,
                        language,
                    );
                    info!(
                        "🔊 Voice mode enabled — Hibiki TTS at {}",
                        state.config.voice.hibiki_endpoint
                    );
                    Arc::new(client)
                }
                "lmstudio" => {
                    let model = state
                        .config
                        .voice
                        .tts_model
                        .as_deref()
                        .unwrap_or("vieneu-tts-v2-turbo");
                    let client =
                        garraia_voice::LmStudioTtsClient::new(tts_endpoint, model, language);
                    match client.health_check().await {
                        Ok(true) => {
                            info!(
                                "🔊 Voice mode enabled — LM Studio TTS at {} (model: {})",
                                tts_endpoint, model
                            );
                        }
                        Ok(false) => {
                            warn!(
                                "⚠️  Voice mode requested but LM Studio is not reachable at {}",
                                tts_endpoint
                            );
                        }
                        Err(e) => {
                            warn!("⚠️  LM Studio TTS health check failed: {e}");
                        }
                    }
                    Arc::new(client)
                }
                _ => {
                    // Default: Chatterbox
                    let client = garraia_voice::ChatterboxClient::new(tts_endpoint, language);
                    match client.health_check().await {
                        Ok(true) => {
                            info!("🔊 Voice mode enabled — Chatterbox TTS at {}", tts_endpoint);
                        }
                        Ok(false) => {
                            warn!(
                                "⚠️  Voice mode requested but Chatterbox is not reachable at {}",
                                tts_endpoint
                            );
                        }
                        Err(e) => {
                            warn!("⚠️  Chatterbox TTS health check failed: {e}");
                        }
                    }
                    Arc::new(client)
                }
            };
            state.voice_client = Some(tts_client);

            // Initialize Whisper STT client (works for both standalone Whisper and LM Studio)
            let stt_client = garraia_voice::WhisperClient::new(
                &state.config.voice.stt_endpoint,
                &state.config.voice.language,
            );
            info!(
                "🎙️  STT initialized — Whisper at {}",
                state.config.voice.stt_endpoint
            );
            state.stt_client = Some(Arc::new(stt_client));
        }

        // Initialize persistent session storage used by channel memory bus hydration.
        let data_dir = state
            .config
            .data_dir
            .clone()
            .or_else(|| dirs::home_dir().map(|h| h.join(".garraia").join("data")))
            .unwrap_or_else(|| ".garraia/data".into());
        if let Err(e) = std::fs::create_dir_all(&data_dir) {
            warn!("failed to create data directory: {e}");
        }
        let sessions_db = data_dir.join("sessions.db");
        match SessionStore::open(&sessions_db) {
            Ok(store) => {
                let store = Arc::new(Mutex::new(store));
                state.set_session_store(Arc::clone(&store));
                // GAR-201: Create ChatSessionManager from the same store for multi-channel session resolution
                state.set_chat_session_manager(Arc::new(ChatSessionManager::new(Arc::clone(
                    &store,
                ))));
                // Issue #924: `register_tool` takes `&self` now, so this no
                // longer needs `Arc::get_mut` — which used to skip registering
                // the schedule tools entirely, with only a `warn!`, whenever
                // the Arc had picked up a second owner.
                state
                    .agents
                    .register_tool(Box::new(garraia_agents::ScheduleHeartbeat::new(
                        Arc::clone(&store),
                    )));
                state
                    .agents
                    .register_tool(Box::new(garraia_agents::ScheduleRecurring::new(store)));
                info!("session store opened at {}", sessions_db.display());
            }
            Err(e) => {
                warn!("failed to open session store: {e}");
            }
        }

        // GAR-391c: wire garraia-auth components from AuthConfig env vars.
        // Fail-soft: if AuthConfig::from_env returns None (any var missing),
        // the gateway boots without /v1/auth/* enabled and the handlers
        // return 503. This is intentional for dev mode.
        match garraia_config::AuthConfig::from_env() {
            Ok(Some(auth_cfg)) => {
                use garraia_auth::{
                    JwtConfig, JwtIssuer as AuthJwtIssuer, LoginConfig, LoginPool, SignupConfig,
                    SignupPool,
                };
                use secrecy::ExposeSecret;

                let login_pool_result = LoginPool::from_dedicated_config(&LoginConfig {
                    database_url: auth_cfg.login_database_url.expose_secret().to_string(),
                    max_connections: 5,
                })
                .await;
                let signup_pool_result = SignupPool::from_dedicated_config(&SignupConfig {
                    database_url: auth_cfg.signup_database_url.expose_secret().to_string(),
                    max_connections: 3,
                })
                .await;
                let jwt_result = AuthJwtIssuer::new(JwtConfig {
                    jwt_secret: auth_cfg.jwt_secret.clone(),
                    refresh_hmac_secret: auth_cfg.refresh_hmac_secret.clone(),
                });

                // Plan 0016 M1-T3: optional AppPool construction.
                // Only attempted when GARRAIA_APP_DATABASE_URL is set.
                // Failures degrade /v1/groups-style handlers to 503
                // but never block the rest of the auth wiring.
                let app_pool_opt: Option<Arc<garraia_auth::AppPool>> = if let Some(app_url) =
                    auth_cfg.app_database_url.as_ref()
                {
                    use garraia_auth::AppPoolConfig;
                    match garraia_auth::AppPool::from_dedicated_config(&AppPoolConfig {
                        database_url: app_url.expose_secret().to_string(),
                        max_connections: 10,
                    })
                    .await
                    {
                        Ok(p) => {
                            info!("garraia-auth AppPool wired (garraia_app role)");
                            Some(Arc::new(p))
                        }
                        Err(e) => {
                            // Security review plan 0016 M1 (M-1):
                            // route the sqlx::Error through the
                            // RedactedStorageError wrapper so the
                            // Postgres connection URL cannot leak
                            // into the warn! log line. Some sqlx
                            // 0.8 connect failure variants embed
                            // substrings of the URL in Display.
                            let redacted = match e {
                                garraia_auth::AuthError::Storage(sqlx_err) => {
                                    garraia_auth::RedactedStorageError::from(sqlx_err).to_string()
                                }
                                other => other.to_string(),
                            };
                            warn!(
                                error = %redacted,
                                "AppPool connect failed; /v1/groups-style handlers will answer 503"
                            );
                            None
                        }
                    }
                } else {
                    info!(
                        "GARRAIA_APP_DATABASE_URL not set; /v1/groups-style handlers will answer 503"
                    );
                    None
                };

                match (login_pool_result, signup_pool_result, jwt_result) {
                    (Ok(login_pool), Ok(signup_pool), Ok(jwt)) => {
                        state.set_auth_components(
                            Arc::new(login_pool),
                            Arc::new(signup_pool),
                            Arc::new(jwt),
                            app_pool_opt,
                        );
                        // Plan 0046 (GAR-379 slice 3): stash the canonical
                        // AuthConfig on AppState so mobile_auth + any
                        // future JWT issuer reach the secret via
                        // `AppState::jwt_signing_secret` instead of
                        // `std::env::var`. Cloning the struct into an
                        // Arc keeps the `SecretString` fields
                        // zero-copy at runtime.
                        state.set_auth_config(Arc::new(auth_cfg.clone()));
                        info!("garraia-auth wired (login + signup pools + jwt)");
                    }
                    (lp, sp, jwt) => {
                        warn!(
                            login_ok = lp.is_ok(),
                            signup_ok = sp.is_ok(),
                            jwt_ok = jwt.is_ok(),
                            "garraia-auth wiring partially failed; /v1/auth/* will return 503"
                        );
                    }
                }
            }
            Ok(None) => {
                info!(
                    "AuthConfig env vars not set (GARRAIA_JWT_SECRET / GARRAIA_REFRESH_HMAC_SECRET / GARRAIA_LOGIN_DATABASE_URL / GARRAIA_SIGNUP_DATABASE_URL); /v1/auth/* will return 503"
                );
            }
            Err(e) => {
                warn!(error = %e, "AuthConfig validation failed; /v1/auth/* will return 503");
            }
        }

        // Plan 0044 (GAR-395 slice 2): wire the object-store backend +
        // upload staging context. Fail-soft: any failure (invalid path,
        // missing HMAC secret, unsupported backend without feature) logs
        // at WARN and leaves `state.object_store` = None so tus PATCH
        // answers 503. Runs AFTER auth wiring so we already know whether
        // the gateway can even serve `/v1/*` at all.
        let (object_store_opt, upload_staging_opt) = build_storage_wiring(&state.config).await;
        state.set_storage_components(object_store_opt, upload_staging_opt.clone());

        // Plan 0047 (GAR-395 slice 3): spawn the tus_uploads expiration
        // worker when both the AppPool and the staging directory are
        // wired. The worker runs until process exit (no shutdown channel
        // in v1 — the slice acceptance criteria accept process lifetime
        // granularity). Fail-soft: if either component is absent the
        // worker is skipped silently — the next slice of GAR-429 will
        // surface this via a readiness probe.
        if let Some(app_pool) = state.app_pool.clone() {
            // Task recurrence: materialises the next occurrence of completed
            // recurring tasks. `tasks.recurrence_rrule` existed since
            // migration 006 with no engine behind it.
            crate::tasks_recurrence_worker::spawn_task_recurrence_worker(
                Arc::clone(&app_pool),
                crate::tasks_recurrence_worker::TaskRecurrenceWorkerConfig::default(),
            );

            let staging_dir = upload_staging_opt.as_ref().map(|s| s.staging_dir.clone());
            let handle = crate::uploads_worker::spawn_uploads_expiration_worker(
                app_pool,
                staging_dir,
                crate::uploads_worker::UploadsExpirationWorkerConfig::default(),
            );
            // Detach: keep the worker alive for the process lifetime.
            // `Box::leak` is acceptable here because the handle outlives
            // `server.run()`, which never returns under normal operation.
            std::mem::forget(handle);
            info!("uploads_expiration_worker spawned (plan 0047)");
        } else {
            debug!("uploads_expiration_worker skipped (no AppPool wired)");
        }

        // Start config hot-reload watcher
        let config_path = garraia_config::ConfigLoader::default_config_dir().join("config.yml");

        if config_path.exists() {
            match ConfigWatcher::start(config_path.clone(), state.current_config()) {
                Ok((_watcher, rx)) => {
                    // Keep watcher alive for the process lifetime.
                    let watcher = Box::new(_watcher);
                    Box::leak(watcher);

                    state.set_config_watcher(rx);
                    info!("config hot-reload enabled for {}", config_path.display());
                }
                Err(e) => {
                    warn!("config watcher failed to start: {e}");
                }
            }
        }

        // Plan 0024 (GAR-412): build the metrics auth config from the
        // TelemetryConfig the CLI already loaded (single snapshot —
        // no second `from_env()` call here per code-review MEDIUM) and
        // attach it to state so the embedded /metrics route picks it up.
        // Spawn the dedicated /metrics listener (startup fail-closed)
        // when telemetry is on AND a Prometheus handle was wired through
        // from the CLI. Any failure here is logged and swallowed — the
        // gateway main listener stays healthy (fail-soft invariant of GAR-384).
        #[cfg(feature = "telemetry")]
        if let Some(tcfg) = self.telemetry_config.as_ref() {
            let auth_cfg = crate::metrics_auth::MetricsAuthConfig::from_telemetry_raw(
                tcfg.metrics_token.as_deref(),
                &tcfg.metrics_allowlist,
            );
            info!(
                mode = auth_cfg.describe_mode(),
                "metrics auth configured for embedded /metrics route"
            );
            state.set_metrics_auth_cfg(auth_cfg.clone());

            if tcfg.metrics_enabled {
                if let Some(handle) = self.metrics_handle.clone() {
                    match tcfg.metrics_bind.parse::<std::net::SocketAddr>() {
                        Ok(bind) => {
                            match crate::metrics_exporter::spawn_dedicated_metrics_listener(
                                auth_cfg, bind, handle,
                            )
                            .await
                            {
                                Ok(addr) => info!(
                                    addr = %addr,
                                    "dedicated /metrics listener spawned (plan 0024)"
                                ),
                                Err(e) => warn!(
                                    error = %e,
                                    "dedicated /metrics listener disabled; gateway continues"
                                ),
                            }
                        }
                        Err(e) => warn!(
                            bind = %tcfg.metrics_bind,
                            error = %e,
                            "invalid GARRAIA_METRICS_BIND; dedicated listener disabled"
                        ),
                    }
                } else {
                    warn!(
                        "metrics enabled but no PrometheusHandle wired; skipping dedicated listener"
                    );
                }
            }
        } else {
            // `telemetry_config` is `None` only when the CLI didn't wire
            // it through (future test harness, edge configs). Embedded
            // `/metrics` keeps the loopback-only default on `AppState`.
            #[cfg(feature = "telemetry")]
            info!("no TelemetryConfig wired into GatewayServer; metrics auth defaults used");
        }

        // Initialize health cache before wrapping state in Arc
        let health_cache = crate::health::new_health_cache();
        state.health_cache = Some(health_cache.clone());

        let state = Arc::new(state);

        // Issue #921: the proactive-send tool needs `AppState.channels` and the
        // session store, so it can only be built once the state is shared —
        // which is exactly why it lives in this crate and not in
        // `garraia-agents`. Registering it here is possible at all because
        // #924 made `register_tool` take `&self`; before that, this point was
        // past the `Arc::get_mut` window.
        //
        // Registered unconditionally: the tool itself reports "canal telegram
        // não está registrado" when there is no adapter, which is a better
        // answer for the model than the tool silently not existing. It is what
        // the reporter hit — the agent said it had no way to send, and could
        // not tell whether that was configuration or capability.
        {
            // Diagnostic only: the tool re-reads the allowlist from
            // `current_config()` on every call, so a change takes effect
            // without a gateway restart (security audit finding, LOW).
            let targets = crate::channel_send::ProactiveTargets::from_config(&state.config);
            if !targets.is_empty() {
                info!(
                    chats = targets.len(),
                    "telegram_send: explicit-target allowlist configured"
                );
            }
            state
                .agents
                .register_tool(Box::new(crate::tools::TelegramSendTool::new(&state)));
        }

        // Spawn background tasks
        state.spawn_session_cleanup();
        state.spawn_token_cleanup(); // GAR-202
        state.spawn_config_applier();

        // ── Boot-time health checks ──────────────────────────────────────
        {
            let boot_results = crate::health::run_all_checks(&state).await;
            crate::health::format_boot_table(&boot_results);

            // Seed cache with boot results
            {
                let mut w = health_cache.write().await;
                *w = boot_results;
            }

            // Start periodic background health checks (every 60s)
            crate::health::spawn_periodic_checks(Arc::clone(&state), health_cache);
        }

        // Spawn MCP health monitor for auto-reconnect. Issue #924: it now
        // carries the runtime, so a reconnect also restores the server's tools
        // into `AgentRuntime` instead of only into `McpManager::connections`.
        mcp_manager_arc.spawn_health_monitor_with_runtime(Some(Arc::clone(&state.agents)));

        // Spawn log file tailer for WebSocket streaming
        let log_tx = state.log_tx.clone();
        tokio::spawn(async move {
            use std::io::SeekFrom;
            use tokio::io::{AsyncBufReadExt, AsyncSeekExt, BufReader};
            let Some(path) = dirs::home_dir().map(|h| h.join(".garraia").join("garraia.log"))
            else {
                tracing::warn!("could not determine home directory; log tailer disabled");
                return;
            };

            while !path.exists() {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }

            if let Ok(mut file) = tokio::fs::File::open(&path).await {
                let _ = file.seek(SeekFrom::End(0)).await;
                let mut reader = BufReader::new(file);
                let mut line = String::new();

                loop {
                    line.clear();
                    match reader.read_line(&mut line).await {
                        Ok(0) => {
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                            // Clear EOF state to keep reading new lines when appended
                            let mut f = reader.into_inner();
                            let _ = f.seek(SeekFrom::Current(0)).await;
                            reader = BufReader::new(f);
                        }
                        Ok(_) => {
                            let text = line.trim_end();
                            if !text.is_empty() {
                                let _ = log_tx.send(serde_json::json!({
                                    "type": "log",
                                    "level": "INFO",
                                    "message": text
                                }));
                            }
                        }
                        Err(_) => {
                            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        }
                    }
                }
            }
        });

        // Start configured Discord channels
        let discord_channels = build_discord_channels(&state.config, &state);
        for mut channel in discord_channels {
            if let Err(e) = channel.connect().await {
                warn!("discord channel failed to connect: {e}");
            } else {
                state.channels.write().await.register(channel);
            }
        }

        // Start background scheduler loop
        let scheduler_state = Arc::clone(&state);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                if let Err(e) = run_scheduler(&scheduler_state).await {
                    tracing::error!("Scheduler error: {e}");
                }
            }
        });

        // Start configured Telegram channels
        let telegram_channels = build_telegram_channels(&state.config, &state);
        for mut channel in telegram_channels {
            if let Err(e) = channel.connect().await {
                warn!("telegram channel failed to connect: {e}");
            } else {
                state.channels.write().await.register(channel);
            }
        }

        // Start configured Slack channels
        let slack_channels = build_slack_channels(&state.config, &state);
        for mut channel in slack_channels {
            if let Err(e) = channel.connect().await {
                warn!("slack channel failed to connect: {e}");
            } else {
                state.channels.write().await.register(channel);
            }
        }

        // Start configured iMessage channels (macOS only)
        #[cfg(target_os = "macos")]
        {
            let imessage_channels = build_imessage_channels(&state.config, &state);
            for mut channel in imessage_channels {
                if let Err(e) = channel.connect().await {
                    warn!("imessage channel failed to connect: {e}");
                } else {
                    state.channels.write().await.register(channel);
                }
            }
        }

        // Build WhatsApp channels (webhook-driven — no persistent connection)
        let whatsapp_channels = build_whatsapp_channels(&state.config, &state);
        for channel in &whatsapp_channels {
            info!(
                "whatsapp channel ready (webhook mode, phone_number_id={})",
                channel.phone_number_id()
            );
        }
        let whatsapp_state: garraia_channels::whatsapp::webhook::WhatsAppState =
            Arc::new(whatsapp_channels);

        // Initialize admin store for the web admin console
        let admin_db_path = data_dir.join("admin.db");
        let mut admin_store_owned = match admin::store::AdminStore::open(&admin_db_path) {
            Ok(store) => {
                info!("admin store opened at {}", admin_db_path.display());
                store
            }
            Err(e) => {
                warn!("failed to open admin store, using in-memory: {e}");
                admin::store::AdminStore::in_memory().expect("in-memory admin store should work")
            }
        };

        // Resolve the admin master key exactly once, here, while we still hold
        // the store exclusively. This also runs the forward-only re-key: until
        // 2026-08-29 the key was derived with a constant PBKDF2 salt shared by
        // every installation. Fail-soft — on error the deployment stays on the
        // legacy key and the next boot retries. See admin/shared.rs.
        let admin_encryption_key = Arc::new(admin::handlers::resolve_admin_encryption_key(
            &mut admin_store_owned,
        ));
        let admin_store = Arc::new(Mutex::new(admin_store_owned));

        // Spawn periodic cleanup of expired admin sessions
        {
            let store = Arc::clone(&admin_store);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
                loop {
                    interval.tick().await;
                    let guard = store.lock().await;
                    let cleaned = guard.cleanup_expired_sessions();
                    if cleaned > 0 {
                        tracing::debug!("cleaned {cleaned} expired admin sessions");
                    }
                }
            });
        }

        let state_for_shutdown = Arc::clone(&state);
        let app = build_router(state, whatsapp_state, admin_store, admin_encryption_key);

        // TLS support: if cert + key paths are configured and tls feature is enabled,
        // use axum-server with rustls. Otherwise, plain HTTP.
        let use_tls = tls_cert.is_some() && tls_key.is_some();

        // Each branch yields a Result instead of `?`-ing out: the MCP/channel
        // cleanup below must run even when serving fails, otherwise the child
        // processes spawned above are orphaned.
        let serve_result: Result<()> = if use_tls {
            #[cfg(feature = "tls")]
            {
                serve_tls(&addr, tls_cert.as_deref(), tls_key.as_deref(), app).await
            }
            #[cfg(not(feature = "tls"))]
            {
                warn!(
                    "TLS cert/key configured but 'tls' feature not enabled — falling back to HTTP"
                );
                serve_plain(&addr, app).await
            }
        } else {
            serve_plain(&addr, app).await
        };

        // Cleanup runs even when the listener errored out: this block used to
        // sit after a `?`, so any serve error skipped it and orphaned every
        // MCP child process. The serve result is propagated at the end.
        //
        // Bounded: `disconnect_all` cancels each service, and a server that
        // ignores stdin EOF can take up to its own drain time; unbounded and
        // sequential, N bad servers could hang shutdown indefinitely.
        if let Some(ref manager) = state_for_shutdown.mcp_manager_arc {
            info!("disconnecting MCP servers...");
            if tokio::time::timeout(MCP_SHUTDOWN_TIMEOUT, manager.disconnect_all())
                .await
                .is_err()
            {
                warn!(
                    "MCP disconnect exceeded {:?}; continuing shutdown (children are killed on drop)",
                    MCP_SHUTDOWN_TIMEOUT
                );
            }
        }

        info!("disconnecting channels...");
        state_for_shutdown
            .channels
            .write()
            .await
            .disconnect_all()
            .await
            .ok();

        serve_result?;

        info!("gateway shut down gracefully");
        Ok(())
    }
}

/// **Test-only router builder** — plan 0016 M2-T2.
///
/// Thin wrapper around the production `router::build_router` that
/// skips the filesystem / bootstrap side effects of the normal
/// `GatewayServer::run` path:
///
/// * No agent runtime compilation from config (`AgentRuntime::new()`)
/// * No channel bootstrap (`ChannelRegistry::new()`)
/// * No MCP filesystem I/O (the harness sets `GARRAIA_CONFIG_DIR`
///   to a tempdir before calling this so `McpPersistenceService`
///   cannot pollute the developer's real config directory)
/// * No admin sqlite file (`AdminStore::in_memory()`)
/// * No TLS, no graceful shutdown listener
///
/// The caller injects pre-built `Arc<LoginPool>`, `Arc<SignupPool>`,
/// `Arc<JwtIssuer>` and optional `Arc<AppPool>` — all produced by
/// the integration test harness against a testcontainer Postgres.
///
/// Returned `Router` is ready for `tower::ServiceExt::oneshot`
/// without any socket binding.
///
/// Gated behind `#[cfg(feature = "test-helpers")]` so the function
/// is invisible to production builds. MUST NOT be called from any
/// non-test code path.
#[cfg(feature = "test-helpers")]
pub async fn build_router_for_test(
    config: AppConfig,
    login_pool: Arc<garraia_auth::LoginPool>,
    signup_pool: Arc<garraia_auth::SignupPool>,
    jwt_issuer: Arc<garraia_auth::JwtIssuer>,
    app_pool: Option<Arc<garraia_auth::AppPool>>,
) -> axum::Router {
    build_router_for_test_with_storage(
        config,
        login_pool,
        signup_pool,
        jwt_issuer,
        app_pool,
        None,
        None,
    )
    .await
}

/// Plan 0044 (GAR-395 slice 2) variant: lets the tus-upload test
/// harness inject a pre-built ObjectStore + UploadStaging alongside
/// the auth wiring, so the PATCH handler has something to commit to
/// without relying on `build_storage_wiring`'s env-var path.
#[cfg(feature = "test-helpers")]
pub async fn build_router_for_test_with_storage(
    config: AppConfig,
    login_pool: Arc<garraia_auth::LoginPool>,
    signup_pool: Arc<garraia_auth::SignupPool>,
    jwt_issuer: Arc<garraia_auth::JwtIssuer>,
    app_pool: Option<Arc<garraia_auth::AppPool>>,
    object_store: Option<Arc<dyn garraia_storage::ObjectStore>>,
    upload_staging: Option<Arc<crate::rest_v1::uploads::UploadStaging>>,
) -> axum::Router {
    use crate::admin;
    use garraia_agents::AgentRuntime;
    use garraia_channels::ChannelRegistry;

    // Zero-config minimal runtime + channel registry. No providers,
    // no tools, no channels wired. Confirmed by team-coordinator gate
    // against state.rs unit tests.
    let mut state = AppState::new(
        config,
        Arc::new(AgentRuntime::new()),
        ChannelRegistry::new(),
    );

    // Inject the auth pieces the harness built against testcontainer.
    state.set_auth_components(login_pool, signup_pool, jwt_issuer, app_pool);
    state.set_storage_components(object_store, upload_staging);
    let state: crate::state::SharedState = Arc::new(state);

    // Minimal collaborators expected by the production router.
    let whatsapp_state: garraia_channels::whatsapp::webhook::WhatsAppState = Arc::new(Vec::new());
    let mut admin_store_owned =
        admin::store::AdminStore::in_memory().expect("in-memory admin store should work");
    let admin_encryption_key = Arc::new(admin::handlers::resolve_admin_encryption_key(
        &mut admin_store_owned,
    ));
    let admin_store = Arc::new(Mutex::new(admin_store_owned));

    build_router(state, whatsapp_state, admin_store, admin_encryption_key)
}

/// Upper bound for cancelling MCP services at shutdown. A server that ignores
/// stdin EOF must not hold the gateway open forever; children are killed when
/// their transport is dropped regardless.
const MCP_SHUTDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Serve plain HTTP until the shutdown signal.
async fn serve_plain(addr: &str, app: axum::Router) -> Result<()> {
    let listener = TcpListener::bind(addr).await?;
    info!("GarraIA gateway listening on http://{}", addr);
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .map_err(|e| garraia_common::Error::Gateway(format!("server error: {e}")))
}

/// Serve HTTPS (rustls) until the process is signalled.
#[cfg(feature = "tls")]
async fn serve_tls(
    addr: &str,
    cert_path: Option<&str>,
    key_path: Option<&str>,
    app: axum::Router,
) -> Result<()> {
    let (Some(cert_path), Some(key_path)) = (cert_path, key_path) else {
        return Err(garraia_common::Error::Gateway(
            "TLS requested without cert/key paths".to_string(),
        ));
    };
    info!("TLS enabled: cert={}, key={}", cert_path, key_path);
    let tls_config = axum_server::tls_rustls::RustlsConfig::from_pem_file(cert_path, key_path)
        .await
        .map_err(|e| garraia_common::Error::Gateway(format!("TLS config error: {e}")))?;
    let sock_addr: std::net::SocketAddr = addr
        .parse()
        .map_err(|e| garraia_common::Error::Gateway(format!("invalid addr: {e}")))?;
    info!("GarraIA gateway listening on https://{}", sock_addr);
    axum_server::bind_rustls(sock_addr, tls_config)
        .serve(app.into_make_service_with_connect_info::<std::net::SocketAddr>())
        .await
        .map_err(|e| garraia_common::Error::Gateway(format!("server error: {e}")))
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => info!("received Ctrl+C, shutting down"),
        () = terminate => info!("received SIGTERM, shutting down"),
    }
}
/// How many times one occurrence is retried before being given up on.
const MAX_TASK_ATTEMPTS: i64 = 3;

async fn run_scheduler(state: &AppState) -> Result<()> {
    let store_mutex = match &state.session_store {
        Some(s) => s,
        None => return Ok(()),
    };

    let tasks = {
        let store = store_mutex.lock().await;
        store.poll_due_tasks()?
    };

    if tasks.is_empty() {
        return Ok(());
    }

    info!("scheduler executing {} due tasks", tasks.len());

    for task in tasks {
        if let Err(e) = execute_scheduled_task(state, store_mutex, &task).await {
            // A failure used to be terminal: one transient provider error
            // silently killed the task (and, with recurrence, the whole
            // schedule). Retry with quadratic backoff first.
            let store = store_mutex.lock().await;
            match store.retry_or_fail_task(&task, MAX_TASK_ATTEMPTS, chrono::Utc::now()) {
                Ok(Some(retry_at)) => tracing::warn!(
                    task = %task.id,
                    retry_at = %retry_at.to_rfc3339(),
                    "scheduled task failed: {e} — will retry"
                ),
                Ok(None) => tracing::error!(
                    task = %task.id,
                    "scheduled task failed after {MAX_TASK_ATTEMPTS} attempts: {e} — giving up"
                ),
                Err(fe) => {
                    tracing::error!("Failed to record failure for task {}: {fe}", task.id)
                }
            }
        }
    }
    Ok(())
}

async fn execute_scheduled_task(
    state: &AppState,
    store_mutex: &Arc<Mutex<SessionStore>>,
    task: &garraia_db::ScheduledTask,
) -> Result<()> {
    let channel_type = &task.channel_id;

    let message = Message {
        id: uuid::Uuid::new_v4().to_string(),
        session_id: SessionId::from_string(&task.session_id),
        channel_id: ChannelId::from_string(channel_type),
        user_id: UserId::from_string(&task.user_id),
        direction: MessageDirection::Incoming,
        content: MessageContent::System(task.payload.clone()),
        timestamp: chrono::Utc::now(),
        metadata: task.session_metadata.clone(),
    };

    // 1. Persist system message to history so agent has context
    {
        let store = store_mutex.lock().await;
        store.append_message(
            &task.session_id,
            "system",
            &task.payload,
            message.timestamp,
            &task.session_metadata,
        )?;
    }

    // 2. Hydrate session history and invoke agent runtime
    state
        .hydrate_session_history(
            &task.session_id,
            Some(channel_type.as_str()),
            Some(task.user_id.as_str()),
        )
        .await;

    let history = state.session_history(&task.session_id);
    let continuity_key = state
        .continuity_key(Some(task.user_id.as_str()))
        .map(|k| k.as_str().to_string());

    let response_text = state
        .agents
        .process_heartbeat(
            &task.session_id,
            &task.payload,
            &history,
            continuity_key.as_deref(),
            Some(task.user_id.as_str()),
        )
        .await?;

    // Issue #921: `task.session_metadata` is written by `AppState::persist_turn`
    // as `{}` or `{"continuity_key": …}` — it never carried a channel address,
    // so step 4 below has been failing on every scheduled Telegram task with
    // "missing telegram_chat_id in metadata", swallowed by a `tracing::error!`.
    // The address lives in `chat_session_keys`, written by `resolve_session`;
    // this is the first code that reads it back.
    let external_id = match (&state.chat_session_manager, channel_type.as_str()) {
        (Some(mgr), "telegram") => mgr
            .external_key_for(&task.session_id, garraia_db::ChatSource::Telegram)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(
                    session = %task.session_id,
                    error = %e,
                    "scheduled task: failed to resolve channel address"
                );
                None
            }),
        _ => None,
    };
    let outgoing_metadata = crate::channel_send::with_channel_address(
        &task.session_metadata,
        channel_type,
        external_id.as_deref(),
    );

    let response_msg = Message {
        id: uuid::Uuid::new_v4().to_string(),
        session_id: SessionId::from_string(&task.session_id),
        channel_id: ChannelId::from_string(channel_type),
        user_id: UserId::from_string("genesis"),
        direction: MessageDirection::Outgoing,
        content: MessageContent::Text(response_text.clone()),
        timestamp: chrono::Utc::now(),
        metadata: outgoing_metadata,
    };

    // 3. Persist assistant response regardless of outbound channel availability.
    {
        let store = store_mutex.lock().await;
        store.append_message(
            &task.session_id,
            "assistant",
            &response_text,
            response_msg.timestamp,
            &task.session_metadata,
        )?;
    }

    // 4. Best-effort delivery to channel adapter.
    if let Some(channel) = state.channels.read().await.get(channel_type.as_str()) {
        if let Err(e) = channel.send_message(&response_msg).await {
            // Issue #921: this used to fire on *every* scheduled Telegram task
            // and nobody noticed, because the response was already persisted
            // above and the task counted as done. Log the address resolution
            // alongside the error so the next failure is diagnosable.
            tracing::error!(
                session = %task.session_id,
                channel = %channel_type,
                addressed = external_id.is_some(),
                "Failed to send scheduled response: {e}"
            );
        }
    } else {
        tracing::warn!(
            "Scheduled response persisted but no channel adapter registered for: {}",
            channel_type
        );
    }

    // 5. Complete the task — or arm the next occurrence when recurring.
    {
        let store = store_mutex.lock().await;
        if task.is_recurring() {
            match store.complete_recurring_run(task, chrono::Utc::now())? {
                Some(next) => info!(
                    task = %task.id,
                    next_run = %next.to_rfc3339(),
                    "recurring task rescheduled"
                ),
                None => info!(task = %task.id, "recurring task reached max_runs — completed"),
            }
        } else {
            store.complete_task(&task.id)?;
        }
    }

    Ok(())
}

/// Plan 0044 (GAR-395 slice 2): construct the ObjectStore backend +
/// UploadStaging from the live config. Fail-soft: any misconfiguration
/// yields `(None, None)` plus a WARN log — the tus PATCH path then
/// answers 503.
///
/// This is intentionally in `server.rs` (not `bootstrap.rs`) because
/// it depends on `AppState` layering and carries feature-gated
/// branches. Moving it later is trivial.
async fn build_storage_wiring(
    config: &AppConfig,
) -> (
    Option<Arc<dyn garraia_storage::ObjectStore>>,
    Option<Arc<crate::rest_v1::uploads::UploadStaging>>,
) {
    use garraia_config::StorageBackend;

    // Resolve the default data_dir once so both LocalFs root and
    // staging_dir share the same prefix when operator leaves them
    // unset.
    let data_dir_fallback = config
        .data_dir
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("./data"));

    let staging_dir = config
        .storage
        .staging_dir
        .clone()
        .unwrap_or_else(|| data_dir_fallback.join("uploads-staging"));

    if let Err(e) = tokio::fs::create_dir_all(&staging_dir).await {
        warn!(
            error = %e,
            staging_dir = %staging_dir.display(),
            "failed to create tus staging directory; PATCH will answer 503"
        );
        return (None, None);
    }

    // Canonicalize so the traversal-safety argument in
    // `uploads::UploadStaging::staging_path` holds end-to-end.
    // std::fs::canonicalize is sync; wrap in spawn_blocking to avoid
    // blocking the runtime.
    let staging_dir_owned = staging_dir.clone();
    let staging_dir_canonical = match tokio::task::spawn_blocking(move || {
        std::fs::canonicalize(&staging_dir_owned)
    })
    .await
    {
        Ok(Ok(p)) => p,
        Ok(Err(e)) => {
            warn!(
                error = %e,
                staging_dir = %staging_dir.display(),
                "failed to canonicalize tus staging directory; PATCH will answer 503"
            );
            return (None, None);
        }
        Err(e) => {
            warn!(
                error = %e,
                "spawn_blocking failed during staging dir canonicalization"
            );
            return (None, None);
        }
    };

    // Plan 0044 §5.6: fail-closed HMAC secret. `GARRAIA_UPLOAD_HMAC_SECRET`
    // must be set (≥32 bytes decoded). Without it, commit aborts
    // because `file_versions.integrity_hmac` is NOT NULL.
    let hmac_secret = match std::env::var("GARRAIA_UPLOAD_HMAC_SECRET") {
        Ok(s) if s.len() >= 32 => s.as_bytes().to_vec(),
        Ok(_) => {
            warn!(
                "GARRAIA_UPLOAD_HMAC_SECRET must be at least 32 bytes; tus PATCH commit will answer 503"
            );
            return (None, None);
        }
        Err(_) => {
            info!(
                "GARRAIA_UPLOAD_HMAC_SECRET not set; tus PATCH commit will answer 503 (expected in dev)"
            );
            return (None, None);
        }
    };

    // Sanity: cap is operator-provided — clamp via the config check
    // range defensively (bootstrap path trusts config but belt +
    // suspenders).
    let max_patch_bytes = config.storage.max_patch_bytes.clamp(
        garraia_config::MAX_PATCH_BYTES_MIN,
        garraia_config::MAX_PATCH_BYTES_MAX,
    );

    let staging = Arc::new(crate::rest_v1::uploads::UploadStaging {
        staging_dir: staging_dir_canonical,
        max_patch_bytes,
        hmac_secret,
    });

    let object_store: Option<Arc<dyn garraia_storage::ObjectStore>> = match config.storage.backend {
        StorageBackend::Local => {
            let root = config
                .storage
                .local_fs_root
                .clone()
                .unwrap_or_else(|| data_dir_fallback.join("storage"));
            if let Err(e) = tokio::fs::create_dir_all(&root).await {
                warn!(
                    error = %e,
                    root = %root.display(),
                    "failed to create LocalFs root; tus PATCH will answer 503"
                );
                return (None, None);
            }
            match garraia_storage::LocalFs::new(&root) {
                Ok(fs) => {
                    info!(
                        root = %root.display(),
                        "ObjectStore wired (LocalFs)"
                    );
                    Some(Arc::new(fs))
                }
                Err(e) => {
                    warn!(error = %e, "LocalFs construction failed; tus PATCH will answer 503");
                    return (None, None);
                }
            }
        }
        StorageBackend::S3 => {
            // Gated behind the `storage-s3` feature. Without it, we
            // cannot construct `S3Compatible` — return None with a
            // clear log so the operator knows why.
            #[cfg(feature = "storage-s3")]
            {
                let s3 = match config.storage.s3.as_ref() {
                    Some(s) => s,
                    None => {
                        warn!(
                            "storage.backend = s3 but [storage.s3] block missing; PATCH will answer 503"
                        );
                        return (None, None);
                    }
                };
                let bucket = s3.bucket.clone().unwrap_or_default();
                let region = s3.region.clone().unwrap_or_default();
                if bucket.trim().is_empty() || region.trim().is_empty() {
                    warn!("storage.s3.bucket or region missing; PATCH will answer 503");
                    return (None, None);
                }
                // Build credentials from explicit access/secret if
                // both set; otherwise let `aws-config` use the
                // env/IAM-role default provider chain.
                let credentials = match (&s3.access_key, &s3.secret_key) {
                    (Some(ak), Some(sk)) => Some(aws_credential_types::Credentials::new(
                        ak.clone(),
                        sk.clone(),
                        None,
                        None,
                        "garraia-gateway",
                    )),
                    _ => None,
                };
                let s3_cfg = garraia_storage::S3Config {
                    bucket,
                    region,
                    endpoint_url: s3.endpoint.clone(),
                    force_path_style: s3.endpoint.is_some(),
                    credentials,
                };
                match garraia_storage::S3Compatible::new(s3_cfg).await {
                    Ok(s3c) => {
                        info!("ObjectStore wired (S3Compatible)");
                        Some(Arc::new(s3c))
                    }
                    Err(e) => {
                        warn!(error = %e, "S3Compatible construction failed; PATCH will answer 503");
                        return (None, None);
                    }
                }
            }
            #[cfg(not(feature = "storage-s3"))]
            {
                warn!(
                    "storage.backend = s3 but garraia-gateway was built without `storage-s3` feature; \
                     tus PATCH will answer 503"
                );
                return (None, None);
            }
        }
        StorageBackend::None => {
            info!("storage.backend = none; tus PATCH will answer 503");
            return (None, None);
        }
    };

    (object_store, Some(staging))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O flag ligado tem de impedir o boot, e a mensagem tem de nomear a
    /// saída — a alternativa (subir com warning) manteria a mentira viva para
    /// quem não lê log.
    #[test]
    fn inert_auth_flag_refuses_to_boot() {
        let mut config = AppConfig::default();
        config.gateway.session_tokens_required = true;

        let err = refuse_inert_auth_flag(&config).expect_err("flag ligado deve recusar");
        let msg = err.to_string();
        assert!(msg.contains("session_tokens_required"), "msg = {msg}");
        assert!(msg.contains("NOT implemented"), "msg = {msg}");
        // A saída de uma linha tem de estar na própria mensagem.
        assert!(msg.contains("remove the flag"), "msg = {msg}");
    }

    /// Quem roda hoje (default `false`) não pode ser afetado.
    #[test]
    fn default_config_boots() {
        assert!(refuse_inert_auth_flag(&AppConfig::default()).is_ok());
    }
}
