//! GAR-184: Dynamic slash commands powered by MCP prompts.
//!
//! Slash commands are triggered when a user message starts with `/`.
//!
//! ## Sources
//!
//! - **Built-in** (`/help`): always available; lists all commands.
//! - **MCP prompts**: discovered at runtime from connected MCP servers.
//!   Each prompt exposed by an MCP server becomes an invocable slash command.
//!
//! ## Argument parsing
//!
//! Arguments are parsed from the text after the command name:
//! - `key=value` pairs: `/summarize text=hello lang=pt` → `{"text":"hello","lang":"pt"}`
//! - Free-form text (no `=`): `/summarize hello world` → `{"input":"hello world"}`
//!
//! ## Integration
//!
//! `resolve()` is called from `openai_api.rs` after building the message list.
//! If it returns `Some(McpPrompt)`, the prompt messages replace the slash command
//! message in the conversation and are forwarded to the LLM.
//!
//! The `/mode` command is intentionally **excluded** — it is handled by the
//! existing `message_mode` logic in `openai_api.rs`.

use std::sync::Arc;

use garraia_agents::{ChatMessage, ChatRole, McpManager, MessagePart};
use serde::Serialize;
use tracing::warn;

// ── Public types ──────────────────────────────────────────────────────────────

/// A discoverable slash command (built-in or from an MCP server).
#[derive(Debug, Clone, Serialize)]
pub struct SlashCommand {
    pub name: String,
    pub description: String,
    pub source: CommandSource,
    pub args: Vec<ArgDef>,
}

/// Where the command originates.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandSource {
    BuiltIn,
    Mcp { server: String },
}

/// Definition of a command argument.
#[derive(Debug, Clone, Serialize)]
pub struct ArgDef {
    pub name: String,
    pub description: Option<String>,
    pub required: bool,
}

/// Result of resolving a slash command invocation.
pub enum ResolvedCommand {
    /// Inject MCP prompt messages as the conversation context for the LLM.
    McpPrompt(Vec<ChatMessage>),
}

// ── Built-in commands ─────────────────────────────────────────────────────────

/// Built-in commands that are always available. `/mode` is excluded (handled elsewhere).
const BUILT_INS: &[(&str, &str)] = &[("help", "List all available slash commands")];

// ── Public API ────────────────────────────────────────────────────────────────

/// List all available slash commands: built-ins + MCP prompts from connected servers.
///
/// Used by `GET /api/slash-commands` for client auto-complete.
pub async fn list_commands(mcp: Option<&Arc<McpManager>>) -> Vec<SlashCommand> {
    let mut commands: Vec<SlashCommand> = BUILT_INS
        .iter()
        .map(|(name, desc)| SlashCommand {
            name: name.to_string(),
            description: desc.to_string(),
            source: CommandSource::BuiltIn,
            args: vec![],
        })
        .collect();

    // Also expose /mode as a built-in (for discovery, even though processing is elsewhere)
    commands.insert(
        0,
        SlashCommand {
            name: "mode".to_string(),
            description: "Set agent mode (e.g. /mode debug)".to_string(),
            source: CommandSource::BuiltIn,
            args: vec![ArgDef {
                name: "mode".to_string(),
                description: Some("Agent mode name".to_string()),
                required: true,
            }],
        },
    );

    if let Some(manager) = mcp {
        for (server, prompts) in manager.list_all_prompts().await {
            for p in prompts {
                commands.push(SlashCommand {
                    name: p.name.clone(),
                    description: p.description.clone().unwrap_or_default(),
                    source: CommandSource::Mcp {
                        server: server.clone(),
                    },
                    args: p
                        .arguments
                        .iter()
                        .map(|a| ArgDef {
                            name: a.name.clone(),
                            description: a.description.clone(),
                            required: a.required,
                        })
                        .collect(),
                });
            }
        }
    }

    commands
}

