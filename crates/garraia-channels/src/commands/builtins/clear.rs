use crate::commands::{CommandContext, CommandResult, SlashCommand};

pub struct ClearCommand;

impl SlashCommand for ClearCommand {
    fn name(&self) -> &'static str {
        "clear"
    }
    fn description(&self) -> &'static str {
        "Reset conversation history"
    }
    fn usage(&self) -> &'static str {
        "/clear"
    }

    fn execute(&self, _ctx: &CommandContext) -> CommandResult {
        // Actual history clearing is done in the gateway callback
        // which has access to the session store via SharedState.
        // This command just returns the confirmation message.
        Ok("Conversation history cleared.".to_string())
    }
}
