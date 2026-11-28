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
    build_agent_runtime, build_channels, build_discord_channels, build_google_chat_channels,
    build_irc_channels, build_line_channels, build_matrix_channels, build_mcp_tools,
    build_openclaw_config, build_signal_channels, build_slack_channels, build_teams_channels,
    build_telegram_channels, build_whatsapp_channels, spawn_openclaw_router,
    warn_if_embeddings_unhealthy,
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

/// Keep trying to connect a channel that failed at boot (issue #928).
///
/// The asymmetry this fixes: once connected, the Telegram polling loop
/// already survives network loss — teloxide retries `getUpdates` forever
/// with 1s→64s exponential backoff. Only the *first* connect had no second
/// chance, so a transient failure in exactly the wrong five seconds left
/// the channel silent until a human restarted the gateway. Slack, OpenClaw
/// and iMessage all reconnect; Telegram was the odd one out.
///
/// Retries forever, like the polling loop it hands over to: the operator's
/// intent in configuring the channel is "be on Telegram when the network
/// allows", and there is no correct number of failures after which that
/// intent expires. The cap keeps the pressure constant (one attempt per
/// minute) instead of growing unbounded.
fn spawn_channel_connect_retry(
    state: Arc<AppState>,
    mut channel: Box<dyn garraia_channels::Channel>,
) {
    tokio::spawn(async move {
        let mut attempt: u32 = 0;
        loop {
            let delay = channel_retry_delay(attempt);
            tokio::time::sleep(delay).await;
            match channel.connect().await {
                Ok(()) => {
                    info!(
                        channel = channel.channel_type(),
                        attempts = attempt + 1,
                        "channel connected after boot-time retry"
                    );
                    state.channels.write().await.register(channel);
                    return;
                }
                Err(e) => {
                    warn!(
                        channel = channel.channel_type(),
                        attempt = attempt + 1,
                        next_retry_secs = channel_retry_delay(attempt + 1).as_secs(),
                        "channel reconnect failed: {e}"
                    );
                    attempt = attempt.saturating_add(1);
                }
            }
        }
    });
}

/// Backoff for [`spawn_channel_connect_retry`]: 2s, 4s, 8s, … capped at 60s.
///
/// Same shape as the OpenClaw client's reconnect loop
/// (`openclaw/client.rs`), which is the most complete of the three channel
/// reconnect implementations. Pure so the boundary cases (growth, the cap,
/// and no overflow at absurd attempt counts) are testable without a clock.
fn channel_retry_delay(attempt: u32) -> std::time::Duration {
    const BASE_SECS: u64 = 2;
    const CAP_SECS: u64 = 60;
    let exp = 2u64.saturating_pow(attempt.min(32));
    std::time::Duration::from_secs(BASE_SECS.saturating_mul(exp).min(CAP_SECS))
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

/// Qual linha logar para o probe de STT (Whisper) no boot — funcao pura, sem
/// I/O e sem relogio, testavel no padrao de `channel_retry_delay` e do
/// `voice_check` do `diagnostics_handler`. O `run()` aplica o timeout e faz a
/// chamada de rede; aqui so se traduz o resultado em qual linha sai. Antes o
/// boot logava `🎙️  STT initialized` incondicionalmente, escondendo um
/// Whisper morto atras de uma linha de sucesso — ao contrario do TTS logo
/// acima, que ja era sondado.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SttBootLog {
    /// Whisper respondeu — `info!`.
    Ready,
    /// Whisper nao respondeu (`health_check` devolveu `Ok(false)`) ou o probe
    /// estourou o `timeouts.health` (`None`) — `warn!` "not reachable".
    Unreachable,
    /// O probe em si falhou com erro (`Err`) — `warn!` com o texto do erro.
    /// Hoje e um ramo DEFENSIVO: `WhisperClient::health_check()` converte erro
    /// de rede em `Ok(false)` (nunca devolve `Err`), entao este caso so dispara
    /// se essa implementacao passar a propagar erro — nao se perde o sinal.
    ProbeError(String),
}

