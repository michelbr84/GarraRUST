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
//! derivation also fell back to it in two states that are not that one —
//! recorded parameters that cannot be used, and a failed salt generation — and
//! in both it became the key that *encrypts*, with a `warn!` as the only sign.
//! Both now refuse to start ([`AdminKeyError`]) instead.

use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use tokio::sync::Mutex;
use tracing::{info, warn};

use super::store::{AdminStore, StoredKdfParams};
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
/// if that re-key fails, the process stays on this key (the one the data is
/// under) and the next start retries. Once parameters are recorded, nothing
/// derives from these again — unusable parameters and a failed salt generation
/// refuse to start rather than fall back here (#1992).
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

/// Why the admin master key could not be resolved. Every variant refuses the
/// boot: the alternative in each case was to hand back a key derived from
/// [`LEGACY_KDF_SALT`] and encrypt new secrets under it (#1992).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AdminKeyError {
    /// `kdf_params` holds a row, so the re-key already committed and every
    /// stored secret is under the key derived from the parameters it recorded.
    /// The row no longer yields that key. The legacy key opens none of those
    /// secrets either, so falling back to it would only downgrade the KDF for
    /// whatever is written next.
    #[error(
        "the admin KDF parameters recorded in admin.db (table `kdf_params`) are unusable: \
         {0}. Refusing to start: the stored admin secrets are encrypted under the key derived \
         from the original parameters, and deriving a different key would open none of them \
         while encrypting new secrets under weaker parameters. Fix: restore admin.db from a \
         backup taken before the row was damaged. Without the original salt the stored admin \
         secrets cannot be recovered; in that case move admin.db aside to start from an empty \
         admin store, then recreate the admin user and re-enter the provider keys"
    )]
    StoredParamsUnusable(String),
    /// The system CSPRNG failed while minting a per-installation salt or the
    /// random `master.key`.
    #[error(
        "the system random number generator failed while generating {0}. Refusing to start \
         rather than protect admin secrets with a key that is not unique to this installation. \
         Fix: restore the host's entropy source and start again"
    )]
    Entropy(&'static str),
}

