//! # Code Review Tool (Phase 5.3)
//!
//! Automated code review on git diff.
//! Gets diff, sends to LLM for review, returns structured feedback.

use async_trait::async_trait;
use garraia_common::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;

use super::repo_dir::RepoDir;
use super::{Tool, ToolContext, ToolOutput};
use crate::providers::{ChatMessage, ChatRole, ContentBlock, LlmProvider, LlmRequest, MessagePart};

/// Default timeout for diff operations
const DEFAULT_TIMEOUT_SECS: u64 = 15;

/// Maximum diff lines to send to LLM
const MAX_DIFF_LINES: usize = 1000;

/// Automated code review tool that analyzes git diffs using an LLM.
/// Returns structured review with issues, suggestions, and severity ratings.
pub struct CodeReviewTool {
    /// LLM provider for the review
    provider: Arc<dyn LlmProvider>,
    /// Model to use
    model: String,
    /// Timeout for git operations
    timeout: Duration,
}

impl CodeReviewTool {
    /// Create a new CodeReviewTool
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        model: impl Into<String>,
        timeout_secs: Option<u64>,
    ) -> Self {
        Self {
            provider,
            model: model.into(),
            timeout: Duration::from_secs(timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS)),
        }
    }

    /// Get git diff output, **from the repository in `repo`**
    async fn get_diff(
        &self,
        repo: &RepoDir,
        commit_range: Option<&str>,
        file_path: Option<&str>,
    ) -> std::result::Result<String, String> {
        // --no-ext-diff: .git/config plantado (diff.external) não transforma
        // o code_review em execução arbitraria (#1075 — auditoria).
        let mut args = vec!["diff".to_string(), "--no-ext-diff".to_string()];

        if let Some(range) = commit_range {
            // #1269 (paridade com o `git_diff`): o `commit_range` vem do modelo
            // e é um argumento argv — começando com `-` ele vira flag do git
            // (`--output=…` escreve arquivo, `-O…`, `--stdin`). Recusado antes
            // de entrar na linha de comando.
            if range.starts_with('-') {
                return Err(format!(
                    "commit_range '{range}' recusado: revisão não pode começar com '-'"
                ));
            }
            args.push(range.to_string());
        }

        if let Some(path) = file_path {
            args.push("--".to_string());
            args.push(path.to_string());
        }

        let mut cmd = Command::new("git");
        // #1258 (mesmo defeito raiz do `git_diff`): sem `current_dir` o git
        // herdava o CWD do processo do gateway, então o `code_review` revisava
        // o diff de outro repositório — ou nenhum. Ver [`RepoDir`].
        if let Some(dir) = repo.cwd_do_git() {
            cmd.current_dir(dir);
        }
        cmd.args(&args);
        // #1269 (paridade com o `git_diff`): o filho nunca lê a entrada padrão
        // do gateway — em terminal, pipe e serviço o comportamento fica
        // determinado, e não há consumo acidental de stdin.
        cmd.stdin(std::process::Stdio::null());
        // #1075 R3 (parity — auditoria do hardening): o filho git herda só a
        // allowlist de env do pai.
        #[cfg(unix)]
        {
            cmd.env_clear();
            for (key, value) in garraia_common::safety_gate::allowed_child_env() {
                cmd.env(key, value);
            }
        }
        let result = tokio::time::timeout(self.timeout, cmd.output()).await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                if stdout.is_empty() {
                    Err("No diff output (no changes found)".to_string())
                } else {
                    // Limit diff size
                    let lines: Vec<&str> = stdout.lines().collect();
                    if lines.len() > MAX_DIFF_LINES {
                        let truncated: String = lines[..MAX_DIFF_LINES].join("\n");
                        Ok(format!(
                            "{}\n\n... (diff truncated, {} lines total)",
                            truncated,
                            lines.len()
                        ))
                    } else {
                        Ok(stdout)
                    }
                }
            }
            Ok(Err(e)) => Err(format!("Failed to run git diff: {}", e)),
            Err(_) => Err(format!(
                "git diff timed out after {}s",
                self.timeout.as_secs()
            )),
        }
    }

    /// Send diff to LLM for review
    async fn review_diff(
        &self,
        diff: &str,
        file_path: Option<&str>,
    ) -> std::result::Result<String, String> {
        let file_context = file_path
            .map(|p| format!(" for file: {}", p))
            .unwrap_or_default();

        let prompt = format!(
            r#"Review the following git diff{} and provide a structured code review.

For each issue found, specify:
1. **Severity**: Critical / Warning / Info / Suggestion
2. **File & Line**: Where the issue is
3. **Issue**: What the problem is
4. **Suggestion**: How to fix it

Focus on:
- Security vulnerabilities (SQL injection, XSS, secret exposure, etc.)
- Logic errors and potential bugs
- Performance issues
- Code quality and best practices
- Error handling (especially unwrap() in Rust)
- Missing edge cases

If the code looks good, say so and mention any minor improvements.

```diff
{}
```

Provide your review in a clear, structured format."#,
            file_context, diff
        );

        let messages = vec![ChatMessage {
            role: ChatRole::User,
            content: MessagePart::Text(prompt),
        }];

        let request = LlmRequest {
            model: self.model.clone(),
            messages,
            system: Some(
                "You are a senior code reviewer. Provide thorough, constructive feedback focused on correctness, security, and best practices. Be concise but specific.".to_string(),
            ),
            max_tokens: Some(4096),
            temperature: Some(0.3),
            tools: vec![],
        };

        let response = self
            .provider
            .complete(&request)
            .await
            .map_err(|e| format!("LLM review error: {}", e))?;

        let review = response
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");

        if review.is_empty() {
            Err("LLM returned empty review".to_string())
        } else {
            Ok(review)
        }
    }
}

