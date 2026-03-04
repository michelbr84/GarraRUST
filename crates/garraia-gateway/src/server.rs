use std::sync::Arc;

use garraia_common::{
    ChannelId, Message, MessageContent, MessageDirection, Result, SessionId, UserId,
};
use garraia_config::{AppConfig, ConfigWatcher};
use garraia_db::{ChatSessionManager, SessionStore};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tracing::{info, warn};

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
}

impl GatewayServer {
    pub fn new(config: AppConfig) -> Self {
        Self { config }
    }

    pub async fn run(self) -> Result<()> {
        let addr = format!("{}:{}", self.config.gateway.host, self.config.gateway.port);

        let mut agents = build_agent_runtime(&self.config);

        // Connect MCP servers and register their tools
        let (mcp_manager, mcp_tools) = build_mcp_tools(&self.config).await;
        let mcp_tool_count = mcp_tools.len();
        let mcp_tool_names: Vec<String> = mcp_tools.iter().map(|t| t.name().to_string()).collect();
        for tool in mcp_tools {
            agents.register_tool(tool);
        }
        if mcp_tool_count > 0 {
            info!(
                mcp_tools = mcp_tool_count,
                tools = ?mcp_tool_names,
                "MCP tools registered into AgentRuntime"
            );
        } else {
            info!("no MCP tools registered (no MCP servers configured or connected)");
        }

        let channels = build_channels(&self.config).await;
        let mut state = AppState::new(self.config, agents, channels);
        state.mcp_manager = Some(mcp_manager);

        // Sync MCP registry with live manager state (populates Running/Stopped statuses).
        state.mcp_registry.sync_from_manager(
            state.mcp_manager.as_ref().unwrap()
        ).await;

        // Register MCP tools as slash commands (must be done before Arc-wrapping)
        state.register_mcp_tools().await;

        // Register built-in commands (must be done before Telegram channels are created)
        crate::commands::register_commands(&mut state.command_registry.write().unwrap());

        // Initialize Chatterbox TTS client if voice is enabled
        if state.config.voice.enabled {
            let voice_client = garraia_voice::ChatterboxClient::new(
                &state.config.voice.tts_endpoint,
                &state.config.voice.language,
            );
            match voice_client.health_check().await {
                Ok(true) => {
                    info!(
                        "🔊 Voice mode enabled — Chatterbox TTS at {}",
                        state.config.voice.tts_endpoint
                    );
                    state.voice_client = Some(Arc::new(voice_client));
                }
                Ok(false) => {
                    warn!(
                        "⚠️  Voice mode requested but Chatterbox is not reachable at {}",
                        state.config.voice.tts_endpoint
                    );
                    // Still store the client so it can be used once server comes up
                    state.voice_client = Some(Arc::new(voice_client));
                }
                Err(e) => {
                    warn!("⚠️  Voice mode health check failed: {e}");
                    state.voice_client = Some(Arc::new(voice_client));
                }
            }

            // Initialize Whisper STT client
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
                state.set_chat_session_manager(Arc::new(ChatSessionManager::new(Arc::clone(&store))));
                state
                    .agents
                    .register_tool(Box::new(garraia_agents::ScheduleHeartbeat::new(store)));
                info!("session store opened at {}", sessions_db.display());
            }
            Err(e) => {
                warn!("failed to open session store: {e}");
            }
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

        // Wrap MCP manager in Arc for health monitor before moving into state
        let mcp_manager_arc = state.mcp_manager.take().map(Arc::new);
        if let Some(ref arc) = mcp_manager_arc {
            state.mcp_manager_arc = Some(Arc::clone(arc));
        }

        // Initialize health cache before wrapping state in Arc
        let health_cache = crate::health::new_health_cache();
        state.health_cache = Some(health_cache.clone());

        let state = Arc::new(state);

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

        // Spawn MCP health monitor for auto-reconnect
        if let Some(ref arc) = mcp_manager_arc {
            arc.spawn_health_monitor();
        }

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

        // Spawn MCP health monitor for auto-reconnect
        if let Some(ref arc) = mcp_manager_arc {
            arc.spawn_health_monitor();
        }

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
        let admin_store = match admin::store::AdminStore::open(&admin_db_path) {
            Ok(store) => {
                info!("admin store opened at {}", admin_db_path.display());
                Arc::new(Mutex::new(store))
            }
            Err(e) => {
                warn!("failed to open admin store, using in-memory: {e}");
                Arc::new(Mutex::new(
                    admin::store::AdminStore::in_memory()
                        .expect("in-memory admin store should work"),
                ))
            }
        };

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
        let app = build_router(state, whatsapp_state, admin_store);

        let listener = TcpListener::bind(&addr).await?;
        info!("GarraIA gateway listening on {}", addr);

        // Graceful shutdown on Ctrl-C / SIGTERM.
        // `into_make_service_with_connect_info` injects ConnectInfo<SocketAddr>
        // so that the rate-limiter can extract per-client IP addresses.
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|e| garraia_common::Error::Gateway(format!("server error: {e}")))?;

        // Disconnect MCP servers on shutdown
        if let Some(ref manager) = state_for_shutdown.mcp_manager_arc {
            info!("disconnecting MCP servers...");
            manager.disconnect_all().await;
        }

        info!("disconnecting channels...");
        state_for_shutdown
            .channels
            .write()
            .await
            .disconnect_all()
            .await
            .ok();

        info!("gateway shut down gracefully");
        Ok(())
    }
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
            tracing::error!("Scheduled task {} failed: {e} — marking as failed", task.id);
            let store = store_mutex.lock().await;
            if let Err(fe) = store.fail_task(&task.id) {
                tracing::error!("Failed to mark task {} as failed: {fe}", task.id);
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

    let response_msg = Message {
        id: uuid::Uuid::new_v4().to_string(),
        session_id: SessionId::from_string(&task.session_id),
        channel_id: ChannelId::from_string(channel_type),
        user_id: UserId::from_string("genesis"),
        direction: MessageDirection::Outgoing,
        content: MessageContent::Text(response_text.clone()),
        timestamp: chrono::Utc::now(),
        metadata: task.session_metadata.clone(),
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
            tracing::error!("Failed to send scheduled response: {e}");
        }
    } else {
        tracing::warn!(
            "Scheduled response persisted but no channel adapter registered for: {}",
            channel_type
        );
    }

    // 5. Complete task
    {
        let store = store_mutex.lock().await;
        store.complete_task(&task.id)?;
    }

    Ok(())
}
