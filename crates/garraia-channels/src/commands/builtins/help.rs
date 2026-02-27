use crate::commands::{CommandContext, CommandResult, Role, SlashCommand};

pub struct HelpCommand;

impl SlashCommand for HelpCommand {
    fn name(&self) -> &'static str {
        "help"
    }
    fn description(&self) -> &'static str {
        "Show available commands"
    }
    fn usage(&self) -> &'static str {
        "/help"
    }

    fn execute(&self, ctx: &CommandContext) -> CommandResult {
        // The help text is dynamically built by the gateway using
        // `CommandRegistry::list_for_role`. This is a fallback.
        let mut help = "GarraIA Commands:\n\
            /help - show this help\n\
            /clear - reset conversation history\n\
            /model [name] - get or set the LLM model"
            .to_string();

        if ctx.user_role >= Role::Owner {
            help.push_str(
                "\n/pair - generate a 6-digit invite code\n\
                 /users - list allowed users",
            );
        }
        Ok(help)
    }
}
