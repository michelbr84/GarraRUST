use crate::commands::{CommandContext, CommandResult, Role, SlashCommand};

pub struct UsersCommand;

impl SlashCommand for UsersCommand {
    fn name(&self) -> &'static str {
        "users"
    }
    fn description(&self) -> &'static str {
        "List allowed users"
    }
    fn usage(&self) -> &'static str {
        "/users"
    }

    fn required_role(&self) -> Role {
        Role::Owner
    }

    fn execute(&self, _ctx: &CommandContext) -> CommandResult {
        // Actual user listing requires Allowlist from the gateway.
        // This is a placeholder — the gateway callback overrides this logic.
        Ok("Listing users...".to_string())
    }
}
