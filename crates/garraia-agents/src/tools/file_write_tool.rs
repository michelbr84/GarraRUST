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

/// #1272 S3: o caminho passa por um componente `.git` (arquivo ou
/// diretorio, qualquer caixa, com `.`/espaco no fim que o NTFS descarta)?
///
/// Escrever em `<repo>/.git/config` define `core.fsmonitor`,
/// `filter.<drv>.clean` ou `diff.<drv>.textconv` — programas que o proximo
/// `git_diff` executaria no HOST. Um arquivo `.git` (gitdir redirect) aponta o
/// git para uma config em outro lugar. A recusa e por COMPONENTE: `.gitignore`,
/// `.gitattributes` e `gitnotes.md` continuam gravaveis.
fn toca_diretorio_git(path: &std::path::Path) -> bool {
    path.components().any(|c| match c {
        std::path::Component::Normal(nome) => nome
            .to_str()
            .map(|n| n.trim_end_matches(['.', ' ']).eq_ignore_ascii_case(".git"))
            // Nome que nao e UTF-8 nao e `.git`.
            .unwrap_or(false),
        _ => false,
    })
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
        // #1296: entrada malformada do modelo é observação soft, não erro do
        // turno — a mensagem nomeia o schema e pede o reenvio.
        let path_str = match input.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => {
                return Ok(super::parametro_ausente(
                    "file_write",
                    r#"{"path": string, "content": string}"#,
                    "path",
                ));
            }
        };

        let content = match input.get("content").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => {
                return Ok(super::parametro_ausente(
                    "file_write",
                    r#"{"path": string, "content": string}"#,
                    "content",
                ));
            }
        };

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
        // #1272 S3: checado no caminho pedido E no confinado (canonico), para
        // um symlink `x -> .git` dentro da raiz nao contornar a regra.
        if toca_diretorio_git(&resolved.path) || toca_diretorio_git(&path) {
            tracing::warn!(
                session = %context.session_id,
                "file_write: escrita dentro de .git recusada (#1272)"
            );
            return Ok(ToolOutput::error(format!(
                "escrita recusada: {described} fica dentro de um diretorio .git. A config do \
                 git define programas que as tools de git executariam no host (#1272)."
            )));
        }

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
mod testes_1272 {
    use super::*;
    use crate::tools::approval::ToolApproval;

    fn ctx(dir: &std::path::Path) -> ToolContext {
        ToolContext {
            session_id: "t-1272".into(),
            user_id: None,
            is_heartbeat: false,
            approval: ToolApproval::None,
            working_dir: Some(dir.to_string_lossy().into_owned()),
            project_id: None,
        }
    }

    #[test]
    fn componente_git_e_reconhecido_sem_bloquear_vizinhos() {
        use std::path::Path;
        for sim in [
            "repo/.git/config",
            "repo/.GIT/hooks/x",
            "repo/.git",
            "a/.git/../.git/config",
            "repo/.git./config",
        ] {
            assert!(toca_diretorio_git(Path::new(sim)), "{sim}");
        }
        for nao in [
            "repo/src/.gitignore",
            "repo/gitnotes.md",
            "repo/.gitattributes",
            "x.git",
        ] {
            assert!(!toca_diretorio_git(Path::new(nao)), "{nao}");
        }
    }

    #[tokio::test]
    async fn file_write_recusa_git_config_e_aceita_gitignore() {
        let raiz = tempfile::tempdir().expect("tmp");
        let dir = raiz.path().canonicalize().expect("canon");
        std::fs::create_dir_all(dir.join(".git")).expect("mkdir");
        let tool = FileWriteTool::new(FileJail::from_roots([dir.to_string_lossy().into_owned()]));
        for alvo in [".git/config", ".GIT/hooks/post-checkout", "sub/.git"] {
            let out = tool
                .execute(
                    &ctx(&dir),
                    serde_json::json!({"path": alvo, "content": "[core]\nfsmonitor = /x"}),
                )
                .await
                .expect("ToolOutput");
            assert!(out.is_error, "{alvo}: {out:?}");
            assert!(out.content.contains(".git"), "{}", out.content);
        }
        assert!(!dir.join(".git/config").exists());
        assert!(!dir.join("sub/.git").exists());
        // symlink para dentro de .git dentro da raiz
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.join(".git"), dir.join("atalho")).expect("ln");
            let out = tool
                .execute(
                    &ctx(&dir),
                    serde_json::json!({"path": "atalho/config", "content": "x"}),
                )
                .await
                .expect("ToolOutput");
            assert!(out.is_error, "symlink contornou: {out:?}");
            assert!(!dir.join(".git/config").exists());
        }
        // gemeo positivo
        let out = tool
            .execute(
                &ctx(&dir),
                serde_json::json!({"path": "src/.gitignore", "content": "target/"}),
            )
            .await
            .expect("ToolOutput");
        assert!(!out.is_error, "{out:?}");
        assert!(dir.join("src/.gitignore").exists());
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

    /// #1296: parâmetros ausentes são observação soft (`Ok` + `is_error`),
    /// não `Err` — cada caso nomeia o parâmetro que faltou.
    #[tokio::test]
    async fn parametros_ausentes_sao_observacao_soft() {
        let tool = FileWriteTool::new(FileJail::sessions_only());
        let ctx = ctx_with(None);

        let sem_tudo = tool
            .execute(&ctx, serde_json::json!({}))
            .await
            .expect("parâmetro ausente é soft-error, não Err");
        assert!(sem_tudo.is_error);
        assert!(sem_tudo.content.contains("'path'"), "{}", sem_tudo.content);

        let sem_conteudo = tool
            .execute(&ctx, serde_json::json!({"path": "/tmp/test"}))
            .await
            .expect("parâmetro ausente é soft-error, não Err");
        assert!(sem_conteudo.is_error);
        assert!(
            sem_conteudo.content.contains("'content'"),
            "{}",
            sem_conteudo.content
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
