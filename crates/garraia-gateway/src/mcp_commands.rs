//! Auto-registration of MCP tools as slash commands.
//!
//! This module provides automatic registration of MCP server tools as
//! Telegram/Discord slash commands when MCP servers connect.
//!
//! This is called at startup after the basic commands are registered.

use garraia_agents::McpManager;
use garraia_channels::CommandContext;
use garraia_channels::commands::{
    ClosureCommand, CommandRegistry, CommandResult, Role, SlashCommand,
};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// A pre-built command ready to be registered
pub struct PrebuiltCommand {
    pub tool_name: String,
    pub command: Box<dyn SlashCommand>,
}

/// Collect all MCP tool commands asynchronously (no lock held).
/// This performs all async operations to gather tool information.
pub async fn collect_mcp_commands(mcp_manager: Arc<McpManager>) -> Vec<PrebuiltCommand> {
    let manager = mcp_manager;

    // Get list of servers - async operation
    let servers = manager.list_servers().await;
    info!(
        "Scanning {} MCP server(s) for tool registration",
        servers.len()
    );

    let mut commands = Vec::new();

    for (server_name, tool_count, is_connected) in servers {
        if !is_connected {
            warn!("MCP server '{}' is not connected, skipping", server_name);
            continue;
        }

        info!(
            "Registering {} tool(s) from MCP server '{}'",
            tool_count, server_name
        );

        // Get tools for this server - async operation
        let tools = manager.tool_info(&server_name).await;

        if tools.is_empty() {
            info!("No tools found for MCP server '{}'", server_name);
            continue;
        }

        for tool in tools {
            let tool_name = tool.name.clone();

            // Create static strings using Box::leak
            let command_name = format!("mcp_{}", sanitize_command_name(&tool_name));
            let command_name_static: &'static str = Box::leak(Box::new(command_name));

            let description = tool
                .description
                .unwrap_or_else(|| format!("MCP tool: {}", tool_name));
            let desc_static: &'static str = Box::leak(Box::new(description));

            let usage_example = format!("/{} [args]", command_name_static);
            let usage_static: &'static str = Box::leak(Box::new(usage_example));

            // Clone the Arc for the closure
            let mcp_manager = Arc::clone(&manager);
            let tool_name_clone = tool_name.clone();
            let server_name_clone = server_name.clone();

            let cmd = ClosureCommand::new(
                command_name_static,
                desc_static,
                usage_static,
                Role::User,
                true,
                move |_ctx: &CommandContext| -> CommandResult {
                    // Build arguments for the tool
                    let args: std::collections::HashMap<String, serde_json::Value> = _ctx
                        .args
                        .iter()
                        .enumerate()
                        .map(|(i, v)| (format!("arg{}", i), serde_json::json!(v)))
                        .collect();

                    // Execute the tool synchronously using current runtime
                    let rt = tokio::runtime::Handle::current();
                    let result = rt.block_on(async {
                        mcp_manager
                            .call_tool(&server_name_clone, &tool_name_clone, args)
                            .await
                    });

                    match result {
                        Ok(output) => Ok(output),
                        Err(e) => Ok(format!("Error: {}", e)),
                    }
                },
            );

            commands.push(PrebuiltCommand {
                tool_name,
                command: Box::new(cmd),
            });
        }
    }

    commands
}

/// Register pre-built commands into the registry (synchronous, no await points).
pub fn register_collected_commands(registry: &mut CommandRegistry, commands: Vec<PrebuiltCommand>) {
    for cmd in commands {
        registry.register(cmd.command);
        info!("Registered MCP tool '{}'", cmd.tool_name);
    }
}

/// Register MCP tools from a manager into the command registry.
/// This is called at startup after basic commands are registered.
///
/// Note: This is kept for backward compatibility but prefer collect_mcp_commands + register_collected_commands.
pub async fn register_mcp_tools(
    registry: &mut CommandRegistry,
    mcp_manager: Option<Arc<McpManager>>,
) {
    let Some(manager) = mcp_manager else {
        debug!("No MCP manager available, skipping MCP tool registration");
        return;
    };

    let commands = collect_mcp_commands(manager).await;
    register_collected_commands(registry, commands);
}

/// Sanitize a tool name to be a valid command name.
fn sanitize_command_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .trim_matches('_')
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_command_name() {
        assert_eq!(sanitize_command_name("my_tool"), "my_tool");
        assert_eq!(sanitize_command_name("my-tool"), "my_tool");
        assert_eq!(sanitize_command_name("my.tool"), "my_tool");
        assert_eq!(sanitize_command_name("my tool"), "my_tool");
        assert_eq!(sanitize_command_name("MY_TOOL"), "my_tool");
    }
}
