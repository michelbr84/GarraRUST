//! Check `binary.mismatch` do `garraia doctor` (#1649): um binario antigo
//! antes no `PATH` responde no lugar do recem-instalado, em silencio.
//!
//! O doctor reporta a versao do binario que esta rodando; sem olhar o resto do
//! `PATH` ele nao tem como saber que existe outra copia, de outra versao, que
//! o pacote (`.deb`) acabou de instalar. Aqui listamos as outras copias de
//! `garraia`/`garra` no `PATH`, perguntamos a versao delas (`--version`, com
//! timeout) e avisamos quando divergem. Diagnostico apenas: nao afeta o exit
//! code do doctor.

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;

const NOMES: [&str; 2] = ["garraia", "garra"];
const TIMEOUT_VERSION: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct OutroBinario {
    pub path: String,
    /// `None` quando o `--version` falhou ou estourou o timeout.
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct BinaryCheck {
    pub running_path: Option<String>,
    pub running_version: &'static str,
    /// Outras copias no `PATH` (caminho canonico distinto do executavel atual).
    pub others: Vec<OutroBinario>,
    /// `true` quando alguma outra copia reporta versao diferente da atual.
    pub mismatch: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_step: Option<String>,
}

/// Extrai a versao de `garra 0.4.8` (ultima palavra da primeira linha).
pub(crate) fn parse_version(stdout: &str) -> Option<String> {
    let v = stdout.lines().next()?.split_whitespace().last()?;
    v.chars().next().filter(char::is_ascii_digit)?;
    Some(v.to_string())
}

/// Parte pura: classifica as copias encontradas contra a versao em execucao.
pub(crate) fn classify(
    running_path: Option<&Path>,
    running_version: &'static str,
    others: Vec<(PathBuf, Option<String>)>,
) -> BinaryCheck {
    let others: Vec<OutroBinario> = others
        .into_iter()
        .map(|(p, version)| OutroBinario {
            path: p.display().to_string(),
            version,
        })
        .collect();
    let mismatch = others
        .iter()
        .any(|o| o.version.as_deref().is_some_and(|v| v != running_version));
    let next_step = mismatch.then(|| {
        "ha outra copia no PATH com versao diferente: remova a antiga, ajuste a ordem do PATH \
         ou rode `hash -r` para o shell reler o PATH"
            .to_string()
    });
    BinaryCheck {
        running_path: running_path.map(|p| p.display().to_string()),
        running_version,
        others,
        mismatch,
        next_step,
    }
}

#[cfg(unix)]
fn executavel(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn executavel(p: &Path) -> bool {
    p.is_file()
}

/// Copias de `garraia`/`garra` no `PATH` diferentes do executavel atual.
fn candidatos(path_env: &str, atual: Option<&Path>) -> Vec<PathBuf> {
    let atual = atual.and_then(|p| p.canonicalize().ok());
    let mut vistos: HashSet<PathBuf> = HashSet::new();
    let mut out = Vec::new();
    for dir in std::env::split_paths(path_env) {
        for nome in NOMES {
            let mut cand = dir.join(nome);
            if cfg!(windows) {
                cand.set_extension("exe");
            }
            if !executavel(&cand) {
                continue;
            }
            let Ok(canon) = cand.canonicalize() else {
                continue;
            };
            if Some(&canon) == atual.as_ref() || !vistos.insert(canon) {
                continue;
            }
            out.push(cand);
        }
    }
    out
}

/// `--version` com timeout; `None` em qualquer falha.
fn versao_de(bin: &Path) -> Option<String> {
    let mut child = Command::new(bin)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let inicio = Instant::now();
    loop {
        match child.try_wait().ok()? {
            Some(status) if status.success() => break,
            Some(_) => return None,
            None if inicio.elapsed() > TIMEOUT_VERSION => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    let mut buf = String::new();
    child
        .stdout
        .take()?
        .take(512)
        .read_to_string(&mut buf)
        .ok()?;
    parse_version(&buf)
}

/// Coleta efetiva (I/O): le o `PATH` e consulta cada copia.
pub(crate) fn collect(path_env: Option<&str>) -> BinaryCheck {
    let atual = std::env::current_exe().ok();
    let others = path_env
        .map(|p| candidatos(p, atual.as_deref()))
        .unwrap_or_default()
        .into_iter()
        .map(|p| {
            let v = versao_de(&p);
            (p, v)
        })
        .collect();
    classify(atual.as_deref(), env!("CARGO_PKG_VERSION"), others)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_le_a_ultima_palavra() {
        assert_eq!(parse_version("garra 0.4.8\n").as_deref(), Some("0.4.8"));
        assert_eq!(parse_version("").as_deref(), None);
        assert_eq!(parse_version("erro: nao achei\n").as_deref(), None);
    }

    #[test]
    fn versao_divergente_vira_mismatch_com_next_step() {
        let c = classify(
            Some(Path::new("/usr/bin/garraia")),
            "0.4.9",
            vec![(
                PathBuf::from("/home/u/.local/bin/garraia"),
                Some("0.4.8".into()),
            )],
        );
        assert!(c.mismatch);
        assert!(c.next_step.is_some());
        assert_eq!(c.others[0].path, "/home/u/.local/bin/garraia");
    }

    #[test]
    fn mesma_versao_ou_desconhecida_nao_alerta() {
        let c = classify(
            None,
            "0.4.9",
            vec![
                (PathBuf::from("/a/garra"), Some("0.4.9".into())),
                (PathBuf::from("/b/garra"), None),
            ],
        );
        assert!(!c.mismatch);
        assert!(c.next_step.is_none());
        assert!(!classify(None, "0.4.9", vec![]).mismatch);
    }

    #[cfg(unix)]
    #[test]
    fn candidatos_acha_copia_executavel_e_ignora_sem_bit_x_e_a_propria() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let velho = a.join("garraia");
        let sem_x = b.join("garraia");
        std::fs::write(&velho, "#!/bin/sh\necho 'garra 0.0.1'\n").unwrap();
        std::fs::write(&sem_x, "x").unwrap();
        std::fs::set_permissions(&velho, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(&sem_x, std::fs::Permissions::from_mode(0o644)).unwrap();

        let path = std::env::join_paths([&a, &b, &a]).unwrap();
        let path = path.to_str().unwrap();
        assert_eq!(candidatos(path, None), vec![velho.clone()]);
        assert!(candidatos(path, Some(&velho)).is_empty());
        assert_eq!(versao_de(&velho).as_deref(), Some("0.0.1"));
    }
}
