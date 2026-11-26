//! The resolved admin master key must open what `admin.db` stores, and the
//! random `master.key` is only minted when nothing is stored (#2013). Split
//! out of `admin/shared.rs`; see the module docs there.

use std::fmt;
use std::path::{Path, PathBuf};

use tracing::warn;

use super::super::store::AdminStore;
use super::{AdminKeyError, MASTER_KEY_LEN};

/// Where the admin master key came from, for [`AdminKeyError::KeyOpensNoStoredSecret`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MasterKeySource {
    /// `GARRAIA_ADMIN_KEY` / `GARRAIA_VAULT_PASSPHRASE` with the recorded
    /// `kdf_params`.
    Passphrase,
    /// The random `<config_dir>/admin/master.key`.
    MasterKeyFile,
}

impl MasterKeySource {
    pub(super) fn likely_cause(self) -> &'static str {
        match self {
            Self::Passphrase => {
                "the passphrase (GARRAIA_ADMIN_KEY / GARRAIA_VAULT_PASSPHRASE) changed after \
                 the secrets were encrypted, or a passphrase was turned on over secrets \
                 encrypted under master.key"
            }
            Self::MasterKeyFile => {
                "<config_dir>/admin/master.key was replaced, or the passphrase the secrets \
                 were encrypted with (GARRAIA_ADMIN_KEY / GARRAIA_VAULT_PASSPHRASE) is no \
                 longer set"
            }
        }
    }

    pub(super) fn fix(self) -> &'static str {
        match self {
            Self::Passphrase => {
                "set back the passphrase (or unset it, to return to master.key) the secrets \
                 were encrypted with"
            }
            Self::MasterKeyFile => {
                "put back the master.key (or set again the passphrase) the secrets were \
                 encrypted with"
            }
        }
    }
}

impl fmt::Display for MasterKeySource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Passphrase => "the admin passphrase and the recorded KDF parameters",
            Self::MasterKeyFile => "<config_dir>/admin/master.key",
        })
    }
}

/// No passphrase configured: a random 32-byte key persisted at
/// `<config_dir>/admin/master.key`. This path never used a salt, so it needs no
/// migration.
///
/// A well-formed file must open the stored secrets ([`check_key_opens_store`]).
/// A missing or wrong-length file is replaced by a new random key **only**
/// while `admin.db` stores no admin secret: then nothing is under the old key
/// and nothing is lost. With secrets stored, minting a key would open none of
/// them and encrypt the next ones under a third key, so the panel goes to 503
/// and the file is left as it is ([`AdminKeyError::MasterKeyFileLost`], #2013).
/// A file that exists but cannot be read (permissions, I/O) is never replaced:
/// that is no proof it is damaged, and the next boot reads it again.
pub(super) fn master_key_file_key(
    store: &AdminStore,
    key_path: &Path,
) -> Result<Vec<u8>, AdminKeyError> {
    let state = match std::fs::read(key_path) {
        Ok(data) if data.len() == MASTER_KEY_LEN => {
            check_key_opens_store(store, &data, MasterKeySource::MasterKeyFile)?;
            return Ok(data);
        }
        Ok(_) => "the wrong length",
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "missing",
        Err(e) => return Err(AdminKeyError::MasterKeyFileUnreadable(e.to_string())),
    };

    let stored = stored_ciphertexts(store).map_err(AdminKeyError::StoredSecretsUnreadable)?;
    if !stored.is_empty() {
        return Err(AdminKeyError::MasterKeyFileLost {
            state,
            count: stored.len(),
        });
    }

    // Chegar aqui com o arquivo EXISTINDO significa que ele tem tamanho
    // errado, e a chave nova logo abaixo vai substitui-lo. Desde o #2013 isso
    // so acontece com o `admin.db` sem nenhum segredo cifrado, entao nada fica
    // para tras; o arquivo antigo ainda e guardado ao lado, sem sobrescrever
    // uma copia preservada antes, para o operador ver o que aconteceu.
    if key_path.exists() {
        let preserved = preserve_aside(key_path);
        let preserved_as = preserved
            .as_deref()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().into_owned());
        warn!(
            preserved = preserved.is_some(),
            preserved_as = preserved_as.as_deref().unwrap_or("-"),
            "admin master.key exists but is {state}; no admin secret is stored, so \
             nothing is lost: generating a new one"
        );
    }

    // Without entropy there is no key to hand back: refuse, like the salt path.
    let key = garraia_security::random_bytes::<MASTER_KEY_LEN>()
        .map_err(|_| AdminKeyError::Entropy("the admin master key"))?
        .to_vec();

    // A key that only lives in memory would encrypt this session's secrets
    // under something the next boot cannot read back: refuse instead.
    write_new_key_file(key_path, &key)
        .map_err(|e| AdminKeyError::MasterKeyFileNotPersisted(e.to_string()))?;
    Ok(key)
}

