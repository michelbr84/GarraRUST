//! Shared admin infrastructure.
//!
//! Slice 9.a of GAR-439 (Q9 of EPIC GAR-430 Quality Gates Phase 3.6).
//! Extracted from `admin/handlers.rs` (3300 LOC) without behavior change.
//! Holds the cross-family `AdminState` value object and the master-key
//! derivation primitive (`derive_encryption_key`) used by the admin
//! sub-router during construction.
//!
//! Future slices (9.b..9.f) will extract per-family handler modules
//! (projects, credentials, channels, mcp_registry, agents) and may
//! later promote response-builder helpers into this module once they
//! have at least one caller migrated.
//!
//! # Master-key derivation (2026-08-29)
//!
//! Until 2026-08-29 `derive_encryption_key` derived the admin-secrets master
//! key with a **constant** PBKDF2 salt (`b"garraia-admin-secrets-v1"`) at
//! 100 000 iterations. Every installation that set `GARRAIA_ADMIN_KEY` or
//! `GARRAIA_VAULT_PASSPHRASE` therefore derived its key from the same salt: a
//! single precomputed table attacks all of them, and two deployments sharing a
//! passphrase shared a master key. CodeQL surfaced it as
//! `rust/hard-coded-cryptographic-value` (security-severity 9.8) once the
//! 2.26.4 Rust extractor started covering the whole workspace.
//!
//! The salt is now random per installation and the iteration count matches the
//! rest of the repo (600 000, as in `admin/store.rs`, `garraia-security`'s
//! `credentials.rs` and `mobile_auth.rs`). The parameters are persisted in the
//! admin store's own `kdf_params` table — same medium as the ciphertexts they
//! describe — and existing installations are migrated forward-only by
//! `migrate_admin_secrets_kdf`, which re-encrypts every stored secret and
//! records the parameters in **one** transaction. If anything fails, nothing is
//! written and the deployment keeps working on the legacy parameters.
//!
//! # What the legacy salt may still do (#1992)
//!
//! Exactly one thing: derive the key of an installation that has **never** been
//! re-keyed, so its ciphertexts can be opened and moved. Until #1992 the
//! derivation also fell back to it in states that are not that one — recorded
//! parameters that cannot be used, a failed salt generation, and (#1999) a
//! missing row over secrets the legacy key does not open — and in all of them
//! it became the key that *encrypts*, with a `warn!` as the only sign.
//!
//! # When there is no safe key (#1992, #1999)
//!
//! None of those states hands out a key any more ([`AdminKeyError`]). The
//! gateway still starts — chat, channels and `/api/*` do not depend on this
//! key — but [`resolve_admin_encryption_key`] returns [`AdminKey::Unavailable`]
//! and `/admin`, page and API, answers 503 until `admin.db` is fixed. The
//! reason goes to the log once, at boot, and never into an HTTP response.
//!
//! # A key must open what is stored (#2013)
//!
//! A usable source is not proof of the right key: a passphrase changed after
//! the re-key, or a `master.key` swapped for another one, derives a key that
//! opens nothing. So whenever admin secrets are stored, the resolved key must
//! open **at least one** of them, and a missing or broken `master.key` is only
//! replaced while nothing is stored. Otherwise the panel goes to 503 the same
//! way.

use std::fmt;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

use super::store::{
    AdminStore, SecretCiphertext, SecretVersionCiphertext, StoredKdfParams, TotpSecretCiphertext,
};
use crate::state::SharedState;

/// AES-256-GCM key length. Also the PBKDF2 output length.
const MASTER_KEY_LEN: usize = 32;
/// PBKDF2 salt length for newly initialised installations.
const KDF_SALT_LEN: usize = 32;
/// Matches `admin/store.rs`, `garraia-security/src/credentials.rs` and
/// `garraia-gateway/src/mobile_auth.rs`. `NonZeroU32` in a `const` context, so
/// a zero would be a compile error rather than a runtime panic — the repo
/// forbids `unwrap()`/`expect()` in production code.
const KDF_ITERATIONS: NonZeroU32 = NonZeroU32::new(600_000).unwrap();
/// Schema version of the stored KDF parameters. Bump when the derivation
/// changes shape.
const KDF_PARAMS_VERSION: u32 = 1;