/// Resolve a user message that starts with `/` into a [`ResolvedCommand`].
///
/// Returns `None` when:
/// - The message does not start with `/`.
/// - The command is `/mode` (handled by `openai_api.rs`).
/// - The command is not recognized (pass through to LLM as plain text).
pub async fn resolve(message: &str, mcp: Option<&Arc<McpManager>>) -> Option<ResolvedCommand> {
    if !message.starts_with('/') {
        return None;
    }

    let rest = &message[1..];
    let (cmd, args_str) = rest
        .split_once(' ')
        .map(|(c, a)| (c.trim(), a.trim()))
        .unwrap_or((rest.trim(), ""));

    let cmd_lower = cmd.to_lowercase();

    // /mode is handled by existing code — skip
    if cmd_lower == "mode" {
        return None;
    }

    // /help — generate list of available commands
    if cmd_lower == "help" {
        let all = list_commands(mcp).await;
        let text = format_help(&all);
        return Some(ResolvedCommand::McpPrompt(vec![ChatMessage {
            role: ChatRole::System,
            content: MessagePart::Text(text),
        }]));
    }

    // MCP prompts
    if let Some(manager) = mcp {
        let all_prompts = manager.list_all_prompts().await;
        for (server, prompts) in &all_prompts {
            if let Some(p) = prompts.iter().find(|p| p.name.to_lowercase() == cmd_lower) {
                let args_map = parse_args(args_str);
                let get_args = if args_map.is_empty() {
                    None
                } else {
                    Some(args_map)
                };

                match manager.get_prompt(server, &p.name, get_args).await {
                    Ok(lines) => {
                        let messages: Vec<ChatMessage> = lines
                            .iter()
                            .filter_map(|line| parse_prompt_line(line))
                            .collect();
                        return Some(ResolvedCommand::McpPrompt(messages));
                    }
                    Err(e) => {
                        warn!(cmd = %cmd, server = %server, err = %e, "slash_command: get_prompt failed");
                        return None;
                    }
                }
            }
        }
    }

    None // Unknown command — let the message pass through to the LLM
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Parse `key=value key2=value2 ...` into a JSON object.
/// Falls back to `{"input": "<whole string>"}` when no `=` is present.
fn parse_args(s: &str) -> serde_json::Map<String, serde_json::Value> {
    let mut map = serde_json::Map::new();
    if s.is_empty() {
        return map;
    }
    if !s.contains('=') {
        map.insert(
            "input".to_string(),
            serde_json::Value::String(s.to_string()),
        );
        return map;
    }
    for pair in s.split_whitespace() {
        if let Some((k, v)) = pair.split_once('=') {
            map.insert(k.to_string(), serde_json::Value::String(v.to_string()));
        }
    }
    map
}

/// Parse a `"[role] text"` line (from `get_prompt()`) into a [`ChatMessage`].
fn parse_prompt_line(line: &str) -> Option<ChatMessage> {
    let (role, text) = if let Some(t) = line.strip_prefix("[user] ") {
        (ChatRole::User, t)
    } else if let Some(t) = line.strip_prefix("[assistant] ") {
        (ChatRole::Assistant, t)
    } else if let Some(t) = line.strip_prefix("[system] ") {
        (ChatRole::System, t)
    } else {
        (ChatRole::User, line) // unrecognized format → user
    };
    Some(ChatMessage {
        role,
        content: MessagePart::Text(text.to_string()),
    })
}

/// Format the help message listing all available slash commands.
fn format_help(commands: &[SlashCommand]) -> String {
    let mut lines = vec!["**Available slash commands:**".to_string()];
    for cmd in commands {
        let args_hint = if cmd.args.is_empty() {
            String::new()
        } else {
            let hints: Vec<String> = cmd
                .args
                .iter()
                .map(|a| {
                    if a.required {
                        format!("<{}>", a.name)
                    } else {
                        format!("[{}]", a.name)
                    }
                })
                .collect();
            format!(" {}", hints.join(" "))
        };
        lines.push(format!(
            "- `/{}{}`  {}",
            cmd.name, args_hint, cmd.description
        ));
    }
    lines.join("\n")
}
