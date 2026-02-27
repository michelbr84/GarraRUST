//! Slash command system for GarraIA.
//!
//! Provides a trait-based, registry-driven command framework that replaces
//! hard-coded `match` dispatch. Commands are auto-discoverable, support
//! role-based access control, and can be synced to Telegram via `setMyCommands`.

mod registry;

pub mod builtins;

pub use registry::CommandRegistry;

use std::fmt;
use std::sync::Arc;

// ─── Roles ───────────────────────────────────────────────────────────

/// Access level required to execute a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    /// Any authenticated/allowed user.
    User = 0,
    /// Administrative access.
    Admin = 1,
    /// Bot owner — full control.
    Owner = 2,
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Role::User => write!(f, "User"),
            Role::Admin => write!(f, "Admin"),
            Role::Owner => write!(f, "Owner"),
        }
    }
}

// ─── Command context ────────────────────────────────────────────────

/// Runtime context passed to every command handler.
pub struct CommandContext {
    /// Telegram/Discord user id.
    pub user_id: String,
    /// User display name.
    pub user_name: String,
    /// Chat/channel id (numeric for Telegram, string-like for Discord).
    pub chat_id: i64,
    /// The full original message text (e.g. "/model deepseek-r1").
    pub full_text: String,
    /// Arguments after the command name (split by whitespace).
    pub args: Vec<String>,
    /// Role of the invoking user.
    pub user_role: Role,
    /// Shared application state (type-erased to avoid coupling to gateway).
    pub state: Option<Arc<dyn std::any::Any + Send + Sync>>,
}

// ─── Command result ─────────────────────────────────────────────────

/// Result of executing a slash command.
pub type CommandResult = std::result::Result<String, CommandError>;

/// Errors that can occur during command execution.
#[derive(Debug)]
pub enum CommandError {
    /// User does not have the required role.
    Unauthorized(String),
    /// Invalid arguments or usage error.
    InvalidArgs(String),
    /// Internal error during execution.
    Internal(String),
    /// Silently drop the response (e.g. blocked user).
    Blocked,
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandError::Unauthorized(msg) => write!(f, "⛔ {msg}"),
            CommandError::InvalidArgs(msg) => write!(f, "❌ {msg}"),
            CommandError::Internal(msg) => write!(f, "💥 Internal error: {msg}"),
            CommandError::Blocked => write!(f, "__blocked__"),
        }
    }
}

// ─── SlashCommand trait ─────────────────────────────────────────────

/// Trait implemented by every slash command.
///
/// Commands are registered in the [`CommandRegistry`] and dispatched
/// automatically when a user sends a message starting with `/`.
pub trait SlashCommand: Send + Sync {
    /// Command name without the leading `/` (e.g. `"help"`).
    fn name(&self) -> &'static str;

    /// Short one-line description shown in Telegram autocomplete.
    fn description(&self) -> &'static str;

    /// Usage string shown when the command is used incorrectly.
    fn usage(&self) -> &'static str;

    /// Minimum role required to execute this command.
    /// Defaults to `Role::User`.
    fn required_role(&self) -> Role {
        Role::User
    }

    /// Whether this command should appear in the Telegram menu
    /// (registered via `setMyCommands`).
    fn show_in_menu(&self) -> bool {
        true
    }

    /// Execute the command and return a response string.
    fn execute(&self, ctx: &CommandContext) -> CommandResult;
}