/// Parameters used before 2026-08-29. Derived from in exactly one state: no
/// KDF parameters recorded yet, i.e. an installation whose ciphertexts may
/// still be under this salt. There the key opens them one last time so
/// [`migrate_admin_secrets_kdf`] can re-encrypt under the per-installation key;
/// if that re-key opens every ciphertext and only its write fails, the process
/// stays on this key (the one the data is under) and the next start retries.
/// Once parameters are recorded, nothing derives from these again — unusable
/// parameters, a failed salt generation and a ciphertext this key does not open
/// leave the admin panel unavailable rather than fall back here (#1992, #1999).
///
/// This is the sole remaining hard-coded cryptographic value in this file and
/// it exists purely so upgrades do not lose data; see
/// `docs/security/codeql-suppressions.md`.
const LEGACY_KDF_SALT: &[u8] = b"garraia-admin-secrets-v1";
const LEGACY_KDF_ITERATIONS: NonZeroU32 = NonZeroU32::new(100_000).unwrap();

/// Floor for an iteration count read back from `kdf_params`. It is the lowest
/// count this code has ever recorded, so a row below it was not written by us.
/// Deliberately **not** tied to [`KDF_ITERATIONS`]: raising that one day must
/// not turn every existing installation's row into a refused boot.
const MIN_STORED_KDF_ITERATIONS: u32 = 600_000;
/// Ceiling for the same count. This code has only ever recorded 600 000; a row
/// above this was not written by us either, and an unbounded count read from
/// an unauthenticated row is a boot that spends hours inside PBKDF2 (`u32::MAX`
/// is ~7 000x the cost of the real count).
const MAX_STORED_KDF_ITERATIONS: u32 = 10_000_000;

