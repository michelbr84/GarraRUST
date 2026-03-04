//! # Git Diff Tool (GAR-237)
//!
//! Ferramenta nativa para retornar `git diff` e status do repositório,
//! com limites de segurança para uso no modo Review.
//!
//! ## Funcionalidades:
//! - `git_diff`: Retorna diferenças do repositório
//! - `git_status`: Retorna status do repositório
//! - Limites de segurança: max_lines, timeout

use async_trait::async_trait;
use garraia_common::{Error, Result};
use std::time::Duration;
use tokio::process::Command;

use super::{Tool, ToolContext, ToolOutput};

/// Timeout padrão para comandos git (em segundos)
const DEFAULT_TIMEOUT_SECS: u64 = 15;

/// Número máximo de linhas retornadas por segurança
const DEFAULT_MAX_LINES: usize = 2000;

/// Número padrão de linhas de contexto no diff
const DEFAULT_CONTEXT_LINES: i32 = 3;

/// Patterns de segredos para filtrar (GAR-237)
const SECRET_PATTERNS: &[&str] = &[
    "token=",
    "api_key",
    "apikey",
    "password=",
    "secret=",
    "bearer ",
    "authorization:",
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
];

/// Ferramenta para executar comandos git de forma segura.
/// Apenas permite operações de leitura (diff, status, log, branch).
pub struct GitDiffTool {
    timeout: Duration,
    max_lines: usize,
}

impl GitDiffTool {
    /// Cria uma nova instância do GitDiffTool
    pub fn new(timeout_secs: Option<u64>, max_lines: Option<usize>) -> Self {
        Self {
            timeout: Duration::from_secs(timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS)),
            max_lines: max_lines.unwrap_or(DEFAULT_MAX_LINES),
        }
    }

    /// Verifica se o output contém possíveis segredos
    #[allow(dead_code)]
    fn contains_secrets(&self, output: &str) -> bool {
        let output_lower = output.to_lowercase();
        SECRET_PATTERNS
            .iter()
            .any(|pattern| output_lower.contains(pattern))
    }

    /// Remove linhas que contêm segredos do output
    fn filter_secrets(&self, output: &str) -> String {
        let lines: Vec<&str> = output.lines().collect();
        let filtered: Vec<&str> = lines
            .iter()
            .filter(|line| {
                let line_lower = line.to_lowercase();
                !SECRET_PATTERNS.iter().any(|p| line_lower.contains(p))
            })
            .copied()
            .collect();
        filtered.join("\n")
    }

    /// Limita o número de linhas do output
    fn limit_lines(&self, output: &str) -> String {
        let lines: Vec<&str> = output.lines().collect();
        if lines.len() > self.max_lines {
            let limited: Vec<&str> = lines.iter().take(self.max_lines).copied().collect();
            let mut result = limited.join("\n");
            result.push_str(&format!(
                "\n\n... ({} linhas adicionales ocultas por limite de segurança)",
                lines.len() - self.max_lines
            ));
            result
        } else {
            output.to_string()
        }
    }

    /// Executa um comando git com timeout
    async fn run_git_command(&self, args: &[String]) -> Result<String> {
        let resultado = tokio::time::timeout(
            self.timeout,
            Command::new("git").args(args.iter().map(|s| s.as_str()).collect::<Vec<_>>()).output(),
        )
        .await;

        match resultado {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                let mut combined = String::new();

                if !stdout.is_empty() {
                    combined.push_str(&stdout);
                }

                if !stderr.is_empty() {
                    if !combined.is_empty() {
                        combined.push('\n');
                    }
                    combined.push_str("STDERR:\n");
                    combined.push_str(&stderr);
                }

                if combined.is_empty() {
                    combined = format!("(sem saída, código de saída: {})", output.status.code().unwrap_or(-1));
                }

                if !output.status.success() {
                    // Se o comando falhou, ainda retorna o output (pode ser "no changes")
                    tracing::warn!("git command failed with status: {}", output.status.code().unwrap_or(-1));
                }

                Ok(combined)
            }
            Ok(Err(e)) => Err(Error::Agent(format!("falha ao executar git: {e}"))),
            Err(_) => Err(Error::Agent(format!(
                "comando git excedeu o tempo limite após {}s",
                self.timeout.as_secs()
            ))),
        }
    }

    /// Obtém o diff do repositório
    async fn get_diff(&self, file_path: Option<&str>, context_lines: i32, from_commit: Option<&str>, to_commit: Option<&str>) -> Result<String> {
        let mut args: Vec<String> = vec!["diff".to_string()];

        // Adiciona linhas de contexto
        args.push("-U".to_string());
        args.push(context_lines.to_string());

        // Se tem range de commits
        if let (Some(from), Some(to)) = (from_commit, to_commit) {
            args.push(format!("{}..{}", from, to));
        }

        // Adiciona file path se especificado
        if let Some(path) = file_path {
            args.push(path.to_string());
        }

        // Executa git diff
        let output = self.run_git_command(&args).await?;

        // Aplica filtros de segurança
        let filtered = self.filter_secrets(&output);
        let limited = self.limit_lines(&filtered);

        Ok(limited)
    }

    /// Obtém o status do repositório
    async fn get_status(&self) -> Result<String> {
        let args: Vec<String> = vec!["status".to_string(), "--porcelain".to_string(), "-b".to_string()];
        
        let output = self.run_git_command(&args).await?;

        // Formata o status de forma mais legível
        let formatted = self.format_status(&output);

        // Aplica filtros de segurança
        let filtered = self.filter_secrets(&formatted);
        let limited = self.limit_lines(&filtered);

        Ok(limited)
    }

    /// Formata a saída do git status --porcelain
    fn format_status(&self, output: &str) -> String {
        let mut result = String::new();

        //获取当前分支
        for line in output.lines() {
            if let Some(stripped) = line.strip_prefix("## ") {
                result.push_str(&format!("Branch: {}\n", stripped));
                continue;
            }

            let status = &line[..2];
            let file = &line[3..];

            let status_desc = match status {
                " M" => "Modificado",
                " A" => "Adicionado",
                " D" => "Deletado",
                " R" => "Renomeado",
                " C" => "Copiado",
                " U" => "Unmerged",
                "??" => "Não rastreado",
                "!!" => "Ignorado",
                _ => "Desconhecido",
            };

            result.push_str(&format!("{}: {}\n", status_desc, file));
        }

        if result.is_empty() {
            result.push_str("Working tree limpo (nenhuma modificação)");
        }

        result
    }
}

