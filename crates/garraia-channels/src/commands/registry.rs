use std::collections::HashMap;

use tracing::info;

use super::{CommandContext, CommandError, CommandResult, Role, SlashCommand};

/// Central registry for slash commands.
///
/// Commands are registered once at startup and looked up by name
/// when a user sends a message starting with `/`.
pub struct CommandRegistry {
    commands: HashMap<String, Box<dyn SlashCommand>>,
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
        self.commands.insert(name, cmd);
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
        let cmd_name = ctx
            .full_text
            .strip_prefix('/')
            .unwrap_or(&ctx.full_text)
            .split_whitespace()
            .next()
            .unwrap_or("");

        let Some(cmd) = self.get(cmd_name) else {
            return Ok(format!(
                "❓ Unknown command: /{cmd_name}\nType /help to see available commands."
            ));
        };

        // Permission check
        if ctx.user_role < cmd.required_role() {
            return Err(CommandError::Unauthorized(format!(
                "Permission denied. Required role: {}",
                cmd.required_role()
            )));
        }

        cmd.execute(ctx)
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
