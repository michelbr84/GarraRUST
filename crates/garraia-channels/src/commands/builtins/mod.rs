//! Built-in slash commands migrated from the hardcoded `handle_command` match.

mod clear;
mod config;
mod health;
mod help;
mod model;
mod mode;
mod pair;
mod providers;
mod start;
mod stats;
mod users;
mod voice;
mod voz;

pub use clear::ClearCommand;
pub use config::ConfigCommand;
pub use health::HealthCommand;
pub use help::HelpCommand;
pub use model::ModelCommand;
pub use mode::{ModeCommand, ModesListCommand};
pub use pair::PairCommand;
pub use providers::ProvidersCommand;
pub use start::StartCommand;
pub use stats::StatsCommand;
pub use users::UsersCommand;
pub use voice::VoiceCommand;
pub use voz::VozCommand;

use super::CommandRegistry;

/// Register all built-in commands into the given registry.
pub fn register_builtins(registry: &mut CommandRegistry) {
    // Core commands (existing)
    registry.register(Box::new(StartCommand));
    registry.register(Box::new(HelpCommand));
    registry.register(Box::new(ClearCommand));
    registry.register(Box::new(ModelCommand));
    registry.register(Box::new(PairCommand));
    registry.register(Box::new(UsersCommand));

    // New commands (Phase 3-4)
    registry.register(Box::new(VozCommand));
    registry.register(Box::new(VoiceCommand));
    registry.register(Box::new(HealthCommand));
    registry.register(Box::new(ProvidersCommand));
    registry.register(Box::new(StatsCommand));
    registry.register(Box::new(ConfigCommand));

    // GAR-223: Mode commands
    registry.register(Box::new(ModeCommand));
    registry.register(Box::new(ModesListCommand));
}
