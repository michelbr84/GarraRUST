//! # Repo Search Tool (Phase 5.3)
//!
//! Searches code semantically using grep + file pattern matching.
//! Returns matching file paths, line numbers, and context.

use super::repo_dir::RepoDir;
use super::{Tool, ToolContext, ToolOutput};
use crate::sandbox::SandboxPolicy;
use crate::sandbox_spawn::{Desfecho, Pedido};
use async_trait::async_trait;
use garraia_common::{Error, Result};
use std::path::Path;
use std::time::Duration;

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

/// As marcas que provam um repositorio no disco, para a recusa rapida da
/// #1380. Nenhuma delas e lida: so a existencia do caminho importa.
const MARCAS_DE_REPO: &[&str] = &[".git", ".hg", ".svn", ".jj"];

/// O diretorio, ou algum acima dele, e um repositorio?
///
/// So metadado, subindo os ancestrais: quatro `stat` por nivel, alguns
/// microssegundos no total — e o oposto do que a #1380 descreve, que era
/// varrer a arvore inteira ate o timeout.
///
/// `.git` entra como arquivo tambem, e nao so como diretorio: num worktree do
/// git (`git worktree add`) e num submodulo ele e um arquivo apontando para o
/// `.git` real, e recusar ali seria recusar um repositorio de verdade.
///
/// `try_exists` em vez de `exists`, e o `Err` conta como marca PRESENTE: o
/// `exists` devolve `false` tanto para "nao ha `.git`" quanto para "nao deu
/// para olhar" (EACCES num diretorio sem permissao de leitura), e confundir
/// os dois faria a recusa pegar um repositorio de verdade. Na duvida a tool
/// nao recusa e busca, que e exatamente o comportamento de antes da #1380.
fn dentro_de_repositorio(dir: &Path) -> bool {
    dir.ancestors().any(|d| {
        MARCAS_DE_REPO
            .iter()
            .any(|marca| matches!(d.join(marca).try_exists(), Ok(true) | Err(_)))
    })
}

/// O caminho inspecionado pode ir na mensagem de recusa?
///
/// Revisao da onda B: `repo_search` vive no modo `search`, que e a superficie
/// read-only dos canais remotos — o mesmo turno em que o `garra_status`
/// DELIBERADAMENTE retem `session.working_dir` (#1347). Sem este portao, o
/// usuario a quem o `garra_status` negou o diretorio da sessao receberia o
/// caminho absoluto do host (`/home/<alguem>/...`, `/root`) pela mensagem de
/// erro do `repo_search` — a mesma informacao, pela porta dos fundos.
///
/// Fail-closed: fora de um turno (teste direto, chamada sem escopo) o bit e
/// `None` e o caminho NAO sai. Quem ve o caminho e o operador local, para
/// quem ele e acionavel.
///
/// B1 (revisao da onda C): esta funcao ja nasceu correta na direcao segura,
/// mas o `turno_restrito()` respondia `None` para SEMPRE aqui — o escopo era
/// aberto so em volta de `garra_status`, e o ramo do operador local era
/// inalcancavel em producao. Quem garante que a frase acima e verdade e o
/// `com_restricao_do_turno` no despacho do runtime, provado pelo teste
/// `bit_do_turno_chega_a_toda_tool_e_nao_so_a_garra_status` — de nivel de
/// DESPACHO, porque o teste de nivel de funcao nao viu o defeito.
fn pode_revelar_caminho() -> bool {
    !crate::tools::turn_tools::turno_restrito().unwrap_or(true)
}