#[async_trait]
impl Tool for CodeReviewTool {
    fn name(&self) -> &str {
        "code_review"
    }

    fn description(&self) -> &str {
        "Performs automated code review on git diff.\n\
         Gets the diff, sends it to an LLM for analysis, and returns structured feedback\n\
         with issues, suggestions, and severity ratings.\n\
         Focuses on security, bugs, performance, and best practices."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "commit_range": {
                    "type": "string",
                    "description": "Git commit range for diff (e.g., 'HEAD~3..HEAD', 'main..feature')"
                },
                "file_path": {
                    "type": "string",
                    "description": "Specific file to review (optional, reviews all changes if omitted)"
                }
            }
        })
    }

    async fn execute(&self, context: &ToolContext, input: serde_json::Value) -> Result<ToolOutput> {
        let commit_range = input.get("commit_range").and_then(|v| v.as_str());
        let file_path = input.get("file_path").and_then(|v| v.as_str());

        // #1258: de qual repositório sai o diff que vai para o LLM.
        let repo = RepoDir::decidir(context.working_dir.as_deref());

        // Get the diff
        let diff = match self.get_diff(&repo, commit_range, file_path).await {
            Ok(d) => d,
            Err(e) => return Ok(ToolOutput::error(repo.com_contexto(&e))),
        };

        // Review the diff
        match self.review_diff(&diff, file_path).await {
            Ok(review) => {
                let mut output = String::new();
                output.push_str("## Code Review\n\n");

                // #1258: a resposta diz de qual repositório ela falou. Sem
                // `working_dir` na sessão o diretório continua sendo o do
                // processo — mas agora explicitamente, não por acidente.
                output.push_str(&format!("**Repositório:** {}\n", repo.descricao()));

                if let Some(range) = commit_range {
                    output.push_str(&format!("**Commit range:** {}\n", range));
                }
                if let Some(path) = file_path {
                    output.push_str(&format!("**File:** {}\n", path));
                }

                output.push_str("\n---\n\n");
                output.push_str(&review);

                Ok(ToolOutput::success(output))
            }
            Err(e) => Ok(ToolOutput::error(
                repo.com_contexto(&format!("Review failed: {}", e)),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::LlmResponse;
    use crate::tools::repo_dir::{contexto_de_teste as ctx, repo_git_temporario};

    /// Provedor que devolve o prompt recebido como se fosse a revisão. É o que
    /// deixa o teste afirmar **qual diff** chegou ao LLM, sem rede nenhuma.
    struct ProvedorQueEcoa;

    #[async_trait]
    impl LlmProvider for ProvedorQueEcoa {
        fn provider_id(&self) -> &str {
            "eco-de-teste"
        }

        async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
            let texto = request
                .messages
                .iter()
                .map(|m| match &m.content {
                    MessagePart::Text(t) => t.clone(),
                    MessagePart::Parts(_) => String::new(),
                })
                .collect::<Vec<_>>()
                .join("\n");

            Ok(LlmResponse {
                content: vec![ContentBlock::Text { text: texto }],
                model: "eco".to_string(),
                stop_reason: None,
                usage: None,
            })
        }

        async fn health_check(&self) -> Result<bool> {
            Ok(true)
        }
    }

    fn tool_com_eco() -> CodeReviewTool {
        CodeReviewTool::new(Arc::new(ProvedorQueEcoa), "modelo-de-teste", Some(15))
    }

    /// #1258 (item 4 da aceitação): o `code_review` tinha o **mesmo** defeito
    /// raiz — `Command::new("git")` sem `current_dir` em `get_diff` —, então
    /// ganha a mesma prova, também pelo caminho do agente.
    ///
    /// **Mutação que este teste pega**: comente o `cmd.current_dir(dir)` de
    /// `CodeReviewTool::get_diff` e ele fica vermelho.
    #[tokio::test]
    async fn diff_revisado_vem_do_working_dir_da_sessao() {
        let repo_a = repo_git_temporario("alvo-review-a", "ramo-review-a");
        let repo_b = repo_git_temporario("alvo-review-b", "ramo-review-b");
        let tool = tool_com_eco();

        for (repo, meu, do_outro) in [
            (&repo_a, "alvo-review-a", "alvo-review-b"),
            (&repo_b, "alvo-review-b", "alvo-review-a"),
        ] {
            let wd = repo.path().to_string_lossy().into_owned();
            let saida = tool
                .execute(&ctx(Some(&wd)), serde_json::json!({}))
                .await
                .expect("execute");

            assert!(!saida.is_error, "{}", saida.content);
            assert!(
                saida.content.contains(meu),
                "o diff revisado tinha de vir de {wd}:\n{}",
                saida.content
            );
            assert!(
                !saida.content.contains(do_outro),
                "o diff revisado veio do repositório errado:\n{}",
                saida.content
            );
            assert!(
                saida
                    .content
                    .contains(&format!("**Repositório:** {wd} (working_dir da sessão)")),
                "{}",
                saida.content
            );
        }
    }

    /// #1258, caso 2 no `code_review`: sem `working_dir` o CWD do processo é
    /// mantido e a resposta o nomeia. Vale para os dois desfechos — a revisão
    /// de uma árvore suja e o "No diff output" de uma árvore limpa, que é
    /// caminho de erro —, então o teste afirma só o que é comum aos dois.
    #[tokio::test]
    async fn sem_working_dir_o_code_review_nomeia_o_repositorio_do_cwd() {
        let tool = tool_com_eco();
        let cwd = std::env::current_dir().expect("CWD do processo de teste");

        let saida = tool
            .execute(&ctx(None), serde_json::json!({}))
            .await
            .expect("execute");

        assert!(
            saida.content.contains(&cwd.display().to_string()),
            "{}",
            saida.content
        );
        assert!(
            saida.content.contains("a sessão não tem working_dir"),
            "{}",
            saida.content
        );
    }

    /// #1269 no `code_review` (paridade): o `commit_range` vem do modelo e é
    /// um argumento argv — começando com `-` viraria flag do git
    /// (`--output=/tmp/x` escreve arquivo, `-O…`, `--stdin`). Recusado antes
    /// de entrar na linha de comando, no mesmo formato que o `git_diff` usa.
    ///
    /// **Mutação que este teste pega**: tire o `if range.starts_with('-')` de
    /// `get_diff` e ele fica vermelho.
    #[tokio::test]
    async fn commit_range_comecando_com_hifen_e_recusado() {
        let tool = tool_com_eco();

        for range in ["--output=/tmp/vazou-1258.diff", "-O/tmp/x", "--stdin"] {
            let saida = tool
                .execute(&ctx(None), serde_json::json!({"commit_range": range}))
                .await
                .expect("execute");

            assert!(saida.is_error, "{range}: {}", saida.content);
            assert!(
                saida.content.contains("recusado"),
                "{range}: {}\n{}",
                range,
                saida.content
            );
        }
    }

    /// Controle positivo da recusa: um `commit_range` legítimo — revisão que
    /// não começa com `-` — passa pela tool, e o diff de verdade chega ao LLM.
    #[tokio::test]
    async fn commit_range_valido_ainda_passa() {
        let repo = repo_git_temporario("alvo-range-valido", "ramo-range-valido");
        let tool = tool_com_eco();
        let wd = repo.path().to_string_lossy().into_owned();

        let saida = tool
            .execute(&ctx(Some(&wd)), serde_json::json!({"commit_range": "HEAD"}))
            .await
            .expect("execute");

        assert!(
            !saida.is_error,
            "`git diff HEAD` num repositório com árvore suja tem de funcionar:\n{}",
            saida.content
        );
        assert!(
            saida.content.contains("alvo-range-valido"),
            "{}",
            saida.content
        );
    }

    #[test]
    fn test_code_review_schema() {
        // We need a provider to create the tool, but we can test the schema statically
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "commit_range": {
                    "type": "string",
                    "description": "Git commit range for diff"
                },
                "file_path": {
                    "type": "string",
                    "description": "Specific file to review"
                }
            }
        });

        assert!(schema.get("properties").is_some());
    }
}