/// Why the admin master key could not be resolved. Every variant leaves the
/// admin panel unavailable — the gateway starts, `/admin` answers 503: the
/// alternative in each case was to hand back a key derived from
/// [`LEGACY_KDF_SALT`] and encrypt new secrets under it (#1992, #1999).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AdminKeyError {
    /// `kdf_params` holds a row, so the re-key already committed and every
    /// stored secret is under the key derived from the parameters it recorded.
    /// The row no longer yields that key. The legacy key opens none of those
    /// secrets either, so falling back to it would only downgrade the KDF for
    /// whatever is written next.
    #[error(
        "the admin KDF parameters recorded in admin.db (table `kdf_params`) are unusable: \
         {0}. The admin console stays disabled (HTTP 503): the stored admin secrets are \
         encrypted under the key derived from the original parameters, and deriving a \
         different key would open none of them while encrypting new secrets under weaker \
         parameters. Fix: restore admin.db from a backup taken before the row was damaged. \
         Without the original salt the stored admin secrets cannot be recovered; in that case \
         move admin.db aside to start from an empty admin store, then recreate the admin user \
         and re-enter the provider keys"
    )]
    StoredParamsUnusable(String),
    /// No parameters are recorded, which is what a never-re-keyed installation
    /// looks like — but the stored ciphertexts could not be shown to open with
    /// the legacy key (#1999). Usually one does not open, so the data is under
    /// some other key, and running on the legacy key would encrypt everything
    /// written next under a third one. A ciphertext that could not even be
    /// *read* lands here too: nothing proves the data is under the legacy key,
    /// and the next boot tries again.
    #[error(
        "no admin KDF parameters are recorded in admin.db (table `kdf_params`), and the stored \
         admin secrets could not be shown to be under the legacy key: {0}. If that is a \
         decryption failure, either the `kdf_params` row was removed after the re-key, or the \
         passphrase changed (GARRAIA_ADMIN_KEY / GARRAIA_VAULT_PASSPHRASE, including a \
         passphrase turned on over secrets encrypted under master.key); if it is a read error \
         (database locked, I/O), restart once admin.db is readable. The admin console stays \
         disabled (HTTP 503). Fix: restore admin.db from a backup, or go back to the passphrase \
         (or master.key) the secrets were encrypted with"
    )]
    NotUnderLegacyKey(String),
    /// The key was resolved from a usable source — recorded KDF parameters
    /// plus the passphrase, or a well-formed `master.key` — but admin secrets
    /// are stored and **none** of them opens with it (#2013). The usual cause is
    /// a passphrase changed after the secrets were encrypted, or a `master.key`
    /// swapped for another installation's. Handing the key out would answer 500
    /// on every read (and on the login of every admin with 2FA) while encrypting
    /// whatever is written next under a key the old data is not under.
    ///
    /// "None", not "some": one damaged row must not take the whole panel down.
    #[error(
        "the admin master key derived from {key_source} opens none of the {count} admin \
         secret(s) stored in admin.db (provider keys, archived versions, 2FA seeds). The most \
         likely cause is that {cause}. The admin console stays disabled (HTTP 503) rather \
         than encrypt new secrets under a key the stored ones are not under. Fix: {fix}, or \
         restore admin.db from a backup that matches the current key",
        cause = .key_source.likely_cause(),
        fix = .key_source.fix()
    )]
    KeyOpensNoStoredSecret { key_source: KeySource, count: usize },
    /// No passphrase is set and `<config_dir>/admin/master.key` is missing,
    /// unreadable or the wrong length, while admin secrets are stored (#2013).
    /// Minting a fresh key here — the behaviour before #2013 — opened none of
    /// them and encrypted everything written next under a third key.
    #[error(
        "no admin passphrase is set and <config_dir>/admin/master.key is {state}, but \
         admin.db stores {count} admin secret(s) encrypted under some key. The admin console \
         stays disabled (HTTP 503) and the file was left untouched, rather than replace it \
         with a new random key that opens none of them. Fix: restore master.key from a \
         backup, or set again the passphrase (GARRAIA_ADMIN_KEY / GARRAIA_VAULT_PASSPHRASE) \
         the secrets were encrypted with; without either, follow \"Admin console answering \
         503\" in docs/security.md to start the admin store over"
    )]
    MasterKeyFileLost { state: &'static str, count: usize },
    /// The admin ciphertexts could not be read back, so nothing proves the
    /// resolved key is the one they are under (#2013).
    #[error(
        "the admin secrets stored in admin.db could not be read to check them against \
         the admin master key: {0}. The admin console stays disabled (HTTP 503); nothing \
         was written. Fix: restart the gateway once admin.db is readable (database locked, \
         I/O error)"
    )]
    StoredSecretsUnreadable(String),
    /// The system CSPRNG failed while minting a per-installation salt or the
    /// random `master.key`.
    #[error(
        "the system random number generator failed while generating {0}. The admin console \
         stays disabled (HTTP 503) rather than protect admin secrets with a key that is not \
         unique to this installation. Fix: restore the host's entropy source and restart the \
         gateway"
    )]
    Entropy(&'static str),
}

/// Where the admin master key came from, for [`AdminKeyError::KeyOpensNoStoredSecret`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    /// `GARRAIA_ADMIN_KEY` / `GARRAIA_VAULT_PASSPHRASE` with the recorded
    /// `kdf_params`.
    Passphrase,
    /// The random `<config_dir>/admin/master.key`.
    MasterKeyFile,
}

impl KeySource {
    fn likely_cause(self) -> &'static str {
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

    fn fix(self) -> &'static str {
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

impl fmt::Display for KeySource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Passphrase => "the admin passphrase and the recorded KDF parameters",
            Self::MasterKeyFile => "<config_dir>/admin/master.key",
        })
    }
}

/// Shared state for admin API handlers.
#[derive(Clone)]
pub struct AdminState {
    pub store: Arc<Mutex<AdminStore>>,
    pub app_state: SharedState,
    /// Master encryption key (derived or loaded at startup) for secrets encryption.
    pub encryption_key: Arc<Vec<u8>>,
}

/// The admin master key as the boot resolved it.
#[derive(Clone)]
pub enum AdminKey {
    /// The key every admin encrypt and decrypt uses.
    Ready(Arc<Vec<u8>>),
    /// No key can be handed out without risking the stored secrets
    /// ([`AdminKeyError`]). The gateway still boots; `/admin` and everything
    /// under it answer 503. The reason went to the log at boot and never into
    /// an HTTP response.
    Unavailable,
}

