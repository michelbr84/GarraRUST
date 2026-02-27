use crate::commands::{CommandContext, CommandResult, SlashCommand};

pub struct ProvidersCommand;

impl SlashCommand for ProvidersCommand {
    fn name(&self) -> &'static str {
        "providers"
    }
    fn description(&self) -> &'static str {
        "List configured AI providers"
    }
    fn usage(&self) -> &'static str {
        "/providers"
    }

    fn execute(&self, _ctx: &CommandContext) -> CommandResult {
        // Actual provider listing is done by the gateway callback
        // which has access to the AgentRuntime and provider registry.
        Ok("🤖 Listing providers...".to_string())
    }
}
