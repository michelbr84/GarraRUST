use crate::commands::{CommandContext, CommandResult, SlashCommand};

pub struct StartCommand;

impl SlashCommand for StartCommand {
    fn name(&self) -> &'static str {
        "start"
    }
    fn description(&self) -> &'static str {
        "Start the bot and show welcome message"
    }
    fn usage(&self) -> &'static str {
        "/start"
    }

    fn show_in_menu(&self) -> bool {
        false
    }

    fn execute(&self, _ctx: &CommandContext) -> CommandResult {
        Ok(
            "Welcome to GarraIA! Send me a message and I will respond.\n\n\
             Commands:\n\
             /help - show this help\n\
             /clear - reset conversation history\n\
             /model [name] - get or set the LLM model\n\
             /pair - generate invite code (owner only)"
                .to_string(),
        )
    }
}