impl From<Arc<Vec<u8>>> for AdminKey {
    fn from(key: Arc<Vec<u8>>) -> Self {
        Self::Ready(key)
    }
}

/// Why [`migrate_admin_secrets_kdf`] did not commit. The split decides whether
/// the legacy key may stay the working key (#1999).
#[derive(Debug)]
enum RekeyError {
    /// A stored ciphertext did not open with the legacy key, could not be read,
    /// or could not be re-encrypted. Nothing proves the data is under the legacy
    /// key, so it cannot be handed out.
    Unverified(String),
    /// Every ciphertext opened with the legacy key and was re-encrypted; only the
    /// write failed. The data is still under the legacy key.
    Deferred(String),
}

impl fmt::Display for RekeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unverified(e) | Self::Deferred(e) => f.write_str(e),
        }
    }
}

/// Persisted PBKDF2 parameters for the admin master key.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct KdfParams {
    version: u32,
    /// Base64 (standard alphabet) of the raw salt bytes.
    salt: String,
    iterations: NonZeroU32,
}

impl KdfParams {
    fn generate() -> Result<Self, String> {
        let salt = garraia_security::random_bytes::<KDF_SALT_LEN>()
            .map_err(|_| "failed to generate KDF salt".to_string())?;
        Ok(Self {
            version: KDF_PARAMS_VERSION,
            salt: BASE64.encode(salt),
            iterations: KDF_ITERATIONS,
        })
    }

    fn salt_bytes(&self) -> Result<Vec<u8>, String> {
        BASE64
            .decode(&self.salt)
            .map_err(|e| format!("the KDF salt is not valid base64: {e}"))
    }
}

/// Outcome of resolving the admin master key at startup.
pub struct AdminKeys {
    /// The key the running process must use for all encrypt operations.
    pub current: Vec<u8>,
    /// Key the stored ciphertexts are currently under, when a re-key is
    /// pending. `None` once the installation is already on per-installation
    /// parameters, or when no passphrase-derived key is in play at all.
    legacy: Option<Vec<u8>>,
    /// Parameters to record **in the same transaction as** a successful re-key.
    /// `None` when there is nothing to commit.
    pending: Option<KdfParams>,
}

impl AdminKeys {
    /// True when [`migrate_admin_secrets_kdf`] still has work to do.
    pub fn migration_pending(&self) -> bool {
        self.legacy.is_some() && self.pending.is_some()
    }
}

/// Infallible by construction: the iteration count is already proven non-zero
/// by its type, so there is no error arm and no `expect()` in the callers.
fn pbkdf2_key(salt: &[u8], iterations: NonZeroU32, passphrase: &str) -> Vec<u8> {
    let mut key = vec![0u8; MASTER_KEY_LEN];
    ring::pbkdf2::derive(
        ring::pbkdf2::PBKDF2_HMAC_SHA256,
        iterations,
        salt,
        passphrase.as_bytes(),
        &mut key,
    );
    key
}

fn admin_dir() -> PathBuf {
    garraia_config::ConfigLoader::default_config_dir().join("admin")
}

/// `GARRAIA_ADMIN_KEY` wins over `GARRAIA_VAULT_PASSPHRASE`, matching the
/// precedence the previous implementation had via its branch order.
fn admin_passphrase() -> Option<String> {
    std::env::var("GARRAIA_ADMIN_KEY")
        .ok()
        .or_else(|| std::env::var("GARRAIA_VAULT_PASSPHRASE").ok())
}

