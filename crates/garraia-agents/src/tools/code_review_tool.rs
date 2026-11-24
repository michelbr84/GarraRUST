//! # Code Review Tool (Phase 5.3)
//!
//! Automated code review on git diff.
//! Gets diff, sends to LLM for review, returns structured feedback.

use async_trait::async_trait;
use garraia_common::Result;
use std::sync::Arc;
use std::time::Duration;

use super::git_diff_tool::{
    PONTA_NAO_RESOLVIDA, linha_do_intervalo_da_arvore, linha_do_intervalo_entre_commits,
    nome_da_revisao, revisao_do_output,
};
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

/// O que um `commit_range` pede ao `git diff`, ja separado em pontas (#1909).
///
/// O `commit_range` e texto livre do modelo e chega ao git como UM argumento.
/// As formas que um unico argumento de revisao tem no `git diff`:
///
/// | pedido   | o git compara                                  |
/// |----------|------------------------------------------------|
/// | (nenhum) | indice .. arvore de trabalho                   |
/// | `A`      | `A` .. arvore de trabalho                      |
/// | `A..B`   | `A` .. `B`                                     |
/// | `A...B`  | base comum de `A` e `B` .. `B`                 |
/// | `A^!`    | pai de `A` .. `A` (o mesmo que `A^..A`)        |
///
/// Ponta vazia vale `HEAD`, como no git (`main..` e `main..HEAD`).
#[derive(Debug, Clone, PartialEq, Eq)]
enum PedidoDeIntervalo {
    /// Sem `commit_range`.
    Arvore,
    /// `A`: uma revisao contra a arvore de trabalho.
    ContraArvore(String),
    /// `A..B` (e `A^!`).
    Entre(String, String),
    /// `A...B`.
    DesdeABaseComum(String, String),
}

/// Separa o `commit_range` nas pontas. Funcao pura: nao valida nem resolve
/// nada — quem resolve e [`CodeReviewTool::resolver_intervalo`], e uma ponta
/// que nao e commit deste repositorio simplesmente nao resolve.
///
/// O `...` e procurado antes do `..`, senao `a...b` sairia como `a` e `.b`.
fn interpretar_commit_range(range: Option<&str>) -> PedidoDeIntervalo {
    // Vazio ou so espaco e "sem intervalo" — modelo manda `""` com frequencia.
    let Some(range) = range.filter(|r| !r.trim().is_empty()) else {
        return PedidoDeIntervalo::Arvore;
    };
    let ou_head = |ponta: &str| {
        if ponta.is_empty() {
            "HEAD".to_string()
        } else {
            ponta.to_string()
        }
    };
    if let Some((de, ate)) = range.split_once("...") {
        PedidoDeIntervalo::DesdeABaseComum(ou_head(de), ou_head(ate))
    } else if let Some((de, ate)) = range.split_once("..") {
        PedidoDeIntervalo::Entre(ou_head(de), ou_head(ate))
    } else if let Some(revisao) = range.strip_suffix("^!").filter(|r| !r.is_empty()) {
        PedidoDeIntervalo::Entre(format!("{revisao}^"), revisao.to_string())
    } else {
        PedidoDeIntervalo::ContraArvore(range.to_string())
    }
}

/// Um intervalo com as pontas ja resolvidas: a linha que o nomeia e o
/// argumento de revisao que o `git diff` recebe — SHAs, nunca os nomes.
#[derive(Debug)]
struct IntervaloResolvido {
    linha: String,
    /// `None` no diff sem commits (indice contra arvore de trabalho).
    argumento: Option<String>,
}

/// A linha do intervalo de `git diff A`: uma revisao contra a arvore de
/// trabalho — o que inclui o que ja esta no indice.
fn linha_do_intervalo_contra_a_arvore(revisao: &str, sha: Option<&str>) -> String {
    format!(
        "Intervalo: {} ({}) .. árvore de trabalho (inclui o que está no índice)",
        nome_da_revisao(revisao),
        sha.unwrap_or(PONTA_NAO_RESOLVIDA),
    )
}

