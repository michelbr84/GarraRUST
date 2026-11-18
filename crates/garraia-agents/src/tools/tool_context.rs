//! Phase 2.2 — ToolContext: working directory resolution and path traversal
//! protection for tool execution within a project context.
//!
//! Issue #923 added the free functions at the bottom of this module. The
//! file tools used to do `PathBuf::from(input)` verbatim: no `~` expansion
//! anywhere in the crate, `ToolContext.working_dir` ignored, and an error
//! that dropped the path it had tried. On Termux that combination made the
//! agent tell a user a file "did not exist" while the filesystem MCP server
//! read the very same path — the agent had no way to know it had looked
//! somewhere else.

use garraia_common::{Error, Result};
use std::path::{Component, Path, PathBuf};

/// Extended context for tool execution within a project.
///
/// Provides path resolution relative to a working directory and validates
/// that resolved paths do not escape the project sandbox.
#[derive(Debug, Clone)]
pub struct ProjectToolContext {
    /// The working directory for path resolution (project root).
    pub working_dir: Option<PathBuf>,
    /// The project ID this context is associated with.
    pub project_id: Option<String>,
    /// When true, paths are strictly confined to `working_dir`.
    pub sandbox_enabled: bool,
}

impl Default for ProjectToolContext {
    fn default() -> Self {
        Self {
            working_dir: None,
            project_id: None,
            sandbox_enabled: true,
        }
    }
}

impl ProjectToolContext {
    /// Create a new context with the given working directory.
    pub fn new(working_dir: Option<PathBuf>, project_id: Option<String>) -> Self {
        Self {
            working_dir,
            project_id,
            sandbox_enabled: true,
        }
    }

    /// Create a context with sandboxing disabled (for trusted sessions).
    pub fn unsandboxed(working_dir: Option<PathBuf>, project_id: Option<String>) -> Self {
        Self {
            working_dir,
            project_id,
            sandbox_enabled: false,
        }
    }

    /// Resolve a potentially relative path against the working directory.
    ///
    /// - If `relative` is absolute, it is returned as-is (but still validated
    ///   when `sandbox_enabled` is true).
    /// - If `relative` is relative and a `working_dir` is set, it is joined
    ///   to the working directory.
    /// - If no `working_dir` is set, the relative path is returned as-is.
    pub fn resolve_path(&self, relative: &str) -> Result<PathBuf> {
        let input = PathBuf::from(relative);

        let resolved = if input.is_absolute() {
            input
        } else if let Some(ref wd) = self.working_dir {
            wd.join(&input)
        } else {
            input
        };

        if self.sandbox_enabled {
            self.validate_path(&resolved)?;
        }

        Ok(resolved)
    }

    /// Validate that a path does not escape the working directory.
    ///
    /// This checks for `..` components and, when sandboxing is enabled,
    /// ensures the canonical path starts with the working directory.
    pub fn validate_path(&self, path: &Path) -> Result<()> {
        // Always reject `..` components to prevent path traversal.
        if path.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(Error::Security(
                "path traversal not allowed (contains '..')".into(),
            ));
        }

        // When sandboxing with a working directory, ensure the resolved path
        // stays within the working directory.
        if self.sandbox_enabled
            && let Some(ref wd) = self.working_dir
        {
            // Canonicalize both paths for comparison. If the file doesn't
            // exist yet (e.g. write target), canonicalize the parent
            // directory instead.
            let wd_canonical = wd.canonicalize().map_err(|e| {
                Error::Agent(format!(
                    "cannot canonicalize working directory '{}': {e}",
                    wd.display()
                ))
            })?;

            let path_canonical = if path.exists() {
                path.canonicalize().map_err(|e| {
                    Error::Agent(format!(
                        "cannot canonicalize path '{}': {e}",
                        path.display()
                    ))
                })?
            } else if let Some(parent) = path.parent() {
                if parent.exists() {
                    let parent_canonical = parent.canonicalize().map_err(|e| {
                        Error::Agent(format!(
                            "cannot canonicalize parent '{}': {e}",
                            parent.display()
                        ))
                    })?;
                    parent_canonical.join(path.file_name().unwrap_or_default())
                } else {
                    // Neither path nor parent exist — reject when sandboxed.
                    return Err(Error::Security(
                        "path parent directory does not exist inside sandbox".into(),
                    ));
                }
            } else {
                return Err(Error::Security(
                    "invalid path for sandbox validation".into(),
                ));
            };

            if !path_canonical.starts_with(&wd_canonical) {
                return Err(Error::Security(format!(
                    "path '{}' escapes working directory '{}'",
                    path.display(),
                    wd.display()
                )));
            }
        }

