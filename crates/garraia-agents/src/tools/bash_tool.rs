use async_trait::async_trait;
use garraia_common::{Error, Result};
use std::time::Duration;
use tokio::process::Command;

use super::{Tool, ToolContext, ToolOutput};

const TIMEOUT_PADRAO_SEGS: u64 = 30;
const MAX_BYTES_SAIDA: usize = 32 * 1024;

/// Deny list of dangerous commands - GAR-236
///
/// Rules:
/// - Patterns use `.contains()` on the lowercased command string.
/// - Keep patterns specific enough to avoid false positives on common flags.
///   Example: use "format c:" not "format" (would block PowerShell -Format flag).
const DENY_LIST: &[&str] = &[
    "rm -rf",
    "rm -r /",
    "rm -f /",
    // Windows disk formatting — use explicit drive letters, not bare "format"
    // which would false-positive on PowerShell's -Format parameter.
    "format c:",
    "format d:",
    "format e:",
    "format f:",
    "diskpart",      // Windows disk management tool — genuinely dangerous
    "fdisk",
    "mkfs",
    "dd if=",
    "> /dev/sd",
    "chmod 777 /",
    "chown -R",
    ":wq!",
    "exit!",
    "curl | sh",
    "wget | sh",
    "sh -c",
    "bash -c",
    "python -m http",
    "nc -",
    "netcat",
    "nmap",
    "ssh root@",
    "sudo su",
    "kill -9 -1",
    "pkill -9",
    "reboot",
    "shutdown",
    "init 0",
    "init 6",
    "halt",
    "poweroff",
];

/// GAR-187: Confirmation list — risky but not catastrophic commands that require
/// explicit user approval before execution. Unlike DENY_LIST (hard block), these
/// are paused and a confirmation prompt is returned to the user.
///
/// Patterns use `.contains()` on the lowercased command string.
const CONFIRM_LIST: &[&str] = &[
    "rm -r",          // recursive delete (not caught by DENY_LIST which only blocks rm -rf / rm -r /)
    "del /s",         // Windows recursive delete
    "del /f",         // Windows force delete
    "rd /s",          // Windows remove directory recursively
    "git reset --hard",
    "git push --force",
    "git push -f",
    "git clean -f",
    "drop table",     // SQL destructive operations
    "drop database",
    "drop schema",
    "truncate table",
    "truncate ",
    "delete from",    // unqualified DELETE (no WHERE)
    "kill ",          // process kill
    "taskkill",
    "stop-process",
    "remove-item -recurse",
    "remove-item -r",
];

/// Allow list of safe commands for read-only mode - GAR-236
const ALLOW_LIST_READONLY: &[&str] = &[
    "ls",
    "dir",
    "pwd",
    "cd",
    "cat",
    "type",
    "head",
    "tail",
    "grep",
    "find",
    "date",       // Unix: get current date/time
    "get-date",   // PowerShell: get current date/time
    "git status",
    "git log",
    "git diff",
    "git branch",
    "cargo",
    "rustc",
];

/// Executa comandos de shell com timeout configurável e limite de saída.
/// No Windows utiliza PowerShell. Em sistemas Unix-like utiliza Bash.
pub struct BashTool {
    timeout: Duration,
    allow_readonly: bool,
    /// GAR-187: When true, commands matching CONFIRM_LIST require user approval
    /// before execution. Approval is signalled via ToolContext.is_confirmation_approved.
    confirmation_enabled: bool,
}

impl BashTool {
    pub fn new(timeout_secs: Option<u64>) -> Self {
        Self {
            timeout: Duration::from_secs(timeout_secs.unwrap_or(TIMEOUT_PADRAO_SEGS)),
            allow_readonly: false,
            confirmation_enabled: false,
        }
    }

    /// GAR-187: Create a BashTool with human-in-the-loop confirmation for risky commands.
    pub fn new_with_confirmation(timeout_secs: Option<u64>) -> Self {
        Self {
            timeout: Duration::from_secs(timeout_secs.unwrap_or(TIMEOUT_PADRAO_SEGS)),
            allow_readonly: false,
            confirmation_enabled: true,
        }
    }

