use async_trait::async_trait;
use garraia_common::{Error, Result};
use std::path::PathBuf;

use super::file_jail::FileJail;
use super::tool_context::{process_home_dir, resolve_tool_path};
use super::{Tool, ToolContext, ToolOutput};

const MAX_BYTES_ESCRITA: usize = 1024 * 1024; // 1MB

/// Escreve conteúdo em um arquivo com confinamento de caminho e limite de
/// tamanho. Cria o arquivo se não existir e sobrescreve se já existir.
///
/// Issue #1244: o jail é obrigatório no construtor — ver [`FileReadTool`].
///
/// [`FileReadTool`]: super::FileReadTool
pub struct FileWriteTool {
    jail: FileJail,
}

impl FileWriteTool {
    pub fn new(jail: FileJail) -> Self {
        Self { jail }
    }

    /// Confina o caminho já resolvido às raízes do jail (#1244).
    ///
    /// O alvo de uma escrita normalmente **não existe**, então `canonicalize`
    /// falharia: o jail sobe até o ancestral existente mais próximo e recola a
    /// cauda. É o que barra `raiz/link-para-fora/novo.txt`, que uma checagem
    /// só do `parent` textual deixaria passar.
    fn confine(&self, context: &ToolContext, path: &std::path::Path) -> Result<PathBuf> {
        self.jail
            .confine(path, context.working_dir.as_deref())
            .map_err(|denial| {
                tracing::warn!(
                    session = %context.session_id,
                    motivo = ?denial,
                    "file_write: caminho recusado pelo jail"
                );
                Error::from(denial)
            })
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

    async fn execute(&self, context: &ToolContext, input: serde_json::Value) -> Result<ToolOutput> {
        let path_str = input
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Agent("parâmetro 'path' ausente".into()))?;

        let content = input
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Agent("parâmetro 'content' ausente".into()))?;

        // Validate UTF-8 (content from JSON is always valid UTF-8, but log it for clarity)
        if content.is_empty() {
            tracing::debug!(path = path_str, "file_write: writing empty file");
        }

        if content.len() > MAX_BYTES_ESCRITA {
            return Ok(ToolOutput::error(format!(
                "conteúdo muito grande: {} bytes (limite: {} bytes)",
                content.len(),
                MAX_BYTES_ESCRITA
            )));
        }

        // Issue #923: expande `~`, junta relativo ao `working_dir` da sessão e
        // rejeita `..` — a mesma resolução do `file_read`. Um write no lugar
        // errado é a metade pior do bug: uma leitura que erra o alvo aparece,
        // um write que cai no CWD do processo do gateway, não.
        let resolved = resolve_tool_path(
            path_str,
            context.working_dir.as_deref(),
            process_home_dir().as_deref(),
        )?;
        let described = resolved.describe();
        let path = self.confine(context, &resolved.path)?;