        Ok(())
    }

    /// Quick check whether a path is allowed under current sandbox rules.
    pub fn is_path_allowed(&self, path: &Path) -> bool {
        self.validate_path(path).is_ok()
    }
}

// ─── Path resolution for the file tools (issue #923) ───────────────────────
//
// Kept as free, *pure* functions with the environment passed in — the same
// shape as `termux_ld_preload` in the MCP manager, and for the same reason:
// the interesting cases (`~` on a machine with no HOME, a relative path with
// no working dir) are the ones you cannot reproduce by running the test suite
// on a normal developer box.

/// How a raw tool argument became a filesystem path. Carried into error
/// messages so a failure says *where* it looked, not just that it failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathOrigin {
    /// The input was already absolute.
    Absolute,
    /// A leading `~`, `~/` or `$HOME` was expanded.
    HomeExpanded,
    /// A relative input joined to the session's `working_dir`.
    JoinedWorkingDir,
    /// A relative input with no `working_dir` to join it to. It resolves
    /// against the *gateway process* CWD, which is almost never what the
    /// caller meant — this is the case worth naming out loud.
    RelativeToProcessCwd,
}

impl PathOrigin {
    /// A short clause explaining the resolution, for error messages.
    pub fn hint(self) -> &'static str {
        match self {
            PathOrigin::Absolute => "caminho absoluto",
            PathOrigin::HomeExpanded => "expandido a partir de ~/$HOME",
            PathOrigin::JoinedWorkingDir => "relativo ao working_dir da sessão",
            PathOrigin::RelativeToProcessCwd => {
                "relativo ao diretório de trabalho do processo do gateway \
                 (a sessão não tem working_dir) — informe um caminho absoluto \
                 ou use ~/"
            }
        }
    }
}

/// A tool argument after resolution, plus how it got there.
#[derive(Debug, Clone)]
pub struct ResolvedPath {
    pub path: PathBuf,
    pub origin: PathOrigin,
    /// The argument exactly as the model wrote it.
    pub requested: String,
    /// O `working_dir` da sessao, guardado para [`ResolvedPath::describe`]
    /// mostrar o caminho RELATIVO a ele quando o alvo cai dentro — e nunca o
    /// prefixo absoluto do host (#1626 / principio da #1465).
    pub working_dir: Option<PathBuf>,
}

impl ResolvedPath {
    /// `"'<mostrado>' (pedido: '<raw>', <hint>)"` — the tail every file-tool
    /// error carries, so the model can tell "wrong path" from "no such file".
    ///
    /// `<mostrado>` e relativo ao `working_dir` da sessao quando o alvo cai
    /// dentro dele (`notas/a.txt`, nao `/home/<user>/.../workspace/<hash>/notas/a.txt`),
    /// via [`caminho_para_exibir_no_turno`]: a mensagem vira o `summary` da tool
    /// call no envelope servido ao orquestrador, e o caminho absoluto do host
    /// nao pode sair por ali (#1626) — nem quando ele cai FORA do
    /// `working_dir` num turno restrito (#1909).
    pub fn describe(&self) -> String {
        format!(
            "'{}' (pedido: '{}', {})",
            caminho_para_exibir_no_turno(&self.path, self.working_dir.as_deref()),
            self.requested,
            self.origin.hint()
        )
    }
}

