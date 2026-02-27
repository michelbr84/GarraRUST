use crate::commands::{CommandContext, CommandResult, SlashCommand};

pub struct HealthCommand;

impl SlashCommand for HealthCommand {
    fn name(&self) -> &'static str {
        "health"
    }
    fn description(&self) -> &'static str {
        "Show system health status"
    }
    fn usage(&self) -> &'static str {
        "/health"
    }

    fn execute(&self, _ctx: &CommandContext) -> CommandResult {
        // Actual health info is injected by the gateway callback
        // which has access to the health cache and provider status.
        Ok("🏥 Checking system health...".to_string())
    }
}
