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
    ) -> Result<std::result::Result<String, String>> {
        // #1269: os valores que vêm da tool call do modelo nunca caem em
        // posição de flag — ver `git_diff_args`. Validado ANTES de qualquer
        // spawn, inclusive o da resolucao das pontas.
        let args = git_diff_args(file_path, context_lines, from_commit, to_commit)
            .map_err(Error::Agent)?;

        // #1909: o intervalo e resolvido ANTES do diff, e o diff roda sobre
        // os SHAs resolvidos. Assim a linha `Intervalo:` nomeia exatamente o
        // que foi comparado: um ramo que se mova entre as duas chamadas nao
        // rotula a resposta com um SHA que nao e o do diff, e uma ponta que o
        // git aceitaria de outro jeito (`HEAD~1..` com a outra vazia, ou um
        // `a...b` montado pela concatenacao) nunca devolve diff sem intervalo
        // nomeado — sem as duas pontas resolvidas, o diff nao roda.
        let (intervalo, args) = match (from_commit, to_commit) {
            (Some(from), Some(to)) => {
                let (rev_de, rev_ate) = (format!("{from}^{{commit}}"), format!("{to}^{{commit}}"));
                let (de, ate) = tokio::join!(
                    self.revisao_de(repo, &rev_de),
                    self.revisao_de(repo, &rev_ate),
                );
                let linha =
                    linha_do_intervalo_entre_commits(from, de.as_deref(), to, ate.as_deref());
                let (Some(de), Some(ate)) = (de, ate) else {
                    // Recusa da TOOL, nao erro do agente: sai como texto
                    // puro, com a linha do intervalo no lugar de sempre.
                    return Ok(Err(format!(
                        "{linha}\n\nO diff não foi executado: cada ponta do intervalo precisa \
                         ser um commit deste repositório. Confira o nome com \
                         operation \"status\" ou use o SHA completo."
                    )));
                };
                let args = git_diff_args(file_path, context_lines, Some(&de), Some(&ate))
                    .map_err(Error::Agent)?;
                (linha, args)
            }
            // Sem intervalo o `git diff` compara o indice com a arvore de
            // trabalho. O `HEAD` nao e uma das pontas, mas e o que diz EM QUAL
            // checkout essa comparacao foi feita.
            _ => (
                linha_do_intervalo_da_arvore(self.revisao_exata(repo).await.as_deref()),
                args,
            ),
        };

        // Executa git diff
        let output = self.run_git_command(&args, repo).await?;

        // Aplica filtros de segurança
        let filtered = self.filter_secrets(&output);
        let limited = self.limit_lines(&filtered);

        // A linha do intervalo entra DEPOIS do filtro e do corte de linhas,
        // para nunca ser a linha que o limite esconde.
        Ok(Ok(format!("{intervalo}\n\n{limited}")))
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
pub(super) const PONTA_NAO_RESOLVIDA: &str = "revisão não resolvida";

/// O nome de uma revisao como o modelo o escreveu, pronto para UMA linha.
///
/// O valor vem da tool call. Caractere de controle (uma quebra de linha,
/// sobretudo) deixaria o modelo escrever linhas inteiras na resposta com a
/// cara de saida da tool; vira `?`, como os parenteses
/// ([`caractere_que_forja_linha`]). O tamanho e cortado pelo mesmo motivo que
/// qualquer eco de entrada: a linha e contexto, nao transporte.
pub(super) fn nome_da_revisao(revisao: &str) -> String {
    const TETO: usize = 120;
    let limpo: String = revisao
        .chars()
        .map(|c| if caractere_que_forja_linha(c) { '?' } else { c })
        .take(TETO)
        .collect();
    if revisao.chars().count() > TETO {
        format!("{limpo}…")
    } else {
        limpo
    }
}

/// O que nao pode sair cru no nome de uma revisao: controle e separadores
/// de linha/paragrafo Unicode (abririam uma "linha" nova para quem le), e os
/// parenteses, que na linha do intervalo delimitam o SHA — com eles o nome
/// `x) y (0000…` desenharia um par `nome (sha)` que a tool nao resolveu.
fn caractere_que_forja_linha(c: char) -> bool {
    c.is_control() || matches!(c, '(' | ')' | '\u{2028}' | '\u{2029}')
}

/// A linha do intervalo de um diff entre commits: cada ponta com o nome que
/// foi pedido e o SHA que ele valia na hora — ou a falta dele, dita.
pub(super) fn linha_do_intervalo_entre_commits(
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
pub(super) fn linha_do_intervalo_da_arvore(head: Option<&str>) -> String {
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
pub(super) fn revisao_do_output(saida: &str) -> Option<&str> {
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
                    Ok(Ok(output)) => Ok(ToolOutput::success(repo.com_contexto(&output))),
                    Ok(Err(recusa)) => Ok(ToolOutput::error(repo.com_contexto(&recusa))),
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
mod tests;