/// Um caminho para MOSTRAR ao modelo/cliente, sem vazar o prefixo do host.
///
/// Relativo ao `working_dir` da sessao quando o caminho cai dentro dele
/// (`notas/a.txt` em vez de `/home/<user>/.local/share/garraia/workspace/<hash>/notas/a.txt`);
/// absoluto so quando nao ha `working_dir` ou o caminho esta fora dele (uma raiz
/// declarada em `agent.file_roots`, que o operador escolheu e ja conhece). Fecha
/// o vazamento do caminho do host no envelope `garra.agent.v1` servido ao
/// orquestrador pela ponte MCP HTTP (#1626), seguindo o principio da #1465
/// (nenhum caminho absoluto do host em superficie servida ao cliente). Os logs
/// do operador seguem com o caminho absoluto de proposito — a disciplina da
/// #1465 e sobre o que SAI para o cliente, nao sobre o log do dono da maquina.
///
/// Compara contra o `working_dir` cru E o canonicalizado: o `path` pode chegar
/// ja canonicalizado (apos [`super::FileJail::confine`]) enquanto o
/// `working_dir` ainda e a string crua da sessao.
///
/// SEM o portao do turno, e por isso so para teste (#1909): nenhuma tool
/// chama esta forma — o que vai para uma mensagem passa por
/// [`caminho_para_exibir_no_turno`]. Deixa-la `pub`, com o nome mais curto,
/// era o convite para o proximo call site reabrir a fresta.
#[cfg(test)]
fn caminho_para_exibir(path: &Path, working_dir: Option<&Path>) -> String {
    caminho_relativo_ao_working_dir(path, working_dir).unwrap_or_else(|| path.display().to_string())
}

/// O rotulo que ocupa o lugar do caminho quando o turno nao pode ver o do
/// host (#1909).
///
/// Nao e o caminho "pedido": esse ja aparece ao lado, no campo `pedido:`, e e
/// texto que o proprio modelo escreveu — devolve-lo aqui nao faria a mensagem
/// mais curta, e um marcador unico e o que um teste consegue afirmar.
pub const CAMINHO_FORA_DO_TURNO: &str = "<outside the session's directory>";

/// O caminho resolvido pode aparecer nas mensagens DESTE turno? (#1909)
///
/// A mesma pergunta que o `repo_search` faz desde a #1380 e o `garra_status`
/// desde a #1347, que retem de proposito o `working_dir` da sessao no turno
/// restrito. Sem este portao o mesmo caminho absoluto do host voltava pelas
/// mensagens de `list_dir`, `file_read` e `file_write`.
///
/// O QUE "turno restrito" E, exatamente: o bit que o runtime passa a
/// `com_restricao_do_turno` e `ToolGate::restringe_por_whitelist()` — o modo
/// em vigor tem allowlist (ou `no_tools`), ou ha um teto por principal que
/// tem. E a FORMA da politica de tools, nao a identidade do remetente. Duas
/// consequencias, as duas de proposito registradas aqui:
///
/// - um modo com allowlist retem o caminho ate no `garra chat` local
///   (`plan`, `search`, `edit`, `debug`, `review`, `architect`,
///   `orchestrator`);
/// - um modo SEM allowlist conta como aberto mesmo vindo de canal remoto sem
///   teto por principal (o `ask`, que so tem lista de negacao) — ali o
///   caminho continua saindo. Fechar isso pede um bit "remetente nao e o
///   operador" vindo do canal, que este portao nao tem (#1909).
///
/// Fail-closed: fora de um turno despachado pelo runtime (teste direto,
/// chamada sem escopo) a resposta e `false` e o caminho NAO sai. Os logs do
/// operador seguem com o caminho inteiro, de proposito.
pub fn pode_revelar_caminho_do_host() -> bool {
    !crate::tools::turn_tools::turno_restrito().unwrap_or(true)
}