/// `probe`: `Some(Ok(true|false))` do `health_check`, `Some(Err(_))` em erro de
/// probe, `None` quando o probe estourou o `timeouts.health`. Timeout e
/// `Ok(false)` sao o mesmo sinal para o operador — o servidor nao esta
/// alcancavel agora. O cliente segue registrado nos tres casos (o servidor
/// pode subir depois), igual ao caminho do TTS.
fn classify_stt_boot(probe: Option<std::result::Result<bool, String>>) -> SttBootLog {
    match probe {
        Some(Ok(true)) => SttBootLog::Ready,
        Some(Ok(false)) | None => SttBootLog::Unreachable,
        Some(Err(e)) => SttBootLog::ProbeError(e),
    }
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
        //
        // `from_path` e nao `dotenv()`: o `Finder` do `dotenv()` (e do
        // `from_filename`) recursa em `directory.parent()` ate achar um
        // `.env`, injetando variaveis de um diretorio pai na subida do
        // gateway — mesmo problema corrigido na CLI (main.rs). `from_path` e
        // `File::open` puro, restrito ao diretorio atual.
        if let Err(e) = dotenvy::from_path(".env") {
            tracing::debug!("no .env file loaded: {e}");
        }

        // Security finding from the #930 investigation: refuse to boot with a
        // hardening flag that does not do what its documentation promised.
        refuse_inert_auth_flag(&self.config)?;

        // #1261 (decisao A): bind nao-loopback sem credencial de gateway
        // RECUSA o boot, antes de qualquer MCP/canal subir e antes do bind.
        // Mesma regra da CLI (`garraia_config::bind`), para embedders que nao
        // passam pela CLI. TLS nao isenta.
        refuse_exposed_bind(&self.config.gateway)?;

        let addr = format!("{}:{}", self.config.gateway.host, self.config.gateway.port);
        // Copia da secao `gateway` para depois do `AppState::new`, que consome
        // a config. `serve_plain`/`serve_tls` recebem a CONFIG, e nao um bool
        // ja decidido aqui (#1241): enquanto existia o bool, um
        // `let api_key_ativa = true;` nesta linha desligava o aviso do boot
        // inteiro sem quebrar teste nenhum — a decisao agora e tomada no
        // ponto de uso, que tem teste de fiacao.
        let gateway_cfg = self.config.gateway.clone();
        let tls_cert = self.config.gateway.tls_cert_path.clone();
        let tls_key = self.config.gateway.tls_key_path.clone();

        // #1378: o workspace padrao das file tools nativas precisa EXISTIR
        // antes do runtime ser montado — `build_agent_runtime` canonicaliza as
        // raizes, e uma raiz que nao resolve e descartada. Fora de
        // `build_agent_runtime` de proposito: aquela funcao e chamada por duas
        // dezenas de testes unitarios com a config default, e criar diretorio
        // a partir dela plantaria `<config_dir>/data/workspace` no `$HOME` de
        // quem roda a suite. Um teste de fonte (`bootstrap::tests`) garante
        // que esta chamada continue aqui, e antes.
        crate::bootstrap::garantir_workspace_padrao(&self.config);
        let agents = build_agent_runtime(&self.config);
        // `build_agent_runtime` e sincrono; o health check do provider de
        // embeddings precisa de await, entao acontece aqui (#951).
        warn_if_embeddings_unhealthy(&agents).await;

        // Provision the default mcp.json BEFORE building tools. This used to
        // happen only inside AppState::new (below), i.e. after build_mcp_tools
        // had already read an absent file — so the very first boot always
        // came up with zero MCP tools.
        //
        // ADR 0024 (#1329): as raizes vem da politica de execucao (nunca
        // `$HOME`) — `agent.file_roots` / `<data_dir>/workspace` em
        // `standard`, `execution.pod_root` em `isolated-pod`.
        crate::mcp::McpPersistenceService::with_default_path().provision_filesystem_if_missing(
            &crate::bootstrap::raizes_do_mcp_filesystem(&self.config),
        );

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
        // reconnects). The health monitor and the admin API read the same Arc
        // from `AppState`, so it has to be installed here, before either of
        // them can observe the state — not ~300 lines below, where it used to
        // be created and where the earlier readers silently saw `None`.
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

            // Initialize Whisper STT client (works for both standalone Whisper
            // and LM Studio). Probe it the same way the TTS above is probed —
            // the old unconditional "STT initialized" line logged success even
            // with Whisper down, which is exactly how the owner (and the Garra
            // agent) were misled into thinking STT was live. The client stays
            // registered either way: the server may come up later, same as TTS.
            let stt_client = garraia_voice::WhisperClient::new(
                &state.config.voice.stt_endpoint,
                &state.config.voice.language,
            );
            let stt_endpoint = state.config.voice.stt_endpoint.clone();
            // `WhisperClient` builds a timeout-less reqwest client, so bound the
            // boot probe by `timeouts.health`: a silently-dropping host must not
            // hang the whole boot. `max(1)` stops a `0` config from meaning
            // "no probe at all".
            let health_budget =
                std::time::Duration::from_secs(state.config.timeouts.health.default_secs.max(1));
            let probe = tokio::time::timeout(health_budget, stt_client.health_check())
                .await
                .ok()
                .map(|r| r.map_err(|e| e.to_string()));
            match classify_stt_boot(probe) {
                SttBootLog::Ready => {
                    info!("🎙️  STT ready — Whisper at {stt_endpoint}");
                }
                SttBootLog::Unreachable => {
                    warn!(
                        "⚠️  Voice mode requested but Whisper STT is not reachable at {stt_endpoint}"
                    );
                }
                SttBootLog::ProbeError(e) => {
                    warn!("⚠️  Whisper STT health check failed: {e}");
                }
            }
            state.stt_client = Some(Arc::new(stt_client));
        }

        // Initialize persistent session storage used by channel memory bus hydration.
        // Fonte única (`AppConfig::resolved_data_dir`) — o fallback inline
        // `~/.garraia/data` divergia do `memory.db`/`admin.db`, que usam o
        // config dir XDG (`ConfigLoader::default_config_dir()/data`).
        let data_dir = state.config.resolved_data_dir();
        if let Err(e) = std::fs::create_dir_all(&data_dir) {
            warn!("failed to create data directory: {e}");
        }
        // Initialize admin store for the web admin console. An existing
        // `admin.db` that does not open refuses the boot instead of falling
        // back to an empty in-memory store, whose unauthenticated setup would
        // accept a new first admin (#2000). Same early spot as the key below,
        // so the refusal happens before any channel is up.
        let admin_db_path = data_dir.join("admin.db");
        let mut admin_store_owned = admin::store::AdminStore::open_for_boot(&admin_db_path)
            .map_err(garraia_common::Error::Gateway)?;
        info!("admin store ready ({})", admin_db_path.display());

        // Resolve the admin master key exactly once, here, while we still hold
        // the store exclusively. This also runs the forward-only re-key: until
        // 2026-08-29 the key was derived with a constant PBKDF2 salt shared by
        // every installation. A re-key that only fails to write is fail-soft —
        // the deployment stays on the legacy key and the next boot retries. A
        // key that cannot be handed out safely (recorded KDF parameters
        // unusable, no entropy, secrets the legacy key does not open) is
        // fail-closed for the admin panel only: the gateway boots and `/admin`
        // answers 503, with the reason in the log (#1992, #1999). See
        // admin/shared.rs.
        let admin_key = admin::handlers::resolve_admin_encryption_key(
            &mut admin_store_owned,
            &admin::handlers::default_admin_dir(),
        );
        let admin_store = Arc::new(Mutex::new(admin_store_owned));

        let sessions_db = data_dir.join("sessions.db");
        match SessionStore::open(&sessions_db) {
            Ok(store) => {
                // #1227 (slice 1): runs `running` deixados por uma queda
                // viram `interrupted` aqui, com ids no log — nunca `goal`
                // (PII). Falha e fail-soft: nao pode impedir a subida.
                garraia_db::agent_runs::log_interrupted_runs(&store);
                // #1227 (slice 2): tarefas agendadas que a queda deixou
                // `running` voltam a `pending` aqui, com id + attempts no
                // log — nunca `payload` (PII). Idem fail-soft.
                garraia_db::log_recovered_leases(&store);
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
        //
        // #1565: `purge_pool_opt` is threaded out of this match so the
        // account_purge_worker spawn site (below, next to the uploads
        // worker) can require BOTH pools. Built here because the URL
        // lives in AuthConfig.
        let mut purge_pool_opt: Option<Arc<garraia_auth::PurgePool>> = None;
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

                // #1565 / migration 034: optional PurgePool construction.
                // Only attempted when GARRAIA_PURGE_DATABASE_URL is set.
                // The worker connects as `garraia_purge` (EXECUTE-only on
                // the four SECURITY DEFINER purge functions); building it
                // from the app URL would defeat the role separation, so a
                // separate env var is mandatory. Absence degrades only the
                // automated purge — DELETE /v1/me still enqueues and the
                // manual procedure keeps working.
                purge_pool_opt = if let Some(purge_url) = auth_cfg.purge_database_url.as_ref() {
                    use garraia_auth::PurgeConfig;
                    match garraia_auth::PurgePool::from_dedicated_config(&PurgeConfig {
                        database_url: purge_url.expose_secret().to_string(),
                        max_connections: 5,
                    })
                    .await
                    {
                        Ok(p) => {
                            info!("garraia-auth PurgePool wired (garraia_purge role)");
                            Some(Arc::new(p))
                        }
                        Err(e) => {
                            // Same redaction path as the AppPool failure
                            // above: never let a connect error print the
                            // purge URL (it carries credentials).
                            let redacted = match e {
                                garraia_auth::AuthError::Storage(sqlx_err) => {
                                    garraia_auth::RedactedStorageError::from(sqlx_err).to_string()
                                }
                                other => other.to_string(),
                            };
                            warn!(
                                error = %redacted,
                                "PurgePool connect failed; account_purge_worker will not spawn"
                            );
                            None
                        }
                    }
                } else {
                    info!(
                        "GARRAIA_PURGE_DATABASE_URL not set; account_purge_worker will not spawn \
                         (DELETE /v1/me still enqueues; manual procedure in \
                         docs/legal/data-subject-requests.md remains)"
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

            // Apagamento definitivo de conta (LGPD art. 18, VI / GDPR art.
            // 17): executa os pedidos de `DELETE /v1/me` cuja carencia
            // venceu. Exige AS DUAS pools: `app_pool` para o audit (o
            // INSERT em audit_events passa pelo role de app sob RLS) e
            // `purge_pool` (garraia_purge, migration 034) para executar
            // as quatro funcoes SECURITY DEFINER — o app role nao tem
            // EXECUTE nelas. Sem a purge pool o worker nao nasce: o
            // pedido fica na fila e o procedimento manual de
            // `docs/legal/data-subject-requests.md` continua valendo.
            // Recebe o `ObjectStore` porque o apagamento tem de alcancar
            // o blob dos arquivos do titular, nao so a linha — sem backend
            // ligado o pedido fica aberto de proposito em vez de fechar
            // mentindo. Ver `account_purge_worker`.
            if let Some(purge_pool) = purge_pool_opt {
                let purge_handle = crate::account_purge_worker::spawn_account_purge_worker(
                    purge_pool,
                    Arc::clone(&app_pool),
                    state.object_store.clone(),
                    crate::account_purge_worker::AccountPurgeWorkerConfig::default(),
                );
                std::mem::forget(purge_handle);
                info!("account_purge_worker spawned (direitos dos titulares)");
            } else {
                warn!(
                    "account_purge_worker skipped: GARRAIA_PURGE_DATABASE_URL unset or \
                     PurgePool connect failed; DELETE /v1/me still enqueues and the \
                     manual procedure in docs/legal/data-subject-requests.md applies"
                );
            }

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

        // Tamanho da memoria no `/metrics` (#957). Sobe **sempre** que ha
        // memoria, e nao junto da retencao: aquele worker so existe quando
        // `memory.retention.enabled` e true, que e off por padrao, e os gauges
        // ficariam mortos para quase todo mundo. Ver o docblock do modulo.
        if let Some(memory) = state.agents.memory_provider() {
            let handle = crate::memory_gauge_worker::spawn_memory_gauge_worker(memory);
            std::mem::forget(handle);
            debug!("memory_gauge_worker spawned");
        }

        // Retencao da memoria do agente (#956): o `compact()` existia e nunca
        // rodava em producao — a memoria crescia sem teto e o recall degradava
        // com ela. A varredura nasce DESLIGADA de proposito; quando esta
        // assim, o boot diz uma vez quantas entradas existem e como ligar, para
        // o operador ter o sinal sem pagar com dado apagado numa atualizacao.
        if let Some(memory) = state.agents.memory_provider() {
            match crate::memory_retention_worker::MemoryRetentionConfig::from_app_config(
                &state.config,
            ) {
                Some(retention) => {
                    let horas = retention.interval.as_secs() / 3600;
                    let dias = retention.max_age_days;
                    let plural_dias = if dias == 1 { "dia" } else { "dias" };
                    let plural_horas = if horas == 1 { "hora" } else { "horas" };
                    let handle = crate::memory_retention_worker::spawn_memory_retention_worker(
                        memory, retention,
                    );
                    // Detach: a varredura vive enquanto o processo viver, como
                    // o worker de uploads do plan 0047.
                    std::mem::forget(handle);
                    info!(
                        "memory_retention_worker spawned: apaga entradas nao-fixadas com mais \
                         de {dias} {plural_dias}, a cada {horas} {plural_horas}"
                    );
                }
                None if state.config.memory.enabled => {
                    info!(
                        "retencao da memoria desligada (memory.retention.enabled=false): a \
                         memoria cresce sem teto. Ligue em config.yml ou rode \
                         `garra memory compact` quando quiser."
                    );
                }
                None => {}
            }
        }

        // #1227 (slice 5): retencao do ledger `agent_runs`. Default 0 =
        // nunca apaga; desligada, o boot avisa uma vez com a contagem. Ver o
        // docblock de `runs_retention_worker`.
        crate::runs_retention_worker::iniciar(&state.config, state.session_store.clone());

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

        // #1050: o bridge OpenClaw. `OpenClawClient::new` ja dispara o loop de
        // conexao, entao construi-lo aqui e o que liga o canal; o `Receiver`
        // fica para depois do `Arc`, porque o roteador precisa do estado
        // compartilhado. Ate agora `state.openclaw_client` era `None`
        // constante e as quatro rotas de `/api/openclaw/*` caiam todas no
        // ramo "nao configurado".
        let openclaw_rx = match build_openclaw_config(&state.config) {
            Some(cfg) => {
                let (client, rx) = garraia_channels::OpenClawClient::new(cfg.clone());
                state.openclaw_client = Some(Arc::clone(&client));
                state.openclaw_config = Some(cfg);
                Some((client, rx))
            }
            None => None,
        };

        let state = Arc::new(state);

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

        // Build Google Chat channels (webhook-driven — no persistent connection).
        //
        // Como o WhatsApp: nao entram no `ChannelRegistry`, viram estado da
        // rota `/webhooks/google-chat`. Um canal sem `audience` nao chega ate
        // aqui — `build_google_chat_channels` o descarta, porque sem ela o
        // webhook aceitaria o token de qualquer outra app do Google Chat.
        let google_chat_channels = build_google_chat_channels(&state.config, &state);
        for channel in &google_chat_channels {
            info!(
                "google chat channel ready (webhook mode, name={})",
                channel.name()
            );
        }
        let google_chat_state: garraia_channels::google_chat::webhook::GoogleChatState =
            Arc::new(google_chat_channels);

        // Build Teams channels (webhook-driven — no persistent connection).
        //
        // Um canal sem `app_id` nao chega ate aqui — `build_teams_channels` o
        // descarta, porque sem ele o webhook aceitaria o token de qualquer
        // outro bot do Bot Framework.
        let teams_channels = build_teams_channels(&state.config, &state);
        for channel in &teams_channels {
            info!(
                "teams channel ready (webhook mode, name={})",
                channel.name()
            );
        }
        let teams_state: garraia_channels::teams::webhook::TeamsState = Arc::new(teams_channels);

        // Build LINE channels (webhook-driven — no persistent connection).
        //
        // Como o WhatsApp: nao entram no `ChannelRegistry`, viram estado da
        // rota `/webhooks/line`. Um canal com `channel_secret` invalido nao
        // chega ate aqui — `build_line_channels` o descarta (#1051).
        let line_channels = build_line_channels(&state.config, &state);
        for channel in &line_channels {
            info!("line channel ready (webhook mode, name={})", channel.name());
        }
        let line_state: garraia_channels::line_channel::webhook::LineState =
            Arc::new(line_channels);

        let push_channels = crate::push_channels::PushChannelStates {
            whatsapp: whatsapp_state,
            google_chat: google_chat_state,
            teams: teams_state,
            line: line_state,
        };

        // `garra_status` reads the live `AppState` (provider, model, tools,
        // features, channels, session mode), so it can only exist once the
        // state is shared. Unconditional: every runtime should be able to
        // describe itself — the v0.4.0 field report was a Garra on a phone
        // saying it "cannot inspect its own runtime", and nothing let it.
        //
        // #1347: registrada DEPOIS de montar os canais push e ANTES de
        // qualquer canal pull conectar. Depois dos push porque o relatorio de
        // canais e o do `/api/channels` saem da mesma funcao
        // (`channels_view::channel_rows`), e ela precisa das contagens push.
        // Antes dos pull porque cada um deles (OpenClaw, Discord, Telegram,
        // IRC, Signal, Matrix, Slack, iMessage, `whatsapp_linked`) ja pode
        // rodar turno assim que conecta, e um turno sem a tool respondia "nao
        // consigo me inspecionar" — a janela que a revisao da #1347 achou
        // quando o registro ficava depois do ultimo pull. Montar os push
        // aqui e seguro: `build_*_channels` so constroi os canais e os
        // callbacks, e ninguem os alcanca antes do listener HTTP subir, no
        // fim de `run`. So as contagens: guardar o `PushChannelStates` na
        // tool fecharia um ciclo de `Arc` (cada canal push segura o
        // `AppState` forte). Um teste do `garra_status` varre esta ordem.
        state
            .agents
            .register_tool(Box::new(crate::tools::GarraStatusTool::new(
                &state,
                push_channels.contagens(),
            )));

        // #1670: a tool de consulta ao catálogo de funcionalidades embutido.
        // Mesma janela do `garra_status` acima (depois dos push, antes do
        // primeiro turno de canal pull): o catálogo é estático e vem do
        // binário, mas a tool precisa do `AppState` para aplicar a mesma
        // decisão de operador do `garra_status` em turnos restritos.
        state.agents.register_tool(Box::new(
            crate::tools::catalog_tool::GarraCapabilitiesTool::new(&state),
        ));

        // #1669: o diagnostico nativo da instalacao — mesma janela de registro
        // do `garra_status` (depois dos canais push, antes de qualquer canal
        // pull rodar turno). Somente leitura, sem shell, e fail-closed para
        // quem nao e o operador (ver `tools::diagnostics_tool`).
        state
            .agents
            .register_tool(Box::new(crate::tools::GarraDiagnosticsTool::new(
                &state,
                push_channels.contagens(),
                garraia_config::ConfigLoader::default_config_dir(),
            )));

        if let Some((client, rx)) = openclaw_rx {
            spawn_openclaw_router(Arc::clone(&state), client, rx);
        }

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
            // Variante de voz (#telegram_send_voice): mesmo destino, mesmo
            // teto, gating proprio pela tabela de capacidades. A disponibilidade
            // da tool exige TTS configurado, entao registrar aqui e barato —
            // quem nao tem voz nunca a ve na lista.
            state
                .agents
                .register_tool(Box::new(crate::tools::TelegramSendVoiceTool::new(&state)));
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
            // #1227 (slice 2): identidade deste processo nas leases
            // (`scheduled_tasks.leased_by`). Gerada uma vez por subida, so
            // para diagnostico — quem reivindicou a tarefa que ficou
            // `running` depois de uma queda.
            let scheduler_owner = uuid::Uuid::new_v4().to_string();
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                if let Err(e) = run_scheduler(&scheduler_state, &scheduler_owner).await {
                    tracing::error!("Scheduler error: {e}");
                }
            }
        });

        // Start configured Telegram channels.
        //
        // Issue #928: a boot-time connect failure used to be terminal — one
        // `warn!` and the channel stayed mute until a manual restart, while a
        // failure *during* polling gets teloxide's own infinite exponential
        // backoff. Five seconds of bad mobile network at boot cost the whole
        // channel. Now the failed channel keeps retrying in the background
        // with the same backoff shape the OpenClaw client uses.
        let telegram_channels = build_telegram_channels(&state.config, &state);
        for mut channel in telegram_channels {
            match channel.connect().await {
                Ok(()) => state.channels.write().await.register(channel),
                Err(e) => {
                    warn!("telegram channel failed to connect: {e}; retrying in background");
                    spawn_channel_connect_retry(Arc::clone(&state), channel);
                }
            }
        }

        // Start configured IRC channels (#1050).
        //
        // Servidor IRC cai e volta; falha no boot vai pelo retry com backoff
        // do Telegram (#928) em vez do `warn!` de uma linha.
        let irc_channels = build_irc_channels(&state.config, &state);
        for mut channel in irc_channels {
            match channel.connect().await {
                Ok(()) => state.channels.write().await.register(channel),
                Err(e) => {
                    warn!("irc channel failed to connect: {e}; retrying in background");
                    spawn_channel_connect_retry(Arc::clone(&state), channel);
                }
            }
        }

        // Start configured Signal channels (#1050).
        //
        // signal-cli e um daemon local que o operador sobe por fora, entao
        // uma falha de conexao no boot e comum e nao deve ser terminal — vai
        // pelo mesmo `spawn_channel_connect_retry` do Telegram (#928), com o
        // backoff exponencial, em vez do `warn!` de uma linha do Discord.
        let signal_channels = build_signal_channels(&state.config, &state);
        for mut channel in signal_channels {
            match channel.connect().await {
                Ok(()) => state.channels.write().await.register(channel),
                Err(e) => {
                    warn!("signal channel failed to connect: {e}; retrying in background");
                    spawn_channel_connect_retry(Arc::clone(&state), channel);
                }
            }
        }

        // Start configured Matrix channels (#1050).
        //
        // O homeserver pode estar fora do ar no boot sem que isso seja
        // permanente, entao vai pelo retry com backoff do Telegram (#928) em
        // vez do `warn!` de uma linha que deixaria o canal mudo ate um
        // restart manual.
        let matrix_channels = build_matrix_channels(&state.config, &state);
        for mut channel in matrix_channels {
            match channel.connect().await {
                Ok(()) => state.channels.write().await.register(channel),
                Err(e) => {
                    warn!("matrix channel failed to connect: {e}; retrying in background");
                    spawn_channel_connect_retry(Arc::clone(&state), channel);
                }
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

        // Sobe o canal WhatsApp por dispositivo vinculado (#1238, fatia D).
        //
        // Canal PULL e supervisao propria: nao entra no `ChannelRegistry` (o
        // `Channel` trait pressupoe `connect()/disconnect()` sincronos sobre um
        // objeto mutavel, e aqui quem vive e um processo filho Node com loop de
        // reconexao proprio), e nao entra no `PushChannelStates` (nao ha
        // webhook). O status dele sai do `AppState::whatsapp_linked`, que o
        // `/api/channels` e o `/api/diagnostics` leem pela MESMA funcao.
        //
        // Nao subir e o caso comum — canal desligado, sem sessao, ou sem Node —
        // e nenhum deles e erro de boot.
        // `Ok(())` e nao `Ok(cancel)`: o `watch::Sender` que mantem o
        // supervisor vivo fica estacionado no `AppState`
        // (`WhatsAppLinkedRuntime::reter_cancelamento`). Ele ja morreu aqui uma
        // vez, solto no fim deste braco do `match`, e o canal inteiro morria
        // com ele em ~0,1 s a cada boot.
        //
        // Desligado e o caso comum e sai em `info!`. Qualquer OUTRO motivo e um
        // canal que o operador ligou e que nao subiu: `warn!`, com a frase de
        // acao do `Display` (#1327) — o nome do enum em `info!` foi o que
        // deixou o sintoma da issue invisivel.
        match crate::bootstrap::spawn_whatsapp_linked(&state) {
            Ok(()) => info!("whatsapp_linked: canal supervisionado"),
            Err(crate::bootstrap::NaoSubiu::Desabilitado) => {
                info!("whatsapp_linked: canal desligado, nada a supervisionar")
            }
            Err(motivo) => warn!("whatsapp_linked: canal nao subiu — {motivo}"),
        }

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
        let app = build_router(state, push_channels, admin_store, admin_key);

        // TLS support: if cert + key paths are configured and tls feature is enabled,
        // use axum-server with rustls. Otherwise, plain HTTP.
        let use_tls = tls_cert.is_some() && tls_key.is_some();
        // #1247: so um dos dois caminhos configurado caia em HTTP puro em
        // silencio — o operador pediu TLS e recebia texto claro. A CLI recusa
        // o boot nesse caso (allowlist do boot gate); aqui fica o aviso para
        // quem embute o `GatewayServer` sem passar pela CLI.
        if let Some(falta) = tls_meio_configurado(tls_cert.as_deref(), tls_key.as_deref()) {
            warn!(
                "TLS half-configured: {falta} is missing, so the gateway serves PLAIN HTTP; \
                 set both gateway.tls_cert_path and gateway.tls_key_path (or neither) — run \
                 `{} config check`",
                garraia_common::executavel::nome()
            );
        }

        // Each branch yields a Result instead of `?`-ing out: the MCP/channel
        // cleanup below must run even when serving fails, otherwise the child
        // processes spawned above are orphaned.
        let serve_result: Result<()> = if use_tls {
            #[cfg(feature = "tls")]
            {
                serve_tls(
                    &addr,
                    tls_cert.as_deref(),
                    tls_key.as_deref(),
                    app,
                    &gateway_cfg,
                )
                .await
            }
            #[cfg(not(feature = "tls"))]
            {
                warn!(
                    "TLS cert/key configured but 'tls' feature not enabled — falling back to HTTP"
                );
                serve_plain(&addr, app, &gateway_cfg).await
            }
        } else {
            serve_plain(&addr, app, &gateway_cfg).await
        };

        // Cleanup runs even when the listener errored out: this block used to
        // sit after a `?`, so any serve error skipped it and orphaned every
        // MCP child process. The serve result is propagated at the end.
        shutdown_subsystems(&state_for_shutdown).await;

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
    let mut admin_store_owned =
        admin::store::AdminStore::in_memory().expect("in-memory admin store should work");
    let admin_key = admin_key_for_test_router(&mut admin_store_owned);
    let admin_store = Arc::new(Mutex::new(admin_store_owned));

    build_router(
        state,
        crate::push_channels::PushChannelStates::empty(),
        admin_store,
        admin_key,
    )
}

/// The admin master key of the test router (#2021).
///
/// Never resolved against the operator's `<config_dir>/admin`. The test store
/// is in-memory and always empty, and over an empty store a wrong-length
/// `master.key` is replaced by a new one (#2013): a test run moved the
/// operator's key to `master.key.unreadable` and wrote another in its place,
/// and the real gateway, with secrets in its `admin.db`, answered 503. The
/// `master.key` written here lands in a throwaway directory removed right
/// after; the key itself stays in memory, which is all this router needs.
#[cfg(feature = "test-helpers")]
pub fn admin_key_for_test_router(
    store: &mut crate::admin::store::AdminStore,
) -> crate::admin::handlers::AdminKey {
    let scratch =
        new_scratch_dir().expect("the test router needs a throwaway directory for its admin key");
    let key = crate::admin::handlers::resolve_admin_encryption_key(store, &scratch);
    // Best-effort: what is left behind is a test key in our own directory.
    let _ = std::fs::remove_dir_all(&scratch);
    key
}

/// A directory this call just created in the shared temp dir, owner-only on
/// Unix. The name is predictable (pid and a counter), so it is created with
/// `create_dir`, never `create_dir_all`: `mkdir` fails on anything already at
/// the path, a planted symlink included, and that name is skipped rather than
/// followed into someone else's directory — such as the operator's
/// `<config_dir>/admin`.
#[cfg(feature = "test-helpers")]
fn new_scratch_dir() -> std::io::Result<std::path::PathBuf> {
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    for _ in 0..100 {
        let dir = std::env::temp_dir().join(format!(
            "garraia-test-admin-key-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match builder.create(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "100 names in the temp dir were already taken",
    ))
}

/// Upper bound for cancelling MCP services at shutdown. A server that ignores
/// stdin EOF must not hold the gateway open forever; children are killed when
/// their transport is dropped regardless.
const MCP_SHUTDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Upper bound for disconnecting the push channels at shutdown. Same reason as
/// [`MCP_SHUTDOWN_TIMEOUT`]: a channel whose `disconnect()` never returns
/// (hung socket, poller ignoring cancellation) must not hold the process open
/// forever.
///
/// 5 s, not 10 s, because the CLI grace on Unix is 30 s total: TLS draining
/// can take 10 s, MCP shutdown another 10 s, and the three together must leave
/// margin before `kill_and_wait` escalates to SIGKILL
/// (`crates/garraia-cli/src/main.rs`).
const CHANNELS_SHUTDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// #1247: qual campo de TLS falta quando so um dos dois esta configurado,
/// ou `None` quando os dois estao (TLS) ou nenhum esta (HTTP de proposito).
/// Os nomes batem com os campos dos achados do `garraia config check`.
pub fn tls_meio_configurado(cert: Option<&str>, key: Option<&str>) -> Option<&'static str> {
    match (cert, key) {
        (Some(_), None) => Some("gateway.tls_key_path"),
        (None, Some(_)) => Some("gateway.tls_cert_path"),
        _ => None,
    }
}

/// #1261 (decisao A): recusa o boot quando o bind pedido e alcancavel da
/// rede sem credencial de gateway e sem o opt-out explicito
/// `gateway.allow_unauthenticated_network_bind` (so no arquivo). Com o
/// opt-out sobe, com `warn!` alto em todo boot.
///
/// Substitui o aviso-sem-recusa do #1241: um bind exposto sem credencial
/// abre o `/ws` (tools de arquivo e de dispositivo) e o
/// `/api/mcp/marketplace/install` — spawn remoto de processo — para quem
/// alcanca a porta, e o boot ja sabia disso e seguia assim mesmo.
pub fn refuse_exposed_bind(gateway: &garraia_config::GatewayConfig) -> Result<()> {
    match garraia_config::bind::verificar(&gateway.host, gateway.port, gateway) {
        Ok(garraia_config::bind::VereditoDoBind::ExpostoPorOptOut) => {
            warn!(
                "{}",
                garraia_config::bind::aviso_de_opt_out(&gateway.host, gateway.port)
            );
            Ok(())
        }
        Ok(_) => Ok(()),
        Err(recusa) => Err(garraia_common::Error::Config(recusa.to_string())),
    }
}

/// A mesma regra de [`refuse_exposed_bind`] sobre o endereco **efetivamente
/// ligado** — defesa em profundidade para o caso de o nome resolver diferente
/// entre a checagem e o bind. Nunca repete o aviso do opt-out (esse sai uma
/// vez, em `run`).
fn conferir_socket(
    bound: &std::net::SocketAddr,
    gateway: &garraia_config::GatewayConfig,
) -> Result<()> {
    garraia_config::bind::decidir(
        &bound.to_string(),
        std::slice::from_ref(bound),
        gateway.api_key_configurada(),
        gateway.allow_unauthenticated_network_bind,
    )
    .map(|_| ())
    .map_err(|recusa| garraia_common::Error::Config(recusa.to_string()))
}

/// Serve plain HTTP until the shutdown signal.
async fn serve_plain(
    addr: &str,
    app: axum::Router,
    gateway: &garraia_config::GatewayConfig,
) -> Result<()> {
    let listener = TcpListener::bind(addr).await?;
    // Depois do bind: `local_addr` e o endereco real, inclusive quando `addr`
    // era um nome. Recusado (#1261), o listener cai aqui sem ter aceitado
    // conexao nenhuma. Sem `local_addr` nao ha como afirmar loopback: recusa.
    let bound = listener
        .local_addr()
        .map_err(|e| garraia_common::Error::Gateway(format!("local_addr: {e}")))?;
    conferir_socket(&bound, gateway)?;
    // #1907: o socket REAL e a unica fonte do endereco ligado. Bind
    // nao-loopback aqui e HTTP em claro por construcao (este e o caminho sem
    // TLS), entao a credencial e os tokens de sessao viajam sem cifra: avisa
    // em todo boot. Avisa e nao recusa porque composicoes containerizadas
    // podem ligar `0.0.0.0` com TLS num proxy externo. O compose deste repo
    // publica HTTP em claro, sem proxy TLS; o degrau de recusa e decisao a parte
    // — ver docs/auth-config.md §5.1.
    if let Some(aviso) = garraia_config::bind::aviso_de_transporte(
        addr,
        std::slice::from_ref(&bound),
        &garraia_common::executavel::nome(),
    ) {
        warn!("{aviso}");
    }
    info!("GarraIA gateway listening on http://{}", addr);
    servir_http_ate(listener, app, shutdown_signal(), HTTP_SHUTDOWN_GRACE).await
}

/// Teto do encerramento gracioso do caminho HTTP, igual ao do TLS
/// (`TLS_SHUTDOWN_GRACE`): passado isso o `serve` e largado com as
/// requisicoes ainda abertas. Os dois caminhos sao excludentes, entao a conta
/// do grace de 30 s da CLI e a mesma: dreno 10 s + MCP 10 s + canais 5 s.
const HTTP_SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(10);

/// Serve HTTP em `listener` ate `sinal`, e depois drena por no maximo `teto`.
///
/// #1913: `with_graceful_shutdown` sozinho espera TODAS as conexoes
/// terminarem, sem prazo — uma unica requisicao pendurada (stream longo,
/// handler travado) segurava o `serve` e, com ele, o `shutdown_subsystems`
/// que vem depois; o gateway so saia pelo SIGKILL da CLI, com a arvore dos
/// servidores MCP para tras. Estourado o teto, o `serve` e largado: o
/// listener fecha, o chamador segue para a limpeza, e as tarefas das
/// conexoes que sobraram morrem com o runtime na saida do processo.
///
/// Consequencia a ter em mente: passado o teto, essas tarefas seguem VIVAS
/// em paralelo com o `shutdown_subsystems` — a limpeza deixou de ter a
/// exclusividade que o dreno sem prazo dava. Por isso cada passo dela tem
/// teto proprio ([`MCP_SHUTDOWN_TIMEOUT`], [`CHANNELS_SHUTDOWN_TIMEOUT`]).
///
/// O sinal e esperado NESTA tarefa (o `with_graceful_shutdown` do axum o
/// moveria para uma tarefa a parte, onde um panico ao instalar o handler
/// sumiria em silencio).
async fn servir_http_ate(
    listener: TcpListener,
    app: axum::Router,
    sinal: impl std::future::Future<Output = ()>,
    teto: std::time::Duration,
) -> Result<()> {
    use std::future::IntoFuture;

    let (dispara, disparado) = tokio::sync::oneshot::channel::<()>();
    let servir = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        let _ = disparado.await;
    })
    .into_future();
    tokio::pin!(servir);
    let servido = tokio::select! {
        r = &mut servir => r,
        () = sinal => {
            // So falha se o serve ja terminou — e ai o braco de cima teria
            // vencido o select.
            let _ = dispara.send(());
            match tokio::time::timeout(teto, &mut servir).await {
                Ok(r) => r,
                Err(_) => {
                    warn!(
                        "HTTP connections still open after {:?}; dropping them to continue shutdown",
                        teto
                    );
                    Ok(())
                }
            }
        }
    };
    servido.map_err(|e| garraia_common::Error::Gateway(format!("server error: {e}")))
}

