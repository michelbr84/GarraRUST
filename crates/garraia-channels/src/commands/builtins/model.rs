use crate::commands::{CommandContext, CommandResult, SlashCommand};

pub struct ModelCommand;

impl SlashCommand for ModelCommand {
    fn name(&self) -> &'static str {
        "model"
    }
    fn description(&self) -> &'static str {
        "Get or set the LLM model"
    }
    fn usage(&self) -> &'static str {
        "/model [name|clear|default]"
    }

    fn execute(&self, ctx: &CommandContext) -> CommandResult {
        // Model get/set requires access to SharedState (channel_models map).
        // This is a placeholder — actual logic lives in the gateway callback.
        if ctx.args.is_empty() {
            Ok("No model override set. Using default.".to_string())
        } else {
            let model = &ctx.args[0];
            if model.eq_ignore_ascii_case("clear") || model.eq_ignore_ascii_case("default") {
                Ok("Model override cleared. Using default.".to_string())
            } else {
                Ok(format!("Model set to: {model}"))
            }
        }
    }
}
