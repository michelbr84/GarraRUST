//! Confinamento de caminhos de projeto a um conjunto de raízes permitidas.
//!
//! ## Por que isto existe
//!
//! `POST /api/projects` aceitava `path` como `String` crua do corpo JSON e
//! guardava sem validar; `GET /api/projects/{id}/files` depois percorria esse
//! diretório **recursivamente**. Como todo o `/api/*` é auth-free por decisão
//! de design, a sequência
//!
//! ```text
//! POST /api/projects {"name":"x","path":"/etc"}
//! GET  /api/projects/{id}/files
//! ```
//!
//! enumerava `/etc` inteiro para qualquer um que alcançasse a porta — e o
//! gateway pode escutar em `0.0.0.0`. O mesmo valia para `/root`, `/proc`,
//! `/var`, e para o `working_dir` de `POST /api/sessions`.
//!
//! ## O que este módulo garante
//!
//! [`confine`] só devolve `Ok` para um caminho que, **depois de resolvido**
//! (`std::fs::canonicalize`, que segue symlinks e elimina `..`), cai sob uma
//! das raízes de [`allowed_roots`]. Resolver antes de comparar é o ponto:
//! comparar a string crua deixaria passar tanto `~/projs/../../etc` quanto um
//! symlink `~/projs/escape → /etc`.
//!
//! Comparação por componente, via [`std::path::Path::starts_with`] — nunca por
//! prefixo de string, que aceitaria `/home/user-evil` como estando sob
//! `/home/user`.
//!
//! ## Raízes
//!
//! Padrão: o home do usuário, e só ele. Um projeto fora do home é raro o
//! bastante para valer uma escolha explícita do operador, e essa escolha é
//! [`ROOTS_ENV`] — uma lista no formato de `PATH` (`:` no unix, `;` no
//! Windows) que **substitui** o padrão.
//!
//! Se nenhuma raiz resolver, [`confine`] rejeita tudo. Fail-closed: sem raiz
//! conhecida não há como afirmar que um caminho é seguro.

use std::path::{Path, PathBuf};

use tracing::warn;

/// Variável de ambiente que **substitui** as raízes padrão.
///
/// Formato de `PATH`: `GARRAIA_PROJECT_ROOTS=/srv/projetos:/opt/code`.
pub const ROOTS_ENV: &str = "GARRAIA_PROJECT_ROOTS";

/// Por que um `path` de projeto foi recusado.
///
/// As variantes não distinguem "não existe" de "existe mas está fora": ambas
/// viram [`ProjectPathError::Unresolvable`] ou
/// [`ProjectPathError::OutsideAllowedRoots`] conforme o caso, e a mensagem
/// devolvida ao cliente é a mesma nos dois — ver `projects_handler`. Um
/// endpoint auth-free não deve virar oráculo de existência de arquivo.
#[derive(Debug, thiserror::Error, PartialEq, Eq, Clone)]
pub enum ProjectPathError {
    #[error("project path must not be empty")]
    Empty,
    #[error("project path could not be resolved")]
    Unresolvable,
    #[error("project path is not a directory")]
    NotADirectory,
    #[error("project path is outside the allowed roots")]
    OutsideAllowedRoots,
    #[error("no allowed project root is configured")]
    NoRootsConfigured,
}

/// Raízes sob as quais um caminho de projeto pode cair, já canonicalizadas.
///
/// Canonicalizar as raízes também é necessário, não só o candidato: no macOS
/// um `TempDir` vive em `/var/folders/...` que resolve para `/private/var/...`,
/// e a comparação falharia contra a forma não resolvida.
///
/// Raízes que não resolvem (não existem, sem permissão) são descartadas com um
/// `warn!` — uma raiz inexistente não pode autorizar nada.
pub fn allowed_roots() -> Vec<PathBuf> {
    let configured: Vec<PathBuf> = match std::env::var(ROOTS_ENV) {
        Ok(raw) if !raw.trim().is_empty() => std::env::split_paths(&raw).collect(),
        _ => dirs::home_dir().into_iter().collect(),
    };

    configured
        .into_iter()
        .filter_map(|root| match std::fs::canonicalize(&root) {
            Ok(resolved) => Some(resolved),
            Err(e) => {
                warn!(
                    root = %root.display(),
                    error = %e,
                    "raiz de projeto ignorada: não foi possível resolver"
                );
                None
            }
        })
        .collect()
}

/// Resolve `raw` e exige que ele caia sob uma das [`allowed_roots`].
///
/// Devolve o caminho **canonicalizado** — é ele que deve ser armazenado e
/// usado, não o `raw`, senão a resolução feita aqui não vale nada na hora de
/// ler o diretório.
pub fn confine(raw: &str) -> Result<PathBuf, ProjectPathError> {
    confine_within(raw, &allowed_roots())
}

/// Núcleo testável de [`confine`], com as raízes injetadas.
pub fn confine_within(raw: &str, roots: &[PathBuf]) -> Result<PathBuf, ProjectPathError> {
    if raw.trim().is_empty() {
        return Err(ProjectPathError::Empty);
    }
    if roots.is_empty() {
        return Err(ProjectPathError::NoRootsConfigured);
    }

    // `canonicalize` resolve symlinks e `..`, e falha se o caminho não existe.
    // Exigir existência é deliberado: um projeto apontando para um diretório
    // inexistente não serve para nada, e sem resolver não há o que confinar.
    let resolved = std::fs::canonicalize(raw).map_err(|_| ProjectPathError::Unresolvable)?;

    if !resolved.is_dir() {
        return Err(ProjectPathError::NotADirectory);
    }

    if roots.iter().any(|root| is_within(&resolved, root)) {
        Ok(resolved)
    } else {
        Err(ProjectPathError::OutsideAllowedRoots)
    }
}