/// A recusa rapida da #1380, ou `None` quando ha onde buscar.
///
/// ## O defeito
///
/// Sem `working_dir` a busca herda o CWD do processo (ver [`RepoDir`]). Isso
/// tem dois significados MUITO diferentes, e o codigo tratava os dois igual:
///
/// (a) **`garra chat` local**, em que o CWD do processo E o repositorio do
///     usuario. Buscar ali e exatamente o que ele pediu — este caso nao pode
///     falhar, e por isso a funcao devolve `None` nele.
/// (b) **Sessao sem projeto selecionado** (o celular, um canal remoto, o
///     gateway subido por systemd de `/`): o CWD e `/`, `$HOME` ou o diretorio
///     de dados, e a varredura recursiva ia ate estourar o timeout de 15s
///     para no fim nao responder nada util. Quinze segundos de I/O por uma
///     resposta que ja era conhecida no primeiro `stat`.
///
/// O que separa (a) de (b) e a unica pergunta barata que existe: ha um
/// repositorio no CWD ou acima dele? A sessao COM `working_dir` nao passa por
/// aqui — quem escolheu o diretorio decide o que ha nele, e recusar mudaria o
/// contrato de quem ja usa a tool assim.
///
/// `revelar_caminho` vem de [`pode_revelar_caminho`] e decide so quanto a
/// mensagem conta, nunca SE recusa: o turno restrito recebe a mesma recusa,
/// sem o caminho absoluto do host.
fn recusa_sem_repositorio(
    repo: &RepoDir,
    timeout: Duration,
    revelar_caminho: bool,
) -> Option<String> {
    let RepoDir::ProcessoCwd(cwd) = repo else {
        return None;
    };
    match cwd {
        // Caso (a): o CWD do processo e (ou esta dentro de) um repositorio.
        Some(dir) if dentro_de_repositorio(dir) => None,
        Some(dir) => {
            // O operador local ve ONDE a tool olhou, porque ali isso e
            // acionavel; o turno restrito ve a mesma recusa sem o caminho.
            let onde = if revelar_caminho {
                format!("the process directory ({})", dir.display())
            } else {
                "the process directory".to_string()
            };
            Some(format!(
                "No active repository to search. This session has no working directory, and {onde} \
                 is not inside a repository (no {} found in it or above it). Select a project for \
                 this session (set its working directory) and search again. Refused immediately \
                 instead of scanning unrelated directories for {}s and timing out — the answer \
                 would not have been about your repository.",
                MARCAS_DE_REPO.join(", "),
                timeout.as_secs()
            ))
        }
        None => Some(
            "No active repository to search. This session has no working directory and the \
             process directory cannot be read, so there is nowhere to search. Select a project \
             for this session (set its working directory) and search again."
                .to_string(),
        ),
    }
}

/// Searches code in a repository using grep and file pattern matching.
/// Returns matching file paths with line numbers and surrounding context.
pub struct RepoSearchTool {
    timeout: Duration,
    max_results: usize,
    /// #1225 S2: `agent.sandbox`. Default `off` = host, como sempre.
    sandbox: SandboxPolicy,
}

