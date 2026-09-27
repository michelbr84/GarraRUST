//! Modos do diretorio da sessao: o store os cria certos, e isto os CONFERE.
//!
//! # Por que conferir, se o store ja cria certo
//!
//! Porque "criar certo" so vale para o que o store criou. O diretorio da conta
//! tambem recebe arquivo de fora — o `recusas-lid.json` que o gateway grava
//! (#1345), um `session.enc` restaurado de backup com `cp`, um salt copiado de
//! outra maquina — e nenhum deles passa pelo `OpenOptions::mode` de
//! [`super::session`]. Ate a #1431 o store apertava em silencio so o que
//! tocava (o diretorio e a `session.key`) e deixava o resto como estava: um
//! `session.enc` 0644 seguia legivel por outro usuario do sistema ate a
//! proxima gravacao, e ninguem ficava sabendo que tinha sido.
//!
//! [`SessionStore::harden_modes`] fecha as duas metades: aperta o diretorio da
//! conta para 0700 e **todo** arquivo regular dentro dele para 0600, e devolve
//! o que encontrou mais aberto — para a CLI **avisar**, sem falhar. A regra e
//! por categoria, e nao por nome: o store nao precisa saber que o gateway
//! grava `recusas-lid.json` para que ele saia daqui 0600.
//!
//! # O que fica de fora, de proposito
//!
//! - **Symlink dentro do diretorio.** `chmod` segue o link, e o alvo e de quem
//!   o plantou. Nao se aperta atraves dele, e nao se reporta: o store nunca
//!   cria link nenhum ali.
//! - **Subdiretorio.** O layout nao tem nenhum; nao ha o que exigir dele.
//! - **Fora de Unix.** Nao ha modo POSIX para conferir: quem governa o acesso e
//!   a ACL herdada do diretorio pai (#1253). A lista volta vazia — nunca finge
//!   que apertou.

use std::path::{Path, PathBuf};

use super::session::{SessionError, SessionStore};

/// Um item do diretorio da sessao que estava mais aberto que o modo de segredo.
///
/// So caminho e modo, nunca conteudo: e o que a CLI imprime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LooseMode {
    /// O diretorio da conta, ou um arquivo dentro dele.
    pub path: PathBuf,
    /// `true` quando `path` e o proprio diretorio da conta.
    pub is_dir: bool,
    /// O modo encontrado (`& 0o777`).
    pub found: u32,
    /// O modo exigido: 0700 para o diretorio, 0600 para arquivo.
    pub wanted: u32,
    /// `None` quando o aperto deu certo; a mensagem do erro quando nao deu —
    /// um arquivo de outro dono, por exemplo, deixado por um `sudo garraia`.
    pub error: Option<String>,
}

impl SessionStore {
    /// Aperta o diretorio da conta (0700) e cada arquivo regular dentro dele
    /// (0600), e devolve o que estava mais aberto que isso.
    ///
    /// Diretorio ausente nao e erro e **nao e criado**: quem ainda nao vinculou
    /// nada nao tem modo para conferir. O `Err` fica para o diretorio que
    /// existe e nao se deixa listar — e quem chama avisa, sem falhar.
    pub fn harden_modes(&self) -> Result<Vec<LooseMode>, SessionError> {
        harden_tree(self.dir())
    }
}

/// Algum bit para grupo ou outros? E a unica definicao de "frouxo" daqui:
/// um 0400 e mais fechado que o 0600 exigido, e alarga-lo seria afrouxar em
/// nome do aperto.
#[cfg(unix)]
fn is_loose(mode: u32) -> bool {
    (mode & 0o077) != 0
}

#[cfg(unix)]
fn harden_tree(dir: &Path) -> Result<Vec<LooseMode>, SessionError> {
    use garraia_common::fs_perms::{
        SECRET_DIR_MODE, SECRET_FILE_MODE, harden_secret_dir, harden_secret_file,
    };
    use std::os::unix::fs::PermissionsExt;

    let io = |path: &Path, source: std::io::Error| SessionError::Io {
        path: path.display().to_string(),
        source,
    };

    // O diretorio da conta segue link, como o resto do store
    // (`create_dir_all`, `set_permissions`): um `default` que e link para
    // outro disco e o diretorio da sessao, e e ele que tem de fechar.
    let meta = match std::fs::metadata(dir) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io(dir, e)),
    };
    if !meta.is_dir() {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    // O DIRETORIO PRIMEIRO. Fechado ele, ninguem alem do dono troca uma
    // entrada por um link entre a leitura do modo e o `chmod` abaixo.
    let found = meta.permissions().mode() & 0o777;
    if is_loose(found) {
        out.push(LooseMode {
            path: dir.to_path_buf(),
            is_dir: true,
            found,
            wanted: SECRET_DIR_MODE,
            error: harden_secret_dir(dir).err().map(|e| e.to_string()),
        });
    }

    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| io(dir, e))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    // Ordem de nome: o aviso sai igual em toda execucao, e o teste o fixa.
    entries.sort();
    for path in entries {
        // `symlink_metadata`: o link nao e seguido nem para ler o modo.
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.file_type().is_file() {
            continue;
        }
        let found = meta.permissions().mode() & 0o777;
        if is_loose(found) {
            let error = harden_secret_file(&path).err().map(|e| e.to_string());
            out.push(LooseMode {
                path,
                is_dir: false,
                found,
                wanted: SECRET_FILE_MODE,
                error,
            });
        }
    }
    Ok(out)
}

