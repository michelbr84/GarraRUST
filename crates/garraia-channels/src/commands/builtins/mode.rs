//! Mode command - GAR-223
//! Handles /mode and /modes commands for agent mode selection

use crate::commands::{CommandContext, CommandResult, SlashCommand};

pub struct ModeCommand;

impl SlashCommand for ModeCommand {
    fn name(&self) -> &'static str {
        "mode"
    }

    fn description(&self) -> &'static str {
        "Get or set the agent execution mode"
    }

    fn usage(&self) -> &'static str {
        "/mode [auto|search|architect|code|ask|debug|orchestrator|review|edit]"
    }

    fn execute(&self, ctx: &CommandContext) -> CommandResult {
        if ctx.args.is_empty() {
            // Show current mode
            Ok("Modo atual: ask (padrão para Telegram)".to_string())
        } else {
            let mode = &ctx.args[0];
            match mode.to_lowercase().as_str() {
                "auto" => Ok("Modo definido: auto".to_string()),
                "search" => Ok("Modo definido: search".to_string()),
                "architect" => Ok("Modo definido: architect".to_string()),
                "code" => Ok("Modo definido: code".to_string()),
                "ask" => Ok("Modo definido: ask".to_string()),
                "debug" => Ok("modo definido: debug".to_string()),
                "orchestrator" => Ok("Modo definido: orchestrator".to_string()),
                "review" => Ok("Modo definido: review".to_string()),
                "edit" => Ok("Modo definido: edit".to_string()),
                _ => Ok(format!(
                    "Modo inválido: {}. Use: auto, search, architect, code, ask, debug, orchestrator, review, edit",
                    mode
                )),
            }
        }
    }
}

pub struct ModesListCommand;

impl SlashCommand for ModesListCommand {
    fn name(&self) -> &'static str {
        "modes"
    }

    fn description(&self) -> &'static str {
        "List all available agent modes"
    }

    fn usage(&self) -> &'static str {
        "/modes"
    }

    fn execute(&self, _ctx: &CommandContext) -> CommandResult {
        let modes = r#"📋 Modos disponíveis:

• auto - Decide automaticamente baseado no contexto
• search - Busca e inspeção sem modificar
• architect - Análise de arquitetura e design  
• code - Desenvolvimento e implementação
• ask - Consulta e explicação (padrão Telegram)
• debug - Debugging e análise de erros
• orchestrator - Execução multi-etapas com planos
• review - Revisão de código e diffs
• edit - Edição focada

Use /mode <nome> para trocar de modo."#;
        Ok(modes.to_string())
    }
}