/// Teto do encerramento gracioso do caminho TLS: passado isso, as conexoes
/// ainda abertas sao cortadas. Sem teto, uma unica conexao pendurada
/// seguraria o `serve` e, com ele, a limpeza de MCP/canais que vem depois.
#[cfg(feature = "tls")]
const TLS_SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(10);

/// Serve HTTPS (rustls) until the shutdown signal.
#[cfg(feature = "tls")]
async fn serve_tls(
    addr: &str,
    cert_path: Option<&str>,
    key_path: Option<&str>,
    app: axum::Router,
    gateway: &garraia_config::GatewayConfig,
) -> Result<()> {
    serve_tls_ate(addr, cert_path, key_path, app, gateway, shutdown_signal()).await
}

/// [`serve_tls`] com o sinal de encerramento injetado.
///
/// #1913: este caminho servia ate o processo morrer — Ctrl+C/SIGTERM matavam
/// o gateway pela acao padrao do sinal, o `serve` nunca retornava e
/// `shutdown_subsystems` nao rodava. Desde que cada servidor MCP stdio tem
/// grupo de processos proprio, o sinal do terminal tambem nao os alcanca
/// mais, entao a arvore deles ficava para tras. O `Handle` e o que faz o
/// `serve` retornar: `sinal` dispara o encerramento gracioso e o chamador
/// segue para a limpeza, igual ao caminho HTTP.
#[cfg(feature = "tls")]
async fn serve_tls_ate(
    addr: &str,
    cert_path: Option<&str>,
    key_path: Option<&str>,
    app: axum::Router,
    gateway: &garraia_config::GatewayConfig,
    sinal: impl std::future::Future<Output = ()>,
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
    // #1261: TLS nao isenta — TLS sem credencial continua aberto.
    conferir_socket(&sock_addr, gateway)?;
    info!("GarraIA gateway listening on https://{}", sock_addr);
    let handle = axum_server::Handle::<std::net::SocketAddr>::new();
    let servir = axum_server::bind_rustls(sock_addr, tls_config)
        .handle(handle.clone())
        .serve(app.into_make_service_with_connect_info::<std::net::SocketAddr>());
    tokio::pin!(servir);
    // O sinal e esperado NESTA tarefa, como no caminho HTTP: numa tarefa a
    // parte, um panico ao instalar o handler sumiria em silencio e o TLS
    // voltaria a servir sem gancho de encerramento. O `serve` tambem retorna
    // sozinho (erro de bind), e ai o sinal e simplesmente largado.
    let servido = tokio::select! {
        r = &mut servir => r,
        () = sinal => {
            handle.graceful_shutdown(Some(TLS_SHUTDOWN_GRACE));
            servir.await
        }
    };
    servido.map_err(|e| garraia_common::Error::Gateway(format!("server error: {e}")))
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

/// #1227 (slice 2): por quanto tempo uma tarefa reivindicada fica `running`
/// antes de a queda do processo ser assumida e a linha voltar a `pending`.
///
/// Nao ha constante de timeout do turno no runtime hoje (o `process_heartbeat`
/// espera o provider ate o fim); 600 s e o teto pratico de um turno com tools
/// dobrado, para que uma execucao lenta mas viva nunca seja reivindicada de
/// novo por outro tick enquanto ainda corre. Quando o runtime ganhar um
/// timeout de turno, este valor deve virar 2x ele — e a origem fica aqui.
const SCHEDULER_LEASE_SECS: i64 = 600;

/// Um tick do scheduler: recupera leases expiradas (explicito, logado),
/// reivindica as tarefas vencidas para `owner` e executa cada uma.
///
/// `owner` e a identidade deste processo (uuid gerado uma vez na subida);
/// vai para `scheduled_tasks.leased_by` so para diagnostico.
async fn run_scheduler(state: &AppState, owner: &str) -> Result<()> {
    let store_mutex = match &state.session_store {
        Some(s) => s,
        None => return Ok(()),
    };

    let tasks = {
        let store = store_mutex.lock().await;
        // Queda no meio do tick anterior deixou `running` sem dono vivo:
        // volta a `pending` AQUI, com log, e e reivindicada logo abaixo no
        // mesmo tick — o re-poll deixa de ser silencioso.
        garraia_db::log_recovered_leases(&store);
        store.claim_due_tasks(
            garraia_db::session_store::DEFAULT_POLL_LIMIT,
            SCHEDULER_LEASE_SECS,
            owner,
        )?
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
    // #1227 (slice 1): o scheduler e o primeiro produtor real do ledger —
    // cada execucao deixa uma linha em `agent_runs` com `session_id`
    // preenchido e desfecho terminal; uma queda no meio vira `interrupted`
    // na subida seguinte (`log_interrupted_runs`). Falha do ledger e
    // fail-soft: nunca impede a tarefa nem muda o fluxo de retry.
    let run_id = ledger_inicia_run_agendado(store_mutex, task).await;
    let resultado = execute_scheduled_task_inner(state, store_mutex, task).await;
    ledger_fecha_run_agendado(store_mutex, &run_id, &resultado).await;
    resultado.map(|_| ())
}

/// Abre a linha do run (`running`) para uma execucao de tarefa agendada.
/// `mode = "heartbeat"` nomeia o produtor; `goal` e o payload da tarefa
/// (a coluna guarda, o log nunca). Devolve o `run_id` mesmo se a gravacao
/// falhar — o fecho do run tambem e fail-soft.
async fn ledger_inicia_run_agendado(
    store_mutex: &Arc<Mutex<SessionStore>>,
    task: &garraia_db::ScheduledTask,
) -> String {
    let run_id = uuid::Uuid::new_v4().to_string();
    let store = store_mutex.lock().await;
    if let Err(e) = store.start_agent_run(
        &run_id,
        Some(&task.session_id),
        &task.payload,
        Some("heartbeat"),
    ) {
        tracing::warn!(erro = %e, "run ledger: falhou ao abrir run da tarefa agendada");
    }
    run_id
}

/// Fecha a linha do run com desfecho terminal: `done` com snippet da
/// resposta e o destino dela (#2002: `entregue`, `sem_entrega` ou
/// `nao_entregue:<motivo>`), ou `error` com o snippet do erro. O ramo de
/// retry do `run_scheduler` continua dono da politica de reexecucao — aqui
/// so fica a trilha.
async fn ledger_fecha_run_agendado(
    store_mutex: &Arc<Mutex<SessionStore>>,
    run_id: &str,
    resultado: &Result<(String, crate::entrega_agendada::Entrega)>,
) {
    let (status, result_snippet, error_snippet) = match resultado {
        Ok((texto, _)) => (garraia_db::RunStatus::Done, Some(texto.as_str()), None),
        Err(e) => (garraia_db::RunStatus::Error, None, Some(e.to_string())),
    };
    let store = store_mutex.lock().await;
    if let Err(e) = store.finish_agent_run(run_id, status, result_snippet, error_snippet.as_deref())
    {
        tracing::warn!(erro = %e, "run ledger: falhou ao fechar run da tarefa agendada");
    }
    if let Ok((_, entrega)) = resultado
        && let Err(e) = store.set_agent_run_delivery(run_id, &entrega.para_o_ledger())
    {
        tracing::warn!(erro = %e, "run ledger: falhou ao gravar a entrega da tarefa agendada");
    }
}

/// Executa a tarefa e devolve a resposta e o destino dela (#2002).
async fn execute_scheduled_task_inner(
    state: &AppState,
    store_mutex: &Arc<Mutex<SessionStore>>,
    task: &garraia_db::ScheduledTask,
) -> Result<(String, crate::entrega_agendada::Entrega)> {
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
    let continuity_key = state.continuity_key().map(|k| k.as_str().to_string());

    // Fronteira de autorizacao (#1985): a tarefa roda com a AUTORIDADE DE
    // QUEM A CRIOU, gravada na criacao — agendar nao eleva privilegio e
    // tambem nao reduz. O construtor central da o contexto da sessao de hoje
    // (modo, diretorio, superficie); `tarefa_agendada::contexto` troca as
    // camadas de teto pelas gravadas, cai no fallback restritivo quando nao
    // ha gravacao, e poe por cima a politica VIGENTE do canal. Nunca ha
    // contexto privilegiado implicito.
    let base = state
        .exec_context_for_msg(
            &task.session_id,
            Some(task.user_id.as_str()),
            Some(&task.payload),
        )
        .await;
    let autoridade = crate::tarefa_agendada::ler(task);
    let vigente = crate::tarefa_agendada::politica_vigente(state, task);
    let exec = crate::tarefa_agendada::contexto(base, &autoridade, &vigente);
    // O coletor recebe as ferramentas que um TETO barrou durante o turno: a
    // tarefa nao pode deixar de fazer o que prometeu em silencio.
    let (resposta, bloqueios) = garraia_agents::tools::turn_tools::coletando_bloqueios_de_teto(
        state.agents.process_heartbeat_with_exec(
            &task.session_id,
            &task.payload,
            &history,
            continuity_key.as_deref(),
            Some(task.user_id.as_str()),
            &exec,
        ),
    )
    .await;
    let response_text = resposta?;
    let response_text = match crate::tarefa_agendada::aviso_da_execucao(
        task,
        &autoridade,
        &vigente,
        &exec,
        &bloqueios,
    ) {
        Some(aviso) => {
            // O payload e a resposta nao entram no log; o id e as ferramentas
            // barradas, sim — e o que o operador precisa para reautorizar.
            // A sessao tambem fica FORA: no WhatsApp vinculado o id dela
            // carrega o telefone do contato (`whatsapp-linked-<jid>`), e o id
            // da tarefa ja leva o operador ate ela (`garraia tasks list`).
            tracing::warn!(
                task = %task.id,
                barradas = ?bloqueios.iter().map(|b| b.ferramenta.as_str()).collect::<Vec<_>>(),
                "tarefa agendada rodou sob um teto que restringe o que ela pediu"
            );
            // O aviso ABRE a resposta: e o que cabe no snippet do ledger
            // (`garra runs`) e o que o canal le primeiro quando ha entrega
            // (passo 4, `entrega_agendada`).
            format!("{aviso}\n\n{response_text}")
        }
        None => response_text,
    };

    // 3. Persist assistant response regardless of outbound channel availability.
    {
        let store = store_mutex.lock().await;
        store.append_message(
            &task.session_id,
            "assistant",
            &response_text,
            chrono::Utc::now(),
            &task.session_metadata,
        )?;
    }

    // 4. Entrega ativa ao canal (#2002), best-effort e DEPOIS de persistir:
    // a resposta ja esta no historico, e entregar e transparencia, nao
    // controle. O desfecho vai ao ledger (`agent_runs.delivery`) e ao log —
    // com o id da tarefa e o canal, nunca o id da sessao, que no WhatsApp
    // vinculado carrega o telefone do contato.
    let entrega = crate::entrega_agendada::entregar(state, task, &response_text).await;
    match entrega {
        crate::entrega_agendada::Entrega::Entregue => info!(
            task = %task.id,
            canal = %channel_type,
            "tarefa agendada: resposta entregue ao canal"
        ),
        crate::entrega_agendada::Entrega::SemCanal => tracing::debug!(
            task = %task.id,
            canal = %channel_type,
            "tarefa agendada: o canal da sessao nao tem entrega ativa; resposta no historico"
        ),
        crate::entrega_agendada::Entrega::NaoEntregue(motivo) => tracing::warn!(
            task = %task.id,
            canal = %channel_type,
            motivo = motivo.as_str(),
            "tarefa agendada: resposta nao entregue ao canal; ficou no historico"
        ),
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
                // `None`: max_runs reached (completed) or the stored schedule
                // no longer computes (failed; the store logs why, #1938).
                None => info!(task = %task.id, "recurring task finished — no next run armed"),
            }
        } else {
            store.complete_task(&task.id)?;
        }
    }

    Ok((response_text, entrega))
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

/// Desliga, na ordem, tudo que este processo pos de pe e que sobrevive ao
/// listener: servidores MCP, canais push e o supervisor do WhatsApp vinculado.
///
/// # Por que isto e uma funcao, e nao um bloco dentro do `serve`
///
/// Porque a unica coisa que se pode testar de um bloco inline e o texto dele.
/// A regressao que importa aqui — "o subsistema novo entrou no boot e ninguem
/// o desligou" — ja aconteceu uma vez neste canal, e uma varredura de fonte a
/// pegaria so ate alguem renomear a chamada.
///
/// # O ciclo de `Arc`, e por que o cancelamento e explicito
///
/// "O `Sender` cai junto com o `AppState`" nao vale para o WhatsApp vinculado:
/// `AppState` guarda `whatsapp_linked: Arc<WhatsAppLinkedRuntime>`, o runtime
/// guarda o `watch::Sender` do supervisor, e a tarefa do supervisor detem um
/// `GatewaySink` que detem um `Arc<AppState>`. A tarefa so termina quando o
/// `cancel_rx` dispara; o `cancel_tx` so cai quando o `AppState` cai; o
/// `AppState` so cai quando a tarefa termina. Sem a chamada abaixo o gateway
/// imprime "shut down gracefully" com a ponte Node viva, mensagens sendo lidas
/// e turnos de agente rodando — ate o processo morrer.
///
/// Quebrar o ciclo com `Weak<AppState>` no sink foi considerado e recusado: o
/// sink usa o estado em todo turno (sessoes, agentes, allowlist), cada uso
/// viraria um `upgrade()` falivel, e o resultado seria um desligamento que
/// depende de **nenhum outro** `Arc<AppState>` ter sobrado em lugar nenhum —
/// mecanismo implicito, que e o que falhou aqui em primeiro lugar. O
/// cancelamento explicito diz o que faz.
async fn shutdown_subsystems(state: &Arc<AppState>) {
    // Bounded: `disconnect_all` cancels each service, and a server that
    // ignores stdin EOF can take up to its own drain time; unbounded and
    // sequential, N bad servers could hang shutdown indefinitely.
    if let Some(ref manager) = state.mcp_manager_arc {
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
    if let Some(limite) = desligar_canais(state, CHANNELS_SHUTDOWN_TIMEOUT).await {
        warn!(
            "channel disconnect exceeded {:?}; continuing shutdown",
            limite
        );
    }

    if state.whatsapp_linked.cancelar() {
        info!("whatsapp_linked: supervisor cancelado");
    }
}

/// Encerra os canais push com teto. Devolve o limite quando ele estourou
/// (para o chamador avisar no log) e `None` quando `disconnect_all` retornou
/// dentro do prazo — inclusive quando algum canal falhou, caso que o proprio
/// registro ja loga por canal.
///
/// O teto cobre tambem a aquisicao do write lock: um `send_message` em
/// andamento segura o lock de leitura enquanto envia, e o desligamento
/// esperaria por ele sem prazo se so a chamada interna fosse limitada.
async fn desligar_canais(
    state: &Arc<AppState>,
    limite: std::time::Duration,
) -> Option<std::time::Duration> {
    let desconexao = async {
        let mut canais = state.channels.write().await;
        canais.disconnect_all().await
    };
    match tokio::time::timeout(limite, desconexao).await {
        Ok(_) => None,
        Err(_) => Some(limite),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// **A outra metade do F1: o canal sobe, e agora ele tambem para.**
    ///
    /// `WhatsAppLinkedRuntime::cancelar()` existia, estava correto e nao tinha
    /// UM chamador de producao — so os dois testes do proprio runtime. O bloco
    /// de desligamento deste arquivo desligava MCP e canais push e passava ao
    /// largo deste. Depois do Ctrl+C o gateway imprimia "shut down gracefully"
    /// com a ponte Node viva, mensagens sendo lidas e turnos de agente rodando.
    ///
    /// E a rota de escape "o `Sender` cai junto com o `AppState`" nao existe,
    /// por um ciclo de `Arc` — ver o docstring de `shutdown_subsystems`.
    ///
    /// Este teste nao varre o fonte: ele **chama** o desligamento e pergunta do
    /// outro lado do canal de cancelamento se o sinal chegou. Uma varredura de
    /// texto morreria no dia em que alguem renomeasse a chamada; esta so morre
    /// se o comportamento morrer junto.
    #[tokio::test]
    async fn o_desligamento_cancela_o_supervisor_do_whatsapp_vinculado() {
        use garraia_agents::AgentRuntime;
        use garraia_channels::ChannelRegistry;

        let state: Arc<AppState> = Arc::new(AppState::new(
            AppConfig::default(),
            Arc::new(AgentRuntime::new()),
            ChannelRegistry::new(),
        ));

        // O supervisor de mentira: so o lado que importa para o desligamento.
        let (cancel_tx, mut cancel_rx) = tokio::sync::watch::channel(false);
        state.whatsapp_linked.reter_cancelamento(cancel_tx);
        assert!(
            state.whatsapp_linked.cancelamento_vivo(),
            "premissa: ha supervisor retido antes do desligamento"
        );
        assert!(
            !*cancel_rx.borrow_and_update(),
            "premissa: ninguem cancelou ainda"
        );

        shutdown_subsystems(&state).await;

        // O que o `serve` do supervisor observa e o valor do canal. (O
        // `has_changed()` aqui devolveria `Err`, porque `cancelar()` **solta**
        // o `Sender` depois de enviar — o que, do lado do supervisor, tambem e
        // um sinal de parada; nao e o que este teste mede.)
        assert!(
            *cancel_rx.borrow(),
            "o supervisor tem de receber o cancelamento no desligamento do gateway; \
             sem isto a ponte Node segue viva e turnos de agente seguem rodando \
             depois de `gateway shut down gracefully`"
        );
        assert!(
            !state.whatsapp_linked.cancelamento_vivo(),
            "e o slot tem de ficar vazio: um segundo desligamento nao tem o que cancelar"
        );
    }

    /// E desligar sem canal vinculado nao pode explodir nem mentir — o caminho
    /// de toda instalacao que nunca rodou `garra whatsapp link`.
    #[tokio::test]
    async fn o_desligamento_sem_supervisor_e_um_noop() {
        use garraia_agents::AgentRuntime;
        use garraia_channels::ChannelRegistry;

        let state: Arc<AppState> = Arc::new(AppState::new(
            AppConfig::default(),
            Arc::new(AgentRuntime::new()),
            ChannelRegistry::new(),
        ));
        shutdown_subsystems(&state).await;
        assert!(!state.whatsapp_linked.cancelamento_vivo());
    }

    /// **Teto do encerramento dos canais.** O `disconnect_all` daqui era a
    /// unica chamada de desligamento sem prazo: se um canal travasse no
    /// `disconnect()` (socket pendurado, poller que ignora cancelamento), o
    /// gateway nunca chegava ao `gateway shut down gracefully` e a CLI so
    /// saia do processo pelo SIGKILL — no meio da limpeza de MCP e do
    /// WhatsApp vinculado. O bloco de MCP logo acima ja tinha teto; o dos
    /// canais nao.
    struct CanalDeTeste {
        entrou: Arc<AtomicUsize>,
        /// `None` = `disconnect()` nunca retorna (quando `erro` e falso).
        atraso: Option<std::time::Duration>,
        /// `disconnect()` devolve erro em vez de encerrar normalmente.
        erro: bool,
    }

    #[async_trait::async_trait]
    impl garraia_channels::Channel for CanalDeTeste {
        fn channel_type(&self) -> &str {
            "canal-de-teste"
        }

        fn display_name(&self) -> &str {
            "Canal de teste"
        }

        async fn connect(&mut self) -> garraia_common::Result<()> {
            Ok(())
        }

        async fn disconnect(&mut self) -> garraia_common::Result<()> {
            self.entrou.fetch_add(1, Ordering::SeqCst);
            match self.atraso {
                Some(atraso) => {
                    tokio::time::sleep(atraso).await;
                    Ok(())
                }
                None if self.erro => Err(garraia_common::Error::Channel(
                    "disconnect falhou (teste)".into(),
                )),
                None => std::future::pending::<garraia_common::Result<()>>().await,
            }
        }

        async fn send_message(
            &self,
            _message: &garraia_common::Message,
        ) -> garraia_common::Result<()> {
            Ok(())
        }

        fn status(&self) -> garraia_channels::ChannelStatus {
            garraia_channels::ChannelStatus::Disconnected
        }
    }

    fn estado_com_canal(
        atraso: Option<std::time::Duration>,
        erro: bool,
    ) -> (Arc<AppState>, Arc<AtomicUsize>) {
        use garraia_agents::AgentRuntime;
        use garraia_channels::ChannelRegistry;

        let entrou = Arc::new(AtomicUsize::new(0));
        let mut registry = ChannelRegistry::new();
        registry.register(Box::new(CanalDeTeste {
            entrou: entrou.clone(),
            atraso,
            erro,
        }));
        let state = Arc::new(AppState::new(
            AppConfig::default(),
            Arc::new(AgentRuntime::new()),
            registry,
        ));
        (state, entrou)
    }

    #[tokio::test(start_paused = true)]
    async fn canal_que_nunca_encerra_respeita_o_teto() {
        let (state, entrou) = estado_com_canal(None, false);
        let limite = std::time::Duration::from_millis(100);

        let estourou = desligar_canais(&state, limite).await;

        assert_eq!(
            estourou,
            Some(limite),
            "o estouro do teto tem de ser devolvido para o chamador avisar no log"
        );
        assert_eq!(
            entrou.load(Ordering::SeqCst),
            1,
            "premissa: o canal foi chamado e ficou pendurado - nao foi pulado"
        );
    }

    /// O teto tem de cobrir a aquisicao do lock, nao so o `disconnect_all`:
    /// um `send_message` em andamento segura o lock de leitura, e o
    /// desligamento espera por ele.
    #[tokio::test(start_paused = true)]
    async fn teto_cobre_a_espera_pelo_lock_dos_canais() {
        let (state, _entrou) = estado_com_canal(None, false);
        let limite = std::time::Duration::from_millis(100);
        let leitor = state.channels.read().await;

        let estourou = desligar_canais(&state, limite).await;

        drop(leitor);
        assert_eq!(
            estourou,
            Some(limite),
            "com o write lock preso por um leitor, o desligamento ainda tem de respeitar o teto"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn canal_que_encerra_a_tempo_nao_avisa_estouro() {
        let (state, entrou) = estado_com_canal(Some(std::time::Duration::from_millis(1)), false);

        let estourou = desligar_canais(&state, std::time::Duration::from_secs(5)).await;

        assert_eq!(
            estourou, None,
            "caminho normal nao pode emitir aviso de teto"
        );
        assert_eq!(entrou.load(Ordering::SeqCst), 1);
    }

    /// Falha de um canal nao e estouro de prazo: o registro ja loga o erro por
    /// canal e o chamador nao pode avisar "exceeded" no caso errado.
    #[tokio::test(start_paused = true)]
    async fn canal_que_falha_ao_encerrar_nao_vira_aviso_de_teto() {
        let (state, entrou) = estado_com_canal(None, true);

        let estourou = desligar_canais(&state, std::time::Duration::from_secs(5)).await;

        assert_eq!(estourou, None);
        assert_eq!(entrou.load(Ordering::SeqCst), 1);
    }

    /// Os testes acima provam a funcao; este prova que o desligamento REAL a
    /// usa. Sem o teto, o `timeout` externo estoura e o teste FALHA - nao
    /// trava a suite (tempo pausado + prazo explicito).
    #[tokio::test(start_paused = true)]
    async fn o_desligamento_do_gateway_nao_passa_do_teto_com_canal_travado() {
        let (state, entrou) = estado_com_canal(None, false);

        tokio::time::timeout(
            CHANNELS_SHUTDOWN_TIMEOUT + std::time::Duration::from_secs(5),
            shutdown_subsystems(&state),
        )
        .await
        .expect(
            "shutdown_subsystems tem de voltar depois do teto dos canais; sem o teto ele espera \
             o disconnect pendurado para sempre",
        );

        assert_eq!(entrou.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn classify_stt_boot_mapeia_cada_resultado_do_probe() {
        assert_eq!(classify_stt_boot(Some(Ok(true))), SttBootLog::Ready);
        // `Ok(false)` do health_check e o timeout (`None`) sao o mesmo sinal
        // para o operador: Whisper nao esta alcancavel agora.
        assert_eq!(classify_stt_boot(Some(Ok(false))), SttBootLog::Unreachable);
        assert_eq!(classify_stt_boot(None), SttBootLog::Unreachable);
        assert_eq!(
            classify_stt_boot(Some(Err("connect failed".to_string()))),
            SttBootLog::ProbeError("connect failed".to_string())
        );
    }

    #[test]
    fn tls_meio_configurado_nomeia_o_campo_que_falta() {
        assert_eq!(tls_meio_configurado(None, None), None);
        assert_eq!(tls_meio_configurado(Some("c"), Some("k")), None);
        assert_eq!(
            tls_meio_configurado(Some("c"), None),
            Some("gateway.tls_key_path")
        );
        assert_eq!(
            tls_meio_configurado(None, Some("k")),
            Some("gateway.tls_cert_path")
        );
    }

    // ---- #1261: recusa de bind exposto sem credencial --------------------

    fn addr(s: &str) -> SocketAddr {
        s.parse().expect("socket addr de teste")
    }

    fn gateway_em(host: &str, api_key: Option<&str>) -> garraia_config::GatewayConfig {
        garraia_config::GatewayConfig {
            host: host.into(),
            port: 0,
            api_key: api_key.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn refuse_exposed_bind_recusa_sem_credencial() {
        for host in ["0.0.0.0", "[::]"] {
            let err = refuse_exposed_bind(&gateway_em(host, None))
                .expect_err("bind exposto sem credencial recusa");
            let msg = err.to_string();
            assert!(msg.contains("refusing to start"), "{msg}");
            assert!(msg.contains("GARRAIA_GATEWAY_API_KEY"), "{msg}");
            assert!(msg.contains("/ws"), "a mensagem nomeia o /ws: {msg}");
        }
    }

    /// Chave vazia ou so com espaco e gate desligado (#1241): recusa igual.
    #[test]
    fn credencial_em_branco_conta_como_ausente() {
        for valor in [None, Some(""), Some("   ")] {
            assert!(
                refuse_exposed_bind(&gateway_em("0.0.0.0", valor)).is_err(),
                "{valor:?}"
            );
        }
    }

    #[test]
    fn refuse_exposed_bind_sobe_com_credencial_loopback_ou_opt_out() {
        assert!(refuse_exposed_bind(&gateway_em("0.0.0.0", Some("uma-credencial"))).is_ok());
        assert!(refuse_exposed_bind(&gateway_em("127.0.0.1", None)).is_ok());
        assert!(refuse_exposed_bind(&gateway_em("[::1]", None)).is_ok());
        let mut g = gateway_em("0.0.0.0", None);
        g.allow_unauthenticated_network_bind = true;
        assert!(refuse_exposed_bind(&g).is_ok());
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn opt_out_avisa_alto_no_boot() {
        let mut g = gateway_em("0.0.0.0", None);
        g.allow_unauthenticated_network_bind = true;
        refuse_exposed_bind(&g).expect("opt-out sobe");
        assert!(logs_contain("allow_unauthenticated_network_bind is true"));
    }

    #[test]
    fn conferir_socket_segue_a_mesma_regra() {
        let sem = gateway_em("x", None);
        assert!(conferir_socket(&addr("0.0.0.0:3888"), &sem).is_err());
        assert!(conferir_socket(&addr("192.168.1.10:3888"), &sem).is_err());
        assert!(conferir_socket(&addr("127.0.0.1:3888"), &sem).is_ok());
        let com = gateway_em("x", Some("k"));
        assert!(conferir_socket(&addr("0.0.0.0:3888"), &com).is_ok());
    }

    // ---- FIACAO: `serve_plain` confere o socket real -------------------
    //
    // `serve_plain` so retorna no shutdown; com timeout, "serviu ate o
    // timeout" e "recusou" sao distinguiveis pelo resultado.

    /// Apagar a chamada em `serve_plain` tem que quebrar AQUI.
    #[tokio::test]
    async fn serve_plain_recusa_socket_exposto_sem_credencial() {
        let r = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            serve_plain("0.0.0.0:0", axum::Router::new(), &gateway_em("x", None)),
        )
        .await
        .expect("a recusa e imediata, nao pode esperar o timeout");
        let err = r.expect_err("socket exposto sem credencial recusa");
        assert!(err.to_string().contains("refusing to start"), "{err}");
    }

    /// O outro lado: com credencial `serve_plain` fica servindo.
    #[tokio::test]
    async fn serve_plain_serve_com_credencial() {
        let r = tokio::time::timeout(
            std::time::Duration::from_millis(400),
            serve_plain(
                "0.0.0.0:0",
                axum::Router::new(),
                &gateway_em("x", Some("k")),
            ),
        )
        .await;
        assert!(r.is_err(), "com credencial serve ate o timeout: {r:?}");
    }

    // ---- #1913: o dreno do caminho HTTP tem teto -----------------------

    /// Rota `/preso` cujo handler nunca responde, e o aviso de que a
    /// requisicao ja chegou nele.
    fn app_com_rota_presa() -> (axum::Router, Arc<tokio::sync::Notify>) {
        let chegou = Arc::new(tokio::sync::Notify::new());
        let aviso = Arc::clone(&chegou);
        let app = axum::Router::new().route(
            "/preso",
            axum::routing::get(move || {
                let aviso = Arc::clone(&aviso);
                async move {
                    aviso.notify_one();
                    std::future::pending::<()>().await;
                }
            }),
        );
        (app, chegou)
    }

    /// Comportamento: uma requisicao que nunca termina nao segura o
    /// encerramento alem do teto — o `serve` retorna `Ok`, avisa no log, e o
    /// chamador chega a `shutdown_subsystems`. **Mutacao**: troque o
    /// `timeout(teto, ..)` por um `.await` direto e este teste estoura o
    /// prazo externo (era o comportamento antigo).
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn dreno_http_respeita_o_teto_com_requisicao_presa() {
        use tokio::io::AsyncWriteExt;

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let porta = listener.local_addr().expect("local_addr");
        let (app, chegou) = app_com_rota_presa();
        let (dispara, sinal) = tokio::sync::oneshot::channel::<()>();
        let teto = std::time::Duration::from_millis(300);
        let servindo = tokio::spawn(servir_http_ate(
            listener,
            app,
            async move {
                let _ = sinal.await;
            },
            teto,
        ));

        // O cliente fica vivo ate o fim: fechar o socket nao e o que solta o
        // servidor aqui.
        let mut cliente = tokio::net::TcpStream::connect(porta)
            .await
            .expect("connect");
        cliente
            .write_all(b"GET /preso HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .expect("request");
        tokio::time::timeout(std::time::Duration::from_secs(5), chegou.notified())
            .await
            .expect("a requisicao chega ao handler");

        let inicio = std::time::Instant::now();
        dispara.send(()).expect("o servidor ainda espera o sinal");
        let fim = tokio::time::timeout(std::time::Duration::from_secs(5), servindo)
            .await
            .expect("o dreno nao passa do teto")
            .expect("a tarefa nao entra em panico");
        assert!(fim.is_ok(), "estourar o teto nao e erro: {fim:?}");
        assert!(
            inicio.elapsed() >= teto,
            "a requisicao aberta ganha o teto inteiro antes de ser largada"
        );
        assert!(logs_contain("HTTP connections still open after"));
        drop(cliente);
    }

    /// O outro lado: sem requisicao aberta o sinal encerra na hora, sem
    /// esperar o teto e sem o aviso.
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn dreno_http_sem_conexao_encerra_sem_esperar_o_teto() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let (dispara, sinal) = tokio::sync::oneshot::channel::<()>();
        let mut servindo = tokio::spawn(servir_http_ate(
            listener,
            axum::Router::new(),
            async move {
                let _ = sinal.await;
            },
            std::time::Duration::from_secs(60),
        ));

        if let Ok(cedo) =
            tokio::time::timeout(std::time::Duration::from_millis(300), &mut servindo).await
        {
            panic!("sem sinal o HTTP deveria seguir servindo, mas retornou: {cedo:?}");
        }

        dispara.send(()).expect("o servidor ainda espera o sinal");
        let fim = tokio::time::timeout(std::time::Duration::from_secs(5), servindo)
            .await
            .expect("sem conexao aberta o serve retorna sem esperar o teto")
            .expect("a tarefa nao entra em panico");
        assert!(fim.is_ok(), "encerramento gracioso nao e erro: {fim:?}");
        assert!(!logs_contain("HTTP connections still open after"));
    }

    /// FIACAO: `serve_plain` entrega o sinal real do processo e o teto de
    /// producao. Sem espacos: a quebra de linha do rustfmt nao e contrato.
    /// Sem linhas de comentario: uma chamada comentada, ou citada num
    /// docstring, nao pode satisfazer a guarda.
    #[test]
    fn serve_plain_drena_com_o_sinal_e_o_teto_de_producao() {
        let fonte: String = include_str!("server.rs")
            .lines()
            .filter(|linha| !linha.trim_start().starts_with("//"))
            .flat_map(str::split_whitespace)
            .collect();
        let inicio = fonte
            .find("asyncfnserve_plain(")
            .expect("serve_plain existe no fonte");
        let resto = &fonte[inicio..];
        let fim = resto
            .find("asyncfnservir_http_ate(")
            .expect("servir_http_ate vem depois de serve_plain");
        assert!(
            resto[..fim]
                .contains("servir_http_ate(listener,app,shutdown_signal(),HTTP_SHUTDOWN_GRACE)"),
            "serve_plain usa o dreno com teto"
        );
        // O orcamento da parada: dreno + MCP + canais abaixo do grace de 30 s
        // da CLI (`kill_and_wait`), nos dois caminhos.
        let soma = HTTP_SHUTDOWN_GRACE + MCP_SHUTDOWN_TIMEOUT + CHANNELS_SHUTDOWN_TIMEOUT;
        assert!(soma < std::time::Duration::from_secs(30), "{soma:?}");
    }

    // ---- #1913: o caminho TLS retorna no sinal -------------------------

    /// Guarda de FONTE, sem feature: nenhum job padrao compila `tls`, e sem
    /// isto apagar o `Handle` so apareceria no step dedicado. O `serve` do
    /// TLS tem de estar ligado a um `Handle`, e o sinal tem de dispara-lo.
    #[test]
    fn serve_tls_esta_ligado_ao_sinal_de_encerramento() {
        let fonte = include_str!("server.rs");
        let inicio = fonte
            .find("\nasync fn serve_tls_ate(")
            .expect("serve_tls_ate existe no fonte");
        let resto = &fonte[inicio..];
        let fim = resto
            .find("\nasync fn shutdown_signal(")
            .expect("shutdown_signal vem depois do TLS");
        let corpo = &resto[..fim];

        assert!(
            corpo.contains(".handle(handle.clone())"),
            "o serve do TLS tem de usar o Handle"
        );
        let espera = corpo
            .find("() = sinal =>")
            .expect("o sinal e esperado na tarefa do chamador");
        let dispara = corpo
            .find("handle.graceful_shutdown(Some(TLS_SHUTDOWN_GRACE))")
            .expect("o sinal dispara o encerramento com teto");
        assert!(espera < dispara, "o encerramento e disparado pelo sinal");
        assert!(
            !corpo.contains("tokio::spawn"),
            "sinal em tarefa a parte engole o panico do handler"
        );

        // Sem espacos: a quebra de linha do rustfmt nao e contrato.
        let entrada: String = fonte[..inicio].split_whitespace().collect();
        assert!(
            entrada.contains("gateway,shutdown_signal()).await")
                || entrada.contains("gateway,shutdown_signal(),).await"),
            "serve_tls entrega o sinal real do processo"
        );
    }

    /// Par cert/chave autoassinado gerado na hora pelo `openssl` da maquina
    /// — nenhuma chave privada fica versionada. `None` sem `openssl`.
    #[cfg(feature = "tls")]
    fn par_tls_de_teste(dir: &std::path::Path) -> Option<(String, String)> {
        let cert = dir.join("cert.pem");
        let key = dir.join("key.pem");
        let saida = std::process::Command::new("openssl")
            .args([
                "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
            ])
            .args(["-subj", "/CN=localhost", "-keyout"])
            .arg(&key)
            .arg("-out")
            .arg(&cert)
            .output()
            .ok()?;
        saida.status.success().then(|| {
            (
                cert.to_string_lossy().into_owned(),
                key.to_string_lossy().into_owned(),
            )
        })
    }

    /// Comportamento: sem sinal o TLS segue servindo; com o sinal, `serve`
    /// retorna `Ok` e o chamador chega a `shutdown_subsystems`. **Mutacao**:
    /// tire o `.handle(handle.clone())` e o `serve` nunca mais retorna.
    #[cfg(feature = "tls")]
    #[tokio::test]
    async fn serve_tls_retorna_quando_o_sinal_dispara() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some((cert, key)) = par_tls_de_teste(dir.path()) else {
            // No CI o step da feature `tls` existe para ESTE teste rodar:
            // pular la seria verde cobrindo nada.
            assert!(
                std::env::var_os("CI").is_none(),
                "openssl e obrigatorio no CI para o teste de TLS"
            );
            eprintln!("openssl indisponivel: teste de TLS pulado");
            return;
        };
        let (dispara, sinal) = tokio::sync::oneshot::channel::<()>();
        let mut servindo = tokio::spawn(async move {
            serve_tls_ate(
                "127.0.0.1:0",
                Some(&cert),
                Some(&key),
                axum::Router::new(),
                &gateway_em("127.0.0.1", None),
                async move {
                    let _ = sinal.await;
                },
            )
            .await
        });

        if let Ok(cedo) =
            tokio::time::timeout(std::time::Duration::from_millis(400), &mut servindo).await
        {
            panic!("sem sinal o TLS deveria seguir servindo, mas retornou: {cedo:?}");
        }

        dispara.send(()).expect("o servidor ainda espera o sinal");
        let fim = tokio::time::timeout(std::time::Duration::from_secs(5), servindo)
            .await
            .expect("com o sinal o serve retorna")
            .expect("a tarefa nao entra em panico");
        assert!(fim.is_ok(), "encerramento gracioso nao e erro: {fim:?}");
    }

    /// FIACAO do #1907: o aviso de transporte vive numa linha de log, entao o
    /// guarda le o FONTE do ponto de entrada. Apagar a chamada de
    /// `serve_plain` (ou o `warn!`) tem que quebrar aqui — sem isso, um bind
    /// exposto em HTTP em claro volta a ser silencioso sem falhar teste algum.
    #[test]
    fn serve_plain_avisa_bind_exposto_em_http_em_claro() {
        let fonte = include_str!("server.rs");
        let inicio = fonte
            .find("async fn serve_plain(")
            .expect("serve_plain existe no fonte");
        let resto = &fonte[inicio..];
        let fim = resto
            .find("\nasync fn serve_tls")
            .expect("serve_tls vem depois de serve_plain");
        let corpo = &resto[..fim];

        let gate = corpo
            .find("conferir_socket(&bound, gateway)?")
            .expect("gate");
        let aviso = corpo
            .find("garraia_config::bind::aviso_de_transporte(")
            .expect("serve_plain deve consultar o aviso (#1907)");
        let log = corpo.find("warn!(\"{aviso}\")").expect("warn do aviso");
        let listening = corpo.find("gateway listening on http").expect("listening");
        assert!(gate < aviso && aviso < log && log < listening);
        assert!(corpo[aviso..log].contains("std::slice::from_ref(&bound)"));

        let tls = &resto[fim..];
        let fim_tls = tls.find("async fn shutdown_signal(").expect("fim do TLS");
        assert!(!tls[..fim_tls].contains("bind::aviso_de_transporte"));
    }

    /// A borda que importa: cresce exponencialmente, satura no teto, e não
    /// estoura com contagens absurdas de tentativa.
    #[test]
    fn retry_delay_grows_and_caps() {
        assert_eq!(channel_retry_delay(0).as_secs(), 2);
        assert_eq!(channel_retry_delay(1).as_secs(), 4);
        assert_eq!(channel_retry_delay(3).as_secs(), 16);
        assert_eq!(channel_retry_delay(5).as_secs(), 60, "teto");
        assert_eq!(channel_retry_delay(u32::MAX).as_secs(), 60, "sem overflow");
    }

    /// Canal que falha N vezes e então conecta — o cenário da #928 em
    /// miniatura: rede ruim nos primeiros segundos do boot.
    struct FlakyChannel {
        fails_left: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl garraia_channels::Channel for FlakyChannel {
        fn channel_type(&self) -> &str {
            "flaky-telegram"
        }
        fn display_name(&self) -> &str {
            "Flaky"
        }
        async fn connect(&mut self) -> Result<()> {
            if self.fails_left.fetch_sub(1, Ordering::SeqCst) > 1 {
                Err(garraia_common::Error::Channel("network down".into()))
            } else {
                Ok(())
            }
        }
        async fn disconnect(&mut self) -> Result<()> {
            Ok(())
        }
        async fn send_message(&self, _m: &Message) -> Result<()> {
            Ok(())
        }
        fn status(&self) -> garraia_channels::ChannelStatus {
            garraia_channels::ChannelStatus::Connected
        }
    }

    /// Fiacao do canal `whatsapp_linked` (#1238, fatia D).
    ///
    /// `Server::run` nao e chamavel de um teste unitario (abre socket, monta
    /// admin store, entra no `serve`), entao esta e a forma honesta de pinar a
    /// chamada: varre o proprio fonte. E fraco de proposito, e esta escrito
    /// aqui que e fraco — mas e a diferenca entre "o supervisor nunca sobe" ser
    /// pego pelo CI e ser pego pelo usuario. E a licao do comando que se
    /// chamava `whats-app` com 599 testes verdes: toda funcao bem testada
    /// precisa de alguem provando que ela e CHAMADA.
    ///
    /// **E so isso que ele prova.** Ele ficou verde durante toda a vida do bug
    /// em que o `watch::Sender` devolvido morria no fim do braco do `match` e
    /// matava o canal em ~0,1 s: a chamada estava la, o resultado e que nao
    /// era retido. Quem prova a retencao e a entrega da mensagem e
    /// `bootstrap::whatsapp_linked::tests::ponta_a_ponta::
    /// o_boot_retem_o_supervisor_e_a_mensagem_chega_ao_agente`, contra a ponte
    /// falsa e sem segurar handle nenhum. Hoje a mutacao "soltar o handle"
    /// nem e expressavel — `spawn_whatsapp_linked` devolve `()`.
    #[test]
    fn o_boot_chama_o_supervisor_do_whatsapp_vinculado() {
        let fonte = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/server.rs"),
        )
        .expect("fonte legivel");
        // So o codigo de producao: o proprio corpo deste teste cita o nome da
        // funcao, e sem este corte ele casaria consigo mesmo — que e como um
        // teste de fiacao vira decoracao. (Verificado por mutacao: com o corte,
        // apagar a chamada do boot deixa este teste vermelho.)
        let producao = fonte
            .split_once("\nmod tests {")
            .map(|(antes, _)| antes.to_string())
            .unwrap_or_else(|| fonte.clone());
        let codigo: String = producao
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            codigo.contains("spawn_whatsapp_linked(&state)"),
            "o boot precisa chamar `spawn_whatsapp_linked`; sem isso o canal \
             nunca sobe e nenhum outro teste percebe"
        );
    }

    /// O comportamento que a #928 pedia sem saber: uma falha transitória de
    /// boot não pode mais custar o canal — o retry de fundo acaba
    /// registrando-o no `ChannelRegistry`, de onde `send_message` o alcança.
    #[tokio::test(start_paused = true)]
    async fn boot_retry_eventually_registers_the_channel() {
        let state = Arc::new(AppState::new(
            AppConfig::default(),
            Arc::new(garraia_agents::AgentRuntime::new()),
            garraia_channels::ChannelRegistry::new(),
        ));

        // Falha 3 vezes (2s + 4s + 8s de backoff), conecta na 4ª.
        let channel = FlakyChannel {
            fails_left: Arc::new(AtomicUsize::new(4)),
        };
        spawn_channel_connect_retry(Arc::clone(&state), Box::new(channel));

        // Com o relógio pausado, cada advance dispara o sleep pendente.
        for _ in 0..8 {
            tokio::time::advance(std::time::Duration::from_secs(60)).await;
            tokio::task::yield_now().await;
        }

        assert!(
            state.channels.read().await.get("flaky-telegram").is_some(),
            "canal deveria ter sido registrado após os retries"
        );
    }

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

    /// #1227 (slice 1): a subida do gateway tem de marcar runs `running`
    /// deixados por uma queda — a chamada vive no bloco `Ok(store)` do
    /// `SessionStore::open`. Se alguém a remover, a tabela volta a acumular
    /// `running` eterno e o restart perde a trilha de auditoria. O scan usa
    /// `concat!` para o próprio teste não casar consigo mesmo.
    #[test]
    fn subida_do_store_marca_runs_interrompidos() {
        let src = include_str!("server.rs");
        let alvo = concat!("log_interrupted", "_runs(&store)");
        let copias = src.matches(alvo).count();
        assert_eq!(copias, 1, "esperava 1 chamada de subida, achei {copias}");
    }

    /// #1227 (slice 2): o scheduler tem de REIVINDICAR as tarefas vencidas
    /// (`claim_due_tasks`, que as poe em `running` sob lease), nunca so
    /// ler (`poll_due_tasks`). Com o poll, uma queda no meio do turno
    /// deixava a linha `pending` e o tick seguinte reexecutava em silencio
    /// — mensagem de sistema duplicada. O scan recorta o corpo de
    /// `run_scheduler` e usa `concat!` para nao casar consigo mesmo.
    #[test]
    fn scheduler_reivindica_em_vez_de_pollar() {
        // Fins de linha normalizados: o checkout do Windows e CRLF e o scan
        // procura `"\n}\n"` (#1762).
        let src = include_str!("server.rs").replace("\r\n", "\n");
        let assinatura = concat!("async fn run_", "scheduler(");
        let inicio = src
            .find(assinatura)
            .expect("run_scheduler tem de existir em server.rs");
        let resto = &src[inicio..];
        let fim = resto
            .find("\n}\n")
            .expect("run_scheduler tem de fechar em coluna zero");
        let corpo = &resto[..fim];

        assert!(
            corpo.contains(concat!("claim_due", "_tasks(")),
            "run_scheduler deve reivindicar via claim_due_tasks (#1227 slice 2)"
        );
        assert!(
            !corpo.contains(concat!("poll_due", "_tasks")),
            "run_scheduler nao pode voltar ao poll read-only — reexecucao silenciosa apos queda"
        );
        assert!(
            corpo.contains(concat!("log_recovered", "_leases(&store)")),
            "cada tick deve recuperar leases expiradas antes de reivindicar"
        );
    }

    /// #1227 (slice 2): a recuperacao de leases roda em DOIS lugares — na
    /// subida (bloco `Ok(store)` do `SessionStore::open`, ao lado de
    /// `log_interrupted_runs`) e no inicio de cada tick. Sem a subida, uma
    /// tarefa que caiu com lease longa espera ate 10 min para ser vista;
    /// sem o tick, uma queda de OUTRO processo sobre o mesmo arquivo nunca
    /// e vista.
    #[test]
    fn leases_expiradas_sao_recuperadas_na_subida_e_no_tick() {
        let src = include_str!("server.rs");
        let alvo = concat!("log_recovered", "_leases(&store)");
        let copias = src.matches(alvo).count();
        assert_eq!(
            copias, 2,
            "esperava 2 chamadas (subida + tick), achei {copias}"
        );
    }

    fn tarefa_fixura() -> garraia_db::ScheduledTask {
        garraia_db::ScheduledTask {
            id: "task-1".to_string(),
            session_id: "sess-1".to_string(),
            channel_id: "telegram".to_string(),
            user_id: "user-1".to_string(),
            execute_at: chrono::Utc::now(),
            payload: "verificar o build".to_string(),
            session_metadata: serde_json::json!({}),
            cron_expr: None,
            timezone: None,
            run_count: 0,
            max_runs: None,
            attempts: 0,
            authority: None,
        }
    }

    /// #1227 (slice 1): cada execucao agendada deixa uma linha em
    /// `agent_runs` com `session_id` preenchido e desfecho terminal — a
    /// queda no meio vira `interrupted` no restart seguinte, em vez de sumir.
    #[tokio::test]
    async fn run_agendado_grava_linha_terminal_com_sessao() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(Mutex::new(
            SessionStore::open(&tmp.path().join("sessions.db")).unwrap(),
        ));
        let task = tarefa_fixura();

        let run_id = ledger_inicia_run_agendado(&store, &task).await;
        ledger_fecha_run_agendado(
            &store,
            &run_id,
            &Ok((
                "resposta do heartbeat".to_string(),
                crate::entrega_agendada::Entrega::NaoEntregue(
                    crate::entrega_agendada::Motivo::PonteForaDoAr,
                ),
            )),
        )
        .await;

        let runs = store.lock().await.list_recent_agent_runs(10).unwrap();
        assert_eq!(runs.len(), 1, "uma execucao, uma linha");
        assert_eq!(runs[0].status, garraia_db::RunStatus::Done);
        assert_eq!(runs[0].session_id.as_deref(), Some("sess-1"));
        assert_eq!(
            runs[0].result_snippet.as_deref(),
            Some("resposta do heartbeat")
        );
        // #2002: o ledger diz se a resposta chegou ao canal.
        assert_eq!(
            runs[0].delivery.as_deref(),
            Some("nao_entregue:ponte_fora_do_ar")
        );
    }

    /// Falha da execucao fecha o run com `error` e snippet do erro — nunca
    /// fica `running` para sempre esperando o restart.
    #[tokio::test]
    async fn run_agendado_com_erro_fecha_com_status_error() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(Mutex::new(
            SessionStore::open(&tmp.path().join("sessions.db")).unwrap(),
        ));
        let task = tarefa_fixura();

        let run_id = ledger_inicia_run_agendado(&store, &task).await;
        ledger_fecha_run_agendado(
            &store,
            &run_id,
            &Err(garraia_common::Error::Agent("boom".to_string())),
        )
        .await;

        let runs = store.lock().await.list_recent_agent_runs(10).unwrap();
        assert_eq!(runs[0].status, garraia_db::RunStatus::Error);
        assert_eq!(runs[0].delivery, None, "erro nao chega a entregar");
        assert!(
            runs[0].error_snippet.as_deref().unwrap().contains("boom"),
            "snippet de erro presente"
        );
    }
}
