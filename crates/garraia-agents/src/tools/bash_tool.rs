use async_trait::async_trait;
use garraia_common::{Error, Result, safety_gate};
use std::time::Duration;
use tokio::process::Command;

use super::{Tool, ToolContext, ToolOutput};

const TIMEOUT_PADRAO_SEGS: u64 = 30;
const MAX_BYTES_SAIDA: usize = 32 * 1024;

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
    "date",     // Unix: get current date/time
    "get-date", // PowerShell: get current date/time
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

    /// Check if command matches the hard-block denylist (GAR-236, GAR-497).
    fn is_dangerous(&self, command: &str) -> bool {
        matches!(
            safety_gate::safety_gate(command),
            Err(garraia_common::SafetyDenied::DangerousCommand { .. })
        )
    }

    /// GAR-187: Check if command matches the risky confirmation tier (GAR-497).
    fn is_risky(&self, command: &str) -> bool {
        safety_gate::is_risky(command).is_err()
    }

    /// Check if command is in allow list (for read-only mode)
    fn is_allowed(&self, command: &str) -> bool {
        if !self.allow_readonly {
            return true; // Not in readonly mode, allow all (except deny list)
        }

        let cmd_lower = command.to_lowercase().trim().to_string();
        for pattern in ALLOW_LIST_READONLY {
            if cmd_lower.starts_with(&pattern.to_lowercase())
                || cmd_lower.contains(&pattern.to_lowercase())
            {
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

    async fn execute(&self, context: &ToolContext, input: serde_json::Value) -> Result<ToolOutput> {
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

        // GAR-187 + #1075 R1: risky tier. With a confirmation channel the
        // command waits for user approval; WITHOUT one (confirmation
        // disabled — e.g. the stateless MCP full-auto path) it is
        // fail-closed BLOCKED: a risky command must never auto-run just
        // because nobody can be asked.
        //
        // #1075 (auditoria do hardening): em modo fail-closed o flag
        // `is_confirmation_approved` é IGNORADO. Ele é derivado do histórico
        // da conversa (marcador `[CONFIRM_REQUIRED]` nas últimas 6 mensagens
        // + palavra de aprovação), e um modelo com saída não sanitizada pode
        // plantar esse marcador — sem canal de confirmação real, não existe
        // aprovação legítima para honrar.
        let aprovado = self.confirmation_enabled && context.is_confirmation_approved;
        if self.is_risky(comando) && !aprovado {
            if self.confirmation_enabled {
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
            tracing::warn!(
                command = %comando,
                session = %context.session_id,
                "bash: risky command BLOCKED (fail-closed: confirmation disabled)"
            );
            return Ok(ToolOutput::error(
                "Comando bloqueado por segurança: comando sensível exige confirmação e este \
                 runtime não possui canal de confirmação (fail-closed)."
                    .to_string(),
            ));
        }

        // GAR-236: Security check - read-only allow list
        if !self.is_allowed(comando) {
            tracing::warn!("Command not in allow list for read-only mode: {}", comando);
            return Ok(ToolOutput::error(
                "Comando não permitido no modo read-only. Use: ls, dir, cat, git, cargo, etc."
                    .to_string(),
            ));
        }

        let (shell, arg) = if cfg!(target_os = "windows") {
            ("powershell", "-Command")
        } else {
            ("bash", "-c")
        };

        let mut cmd = Command::new(shell);
        cmd.arg(arg).arg(comando);
        // #1075 R3: the child runs in the session working_dir when set, and
        // (unix) inherits ONLY the allowlisted variables — the parent
        // process (MCP server / gateway) carries secrets in its env that
        // must never reach an LLM-driven shell. Windows PowerShell needs
        // more of its environment to boot, so the scrub is unix-only.
        if let Some(dir) = context.working_dir.as_deref() {
            cmd.current_dir(dir);
        }
        #[cfg(unix)]
        {
            cmd.env_clear();
            for (key, value) in safety_gate::allowed_child_env() {
                cmd.env(key, value);
            }
        }

        let resultado = tokio::time::timeout(self.timeout, cmd.output()).await;

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

    fn ctx(approved: bool) -> ToolContext {
        ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
            is_confirmation_approved: approved,
            working_dir: None,
            project_id: None,
        }
    }

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

    // ── R1 (#1075): risky tier fail-closed when confirmation is unavailable ──

    #[tokio::test]
    async fn r1_risky_bloqueado_sem_canal_de_confirmacao() {
        // Fail-closed: with confirmation DISABLED there is no approval
        // channel (e.g. the stateless MCP full-auto path) — a risky
        // command is BLOCKED, never auto-run.
        unsafe {
            std::env::set_var("GARRAIA_R1_CANARY", "leak-canary");
        }
        let tool = BashTool::new(None); // confirmation_enabled = false
        let output = tool
            .execute(
                &ctx(false),
                serde_json::json!({"command": "printenv GARRAIA_R1_CANARY"}),
            )
            .await
            .unwrap();

        assert!(output.is_error, "{}", output.content);
        assert!(output.content.contains("bloqueado"), "{}", output.content);
        // The command must NOT have run: the canary value never shows up.
        assert!(
            !output.content.contains("leak-canary"),
            "risky command auto-ran without confirmation: {}",
            output.content
        );
    }

    #[tokio::test]
    async fn r1_pede_confirmacao_com_canal_ativo() {
        let tool = BashTool::new_with_confirmation(None);
        let output = tool
            .execute(&ctx(false), serde_json::json!({"command": "printenv PATH"}))
            .await
            .unwrap();

        assert!(output.requires_confirmation, "{}", output.content);
        assert!(
            output.content.contains("CONFIRM_REQUIRED"),
            "{}",
            output.content
        );
        // confirmation_request carries is_error=true by design (GAR-187).
        assert!(output.is_error);
    }

    #[tokio::test]
    async fn r1_aprovado_executa() {
        // Confirmation channel ON + already-approved context: the risky
        // command runs (no prompt, no block) — fluxo GAR-187 legítimo.
        let tool = BashTool::new_with_confirmation(None);
        let output = tool
            .execute(&ctx(true), serde_json::json!({"command": "printenv PATH"}))
            .await
            .unwrap();
        assert!(!output.is_error, "{}", output.content);
        assert!(
            !output.content.contains("CONFIRM_REQUIRED"),
            "{}",
            output.content
        );
    }

    #[tokio::test]
    async fn r1_fail_closed_ignora_is_confirmation_approved() {
        // #1075 (auditoria): com confirmação DESLIGADA o flag aprovado é
        // ignorado — o flag vem do histórico e pode ser contaminado por um
        // marcador [CONFIRM_REQUIRED] forjado; sem canal real não há
        // aprovação legítima. Mesmo com approved=true, printenv é bloqueado
        // e o canário não vaza.
        unsafe {
            std::env::set_var("GARRAIA_R1_CANARY", "leak-canary");
        }
        let tool = BashTool::new(None); // confirmation_enabled = false
        let output = tool
            .execute(
                &ctx(true), // aprovado — deve ser IGNORADO
                serde_json::json!({"command": "printenv GARRAIA_R1_CANARY"}),
            )
            .await
            .unwrap();

        assert!(output.is_error, "{}", output.content);
        assert!(output.content.contains("bloqueado"), "{}", output.content);
        assert!(
            !output.content.contains("leak-canary"),
            "approval flag leaked through in fail-closed mode: {}",
            output.content
        );
    }

    // ── R3 (#1075): spawn env isolation + working_dir ──────────────────────

    #[cfg(unix)]
    #[tokio::test]
    async fn r3_bash_tool_isola_ambiente() {
        // The child shell must NOT see the parent's secrets, even when the
        // parent (MCP server) carries API keys; PATH must survive so the
        // shell can find programs.
        unsafe {
            std::env::set_var("GARRAIA_R3_JWT", "jwt-should-not-leak");
            std::env::set_var("ANTHROPIC_API_KEY", "sk-should-not-leak");
        }
        let tool = BashTool::new(None);
        let output = tool
            .execute(
                &ctx(false),
                serde_json::json!({
                    "command": "echo \"JWT=[$GARRAIA_R3_JWT] KEY=[$ANTHROPIC_API_KEY] PATH_OK=$([ -n \"$PATH\" ] && echo sim || echo nao)\""
                }),
            )
            .await
            .unwrap();

        assert!(!output.is_error, "{}", output.content);
        assert!(
            output.content.contains("JWT=[]"),
            "child must not see parent env: {}",
            output.content
        );
        assert!(
            output.content.contains("KEY=[]"),
            "child must not see parent env: {}",
            output.content
        );
        assert!(output.content.contains("PATH_OK=sim"), "{}", output.content);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn r3_working_dir_aplica_ao_spawn() {
        // Unique dir so the assertion can't pass by CWD coincidence.
        let dir = std::env::temp_dir().join(format!("garraia-r3-cwd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut c = ctx(false);
        c.working_dir = Some(dir.to_string_lossy().to_string());
        let tool = BashTool::new(None);
        let output = tool
            .execute(&c, serde_json::json!({"command": "pwd"}))
            .await
            .unwrap();
        let pwd = output.content.trim();
        assert_eq!(
            pwd,
            dir.to_string_lossy().as_ref(),
            "bash must run in working_dir"
        );
        let _ = std::fs::remove_dir(&dir);
    }
}
