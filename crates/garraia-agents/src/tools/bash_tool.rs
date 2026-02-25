use async_trait::async_trait;
use garraia_common::{Error, Result};
use std::time::Duration;
use tokio::process::Command;

use super::{Tool, ToolContext, ToolOutput};

const TIMEOUT_PADRAO_SEGS: u64 = 30;
const MAX_BYTES_SAIDA: usize = 32 * 1024;

/// Executa comandos de shell com timeout configurável e limite de saída.
/// No Windows utiliza PowerShell. Em sistemas Unix-like utiliza Bash.
pub struct BashTool {
    timeout: Duration,
}

impl BashTool {
    pub fn new(timeout_secs: Option<u64>) -> Self {
        Self {
            timeout: Duration::from_secs(timeout_secs.unwrap_or(TIMEOUT_PADRAO_SEGS)),
        }
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
        _context: &ToolContext,
        input: serde_json::Value,
    ) -> Result<ToolOutput> {
        let comando = input
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Agent("parâmetro 'command' ausente".into()))?;

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
        };

        let result = tool.execute(&ctx, serde_json::json!({})).await;

        assert!(result.is_err());
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
        };

        let output = tool
            .execute(&ctx, serde_json::json!({"command": cmd}))
            .await
            .unwrap();

        assert!(output.content.contains("err"));
    }
}