/// Derive the admin master key from a passphrase, reporting whether a re-key is
/// still pending. The passphrase is passed in rather than read from the
/// environment so this can be driven deterministically from tests.
///
/// `stored` is the installation's recorded KDF parameters. Two outcomes, two
/// failures:
///
/// 1. Parameters exist and are usable -> derive with them. Steady state,
///    nothing pending.
/// 2. No parameters exist -> either a fresh install or one still on the legacy
///    constant salt. Generate per-installation parameters, derive both the new
///    and the legacy key, and report the migration as pending. The parameters
///    are recorded only when [`migrate_admin_secrets_kdf`] commits, in the same
///    transaction as the re-encrypted ciphertexts.
/// 3. Parameters exist and are unusable -> [`AdminKeyError::StoredParamsUnusable`].
/// 4. The per-installation salt cannot be generated -> [`AdminKeyError::Entropy`].
///
/// The key that encrypts (`current`) is therefore never derived from
/// [`LEGACY_KDF_SALT`] in this function.
fn derive_with_passphrase(
    stored: Option<StoredKdfParams>,
    passphrase: &str,
) -> Result<AdminKeys, AdminKeyError> {
    if let Some(stored) = stored {
        let (salt, iterations) =
            usable_stored_params(&stored).map_err(AdminKeyError::StoredParamsUnusable)?;
        return Ok(AdminKeys {
            current: pbkdf2_key(&salt, iterations, passphrase),
            legacy: None,
            pending: None,
        });
    }

    let params = KdfParams::generate()
        .map_err(|_| AdminKeyError::Entropy("the per-installation KDF salt"))?;
    // Decoding what `generate` encoded a line ago cannot fail; if it somehow
    // does, there is no per-installation salt to derive from, which is the
    // same failure.
    let salt = params
        .salt_bytes()
        .map_err(|_| AdminKeyError::Entropy("the per-installation KDF salt"))?;
    Ok(AdminKeys {
        current: pbkdf2_key(&salt, params.iterations, passphrase),
        legacy: Some(pbkdf2_key(
            LEGACY_KDF_SALT,
            LEGACY_KDF_ITERATIONS,
            passphrase,
        )),
        pending: Some(params),
    })
}

/// Accept a `kdf_params` row only if it has the shape this code records:
/// a schema version this build reads, a [`KDF_SALT_LEN`]-byte salt, and an
/// iteration count between [`MIN_STORED_KDF_ITERATIONS`] and
/// [`MAX_STORED_KDF_ITERATIONS`].
///
/// Strict on purpose. The row is not authenticated, so whoever can write to
/// `admin.db` can rewrite it; accepting "any base64, any non-zero count" would
/// let that actor pick a one-iteration KDF for every secret written afterwards.
/// A wrong value that still passes here derives a key that opens nothing —
/// loud at the first decrypt — but never a weaker one.
fn usable_stored_params(stored: &StoredKdfParams) -> Result<(Vec<u8>, NonZeroU32), String> {
    let (version, salt_b64, iterations) = stored;
    // Versions this build can read: every one up to its own. Today that is
    // just 1. A newer row was written by a newer build and must not be guessed
    // at; an older one stays readable, so bumping `KDF_PARAMS_VERSION` cannot
    // by itself lock existing installations out — whoever bumps it adds the
    // per-version derivation here.
    if *version == 0 || *version > KDF_PARAMS_VERSION {
        return Err(format!(
            "unknown parameter version {version} (this build reads up to version \
             {KDF_PARAMS_VERSION})"
        ));
    }
    let salt = BASE64
        .decode(salt_b64)
        .map_err(|e| format!("the salt is not valid base64 ({e})"))?;
    if salt.len() != KDF_SALT_LEN {
        return Err(format!(
            "the salt is {} bytes long, expected {KDF_SALT_LEN}",
            salt.len()
        ));
    }
    let iterations = NonZeroU32::new(*iterations)
        .filter(|n| n.get() >= MIN_STORED_KDF_ITERATIONS)
        .ok_or_else(|| {
            format!(
                "the iteration count is {iterations}, below the minimum of \
                 {MIN_STORED_KDF_ITERATIONS} this installation was created with"
            )
        })?;
    if iterations.get() > MAX_STORED_KDF_ITERATIONS {
        return Err(format!(
            "the iteration count is {iterations}, above the maximum of \
             {MAX_STORED_KDF_ITERATIONS} this code accepts"
        ));
    }
    Ok((salt, iterations))
}