        // Cria diretórios pai se necessário
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                Error::Agent(format!("falha ao criar diretórios para {described}: {e}"))
            })?;
        }

        // Check if file exists and is readonly before attempting write
        if path.exists() {
            match tokio::fs::metadata(&path).await {
                Ok(metadata) => {
                    if metadata.permissions().readonly() {
                        tracing::warn!(
                            path = path_str,
                            "file_write: arquivo é somente-leitura (readonly)"
                        );
                        let shown = path.display();
                        return Ok(ToolOutput::error(format!(
                            "arquivo {} é somente-leitura (readonly). \
                             Remova a proteção com: attrib -R \"{}\" (Windows) ou chmod u+w \"{}\" (Linux)",
                            described, shown, shown
                        )));
                    }
                }
                Err(e) => {
                    tracing::debug!(
                        path = path_str,
                        error = %e,
                        "file_write: não foi possível ler metadados (arquivo pode não existir ainda)"
                    );
                }
            }
        }
        // GAR-133: Safe Code Patch — create backup before overwriting existing files
        if path.exists() {
            let backup_path = path.with_extension(format!(
                "{}.bak",
                path.extension()
                    .map_or("".to_string(), |e| e.to_string_lossy().to_string())
            ));
            match tokio::fs::copy(&path, &backup_path).await {
                Ok(bytes) => {
                    tracing::debug!(
                        original = path_str,
                        backup = %backup_path.display(),
                        bytes,
                        "file_write: backup created before overwrite"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        path = path_str,
                        error = %e,
                        "file_write: could not create backup (continuing anyway)"
                    );
                }
            }
        }

        // Write content as UTF-8
        tokio::fs::write(&path, content.as_bytes())
            .await
            .map_err(|e| {
                tracing::error!(
                    path = path_str,
                    error = %e,
                    "file_write: falha ao escrever arquivo"
                );
                Error::Agent(format!("falha ao escrever {described}: {e}"))
            })?;

        tracing::info!(
            path = path_str,
            bytes = content.len(),
            "file_write: arquivo escrito com sucesso"
        );

        Ok(ToolOutput::success(format!(
            "escreveu {} bytes em {}",
            content.len(),
            path.display()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::file_jail::DENIAL_MESSAGE;

    fn ctx_with(working_dir: Option<&str>) -> ToolContext {
        ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
            approval: crate::tools::approval::ToolApproval::None,
            working_dir: working_dir.map(str::to_string),
            project_id: None,
        }
    }

    fn raiz() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = std::fs::canonicalize(tmp.path()).expect("canonicalize");
        (tmp, root)
    }

    #[tokio::test]
    async fn escreve_arquivo_com_sucesso() {
        let (_t, root) = raiz();
        let file_path = root.join("test.txt");

        let tool = FileWriteTool::new(FileJail::from_roots([&root]));

        let output = tool
            .execute(
                &ctx_with(None),
                serde_json::json!({
                    "path": file_path.to_str().expect("utf8"),
                    "content": "hello world"
                }),
            )
            .await
            .expect("deve escrever");

        assert!(!output.is_error);
        assert_eq!(
            std::fs::read_to_string(&file_path).expect("read"),
            "hello world"
        );
    }

    #[tokio::test]
    async fn retorna_erro_se_parametros_ausentes() {
        let tool = FileWriteTool::new(FileJail::sessions_only());
        let ctx = ctx_with(None);

        assert!(tool.execute(&ctx, serde_json::json!({})).await.is_err());
        assert!(
            tool.execute(&ctx, serde_json::json!({"path": "/tmp/test"}))
                .await
                .is_err()
        );
    }

    // ─── issue #1244: o jail ───────────────────────────────────────────────

    /// Escrita fora da raiz é recusada — e o arquivo alvo **não** aparece no
    /// disco.
    #[tokio::test]
    async fn escrita_fora_da_raiz_e_recusada() {
        let (_t, root) = raiz();
        let (_t2, fora) = raiz();
        let alvo = fora.join("plantado.txt");

        let tool = FileWriteTool::new(FileJail::from_roots([&root]));
        let err = tool
            .execute(
                &ctx_with(None),
                serde_json::json!({
                    "path": alvo.to_str().expect("utf8"),
                    "content": "carga"
                }),
            )
            .await
            .expect_err("fora da raiz deve ser recusado");

        assert!(err.to_string().ends_with(DENIAL_MESSAGE), "{err}");
        assert!(!alvo.exists(), "o arquivo nao pode ter sido criado");
    }

    /// O caso que uma checagem só do `parent` textual deixaria passar: o pai
    /// existe, está dentro da raiz pelo nome, e é um symlink para fora. O
    /// alvo ainda não existe, então `canonicalize` do próprio alvo falha — é
    /// por isso que o jail sobe até o ancestral existente e canonicaliza ELE.
    #[cfg(unix)]
    #[tokio::test]
    async fn escrita_atraves_de_symlink_de_diretorio_e_recusada() {
        let (_t, root) = raiz();
        let (_t2, fora) = raiz();
        std::os::unix::fs::symlink(&fora, root.join("saida")).expect("symlink");
        let alvo_real = fora.join("plantado.txt");

        let tool = FileWriteTool::new(FileJail::sessions_only());
        let err = tool
            .execute(
                &ctx_with(Some(root.to_str().expect("utf8"))),
                serde_json::json!({ "path": "saida/plantado.txt", "content": "carga" }),
            )
            .await
            .expect_err("symlink de diretorio para fora deve ser recusado");

        assert!(err.to_string().ends_with(DENIAL_MESSAGE), "{err}");
        assert!(!alvo_real.exists(), "a escrita vazou pelo symlink");
    }

    /// O furo da auditoria R4, provado pelo **call site** e não só pela
    /// função pura: `raiz/evil` é um symlink pendurado para fora da raiz.
    /// `canonicalize` falha (o alvo não existe), o jail recolava a cauda
    /// dentro da raiz, o `starts_with` aprovava — e o `open(O_CREAT)` do
    /// `tokio::fs::write` **segue** o link e materializa o byte lá fora.
    ///
    /// É o vetor plausível: um repositório clonado traz o symlink versionado
    /// no git, o CWD é raiz na CLI, e uma injeção indireta no README manda
    /// escrever em `evil`.
    #[cfg(unix)]
    #[tokio::test]
    async fn escrita_em_symlink_pendurado_e_recusada() {
        let (_t, root) = raiz();
        let (_t2, fora) = raiz();
        let alvo_fora = fora.join("authorized_keys");
        std::os::unix::fs::symlink(&alvo_fora, root.join("evil")).expect("symlink");
        assert!(!alvo_fora.exists(), "precondicao: o alvo ainda nao existe");

        let tool = FileWriteTool::new(FileJail::sessions_only());
        let err = tool
            .execute(
                &ctx_with(Some(root.to_str().expect("utf8"))),
                serde_json::json!({ "path": "evil", "content": "ssh-rsa AAAA..." }),
            )
            .await
            .expect_err("symlink pendurado para fora deve ser recusado");

        assert!(err.to_string().ends_with(DENIAL_MESSAGE), "{err}");
        assert!(
            !alvo_fora.exists(),
            "a escrita vazou pelo symlink pendurado: {}",
            alvo_fora.display()
        );
    }

    /// A mesma coisa com o pendurado como **diretório pai**.
    #[cfg(unix)]
    #[tokio::test]
    async fn escrita_atraves_de_pai_symlink_pendurado_e_recusada() {
        let (_t, root) = raiz();
        let (_t2, fora) = raiz();
        let dir_fora = fora.join("dir-inexistente");
        std::os::unix::fs::symlink(&dir_fora, root.join("saida")).expect("symlink");

        let tool = FileWriteTool::new(FileJail::sessions_only());
        let err = tool
            .execute(
                &ctx_with(Some(root.to_str().expect("utf8"))),
                serde_json::json!({ "path": "saida/plantado.txt", "content": "carga" }),
            )
            .await
            .expect_err("pai pendurado para fora deve ser recusado");

        assert!(err.to_string().ends_with(DENIAL_MESSAGE), "{err}");
        assert!(!dir_fora.exists(), "criou diretorio fora da raiz");
    }

    /// Fail-closed: sem raiz de config e sem `working_dir`, não escreve nada.
    #[tokio::test]
    async fn sem_raiz_nenhuma_nao_escreve() {
        let (_t, root) = raiz();
        let alvo = root.join("x.txt");

        let tool = FileWriteTool::new(FileJail::sessions_only());
        let err = tool
            .execute(
                &ctx_with(None),
                serde_json::json!({ "path": alvo.to_str().expect("utf8"), "content": "x" }),
            )
            .await
            .expect_err("sem raiz deve recusar");

        assert!(err.to_string().ends_with(DENIAL_MESSAGE), "{err}");
        assert!(!alvo.exists());
    }

    /// Dentro da raiz continua sem fricção, inclusive criando subdiretório.
    #[tokio::test]
    async fn dentro_da_raiz_cria_subdiretorio() {
        let (_t, root) = raiz();
        let tool = FileWriteTool::new(FileJail::sessions_only());

        let out = tool
            .execute(
                &ctx_with(Some(root.to_str().expect("utf8"))),
                serde_json::json!({ "path": "notas/dia.md", "content": "oi" }),
            )
            .await
            .expect("dentro da raiz deve escrever");

        assert!(!out.is_error);
        assert_eq!(
            std::fs::read_to_string(root.join("notas/dia.md")).expect("read"),
            "oi"
        );
    }
}