impl RepoSearchTool {
    /// Create a new RepoSearchTool
    pub fn new(timeout_secs: Option<u64>, max_results: Option<usize>) -> Self {
        Self {
            timeout: Duration::from_secs(timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS)),
            max_results: max_results.unwrap_or(DEFAULT_MAX_RESULTS),
            sandbox: SandboxPolicy::default(),
        }
    }

    /// #1225 S2: a policy de `agent.sandbox` que o spawn consulta.
    pub fn set_sandbox_policy(&mut self, policy: SandboxPolicy) {
        self.sandbox = policy;
    }

    /// #1225 S2: [`Self::set_sandbox_policy`] em forma de builder, para o
    /// ponto de registro.
    #[must_use = "devolve a tool com a policy; o receptor e consumido"]
    pub fn com_sandbox(mut self, policy: SandboxPolicy) -> Self {
        self.sandbox = policy;
        self
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
        //
        // #1225 S2: o spawn passa por `sandbox_spawn::executar`, que consulta
        // `agent.sandbox` — no container quando a policy se aplica, nunca no
        // host quando ela foi pedida e nao pode ser aplicada. La tambem moram
        // o `stdin` nulo (#1266: sem ele o ripgrep busca no stdin herdado e
        // fica pendurado) e a allowlist de env (#1075 R3: o
        // RIPGREP_CONFIG_PATH do pai nao alcanca o rg).
        //
        // #1380: `RepoDir::decidir` ja separa "a sessao escolheu um
        // diretorio" de "herda o CWD do processo" (normalizando o
        // `working_dir` em branco, que nao e projeto nenhum). Sem repositorio
        // no CWD herdado nao ha o que buscar, e a recusa sai aqui — antes de
        // qualquer spawn — em vez de depois do timeout.
        let repo = RepoDir::decidir(context.working_dir.as_deref());
        if let Some(motivo) = recusa_sem_repositorio(&repo, self.timeout, pode_revelar_caminho()) {
            return Ok(ToolOutput::error(motivo));
        }
        let cwd = repo.cwd_do_git();
        let args_rg = rg_args(query, file_pattern, context_lines, max_results);
        let rg = crate::sandbox_spawn::executar(
            &self.sandbox,
            Pedido {
                tool: self.name(),
                programa: "rg",
                args: &args_rg,
                cwd,
                env: &[],
                timeout: self.timeout,
            },
        )
        .await;

        match rg {
            Desfecho::Saida(output) => {
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
            Desfecho::NaoExecutou(e) => {
                // rg nao existe (no host, ou na imagem do sandbox): grep no
                // MESMO lugar — o fallback nunca troca container por host.
                let (programa, args) = if cfg!(target_os = "windows") {
                    ("findstr", findstr_args(query))
                } else {
                    ("grep", grep_args(query, context_lines))
                };
                let fallback = crate::sandbox_spawn::executar(
                    &self.sandbox,
                    Pedido {
                        tool: self.name(),
                        programa,
                        args: &args,
                        cwd,
                        env: &[],
                        timeout: self.timeout,
                    },
                )
                .await;

                match fallback {
                    Desfecho::Saida(output) => {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        if stdout.is_empty() {
                            Ok(ToolOutput::success("No matches found."))
                        } else {
                            Ok(ToolOutput::success(self.truncate_output(&stdout)))
                        }
                    }
                    Desfecho::NaoExecutou(e2) => Ok(ToolOutput::error(format!(
                        "Search failed (rg: {}, grep: {})",
                        e, e2
                    ))),
                    Desfecho::Recusado(motivo) => Ok(ToolOutput::error(motivo)),
                    Desfecho::Timeout => Ok(ToolOutput::error(format!(
                        "Search timed out after {}s",
                        self.timeout.as_secs()
                    ))),
                }
            }
            Desfecho::Recusado(motivo) => Ok(ToolOutput::error(motivo)),
            Desfecho::Timeout => Ok(ToolOutput::error(format!(
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

    // ─── #1380: recusa rapida quando nao ha repositorio ativo ─────────────

    /// O contexto que a #1380 descreve: sessao sem `working_dir` (celular,
    /// canal remoto, gateway sem projeto selecionado). Vem do helper que o
    /// `repo_dir` ja expoe aos testes, para nao haver duas nocoes de
    /// "contexto sem diretorio" na crate.
    fn ctx_sem_working_dir() -> ToolContext {
        crate::tools::repo_dir::contexto_de_teste(None)
    }

    /// O programa esta no `PATH`? Sonda de metadado, sem executar nada.
    #[cfg(not(windows))]
    fn existe_no_path(programa: &str) -> bool {
        std::env::var_os("PATH").is_some_and(|path| {
            std::env::split_paths(&path).any(|dir| dir.join(programa).is_file())
        })
    }

    /// Um diretorio fundo SEM nenhuma marca de repositorio acima dele: o
    /// `tempdir` do sistema nao esta dentro de um checkout.
    fn dir_sem_repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(tmp.path().join("a/b/c/d")).expect("subdirs");
        tmp
    }

    /// O caso (b): nada de repositorio no diretorio herdado. A tool recusa, e
    /// a decisao custa alguns `stat` — nao os 15s de varredura da issue.
    #[test]
    fn sem_repositorio_no_cwd_herdado_recusa_e_e_barato() {
        let tmp = dir_sem_repo();
        let fundo = tmp.path().join("a/b/c/d");
        assert!(!dentro_de_repositorio(&fundo), "{}", fundo.display());

        let repo = RepoDir::ProcessoCwd(Some(fundo.clone()));
        let antes = std::time::Instant::now();
        let motivo = recusa_sem_repositorio(&repo, Duration::from_secs(15), true)
            .expect("sem repositorio, a tool tem de recusar");
        let gasto = antes.elapsed();

        assert!(
            gasto < Duration::from_millis(500),
            "a recusa levou {gasto:?} — a issue pede resposta imediata, nao o timeout"
        );
        assert!(motivo.contains("No active repository"), "{motivo}");
        // A mensagem diz ONDE ele olhou e O QUE fazer: sem isso o operador
        // nao tem como distinguir isto de "a busca nao encontrou nada".
        assert!(motivo.contains(&fundo.display().to_string()), "{motivo}");
        assert!(motivo.contains("working directory"), "{motivo}");
        assert!(motivo.contains(".git"), "{motivo}");

        // CWD ilegivel tambem recusa, e sem citar caminho nenhum.
        let motivo =
            recusa_sem_repositorio(&RepoDir::ProcessoCwd(None), Duration::from_secs(15), true)
                .expect("sem CWD legivel nao ha onde buscar");
        assert!(motivo.contains("No active repository"), "{motivo}");
    }

    /// Revisao da onda B: o turno restrito recebe a MESMA recusa, sem o
    /// caminho absoluto do host. `repo_search` vive no modo `search`, o mesmo
    /// turno em que o `garra_status` retem `session.working_dir` (#1347) —
    /// sem este portao a mensagem de erro entregaria pela porta dos fundos o
    /// caminho que a outra tool nega.
    #[test]
    fn turno_restrito_recusa_sem_entregar_o_caminho_do_host() {
        let tmp = dir_sem_repo();
        let fundo = tmp.path().join("a/b/c/d");
        let repo = RepoDir::ProcessoCwd(Some(fundo.clone()));

        let motivo = recusa_sem_repositorio(&repo, Duration::from_secs(15), false)
            .expect("o turno restrito recusa igual: muda o que a mensagem conta, nao a decisao");
        // Espelho do caso aberto: mesma recusa acionavel, sem o caminho.
        assert!(motivo.contains("No active repository"), "{motivo}");
        assert!(motivo.contains("working directory"), "{motivo}");
        assert!(motivo.contains("the process directory"), "{motivo}");
        assert!(
            !motivo.contains(&fundo.display().to_string()),
            "o caminho do host vazou para um turno restrito: {motivo}"
        );
        // Nem o caminho inteiro, nem um pedaco dele que ja localize o host.
        //
        // So o componente que ESTE teste criou e que identifica o host: o
        // nome sorteado do tempdir. Varrer o caminho inteiro arrastaria junto
        // os componentes do `$TMPDIR` herdado do runner, e um `TMPDIR` que
        // por acaso contivesse "search" ou "project" derrubaria o teste por
        // um motivo que nao e o desta issue; os diretorios `a/b/c/d` sao
        // letras soltas, que casam com qualquer prosa.
        let nome_do_tempdir = tmp
            .path()
            .file_name()
            .expect("tempdir tem nome")
            .to_string_lossy()
            .to_string();
        assert!(
            !motivo.contains(&nome_do_tempdir),
            "o componente {nome_do_tempdir:?} do caminho vazou: {motivo}"
        );
    }

    /// O portao que liga o bit do turno a mensagem, com o fail-closed que o
    /// resto nao cobre: sem turno (chamada fora de escopo) o caminho NAO sai.
    #[tokio::test]
    async fn caminho_so_sai_no_turno_aberto_e_fail_closed_sem_turno() {
        use crate::tools::turn_tools::com_ferramentas_do_turno;

        assert!(
            !pode_revelar_caminho(),
            "fora de um turno o bit e desconhecido, e o fail-closed e esconder"
        );
        let restrito = com_ferramentas_do_turno(vec!["repo_search".to_string()], true, async {
            pode_revelar_caminho()
        })
        .await;
        assert!(!restrito, "turno restrito nao pode ver o caminho do host");
        let aberto = com_ferramentas_do_turno(vec!["repo_search".to_string()], false, async {
            pode_revelar_caminho()
        })
        .await;
        assert!(aberto, "o operador local ve onde a tool olhou");
    }

    /// O caso (a), que a recusa NAO pode pegar: sem `working_dir`, mas com o
    /// CWD do processo dentro de um repositorio — o `garra chat` rodando na
    /// raiz do projeto do usuario. Aqui a tool segue buscando, como sempre.
    #[test]
    fn sem_working_dir_com_repositorio_no_cwd_nao_recusa() {
        let tmp = tempfile::tempdir().expect("tempdir");
        for (marca, como_diretorio) in [
            (".git", true),
            // Worktree e submodulo: `.git` e ARQUIVO, e continua sendo
            // repositorio.
            (".git", false),
            (".hg", true),
            (".svn", true),
            (".jj", true),
        ] {
            let raiz = tmp.path().join(format!("{marca}-{como_diretorio}"));
            let fundo = raiz.join("src/tools");
            std::fs::create_dir_all(&fundo).expect("subdirs");
            if como_diretorio {
                std::fs::create_dir_all(raiz.join(marca)).expect("marca");
            } else {
                std::fs::write(raiz.join(marca), "gitdir: /outro/lugar\n").expect("marca");
            }

            // Tanto na raiz quanto la no fundo: a marca vale para a arvore.
            for dir in [&raiz, &fundo] {
                assert!(dentro_de_repositorio(dir), "{}", dir.display());
                let repo = RepoDir::ProcessoCwd(Some(dir.clone()));
                assert_eq!(
                    recusa_sem_repositorio(&repo, Duration::from_secs(15), true),
                    None,
                    "{marca} em {} foi recusado: e o caso do `garra chat` local",
                    dir.display()
                );
            }
        }
    }

    /// A sessao que ESCOLHEU um diretorio nunca passa pela recusa, tenha ele
    /// repositorio ou nao: quem escolheu decide o que ha la, e recusar
    /// mudaria o contrato de quem ja usa a tool assim.
    #[test]
    fn working_dir_da_sessao_nunca_e_recusado() {
        let tmp = dir_sem_repo();
        let repo = RepoDir::decidir(Some(&tmp.path().to_string_lossy()));
        assert!(matches!(repo, RepoDir::Sessao(_)), "{repo:?}");
        assert_eq!(
            recusa_sem_repositorio(&repo, Duration::from_secs(15), true),
            None
        );
    }

    /// Regressao de ponta a ponta do caso (a), pela `execute` de verdade: o
    /// processo de teste roda no diretorio do crate, dentro deste
    /// repositorio, e a sessao nao tem `working_dir` — exatamente a forma que
    /// a correcao nao pode quebrar.
    ///
    /// A agulha e a propria string literal desta linha: ela existe neste
    /// arquivo fonte, entao um resultado com `repo_search_tool.rs` prova que a
    /// busca rodou no repositorio herdado do CWD, e nao que a tool respondeu
    /// qualquer coisa.
    #[cfg(not(windows))]
    #[tokio::test]
    async fn regressao_1380_sem_working_dir_no_repo_do_cwd_ainda_busca() {
        let cwd = std::env::current_dir().expect("CWD do processo de teste");
        assert!(
            dentro_de_repositorio(&cwd),
            "este teste precisa rodar dentro do checkout ({}): e o caso (a) da #1380",
            cwd.display()
        );

        // A decisao em si nao depende de programa nenhum, e vale sempre.
        assert_eq!(
            recusa_sem_repositorio(&RepoDir::decidir(None), Duration::from_secs(15), true),
            None,
            "a sessao sem working_dir dentro do checkout nao pode ser recusada"
        );
        // A busca de verdade so com o `rg`: sem ele o fallback e um `grep -r`
        // que nao le `.gitignore` e desceria em `target/`, o que torna o teste
        // lento por um motivo que nao e o desta issue.
        if !existe_no_path("rg") {
            eprintln!("rg ausente no PATH: parte da execucao deste teste foi pulada");
            return;
        }

        let tool = RepoSearchTool::new(Some(20), None);
        let saida = tool
            .execute(
                &ctx_sem_working_dir(),
                serde_json::json!({
                    "query": "agulha_da_regressao_1380",
                    "file_pattern": "*.rs",
                    "max_results": 5,
                    "context_lines": 0,
                }),
            )
            .await
            .expect("executa");

        assert!(!saida.is_error, "{}", saida.content);
        assert!(
            !saida.content.contains("No active repository"),
            "a recusa da #1380 pegou o caso legitimo: {}",
            saida.content
        );
        assert!(
            saida.content.contains("repo_search_tool.rs"),
            "a busca tinha de achar a agulha neste arquivo: {}",
            saida.content
        );
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
