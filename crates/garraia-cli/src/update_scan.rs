//! `garraia update` trocava so o proprio binario; um `/usr/bin/garraia` de
//! outra instalacao seguia no PATH, intocado e sem aviso (#1030). Este modulo
//! e a varredura pos-update: acha os outros `garraia`/`garra`, pergunta a
//! versao de cada um e diz quem sombreia quem. So imprime — remover e do
//! operador.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::update::strip_v;

/// Nomes com que o GarraIA aparece no PATH: o binario e o alias curto.
pub(crate) const BINARY_NAMES: &[&str] = &["garraia", "garra"];

/// Onde os pacotes de sistema instalam, mesmo que o PATH deste shell nao os
/// liste (o do cron e do systemd lista).
#[cfg(unix)]
const SYSTEM_BIN_DIRS: &[&str] = &["/usr/local/bin", "/usr/bin"];
#[cfg(not(unix))]
const SYSTEM_BIN_DIRS: &[&str] = &[];

/// Um binario do GarraIA no PATH que nao e o que acabou de ser atualizado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtherBinary {
    pub path: PathBuf,
    /// `true` quando o diretorio dele vem ANTES do diretorio do atualizado
    /// no PATH: e ele, nao o atualizado, que `garraia` resolve neste shell.
    pub precedes: bool,
}

/// Procura outros `garraia`/`garra` nos diretorios do PATH e nos de sistema,
/// excluindo o proprio `current` (por caminho canonico, entao um symlink
/// `garra -> garraia` ao lado do atualizado nao e "outro").
///
/// O PATH entra como lista em vez de ser lido aqui, para o teste montar o
/// cenario inteiro num diretorio temporario.
pub(crate) fn find_other_binaries(
    path_dirs: &[PathBuf],
    system_dirs: &[PathBuf],
    current: &Path,
) -> Vec<OtherBinary> {
    let canonical = |p: &Path| fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let current_canon = canonical(current);
    let current_dir = current_canon.parent().map(Path::to_path_buf);
    let current_dir_index = path_dirs
        .iter()
        .position(|d| current_dir.as_deref() == Some(canonical(d).as_path()));

    let names: Vec<String> = BINARY_NAMES
        .iter()
        .map(|n| format!("{n}{}", std::env::consts::EXE_SUFFIX))
        .collect();

    let mut seen: Vec<PathBuf> = vec![current_canon.clone()];
    let mut found = Vec::new();
    let candidates = path_dirs
        .iter()
        .enumerate()
        .map(|(i, d)| (d, Some(i)))
        .chain(system_dirs.iter().map(|d| (d, None)));
    for (dir, index) in candidates {
        for name in &names {
            let candidate = dir.join(name);
            if !candidate.is_file() {
                continue;
            }
            let canon = canonical(&candidate);
            if seen.contains(&canon) {
                continue;
            }
            seen.push(canon);
            let precedes = match (index, current_dir_index) {
                (Some(i), Some(cur)) => i < cur,
                // Fora do PATH deste shell, ou o atualizado fora do PATH:
                // nao ha "antes"; e so um outro binario.
                _ => false,
            };
            found.push(OtherBinary {
                path: candidate,
                precedes,
            });
        }
    }
    found
}

/// A versao que um binario achado no PATH diz ter. Roda `--version` com
/// timeout: e um executavel que o usuario ja tem no PATH com o nome do
/// GarraIA, e um binario quebrado nao pode travar o update.
async fn probe_version(path: &Path) -> Option<String> {
    let out = tokio::time::timeout(
        Duration::from_secs(3),
        tokio::process::Command::new(path).arg("--version").output(),
    )
    .await
    .ok()?
    .ok()?;
    parse_version_output(&String::from_utf8_lossy(&out.stdout))
}

/// `garraia 0.3.4` (ou `garraia v0.3.4`) → `0.3.4`. So a primeira linha.
fn parse_version_output(stdout: &str) -> Option<String> {
    let first = stdout.lines().find(|l| !l.trim().is_empty())?;
    let last = first.split_whitespace().last()?;
    let v = strip_v(last);
    (!v.is_empty() && v.chars().next().is_some_and(|c| c.is_ascii_digit())).then(|| v.to_string())
}

