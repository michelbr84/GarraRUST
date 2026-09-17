//! Permissoes de arquivo e diretorio para material sensivel.
//!
//! # Por que aqui
//!
//! `garraia-config::loader::harden_secret_file` ja existia e ja fazia a metade
//! do trabalho (0600 num arquivo). Faltava a outra metade — **diretorio 0700** —
//! e o unico consumidor novo que precisa das duas, o store de sessao do
//! WhatsApp vinculado, vive em `garraia-channels`, que nao depende (e nao deve
//! passar a depender) de `garraia-config`.
//!
//! `garraia-common` e a crate que todas elas ja linkam, entao a politica passa
//! a morar aqui e o helper de `garraia-config` delega. Uma unica definicao de
//! "o que e modo seguro" evita o cenario classico de duas copias divergirem e
//! uma delas relaxar sem ninguem notar.
//!
//! # O que estas funcoes NAO fazem
//!
//! Elas **apertam**, nunca criam. Quem cria o arquivo e responsavel por
//! escreve-lo; estas funcoes so corrigem o modo depois, porque `std::fs::write`
//! respeita o umask do processo e o umask tipico (`0022`) deixa o arquivo em
//! `0644`. Chamar depois de escrever e a ordem correta: a janela entre criar e
//! apertar existe, e e por isso que o store de sessao escreve o temporario
//! **ja apertado** antes do `rename` (ver `whatsapp_linked::session`).
//!
//! Fora de Unix sao no-op: quem governa o acesso la e a ACL do diretorio pai,
//! e fingir que um `chmod` aconteceu seria pior do que dizer que nao.

use std::io;
use std::path::Path;

/// Modo de arquivo que so o dono le e escreve.
pub const SECRET_FILE_MODE: u32 = 0o600;

/// Modo de diretorio que so o dono atravessa e lista.
pub const SECRET_DIR_MODE: u32 = 0o700;

/// Aperta `path` para `0600` (Unix). No-op fora de Unix.
pub fn harden_secret_file(path: &Path) -> io::Result<()> {
    set_mode(path, SECRET_FILE_MODE)
}

/// Aperta `path` para `0700` (Unix). No-op fora de Unix.
pub fn harden_secret_dir(path: &Path) -> io::Result<()> {
    set_mode(path, SECRET_DIR_MODE)
}

/// Cria `path` (e os pais) e o aperta para `0700`.
///
/// Os pais criados no caminho herdam o umask; so o diretorio folha e apertado,
/// que e o que carrega o material. Isso e deliberado: apertar
/// `~/.config/garraia/data` inteiro quebraria qualquer outro consumidor que o
/// compartilhe com outro usuario do sistema.
pub fn create_secret_dir(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)?;
    harden_secret_dir(path)
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(path: &Path, _mode: u32) -> io::Result<()> {
    // Existencia ainda e verificada: um caminho errado deve falhar aqui, e nao
    // silenciosamente virar sucesso em Windows.
    if path.exists() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("path not found: {}", path.display()),
        ))
    }
}

/// Modo efetivo (`& 0o777`) de `path`, quando a plataforma tem um.
///
/// Existe para os testes poderem afirmar o modo sem repetir o `cfg` em cada
/// arquivo de teste.
pub fn mode_of(path: &Path) -> io::Result<Option<u32>> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Ok(Some(std::fs::metadata(path)?.permissions().mode() & 0o777))
    }
    #[cfg(not(unix))]
    {
        let _ = std::fs::metadata(path)?;
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hardening_a_loose_file_tightens_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("secret");
        std::fs::write(&file, b"x").expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o666))
                .expect("loosen");
            assert_eq!(mode_of(&file).expect("mode"), Some(0o666));
        }

        harden_secret_file(&file).expect("harden");

        if let Some(mode) = mode_of(&file).expect("mode") {
            assert_eq!(mode, 0o600, "arquivo de segredo precisa ficar 0600");
        }
    }

    #[test]
    fn create_secret_dir_is_idempotent_and_owner_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        let nested = dir.path().join("a").join("b");

        create_secret_dir(&nested).expect("first");
        create_secret_dir(&nested).expect("second");

        if let Some(mode) = mode_of(&nested).expect("mode") {
            assert_eq!(mode, 0o700, "diretorio de segredo precisa ficar 0700");
        }
    }

    #[test]
    fn hardening_a_missing_path_is_an_error_on_every_platform() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("nope");
        assert!(harden_secret_file(&missing).is_err());
    }
}
