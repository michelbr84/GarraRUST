use crate::state::AppState;
use garraia_agents::AgentMode;
use garraia_channels::commands::{
    ClosureCommand, CommandContext, CommandRegistry, CommandResult, Role,
};
pub fn register_commands(registry: &mut CommandRegistry) {
    // Overwrite the placeholders with actual logic that accesses AppState

    // /start
    registry.register(Box::new(ClosureCommand::new(
        "start",
        "Start interacting with the bot",
        "/start",
        Role::User,
        true,
        |ctx: &CommandContext| -> CommandResult {
            let state: &AppState = ctx.state.as_ref().unwrap().downcast_ref::<AppState>().unwrap();
            let mut list = state.allowlist.lock().unwrap();
            let is_allowed = list.is_allowed(&ctx.user_id);
            if is_allowed {
                Ok("Welcome to GarraIA! Send me a message and I will respond.\n\nType /help to see available commands.".to_string())
            } else if list.needs_owner() {
                list.claim_owner(&ctx.user_id);
                Ok(format!(
                    "Welcome, {}! You are now the owner of this GarraIA bot.\n\nUse /pair to generate a code for adding other users.",
                    ctx.user_name
                ))
            } else {
                Ok("This bot is private. Send the 6-digit pairing code you received to get access.".to_string())
            }
        },
    )));

    // /help
    registry.register(Box::new(ClosureCommand::new(
        "help",
        "Show available commands",
        "/help",
        Role::User,
        true,
        |ctx: &CommandContext| -> CommandResult {
            let state = ctx
                .state
                .as_ref()
                .unwrap()
                .downcast_ref::<AppState>()
                .unwrap();
            let registry = state.command_registry.read().unwrap();
            let cmds = registry.list_for_role(ctx.user_role);
            let mut help = String::from("📋 GarraIA Commands\n\n");
            for (name, desc) in &cmds {
                help.push_str(&format!("/{name} — {desc}\n"));
            }
            Ok(help)
        },
    )));

    // /clear
    registry.register(Box::new(ClosureCommand::new(
        "clear",
        "Clear current conversation history",
        "/clear",
        Role::User,
        true,
        |ctx: &CommandContext| -> CommandResult {
            let state = ctx
                .state
                .as_ref()
                .unwrap()
                .downcast_ref::<AppState>()
                .unwrap();
            let session_id = format!("telegram-{}", ctx.chat_id); // Note: Discord uses strings, but chat_id is i64 here... we'll just use it for telegram for now
            if let Some(mut session) = state.sessions.get_mut(&session_id) {
                session.history.clear();
            }
            Ok("🗑️ Conversation history cleared.".to_string())
        },
    )));

    // /model
    registry.register(Box::new(ClosureCommand::new(
        "model",
        "Get or set the LLM model",
        "/model [name|clear|default]",
        Role::User,
        true,
        |ctx: &CommandContext| -> CommandResult {
            let state = ctx
                .state
                .as_ref()
                .unwrap()
                .downcast_ref::<AppState>()
                .unwrap();
            let session_id = format!("telegram-{}", ctx.chat_id);
            if ctx.args.is_empty() {
                let current = state.channel_models.get(&session_id);
                if let Some(m) = current {
                    Ok(format!("🤖 Current model: {}", m.value()))
                } else {
                    Ok("🤖 No model override set. Using default.".to_string())
                }
            } else {
                let new_model = ctx.args[0].to_string();
                if new_model.eq_ignore_ascii_case("clear")
                    || new_model.eq_ignore_ascii_case("default")
                {
                    state.channel_models.remove(&session_id);
                    Ok("🤖 Model override cleared. Using default.".to_string())
                } else {
                    state.channel_models.insert(session_id, new_model.clone());
                    Ok(format!("🤖 Model set to: {}", new_model))
                }
            }
        },
    )));

    // /pair
    registry.register(Box::new(ClosureCommand::new(
        "pair",
        "Generate a 6-digit invite code",
        "/pair",
        Role::Owner,
        true,
        |ctx: &CommandContext| -> CommandResult {
            let state = ctx.state.as_ref().unwrap().downcast_ref::<AppState>().unwrap();
            let code = state.pairing.lock().unwrap().generate("telegram");
            Ok(format!("🔗 Pairing code: {code}\n\nShare this with the person you want to invite. They should send this code to the bot within 5 minutes."))
        }
    )));

    // /users
    registry.register(Box::new(ClosureCommand::new(
        "users",
        "List allowed users",
        "/users",
        Role::Owner,
        true,
        |ctx: &CommandContext| -> CommandResult {
            let state = ctx
                .state
                .as_ref()
                .unwrap()
                .downcast_ref::<AppState>()
                .unwrap();
            let list = state.allowlist.lock().unwrap();
            let users = list.list_users();
            let owner = list.owner().unwrap_or("none");
            Ok(format!(
                "👥 Owner: {owner}\nAllowed users ({}):\n{}",
                users.len(),
                users.join("\n")
            ))
        },
    )));

    // Commands that don't do anything yet but are registered
    registry.register(Box::new(ClosureCommand::new(
        "voz",
        "Ativar interação por vóz",
        "/voz",
        Role::User,
        true,
        |_ctx: &CommandContext| -> CommandResult { Ok("🎙️ Em breve...".to_string()) },
    )));

    registry.register(Box::new(ClosureCommand::new(
        "voice",
        "Voice pipeline",
        "/voice",
        Role::User,
        true,
        |_ctx: &CommandContext| -> CommandResult { Ok("🎙️ Em breve...".to_string()) },
    )));

    registry.register(Box::new(ClosureCommand::new(
        "health",
        "Check system health",
        "/health",
        Role::Admin,
        true,
        |ctx: &CommandContext| -> CommandResult {
            let state = ctx
                .state
                .as_ref()
                .unwrap()
                .downcast_ref::<AppState>()
                .unwrap();
            if let Some(cache) = &state.health_cache {
                if let Ok(checks) = cache.try_read() {
                    let mut output = String::from("🏥 System Health:\n\n");
                    for check in checks.iter() {
                        let icon = if check.ok { "✅" } else { "❌" };
                        let latency = check
                            .latency_ms
                            .map(|ms| format!("{}ms", ms))
                            .unwrap_or_else(|| "-".to_string());
                        output.push_str(&format!("{} **{}**: {}\n", icon, check.name, latency));
                        if let Some(err) = &check.error {
                            output.push_str(&format!("  └ ⚠️ Error: {}\n", err));
                        }
                    }
                    Ok(output)
                } else {
                    Ok("🏥 Health monitoring is temporarily busy".to_string())
                }
            } else {
                Ok("🏥 Health monitoring not active".to_string())
            }
        },
    )));

    registry.register(Box::new(ClosureCommand::new(
        "providers",
        "Check configured LLM providers",
        "/providers",
        Role::Admin,
        true,
        |ctx: &CommandContext| -> CommandResult {
            let state = ctx
                .state
                .as_ref()
                .unwrap()
                .downcast_ref::<AppState>()
                .unwrap();
            let mut list = String::from("🔌 **Configured LLM Providers**\n\n");
            for name in state.config.llm.keys() {
                list.push_str(&format!("- **{}**\n", name));
            }
            if state.config.llm.is_empty() {
                list.push_str("*(None)*\n");
            }
            Ok(list)
        },
    )));

    registry.register(Box::new(ClosureCommand::new(
        "stats",
        "Show system statistics",
        "/stats",
        Role::Admin,
        true,
        |ctx: &CommandContext| -> CommandResult {
            let state = ctx
                .state
                .as_ref()
                .unwrap()
                .downcast_ref::<AppState>()
                .unwrap();
            let sessions_count = state.sessions.len();
            let overrides_count = state.channel_models.len();
            let a2a_tasks = state.a2a_tasks.len();
            Ok(format!(
                "📊 **System Stats**\n\nActive Sessions: {}\nModel Overrides: {}\nIn-flight A2A Tasks: {}",
                sessions_count, overrides_count, a2a_tasks
            ))
        },
    )));

    registry.register(Box::new(ClosureCommand::new(
        "config",
        "Show system config summary",
        "/config",
        Role::Owner,
        true,
        |ctx: &CommandContext| -> CommandResult {
            let state = ctx
                .state
                .as_ref()
                .unwrap()
                .downcast_ref::<AppState>()
                .unwrap();
            let channels = if state.config.channels.is_empty() { "None".to_string() } else { state.config.channels.keys().cloned().collect::<Vec<_>>().join(", ") };
            let llms = if state.config.llm.is_empty() { "None".to_string() } else { state.config.llm.keys().cloned().collect::<Vec<_>>().join(", ") };
            let agents = if state.config.agents.is_empty() { "None".to_string() } else { state.config.agents.keys().cloned().collect::<Vec<_>>().join(", ") };
            let log_level = state.config.log_level.as_deref().unwrap_or("info");

            Ok(format!(
                "⚙️ **Configuration Summary**\n\nChannels: {}\nLLM Providers: {}\nAgents: {}\nLog Level: {}",
                channels,
                llms,
                agents,
                log_level
            ))
        },
    )));

    registry.register(Box::new(ClosureCommand::new(
        "mcp",
        "List configured MCP servers",
        "/mcp",
        Role::Owner,
        true,
        |ctx: &CommandContext| -> CommandResult {
            let state = ctx
                .state
                .as_ref()
                .unwrap()
                .downcast_ref::<AppState>()
                .unwrap();
            let servers = state.config.mcp.keys().cloned().collect::<Vec<_>>();
            if servers.is_empty() {
                Ok("🔌 No MCP servers configured.".to_string())
            } else {
                Ok(format!(
                    "🔌 **Configured MCP Servers**\n\n- {}",
                    servers.join("\n- ")
                ))
            }
        },
    )));

    // GAR-223: /mode - Get or set agent mode
    registry.register(Box::new(ClosureCommand::new(
        "mode",
        "Get or set the agent mode",
        "/mode [name|clear]",
        Role::User,
        true,
        |ctx: &CommandContext| -> CommandResult {
            let state = ctx
                .state
                .as_ref()
                .unwrap()
                .downcast_ref::<AppState>()
                .unwrap();
            let session_id = format!("telegram-{}", ctx.chat_id);

            if ctx.args.is_empty() {
                // Show current mode
                if let Some(store) = &state.session_store {
                    let store = tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current().block_on(async { store.lock().await })
                    });
                    match store.get_agent_mode(&session_id) {
                        Ok(Some(mode)) => Ok(format!("🎯 Current mode: {}", mode)),
                        Ok(None) => Ok("🎯 Current mode: ask (default)".to_string()),
                        Err(_) => Ok("🎯 Current mode: ask (default)".to_string()),
                    }
                } else {
                    Ok("🎯 Current mode: ask (default)".to_string())
                }
            } else {
                let new_mode = ctx.args[0].to_string();
                if new_mode.eq_ignore_ascii_case("clear") {
                    // Clear mode (reset to default)
                    if let Some(store) = &state.session_store {
                        let store = tokio::task::block_in_place(|| {
                            tokio::runtime::Handle::current().block_on(async { store.lock().await })
                        });
                        let _ = store.clear_agent_mode(&session_id);
                    }
                    Ok("🎯 Mode cleared. Using default (ask).".to_string())
                } else if AgentMode::from_str(&new_mode).is_some() {
                    // Set mode
                    if let Some(store) = &state.session_store {
                        let store = tokio::task::block_in_place(|| {
                            tokio::runtime::Handle::current().block_on(async { store.lock().await })
                        });
                        let _ = store.set_agent_mode(&session_id, &new_mode.to_lowercase());
                    }
                    Ok(format!("🎯 Mode set to: {}", new_mode.to_lowercase()))
                } else {
                    Ok(format!(
                        "❌ Unknown mode: {}\nUse /modes to see available modes.",
                        new_mode
                    ))
                }
            }
        },
    )));

    // GAR-223: /modes - List available modes
    registry.register(Box::new(ClosureCommand::new(
        "modes",
        "List available agent modes",
        "/modes",
        Role::User,
        true,
        |_ctx: &CommandContext| -> CommandResult {
            let modes = vec![
                ("auto", "Decides automatically based on content"),
                ("search", "Search and inspection (read-only)"),
                ("architect", "Design and planning"),
                ("code", "Active implementation (allows file writes)"),
                ("ask", "Questions only (Telegram default)"),
                ("debug", "Debug errors and stack traces"),
                ("orchestrator", "Multi-step execution"),
                ("review", "Code review and analysis"),
                ("edit", "Precise file editing"),
            ];

            let mut output = String::from("🎯 **Available Modes**\n\n");
            for (name, desc) in modes {
                output.push_str(&format!("• **{}** — {}\n", name, desc));
            }
            output.push_str("\nUse /mode <name> to change mode.");
            Ok(output)
        },
    )));
}
