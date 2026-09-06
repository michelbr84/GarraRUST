//! Comandos de barra genericos por canal.
//!
//! # Este modulo nao esta ligado a nada (#987)
//!
//! `register_builtins` **nao e chamado em lugar nenhum** — o unico hit no
//! repositorio e o re-export em `lib.rs`. O registry que atende o usuario de
//! verdade nasce vazio em `garraia-gateway/src/state.rs` e e preenchido
//! exclusivamente por `garraia_gateway::commands::register_commands`
//! (`server.rs`).
//!
//! Isso ja enganou pelo menos duas vezes: a #987 descreveu o `/mode` daqui
//! como "um segundo handler com comportamento divergente" (nao ha divergencia:
//! ele nunca roda), e a #984 citou o placeholder do `/stats` daqui como algo a
//! remover do produto. **Nao ha divergencia possivel entre codigo que roda e
//! codigo que nao e alcancado** — o que ha e uma armadilha de leitura.
//!
//! O `/mode` e o `/modes` foram removidos daqui pelo #987, porque eram os que
//! prometiam persistir o modo e nao persistiam nada. Os outros doze continuam
//! neste estado; liga-los ou apaga-los e decisao de quem os escreveu.

mod clear;
mod config;
mod health;
mod help;
mod model;
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
}
