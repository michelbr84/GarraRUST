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
            // Plan 0250 (GAR-771): warmer, PT-BR help header in Garra's voice.
            let mut help = String::from(
                "Claro! 🐾 Aqui está o que eu sei fazer por atalho.\n\
                 Mas lembra: você não precisa decorar nada — é só falar comigo \
                 normalmente.\n\n",
            );
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
            // GAR-202 + #982: a chave vem do MESMO resolvedor que a execucao
            // usa. Montar `telegram-{chat_id}` aqui gravava numa linha que o
            // runtime nunca lia — ver `AppState::telegram_session_id`.
            let session_id = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    state
                        .telegram_session_id(ctx.chat_id, ctx.user_id.parse::<i64>().ok())
                        .await
                })
            }); // Note: Discord uses strings, but chat_id is i64 here... we'll just use it for telegram for now
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
            // GAR-202 + #982: a chave vem do MESMO resolvedor que a execucao
            // usa. Montar `telegram-{chat_id}` aqui gravava numa linha que o
            // runtime nunca lia — ver `AppState::telegram_session_id`.
            let session_id = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    state
                        .telegram_session_id(ctx.chat_id, ctx.user_id.parse::<i64>().ok())
                        .await
                })
            });
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
            // Sem `unwrap()`: regra absoluta 4. O padrao herdado neste arquivo
            // derruba o handler se o estado faltar ou vier de outro tipo, e um
            // comando de chat nao deveria conseguir isso.
            let Some(state) = ctx
                .state
                .as_ref()
                .and_then(|s| s.downcast_ref::<AppState>())
            else {
                return Ok("⚠️ Estado indisponivel para este comando.".to_string());
            };

            let session_id = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    state
                        .telegram_session_id(ctx.chat_id, ctx.user_id.parse::<i64>().ok())
                        .await
                })
            });

            let mut out = String::from("📊 **Estatisticas**\n");

            // ── O turno: o que foi usado de verdade (#984) ──────────────────
            //
            // A issue e explicita: provider/modelo **configurado** nao e o
            // efetivo, porque o runtime resolve override, prefixo de modelo,
            // `tools_model` e fallback pelo caminho. Entao isto vem do registro
            // que o runtime escreve no fim do turno, e nao da config.
            match state.agents.last_turn_stats(&session_id) {
                Some(t) => {
                    out.push_str("\n**Ultima resposta**\n");
                    out.push_str(&format!("- Provider: {}\n", t.provider));
                    out.push_str(&format!(
                        "- Modelo: {}{}\n",
                        t.model,
                        if t.model_confirmado {
                            ""
                        } else {
                            " *(pedido; o streaming nao reporta o efetivo)*"
                        }
                    ));
                    if t.fallback {
                        out.push_str("- ⚠️ Respondido por **fallback**\n");
                    }
                    out.push_str(&format!("- Ferramentas executadas: {}\n", t.tool_calls));
                    if t.tokens_conhecidos {
                        out.push_str(&format!(
                            "- Tokens: {} entrada / {} saida\n",
                            t.input_tokens, t.output_tokens
                        ));
                    } else {
                        out.push_str("- Tokens: *(nao informados pelo provider)*\n");
                    }
                    out.push_str(&format!("- Latencia: {} ms\n", t.latency_ms));
                }
                None => {
                    out.push_str("\n**Ultima resposta**: *(nenhum turno nesta sessao ainda)*\n");
                }
            }

            // ── A sessao: modo aplicado e objetivo ──────────────────────────
            //
            // "Modo aplicado, nao apenas o modo salvo": o #988 separou escolha
            // de deducao, e so a escolha liga a `ToolPolicy`. Mostrar so o modo
            // salvo faria o usuario acreditar numa restricao que nao vale.
            if let Some(store) = &state.session_store {
                let store = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(async { store.lock().await })
                });
                out.push_str("\n**Sessao**\n");
                let salvo = store.get_agent_mode(&session_id).ok().flatten();
                let escolhido = store.get_chosen_agent_mode(&session_id).ok().flatten();
                match (&salvo, &escolhido) {
                    (Some(m), Some(_)) => {
                        out.push_str(&format!("- Modo: {m} *(escolhido — a politica vale)*\n"))
                    }
                    (Some(m), None) => out.push_str(&format!(
                        "- Modo: {m} *(deduzido — a politica **nao** vale; use `/mode {m}` para \
                         aplica-la)*\n"
                    )),
                    _ => out.push_str("- Modo: nenhum escolhido *(sem restricao de ferramenta)*\n"),
                }
                match store.get_session_goal(&session_id) {
                    Ok(Some(goal)) => out.push_str(&format!("- Objetivo: {goal}\n")),
                    _ => out.push_str("- Objetivo: *(nenhum)*\n"),
                }
            }

            // ── O processo, para quem administra ────────────────────────────
            out.push_str(&format!(
                "\n**Processo**\n- Sessoes ativas: {}\n- Overrides de modelo: {}\n- Tarefas A2A \
                 em voo: {}\n",
                state.sessions.len(),
                state.channel_models.len(),
                state.a2a_tasks.len()
            ));

            Ok(out)
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

    // #983: /goal - objetivo persistente da sessao
    registry.register(Box::new(ClosureCommand::new(
        "goal",
        "Get, set or clear the session goal",
        "/goal [texto|clear]",
        Role::User,
        true,
        |ctx: &CommandContext| -> CommandResult {
            // Sem `unwrap()`: a regra absoluta 4 vale aqui. O padrao herdado
            // neste arquivo e `ctx.state.as_ref().unwrap().downcast_ref().unwrap()`,
            // que derruba o handler se o estado faltar ou vier de outro tipo —
            // e um comando de chat nao deveria conseguir isso.
            let Some(state) = ctx
                .state
                .as_ref()
                .and_then(|s| s.downcast_ref::<AppState>())
            else {
                return Ok("⚠️ Estado indisponivel para este comando.".to_string());
            };

            // A mesma chave que a execucao usa (GAR-202 + #982): montar
            // `telegram-{chat_id}` aqui gravaria numa linha que o runtime nunca
            // le.
            let session_id = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    state
                        .telegram_session_id(ctx.chat_id, ctx.user_id.parse::<i64>().ok())
                        .await
                })
            });

            let Some(store) = &state.session_store else {
                return Ok("⚠️ Sem armazenamento de sessao: o objetivo nao persiste.".to_string());
            };
            let store = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async { store.lock().await })
            });

            let pedido = ctx.args.join(" ");
            let pedido = pedido.trim();

            if pedido.is_empty() {
                return match store.get_session_goal(&session_id) {
                    Ok(Some(goal)) => Ok(format!("🎯 Objetivo da sessao: {goal}")),
                    Ok(None) => Ok(
                        "🎯 Nenhum objetivo definido. Use `/goal <texto>` para definir.".to_string(),
                    ),
                    Err(e) => {
                        tracing::warn!(session_id = %session_id, erro = %e, "falhou ao ler o objetivo");
                        Ok("⚠️ Nao consegui ler o objetivo desta sessao.".to_string())
                    }
                };
            }

            if pedido.eq_ignore_ascii_case("clear") {
                return match store.clear_session_goal(&session_id) {
                    Ok(()) => Ok("🎯 Objetivo removido.".to_string()),
                    Err(e) => {
                        tracing::warn!(session_id = %session_id, erro = %e, "falhou ao limpar o objetivo");
                        Ok("⚠️ Nao consegui remover o objetivo. Mande uma mensagem primeiro e tente de novo.".to_string())
                    }
                };
            }

            // Nao engula o erro, pela mesma razao do `/mode`: `set_*` falha
            // quando a linha da sessao ainda nao existe, e responder "objetivo
            // definido" com o banco intacto e a falha que o #1008 corrigiu.
            match store.set_session_goal(&session_id, pedido) {
                Ok(()) => Ok(format!("🎯 Objetivo definido: {pedido}")),
                Err(e) => {
                    tracing::warn!(session_id = %session_id, erro = %e, "falhou ao gravar o objetivo");
                    Ok("⚠️ Nao consegui salvar o objetivo. Mande uma mensagem primeiro e tente de novo.".to_string())
                }
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
            // GAR-202 + #982: a chave vem do MESMO resolvedor que a execucao
            // usa. Montar `telegram-{chat_id}` aqui gravava numa linha que o
            // runtime nunca lia — ver `AppState::telegram_session_id`.
            let session_id = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    state
                        .telegram_session_id(ctx.chat_id, ctx.user_id.parse::<i64>().ok())
                        .await
                })
            });

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
                        // Nao engula o erro: `set_agent_mode` falha quando a
                        // linha da sessao ainda nao existe, e o `let _ =` de
                        // antes transformava isso em "modo definido" para o
                        // usuario com o banco intacto.
                        if let Err(e) = store.set_agent_mode(&session_id, &new_mode.to_lowercase())
                        {
                            tracing::warn!(
                                session_id = %session_id,
                                erro = %e,
                                "falhou ao gravar o modo escolhido"
                            );
                            return Ok(format!(
                                "⚠️ Nao consegui salvar o modo `{}`. Mande uma mensagem \
                                 primeiro e tente de novo.",
                                new_mode.to_lowercase()
                            ));
                        }
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