#[async_trait]
impl Tool for GitDiffTool {
    fn name(&self) -> &str {
        "git_diff"
    }

    fn description(&self) -> &str {
        "Retorna diferenças do repositório git (diff) e status.\n\
         Use 'operation' para escolher entre 'diff' ou 'status'.\n\
         - diff: retorna as mudanças (aceita file_path, context_lines, from_commit, to_commit)\n\
         - status: retorna arquivos modificados, adicionados, deletados e branch atual\n\
         Limites de segurança: máximo de linhas retornadas, timeout de 15s"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["diff", "status"],
                    "description": "Operação a executar: 'diff' ou 'status'"
                },
                "file_path": {
                    "type": "string",
                    "description": "Caminho do arquivo específico para ver diff (opcional)"
                },
                "context_lines": {
                    "type": "integer",
                    "description": "Número de linhas de contexto no diff (padrão: 3)",
                    "default": 3
                },
                "max_lines": {
                    "type": "integer",
                    "description": "Máximo de linhas no resultado (padrão: 2000)",
                    "default": 2000
                },
                "from_commit": {
                    "type": "string",
                    "description": "Commit inicial para diff entre commits (opcional)"
                },
                "to_commit": {
                    "type": "string",
                    "description": "Commit final para diff entre commits (opcional)"
                }
            },
            "required": ["operation"]
        })
    }

    async fn execute(
        &self,
        _context: &ToolContext,
        input: serde_json::Value,
    ) -> Result<ToolOutput> {
        let operation = input
            .get("operation")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Agent("parâmetro 'operation' ausente".into()))?;

        match operation {
            "diff" => {
                let file_path = input.get("file_path").and_then(|v| v.as_str());
                let context_lines = input
                    .get("context_lines")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(DEFAULT_CONTEXT_LINES as i64) as i32;
                let from_commit = input.get("from_commit").and_then(|v| v.as_str());
                let to_commit = input.get("to_commit").and_then(|v| v.as_str());

                // Validação: se um commit é especificado, ambos devem ser
                if (from_commit.is_some() && to_commit.is_none())
                    || (from_commit.is_none() && to_commit.is_some())
                {
                    return Ok(ToolOutput::error(
                        "Para diff entre commits, especifique ambos 'from_commit' e 'to_commit'".to_string(),
                    ));
                }

                match self.get_diff(file_path, context_lines, from_commit, to_commit).await {
                    Ok(output) => Ok(ToolOutput::success(output)),
                    Err(e) => Ok(ToolOutput::error(e.to_string())),
                }
            }
            "status" => {
                match self.get_status().await {
                    Ok(output) => Ok(ToolOutput::success(output)),
                    Err(e) => Ok(ToolOutput::error(e.to_string())),
                }
            }
            _ => Ok(ToolOutput::error(format!(
                "operação '{}' não suportada. Use 'diff' ou 'status'",
                operation
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_git_status() {
        let tool = GitDiffTool::new(Some(10), Some(100));

        let ctx = ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
            is_confirmation_approved: false,
        };

        let output = tool
            .execute(&ctx, serde_json::json!({"operation": "status"}))
            .await
            .unwrap();

        // Status should work even if there's no git repo
        println!("Status output: {}", output.content);
    }

    #[tokio::test]
    async fn test_git_diff_no_args() {
        let tool = GitDiffTool::new(Some(10), Some(100));

        let ctx = ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
            is_confirmation_approved: false,
        };

        let output = tool
            .execute(&ctx, serde_json::json!({"operation": "diff"}))
            .await
            .unwrap();

        // Diff should work even if there are no changes
        println!("Diff output: {}", output.content);
    }

    #[tokio::test]
    async fn test_git_diff_with_file() {
        let tool = GitDiffTool::new(Some(10), Some(100));

        let ctx = ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
            is_confirmation_approved: false,
        };

        // Try diff on a non-existent file
        let output = tool
            .execute(&ctx, serde_json::json!({
                "operation": "diff",
                "file_path": "Cargo.toml"
            }))
            .await
            .unwrap();

        println!("Diff file output: {}", output.content);
    }

    #[tokio::test]
    async fn test_invalid_operation() {
        let tool = GitDiffTool::new(Some(10), Some(100));

        let ctx = ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
            is_confirmation_approved: false,
        };

        let output = tool
            .execute(&ctx, serde_json::json!({"operation": "invalid"}))
            .await
            .unwrap();

        assert!(output.is_error);
    }

    #[tokio::test]
    async fn test_missing_operation() {
        let tool = GitDiffTool::new(Some(10), Some(100));

        let ctx = ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
            is_confirmation_approved: false,
        };

        let result = tool
            .execute(&ctx, serde_json::json!({}))
            .await;

        assert!(result.is_err());
    }
}
