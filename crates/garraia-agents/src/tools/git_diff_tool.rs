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

use super::repo_dir::RepoDir;
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

/// Argumentos do `git diff`, com valores vindos do modelo **nunca** em
/// posição de flag (#1269 — mesma classe do #1266, corrigido no PR #1268
/// para a `repo_search`).
///
/// `file_path` e `from_commit`/`to_commit` chegam crus da tool call do
/// modelo. O PR #1075 põe `--no-ext-diff` na frente do comando, mas isso não
/// protege: no git, quando a mesma flag aparece mais de uma vez na linha de
/// comando, **a última vence** — um `file_path` igual a `--ext-diff` reabre a
/// execução de comando externo via `diff.external` de um `.git/config`
/// plantado, executando o que o modelo escolher.
///
/// Duas defesas, cada uma fechando o seu vetor:
///
/// 1. O `file_path` vai **depois** do terminador `--`: tudo depois dele é
///    pathspec, nunca opção. Uma consulta legítima por um arquivo cujo nome
///    começa com `-` continua funcionando, buscada literalmente.
/// 2. O token de range `{from}..{to}` não pode ir depois do `--` (viraria
///    pathspec e perde a semântica de revisões), então fica antes dele com
///    validação própria: revisão que começa com `-` põe o token inteiro em
///    posição de opção — `--output=/tmp/../alvo` escreve o diff em caminho
///    escolhido pelo modelo, `-O<arquivo>` lê ordem de arquivo plantado.
///    Revisão legítima não começa com `-` (o git nem consegue invocar uma
///    assim em posição de revisão), então a recusa é fail-closed e controlada.
///
/// É função pura de propósito — o teste afirma a ordem dos argumentos sem
/// subir processo nenhum, e os testes de execução do runtime provam o efeito.
///
/// O contexto de linhas vai colado (`-U{context}`) — o `-U` do git é flag de
/// argumento opcional que só aceita valor anexado: na forma `-U 3` o `3` sobra
/// como argumento posicional (revisão) e o git morre com `bad revision '3'`
/// (verificado no git 2.43). Essa quebra não é inocente aqui: com o diff
/// morrendo antes de qualquer efeito, um argv injetado passava a ser
/// indetectável por efeito observável — o teste de mutação do terminador não
/// conseguia falhar nem sem o `--`.
fn git_diff_args(
    file_path: Option<&str>,
    context_lines: i32,
    from_commit: Option<&str>,
    to_commit: Option<&str>,
) -> std::result::Result<Vec<String>, String> {
    let mut args: Vec<String> = vec![
        "diff".to_string(),
        "--no-ext-diff".to_string(),
        format!("-U{context_lines}"),
    ];

    // Se tem range de commits — validado antes de entrar na linha de comando.
    if let (Some(from), Some(to)) = (from_commit, to_commit) {
        for revisao in [from, to] {
            if revisao.starts_with('-') {
                return Err(format!(
                    "revisão '{revisao}' recusada: revisões de diff não podem começar com '-'"
                ));
            }
        }
        args.push(format!("{from}..{to}"));
    }

    args.push("--".to_string());

    if let Some(path) = file_path {
        args.push(path.to_string());
    }

    Ok(args)
}

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

    /// Executa um comando git com timeout, **no repositório de `repo`**.
    async fn run_git_command(&self, args: &[String], repo: &RepoDir) -> Result<String> {
        let mut cmd = Command::new("git");
        // #1258: o git roda no `working_dir` da sessão quando há um. Sem esta
        // linha ele herdava o CWD do processo do gateway — respondendo sobre
        // outro repositório, e de forma dependente de como o processo subiu
        // (`garra start`, systemd com `WorkingDirectory=`, sidecar, container).
        // Sem `working_dir` o CWD é mantido de propósito, e quem formata a
        // resposta nomeia o diretório (ver [`RepoDir`]).
        if let Some(dir) = repo.cwd_do_git() {
            cmd.current_dir(dir);
        }
        cmd.args(args.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        // #1269 (paridade com o #1266/PR #1268): o filho nunca le a entrada
        // padrao do gateway — em terminal, pipe e servico o comportamento fica
        // o mesmo, e o teste de regressao da injecao nao passa por acidente.
        cmd.stdin(std::process::Stdio::null());
        // #1075 R3 (parity — auditoria do hardening): o filho git herda só a
        // allowlist de env — um .gitconfig plantado com diff.external é
        // execução arbitraria, e não pode carregar segredos do pai junto.
        #[cfg(unix)]
        {
            cmd.env_clear();
            for (key, value) in garraia_common::safety_gate::allowed_child_env() {
                cmd.env(key, value);
            }
        }
        let resultado = tokio::time::timeout(self.timeout, cmd.output()).await;

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
                    combined = format!(
                        "(sem saída, código de saída: {})",
                        output.status.code().unwrap_or(-1)
                    );
                }

                if !output.status.success() {
                    // Se o comando falhou, ainda retorna o output (pode ser "no changes")
                    tracing::warn!(
                        "git command failed with status: {}",
                        output.status.code().unwrap_or(-1)
                    );
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

    /// Obtém o diff do repositório de `repo`
    async fn get_diff(
        &self,
        repo: &RepoDir,
        file_path: Option<&str>,
        context_lines: i32,
        from_commit: Option<&str>,
        to_commit: Option<&str>,
    ) -> Result<String> {
        // #1269: os valores que vêm da tool call do modelo nunca caem em
        // posição de flag — ver `git_diff_args`.
        let args = git_diff_args(file_path, context_lines, from_commit, to_commit)
            .map_err(Error::Agent)?;

        // Executa git diff
        let output = self.run_git_command(&args, repo).await?;

        // Aplica filtros de segurança
        let filtered = self.filter_secrets(&output);
        let limited = self.limit_lines(&filtered);

        Ok(limited)
    }

    /// Obtém o status do repositório de `repo`
    async fn get_status(&self, repo: &RepoDir) -> Result<String> {
        let args: Vec<String> = vec![
            "status".to_string(),
            "--porcelain".to_string(),
            "-b".to_string(),
        ];

        let output = self.run_git_command(&args, repo).await?;

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

    async fn execute(&self, context: &ToolContext, input: serde_json::Value) -> Result<ToolOutput> {
        let operation = input
            .get("operation")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Agent("parâmetro 'operation' ausente".into()))?;

        // #1258: de qual repositório esta chamada fala. Decidido uma vez, aqui,
        // e carregado até o `Command` — e até a resposta, que passa a nomeá-lo.
        let repo = RepoDir::decidir(context.working_dir.as_deref());

        match operation {
            "diff" => {
                let file_path = input.get("file_path").and_then(|v| v.as_str());
                let context_lines = input
                    .get("context_lines")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(DEFAULT_CONTEXT_LINES as i64)
                    as i32;
                let from_commit = input.get("from_commit").and_then(|v| v.as_str());
                let to_commit = input.get("to_commit").and_then(|v| v.as_str());

                // Validação: se um commit é especificado, ambos devem ser
                if (from_commit.is_some() && to_commit.is_none())
                    || (from_commit.is_none() && to_commit.is_some())
                {
                    return Ok(ToolOutput::error(
                        "Para diff entre commits, especifique ambos 'from_commit' e 'to_commit'"
                            .to_string(),
                    ));
                }

                match self
                    .get_diff(&repo, file_path, context_lines, from_commit, to_commit)
                    .await
                {
                    Ok(output) => Ok(ToolOutput::success(repo.com_contexto(&output))),
                    Err(e) => Ok(ToolOutput::error(repo.com_contexto(&e.to_string()))),
                }
            }
            "status" => match self.get_status(&repo).await {
                Ok(output) => Ok(ToolOutput::success(repo.com_contexto(&output))),
                Err(e) => Ok(ToolOutput::error(repo.com_contexto(&e.to_string()))),
            },
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
    use crate::tools::repo_dir::{contexto_de_teste as ctx, repo_git_temporario};

    // ─── #1258: o git roda no repositório da sessão ────────────────────────
    //
    // Os quatro testes abaixo exercitam a tool pelo **caminho do agente**
    // (`execute` com `ToolContext`), não por `run_git_command`. A issue pede
    // esse formato de propósito: um teste que afirma a função interna deixa o
    // ponto de chamada de produção descoberto, e foi assim que o
    // `current_dir` ausente sobreviveu a duas rodadas de hardening desta
    // mesma tool (#1075, #1269).

    /// #1258: o `git diff` vem do `working_dir` da sessão.
    ///
    /// Dois repositórios temporários, cada um com um arquivo modificado de
    /// nome único. Cada chamada cita o marcador do **seu** repositório e não o
    /// do outro — o que só é possível se o `current_dir` daquela chamada valeu.
    ///
    /// **Mutação que este teste pega**: comente o `cmd.current_dir(dir)` de
    /// `run_git_command` e ele fica vermelho. Sem ele o git roda no CWD do
    /// processo de teste — a própria árvore do GarraRUST, que é outro
    /// repositório e cujo diff nunca menciona esses marcadores.
    #[tokio::test]
    async fn diff_vem_do_working_dir_da_sessao() {
        let repo_a = repo_git_temporario("alvo-do-repo-a", "ramo-a");
        let repo_b = repo_git_temporario("alvo-do-repo-b", "ramo-b");
        let tool = GitDiffTool::new(Some(15), Some(500));

        for (repo, meu, do_outro) in [
            (&repo_a, "alvo-do-repo-a", "alvo-do-repo-b"),
            (&repo_b, "alvo-do-repo-b", "alvo-do-repo-a"),
        ] {
            let wd = repo.path().to_string_lossy().into_owned();
            let saida = tool
                .execute(&ctx(Some(&wd)), serde_json::json!({"operation": "diff"}))
                .await
                .expect("execute");

            assert!(!saida.is_error, "{}", saida.content);
            assert!(
                saida.content.contains(meu),
                "o diff tinha de vir de {wd}:\n{}",
                saida.content
            );
            assert!(
                !saida.content.contains(do_outro),
                "o diff veio do repositório errado:\n{}",
                saida.content
            );
        }
    }

    /// #1258: a mesma prova para `operation: "status"`, que passa pelo mesmo
    /// `run_git_command`. Aqui o marcador é o **nome do ramo**, que não depende
    /// do estado sujo da árvore de quem roda a suite.
    #[tokio::test]
    async fn status_vem_do_working_dir_da_sessao() {
        let repo = repo_git_temporario("alvo-do-status", "ramo-so-deste-repo");
        let tool = GitDiffTool::new(Some(15), Some(500));
        let wd = repo.path().to_string_lossy().into_owned();

        let saida = tool
            .execute(&ctx(Some(&wd)), serde_json::json!({"operation": "status"}))
            .await
            .expect("execute");

        assert!(!saida.is_error, "{}", saida.content);
        assert!(
            saida.content.contains("Branch: ramo-so-deste-repo"),
            "{}",
            saida.content
        );
        assert_eq!(
            saida.content.lines().next().unwrap_or_default(),
            format!("Repositório: {wd} (working_dir da sessão)")
        );
    }

    /// #1258, caso 2 — sessão **sem** `working_dir`, que no gateway é o comum
    /// (um prompt de Telegram não traz projeto). O CWD do processo é mantido,
    /// como antes, mas a resposta passa a dizer de qual repositório ela falou:
    /// a resposta errada *silenciosa* era o defeito.
    #[tokio::test]
    async fn sem_working_dir_a_resposta_nomeia_o_repositorio_do_cwd() {
        let tool = GitDiffTool::new(Some(15), Some(500));
        let cwd = std::env::current_dir().expect("CWD do processo de teste");

        for operacao in ["diff", "status"] {
            let saida = tool
                .execute(&ctx(None), serde_json::json!({"operation": operacao}))
                .await
                .expect("execute");

            let primeira = saida.content.lines().next().unwrap_or_default();
            assert!(
                primeira.starts_with("Repositório: "),
                "{operacao}: {primeira}"
            );
            assert!(
                primeira.contains(&cwd.display().to_string()),
                "{operacao}: a resposta tem de nomear o CWD do processo: {primeira}"
            );
            assert!(
                primeira.contains("a sessão não tem working_dir"),
                "{operacao}: {primeira}"
            );
        }
    }

    /// #1258: `working_dir` que não existe falha **nomeando o diretório**, e
    /// não cai de volta no CWD do processo. Um "No such file or directory" sem
    /// dizer qual diretório era o diagnóstico impossível que a issue descreve.
    #[tokio::test]
    async fn working_dir_inexistente_erra_nomeando_o_diretorio() {
        let tool = GitDiffTool::new(Some(15), Some(500));
        let inexistente = "/nao/existe/em/lugar/nenhum-1258";

        let saida = tool
            .execute(
                &ctx(Some(inexistente)),
                serde_json::json!({"operation": "diff"}),
            )
            .await
            .expect("execute");

        assert!(saida.is_error, "{}", saida.content);
        assert!(saida.content.contains(inexistente), "{}", saida.content);
    }

    #[tokio::test]
    async fn test_git_status() {
        let tool = GitDiffTool::new(Some(10), Some(100));

        let ctx = ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
            approval: crate::tools::approval::ToolApproval::None,
            working_dir: None,
            project_id: None,
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
            approval: crate::tools::approval::ToolApproval::None,
            working_dir: None,
            project_id: None,
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
            approval: crate::tools::approval::ToolApproval::None,
            working_dir: None,
            project_id: None,
        };

        // Try diff on a non-existent file
        let output = tool
            .execute(
                &ctx,
                serde_json::json!({
                    "operation": "diff",
                    "file_path": "Cargo.toml"
                }),
            )
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
            approval: crate::tools::approval::ToolApproval::None,
            working_dir: None,
            project_id: None,
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
            approval: crate::tools::approval::ToolApproval::None,
            working_dir: None,
            project_id: None,
        };

        let result = tool.execute(&ctx, serde_json::json!({})).await;

        assert!(result.is_err());
    }

    /// #1269: file_path adversarial igual a uma flag do git fica DEPOIS do
    /// `--` — pathspec, nunca opção. O `--no-ext-diff` deixa de ser desfeito
    /// pela última ocorrência da flag.
    #[test]
    fn file_path_adversarial_vai_depois_do_terminador() {
        let args =
            git_diff_args(Some("--ext-diff"), 3, None, None).expect("file_path é sempre válido");
        assert_eq!(
            args,
            vec![
                "diff".to_string(),
                "--no-ext-diff".to_string(),
                "-U3".to_string(),
                "--".to_string(),
                "--ext-diff".to_string(),
            ]
        );
    }

    /// #1269: range de commits preserva a semântica de revisões (antes do
    /// `--`) e o file_path, quando junto, fica depois.
    #[test]
    fn range_e_file_path_ficam_nos_lados_certos_do_terminador() {
        let args = git_diff_args(Some("src/main.rs"), 3, Some("abc123"), Some("def456"))
            .expect("revisões válidas");
        assert_eq!(
            args,
            vec![
                "diff".to_string(),
                "--no-ext-diff".to_string(),
                "-U3".to_string(),
                "abc123..def456".to_string(),
                "--".to_string(),
                "src/main.rs".to_string(),
            ]
        );
    }

    /// #1269 (segundo achado): `from` começando com `-` põe o token inteiro
    /// em posição de opção — `--output=/tmp/../alvo` escreve o diff onde o
    /// modelo escolher. Recusa controlada, sem subir o git.
    #[test]
    fn from_commit_adversarial_e_recusado() {
        let err = git_diff_args(None, 3, Some("--output=/tmp/"), Some("alvo"))
            .expect_err("from começando com '-' é recusado");
        assert!(
            err.contains("'--output=/tmp/'"),
            "mensagem cita a revisão: {err}"
        );
    }

    /// #1269: `to` adversarial recebe a mesma recusa.
    #[test]
    fn to_commit_adversarial_e_recusado() {
        let err = git_diff_args(None, 3, Some("abc123"), Some("-O"))
            .expect_err("to começando com '-' é recusado");
        assert!(err.contains("'-O'"), "mensagem cita a revisão: {err}");
    }

    /// Caso sem nada do modelo: o `--` termina a lista de opções e não muda
    /// a semântica do diff inteiro do working tree.
    #[test]
    fn sem_valores_do_modelo_termina_na_posicao_de_pathspec() {
        let args = git_diff_args(None, 3, None, None).expect("nada a recusar");
        assert_eq!(
            args,
            vec![
                "diff".to_string(),
                "--no-ext-diff".to_string(),
                "-U3".to_string(),
                "--".to_string(),
            ]
        );
    }
}