/// Shared state for admin API handlers.
#[derive(Clone)]
pub struct AdminState {
    pub store: Arc<Mutex<AdminStore>>,
    pub app_state: SharedState,
    /// Master encryption key (derived or loaded at startup) for secrets encryption.
    pub encryption_key: Arc<Vec<u8>>,
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
            .map_err(|e| format!("kdf.json has a malformed salt: {e}"))
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
/// refusals:
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
    // same refusal.
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
/// the current schema version, a [`KDF_SALT_LEN`]-byte salt, and at least
/// [`MIN_STORED_KDF_ITERATIONS`] iterations.
///
/// Strict on purpose. The row is not authenticated, so whoever can write to
/// `admin.db` can rewrite it; accepting "any base64, any non-zero count" would
/// let that actor pick a one-iteration KDF for every secret written afterwards.
/// A wrong value that still passes here derives a key that opens nothing —
/// loud at the first decrypt — but never a weaker one.
fn usable_stored_params(stored: &StoredKdfParams) -> Result<(Vec<u8>, NonZeroU32), String> {
    let (version, salt_b64, iterations) = stored;
    if *version != KDF_PARAMS_VERSION {
        return Err(format!(
            "unknown parameter version {version} (this build reads version {KDF_PARAMS_VERSION})"
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
    if *iterations < MIN_STORED_KDF_ITERATIONS {
        return Err(format!(
            "the iteration count is {iterations}, below the minimum of \
             {MIN_STORED_KDF_ITERATIONS} this installation was created with"
        ));
    }
    let iterations =
        NonZeroU32::new(*iterations).ok_or_else(|| "the iteration count is zero".to_string())?;
    Ok((salt, iterations))
}

/// No passphrase configured: a random 32-byte key persisted at
/// `<config_dir>/admin/master.key`. This path never used a salt, so it needs no
/// migration.
fn master_key_file_keys() -> Result<AdminKeys, AdminKeyError> {
    let key_path = admin_dir().join("master.key");

    if let Ok(data) = std::fs::read(&key_path)
        && data.len() == MASTER_KEY_LEN
    {
        return Ok(AdminKeys {
            current: data,
            legacy: None,
            pending: None,
        });
    }

    // Chegar aqui com o arquivo EXISTINDO significa que ele esta ilegivel ou
    // com tamanho errado, e a chave nova logo abaixo vai substitui-lo. Antes
    // do #1141 isso custava as chaves de provider; agora custa tambem o
    // segredo do 2FA de todo admin que tem 2FA ligado — o login passa a
    // responder 500 e o dono so volta pelo break-glass de `docs/security.md`.
    // Guardar o arquivo antigo em vez de sobrescrever nao recupera a chave
    // (32 bytes truncados nao voltam), mas tira o "silenciosamente": o
    // operador ve o aviso e encontra o arquivo preservado ao lado.
    if key_path.exists() {
        let preservado = key_path.with_extension("key.unreadable");
        let movido = std::fs::rename(&key_path, &preservado).is_ok();
        warn!(
            preserved = movido,
            "admin master.key exists but is unreadable or has the wrong length; \
             generating a new one. Every secret encrypted under the old key — \
             provider keys AND admin 2FA secrets — becomes unreadable. Admins \
             with 2FA will get HTTP 500 on login until the second factor is \
             cleared; see docs/security.md"
        );
    }

    // Without entropy there is no key to hand back: refuse, like the salt path.
    let key = garraia_security::random_bytes::<MASTER_KEY_LEN>()
        .map_err(|_| AdminKeyError::Entropy("the admin master key"))?
        .to_vec();

    if let Some(parent) = key_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::write(&key_path, &key).is_ok() {
        // This file *is* the AES-256-GCM master key. Under the usual 0o022
        // umask `fs::write` would leave it 0o644 — readable by every local
        // user. Narrow it immediately.
        restrict_to_owner(&key_path);
    }

    Ok(AdminKeys {
        current: key,
        legacy: None,
        pending: None,
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
/// * the **re-key** fails (a ciphertext does not open with the legacy key, the
///   transaction does not commit) -> fail-soft. Returns the **legacy** key, the
///   one the stored ciphertexts are actually under, and the next boot retries.
///   Nothing was written, so nothing is lost by waiting.
/// * the key **cannot be resolved** at all (recorded parameters unusable, no
///   entropy) -> `Err`, and the caller refuses to start. There is no key the
///   data is under to fall back to; see [`AdminKeyError`].
pub fn resolve_admin_encryption_key(store: &mut AdminStore) -> Result<Vec<u8>, AdminKeyError> {
    match admin_passphrase() {
        Some(passphrase) => resolve_with_passphrase(store, &passphrase),
        // The recorded parameters play no part here, so a damaged row must not
        // block an installation that does not derive from a passphrase.
        None => Ok(master_key_file_keys()?.current),
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
        return Ok(keys.current);
    };
    let legacy = legacy.clone();

    match migrate_admin_secrets_kdf(store, &keys) {
        Ok(_) => Ok(keys.current),
        Err(e) => {
            warn!(
                "admin master-key re-key deferred ({e}); staying on the legacy \
                 shared salt, will retry on next start"
            );
            Ok(legacy)
        }
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
/// * `kdf.json` is written **last**. If the process dies before that, the next
///   boot simply derives the legacy key again and retries — the ciphertexts are
///   still under the legacy key because the transaction never committed.
///
/// Returns the number of rows re-encrypted, or `Ok(0)` when there is nothing to
/// do. Errors are reported to the caller, which logs and carries on with the
/// legacy key rather than refusing to boot.
fn migrate_admin_secrets_kdf(store: &mut AdminStore, keys: &AdminKeys) -> Result<usize, String> {
    let (Some(legacy), Some(params)) = (keys.legacy.as_ref(), keys.pending.as_ref()) else {
        return Ok(0);
    };

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

    // Ciphertexts and the parameters that describe them land together or not at
    // all. An earlier revision committed the SQL and then wrote a `kdf.json`;
    // a crash or a failed write in between left the data under a key nothing
    // recorded, and the next boot would mint a third salt and open nothing.
    let rekeyed = store.apply_secret_rekey(
        &new_secrets,
        &new_versions,
        &new_totp,
        &(params.version, params.salt.clone(), params.iterations.get()),
    )?;

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

#[cfg(test)]
mod tests {
    use super::*;

    const PASS: &str = "correct horse battery staple";

    fn params_of(keys: &AdminKeys) -> StoredKdfParams {
        let p = keys.pending.as_ref().expect("pending params");
        (p.version, p.salt.clone(), p.iterations.get())
    }

    #[test]
    fn generated_salts_differ_between_installations() {
        let a = KdfParams::generate().expect("a");
        let b = KdfParams::generate().expect("b");
        assert_ne!(a.salt, b.salt, "two installations must not share a salt");
        assert_eq!(a.salt_bytes().expect("decode").len(), KDF_SALT_LEN);

        // The regression this whole change exists for: same passphrase, two
        // installations, two different master keys.
        let key_a = pbkdf2_key(&a.salt_bytes().unwrap(), a.iterations, PASS);
        let key_b = pbkdf2_key(&b.salt_bytes().unwrap(), b.iterations, PASS);
        assert_ne!(key_a, key_b);
    }

    #[test]
    fn new_parameters_differ_from_legacy() {
        // Compile-time, so weakening them fails the build rather than a test.
        const _: () = assert!(KDF_ITERATIONS.get() >= 600_000);
        const _: () = assert!(KDF_ITERATIONS.get() != LEGACY_KDF_ITERATIONS.get());
        const _: () = assert!(KDF_SALT_LEN == 32);
        assert_ne!(KDF_ITERATIONS, LEGACY_KDF_ITERATIONS);
    }

    #[test]
    fn stored_params_reproduce_the_same_key_every_boot() {
        // The bug the single-resolution-point design exists to prevent: with no
        // params recorded, each derivation mints a fresh salt. Once they ARE
        // recorded, every later boot must land on the exact same key.
        let first = derive_with_passphrase(None, PASS).expect("first boot");
        assert!(first.migration_pending());
        let recorded = params_of(&first);

        let second = derive_with_passphrase(Some(recorded.clone()), PASS).expect("second boot");
        let third = derive_with_passphrase(Some(recorded), PASS).expect("third boot");
        assert_eq!(first.current, second.current);
        assert_eq!(second.current, third.current);
        assert!(!second.migration_pending(), "nothing left to migrate");
    }

    // ── #1992: nothing but a never-re-keyed install derives from the legacy salt ──

    /// A `kdf_params` row as this code records it, for tests that damage one
    /// field at a time.
    fn good_row() -> StoredKdfParams {
        (
            KDF_PARAMS_VERSION,
            BASE64.encode([7u8; KDF_SALT_LEN]),
            KDF_ITERATIONS.get(),
        )
    }

    /// Before #1992 every one of these rows made `current` the key derived from
    /// `LEGACY_KDF_SALT` — the key that encrypts — with `legacy: None` and
    /// `pending: None`, so no migration was on its way either. A recorded row
    /// means the re-key committed and the secrets are under the NEW key, so the
    /// legacy key opened none of them: the fallback bought nothing and cost a
    /// KDF downgrade on every secret written afterwards.
    #[test]
    fn unusable_stored_params_refuse_instead_of_falling_back_to_the_legacy_key() {
        let (version, salt, iterations) = good_row();
        let cases = [
            (
                (version, "!!!not base64!!!".to_string(), iterations),
                "not valid base64",
            ),
            ((version, salt.clone(), 0), "iteration count is 0"),
            // Well-formed, and exactly what a KDF downgrade through a writable
            // admin.db looks like.
            ((version, salt.clone(), 1), "iteration count is 1"),
            (
                (version, salt.clone(), LEGACY_KDF_ITERATIONS.get()),
                "below the minimum",
            ),
            (
                (version, BASE64.encode([7u8; 8]), iterations),
                "8 bytes long",
            ),
            ((version, String::new(), iterations), "0 bytes long"),
            (
                (version + 1, salt.clone(), iterations),
                "unknown parameter version",
            ),
        ];

        for (bad, reason) in cases {
            let err = match derive_with_passphrase(Some(bad.clone()), PASS) {
                Ok(_) => panic!("{bad:?} must not yield a key"),
                Err(e) => e,
            };
            assert!(
                matches!(err, AdminKeyError::StoredParamsUnusable(_)),
                "{bad:?} -> {err:?}"
            );
            let msg = err.to_string();
            assert!(msg.contains(reason), "{bad:?}: {msg}");
            // Actionable: names the table, says it refuses, says what to do.
            assert!(msg.contains("kdf_params"), "{msg}");
            assert!(msg.contains("Refusing to start"), "{msg}");
            assert!(msg.contains("restore admin.db from a backup"), "{msg}");
        }
    }

    /// The guard above must not reject what this very code records.
    #[test]
    fn a_row_as_recorded_by_the_rekey_is_usable() {
        const _: () = assert!(KDF_ITERATIONS.get() >= MIN_STORED_KDF_ITERATIONS);
        const _: () = assert!(LEGACY_KDF_ITERATIONS.get() < MIN_STORED_KDF_ITERATIONS);

        let keys = derive_with_passphrase(Some(good_row()), PASS).expect("usable");
        assert!(!keys.migration_pending());
        assert_ne!(
            keys.current,
            pbkdf2_key(LEGACY_KDF_SALT, LEGACY_KDF_ITERATIONS, PASS)
        );

        let fresh = params_of(&derive_with_passphrase(None, PASS).expect("fresh"));
        assert!(usable_stored_params(&fresh).is_ok(), "{fresh:?}");
    }

    /// The property the issue is about, stated directly: whatever the input,
    /// the key that ENCRYPTS is never the one derived from the constant salt.
    #[test]
    fn the_encrypting_key_is_never_derived_from_the_legacy_salt() {
        let legacy = pbkdf2_key(LEGACY_KDF_SALT, LEGACY_KDF_ITERATIONS, PASS);
        let (version, salt, iterations) = good_row();

        for stored in [
            None,
            Some(good_row()),
            Some((version, "!!!".to_string(), iterations)),
            Some((version, salt, 0)),
        ] {
            if let Ok(keys) = derive_with_passphrase(stored, PASS) {
                assert_ne!(keys.current, legacy);
            }
        }
    }

    /// End to end through the boot entry point: a damaged row refuses, and the
    /// refusal writes nothing — neither the row nor the secret it describes.
    #[test]
    fn resolve_refuses_a_damaged_row_and_leaves_the_store_untouched() {
        let mut store = AdminStore::in_memory().expect("in-memory store");

        // A healthy, re-keyed installation holding one secret.
        let key = resolve_with_passphrase(&mut store, PASS).expect("first boot");
        let recorded = store.kdf_params().expect("readable").expect("recorded");
        let (ciphertext, nonce) =
            super::super::secrets::encrypt_value(b"sk-live-do-not-leak", &key).expect("encrypt");
        store
            .set_secret("default", "openai", "api_key", &ciphertext, &nonce, None)
            .expect("seed secret");
        assert_eq!(
            resolve_with_passphrase(&mut store, PASS).expect("second boot"),
            key,
            "steady state must land on the same key"
        );

        // The row is damaged (or tampered with): same salt, one iteration.
        let damaged = (recorded.0, recorded.1.clone(), 1u32);
        store
            .apply_secret_rekey(&[], &[], &[], &damaged)
            .expect("write the damaged row");

        let err = resolve_with_passphrase(&mut store, PASS).expect_err("must refuse");
        assert!(
            matches!(err, AdminKeyError::StoredParamsUnusable(_)),
            "{err:?}"
        );
        assert_eq!(
            store.kdf_params().expect("readable"),
            Some(damaged),
            "a refusal must not rewrite the row it refused"
        );
        let (stored, stored_nonce) = store
            .get_secret_raw("default", "openai", "api_key")
            .expect("secret still present");
        assert_eq!(stored, ciphertext, "ciphertext must be byte-identical");
        assert_eq!(stored_nonce, nonce);

        // And putting the row back is all it takes.
        store
            .apply_secret_rekey(&[], &[], &[], &recorded)
            .expect("restore the row");
        assert_eq!(
            resolve_with_passphrase(&mut store, PASS).expect("boots again"),
            key
        );
    }

    // ── Forward-only re-key migration ────────────────────────────────────

    fn pending_keys(passphrase: &str) -> AdminKeys {
        derive_with_passphrase(None, passphrase).expect("derive")
    }

    #[test]
    fn migration_reencrypts_secrets_and_records_params_atomically() {
        let mut store = AdminStore::in_memory().expect("in-memory store");
        let keys = pending_keys(PASS);
        assert!(keys.migration_pending());
        assert!(
            store.kdf_params().expect("readable").is_none(),
            "nothing recorded yet"
        );

        // A secret as it exists on a legacy install: under the key derived from
        // the shared constant salt.
        let legacy = keys.legacy.clone().expect("legacy key");
        let (ciphertext, nonce) =
            super::super::secrets::encrypt_value(b"sk-live-do-not-leak", &legacy).expect("encrypt");
        store
            .set_secret("default", "openai", "api_key", &ciphertext, &nonce, None)
            .expect("seed secret");

        let rekeyed = migrate_admin_secrets_kdf(&mut store, &keys).expect("migration");
        assert_eq!(rekeyed, 1, "one live secret, no archived versions yet");

        let (stored, stored_nonce) = store
            .get_secret_raw("default", "openai", "api_key")
            .expect("secret still present");
        let plaintext = super::super::secrets::decrypt_value(&stored, &stored_nonce, &keys.current)
            .expect("must decrypt with the new key");
        assert_eq!(plaintext, b"sk-live-do-not-leak");
        assert!(
            super::super::secrets::decrypt_value(&stored, &stored_nonce, &legacy).is_err(),
            "the legacy key must no longer open the secret"
        );

        // Params landed in the SAME transaction, so the next boot rederives the
        // very key the ciphertexts are now under.
        let recorded = store
            .kdf_params()
            .expect("readable")
            .expect("params recorded");
        let next_boot = derive_with_passphrase(Some(recorded), PASS).expect("next boot");
        assert_eq!(next_boot.current, keys.current);
        assert!(!next_boot.migration_pending());
    }

    // ── #1141: o segredo do 2FA entra no re-key da chave mestra ──────────

    /// O buraco que a revisao deste PR pegou: cifrar o segredo do 2FA criou
    /// uma dependencia da chave mestra que a rotacao de KDF nao conhecia. Se
    /// o re-key re-cifrasse so `secrets`/`secret_versions`, o segredo do
    /// segundo fator ficaria sob a chave antiga enquanto o processo passa a
    /// usar a nova — `get_totp_secret` viraria `Err` e o login do dono
    /// responderia 500 para sempre, sem recovery code para sair.
    #[test]
    fn migration_reencrypts_the_admin_totp_secret_too() {
        let mut store = AdminStore::in_memory().expect("in-memory store");
        let keys = pending_keys(PASS);
        let legacy = keys.legacy.clone().expect("legacy key");
        let user = store
            .create_user("dono", "senha", super::super::rbac::Role::Admin)
            .expect("usuario");

        // Estado de uma instalacao legada: o segredo do 2FA cifrado sob a
        // chave derivada do salt compartilhado.
        store
            .set_pending_totp_secret(&user.id, "JBSWY3DPEHPK3PXP", &legacy)
            .expect("segredo sob a chave legada");
        store.enable_totp(&user.id).expect("2fa ligado");

        migrate_admin_secrets_kdf(&mut store, &keys).expect("migration");

        // Depois do re-key o segredo tem que abrir com a chave NOVA — que e a
        // que o processo passa a usar a partir daqui.
        assert_eq!(
            store
                .get_totp_secret(&user.id, &keys.current)
                .expect("segredo legivel sob a chave nova")
                .as_deref(),
            Some("JBSWY3DPEHPK3PXP"),
            "o re-key tem que levar o segredo do 2FA junto, senao o dono fica \
             trancado fora do painel"
        );
        assert!(
            store.get_totp_secret(&user.id, &legacy).is_err(),
            "a chave legada nao pode mais abrir o segredo"
        );
    }

    /// Fail-closed tambem no lado do 2FA: um segredo TOTP que nao abre com a
    /// chave legada aborta o re-key inteiro, sem gravar `kdf_params`. Gravar
    /// parametros de uma chave que nao abre esse segredo transformaria uma
    /// inconsistencia recuperavel (ainda da para rodar na legada) em perda
    /// definitiva.
    #[test]
    fn a_totp_secret_that_does_not_decrypt_aborts_the_rekey() {
        let mut store = AdminStore::in_memory().expect("in-memory store");
        let keys = pending_keys(PASS);
        let user = store
            .create_user("dono", "senha", super::super::rbac::Role::Admin)
            .expect("usuario");

        let unrelated = vec![5u8; MASTER_KEY_LEN];
        store
            .set_pending_totp_secret(&user.id, "JBSWY3DPEHPK3PXP", &unrelated)
            .expect("segredo sob uma chave alheia");

        let err = migrate_admin_secrets_kdf(&mut store, &keys).expect_err("must refuse");
        assert!(err.contains("totp secret"), "{err}");
        assert!(
            store.kdf_params().expect("readable").is_none(),
            "params nao podem ser gravados quando o re-key falhou"
        );
        assert_eq!(
            store
                .get_totp_secret(&user.id, &unrelated)
                .expect("segredo intacto")
                .as_deref(),
            Some("JBSWY3DPEHPK3PXP"),
            "o ciphertext tem que ficar byte-identico"
        );
    }

    #[test]
    fn migration_is_a_no_op_without_pending_params() {
        let mut store = AdminStore::in_memory().expect("in-memory store");
        let keys = AdminKeys {
            current: vec![7u8; MASTER_KEY_LEN],
            legacy: None,
            pending: None,
        };
        assert!(!keys.migration_pending());
        assert_eq!(migrate_admin_secrets_kdf(&mut store, &keys).unwrap(), 0);
        assert!(store.kdf_params().expect("readable").is_none());
    }

    #[test]
    fn failed_migration_writes_nothing_at_all() {
        // Fail-closed: a ciphertext that does not open with the legacy key means
        // our assumption about this installation is wrong. Neither the rows nor
        // the params may be written — recording params for a key that opens
        // nothing is exactly the data-loss window this design removes.
        let mut store = AdminStore::in_memory().expect("in-memory store");
        let keys = pending_keys("passphrase");

        let unrelated = vec![3u8; MASTER_KEY_LEN];
        let (ciphertext, nonce) =
            super::super::secrets::encrypt_value(b"opaque", &unrelated).expect("encrypt");
        store
            .set_secret("default", "acme", "token", &ciphertext, &nonce, None)
            .expect("seed");

        let err = migrate_admin_secrets_kdf(&mut store, &keys).expect_err("must refuse");
        assert!(err.contains("does not decrypt"), "{err}");

        let (stored, stored_nonce) = store
            .get_secret_raw("default", "acme", "token")
            .expect("still present");
        assert_eq!(stored, ciphertext, "ciphertext must be byte-identical");
        assert_eq!(stored_nonce, nonce);
        assert!(
            store.kdf_params().expect("readable").is_none(),
            "params must not be recorded when the re-key failed"
        );
    }

    /// The one fallback that stays (#1992 keeps it on purpose): no parameters
    /// recorded, and a ciphertext the legacy key does not open. The re-key is
    /// deferred and the process runs on the legacy key — the key the data of a
    /// never-re-keyed install is under — until a later boot can migrate.
    #[test]
    fn resolve_falls_back_to_the_legacy_key_when_the_migration_fails() {
        let mut store = AdminStore::in_memory().expect("in-memory store");
        let legacy = pbkdf2_key(LEGACY_KDF_SALT, LEGACY_KDF_ITERATIONS, PASS);

        let unrelated = vec![9u8; MASTER_KEY_LEN];
        let (ciphertext, nonce) =
            super::super::secrets::encrypt_value(b"x", &unrelated).expect("encrypt");
        store
            .set_secret("default", "acme", "token", &ciphertext, &nonce, None)
            .expect("seed");

        let key = resolve_with_passphrase(&mut store, PASS).expect("deferred, not refused");
        assert_eq!(key, legacy, "must hand back the key the data is under");
        assert!(
            store.kdf_params().expect("readable").is_none(),
            "a deferred re-key records nothing, so the next boot retries"
        );
    }
}