/// A linha do intervalo de `git diff A...B`: o lado de partida e a base
/// comum das duas pontas, que o git calcula; a linha nomeia as pontas de que
/// ela sai (com SHA) e nao finge um SHA de base que a tool nao leu.
fn linha_do_intervalo_desde_a_base_comum(
    de: &str,
    sha_de: Option<&str>,
    ate: &str,
    sha_ate: Option<&str>,
) -> String {
    let (nome_de, nome_ate) = (nome_da_revisao(de), nome_da_revisao(ate));
    let sha_ate = sha_ate.unwrap_or(PONTA_NAO_RESOLVIDA);
    format!(
        "Intervalo: base comum de {nome_de} ({}) e {nome_ate} ({sha_ate}) .. {nome_ate} ({sha_ate})",
        sha_de.unwrap_or(PONTA_NAO_RESOLVIDA),
    )
}

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

    /// Roda UM git no repositorio de `repo`, com o prefixo endurecido ja
    /// calculado. #1258: o git roda no `working_dir` da sessão quando há um —
    /// ver [`RepoDir`]. #1225 S2: spawn por `sandbox_spawn::executar` (env
    /// reduzido, stdin nulo, container quando `agent.sandbox` se aplica).
    async fn rodar_git(
        &self,
        repo: &RepoDir,
        prefixo: &[String],
        args: Vec<String>,
    ) -> crate::sandbox_spawn::Desfecho {
        let mut argv: Vec<String> = prefixo.to_vec();
        argv.extend(args);
        crate::sandbox_spawn::executar(
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
        .await
    }

    /// O SHA completo do commit que `revisao` nomeia; `Ok(None)` quando o git
    /// RODOU e nao a resolve para UM commit deste repositorio (#1909).
    ///
    /// `--verify` com `^{commit}`: uma arvore, um blob ou um nome que nao
    /// existe nao viram ponta de intervalo. So conta a saida de um git que
    /// terminou com sucesso, com exatamente uma linha em forma de SHA.
    ///
    /// `Err` e outra coisa: o git NAO rodou (sandbox recusou, timeout, spawn
    /// falhou). Isso nao pode sair como "ponta nao resolvida" — o modelo iria
    /// cacar um nome de revisao que nao e o problema.
    async fn revisao_de(
        &self,
        repo: &RepoDir,
        prefixo: &[String],
        revisao: &str,
    ) -> std::result::Result<Option<String>, String> {
        // Uma revisao so nao contem `..` (regra do `git check-ref-format`):
        // o que sobrou com `..` depois da separacao e intervalo dentro de
        // ponta, e nem chega ao git. O `-` inicial ja foi recusado antes.
        if revisao.contains("..") || revisao.starts_with('-') {
            return Ok(None);
        }
        let args = vec![
            "rev-parse".to_string(),
            "--verify".to_string(),
            format!("{revisao}^{{commit}}"),
        ];
        let output = match self.rodar_git(repo, prefixo, args).await {
            crate::sandbox_spawn::Desfecho::Saida(output) => output,
            crate::sandbox_spawn::Desfecho::NaoExecutou(e) => {
                return Err(format!("Failed to run git rev-parse: {e}"));
            }
            crate::sandbox_spawn::Desfecho::Recusado(motivo) => return Err(motivo),
            crate::sandbox_spawn::Desfecho::Timeout => {
                return Err(format!(
                    "git rev-parse timed out after {}s",
                    self.timeout.as_secs()
                ));
            }
        };
        if !output.status.success() {
            return Ok(None);
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut linhas = stdout.lines().filter(|l| !l.trim().is_empty());
        let unica = linhas.next().and_then(revisao_do_output);
        Ok(match linhas.next() {
            Some(_) => None,
            None => unica.map(str::to_string),
        })
    }

    /// Resolve as pontas do pedido ANTES do diff (#1909, paridade com o
    /// `git_diff`). O diff roda sobre os SHAs resolvidos, entao a linha
    /// `Intervalo:` nomeia exatamente o que foi revisado: um ramo que se mova
    /// entre as chamadas nao rotula a revisao com um SHA que nao e o do diff.
    ///
    /// `Err` carrega a recusa pronta: sem todas as pontas resolvidas o diff
    /// nao roda — e nada vai para o LLM.
    async fn resolver_intervalo(
        &self,
        repo: &RepoDir,
        prefixo: &[String],
        pedido: &PedidoDeIntervalo,
    ) -> std::result::Result<IntervaloResolvido, String> {
        let recusa = |linha: String| {
            format!(
                "{linha}\n\nO diff não foi executado e nada foi revisado: cada ponta do \
                 commit_range precisa ser um commit deste repositório. Formas aceitas: \
                 'A', 'A..B', 'A...B' e 'A^!'."
            )
        };
        match pedido {
            // Sem intervalo o `git diff` compara o indice com a arvore de
            // trabalho. O `HEAD` nao e uma das pontas, mas diz EM QUAL
            // checkout a comparacao foi feita; sem commit legivel a linha
            // diz isso e o diff segue.
            PedidoDeIntervalo::Arvore => Ok(IntervaloResolvido {
                linha: linha_do_intervalo_da_arvore(
                    self.revisao_de(repo, prefixo, "HEAD").await?.as_deref(),
                ),
                argumento: None,
            }),
            PedidoDeIntervalo::ContraArvore(revisao) => {
                let sha = self.revisao_de(repo, prefixo, revisao).await?;
                let linha = linha_do_intervalo_contra_a_arvore(revisao, sha.as_deref());
                match sha {
                    Some(sha) => Ok(IntervaloResolvido {
                        linha,
                        argumento: Some(sha),
                    }),
                    None => Err(recusa(linha)),
                }
            }
            PedidoDeIntervalo::Entre(de, ate) => {
                let (sha_de, sha_ate) = tokio::join!(
                    self.revisao_de(repo, prefixo, de),
                    self.revisao_de(repo, prefixo, ate),
                );
                let (sha_de, sha_ate) = (sha_de?, sha_ate?);
                let linha = linha_do_intervalo_entre_commits(
                    de,
                    sha_de.as_deref(),
                    ate,
                    sha_ate.as_deref(),
                );
                match (sha_de, sha_ate) {
                    (Some(sha_de), Some(sha_ate)) => Ok(IntervaloResolvido {
                        linha,
                        argumento: Some(format!("{sha_de}..{sha_ate}")),
                    }),
                    _ => Err(recusa(linha)),
                }
            }
            PedidoDeIntervalo::DesdeABaseComum(de, ate) => {
                let (sha_de, sha_ate) = tokio::join!(
                    self.revisao_de(repo, prefixo, de),
                    self.revisao_de(repo, prefixo, ate),
                );
                let (sha_de, sha_ate) = (sha_de?, sha_ate?);
                let linha = linha_do_intervalo_desde_a_base_comum(
                    de,
                    sha_de.as_deref(),
                    ate,
                    sha_ate.as_deref(),
                );
                match (sha_de, sha_ate) {
                    (Some(sha_de), Some(sha_ate)) => Ok(IntervaloResolvido {
                        linha,
                        argumento: Some(format!("{sha_de}...{sha_ate}")),
                    }),
                    _ => Err(recusa(linha)),
                }
            }
        }
    }

    /// Get git diff output, **from the repository in `repo`**, junto da linha
    /// que nomeia o intervalo exato comparado (#1909).
    ///
    /// No erro a linha do intervalo vem no proprio texto quando ja era
    /// conhecida: "nada mudou" e "o git falhou" dizem ENTRE O QUE.
    async fn get_diff(
        &self,
        repo: &RepoDir,
        commit_range: Option<&str>,
        file_path: Option<&str>,
    ) -> std::result::Result<(String, String), String> {
        // #1269 (paridade com o `git_diff`): o `commit_range` vem do modelo
        // e é um argumento argv — começando com `-` ele vira flag do git
        // (`--output=…` escreve arquivo, `-O…`, `--stdin`). Recusado antes
        // de qualquer spawn, e vale para CADA ponta: a segunda de `a..-x`
        // chegaria ao `rev-parse` em posicao de opcao.
        if let Some(range) = commit_range
            && range.starts_with('-')
        {
            return Err(format!(
                "commit_range '{range}' recusado: revisão não pode começar com '-'"
            ));
        }
        let pedido = interpretar_commit_range(commit_range);
        let pontas: &[&String] = match &pedido {
            PedidoDeIntervalo::Arvore => &[],
            PedidoDeIntervalo::ContraArvore(a) => &[a],
            PedidoDeIntervalo::Entre(a, b) | PedidoDeIntervalo::DesdeABaseComum(a, b) => &[a, b],
        };
        if let Some(ponta) = pontas.iter().find(|p| p.starts_with('-')) {
            return Err(format!(
                "commit_range recusado: a revisão '{}' não pode começar com '-'",
                nome_da_revisao(ponta)
            ));
        }

        // #1272 S3: mesmo prefixo endurecido do `git_diff`, calculado uma vez
        // para a resolucao das pontas e para o diff.
        let prefixo = crate::git_endurecido::prefixo(repo.cwd_do_git(), self.timeout).await?;
        let intervalo = self.resolver_intervalo(repo, &prefixo, &pedido).await?;
        let linha = intervalo.linha;

        // --no-ext-diff: .git/config plantado (diff.external) não transforma
        // o code_review em execução arbitraria (#1075 — auditoria).
        // #1272 S3: e `--no-textconv`, pelo mesmo motivo.
        let mut args = vec!["diff".to_string()];
        args.extend(
            crate::git_endurecido::OPCOES_DO_DIFF
                .iter()
                .map(|s| s.to_string()),
        );
        args.extend(intervalo.argumento);
        if let Some(path) = file_path {
            args.push("--".to_string());
            args.push(path.to_string());
        }

        match self.rodar_git(repo, &prefixo, args).await {
            crate::sandbox_spawn::Desfecho::Saida(output) => {
                // #1909: um git que falhou nao e "nenhuma mudanca". Sem
                // `--exit-code` o `git diff` sai com 0 havendo ou nao
                // diferenca, entao codigo diferente de zero e falha (duas
                // pontas sem base comum num `A...B`, objeto corrompido) — e
                // antes saia como "No diff output (no changes found)".
                if !output.status.success() {
                    let stderr = crate::git_endurecido::stderr_para_o_turno(
                        &String::from_utf8_lossy(&output.stderr),
                        repo.cwd_do_git(),
                    );
                    return Err(format!(
                        "{linha}\n\ngit diff failed (exit code {}), nothing was reviewed: {}",
                        output.status.code().unwrap_or(-1),
                        stderr.trim()
                    ));
                }
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                if stdout.is_empty() {
                    Err(format!("{linha}\n\nNo diff output (no changes found)"))
                } else {
                    // Limit diff size
                    let lines: Vec<&str> = stdout.lines().collect();
                    if lines.len() > MAX_DIFF_LINES {
                        let truncated: String = lines[..MAX_DIFF_LINES].join("\n");
                        Ok((
                            linha,
                            format!(
                                "{}\n\n... (diff truncated, {} lines total)",
                                truncated,
                                lines.len()
                            ),
                        ))
                    } else {
                        Ok((linha, stdout))
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
                    "description": "Git commit range for diff: 'A..B' (e.g., 'HEAD~3..HEAD', 'main..feature'), 'A...B' (changes on B since its common base with A), 'A^!' (one commit) or a single revision 'A' (A against the working tree). Each end must be a commit of this repository; the review names the exact SHAs compared."
                },
                "file_path": {
                    "type": "string",
                    "description": "Specific file to review (optional, reviews all changes if omitted)"
                }
            }
        })
    }

    /// #1780: o bound total da revisão — git (`timeout`) + LLM
    /// (`timeout_de_llm`) somados. #1909: o git conta TRES vezes, uma por
    /// etapa em sequencia, cada uma com o seu `timeout`: o prefixo
    /// endurecido (`git config`), a resolucao das pontas (em paralelo) e o
    /// diff. O wrap externo do runtime usa o maior
    /// entre este e o teto do modo, então a revisão deixa de morrer no 20s
    /// do modo `review` enquanto o LLM ainda trabalha dentro do teto que a
    /// própria tool respeita.
    fn teto_proprio(&self) -> Option<std::time::Duration> {
        Some(self.timeout * 3 + self.timeout_de_llm)
    }

    async fn execute(&self, context: &ToolContext, input: serde_json::Value) -> Result<ToolOutput> {
        let commit_range = input.get("commit_range").and_then(|v| v.as_str());
        let file_path = input.get("file_path").and_then(|v| v.as_str());

        // #1258: de qual repositório sai o diff que vai para o LLM.
        let repo = RepoDir::decidir(context.working_dir.as_deref());

        // Get the diff
        let (intervalo, diff) = match self.get_diff(&repo, commit_range, file_path).await {
            Ok(par) => par,
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

                // #1909: o `commit_range` e o `file_path` sao texto do modelo
                // ecoado no MESMO cabecalho da linha `Intervalo:`. Crus, uma
                // quebra de linha dentro deles desenhava uma linha
                // `Intervalo:` forjada acima da verdadeira (uma ponta como
                // `HEAD^{/x\n...\n|.}` resolve). Saem em uma linha so.
                if let Some(range) = commit_range {
                    output.push_str(&format!("**Commit range:** {}\n", nome_da_revisao(range)));
                }
                // #1909: o intervalo EXATO que foi revisado, com os SHAs que
                // os nomes valiam na hora. Mesma linha do `git_diff`.
                output.push_str(&intervalo);
                output.push('\n');
                if let Some(path) = file_path {
                    output.push_str(&format!("**File:** {}\n", nome_da_revisao(path)));
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
mod tests;

#[cfg(test)]
mod tests_do_intervalo;
