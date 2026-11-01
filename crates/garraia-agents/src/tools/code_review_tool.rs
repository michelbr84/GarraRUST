//! # Code Review Tool (Phase 5.3)
//!
//! Automated code review on git diff.
//! Gets diff, sends to LLM for review, returns structured feedback.

use async_trait::async_trait;
use garraia_common::Result;
use std::sync::Arc;
use std::time::Duration;

use super::repo_dir::RepoDir;
use super::{Tool, ToolContext, ToolOutput};
use crate::providers::{ChatMessage, ChatRole, ContentBlock, LlmProvider, LlmRequest, MessagePart};

/// Default timeout for diff operations
const DEFAULT_TIMEOUT_SECS: u64 = 15;

/// #1780: teto da chamada de LLM por dentro do review. O diff tem o teto de
/// `DEFAULT_TIMEOUT_SECS`; o LLM não tinha teto nenhum — a chamada leva
/// 30-120s de rotina contra provider real, e era o teto do modo (20s no
/// `review`) que a matava antes de responder, virando a revisão inteira em
/// erro. Generoso de propósito: revisão de 4096 tokens contra provider lento.
const LLM_TIMEOUT_SECS: u64 = 180;

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
    /// #1780: teto da chamada de LLM por dentro do review. Default
    /// [`LLM_TIMEOUT_SECS`]; sobrescrito em teste por [`Self::com_timeout_de_llm`].
    timeout_de_llm: Duration,
    /// #1225 S2: `agent.sandbox`. Default `off` = host, como sempre.
    sandbox: crate::sandbox::SandboxPolicy,
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
            timeout_de_llm: Duration::from_secs(LLM_TIMEOUT_SECS),
            sandbox: crate::sandbox::SandboxPolicy::default(),
        }
    }

    /// #1780: sobrepõe o teto da chamada de LLM. Testes usam; produção fica
    /// no default de [`LLM_TIMEOUT_SECS`].
    #[must_use = "devolve a tool com o teto de LLM; o receptor é consumido"]
    pub fn com_timeout_de_llm(mut self, secs: u64) -> Self {
        self.timeout_de_llm = Duration::from_secs(secs);
        self
    }

    /// #1225 S2: a policy de `agent.sandbox` que o spawn consulta.
    pub fn set_sandbox_policy(&mut self, policy: crate::sandbox::SandboxPolicy) {
        self.sandbox = policy;
    }

    /// #1225 S2: [`Self::set_sandbox_policy`] em forma de builder, para o
    /// ponto de registro.
    #[must_use = "devolve a tool com a policy; o receptor e consumido"]
    pub fn com_sandbox(mut self, policy: crate::sandbox::SandboxPolicy) -> Self {
        self.sandbox = policy;
        self
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
        // #1272 S3: e `--no-textconv`, pelo mesmo motivo.
        let mut args = vec!["diff".to_string()];
        args.extend(
            crate::git_endurecido::OPCOES_DO_DIFF
                .iter()
                .map(|s| s.to_string()),
        );

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

        // #1272 S3: mesmo prefixo endurecido do `git_diff`.
        let prefixo = crate::git_endurecido::prefixo(repo.cwd_do_git(), self.timeout).await?;
        // #1258 (mesmo defeito raiz do `git_diff`): o git roda no
        // `working_dir` da sessão quando há um — ver [`RepoDir`].
        // #1225 S2: spawn por `sandbox_spawn::executar` (env reduzido, stdin
        // nulo, container quando `agent.sandbox` se aplica).
        let mut argv: Vec<String> = prefixo;
        argv.extend(args);
        let result = crate::sandbox_spawn::executar(
            &self.sandbox,
            crate::sandbox_spawn::Pedido {
                tool: "code_review",
                programa: "git",
                args: &argv,
                cwd: repo.cwd_do_git(),
                env: crate::git_endurecido::ENV,
                timeout: self.timeout,
            },
        )
        .await;

        match result {
            crate::sandbox_spawn::Desfecho::Saida(output) => {
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
            crate::sandbox_spawn::Desfecho::NaoExecutou(e) => {
                Err(format!("Failed to run git diff: {}", e))
            }
            crate::sandbox_spawn::Desfecho::Recusado(motivo) => Err(motivo),
            crate::sandbox_spawn::Desfecho::Timeout => Err(format!(
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

        // #1780: a chamada ao LLM tinha teto ZERO — o diff morria no teto
        // proprio da tool, mas o LLM ficava pendurado até o wrap externo do
        // runtime matar tudo no teto do modo, virando a revisão inteira em
        // `tool timeout`. O teto é daqui para dentro: morre com erro
        // nomeando a etapa, antes do wrap externo.
        let resposta =
            tokio::time::timeout(self.timeout_de_llm, self.provider.complete(&request)).await;
        let response = match resposta {
            Ok(inner) => inner.map_err(|e| format!("LLM review error: {}", e))?,
            Err(_) => {
                return Err(format!(
                    "LLM review timed out after {}s (#1780: o teto da chamada de LLM da tool)",
                    self.timeout_de_llm.as_secs()
                ));
            }
        };

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

    /// #1780: o bound total da revisão — diff (`timeout`) + LLM
    /// (`timeout_de_llm`) somados. O wrap externo do runtime usa o maior
    /// entre este e o teto do modo, então a revisão deixa de morrer no 20s
    /// do modo `review` enquanto o LLM ainda trabalha dentro do teto que a
    /// própria tool respeita.
    fn teto_proprio(&self) -> Option<std::time::Duration> {
        Some(self.timeout + self.timeout_de_llm)
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

    /// #1780: provedor que dorme mais que o teto de LLM configurado — a
    /// chamada morre dentro da tool com erro nomeando a etapa, e não fica
    /// pendurada até o wrap externo do runtime.
    struct ProvedorLento(Duration);

    #[async_trait]
    impl LlmProvider for ProvedorLento {
        fn provider_id(&self) -> &str {
            "lento-de-teste"
        }

        async fn complete(&self, _request: &LlmRequest) -> Result<LlmResponse> {
            tokio::time::sleep(self.0).await;
            Ok(LlmResponse {
                content: vec![ContentBlock::Text {
                    text: "revisão que nunca chega".to_string(),
                }],
                model: "lento".to_string(),
                stop_reason: None,
                usage: None,
            })
        }

        async fn health_check(&self) -> Result<bool> {
            Ok(true)
        }
    }

    /// #1780: o LLM sem resposta dentro do teto proprio da tool vira erro
    /// preciso, antes de o wrap externo matar a chamada inteira.
    ///
    /// **Mutação que este teste pega**: tire o `tokio::time::timeout` de
    /// `review_diff` e ele fica vermelho (o erro seria do provider, sem
    /// "timed out", ou a chamada pendura).
    #[tokio::test]
    async fn llm_sem_resposta_dentro_do_teto_da_tool_e_erro_preciso() {
        let repo = repo_git_temporario("alvo-review-lento", "ramo-review-lento");
        let tool = CodeReviewTool::new(
            Arc::new(ProvedorLento(Duration::from_secs(5))),
            "modelo-de-teste",
            Some(15),
        )
        .com_timeout_de_llm(1);
        let wd = repo.path().to_string_lossy().into_owned();

        let saida = tool
            .execute(&ctx(Some(&wd)), serde_json::json!({}))
            .await
            .expect("execute");

        assert!(saida.is_error, "{}", saida.content);
        assert!(
            saida.content.contains("LLM review timed out after 1s"),
            "erro de LLM sem o teto aplicado:\n{}",
            saida.content
        );
    }

    /// #1780: o teto declarado cobre o diff **e** o LLM — o wrap externo usa
    /// o maior entre este e o teto do modo.
    #[test]
    fn teto_proprio_cobre_diff_mais_llm() {
        let tool = tool_com_eco();
        assert_eq!(
            tool.teto_proprio(),
            Some(Duration::from_secs(15 + LLM_TIMEOUT_SECS))
        );
        // O default (sem override) é o const; o override de teste muda o
        // bound declarado na mesma medida.
        let custom =
            CodeReviewTool::new(Arc::new(ProvedorQueEcoa), "m", None).com_timeout_de_llm(7);
        assert_eq!(
            custom.teto_proprio(),
            Some(Duration::from_secs(DEFAULT_TIMEOUT_SECS + 7))
        );
    }

    /// #1272 S3: o `code_review` usa o mesmo git endurecido — config
    /// plantada nao executa nada, e o diff ainda chega ao revisor.
    #[cfg(unix)]
    #[tokio::test]
    async fn config_plantada_no_repo_nao_roda_programa_nenhum() {
        let repo = repo_git_temporario("alvo-review-1272", "ramo-review-1272");
        let marcas = tempfile::tempdir().expect("tmp");
        let marcadores =
            crate::git_endurecido::planta_programas_no_repo(repo.path(), marcas.path());
        let wd = repo.path().to_string_lossy().into_owned();
        let saida = tool_com_eco()
            .execute(&ctx(Some(&wd)), serde_json::json!({}))
            .await
            .expect("execute");
        assert!(!saida.is_error, "{}", saida.content);
        assert!(
            saida.content.contains("alvo-review-1272"),
            "{}",
            saida.content
        );
        for m in &marcadores {
            assert!(!m.exists(), "code_review executou {}", m.display());
        }
    }

    /// #1272 S3: filtro declarado na config PROPRIA de um submodulo, com o
    /// superprojeto em `diff.submodule=diff`, nao roda pelo `code_review`.
    #[cfg(unix)]
    #[tokio::test]
    async fn code_review_nao_roda_filtro_de_submodulo() {
        let repo = repo_git_temporario("alvo-review-sub", "ramo-review-sub");
        let marcas = tempfile::tempdir().expect("tmp");
        let marca = crate::git_endurecido::planta_filtro_em_submodulo(repo.path(), marcas.path());
        let wd = repo.path().to_string_lossy().into_owned();
        let saida = tool_com_eco()
            .execute(&ctx(Some(&wd)), serde_json::json!({}))
            .await
            .expect("execute");
        assert!(!saida.is_error, "{}", saida.content);
        assert!(!marca.exists(), "code_review rodou o filtro do submodulo");
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
            // #1258: a procedência é nomeada. #1626: sem o caminho absoluto, que
            // vazaria o dir por-orquestrador no envelope servido pela ponte.
            assert!(
                saida
                    .content
                    .contains("**Repositório:** o working_dir da sessão"),
                "{}",
                saida.content
            );
            assert!(
                !saida.content.contains(&wd),
                "o caminho absoluto do working_dir vazou na revisão:\n{}",
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
