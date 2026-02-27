use crate::commands::{CommandContext, CommandResult, SlashCommand};

pub struct VozCommand;

impl SlashCommand for VozCommand {
    fn name(&self) -> &'static str {
        "voz"
    }
    fn description(&self) -> &'static str {
        "Toggle voice response mode on/off"
    }
    fn usage(&self) -> &'static str {
        "/voz [on|off]"
    }

    fn execute(&self, ctx: &CommandContext) -> CommandResult {
        if ctx.args.is_empty() {
            return Ok("🎙️ Voice Mode\n\n\
                 Usage:\n\
                 /voz on  — enable voice responses\n\
                 /voz off — disable voice responses\n\n\
                 When enabled, the bot will respond with audio messages \
                 using text-to-speech."
                .to_string());
        }

        match ctx.args[0].to_lowercase().as_str() {
            "on" | "1" | "true" | "sim" => {
                // Actual toggle is handled by the gateway callback
                Ok("🎙️ Voice mode enabled! I'll respond with audio from now on.".to_string())
            }
            "off" | "0" | "false" | "nao" | "não" => {
                Ok("🔇 Voice mode disabled. Back to text responses.".to_string())
            }
            _ => Ok("❌ Usage: /voz on or /voz off".to_string()),
        }
    }
}
