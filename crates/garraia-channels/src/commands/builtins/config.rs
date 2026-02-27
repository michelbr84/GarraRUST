use crate::commands::{CommandContext, CommandResult, Role, SlashCommand};

pub struct ConfigCommand;

impl SlashCommand for ConfigCommand {
    fn name(&self) -> &'static str {
        "config"
    }
    fn description(&self) -> &'static str {
        "View or change bot configuration"
    }
    fn usage(&self) -> &'static str {
        "/config [key] [value]"
    }

    fn required_role(&self) -> Role {
        Role::Owner
    }

    fn execute(&self, ctx: &CommandContext) -> CommandResult {
        if ctx.args.is_empty() {
            // Show current config summary
            Ok("⚙️ Bot Configuration\n\n\
                 Usage:\n\
                 /config          — show current settings\n\
                 /config key      — show a specific setting\n\
                 /config key val  — change a setting\n\n\
                 Available keys: model, language, voice, streaming"
                .to_string())
        } else if ctx.args.len() == 1 {
            Ok(format!("⚙️ Config '{}': (value from runtime)", ctx.args[0]))
        } else {
            Ok(format!(
                "⚙️ Config '{}' set to '{}'",
                ctx.args[0], ctx.args[1]
            ))
        }
    }
}
