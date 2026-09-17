//! # Repo Search Tool (Phase 5.3)
//!
//! Searches code semantically using grep + file pattern matching.
//! Returns matching file paths, line numbers, and context.

use async_trait::async_trait;
use garraia_common::{Error, Result};
use std::time::Duration;
use tokio::process::Command;

use super::{Tool, ToolContext, ToolOutput};

/// Maximum output size in bytes
const MAX_OUTPUT_BYTES: usize = 32 * 1024;

/// Default timeout for search operations
const DEFAULT_TIMEOUT_SECS: u64 = 15;

/// Default maximum results returned
const DEFAULT_MAX_RESULTS: usize = 50;

/// Default context lines around matches
const DEFAULT_CONTEXT_LINES: u32 = 2;

/// Argumentos do `rg`, com a `query` **sempre** depois do terminador `--`.
///
/// #1266 (P0): a `query` vem crua da tool call do modelo, e sem terminador ela
/// cai em posicao de flag — `rg --pre=/bin/sh` faz o proprio ripgrep executar
/// cada arquivo varrido como script, o que atravessa o jail de diretorio e o
/// allowlist do `bash_tool` sem passar por nenhum dos dois. O `--` fecha a
/// classe inteira: tudo depois dele e padrao de busca, nunca opcao.
///
/// O `file_pattern` vai na forma `--glob=<valor>` em vez de dois argumentos
/// separados: assim o valor fica preso ao nome da opcao pelo `=` e nao depende
/// de como esta ou aquela versao do ripgrep resolve um valor que comeca com
/// `-`. Isto e funcao pura de proposito — o teste consegue afirmar a ordem dos
/// argumentos sem subir processo nenhum.
fn rg_args(
    query: &str,
    file_pattern: Option<&str>,
    context_lines: u32,
    max_results: usize,
) -> Vec<String> {
    let mut args = vec![
        "--line-number".to_string(),
        "--no-heading".to_string(),
        "--color".to_string(),
        "never".to_string(),
        "-C".to_string(),
        context_lines.to_string(),
        "--max-count".to_string(),
        max_results.to_string(),
    ];
    if let Some(pattern) = file_pattern {
        args.push(format!("--glob={pattern}"));
    }
    args.push("--".to_string());
    args.push(query.to_string());
    args.push(".".to_string());
    args
}

/// Argumentos do `grep` (fallback Unix), mesma regra do `--` (#1266).
///
/// Sem ele, uma `query` como `-f/etc/passwd` faz o grep ler os padroes de um
/// arquivo escolhido pelo modelo em vez de procurar o texto pedido.
fn grep_args(query: &str, context_lines: u32) -> Vec<String> {
    vec![
        "-rn".to_string(),
        "--color=never".to_string(),
        "-C".to_string(),
        context_lines.to_string(),
        "--".to_string(),
        query.to_string(),
        ".".to_string(),
    ]
}

/// Argumentos do `findstr` (fallback Windows) — #1266.
///
/// O `findstr` **nao tem** terminador `--`: as opcoes dele sao `/s`, `/n`, etc.,
/// e qualquer argumento que comece com `/` e lido como opcao mesmo entre aspas.
/// O equivalente documentado pela Microsoft e `/C:<string>`, que declara o texto
/// como string de busca literal. Como o texto viaja grudado no proprio nome da
/// opcao, nao sobra posicao em que ele possa virar flag — nem comecando com `/`
/// nem com `-` (que o findstr, alias, nunca trata como opcao).
///
/// Efeito colateral aceito: `/C:` torna a busca **literal** nesse fallback, em
/// vez da lista de termos separados por espaco que o findstr usa por padrao.
/// Perder regex num caminho de fallback vale menos que a classe de injecao.
fn findstr_args(query: &str) -> Vec<String> {
    vec![
        "/s".to_string(),
        "/n".to_string(),
        format!("/C:{query}"),
        "*.*".to_string(),
    ]
}