/// `path` está sob `root` (ou é o próprio `root`)?
///
/// `Path::starts_with` compara **componente a componente**, então
/// `/home/user-evil` não é considerado dentro de `/home/user`. Um
/// `str::starts_with` sobre as strings aceitaria — é a armadilha clássica
/// desse tipo de checagem.
fn is_within(path: &Path, root: &Path) -> bool {
    path.starts_with(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cria `root/<sub>` e devolve (tempdir, root canonicalizado).
    fn root_with(sub: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = std::fs::canonicalize(tmp.path()).expect("canonicalize root");
        let child = root.join(sub);
        std::fs::create_dir_all(&child).expect("create child");
        (tmp, root, child)
    }

    #[test]
    fn accepts_a_directory_under_the_root() {
        let (_tmp, root, child) = root_with("projeto");
        let got = confine_within(child.to_str().unwrap(), &[root]).expect("deve aceitar");
        assert_eq!(got, child);
    }

    #[test]
    fn accepts_the_root_itself() {
        let (_tmp, root, _child) = root_with("projeto");
        let got = confine_within(root.to_str().unwrap(), std::slice::from_ref(&root))
            .expect("deve aceitar");
        assert_eq!(got, root);
    }

    #[test]
    fn rejects_absolute_path_outside_the_root() {
        let (_tmp, root, _child) = root_with("projeto");
        assert_eq!(
            confine_within("/etc", &[root]),
            Err(ProjectPathError::OutsideAllowedRoots)
        );
    }

    /// O vetor original: `..` escapando da raiz. `canonicalize` o achata antes
    /// da comparação, então a fuga aparece como o caminho real que é.
    #[test]
    fn rejects_dotdot_traversal_out_of_the_root() {
        let (_tmp, root, child) = root_with("projeto");
        let escape = format!("{}/../../../../etc", child.display());
        assert_eq!(
            confine_within(&escape, &[root]),
            Err(ProjectPathError::OutsideAllowedRoots)
        );
    }

    /// Symlink apontando para fora: é por isso que a comparação acontece
    /// depois de `canonicalize`, e não sobre a string crua.
    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escaping_the_root() {
        let (_tmp, root, _child) = root_with("projeto");
        let link = root.join("fuga");
        std::os::unix::fs::symlink("/etc", &link).expect("symlink");

        assert_eq!(
            confine_within(link.to_str().unwrap(), &[root]),
            Err(ProjectPathError::OutsideAllowedRoots)
        );
    }

    /// Armadilha do prefixo de string: `/home/user-evil` NÃO está sob
    /// `/home/user`. `Path::starts_with` compara componentes, `str` não.
    #[test]
    fn sibling_with_shared_string_prefix_is_not_within() {
        let (_tmp, root, _child) = root_with("projeto");
        let sibling = root.parent().expect("parent").join(format!(
            "{}-evil",
            root.file_name().unwrap().to_string_lossy()
        ));
        std::fs::create_dir_all(&sibling).expect("create sibling");

        let got = confine_within(sibling.to_str().unwrap(), &[root]);
        std::fs::remove_dir_all(&sibling).ok();
        assert_eq!(got, Err(ProjectPathError::OutsideAllowedRoots));
    }

    #[test]
    fn rejects_a_file_that_is_not_a_directory() {
        let (_tmp, root, _child) = root_with("projeto");
        let file = root.join("arquivo.txt");
        std::fs::write(&file, b"x").expect("write");
        assert_eq!(
            confine_within(file.to_str().unwrap(), &[root]),
            Err(ProjectPathError::NotADirectory)
        );
    }

    #[test]
    fn rejects_nonexistent_path() {
        let (_tmp, root, _child) = root_with("projeto");
        let missing = root.join("nao-existe");
        assert_eq!(
            confine_within(missing.to_str().unwrap(), &[root]),
            Err(ProjectPathError::Unresolvable)
        );
    }

    #[test]
    fn rejects_empty_input() {
        let (_tmp, root, _child) = root_with("projeto");
        assert_eq!(
            confine_within("", std::slice::from_ref(&root)),
            Err(ProjectPathError::Empty)
        );
        assert_eq!(confine_within("   ", &[root]), Err(ProjectPathError::Empty));
    }

    /// Fail-closed: sem raiz configurada nada é aceito, nem um caminho que
    /// existe e é diretório.
    #[test]
    fn rejects_everything_when_no_roots_are_configured() {
        let (_tmp, _root, child) = root_with("projeto");
        assert_eq!(
            confine_within(child.to_str().unwrap(), &[]),
            Err(ProjectPathError::NoRootsConfigured)
        );
    }

    /// Várias raízes: basta cair sob uma.
    #[test]
    fn accepts_under_any_of_several_roots() {
        let (_tmp_a, root_a, _child_a) = root_with("a");
        let (_tmp_b, root_b, child_b) = root_with("b");
        let got = confine_within(child_b.to_str().unwrap(), &[root_a, root_b])
            .expect("deve aceitar sob a segunda raiz");
        assert_eq!(got, child_b);
    }
}