/// No passphrase configured: a random 32-byte key persisted at
/// `<config_dir>/admin/master.key`. This path never used a salt, so it needs no
/// migration.
///
/// A well-formed file must open the stored secrets ([`check_key_opens_store`]).
/// A missing, unreadable or wrong-length file is replaced by a new random key
/// **only** while `admin.db` stores no admin secret: then nothing is under the
/// old key and nothing is lost. With secrets stored, minting a key would open
/// none of them and encrypt the next ones under a third key, so the panel goes
/// to 503 and the file is left as it is ([`AdminKeyError::MasterKeyFileLost`],
/// #2013).
fn master_key_file_key(store: &AdminStore, key_path: &Path) -> Result<Vec<u8>, AdminKeyError> {
    let state = match std::fs::read(key_path) {
        Ok(data) if data.len() == MASTER_KEY_LEN => {
            check_key_opens_store(store, &data, KeySource::MasterKeyFile)?;
            return Ok(data);
        }
        Ok(_) => "the wrong length",
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "missing",
        Err(_) => "unreadable",
    };

    let stored = stored_ciphertexts(store).map_err(AdminKeyError::StoredSecretsUnreadable)?;
    if !stored.is_empty() {
        return Err(AdminKeyError::MasterKeyFileLost {
            state,
            count: stored.len(),
        });
    }

    // Chegar aqui com o arquivo EXISTINDO significa que ele esta ilegivel ou
    // com tamanho errado, e a chave nova logo abaixo vai substitui-lo. Desde o
    // #2013 isso so acontece com o `admin.db` sem nenhum segredo cifrado, entao
    // nada fica para tras; o arquivo antigo ainda e guardado ao lado, para o
    // operador ver o que aconteceu.
    if key_path.exists() {
        let preservado = key_path.with_extension("key.unreadable");
        let movido = std::fs::rename(key_path, &preservado).is_ok();
        warn!(
            preserved = movido,
            "admin master.key exists but is {state}; no admin secret is stored, so \
             nothing is lost: generating a new one"
        );
    }

    // Without entropy there is no key to hand back: refuse, like the salt path.
    let key = garraia_security::random_bytes::<MASTER_KEY_LEN>()
        .map_err(|_| AdminKeyError::Entropy("the admin master key"))?
        .to_vec();

    if let Some(parent) = key_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::write(key_path, &key).is_ok() {
        // This file *is* the AES-256-GCM master key. Under the usual 0o022
        // umask `fs::write` would leave it 0o644 — readable by every local
        // user. Narrow it immediately.
        restrict_to_owner(key_path);
    }

    Ok(key)
}

/// One stored admin ciphertext, as `(encrypted, nonce)`.
type Ciphertext = (Vec<u8>, Vec<u8>);

