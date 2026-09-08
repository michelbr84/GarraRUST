use std::collections::HashMap;
use std::sync::Arc;

use tracing::info;

use super::{CommandContext, CommandError, CommandResult, Role, SlashCommand};

/// Central registry for slash commands.
///
/// Commands are registered once at startup and looked up by name
/// when a user sends a message starting with `/`.
pub struct ClosureCommand<F> {
    name: &'static str,
    desc: &'static str,
    usage: &'static str,
    role: Role,
    show_in_menu: bool,
    f: F,
}

impl<F> ClosureCommand<F>
where
    F: Fn(&CommandContext) -> CommandResult + Send + Sync + 'static,
{
    pub fn new(
        name: &'static str,
        desc: &'static str,
        usage: &'static str,
        role: Role,
        show_in_menu: bool,
        f: F,
    ) -> Self {
        Self {
            name,
            desc,
            usage,
            role,
            show_in_menu,
            f,
        }
    }
}

impl<F> SlashCommand for ClosureCommand<F>
where
    F: Fn(&CommandContext) -> CommandResult + Send + Sync + 'static,
{
    fn name(&self) -> &'static str {
        self.name
    }
    fn description(&self) -> &'static str {
        self.desc
    }
    fn usage(&self) -> &'static str {
        self.usage
    }
    fn required_role(&self) -> Role {
        self.role
    }
    fn show_in_menu(&self) -> bool {
        self.show_in_menu
    }
    fn execute(&self, ctx: &CommandContext) -> CommandResult {
        (self.f)(ctx)
    }
}
pub struct CommandRegistry {
    /// `Arc`, not `Box`, so [`Self::resolve`] can hand a caller a handle it
    /// executes **after** releasing the registry lock. See `resolve`.
    commands: HashMap<String, Arc<dyn SlashCommand>>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
        }
    }

    /// Register a command. Overwrites any existing command with the same name.
    pub fn register(&mut self, cmd: Box<dyn SlashCommand>) {
        let name = cmd.name().to_string();
        info!("registered slash command: /{name}");
        self.commands.insert(name, Arc::from(cmd));
    }

    /// The command name in `full_text` (`"/model x"` → `"model"`), empty when
    /// there is none.
    pub fn command_name(full_text: &str) -> &str {
        full_text
            .strip_prefix('/')
            .unwrap_or(full_text)
            .split_whitespace()
            .next()
            .unwrap_or("")
    }

    /// The reply for a name nobody registered — one string, shared by every
    /// dispatcher so the user reads the same thing on every channel.
    pub fn unknown_command_reply(name: &str) -> String {
        format!("❓ Unknown command: /{name}\nType /help to see available commands.")
    }

    /// A cloned handle to the command `full_text` names, or `None`.
    ///
    /// Exists so a caller can **drop the registry lock before executing**.
    /// [`Self::dispatch`] keeps the read guard alive for the whole run; a
    /// command that reads the registry again while running — `/help` lists
    /// it — then blocks the moment a writer is waiting (writer-preferring
    /// `RwLock`: the pending write blocks the inner read, the outer read
    /// blocks the write). The Telegram adapter carried that latent deadlock
    /// since GAR-184; exposing commands over HTTP is what made it worth
    /// closing. Pair with [`Self::run`].
    pub fn resolve(&self, full_text: &str) -> Option<Arc<dyn SlashCommand>> {
        self.commands.get(Self::command_name(full_text)).cloned()
    }

    /// Permission check + execute: the second half of [`Self::dispatch`],
    /// for a handle obtained through [`Self::resolve`] with the lock gone.
    pub fn run(cmd: &dyn SlashCommand, ctx: &CommandContext) -> CommandResult {
        if ctx.user_role < cmd.required_role() {
            return Err(CommandError::Unauthorized(format!(
                "Permission denied. Required role: {}",
                cmd.required_role()
            )));
        }
        cmd.execute(ctx)
    }

    /// Look up a command by name (without the `/` prefix).
    pub fn get(&self, name: &str) -> Option<&dyn SlashCommand> {
        self.commands.get(name).map(|c| c.as_ref())
    }

    /// List all registered commands as `(name, description)` pairs,
    /// sorted alphabetically by name.
    pub fn list(&self) -> Vec<(&str, &str)> {
        let mut cmds: Vec<_> = self
            .commands
            .values()
            .map(|c| (c.name(), c.description()))
            .collect();
        cmds.sort_by_key(|(name, _)| *name);
        cmds
    }

    /// List commands visible to a specific role, sorted alphabetically.
    pub fn list_for_role(&self, role: Role) -> Vec<(&str, &str)> {
        let mut cmds: Vec<_> = self
            .commands
            .values()
            .filter(|c| role >= c.required_role())
            .map(|c| (c.name(), c.description()))
            .collect();
        cmds.sort_by_key(|(name, _)| *name);
        cmds
    }

    /// Commands that should appear in the Telegram menu (via `setMyCommands`).
    pub fn telegram_commands(&self) -> Vec<(&str, &str)> {
        let mut cmds: Vec<_> = self
            .commands
            .values()
            .filter(|c| c.show_in_menu())
            .map(|c| (c.name(), c.description()))
            .collect();
        cmds.sort_by_key(|(name, _)| *name);
        cmds
    }

    /// Dispatch a command: look up, check permissions, execute.
    ///
    /// Returns a user-facing response string or a `CommandError`.
    pub fn dispatch(&self, ctx: &CommandContext) -> CommandResult {
        let cmd_name = Self::command_name(&ctx.full_text);
        let Some(cmd) = self.get(cmd_name) else {
            return Ok(Self::unknown_command_reply(cmd_name));
        };
        Self::run(cmd, ctx)
    }

    /// Number of registered commands.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockCmd;
    impl SlashCommand for MockCmd {
        fn name(&self) -> &'static str {
            "test"
        }
        fn description(&self) -> &'static str {
            "A test command"
        }
        fn usage(&self) -> &'static str {
            "/test"
        }
        fn execute(&self, _ctx: &CommandContext) -> CommandResult {
            Ok("ok".to_string())
        }
    }

    struct AdminCmd;
    impl SlashCommand for AdminCmd {
        fn name(&self) -> &'static str {
            "admin"
        }
        fn description(&self) -> &'static str {
            "Admin only"
        }
        fn usage(&self) -> &'static str {
            "/admin"
        }
        fn required_role(&self) -> Role {
            Role::Admin
        }
        fn execute(&self, _ctx: &CommandContext) -> CommandResult {
            Ok("admin ok".to_string())
        }
    }

    fn make_ctx(text: &str, role: Role) -> CommandContext {
        CommandContext {
            user_id: "123".to_string(),
            user_name: "Test".to_string(),
            chat_id: 456,
            full_text: text.to_string(),
            args: text
                .split_whitespace()
                .skip(1)
                .map(|s| s.to_string())
                .collect(),
            user_role: role,
            state: None,
            session_id: None,
        }
    }

    #[test]
    fn register_and_get() {
        let mut reg = CommandRegistry::new();
        reg.register(Box::new(MockCmd));
        assert!(reg.get("test").is_some());
        assert!(reg.get("unknown").is_none());
    }

    #[test]
    fn list_sorted() {
        let mut reg = CommandRegistry::new();
        reg.register(Box::new(AdminCmd));
        reg.register(Box::new(MockCmd));
        let list = reg.list();
        assert_eq!(list[0].0, "admin");
        assert_eq!(list[1].0, "test");
    }

    #[test]
    fn dispatch_unknown_command() {
        let reg = CommandRegistry::new();
        let ctx = make_ctx("/xyz", Role::User);
        let result = reg.dispatch(&ctx);
        assert!(result.unwrap().contains("Unknown command"));
    }

    #[test]
    fn dispatch_permission_denied() {
        let mut reg = CommandRegistry::new();
        reg.register(Box::new(AdminCmd));
        let ctx = make_ctx("/admin", Role::User);
        let result = reg.dispatch(&ctx);
        assert!(result.is_err());
    }

    #[test]
    fn dispatch_success() {
        let mut reg = CommandRegistry::new();
        reg.register(Box::new(MockCmd));
        let ctx = make_ctx("/test", Role::User);
        let result = reg.dispatch(&ctx);
        assert_eq!(result.unwrap(), "ok");
    }

    #[test]
    fn list_for_role_filters() {
        let mut reg = CommandRegistry::new();
        reg.register(Box::new(MockCmd));
        reg.register(Box::new(AdminCmd));
        let user_cmds = reg.list_for_role(Role::User);
        assert_eq!(user_cmds.len(), 1);
        assert_eq!(user_cmds[0].0, "test");
        let admin_cmds = reg.list_for_role(Role::Admin);
        assert_eq!(admin_cmds.len(), 2);
    }
}
