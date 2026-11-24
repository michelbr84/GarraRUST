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
    let mut args: Vec<String> = vec!["diff".to_string()];
    // #1272 S3: `--no-ext-diff` e `--no-textconv` — nenhum programa da config
    // do repositorio roda para montar o diff.
    args.extend(
        crate::git_endurecido::OPCOES_DO_DIFF
            .iter()
            .map(|s| s.to_string()),
    );
    args.push(format!("-U{context_lines}"));

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
    /// #1225 S2: `agent.sandbox`. Default `off` = host, como sempre.
    sandbox: crate::sandbox::SandboxPolicy,
}

impl GitDiffTool {
    /// Cria uma nova instância do GitDiffTool
    pub fn new(timeout_secs: Option<u64>, max_lines: Option<usize>) -> Self {
        Self {
            timeout: Duration::from_secs(timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS)),
            max_lines: max_lines.unwrap_or(DEFAULT_MAX_LINES),
            sandbox: crate::sandbox::SandboxPolicy::default(),
        }
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
        // #1272 S3: `-c core.fsmonitor=false`, bare implicito recusado, sem
        // hooks e todo `filter.<drv>` da config anulado — antes do subcomando.
        let prefixo = crate::git_endurecido::prefixo(repo.cwd_do_git(), self.timeout)
            .await
            .map_err(Error::Agent)?;
        // #1258: o git roda no `working_dir` da sessão quando há um. Sem ele
        // o CWD do processo é mantido de propósito, e quem formata a resposta
        // nomeia o diretório (ver [`RepoDir`]).
        //
        // #1225 S2: o spawn passa por `sandbox_spawn::executar` — env do filho
        // reduzido a allowlist (#1075 R3: um .gitconfig plantado com
        // diff.external é execução arbitrária e não pode carregar segredos do
        // pai), stdin nulo (#1269) e, com `agent.sandbox` aplicável, o git
        // roda no container.
        let mut argv: Vec<String> = prefixo;
        argv.extend(args.iter().cloned());
        let resultado = crate::sandbox_spawn::executar(
            &self.sandbox,
            crate::sandbox_spawn::Pedido {
                tool: "git_diff",
                programa: "git",
                args: &argv,
                cwd: repo.cwd_do_git(),
                env: crate::git_endurecido::ENV,
                timeout: self.timeout,
            },
        )
        .await;