/// Every admin ciphertext in the store: live secrets, archived versions and the
/// panel's 2FA seeds — the same set the re-key moves.
fn stored_ciphertexts(store: &AdminStore) -> Result<Vec<Ciphertext>, String> {
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
fn check_key_opens_store(
    store: &AdminStore,
    key: &[u8],
    key_source: KeySource,
) -> Result<(), AdminKeyError> {
    let stored = stored_ciphertexts(store).map_err(AdminKeyError::StoredSecretsUnreadable)?;
    if stored.is_empty()
        || stored
            .iter()
            .any(|(encrypted, nonce)| super::secrets::decrypt_value(encrypted, nonce, key).is_ok())
    {
        return Ok(());
    }
    Err(AdminKeyError::KeyOpensNoStoredSecret {
        key_source,
        count: stored.len(),
    })
}

/// Best-effort `chmod 0600`. Non-Unix targets have no equivalent and are left
/// to the platform's own ACL defaults.
fn restrict_to_owner(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
            warn!("could not restrict {} to owner-only: {e}", path.display());
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Resolve the master encryption key for the admin secrets store, running the
/// forward-only re-key first when one is pending.
///
/// **This is the only entry point callers should use.** Deriving separately in
/// two places would be a correctness bug, not just waste: with no parameters
/// recorded yet, [`derive_with_passphrase`] generates a *fresh random salt* on
/// every call, so a second, independent derivation yields a different key.
///
/// Key source: `GARRAIA_ADMIN_KEY` / `GARRAIA_VAULT_PASSPHRASE` when set
/// (PBKDF2 with the installation's recorded parameters), otherwise the random
/// `master.key` file, which never used a salt and needs no migration.
///
/// Two different failures, two different answers:
///
/// * the **re-key write** fails after every stored ciphertext opened with the
///   legacy key (disk full, database locked) -> fail-soft. Returns the
///   **legacy** key, the one the data is provably under, and the next boot
///   retries. Nothing was written, so nothing is lost by waiting.
/// * anything else (recorded parameters unusable, no entropy, a ciphertext the
///   legacy key does not open, stored secrets none of which opens with the
///   resolved key, a lost `master.key` over stored secrets) ->
///   [`AdminKey::Unavailable`], logged here once
///   with the reason. The gateway boots and the admin panel answers 503: there
///   is no key the data is under to hand out; see [`AdminKeyError`].
pub fn resolve_admin_encryption_key(store: &mut AdminStore) -> AdminKey {
    match resolve_admin_key(store) {
        Ok(key) => AdminKey::Ready(Arc::new(key)),
        Err(e) => {
            error!(
                "admin panel disabled: {e}. The rest of the gateway keeps running; /admin \
                 answers 503 until this is fixed. See docs/security.md, \"Admin console \
                 answering 503\""
            );
            AdminKey::Unavailable
        }
    }
}

fn resolve_admin_key(store: &mut AdminStore) -> Result<Vec<u8>, AdminKeyError> {
    match admin_passphrase() {
        Some(passphrase) => resolve_with_passphrase(store, &passphrase),
        // The recorded parameters play no part here, so a damaged row must not
        // block an installation that does not derive from a passphrase.
        None => master_key_file_key(store, &admin_dir().join("master.key")),
    }
}

/// The passphrase half of [`resolve_admin_encryption_key`], split out so tests
/// can drive it without touching the process environment.
fn resolve_with_passphrase(
    store: &mut AdminStore,
    passphrase: &str,
) -> Result<Vec<u8>, AdminKeyError> {
    let stored = store
        .kdf_params()
        .map_err(AdminKeyError::StoredParamsUnusable)?;
    let keys = derive_with_passphrase(stored, passphrase)?;
    let (Some(legacy), Some(_)) = (keys.legacy.as_ref(), keys.pending.as_ref()) else {
        // Steady state: the recorded parameters are usable, but that says
        // nothing about the passphrase. A changed one derives a key that opens
        // none of the stored secrets (#2013).
        check_key_opens_store(store, &keys.current, KeySource::Passphrase)?;
        return Ok(keys.current);
    };
    let legacy = legacy.clone();

    match migrate_admin_secrets_kdf(store, &keys) {
        Ok(_) => Ok(keys.current),
        Err(RekeyError::Deferred(e)) => {
            warn!(
                "admin master-key re-key deferred ({e}); staying on the legacy \
                 shared salt, will retry on next start"
            );
            Ok(legacy)
        }
        // No parameters recorded AND a ciphertext the legacy key does not open:
        // the data is under some other key (row removed after the re-key, a new
        // passphrase over old secrets). Staying on the legacy key would encrypt
        // everything written next under a key that is not theirs (#1999).
        Err(RekeyError::Unverified(e)) => Err(AdminKeyError::NotUnderLegacyKey(e)),
    }
}

/// Re-encrypt every stored admin secret under `keys.current`, then persist the
/// new KDF parameters.
///
/// "Every stored secret" includes the panel's TOTP secrets
/// (`admin_users.totp_secret_enc`) since #1141 — they are under the same
/// master key, and a row left behind by a re-key is an owner locked out of
/// the console, not just an unreadable credential.
///
/// Forward-only and fail-closed:
///
/// * every ciphertext is decrypted with the legacy key and re-encrypted with
///   the current one **in memory** first;
/// * the writes land in a single SQLite transaction, so a failure mid-way
///   leaves the store byte-identical;
/// * the new KDF parameters are written to `kdf_params` in that **same**
///   transaction. If the process dies before it commits, the next boot derives
///   the legacy key again and retries — the ciphertexts are still under it.
///
/// Returns the number of rows re-encrypted, or `Ok(0)` when there is nothing to
/// do. A failure before the write is [`RekeyError::Unverified`]; a failure of
/// the write itself, with every ciphertext already opened by the legacy key, is
/// [`RekeyError::Deferred`]. Only the second lets the caller stay on the legacy
/// key (#1999).
fn migrate_admin_secrets_kdf(
    store: &mut AdminStore,
    keys: &AdminKeys,
) -> Result<usize, RekeyError> {
    let (Some(legacy), Some(params)) = (keys.legacy.as_ref(), keys.pending.as_ref()) else {
        return Ok(0);
    };
    // Everything up to the write proves (or fails to prove) that the data is
    // under the legacy key; none of it may leave the process on that key.
    let rekeyed = reencrypt_under_current(store, keys, legacy)
        .map_err(RekeyError::Unverified)
        .and_then(|(new_secrets, new_versions, new_totp)| {
            // Ciphertexts and the parameters that describe them land together or
            // not at all. An earlier revision committed the SQL and then wrote a
            // `kdf.json`; a crash or a failed write in between left the data
            // under a key nothing recorded, and the next boot would mint a third
            // salt and open nothing.
            store
                .apply_secret_rekey(
                    &new_secrets,
                    &new_versions,
                    &new_totp,
                    &(params.version, params.salt.clone(), params.iterations.get()),
                )
                .map_err(RekeyError::Deferred)
        })?;

    if rekeyed > 0 {
        info!(
            rekeyed,
            "admin secrets re-encrypted under a per-installation master key"
        );
    } else {
        info!("admin master key now uses per-installation KDF parameters");
    }
    Ok(rekeyed)
}

/// Rows re-encrypted under `keys.current`, ready for
/// [`AdminStore::apply_secret_rekey`]: live secrets, archived versions and the
/// panel's TOTP secrets.
type ReencryptedRows = (
    Vec<SecretCiphertext>,
    Vec<SecretVersionCiphertext>,
    Vec<TotpSecretCiphertext>,
);

/// Read every admin ciphertext, open it with `legacy` and re-encrypt it under
/// `keys.current`, in memory only. Any `Err` means the store's contents are
/// not provably under `legacy`.
fn reencrypt_under_current(
    store: &AdminStore,
    keys: &AdminKeys,
    legacy: &[u8],
) -> Result<ReencryptedRows, String> {
    let secrets = store.secret_ciphertexts()?;
    let versions = store.secret_version_ciphertexts()?;
    let totp_secrets = store.totp_secret_ciphertexts()?;

    let mut new_secrets = Vec::with_capacity(secrets.len());
    for (id, encrypted, nonce) in &secrets {
        let plaintext = super::secrets::decrypt_value(encrypted, nonce, legacy)
            .map_err(|e| format!("secret {id} does not decrypt with the legacy key: {e}"))?;
        let (re_encrypted, new_nonce) = super::secrets::encrypt_value(&plaintext, &keys.current)
            .map_err(|e| format!("failed to re-encrypt secret {id}: {e}"))?;
        new_secrets.push((id.clone(), re_encrypted, new_nonce));
    }

    let mut new_versions = Vec::with_capacity(versions.len());
    for (id, encrypted, nonce) in &versions {
        let plaintext = super::secrets::decrypt_value(encrypted, nonce, legacy).map_err(|e| {
            format!("secret version {id} does not decrypt with the legacy key: {e}")
        })?;
        let (re_encrypted, new_nonce) = super::secrets::encrypt_value(&plaintext, &keys.current)
            .map_err(|e| format!("failed to re-encrypt secret version {id}: {e}"))?;
        new_versions.push((*id, re_encrypted, new_nonce));
    }

    // The panel's 2FA secret is encrypted under this same master key since
    // #1141, so it re-keys with everything else. Leaving it behind would not
    // lose a provider credential — it would lock the owner out of the console:
    // `get_totp_secret` would return `Err` under the new key and the login
    // would answer 500 forever, with no recovery code to fall back on.
    let mut new_totp = Vec::with_capacity(totp_secrets.len());
    for (user_id, encrypted, nonce) in &totp_secrets {
        let plaintext = super::secrets::decrypt_value(encrypted, nonce, legacy).map_err(|e| {
            format!("totp secret for user {user_id} does not decrypt with the legacy key: {e}")
        })?;
        let (re_encrypted, new_nonce) = super::secrets::encrypt_value(&plaintext, &keys.current)
            .map_err(|e| format!("failed to re-encrypt totp secret for user {user_id}: {e}"))?;
        new_totp.push((user_id.clone(), re_encrypted, new_nonce));
    }

    Ok((new_secrets, new_versions, new_totp))
}

#[cfg(test)]
mod tests;
