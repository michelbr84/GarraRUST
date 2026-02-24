use async_trait::async_trait;
use garraia_common::{Error, Result};
use std::path::PathBuf;

use super::{Tool, ToolContext, ToolOutput};

const MAX_BYTES_LEITURA: u64 = 1024 * 1024; // 1MB

/// Lê o conteúdo de um arquivo com validação de caminho e limite de tamanho.
pub struct FileReadTool {
    allowed_directories: Option<Vec<PathBuf>>,
}

impl FileReadTool {
    pub fn new(allowed_directories: Option<Vec<PathBuf>>) -> Self {
        Self { allowed_directories }
    }

    fn validate_path(&self, path: &std::path::Path) -> Result<()> {
        // Bloqueia tentativa de path traversal (../)
        if path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(Error::Security("path traversal não permitido".into()));
        }

        if let Some(allowed) = &self.allowed_directories {
            let canonical = path
                .canonicalize()
                .map_err(|e| Error::Agent(format!("não foi possível resolver o caminho: {e}")))?;

            if !allowed.iter().any(|dir| canonical.starts_with(dir)) {
                return Err(Error::Security(
                    "caminho fora dos diretórios permitidos".into(),
                ));
            }
        }

        Ok(())
    }
}

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &str {
        "file_read"
    }

    fn description(&self) -> &str {
        "Lê o conteúdo de um arquivo no caminho informado."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Caminho do arquivo a ser lido"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(
        &self,
        _context: &ToolContext,
        input: serde_json::Value,
    ) -> Result<ToolOutput> {
        let path_str = input
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Agent("parâmetro 'path' ausente".into()))?;

        let path = PathBuf::from(path_str);
        self.validate_path(&path)?;

        let metadata = tokio::fs::metadata(&path)
            .await
            .map_err(|e| Error::Agent(format!("não foi possível ler metadados do arquivo: {e}")))?;

        if metadata.len() > MAX_BYTES_LEITURA {
            return Ok(ToolOutput::error(format!(
                "arquivo muito grande: {} bytes (limite: {} bytes)",
                metadata.len(),
                MAX_BYTES_LEITURA
            )));
        }

        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| Error::Agent(format!("falha ao ler arquivo: {e}")))?;

        Ok(ToolOutput::success(content))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn le_arquivo_existente() {
        let mut tmp = NamedTempFile::new().unwrap();
        write!(tmp, "hello world").unwrap();

        let tool = FileReadTool::new(None);

        let ctx = ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
        };

        let output = tool
            .execute(
                &ctx,
                serde_json::json!({"path": tmp.path().to_str().unwrap()}),
            )
            .await
            .unwrap();

        assert!(!output.is_error);
        assert_eq!(output.content, "hello world");
    }

    #[tokio::test]
    async fn retorna_erro_para_arquivo_inexistente() {
        let tool = FileReadTool::new(None);

        let ctx = ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
        };

        let result = tool
            .execute(&ctx, serde_json::json!({"path": "/nonexistent/file.txt"}))
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn retorna_erro_se_parametro_ausente() {
        let tool = FileReadTool::new(None);

        let ctx = ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
        };

        let result = tool.execute(&ctx, serde_json::json!({})).await;

        assert!(result.is_err());
    }
}