        match resultado {
            crate::sandbox_spawn::Desfecho::Saida(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                // #1909: o stderr do git nomeia caminhos absolutos do host
                // nas recusas; num turno restrito eles viram o marcador.
                let stderr = crate::git_endurecido::stderr_para_o_turno(
                    &String::from_utf8_lossy(&output.stderr),
                    repo.cwd_do_git(),
                );

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
            crate::sandbox_spawn::Desfecho::NaoExecutou(e) => {
                Err(Error::Agent(format!("falha ao executar git: {e}")))
            }
            crate::sandbox_spawn::Desfecho::Recusado(motivo) => Err(Error::Agent(motivo)),
            crate::sandbox_spawn::Desfecho::Timeout => Err(Error::Agent(format!(
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

        // #1909: o intervalo exato entra na resposta DEPOIS do filtro e do
        // corte de linhas, para nunca ser a linha que o limite esconde. So
        // chega aqui com as revisoes ja validadas por `git_diff_args`.
        let intervalo = self.intervalo_do_diff(repo, from_commit, to_commit).await;

        Ok(format!("{intervalo}\n\n{limited}"))
    }

    /// De QUAIS revisoes o `diff` desta chamada fala (#1909).
    ///
    /// O `status` ja nomeava a revisao do checkout (#1934); o `diff` devolvia
    /// as mudancas sem dizer entre o que e o que. `main..feature` e nome
    /// movel: o mesmo pedido, feito duas vezes, pode falar de commits
    /// diferentes, e quem le o resultado remotamente nao tinha como saber de
    /// qual. A issue pede "revisao exata, estado do checkout e intervalo do
    /// diff" sem shell generico — este e o terceiro.
    ///
    /// Cada ponta e resolvida por um `rev-parse --verify` proprio, e so vale
    /// quando a saida e UMA revisao ([`revisao_unica_do_output`]): resolver as
    /// duas numa chamada so deixaria uma ponta como `a..b` deslocar a posicao
    /// da outra e rotular um SHA com o nome errado.
    async fn intervalo_do_diff(
        &self,
        repo: &RepoDir,
        from_commit: Option<&str>,
        to_commit: Option<&str>,
    ) -> String {
        match (from_commit, to_commit) {
            (Some(from), Some(to)) => {
                let (rev_de, rev_ate) = (format!("{from}^{{commit}}"), format!("{to}^{{commit}}"));
                let (de, ate) = tokio::join!(
                    self.revisao_de(repo, &rev_de),
                    self.revisao_de(repo, &rev_ate),
                );
                linha_do_intervalo_entre_commits(from, de.as_deref(), to, ate.as_deref())
            }
            // Sem intervalo o `git diff` compara o indice com a arvore de
            // trabalho. O `HEAD` nao e uma das pontas, mas e o que diz EM QUAL
            // checkout essa comparacao foi feita.
            _ => linha_do_intervalo_da_arvore(self.revisao_exata(repo).await.as_deref()),
        }
    }

    /// Obtém o status do repositório de `repo`
    async fn get_status(&self, repo: &RepoDir) -> Result<String> {
        let mut args: Vec<String> = vec![
            "status".to_string(),
            "--porcelain".to_string(),
            "-b".to_string(),
        ];
        // #1272 S3: sem descer em submodulo (config propria, filtros dele).
        args.extend(
            crate::git_endurecido::OPCOES_DO_STATUS
                .iter()
                .map(|s| s.to_string()),
        );

        let output = self.run_git_command(&args, repo).await?;

        // #1909: a revisao exata entra no status. Sem ela "Branch: main" nao
        // diz QUAL main o operador esta vendo, e o fluxo de revisao remota
        // ficava sem identidade de checkout (a issue pede "revisao exata,
        // estado do checkout e intervalo do diff" sem shell generico).
        let revisao = self.revisao_exata(repo).await;

        // Formata o status de forma mais legível
        let formatted = format_status(&output);
        let com_revisao = anexar_revisao(&formatted, revisao.as_deref());

        // Aplica filtros de segurança
        let filtered = self.filter_secrets(&com_revisao);
        let limited = self.limit_lines(&filtered);

        Ok(limited)
    }

    /// O SHA completo do `HEAD` do mesmo repositório do `status` (#1909).
    ///
    /// `None` quando o git não devolve revisão legível — um repositório
    /// recém-criado, sem commit, não pode derrubar o `status` inteiro: o
    /// estado do checkout (branch, arquivos) continua sendo a resposta útil.
    async fn revisao_exata(&self, repo: &RepoDir) -> Option<String> {
        self.revisao_de(repo, "HEAD").await
    }

    /// O SHA completo de UMA revisao, ou `None` quando o git nao a resolve.
    ///
    /// Quem chama com valor vindo do modelo ja recusou o que comeca com `-`
    /// (`git_diff_args`), e mesmo assim o que volta so vira "revisao" quando a
    /// saida e a de um `--verify` que deu certo ([`revisao_unica_do_output`]).
    async fn revisao_de(&self, repo: &RepoDir, revisao: &str) -> Option<String> {
        // Uma revisao so nao contem `..`: nome de ref nao pode ter (regra do
        // `git check-ref-format`), e `a..b` / `a...b` sao INTERVALOS. Nem
        // chega ao git — ver [`revisao_unica_do_output`] para o porque.
        if revisao.contains("..") {
            return None;
        }
        let args = vec![
            "rev-parse".to_string(),
            "--verify".to_string(),
            revisao.to_string(),
        ];
        let saida = self.run_git_command(&args, repo).await.ok()?;
        revisao_unica_do_output(&saida).map(str::to_string)
    }
}

/// Separa o que o git imprimiu no stdout do bloco `STDERR:` que
/// `run_git_command` anexa. O cabecalho so conta no comeco de uma linha.
fn separar_stderr(output: &str) -> (&str, Option<&str>) {
    match output.find("STDERR:\n") {
        Some(i) if i == 0 || output[..i].ends_with('\n') => (&output[..i], Some(&output[i..])),
        _ => (output, None),
    }
}

/// O SHA de um `rev-parse --verify <revisao>` que DEU CERTO, e so dele.
///
/// `run_git_command` devolve `Ok` com o codigo de saida descartado, entao o
/// sucesso tem de ser lido da forma da saida. E a forma engana: para um
/// intervalo (`a..b`), o git 2.43 imprime os dois SHAs no stdout e SO DEPOIS
/// morre com `fatal: Needed a single revision` — a primeira linha tem cara de
/// SHA e e a revisao ERRADA (a ponta `b`, nao a pedida). Por isso: exatamente
/// uma linha no stdout, com forma de SHA, e nenhum `fatal:` no stderr. Aviso
/// que nao e `fatal:` (o pull do docker no sandbox) nao derruba a leitura.
fn revisao_unica_do_output(saida: &str) -> Option<&str> {
    let (stdout, stderr) = separar_stderr(saida);
    if stderr.is_some_and(|bloco| bloco.lines().any(|l| l.trim_start().starts_with("fatal:"))) {
        return None;
    }
    let mut linhas = stdout.lines().filter(|l| !l.trim().is_empty());
    let unica = linhas.next()?;
    if linhas.next().is_some() {
        return None;
    }
    revisao_do_output(unica)
}

/// O que dizer no lugar do SHA de uma ponta que o git nao resolveu.
const PONTA_NAO_RESOLVIDA: &str = "revisão não resolvida";

/// O nome de uma revisao como o modelo o escreveu, pronto para UMA linha.
///
/// O valor vem da tool call. Caractere de controle (uma quebra de linha,
/// sobretudo) deixaria o modelo escrever linhas inteiras na resposta com a
/// cara de saida da tool; vira `?`. O tamanho e cortado pelo mesmo motivo que
/// qualquer eco de entrada: a linha e contexto, nao transporte.
fn nome_da_revisao(revisao: &str) -> String {
    const TETO: usize = 120;
    let limpo: String = revisao
        .chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .take(TETO)
        .collect();
    if revisao.chars().count() > TETO {
        format!("{limpo}…")
    } else {
        limpo
    }
}

/// A linha do intervalo de um diff entre commits: cada ponta com o nome que
/// foi pedido e o SHA que ele valia na hora — ou a falta dele, dita.
fn linha_do_intervalo_entre_commits(
    from: &str,
    sha_de: Option<&str>,
    to: &str,
    sha_ate: Option<&str>,
) -> String {
    format!(
        "Intervalo: {} ({}) .. {} ({})",
        nome_da_revisao(from),
        sha_de.unwrap_or(PONTA_NAO_RESOLVIDA),
        nome_da_revisao(to),
        sha_ate.unwrap_or(PONTA_NAO_RESOLVIDA),
    )
}

/// A linha do intervalo de um diff sem commits: indice contra arvore de
/// trabalho, com o `HEAD` do checkout em que isso foi medido.
fn linha_do_intervalo_da_arvore(head: Option<&str>) -> String {
    match head {
        Some(sha) => format!(
            "Intervalo: índice .. árvore de trabalho (mudanças ainda fora do índice; HEAD em {sha})"
        ),
        None => "Intervalo: índice .. árvore de trabalho (mudanças ainda fora do índice; \
                 HEAD indisponível — repositório sem commit legível)"
            .to_string(),
    }
}

/// O SHA que o `rev-parse --verify HEAD` devolveu, se a saída tiver um.
///
/// `run_git_command` junta stdout e stderr e devolve `Ok` mesmo quando o git
/// falha — num repositório sem commit o stdout é vazio e o combinado começa em
/// `STDERR:` com `fatal: Needed a single revision` (verificado no git 2.43).
/// Por isso a validação é a FORMA (40 a 64 dígitos hexadecimais — SHA-1 e
/// SHA-256), não o código de saída: texto qualquer do git nunca vira
/// "revisão" na resposta.
fn revisao_do_output(saida: &str) -> Option<&str> {
    let primeira = saida.lines().next()?.trim();
    let parece_sha =
        (40..=64).contains(&primeira.len()) && primeira.bytes().all(|b| b.is_ascii_hexdigit());
    parece_sha.then_some(primeira)
}

/// A linha da revisão exata, com o motivo explícito quando não há uma.
fn linha_da_revisao(revisao: Option<&str>) -> String {
    match revisao {
        Some(sha) => format!("Revisão: {sha}"),
        None => "Revisão: (indisponível — repositório sem commit legível)".to_string(),
    }
}

/// Insere a linha da revisão logo depois de `Branch:`, que é onde o operador
/// procura a identidade do checkout. Sem linha de branch (git recusado, dir
/// que não é repositório) a revisão vai no topo, e o texto do git que já
/// estava na resposta fica intacto embaixo.
fn anexar_revisao(status: &str, revisao: Option<&str>) -> String {
    let mut linhas: Vec<String> = status.lines().map(str::to_string).collect();
    let posicao = linhas
        .iter()
        .position(|l| l.starts_with("Branch: "))
        .map_or(0, |i| i + 1);
    linhas.insert(posicao, linha_da_revisao(revisao));
    let mut saida = linhas.join("\n");
    saida.push('\n');
    saida
}

/// Formata a saida de `git status --porcelain -b` (mais o bloco `STDERR:`
/// que `run_git_command` anexa). Nunca entra em panico: linha curta, vazia
/// ou com caractere multibyte no corte sai crua em vez de ser fatiada — o
/// separador em branco antes do `STDERR:` e o aviso de pull do docker no
/// sandbox tornavam o `&line[..2]` antigo um panic de rotina (review da
/// #1225, SANDBOX-7). O que vem depois de `STDERR:` e anexado sem parse.
fn format_status(output: &str) -> String {
    let (stdout, stderr) = separar_stderr(output);
    let mut result = String::new();

    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Some(stripped) = line.strip_prefix("## ") {
            result.push_str(&format!("Branch: {}\n", stripped));
            continue;
        }

        let (Some(status), Some(file)) = (line.get(..2), line.get(3..)) else {
            result.push_str(line);
            result.push('\n');
            continue;
        };

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
    if let Some(bloco) = stderr {
        result.push('\n');
        result.push_str(bloco);
    }

    result
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
           e o intervalo exato comparado (SHA completo de cada ponta)\n\
         - status: retorna arquivos modificados, adicionados, deletados, branch atual\n\
           e a revisão exata do checkout (SHA completo do HEAD)\n\
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
        // #1296: entrada malformada do modelo é observação soft, não erro do
        // turno — o valor desconhecido já era soft; a ausência agora também.
        let operation = match input.get("operation").and_then(|v| v.as_str()) {
            Some(op) => op,
            None => {
                return Ok(super::parametro_ausente(
                    "git_diff",
                    r#"{"operation": "diff" | "status"}"#,
                    "operation",
                ));
            }
        };

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

    /// Review da #1225 (SANDBOX-7): nenhuma entrada derruba o
    /// `format_status` — linha vazia, curta, multibyte no corte, o bloco
    /// `STDERR:` que `run_git_command` anexa, e combinacoes geradas.
    #[test]
    fn format_status_nao_entra_em_panico_com_linha_curta_ou_estranha() {
        let pecas = [
            "",
            "\n",
            "a",
            "ab",
            "abc",
            "é",
            "aé",
            "éé",
            "a\u{1F600}b",
            " M",
            " M ",
            "??",
            "## main",
            "##",
            "STDERR:",
            "STDERR:\n",
            "x\n\n",
            "\r\n",
            " M arquivo.rs",
        ];
        for a in pecas {
            for b in pecas {
                let entrada = format!("{a}\n{b}");
                let _ = format_status(&entrada);
                let _ = format_status(&format!("{a}{b}"));
            }
        }
        // O caso real: stdout com `\n` final + o separador + stderr do docker.
        let real = "## main\n M a.rs\n\nSTDERR:\nUnable to find image 'x' locally\nab\n";
        let saida = format_status(real);
        assert!(saida.contains("Branch: main"), "{saida}");
        assert!(saida.contains("Modificado: a.rs"), "{saida}");
        assert!(saida.contains("STDERR:\nUnable to find image"), "{saida}");
        assert!(!saida.contains("Desconhecido: nable"), "{saida}");
    }
    use crate::tools::repo_dir::{contexto_de_teste as ctx, repo_git_temporario};

    // ─── #1909: revisao exata no status ────────────────────────────────────

    /// So a FORMA de SHA passa por `revisao_do_output`. Num repositorio sem
    /// commit o git imprime `HEAD` e o `fatal:` atras de `STDERR:`; nenhum dos
    /// dois pode virar "Revisão:" na resposta ao operador.
    #[test]
    fn revisao_do_output_so_aceita_sha() {
        let sha1 = "5483327950f3aed7a5897cb5eefcf782e46d18e8";
        let sha256 = "b".repeat(64);
        assert_eq!(revisao_do_output(&format!("{sha1}\n")), Some(sha1));
        assert_eq!(revisao_do_output(&sha256), Some(sha256.as_str()));
        // A forma REAL do repo sem commit: stdout vazio, `STDERR:` primeiro.
        assert_eq!(
            revisao_do_output("STDERR:\nfatal: Needed a single revision\n"),
            None
        );
        assert_eq!(revisao_do_output(""), None);
        assert_eq!(revisao_do_output(&"a".repeat(39)), None);
        assert_eq!(revisao_do_output(&"a".repeat(65)), None);
        assert_eq!(revisao_do_output(&"g".repeat(40)), None);
    }

    /// A revisao entra logo depois de `Branch:` — e, quando o git recusou o
    /// diretorio (nao ha linha de branch), no topo, sem engolir o texto que o
    /// git devolveu.
    #[test]
    fn revisao_entra_depois_do_branch() {
        let saida = anexar_revisao("Branch: main\nModificado: a.rs\n", Some("abc123"));
        let linhas: Vec<&str> = saida.lines().collect();
        assert_eq!(
            linhas,
            vec!["Branch: main", "Revisão: abc123", "Modificado: a.rs"]
        );

        let recusado =
            "Working tree limpo (nenhuma modificação)\n\nSTDERR:\nfatal: not a git repository\n";
        let saida = anexar_revisao(recusado, None);
        // O texto do git fica INTEIRO embaixo: nem o cabeçalho `STDERR:` nem
        // a linha em branco podem ser engolidos pela inserção.
        assert_eq!(
            saida.lines().collect::<Vec<_>>(),
            vec![
                "Revisão: (indisponível — repositório sem commit legível)",
                "Working tree limpo (nenhuma modificação)",
                "",
                "STDERR:",
                "fatal: not a git repository",
            ]
        );
    }

    /// O caminho real do agente: dois repositorios temporarios, cada um com a
    /// SUA revisao, e o status nomeia o SHA que `git rev-parse HEAD` le
    /// NAQUELE diretorio. **Mutacao**: apontar `revisao_exata` para o CWD do
    /// processo em vez de `repo.cwd_do_git()` (o mesmo defeito que a #1258
    /// fechou no diff) e o SHA deixa de casar.
    #[tokio::test]
    async fn status_traz_a_revisao_exata_do_checkout() {
        let tool = GitDiffTool::new(Some(15), Some(500));
        for marcador in ["alvo-da-revisao-a", "alvo-da-revisao-b"] {
            let repo = repo_git_temporario(marcador, "ramo-da-revisao");
            let saida_git = std::process::Command::new("git")
                .current_dir(repo.path())
                .args(["rev-parse", "HEAD"])
                .output()
                .expect("git");
            let sha = String::from_utf8_lossy(&saida_git.stdout)
                .trim()
                .to_string();
            // SHA-1 por padrão, SHA-256 numa máquina com GIT_DEFAULT_HASH: a
            // produção aceita as duas formas, o teste também.
            assert!(
                (40..=64).contains(&sha.len()),
                "revisao fora das formas aceitas: {sha}"
            );

            let wd = repo.path().to_string_lossy().into_owned();
            let saida = tool
                .execute(
                    &ctx(Some(&wd)),
                    serde_json::json!({ "operation": "status" }),
                )
                .await
                .expect("execute");
            assert!(!saida.is_error, "{}", saida.content);
            assert!(
                saida.content.contains(&format!("Revisão: {sha}")),
                "o status nao nomeou a revisao {sha}:\n{}",
                saida.content
            );
            assert!(
                saida.content.contains("Branch: ramo-da-revisao"),
                "{}",
                saida.content
            );
        }
    }

    /// Repositorio sem commit nenhum: o `status` continua respondendo (branch
    /// e arquivos sao a informacao util) e diz por que a revisao falta, em vez
    /// de a tool inteira falhar.
    #[tokio::test]
    async fn status_sem_commit_diz_que_a_revisao_falta() {
        let dir = tempfile::tempdir().expect("tmp");
        let ok = std::process::Command::new("git")
            .current_dir(dir.path())
            .args(["init", "-b", "ramo-vazio", "-q", "."])
            .status()
            .expect("git")
            .success();
        assert!(ok);

        let tool = GitDiffTool::new(Some(15), Some(500));
        let wd = dir.path().to_string_lossy().into_owned();
        let saida = tool
            .execute(
                &ctx(Some(&wd)),
                serde_json::json!({ "operation": "status" }),
            )
            .await
            .expect("execute");
        assert!(!saida.is_error, "{}", saida.content);
        assert!(
            saida.content.contains("Revisão: (indisponível"),
            "{}",
            saida.content
        );
        // O git nomeia o ramo sem commit como "No commits yet on <ramo>";
        // o que importa aqui e a revisao faltar com motivo, sem derrubar
        // branch nem estado.
        assert!(saida.content.contains("ramo-vazio"), "{}", saida.content);
    }

    // ─── #1909: intervalo exato no diff ────────────────────────────────────

    /// `git rev-parse <revisao>` no repositorio do teste — a fonte
    /// independente contra a qual a resposta da tool e conferida.
    fn sha_de(dir: &std::path::Path, revisao: &str) -> String {
        let saida = std::process::Command::new("git")
            .current_dir(dir)
            .args(["rev-parse", "--verify", revisao])
            .output()
            .expect("git");
        assert!(saida.status.success(), "rev-parse {revisao} falhou");
        String::from_utf8_lossy(&saida.stdout).trim().to_string()
    }

    /// Commita o que estiver sujo na arvore, com a mesma identidade por `-c`
    /// do `repo_git_temporario` (CI nao tem `user.name`).
    fn commitar_tudo(dir: &std::path::Path, mensagem: &str) {
        let ok = std::process::Command::new("git")
            .current_dir(dir)
            .args([
                "-c",
                "user.email=test@test",
                "-c",
                "user.name=t",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "-am",
                mensagem,
            ])
            .status()
            .expect("git")
            .success();
        assert!(ok, "commit de teste falhou");
    }

    /// A saida REAL do git 2.43 para `rev-parse --verify 'a..b^{commit}'`:
    /// dois SHAs no stdout e o `fatal:` depois. A primeira linha tem forma de
    /// SHA — e e a ponta errada. So uma linha unica, sem `fatal:`, e revisao.
    #[test]
    fn revisao_unica_recusa_saida_de_intervalo_e_de_falha() {
        let a = "9da1032b668ecfc3312f13e3556c69f9f0a71c99";
        let b = "c50aff60f75c1997a17800949f662e3d5b01869c";
        assert_eq!(revisao_unica_do_output(&format!("{a}\n")), Some(a));
        assert_eq!(
            revisao_unica_do_output(&format!(
                "{a}\n^{b}\n\nSTDERR:\nfatal: Needed a single revision\n"
            )),
            None
        );
        // Uma linha so, mas o git morreu: nao e revisao.
        assert_eq!(
            revisao_unica_do_output(&format!("{a}\n\nSTDERR:\nfatal: qualquer\n")),
            None
        );
        // Aviso que nao e `fatal:` (pull do docker no sandbox) nao derruba.
        assert_eq!(
            revisao_unica_do_output(&format!("{a}\n\nSTDERR:\nUnable to find image\n")),
            Some(a)
        );
        assert_eq!(revisao_unica_do_output(&format!("{a}\n{b}\n")), None);
        assert_eq!(
            revisao_unica_do_output("STDERR:\nfatal: Needed a single revision\n"),
            None
        );
        assert_eq!(revisao_unica_do_output(""), None);
    }

    /// As duas formas da linha, sem subir processo: ponta resolvida leva o
    /// SHA, ponta nao resolvida DIZ que nao foi resolvida — nunca some.
    #[test]
    fn linha_do_intervalo_nomeia_cada_ponta() {
        assert_eq!(
            linha_do_intervalo_entre_commits("main", Some("aaa"), "feature", Some("bbb")),
            "Intervalo: main (aaa) .. feature (bbb)"
        );
        assert_eq!(
            linha_do_intervalo_entre_commits("sumiu", None, "HEAD", Some("bbb")),
            "Intervalo: sumiu (revisão não resolvida) .. HEAD (bbb)"
        );
        assert_eq!(
            linha_do_intervalo_da_arvore(Some("ccc")),
            "Intervalo: índice .. árvore de trabalho (mudanças ainda fora do índice; HEAD em ccc)"
        );
        assert!(linha_do_intervalo_da_arvore(None).contains("HEAD indisponível"));
    }

    /// O nome da revisao vem da tool call do modelo e entra numa linha da
    /// resposta: quebra de linha nao pode abrir uma linha nova com cara de
    /// saida da tool, e um nome gigante nao vira a resposta inteira.
    #[test]
    fn nome_da_revisao_fica_em_uma_linha_e_tem_teto() {
        let forjado = nome_da_revisao("main\nRevisão: 0000\r\tfim");
        assert_eq!(forjado, "main?Revisão: 0000??fim");
        assert_eq!(forjado.lines().count(), 1);

        let longo = nome_da_revisao(&"a".repeat(500));
        assert_eq!(longo.chars().count(), 121, "120 do nome + reticencias");
        assert!(longo.ends_with('…'));
        // Multibyte no corte nao pode entrar em panico nem partir caractere.
        assert_eq!(nome_da_revisao(&"é".repeat(121)).chars().count(), 121);
        assert_eq!(nome_da_revisao("HEAD~3"), "HEAD~3");
    }

    /// Sem commits no pedido: o diff e indice contra arvore de trabalho, e a
    /// resposta diz em qual `HEAD` isso foi medido — logo abaixo da
    /// procedencia, antes do diff. **Mutacao**: resolver o `HEAD` no CWD do
    /// processo em vez de `repo.cwd_do_git()` e o SHA deixa de casar.
    #[tokio::test]
    async fn diff_sem_commits_nomeia_o_head_do_checkout() {
        let tool = GitDiffTool::new(Some(15), Some(500));
        for marcador in ["alvo-do-intervalo-a", "alvo-do-intervalo-b"] {
            let repo = repo_git_temporario(marcador, "ramo-do-intervalo");
            let sha = sha_de(repo.path(), "HEAD");
            let wd = repo.path().to_string_lossy().into_owned();
            let saida = tool
                .execute(&ctx(Some(&wd)), serde_json::json!({ "operation": "diff" }))
                .await
                .expect("execute");
            assert!(!saida.is_error, "{}", saida.content);
            let linhas: Vec<&str> = saida.content.lines().collect();
            assert_eq!(linhas[0], "Repositório: o working_dir da sessão");
            assert_eq!(
                linhas[2],
                format!(
                    "Intervalo: índice .. árvore de trabalho (mudanças ainda fora do índice; \
                     HEAD em {sha})"
                ),
                "{}",
                saida.content
            );
            // O diff em si continua vindo, depois da linha do intervalo.
            assert!(saida.content.contains(marcador), "{}", saida.content);
        }
    }

    /// Diff entre commits: cada ponta sai com o nome pedido e o SHA que o
    /// `git rev-parse` le NAQUELE repositorio. O nome do ramo e movel — e
    /// exatamente por isso que a resposta precisa do SHA.
    #[tokio::test]
    async fn diff_entre_commits_nomeia_o_sha_de_cada_ponta() {
        let repo = repo_git_temporario("alvo-entre-commits", "ramo-entre-commits");
        commitar_tudo(repo.path(), "segundo");
        let de = sha_de(repo.path(), "HEAD~1");
        let ate = sha_de(repo.path(), "HEAD");
        assert_ne!(de, ate);

        let tool = GitDiffTool::new(Some(15), Some(500));
        let wd = repo.path().to_string_lossy().into_owned();
        let saida = tool
            .execute(
                &ctx(Some(&wd)),
                serde_json::json!({
                    "operation": "diff",
                    "from_commit": "HEAD~1",
                    "to_commit": "ramo-entre-commits",
                }),
            )
            .await
            .expect("execute");
        assert!(!saida.is_error, "{}", saida.content);
        assert_eq!(
            saida.content.lines().nth(2).unwrap_or_default(),
            format!("Intervalo: HEAD~1 ({de}) .. ramo-entre-commits ({ate})"),
            "{}",
            saida.content
        );
        // E o diff desse intervalo e o do segundo commit.
        assert!(
            saida.content.contains("+linha alterada"),
            "{}",
            saida.content
        );
    }

    /// Ponta que o git nao resolve: a linha DIZ isso, em vez de sumir ou de
    /// emprestar um SHA que nao e dela. O caso `a..b` numa ponta so pegou um
    /// defeito real na primeira versao: mesmo com `--verify` o git imprime os
    /// SHAs do intervalo antes de falhar, e a ponta saia rotulada com o SHA
    /// de `HEAD` (ver `revisao_unica_do_output`).
    #[tokio::test]
    async fn ponta_nao_resolvida_e_dita_sem_contaminar_a_outra() {
        let repo = repo_git_temporario("alvo-ponta-ruim", "ramo-ponta-ruim");
        commitar_tudo(repo.path(), "segundo");
        let head = sha_de(repo.path(), "HEAD");
        let tool = GitDiffTool::new(Some(15), Some(500));
        let wd = repo.path().to_string_lossy().into_owned();

        for ponta_ruim in ["ramo-que-nao-existe", "HEAD~1..HEAD", "HEAD~1...HEAD"] {
            let saida = tool
                .execute(
                    &ctx(Some(&wd)),
                    serde_json::json!({
                        "operation": "diff",
                        "from_commit": ponta_ruim,
                        "to_commit": "HEAD",
                    }),
                )
                .await
                .expect("execute");
            assert_eq!(
                saida.content.lines().nth(2).unwrap_or_default(),
                format!("Intervalo: {ponta_ruim} (revisão não resolvida) .. HEAD ({head})"),
                "{}",
                saida.content
            );
        }
    }

    /// Repositorio sem commit: o diff continua respondendo e a linha do
    /// intervalo diz por que o `HEAD` falta.
    #[tokio::test]
    async fn diff_sem_commit_diz_que_o_head_falta() {
        let dir = tempfile::tempdir().expect("tmp");
        let ok = std::process::Command::new("git")
            .current_dir(dir.path())
            .args(["init", "-b", "ramo-vazio", "-q", "."])
            .status()
            .expect("git")
            .success();
        assert!(ok);

        let tool = GitDiffTool::new(Some(15), Some(500));
        let wd = dir.path().to_string_lossy().into_owned();
        let saida = tool
            .execute(&ctx(Some(&wd)), serde_json::json!({ "operation": "diff" }))
            .await
            .expect("execute");
        assert!(!saida.is_error, "{}", saida.content);
        assert!(
            saida
                .content
                .lines()
                .nth(2)
                .unwrap_or_default()
                .contains("HEAD indisponível"),
            "{}",
            saida.content
        );
    }

    /// O limite de linhas corta o DIFF, nunca a linha do intervalo: ela entra
    /// depois do corte.
    #[tokio::test]
    async fn limite_de_linhas_nao_esconde_o_intervalo() {
        let repo = repo_git_temporario("alvo-do-limite", "ramo-do-limite");
        let tool = GitDiffTool::new(Some(15), Some(1));
        let wd = repo.path().to_string_lossy().into_owned();
        let saida = tool
            .execute(&ctx(Some(&wd)), serde_json::json!({ "operation": "diff" }))
            .await
            .expect("execute");
        assert!(
            saida
                .content
                .contains("Intervalo: índice .. árvore de trabalho"),
            "{}",
            saida.content
        );
        assert!(
            saida.content.contains("ocultas por limite"),
            "{}",
            saida.content
        );
    }

    // ─── #1272 S3: config plantada nao executa nada no host ────────────────

    /// Um repo com `core.fsmonitor`, `diff.x.textconv` e `filter.x.clean`
    /// plantados: `diff` e `status` pela tool nao criam marcador nenhum e o
    /// diff continua vindo. Gemeo: o argv ANTIGO (`git diff --no-ext-diff`)
    /// cria os tres — prova que o teste nao passa por vacuidade e prende as
    /// flags.
    #[cfg(unix)]
    #[tokio::test]
    async fn config_plantada_no_repo_nao_roda_programa_nenhum() {
        let repo = repo_git_temporario("alvo-1272", "ramo-1272");
        let marcas = tempfile::tempdir().expect("tmp");
        let [fsm, tc, clean] =
            crate::git_endurecido::planta_programas_no_repo(repo.path(), marcas.path());
        let tool = GitDiffTool::new(Some(15), Some(500));
        let wd = repo.path().to_string_lossy().into_owned();
        for op in ["diff", "status"] {
            let saida = tool
                .execute(&ctx(Some(&wd)), serde_json::json!({"operation": op}))
                .await
                .expect("execute");
            assert!(!saida.is_error, "{op}: {}", saida.content);
            if op == "diff" {
                assert!(
                    saida.content.contains("linha alterada"),
                    "{}",
                    saida.content
                );
            }
            for m in [&fsm, &tc, &clean] {
                assert!(!m.exists(), "{op} executou {}", m.display());
            }
        }

        // Gemeo: sem o endurecimento, o mesmo repo executa os tres.
        let ok = std::process::Command::new("git")
            .current_dir(repo.path())
            .args(["diff", "--no-ext-diff"])
            .output()
            .expect("git")
            .status
            .success();
        assert!(ok);
        for m in [&fsm, &tc, &clean] {
            assert!(m.exists(), "o gemeo nao reproduziu {}", m.display());
        }
    }

    /// Um submodulo cuja config PROPRIA declara `filter.evil.clean`, com o
    /// superprojeto em `diff.submodule=diff`: `diff` e `status` pela tool nao
    /// descem nele, entao o filtro nao roda. Gemeo: o prefixo antigo (sem as
    /// chaves de submodulo) roda o filtro — prova que o teste nao e vazio.
    #[cfg(unix)]
    #[tokio::test]
    async fn filtro_declarado_em_submodulo_nao_roda() {
        let repo = repo_git_temporario("alvo-sub", "ramo-sub");
        let marcas = tempfile::tempdir().expect("tmp");
        let marca = crate::git_endurecido::planta_filtro_em_submodulo(repo.path(), marcas.path());
        let tool = GitDiffTool::new(Some(15), Some(500));
        let wd = repo.path().to_string_lossy().into_owned();
        for op in ["diff", "status"] {
            let saida = tool
                .execute(&ctx(Some(&wd)), serde_json::json!({"operation": op}))
                .await
                .expect("execute");
            assert!(!saida.is_error, "{op}: {}", saida.content);
            assert!(!marca.exists(), "{op} rodou o filtro do submodulo");
        }

        // Gemeo: o endurecimento de antes (fsmonitor/bare/hooks, sem as
        // chaves de submodulo) roda o filtro do submodulo.
        let ok = std::process::Command::new("git")
            .current_dir(repo.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .args([
                "-c",
                "core.fsmonitor=false",
                "-c",
                "safe.bareRepository=explicit",
                "-c",
                "core.hooksPath=/dev/null",
                "diff",
                "--no-ext-diff",
                "--no-textconv",
            ])
            .output()
            .expect("git")
            .status
            .success();
        assert!(ok);
        assert!(
            marca.exists(),
            "o gemeo nao reproduziu o filtro do submodulo"
        );
    }

    /// `safe.bareRepository=explicit`: um repositorio bare implicito como
    /// working_dir e recusado pelo git em vez de lido.
    #[tokio::test]
    async fn repositorio_bare_implicito_e_recusado() {
        let dir = tempfile::tempdir().expect("tmp");
        let ok = std::process::Command::new("git")
            .current_dir(dir.path())
            .args(["init", "--bare", "-q", "."])
            .status()
            .expect("git")
            .success();
        assert!(ok);
        let tool = GitDiffTool::new(Some(15), Some(500));
        let wd = dir.path().to_string_lossy().into_owned();
        let saida = tool
            .execute(&ctx(Some(&wd)), serde_json::json!({"operation": "status"}))
            .await
            .expect("execute");
        // O git recusa o diretorio (a tool devolve a saida de erro dele).
        assert!(
            saida.content.contains("safe.bareRepository"),
            "bare implicito foi lido: {}",
            saida.content
        );
    }

    /// #1909: a recusa do git cita o diretorio por extenso, e ela sai no
    /// corpo da resposta. Num turno restrito o caminho do host nao sai — a
    /// causa (`safe.bareRepository`) continua dita; no aberto, sai como antes.
    ///
    /// So unix, como os vizinhos que dependem da forma do caminho: no Windows
    /// o git imprime `C:/...` e o `canonicalize` devolve a forma verbatim, entao
    /// comparar com o caminho do tempdir nao provaria nada. A logica de
    /// redacao (incluindo as formas do Windows) e coberta, em toda
    /// plataforma, pelos testes puros de `git_endurecido`.
    #[cfg(unix)]
    #[tokio::test]
    async fn recusa_do_git_no_turno_restrito_nao_nomeia_o_host() {
        let dir = tempfile::tempdir().expect("tmp");
        let ok = std::process::Command::new("git")
            .current_dir(dir.path())
            .args(["init", "--bare", "-q", "."])
            .status()
            .expect("git")
            .success();
        assert!(ok);
        let tool = GitDiffTool::new(Some(15), Some(500));
        let wd = dir.path().to_string_lossy().into_owned();
        // O git imprime o caminho canonico (no macOS, `/private/var/...`).
        let canonico = std::fs::canonicalize(dir.path())
            .expect("canonicalize")
            .to_string_lossy()
            .into_owned();

        for operacao in ["status", "diff"] {
            let restrito = crate::tools::turn_tools::com_restricao_do_turno(true, async {
                tool.execute(
                    &ctx(Some(&wd)),
                    serde_json::json!({ "operation": operacao }),
                )
                .await
            })
            .await;
            let texto = match restrito {
                Ok(saida) => saida.content,
                Err(e) => e.to_string(),
            };
            assert!(
                !texto.contains(&wd) && !texto.contains(&canonico),
                "{operacao}: o caminho do host saiu no turno restrito:\n{texto}"
            );
            // So o `status` recebe a recusa que cita o diretorio; o `diff`
            // num bare cai no uso do `--no-index`, que nao nomeia caminho.
            if operacao == "status" {
                assert!(texto.contains("safe.bareRepository"), "{texto}");
                // O diretorio citado e o proprio working_dir da sessao: sai
                // relativo, nao como "fora do diretorio da sessao".
                assert!(texto.contains("cannot use bare repository '.'"), "{texto}");
            }
        }

        // Turno aberto (operador local): o texto do git sai inteiro.
        let aberto = crate::tools::turn_tools::com_restricao_do_turno(false, async {
            tool.execute(
                &ctx(Some(&wd)),
                serde_json::json!({ "operation": "status" }),
            )
            .await
        })
        .await;
        let texto = match aberto {
            Ok(saida) => saida.content,
            Err(e) => e.to_string(),
        };
        assert!(
            texto.contains(&wd) || texto.contains(&canonico),
            "o turno aberto perdeu o caminho:\n{texto}"
        );
    }

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
        // #1258: a procedência é nomeada. #1626: sem o caminho absoluto, que
        // vazaria o dir por-orquestrador no envelope servido pela ponte.
        assert_eq!(
            saida.content.lines().next().unwrap_or_default(),
            "Repositório: o working_dir da sessão"
        );
        assert!(
            !saida.content.contains(&wd),
            "o caminho absoluto do working_dir vazou na resposta:\n{}",
            saida.content
        );
    }

    /// #1258, caso 2 — sessão **sem** `working_dir`, que no gateway é o comum
    /// (um prompt de Telegram não traz projeto). O CWD do processo é mantido,
    /// como antes, mas a resposta passa a dizer de qual repositório ela falou:
    /// a resposta errada *silenciosa* era o defeito. Turno ABERTO (operador
    /// local), onde o caminho é acionável; no restrito ele não sai (#1909).
    #[tokio::test]
    async fn sem_working_dir_a_resposta_nomeia_o_repositorio_do_cwd() {
        let tool = GitDiffTool::new(Some(15), Some(500));
        let cwd = std::env::current_dir().expect("CWD do processo de teste");

        for operacao in ["diff", "status"] {
            let saida = crate::tools::turn_tools::com_restricao_do_turno(false, async {
                tool.execute(&ctx(None), serde_json::json!({ "operation": operacao }))
                    .await
                    .expect("execute")
            })
            .await;

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

    /// #1909: a mesma resposta de sessão sem `working_dir`, agora num turno
    /// restrito — o CWD do processo do gateway é caminho do HOST. A
    /// procedência continua dita (é o que desfaz a confusão silenciosa da
    /// #1258); o lugar do caminho é o marcador das outras tools.
    #[tokio::test]
    async fn sem_working_dir_no_turno_restrito_a_resposta_nao_nomeia_o_cwd() {
        let tool = GitDiffTool::new(Some(15), Some(500));
        let cwd = std::env::current_dir().expect("CWD do processo de teste");

        let saida = crate::tools::turn_tools::com_restricao_do_turno(true, async {
            tool.execute(&ctx(None), serde_json::json!({ "operation": "status" }))
                .await
                .expect("execute")
        })
        .await;

        let primeira = saida.content.lines().next().unwrap_or_default();
        assert!(primeira.starts_with("Repositório: "), "{primeira}");
        assert!(
            primeira.contains(crate::tools::tool_context::CAMINHO_FORA_DO_TURNO),
            "o lugar do caminho deveria ser o marcador: {primeira}"
        );
        assert!(
            !saida.content.contains(&cwd.display().to_string()),
            "o CWD do host saiu no turno restrito:\n{}",
            saida.content
        );
    }

    /// #1258: `working_dir` que não existe falha **nomeando a procedência** (o
    /// working_dir da sessão), e não cai de volta no CWD do processo — era a
    /// confusão silenciosa que a issue descreve. #1626: a procedência é nomeada
    /// sem o caminho absoluto, que vazaria o dir por-orquestrador no envelope
    /// servido pela ponte (a #1258 segue distinguindo working_dir de CWD, que é
    /// o que resolve a confusão).
    #[tokio::test]
    async fn working_dir_inexistente_nomeia_a_procedencia_sem_o_caminho() {
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
        // Nomeia o working_dir da sessão (não cai no CWD), mas sem o caminho.
        assert!(
            saida.content.contains("working_dir da sessão"),
            "{}",
            saida.content
        );
        assert!(
            !saida.content.contains(inexistente),
            "o caminho absoluto do working_dir vazou:\n{}",
            saida.content
        );
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

    /// #1296: operação ausente é observação soft (`Ok` + `is_error`), não
    /// `Err` — a mensagem nomeia o parâmetro e o schema esperado.
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

        let output = tool
            .execute(&ctx, serde_json::json!({}))
            .await
            .expect("parâmetro ausente é soft-error, não Err");

        assert!(output.is_error);
        assert!(output.content.contains("'operation'"), "{}", output.content);
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
                "--no-textconv".to_string(),
                "--ignore-submodules=all".to_string(),
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
                "--no-textconv".to_string(),
                "--ignore-submodules=all".to_string(),
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
                "--no-textconv".to_string(),
                "--ignore-submodules=all".to_string(),
                "-U3".to_string(),
                "--".to_string(),
            ]
        );
    }
}