/// Move a damaged `master.key` aside as `master.key.unreadable`, or
/// `master.key.unreadable.N` when an earlier copy is already there: a copy
/// preserved before is never overwritten. Best-effort; `None` when it stayed.
fn preserve_aside(key_path: &Path) -> Option<PathBuf> {
    (0..100u32).find_map(|n| {
        let aside = if n == 0 {
            key_path.with_extension("key.unreadable")
        } else {
            key_path.with_extension(format!("key.unreadable.{n}"))
        };
        // `hard_link` fails when `aside` exists, unlike `rename`, which would
        // replace it.
        std::fs::hard_link(key_path, &aside).ok()?;
        match std::fs::remove_file(key_path) {
            Ok(()) => Some(aside),
            Err(_) => {
                let _ = std::fs::remove_file(&aside);
                None
            }
        }
    })
}

/// Create `master.key` with `key`, never replacing a file that appeared in the
/// meantime (`create_new`: two gateways booting on the same config dir cannot
/// both win), and on Unix owner-only from the first byte — the file *is* the
/// AES-256-GCM master key, and `fs::write` followed by a `chmod` left it 0o644
/// under the usual umask for a moment. A failure mid-write leaves a file of the
/// wrong length, which the next boot over the same empty store sets aside.
fn write_new_key_file(key_path: &Path, key: &[u8]) -> std::io::Result<()> {
    use std::io::Write;

    if let Some(parent) = key_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(key_path)?;
    file.write_all(key)?;
    // The `open` mode is masked by the umask; this one is not. A umask that
    // strips the owner's read bit would otherwise leave a key the next boot
    // cannot read.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.sync_all()
}

/// One stored admin ciphertext, as `(encrypted, nonce)`.
type Ciphertext = (Vec<u8>, Vec<u8>);

/// Every admin ciphertext in the store: live secrets, archived versions and the
/// panel's 2FA seeds — the same set the re-key moves.
pub(super) fn stored_ciphertexts(store: &AdminStore) -> Result<Vec<Ciphertext>, String> {
    let secrets = store.secret_ciphertexts()?;
    let versions = store.secret_version_ciphertexts()?;
    let totp_secrets = store.totp_secret_ciphertexts()?;
    Ok(secrets
        .into_iter()
        .map(|(_, encrypted, nonce)| (encrypted, nonce))
        .chain(
            versions
                .into_iter()
                .map(|(_, encrypted, nonce)| (encrypted, nonce)),
        )
        .chain(
            totp_secrets
                .into_iter()
                .map(|(_, encrypted, nonce)| (encrypted, nonce)),
        )
        .collect())
}

/// Accept `key` only if the store holds no admin ciphertext or **at least one**
/// opens with it (#2013).
///
/// "At least one", not "all": a single damaged row must not take the whole
/// panel down, and that row answers 500 on its own read. "None" is a different
/// state: the key is not the one the data is under — usually a changed
/// passphrase or a swapped `master.key` — and handing it out would encrypt
/// everything written next under it.
///
/// A read error also refuses: nothing proves the key is the right one, and the
/// next boot checks again.
pub(super) fn check_key_opens_store(
    store: &AdminStore,
    key: &[u8],
    key_source: MasterKeySource,
) -> Result<(), AdminKeyError> {
    let stored = stored_ciphertexts(store).map_err(AdminKeyError::StoredSecretsUnreadable)?;
    if stored.is_empty()
        || stored.iter().any(|(encrypted, nonce)| {
            super::super::secrets::decrypt_value(encrypted, nonce, key).is_ok()
        })
    {
        return Ok(());
    }
    Err(AdminKeyError::KeyOpensNoStoredSecret {
        key_source,
        count: stored.len(),
    })
}
