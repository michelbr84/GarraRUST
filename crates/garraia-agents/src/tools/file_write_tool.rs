use async_trait::async_trait;
use garraia_common::{Error, Result};
use std::path::PathBuf;

use super::{Tool, ToolContext, ToolOutput};

const MAX_BYTES_ESCRITA: usize = 1024 * 1024; // 1MB

/// Escreve conteúdo em um arquivo com validação de caminho e limite de tamanho.
/// Cria o arquivo se não existir e sobrescreve se já existir.
pub struct FileWriteTool {
    allowed_directories: Option<Vec<PathBuf>>,
}

impl FileWriteTool {
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
            // Para escrita, o diretório pai deve existir e estar dentro dos permitidos
            let parent = path
                .parent()
                .ok_or_else(|| Error::Agent("caminho inválido".into()))?;

            let canonical = parent
                .canonicalize()
                .map_err(|e| {
                    Error::Agent(format!("não foi possível resolver diretório pai: {e}"))
                })?;

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
impl Tool for FileWriteTool {
    fn name(&self) -> &str {
        "file_write"
    }

    fn description(&self) -> &str {
        "Escreve conteúdo em um arquivo no caminho informado. Cria o arquivo se não existir e sobrescreve se já existir."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Caminho do arquivo onde o conteúdo será escrito"
                },
                "content": {
                    "type": "string",
                    "description": "Conteúdo a ser escrito no arquivo"
                }
            },
            "required": ["path", "content"]
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

        let content = input
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Agent("parâmetro 'content' ausente".into()))?;

        if content.len() > MAX_BYTES_ESCRITA {
            return Ok(ToolOutput::error(format!(
                "conteúdo muito grande: {} bytes (limite: {} bytes)",
                content.len(),
                MAX_BYTES_ESCRITA
            )));
        }

        let path = PathBuf::from(path_str);
        self.validate_path(&path)?;

        // Cria diretórios pai se necessário
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| Error::Agent(format!("falha ao criar diretórios: {e}")))?;
        }

        tokio::fs::write(&path, content)
            .await
            .map_err(|e| Error::Agent(format!("falha ao escrever arquivo: {e}")))?;

        Ok(ToolOutput::success(format!(
            "escreveu {} bytes em {}",
            content.len(),
            path_str
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn escreve_arquivo_com_sucesso() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");

        let tool = FileWriteTool::new(None);

        let ctx = ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
        };

        let output = tool
            .execute(
                &ctx,
                serde_json::json!({
                    "path": file_path.to_str().unwrap(),
                    "content": "hello world"
                }),
            )
            .await
            .unwrap();

        assert!(!output.is_error);

        let written = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(written, "hello world");
    }

    #[tokio::test]
    async fn retorna_erro_se_parametros_ausentes() {
        let tool = FileWriteTool::new(None);

        let ctx = ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
        };

        assert!(tool.execute(&ctx, serde_json::json!({})).await.is_err());

        assert!(
            tool.execute(&ctx, serde_json::json!({"path": "/tmp/test"}))
                .await
                .is_err()
        );
    }
}