    /// Create a read-only BashTool that only allows safe commands
    pub fn new_readonly(timeout_secs: Option<u64>) -> Self {
        Self {
            timeout: Duration::from_secs(timeout_secs.unwrap_or(TIMEOUT_PADRAO_SEGS)),
            allow_readonly: true,
            confirmation_enabled: false,
        }
    }

    /// Check if command is in deny list
    fn is_dangerous(&self, command: &str) -> bool {
        let cmd_lower = command.to_lowercase();
        for pattern in DENY_LIST {
            if cmd_lower.contains(&pattern.to_lowercase()) {
                tracing::warn!("Blocked dangerous command pattern: {}", pattern);
                return true;
            }
        }
        false
    }

    /// GAR-187: Check if command matches the risky confirmation tier.
    fn is_risky(&self, command: &str) -> bool {
        let cmd_lower = command.to_lowercase();
        CONFIRM_LIST.iter().any(|p| cmd_lower.contains(&p.to_lowercase()))
    }

    /// Check if command is in allow list (for read-only mode)
    fn is_allowed(&self, command: &str) -> bool {
        if !self.allow_readonly {
            return true; // Not in readonly mode, allow all (except deny list)
        }

        let cmd_lower = command.to_lowercase().trim().to_string();
        for pattern in ALLOW_LIST_READONLY {
            if cmd_lower.starts_with(&pattern.to_lowercase())
                || cmd_lower.contains(&pattern.to_lowercase()) {
                return true;
            }
        }
        false
    }
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        if cfg!(target_os = "windows") {
            "Executa um comando de shell usando PowerShell. Retorna a saída."
        } else {
            "Executa um comando de shell usando Bash. Retorna a saída."
        }
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Comando de shell a ser executado"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(
        &self,
        context: &ToolContext,
        input: serde_json::Value,
    ) -> Result<ToolOutput> {
        let comando = input
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Agent("parâmetro 'command' ausente".into()))?;

        // GAR-236: Security check - deny list (hard block, never executes)
        if self.is_dangerous(comando) {
            tracing::error!("Blocked dangerous command: {}", comando);
            return Ok(ToolOutput::error(
                "Comando bloqueado por segurança: padrão perigoso detectado".to_string(),
            ));
        }

        // GAR-187: Risky tier — requires user confirmation before execution.
        // Skipped if the user has already approved via ToolContext.is_confirmation_approved.
        if self.confirmation_enabled && self.is_risky(comando) && !context.is_confirmation_approved {
            tracing::warn!(
                command = %comando,
                session = %context.session_id,
                "bash: risky command requires user confirmation"
            );
            return Ok(ToolOutput::confirmation_request(format!(
                "[CONFIRM_REQUIRED] O comando a seguir requer confirmação antes de ser executado:\n\
                 ```\n{comando}\n```\n\
                 Responda **sim** para executar ou **não** para cancelar."
            )));
        }

        // GAR-236: Security check - read-only allow list
        if !self.is_allowed(comando) {
            tracing::warn!("Command not in allow list for read-only mode: {}", comando);
            return Ok(ToolOutput::error(
                "Comando não permitido no modo read-only. Use: ls, dir, cat, git, cargo, etc.".to_string(),
            ));
        }

        let (shell, arg) = if cfg!(target_os = "windows") {
            ("powershell", "-Command")
        } else {
            ("bash", "-c")
        };

        let resultado = tokio::time::timeout(
            self.timeout,
            Command::new(shell).arg(arg).arg(comando).output(),
        )
        .await;

        match resultado {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                let mut combinado = String::new();

                if !stdout.is_empty() {
                    combinado.push_str(&stdout);
                }

                if !stderr.is_empty() {
                    if !combinado.is_empty() {
                        combinado.push('\n');
                    }
                    combinado.push_str("STDERR:\n");
                    combinado.push_str(&stderr);
                }

                // Truncar se exceder limite
                if combinado.len() > MAX_BYTES_SAIDA {
                    let mut end = MAX_BYTES_SAIDA;
                    while end > 0 && !combinado.is_char_boundary(end) {
                        end -= 1;
                    }
                    combinado.truncate(end);
                    combinado.push_str("\n... (saída truncada)");
                }

                if combinado.is_empty() {
                    combinado =
                        format!("(código de saída: {})", output.status.code().unwrap_or(-1));
                }

                if output.status.success() {
                    Ok(ToolOutput::success(combinado))
                } else {
                    Ok(ToolOutput::error(format!(
                        "código de saída {}: {}",
                        output.status.code().unwrap_or(-1),
                        combinado
                    )))
                }
            }
            Ok(Err(e)) => Ok(ToolOutput::error(format!("falha ao executar comando: {e}"))),
            Err(_) => Ok(ToolOutput::error(format!(
                "comando excedeu o tempo limite após {}s",
                self.timeout.as_secs()
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn executa_comando_simples() {
        let tool = BashTool::new(None);

        let ctx = ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
            is_confirmation_approved: false,
            working_dir: None,
            project_id: None,
        };

        let output = tool
            .execute(&ctx, serde_json::json!({"command": "echo hello"}))
            .await
            .unwrap();

        assert!(!output.is_error);
        assert!(output.content.contains("hello"));
    }

    #[tokio::test]
    async fn reporta_erro_em_comando_falho() {
        let tool = BashTool::new(None);

        let cmd = if cfg!(target_os = "windows") {
            "exit 1"
        } else {
            "false"
        };

        let ctx = ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
            is_confirmation_approved: false,
            working_dir: None,
            project_id: None,
        };

        let output = tool
            .execute(&ctx, serde_json::json!({"command": cmd}))
            .await
            .unwrap();

        assert!(output.is_error);
    }

    #[tokio::test]
    async fn retorna_erro_se_faltar_comando() {
        let tool = BashTool::new(None);

        let ctx = ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
            is_confirmation_approved: false,
            working_dir: None,
            project_id: None,
        };

        let result = tool.execute(&ctx, serde_json::json!({})).await;

        assert!(result.is_err());
    }

    #[test]
    fn powershell_format_flag_nao_e_bloqueado() {
        let tool = BashTool::new(None);
        // PowerShell -Format parameter must NOT be blocked
        assert!(!tool.is_dangerous("Get-Date -Format \"HH:mm:ss\""));
        assert!(!tool.is_dangerous("get-date -format 'yyyy-MM-dd'"));
        assert!(!tool.is_dangerous("Select-Object -Property Name, Format"));
    }

    #[test]
    fn format_disco_e_bloqueado() {
        let tool = BashTool::new(None);
        assert!(tool.is_dangerous("format c:"));
        assert!(tool.is_dangerous("FORMAT C: /Q"));
        assert!(tool.is_dangerous("format d: /fs:ntfs"));
        assert!(tool.is_dangerous("diskpart"));
    }

    #[test]
    fn comandos_perigosos_sao_bloqueados() {
        let tool = BashTool::new(None);
        assert!(tool.is_dangerous("rm -rf /"));
        assert!(tool.is_dangerous("mkfs.ext4 /dev/sda"));
        assert!(tool.is_dangerous("dd if=/dev/zero of=/dev/sda"));
        assert!(tool.is_dangerous("shutdown -h now"));
    }

    #[test]
    fn comandos_seguros_nao_sao_bloqueados() {
        let tool = BashTool::new(None);
        assert!(!tool.is_dangerous("date"));
        assert!(!tool.is_dangerous("echo hello world"));
        assert!(!tool.is_dangerous("ls -la"));
        assert!(!tool.is_dangerous("cargo build"));
        assert!(!tool.is_dangerous("git log --oneline"));
    }

    #[tokio::test]
    async fn captura_stderr() {
        let tool = BashTool::new(None);

        let cmd = if cfg!(target_os = "windows") {
            "Write-Error 'err'"
        } else {
            "echo err >&2"
        };

        let ctx = ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
            is_confirmation_approved: false,
            working_dir: None,
            project_id: None,
        };

        let output = tool
            .execute(&ctx, serde_json::json!({"command": cmd}))
            .await
            .unwrap();

        assert!(output.content.contains("err"));
    }
}
