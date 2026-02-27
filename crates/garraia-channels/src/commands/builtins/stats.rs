use crate::commands::{CommandContext, CommandResult, SlashCommand};

pub struct StatsCommand;

impl SlashCommand for StatsCommand {
    fn name(&self) -> &'static str {
        "stats"
    }
    fn description(&self) -> &'static str {
        "Show bot usage statistics"
    }
    fn usage(&self) -> &'static str {
        "/stats"
    }

    fn execute(&self, _ctx: &CommandContext) -> CommandResult {
        // Actual stats are populated by the gateway callback
        // which has access to session counters, message counts, etc.
        Ok("📊 Fetching statistics...".to_string())
    }
}
