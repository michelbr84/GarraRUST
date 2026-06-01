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

    fn execute(&self, ctx: &CommandContext) -> CommandResult {
        // Plan 0250 (GAR-771): warm, PT-BR-first welcome that introduces Garra
        // in the first person and nudges the user toward a first interaction.
        let name = ctx.user_name.trim();
        let greeting = if name.is_empty() {
            "Oi! 👋 Eu sou o Garra".to_string()
        } else {
            format!("Oi, {name}! 👋 Eu sou o Garra")
        };
        Ok(format!(
            "{greeting}, seu assistente pessoal.\n\n\
             Pode falar comigo como você falaria com um amigo — é só me mandar \
             uma mensagem. Por exemplo:\n\
             • \"Me ajuda a organizar minha semana\"\n\
             • \"Resume esse texto pra mim\"\n\
             • \"O que você consegue fazer?\"\n\n\
             Se quiser ver os atalhos, é só mandar /help. Vamos lá? 🐾"
        ))
    }
}