/// Fora de Unix nao ha modo POSIX para conferir — ver o topo do modulo.
#[cfg(not(unix))]
fn harden_tree(_dir: &Path) -> Result<Vec<LooseMode>, SessionError> {
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::whatsapp_linked::session::{SessionBlob, SessionKey};

    fn blob() -> SessionBlob {
        SessionBlob::new("eyJjcmVkcyI6InBlcm1pc3NvZXMifQ==")
    }

    /// O modo em octal, para a falha dizer `644` e nao `420`.
    #[cfg(unix)]
    fn mode(path: &Path) -> String {
        use std::os::unix::fs::PermissionsExt;
        let bits = std::fs::symlink_metadata(path)
            .unwrap_or_else(|e| panic!("stat {}: {e}", path.display()))
            .permissions()
            .mode()
            & 0o777;
        format!("{bits:o}")
    }

    #[cfg(unix)]
    fn set_mode(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .unwrap_or_else(|e| panic!("chmod {}: {e}", path.display()));
    }

    /// **O aviso e o conserto, juntos.** Um diretorio restaurado de backup
    /// com os modos do umask — o caso real que o store nunca via — sai daqui
    /// fechado, e cada item frouxo volta na lista com o modo que tinha.
    #[cfg(unix)]
    #[test]
    fn loose_modes_are_reported_and_tightened_and_a_second_pass_is_silent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SessionStore::new(dir.path().join("whatsapp").join("default"));
        let key = SessionKey::resolve(store.dir(), None).expect("chave");
        store.save(&blob(), &key).expect("save");
        let recusas = store.dir().join("recusas-lid.json");
        std::fs::write(&recusas, br#"{"pid":1,"recusas":1,"final4":"0000"}"#).expect("recusas");

        set_mode(store.dir(), 0o755);
        set_mode(&store.blob_path(), 0o644);
        set_mode(&store.key_path(), 0o640);
        set_mode(&recusas, 0o666);

        let achados = store.harden_modes().expect("harden");

        let resumo: Vec<String> = achados
            .iter()
            .map(|a| {
                format!(
                    "{} dir={} {:o}->{:o}",
                    a.path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    a.is_dir,
                    a.found,
                    a.wanted,
                )
            })
            .collect();
        assert_eq!(
            resumo,
            vec![
                "default dir=true 755->700",
                "recusas-lid.json dir=false 666->600",
                "session.enc dir=false 644->600",
                "session.key dir=false 640->600",
            ],
            "o diretorio primeiro, depois os arquivos em ordem de nome"
        );
        assert!(
            achados.iter().all(|a| a.error.is_none()),
            "nada aqui era de outro dono: {achados:?}"
        );

        assert_eq!(mode(store.dir()), "700", "diretorio da conta");
        for arquivo in [store.blob_path(), store.key_path(), recusas] {
            assert_eq!(mode(&arquivo), "600", "{}", arquivo.display());
        }

        assert!(
            store.harden_modes().expect("de novo").is_empty(),
            "com tudo fechado nao ha o que avisar — o aviso nao pode virar ruido"
        );
    }

    /// Os modos ja fechados — inclusive os MAIS fechados que o exigido — nao
    /// sao "consertados": 0400 e mais restrito que 0600, e alarga-lo seria
    /// afrouxar em nome do aperto.
    #[cfg(unix)]
    #[test]
    fn a_mode_tighter_than_required_is_left_alone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SessionStore::new(dir.path().join("wa"));
        let key = SessionKey::resolve(store.dir(), None).expect("chave");
        store.save(&blob(), &key).expect("save");
        set_mode(&store.blob_path(), 0o400);

        assert!(store.harden_modes().expect("harden").is_empty());
        assert_eq!(mode(&store.blob_path()), "400");
    }

    /// Um link dentro do diretorio nao e seguido: o `chmod` atravessaria o
    /// link e apertaria — ou falharia em — um arquivo que nao e da sessao.
    #[cfg(unix)]
    #[test]
    fn a_symlink_inside_the_dir_is_neither_followed_nor_reported() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SessionStore::new(dir.path().join("wa"));
        let key = SessionKey::resolve(store.dir(), None).expect("chave");
        store.save(&blob(), &key).expect("save");
        let alheio = dir.path().join("arquivo-de-outra-pessoa");
        std::fs::write(&alheio, b"nao e da sessao").expect("alheio");
        set_mode(&alheio, 0o644);
        std::os::unix::fs::symlink(&alheio, store.dir().join("plantado")).expect("symlink");

        let achados = store.harden_modes().expect("harden");

        assert!(
            achados.iter().all(|a| !a.path.ends_with("plantado")),
            "o link nao entra no relatorio: {achados:?}"
        );
        assert_eq!(mode(&alheio), "644", "e o alvo dele continua como estava");
    }

    /// Quem ainda nao vinculou nada nao ganha diretorio por ter perguntado.
    #[test]
    fn a_missing_dir_is_not_an_error_and_is_not_created() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SessionStore::new(dir.path().join("nunca-vinculado"));

        assert!(store.harden_modes().expect("harden").is_empty());
        assert!(!store.dir().exists(), "conferir modo nao cria diretorio");
    }

    // ── Sob umask permissiva, num processo filho ──────────────────────────
    //
    // A umask e do processo inteiro: mexer nela aqui dentro mudaria o modo
    // de todo arquivo que os outros testes deste binario criam ao mesmo
    // tempo. Entao o teste se re-executa sob `sh -c 'umask 000; exec …'` e o
    // filho roda so o papel abaixo — o mesmo molde de
    // `garraia_common::process_hardening`.

    #[cfg(unix)]
    const PAPEL_UMASK: &str = "GARRAIA_TESTE_1431_UMASK_DIR";

    /// O nome que o harness conhece: `--exact` casa o caminho inteiro, sem o
    /// segmento da crate. Derivado de `module_path!()` para que mover o
    /// modulo nao transforme o teste num filho que roda zero testes.
    #[cfg(unix)]
    fn caminho_do_teste(nome: &str) -> String {
        let modulo = module_path!();
        let sem_crate = modulo.split_once("::").map_or(modulo, |(_, r)| r);
        format!("{sem_crate}::{nome}")
    }

    /// **A umask so pode fechar, e aqui ela nao fecha nada.** Com `umask 000`
    /// um `create_dir_all` sai 0777 e um `fs::write` sai 0666: tudo o que o
    /// store cria tem de sair 0700/0600 mesmo assim, e o arquivo que ele NAO
    /// criou (o `recusas-lid.json` do gateway, escrito cru) tem de sair 0600
    /// do `harden_modes`, reportado.
    #[cfg(unix)]
    #[test]
    fn under_a_permissive_umask_the_final_modes_are_still_0700_and_0600() {
        let dir = tempfile::tempdir().expect("tempdir");
        let exe = std::env::current_exe().expect("binario de teste");
        let out = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg("umask 000 && exec \"$0\" \"$@\"")
            .arg(exe)
            .args([
                caminho_do_teste("papel_umask_permissiva").as_str(),
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(PAPEL_UMASK, dir.path())
            .output()
            .expect("re-executar o binario de teste");
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            out.status.success(),
            "o papel falhou sob umask 000:\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        assert!(
            stdout.contains("PAPEL_UMASK_OK"),
            "o filho nao rodou o papel — sem ele este teste nao provou nada:\n{stdout}\n{stderr}"
        );
    }

    /// O papel: so faz alguma coisa re-executado pelo teste acima.
    #[cfg(unix)]
    #[test]
    fn papel_umask_permissiva() {
        let Some(raiz) = std::env::var_os(PAPEL_UMASK) else {
            return;
        };
        let raiz = PathBuf::from(raiz);

        // Prova de que a umask chegou mesmo: sem isto, um `sh` que ignorasse o
        // `umask 000` deixaria o teste verde pelo motivo errado.
        let sonda = raiz.join("sonda");
        std::fs::write(&sonda, b"x").expect("sonda");
        assert_eq!(mode(&sonda), "666", "o filho nao esta sob umask 000");

        let store = SessionStore::new(raiz.join("whatsapp").join("default"));
        let key = SessionKey::resolve(store.dir(), None).expect("chave");
        store.save(&blob(), &key).expect("save");
        assert!(store.archive().expect("archive"));
        store.save(&blob(), &key).expect("save de novo");

        let com_passphrase = SessionStore::new(raiz.join("whatsapp").join("cofre"));
        let chave_do_cofre =
            SessionKey::resolve(com_passphrase.dir(), Some("sintetica-1431")).expect("chave");
        com_passphrase.save(&blob(), &chave_do_cofre).expect("save");

        for criado in [store.dir(), com_passphrase.dir()] {
            assert_eq!(mode(criado), "700", "{}", criado.display());
        }
        for criado in [
            store.blob_path(),
            store.archive_path(),
            store.key_path(),
            com_passphrase.blob_path(),
            com_passphrase.salt_path(),
        ] {
            assert_eq!(mode(&criado), "600", "{}", criado.display());
        }

        // O que o store NAO criou: escrito cru, como qualquer processo faria.
        let recusas = store.dir().join("recusas-lid.json");
        std::fs::write(&recusas, b"{}").expect("recusas");
        assert_eq!(mode(&recusas), "666", "nasce com o que a umask deixou");

        let achados = store.harden_modes().expect("harden");

        assert_eq!(mode(&recusas), "600", "e sai 0600 do harden_modes");
        assert!(
            achados
                .iter()
                .any(|a| a.path == recusas && a.found == 0o666 && a.wanted == 0o600),
            "e o aperto e reportado: {achados:?}"
        );
        println!("PAPEL_UMASK_OK");
    }
}