/// Searches code in a repository using grep and file pattern matching.
/// Returns matching file paths with line numbers and surrounding context.
pub struct RepoSearchTool {
    timeout: Duration,
    max_results: usize,
}

impl RepoSearchTool {
    /// Create a new RepoSearchTool
    pub fn new(timeout_secs: Option<u64>, max_results: Option<usize>) -> Self {
        Self {
            timeout: Duration::from_secs(timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS)),
            max_results: max_results.unwrap_or(DEFAULT_MAX_RESULTS),
        }
    }

    /// Truncate output if too large
    fn truncate_output(&self, output: &str) -> String {
        if output.len() > MAX_OUTPUT_BYTES {
            let mut end = MAX_OUTPUT_BYTES;
            while end > 0 && !output.is_char_boundary(end) {
                end -= 1;
            }
            let mut truncated = output[..end].to_string();
            truncated.push_str("\n\n... (output truncated)");
            truncated
        } else {
            output.to_string()
        }
    }
}

#[async_trait]
impl Tool for RepoSearchTool {
    fn name(&self) -> &str {
        "repo_search"
    }

    fn description(&self) -> &str {
        "Searches code in the repository using pattern matching.\n\
         Finds files containing the query string, returns file paths, line numbers, and context.\n\
         Supports file pattern filtering (e.g., '*.rs', '*.py')."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query string (regex supported)"
                },
                "file_pattern": {
                    "type": "string",
                    "description": "Glob pattern to filter files (e.g., '*.rs', '**/*.ts')"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of matches to return (default: 50)"
                },
                "context_lines": {
                    "type": "integer",
                    "description": "Number of context lines around each match (default: 2)"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, context: &ToolContext, input: serde_json::Value) -> Result<ToolOutput> {
        let query = input
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Agent("parameter 'query' is required".into()))?;

        if query.is_empty() {
            return Ok(ToolOutput::error("query cannot be empty"));
        }

