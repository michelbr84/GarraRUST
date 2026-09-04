use async_trait::async_trait;
use garraia_common::{Error, Result};
use std::path::PathBuf;
use tracing::{debug, warn};

use super::tool_context::{ResolvedPath, process_home_dir, resolve_tool_path};
use super::{Tool, ToolContext, ToolOutput};

const MAX_BYTES_LEITURA: u64 = 1024 * 1024; // 1MB

/// Lê o conteúdo de um arquivo com validação de caminho e limite de tamanho.
pub struct FileReadTool {
    allowed_directories: Option<Vec<PathBuf>>,
}

impl FileReadTool {
    pub fn new(allowed_directories: Option<Vec<PathBuf>>) -> Self {
        Self {
            allowed_directories,
        }
    }

    /// Resolve o argumento cru do modelo: expande `~`, junta caminho relativo
    /// ao `working_dir` da sessão, e rejeita `..`. Ver `tool_context` (#923).
    fn resolve(&self, context: &ToolContext, path_str: &str) -> Result<ResolvedPath> {
        resolve_tool_path(
            path_str,
            context.working_dir.as_deref(),
            process_home_dir().as_deref(),
        )
    }

    fn validate_path(&self, path: &std::path::Path) -> Result<()> {
        // `..` já é rejeitado em `resolve_tool_path`, antes da expansão. A
        // checagem segue aqui porque `validate_path` é chamada direto nos
        // testes e porque defesa em profundidade num caminho de arquivo é
        // barata.
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

    async fn execute(&self, context: &ToolContext, input: serde_json::Value) -> Result<ToolOutput> {
        let path_str = input
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Agent("parâmetro 'path' ausente".into()))?;

        let resolved = self.resolve(context, path_str)?;
        let path = &resolved.path;
        self.validate_path(path)?;
        debug!(
            requested = %resolved.requested,
            resolved = %path.display(),
            origin = ?resolved.origin,
            "file_read"
        );

        // A mensagem carrega o caminho resolvido e COMO ele foi resolvido.
        // Sem isso o modelo recebia apenas "No such file or directory" e
        // reportava ao usuário que o arquivo não existia — que foi
        // literalmente o que aconteceu na issue #923.
        let metadata = tokio::fs::metadata(path).await.map_err(|e| {
            warn!(resolved = %path.display(), error = %e, "file_read: stat falhou");
            Error::Agent(format!(
                "não foi possível ler metadados de {}: {e}",
                resolved.describe()
            ))
        })?;

        if metadata.len() > MAX_BYTES_LEITURA {
            return Ok(ToolOutput::error(format!(
                "arquivo muito grande: {} tem {} bytes (limite: {} bytes)",
                resolved.describe(),
                metadata.len(),
                MAX_BYTES_LEITURA
            )));
        }

        let content = tokio::fs::read_to_string(path).await.map_err(|e| {
            warn!(resolved = %path.display(), error = %e, "file_read: leitura falhou");
            Error::Agent(format!("falha ao ler {}: {e}", resolved.describe()))
        })?;

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
            is_confirmation_approved: false,
            working_dir: None,
            project_id: None,
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
            is_confirmation_approved: false,
            working_dir: None,
            project_id: None,
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
            is_confirmation_approved: false,
            working_dir: None,
            project_id: None,
        };

        let result = tool.execute(&ctx, serde_json::json!({})).await;

        assert!(result.is_err());
    }

    // ─── issue #923 ────────────────────────────────────────────────────────

    fn ctx_with(working_dir: Option<&str>) -> ToolContext {
        ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
            is_confirmation_approved: false,
            working_dir: working_dir.map(str::to_string),
            project_id: None,
        }
    }

    /// O caminho relativo passa a resolver contra o `working_dir` da sessão.
    /// Antes ele ia direto para o CWD do processo do gateway — na Termux,
    /// seja lá qual diretório tenha iniciado o serviço.
    #[tokio::test]
    async fn resolves_a_relative_path_against_working_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("metas.md"), "conteudo").unwrap();

        let tool = FileReadTool::new(None);
        let out = tool
            .execute(
                &ctx_with(Some(dir.path().to_str().unwrap())),
                serde_json::json!({ "path": "metas.md" }),
            )
            .await;
        let out = out.expect("relativo + working_dir deve resolver");
        assert!(!out.is_error);
        assert_eq!(out.content, "conteudo");
    }

    /// `~/...` passa a ser expandido. Sem isso o `PathBuf::from` criava um
    /// diretório literal chamado `~`, e o ENOENT resultante virava
    /// "o arquivo não existe" na boca do agente.
    #[tokio::test]
    async fn expands_tilde_using_home() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.md"), "de casa").unwrap();

        // SAFETY: processo de teste; HOME é restaurado ao fim.
        let prev = std::env::var_os("HOME");
        unsafe { std::env::set_var("HOME", dir.path()) };

        let tool = FileReadTool::new(None);
        let out = tool
            .execute(&ctx_with(None), serde_json::json!({ "path": "~/notes.md" }))
            .await;

        unsafe {
            match prev {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
        }

        let out = out.expect("~ deve expandir");
        assert!(!out.is_error);
        assert_eq!(out.content, "de casa");
    }

    /// O coração da #923: a mensagem tem de dizer ONDE procurou e POR QUE ali.
    /// Um "No such file or directory" pelado é indistinguível, para o modelo,
    /// de "o arquivo não existe" — e foi assim que o usuário foi informado.
    #[tokio::test]
    async fn error_names_the_resolved_path_and_the_reason() {
        let tool = FileReadTool::new(None);
        let err = tool
            .execute(
                &ctx_with(Some("/work/proj")),
                serde_json::json!({ "path": "sumido.md" }),
            )
            .await
            .expect_err("arquivo inexistente deve falhar");

        let msg = err.to_string();
        assert!(msg.contains("/work/proj/sumido.md"), "msg = {msg}");
        assert!(msg.contains("pedido: 'sumido.md'"), "msg = {msg}");
        assert!(msg.contains("working_dir"), "msg = {msg}");
    }

    /// A resolução não afrouxa a postura de segurança: `..` segue barrado, e
    /// agora é barrado antes da expansão do `~`.
    #[tokio::test]
    async fn still_rejects_traversal() {
        let tool = FileReadTool::new(None);
        for path in ["../../etc/passwd", "~/../../etc/passwd"] {
            assert!(
                tool.execute(
                    &ctx_with(Some("/work")),
                    serde_json::json!({ "path": path })
                )
                .await
                .is_err(),
                "{path} deveria ser rejeitado"
            );
        }
    }
}