/// O aviso do #1030, pronto para imprimir. `None` quando nao ha o que dizer:
/// nenhum outro binario, ou so copias da mesma versao (inofensivas).
pub(crate) fn shadow_report(
    current: &Path,
    current_version: &str,
    others: &[(OtherBinary, Option<String>)],
) -> Option<String> {
    let mut lines = Vec::new();
    for (other, version) in others {
        if version.as_deref() == Some(current_version) {
            continue;
        }
        let ver = version.as_deref().unwrap_or("versao desconhecida");
        if other.precedes {
            lines.push(format!(
                "⚠ Binario antigo tem precedencia no PATH: {} ({ver}) vem ANTES de {} ({current_version}) — `garraia` continua rodando a versao antiga.",
                other.path.display(),
                current.display()
            ));
        } else {
            lines.push(format!(
                "⚠ Binario antigo detectado: {} ({ver}) sombreado por {} ({current_version}).",
                other.path.display(),
                current.display()
            ));
        }
        lines.push(format!("  Remova com: {}", remove_hint(&other.path)));
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn remove_hint(path: &Path) -> String {
    if cfg!(windows) {
        return format!("Remove-Item \"{}\"", path.display());
    }
    let needs_root = path.starts_with("/usr") || path.starts_with("/opt");
    format!(
        "{}rm {}",
        if needs_root { "sudo " } else { "" },
        path.display()
    )
}

/// PATH real + diretorios de sistema, sondando a versao de cada achado.
pub(crate) async fn report_other_binaries(current: &Path, current_version: &str) -> Option<String> {
    let path_dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    let system_dirs: Vec<PathBuf> = SYSTEM_BIN_DIRS.iter().map(PathBuf::from).collect();
    let others = find_other_binaries(&path_dirs, &system_dirs, current);
    if others.is_empty() {
        return None;
    }
    let mut probed = Vec::with_capacity(others.len());
    for other in others {
        let version = probe_version(&other.path).await;
        probed.push((other, version));
    }
    shadow_report(current, current_version, &probed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch_exe(path: &Path) {
        fs::write(path, b"#!/bin/sh\nexit 0\n").expect("write fake binary");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("chmod");
        }
    }

    /// #1030: o update trocava so o proprio binario e nada dizia sobre o
    /// `/usr/bin/garraia` de outra instalacao. A varredura acha os outros,
    /// pula o proprio (e symlinks para ele) e diz quem vem antes no PATH.
    #[test]
    fn find_other_binaries_skips_itself_and_orders_by_path() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let local = tmp.path().join("local-bin");
        let system = tmp.path().join("usr-bin");
        let other = tmp.path().join("other-bin");
        for d in [&local, &system, &other] {
            fs::create_dir_all(d).expect("mkdir");
        }
        let exe = format!("garraia{}", std::env::consts::EXE_SUFFIX);
        let current = local.join(&exe);
        touch_exe(&current);
        touch_exe(&system.join(&exe));
        touch_exe(&other.join(format!("garra{}", std::env::consts::EXE_SUFFIX)));
        #[cfg(unix)]
        std::os::unix::fs::symlink(&current, local.join("garra")).expect("symlink");

        // PATH: other-bin vem ANTES de local-bin; usr-bin so pela lista de sistema.
        let found = find_other_binaries(
            &[other.clone(), local.clone()],
            std::slice::from_ref(&system),
            &current,
        );

        let paths: Vec<&Path> = found.iter().map(|o| o.path.as_path()).collect();
        assert!(
            !paths
                .iter()
                .any(|p| *p == current || p.ends_with("local-bin/garra")),
            "o proprio (e o symlink para ele) nao sao 'outros': {paths:?}"
        );
        let garra = found
            .iter()
            .find(|o| o.path.starts_with(&other))
            .expect("garra em other-bin");
        assert!(garra.precedes, "other-bin vem antes de local-bin no PATH");
        let sys = found
            .iter()
            .find(|o| o.path.starts_with(&system))
            .expect("garraia em usr-bin");
        assert!(!sys.precedes, "fora do PATH deste shell nao ha 'antes'");
        assert_eq!(found.len(), 2, "{paths:?}");
    }

    #[test]
    fn parse_version_output_reads_clap_style_lines() {
        assert_eq!(
            parse_version_output("garraia 0.3.4\n").as_deref(),
            Some("0.3.4")
        );
        assert_eq!(
            parse_version_output("\ngarra v0.4.0").as_deref(),
            Some("0.4.0")
        );
        assert_eq!(parse_version_output("not a version"), None);
        assert_eq!(parse_version_output(""), None);
    }

    #[test]
    fn shadow_report_names_precedence_and_skips_same_version() {
        let current = Path::new("/home/u/.local/bin/garraia");
        let old = OtherBinary {
            path: PathBuf::from("/usr/bin/garraia"),
            precedes: false,
        };
        let first = OtherBinary {
            path: PathBuf::from("/opt/garra/garra"),
            precedes: true,
        };
        let same = OtherBinary {
            path: PathBuf::from("/tmp/dup/garraia"),
            precedes: true,
        };

        let report = shadow_report(
            current,
            "0.3.9",
            &[
                (old.clone(), Some("0.3.4".into())),
                (first.clone(), None),
                (same, Some("0.3.9".into())),
            ],
        )
        .expect("ha o que avisar");

        assert!(
            report.contains(
                "/usr/bin/garraia (0.3.4) sombreado por /home/u/.local/bin/garraia (0.3.9)"
            ),
            "{report}"
        );
        assert!(report.contains("sudo rm /usr/bin/garraia"), "{report}");
        assert!(
            report.contains("/opt/garra/garra (versao desconhecida) vem ANTES de"),
            "{report}"
        );
        assert!(
            !report.contains("/tmp/dup"),
            "mesma versao nao e problema: {report}"
        );

        // So copias da mesma versao: nada a dizer.
        assert!(shadow_report(current, "0.3.9", &[(old, Some("0.3.9".into()))]).is_none());
    }
}