        let file_pattern = input.get("file_pattern").and_then(|v| v.as_str());
        let max_results = input
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(self.max_results as u64) as usize;
        let context_lines = input
            .get("context_lines")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_CONTEXT_LINES as u64) as u32;

        // Build and execute the search command. `.` abaixo e o CWD do
        // processo — o diretorio da sessao, quando ha um, e o que o usuario
        // quer dizer com "o repositorio" (auditoria do #1039: sem isto, um
        // gateway lancado de dentro de um repo buscava nesse repo).
        let mut cmd = Command::new("rg");
        if let Some(wd) = context.working_dir.as_deref() {
            cmd.current_dir(wd);
        }
        // #1075 R3 (parity — auditoria do hardening): o filho herda só a
        // allowlist de env (RIPGREP_CONFIG_PATH do pai não alcança o rg).
        #[cfg(unix)]
        {
            cmd.env_clear();
            for (key, value) in garraia_common::safety_gate::allowed_child_env() {
                cmd.env(key, value);
            }
        }
        // #1266: o filho nunca le a entrada padrao do gateway. Sem isto o
        // ripgrep decide o que fazer olhando o stdin herdado — com um pipe no
        // lugar (`garra ask`, teste, servico), ele **busca no stdin** em vez de
        // varrer o diretorio, e fica pendurado ate o timeout consumindo a
        // entrada de quem o chamou. `Stdio::null()` torna o comportamento o
        // mesmo em terminal, pipe e servico.
        cmd.stdin(std::process::Stdio::null()).args(rg_args(
            query,
            file_pattern,
            context_lines,
            max_results,
        ));

        let result = tokio::time::timeout(self.timeout, cmd.output()).await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                if stdout.is_empty() && output.status.code() == Some(1) {
                    // rg returns exit code 1 when no matches found
                    return Ok(ToolOutput::success("No matches found."));
                }

                let mut combined = String::new();
                if !stdout.is_empty() {
                    combined.push_str(&stdout);
                }
                if !stderr.is_empty() && !output.status.success() {
                    combined.push_str("\nSTDERR: ");
                    combined.push_str(&stderr);
                }

                if combined.is_empty() {
                    combined = "No matches found.".to_string();
                }

                Ok(ToolOutput::success(self.truncate_output(&combined)))
            }
            Ok(Err(e)) => {
                // rg not found, try grep as fallback
                let mut grep_cmd = Command::new(if cfg!(target_os = "windows") {
                    "findstr"
                } else {
                    "grep"
                });
                if let Some(wd) = context.working_dir.as_deref() {
                    grep_cmd.current_dir(wd);
                }
                // #1075 R3 (parity): mesma allowlist do rg acima.
                #[cfg(unix)]
                {
                    grep_cmd.env_clear();
                    for (key, value) in garraia_common::safety_gate::allowed_child_env() {
                        grep_cmd.env(key, value);
                    }
                }

                // #1266: mesma regra do rg — o fallback tambem nao herda stdin.
                grep_cmd.stdin(std::process::Stdio::null());

                if cfg!(target_os = "windows") {
                    grep_cmd.args(findstr_args(query));
                } else {
                    grep_cmd.args(grep_args(query, context_lines));
                }

                let fallback = tokio::time::timeout(self.timeout, grep_cmd.output()).await;

                match fallback {
                    Ok(Ok(output)) => {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        if stdout.is_empty() {
                            Ok(ToolOutput::success("No matches found."))
                        } else {
                            Ok(ToolOutput::success(self.truncate_output(&stdout)))
                        }
                    }
                    Ok(Err(e2)) => Ok(ToolOutput::error(format!(
                        "Search failed (rg: {}, grep: {})",
                        e, e2
                    ))),
                    Err(_) => Ok(ToolOutput::error(format!(
                        "Search timed out after {}s",
                        self.timeout.as_secs()
                    ))),
                }
            }
            Err(_) => Ok(ToolOutput::error(format!(
                "Search timed out after {}s",
                self.timeout.as_secs()
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repo_search_schema() {
        let tool = RepoSearchTool::new(None, None);
        let schema = tool.input_schema();
        assert!(schema.get("properties").is_some());
        assert_eq!(schema["required"].as_array().map(|a| a.len()), Some(1));
    }

    /// A busca acontece no diretorio da sessao, nao no CWD do gateway.
    #[cfg(not(windows))]
    #[tokio::test]
    async fn searches_in_the_session_dir() {
        let dir = std::env::temp_dir().join(format!("garra-repo-search-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tempdir");
        std::fs::write(dir.join("notas.txt"), "agulha_unica_xyz\n").expect("write");

        let tool = RepoSearchTool::new(Some(10), None);
        let ctx = ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
            approval: crate::tools::approval::ToolApproval::None,
            working_dir: Some(dir.to_string_lossy().into_owned()),
            project_id: None,
        };
        let result = tool
            .execute(&ctx, serde_json::json!({"query": "agulha_unica_xyz"}))
            .await
            .expect("executa");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!result.is_error, "{}", result.content);
        assert!(result.content.contains("notas.txt"), "{}", result.content);
    }

    #[tokio::test]
    async fn test_repo_search_empty_query() {
        let tool = RepoSearchTool::new(None, None);
        let ctx = ToolContext {
            session_id: "test".into(),
            user_id: None,
            is_heartbeat: false,
            approval: crate::tools::approval::ToolApproval::None,
            working_dir: None,
            project_id: None,
        };

        let result = tool
            .execute(&ctx, serde_json::json!({"query": ""}))
            .await
            .expect("should not error");

        assert!(result.is_error);
    }

    #[tokio::test]
    async fn test_repo_search_missing_query() {
        let tool = RepoSearchTool::new(None, None);
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

    // ─── #1266: a query do modelo nunca pode cair em posicao de flag ──────
    //
    // Os tres call sites (`rg`, `grep`, `findstr`) sao cobertos aqui pelos
    // construtores puros de argumento; o caminho do `rg` ganha ainda um teste
    // de execucao pela tool que o runtime registra, em `runtime.rs`
    // (`repo_search_registrada_nao_executa_pre_do_ripgrep`), porque e nele que
    // a injecao virava execucao de comando.

    /// Posicao do `--` no `rg`: tudo que vem do modelo fica depois dele.
    #[test]
    fn rg_args_poem_a_query_depois_do_terminador() {
        let args = rg_args("--pre=/bin/sh", None, 2, 50);
        let term = args
            .iter()
            .position(|a| a == "--")
            .expect("o terminador tem de existir");
        let query = args
            .iter()
            .position(|a| a == "--pre=/bin/sh")
            .expect("a query tem de estar na linha");
        assert!(
            term < query,
            "query antes do terminador vira flag: {args:?}"
        );
        // E nada de padrao de busca duplicado antes do terminador.
        assert!(
            !args[..term].iter().any(|a| a == "--pre=/bin/sh"),
            "{args:?}"
        );
    }

    /// O `file_pattern` tambem vem do modelo: fica colado no `--glob=` para
    /// nao depender de como o parser da vez resolve um valor com `-`.
    #[test]
    fn rg_args_colam_o_glob_no_nome_da_opcao() {
        let args = rg_args("agulha", Some("--pre=/bin/sh"), 2, 50);
        assert!(
            args.contains(&"--glob=--pre=/bin/sh".to_string()),
            "{args:?}"
        );
        assert!(!args.iter().any(|a| a == "--glob"), "{args:?}");
    }

    /// Mesma regra no fallback Unix.
    #[test]
    fn grep_args_poem_a_query_depois_do_terminador() {
        let args = grep_args("-f/etc/passwd", 2);
        let term = args.iter().position(|a| a == "--").expect("terminador");
        let query = args
            .iter()
            .position(|a| a == "-f/etc/passwd")
            .expect("query");
        assert!(term < query, "{args:?}");
    }

    /// O `findstr` nao tem `--`; o equivalente e `/C:`, e a query nunca pode
    /// aparecer como argumento solto (ai um `/` inicial viraria opcao).
    #[test]
    fn findstr_args_embrulham_a_query_em_barra_c() {
        for adversarial in ["/OFF", "-f/etc/passwd", "--pre=/bin/sh"] {
            let args = findstr_args(adversarial);
            assert!(args.contains(&format!("/C:{adversarial}")), "{args:?}");
            assert!(
                !args.iter().any(|a| a == adversarial),
                "query solta na linha do findstr: {args:?}"
            );
        }
    }

    /// Fallback Unix de ponta a ponta: os argumentos que a tool monta, dados
    /// ao `grep` de verdade. O caminho de fallback so dispara quando o `rg`
    /// nao existe na maquina, o que nao da para forcar de dentro do teste —
    /// entao o que se exercita e exatamente a linha de comando que a tool
    /// produz, sem reescreve-la a mao.
    ///
    /// Sem o `--`, o grep leria `/etc/passwd` como arquivo de padroes (`-f`) e
    /// nao acharia a linha; com ele, a busca e literal e acha.
    #[cfg(not(windows))]
    #[test]
    fn grep_trata_query_com_traco_como_texto_literal() {
        let dir = std::env::temp_dir().join(format!(
            "garra-grep-flag-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("tempdir");
        std::fs::write(dir.join("notas.txt"), "antes -f/etc/passwd depois\n").expect("write");

        let saida = std::process::Command::new("grep")
            .args(grep_args("-f/etc/passwd", 0))
            .current_dir(&dir)
            .output();
        let _ = std::fs::remove_dir_all(&dir);

        let saida = saida.expect("grep tem de existir em Unix");
        let stdout = String::from_utf8_lossy(&saida.stdout);
        assert!(
            stdout.contains("notas.txt"),
            "a query devia ser padrao literal; stdout={stdout:?} stderr={:?}",
            String::from_utf8_lossy(&saida.stderr)
        );
    }
}