/// A parte que e sempre segura de mostrar: o caminho relativo
/// ao `working_dir`, quando ele cai dentro. `None` quando nao ha `working_dir`
/// ou o alvo esta fora dele.
///
/// `strip_prefix` e LEXICO: `<wd>/../irmao/x` comeca pelo `working_dir` e
/// mesmo assim aponta para fora dele. Um resto com `..` nao conta como
/// "dentro" — sem isso o turno restrito recebia `../irmao/x`, o nome de um
/// diretorio vizinho ao da sessao, no lugar do marcador (#1909).
fn caminho_relativo_ao_working_dir(path: &Path, working_dir: Option<&Path>) -> Option<String> {
    let wd = working_dir.filter(|w| !w.as_os_str().is_empty())?;
    let bases = std::iter::once(wd.to_path_buf()).chain(std::fs::canonicalize(wd).ok());
    for base in bases {
        if let Ok(rel) = path.strip_prefix(&base) {
            if rel
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                continue;
            }
            return Some(if rel.as_os_str().is_empty() {
                ".".to_string()
            } else {
                rel.display().to_string()
            });
        }
    }
    None
}

/// Um caminho para MOSTRAR numa mensagem de tool, com o portao do turno
/// (#1909). E a unica forma que as tools usam.
///
/// Tres casos, nesta ordem:
///
/// 1. o alvo cai dentro do `working_dir` da sessao — a forma relativa nao
///    nomeia o host e e a que o modelo reconhece; sai sempre;
/// 2. o que sobra e um caminho RELATIVO (o argumento do modelo, ainda nao
///    canonicalizado) ou um turno que pode ver
///    ([`pode_revelar_caminho_do_host`]) — sai, como antes (o `garra chat`
///    local ve o absoluto, onde ele e acionavel; um relativo nao nomeia host
///    nenhum, seja qual for o turno);
/// 3. um caminho absoluto fora do `working_dir` num turno restrito — sai
///    [`CAMINHO_FORA_DO_TURNO`].
pub fn caminho_para_exibir_no_turno(path: &Path, working_dir: Option<&Path>) -> String {
    if let Some(rel) = caminho_relativo_ao_working_dir(path, working_dir) {
        return rel;
    }
    if !path.is_absolute() || pode_revelar_caminho_do_host() {
        path.display().to_string()
    } else {
        CAMINHO_FORA_DO_TURNO.to_string()
    }
}

/// O comeco, constante, da recusa de [`recusa_sessao_sem_diretorio`]. E por
/// ele que o circuit breaker reconhece a causa (`breaker::classificar`): o
/// resto da frase muda por tool, o comeco nao.
pub const SESSAO_SEM_DIRETORIO: &str = "The session's working directory is not accessible";

/// A recusa quando o diretório da sessão não serve como raiz (#1909), ou
/// `None` quando ele existe e é diretório.
///
/// ## Por que isto é compartilhado
///
/// O `repo_search` ganhou esta recusa na #1932 e ela resolveu um defeito
/// concreto do dogfood: o modelo ofereceu um worktree, a busca rodou no
/// workspace da sessão, e a resposta veio como "No matches found" — que lê
/// como "não existe". Agora diga-se o mesmo no `list_dir` e no `file_read`:
/// lá o diretório da sessão ausente caía na recusa do **jail** ("acesso
/// negado"), que lê como política de segurança, quando a causa é outra (o
/// projeto foi movido, o worktree foi removido, a sessão aponta para um
/// caminho que morreu). Três tools, uma frase: o modelo reconhece a MESMA
/// causa e pede a mesma ação (selecionar um projeto) em vez de trocar o
/// caminho e tentar de novo.
///
/// `o_que_nao_foi_feito` é a consequência ("nothing was searched") e `repita`
/// é o convite ("search again") — cada tool diz o que é dela sem reescrever a
/// frase inteira.
///
/// ## O que a mensagem NÃO diz
///
/// O caminho não entra: ele pode ser a subárvore por-orquestrador do host
/// (mesmo princípio do `caminho_para_exibir`, #1626, e de
/// `escopo_da_busca`). O que sai é o motivo (`it does not exist` /
/// `it is not a directory`) sobre o diretório **da própria sessão**, que não
/// é oráculo de existência de caminho alheio.
///
/// Erro de outra natureza ao olhar (`EACCES` num ancestral, por exemplo) não
/// recusa: na dúvida a tool segue e o erro real aparece adiante.
pub fn recusa_sessao_sem_diretorio(
    working_dir: Option<&Path>,
    o_que_nao_foi_feito: &str,
    repita: &str,
) -> Option<String> {
    // `trim`, e não `is_empty`: um `working_dir` só com espaços não é diretório
    // nenhum, e `stat("   ")` mediria o CWD do processo para depois afirmar
    // "it does not exist" sobre ele — mentira sobre um caminho que o usuário
    // não pediu. Sem diretório de sessão a recusa não é desta função; a tool
    // segue para o caminho normal (jail).
    let dir = working_dir.filter(|d| !d.as_os_str().to_string_lossy().trim().is_empty())?;
    let motivo = match std::fs::metadata(dir) {
        Ok(meta) if meta.is_dir() => return None,
        Ok(_) => "it is not a directory",
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "it does not exist",
        Err(_) => return None,
    };
    Some(format!(
        "{SESSAO_SEM_DIRETORIO} ({motivo}), so {o_que_nao_foi_feito}. \
         This is not an empty result: select an existing project for this session (set its \
         working directory) and {repita}."
    ))
}

