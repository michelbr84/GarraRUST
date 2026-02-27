use crate::commands::{CommandContext, CommandResult, SlashCommand};

/// Alias for /voz — English-speaking users.
pub struct VoiceCommand;

impl SlashCommand for VoiceCommand {
    fn name(&self) -> &'static str {
        "voice"
    }
    fn description(&self) -> &'static str {
        "Toggle voice response mode (alias for /voz)"
    }
    fn usage(&self) -> &'static str {
        "/voice [on|off]"
    }

    fn show_in_menu(&self) -> bool {
        false
    }

    fn execute(&self, ctx: &CommandContext) -> CommandResult {
        // Delegate to the same logic as /voz
        if ctx.args.is_empty() {
            return Ok("🎙️ Voice Mode\n\n\
                 Usage: /voice on | /voice off\n\
                 (alias for /voz)"
                .to_string());
        }

        match ctx.args[0].to_lowercase().as_str() {
            "on" | "1" | "true" => {
                Ok("🎙️ Voice mode enabled! I'll respond with audio.".to_string())
            }
            "off" | "0" | "false" => Ok("🔇 Voice mode disabled. Text responses only.".to_string()),
            _ => Ok("❌ Usage: /voice on or /voice off".to_string()),
        }
    }
}
