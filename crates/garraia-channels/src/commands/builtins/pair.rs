use crate::commands::{CommandContext, CommandResult, Role, SlashCommand};

pub struct PairCommand;

impl SlashCommand for PairCommand {
    fn name(&self) -> &'static str {
        "pair"
    }
    fn description(&self) -> &'static str {
        "Generate a 6-digit invite code"
    }
    fn usage(&self) -> &'static str {
        "/pair"
    }

    fn required_role(&self) -> Role {
        Role::Owner
    }

    fn execute(&self, _ctx: &CommandContext) -> CommandResult {
        // Actual pairing code generation requires PairingManager from gateway.
        // This is a placeholder — the gateway callback overrides this logic.
        Ok("Generating pairing code...".to_string())
    }
}