/// Expand a leading `~`, `~/…` or `$HOME/…` against `home`.
///
/// Only a *leading* segment is expanded, and only when `home` is known: a
/// path with `~` in the middle is a legitimate filename, and silently
/// rewriting it would be worse than leaving it alone. With `home` absent the
/// input is returned untouched, which fails later with a message naming the
/// literal `~` path — a comprehensible error rather than a wrong guess.
pub fn expand_home(raw: &str, home: Option<&Path>) -> (PathBuf, bool) {
    let Some(home) = home else {
        return (PathBuf::from(raw), false);
    };

    let rest = if raw == "~" || raw == "$HOME" {
        Some("")
    } else {
        raw.strip_prefix("~/")
            .or_else(|| raw.strip_prefix("$HOME/"))
    };

    match rest {
        Some("") => (home.to_path_buf(), true),
        Some(r) => (home.join(r), true),
        None => (PathBuf::from(raw), false),
    }
}

/// Resolve a raw tool path argument: expand `~`, then join a relative path to
/// the session's `working_dir` when there is one.
///
/// `..` is rejected before anything else, exactly as the file tools did
/// before — this is a resolution change, not a loosening of the security
/// posture. The check runs on the raw input so a `~/../etc` cannot smuggle a
/// traversal in through the expansion.
pub fn resolve_tool_path(
    raw: &str,
    working_dir: Option<&str>,
    home: Option<&Path>,
) -> Result<ResolvedPath> {
    if Path::new(raw)
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(Error::Security("path traversal não permitido".into()));
    }

    let (expanded, was_home) = expand_home(raw, home);

    let (path, origin) = if was_home {
        (expanded, PathOrigin::HomeExpanded)
    } else if expanded.is_absolute() {
        (expanded, PathOrigin::Absolute)
    } else if let Some(wd) = working_dir.filter(|w| !w.is_empty()) {
        (Path::new(wd).join(&expanded), PathOrigin::JoinedWorkingDir)
    } else {
        (expanded, PathOrigin::RelativeToProcessCwd)
    };

    Ok(ResolvedPath {
        path,
        origin,
        requested: raw.to_string(),
        working_dir: working_dir
            .filter(|w| !w.trim().is_empty())
            .map(PathBuf::from),
    })
}

/// The process home directory, or `None` when the environment has none.
///
/// Deliberately reads the environment rather than pulling in `dirs`: the
/// pure functions above take `home` as a parameter, so this is the whole
/// impure surface and a new dependency edge on `garraia-agents` would buy
/// nothing. `HOME` is always set inside Termux, which is the reported case.
pub fn process_home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

// Em arquivo proprio para manter este abaixo do teto de 700 linhas do
// Quality Ratchet (mesmo arranjo do `repo_search_tool/tests.rs`).
#[cfg(test)]
mod tests;
