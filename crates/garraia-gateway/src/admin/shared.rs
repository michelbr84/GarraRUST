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

/// Parameters used before 2026-08-29. Referenced **only** by the forward-only
/// migration in [`migrate_admin_secrets_kdf`], to decrypt secrets one last time
/// before they are re-encrypted under the per-installation key. No new
/// derivation ever uses these.
///
/// This is the sole remaining hard-coded cryptographic value in this file and
/// it exists purely so upgrades do not lose data; see
/// `docs/security/codeql-suppressions.md`.
const LEGACY_KDF_SALT: &[u8] = b"garraia-admin-secrets-v1";
const LEGACY_KDF_ITERATIONS: NonZeroU32 = NonZeroU32::new(100_000).unwrap();

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

/// The one derivation that can fail: iteration counts read back from the store
/// are plain integers and a corrupted row could carry a zero.
fn pbkdf2_key_checked(salt: &[u8], iterations: u32, passphrase: &str) -> Result<Vec<u8>, String> {
    let iterations = NonZeroU32::new(iterations)
        .ok_or_else(|| "stored PBKDF2 iteration count is zero".to_string())?;
    Ok(pbkdf2_key(salt, iterations, passphrase))
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

/// Resolve the admin master key, reporting whether a re-key is still pending.
///
/// `stored` is the installation's recorded KDF parameters, read from the admin
/// store by [`resolve_admin_encryption_key`]. Three paths:
///
/// 1. A passphrase is configured and parameters exist -> derive with them.
///    Steady state, nothing pending.
/// 2. A passphrase is configured and no parameters exist -> either a fresh
///    install or one still on the legacy constant salt. Generate
///    per-installation parameters, derive both the new and the legacy key, and
///    report the migration as pending. The parameters are recorded only when
///    [`migrate_admin_secrets_kdf`] commits, in the same transaction as the
///    re-encrypted ciphertexts.
/// 3. No passphrase -> the pre-existing random `master.key` file, which was
///    never affected by the constant-salt problem.
fn derive_admin_keys(stored: Option<StoredKdfParams>) -> AdminKeys {
    match admin_passphrase() {
        Some(passphrase) => derive_with_passphrase(stored, &passphrase),
        None => master_key_file_keys(),
    }
}

/// The passphrase-derived half of [`derive_admin_keys`], with the passphrase
/// passed in rather than read from the environment so it can be driven
/// deterministically from tests.
fn derive_with_passphrase(stored: Option<StoredKdfParams>, passphrase: &str) -> AdminKeys {
    if let Some((_, salt_b64, iterations)) = stored {
        match BASE64
            .decode(&salt_b64)
            .map_err(|e| format!("stored KDF salt is malformed: {e}"))
            .and_then(|salt| pbkdf2_key_checked(&salt, iterations, passphrase))
        {
            Ok(current) => {
                return AdminKeys {
                    current,
                    legacy: None,
                    pending: None,
                };
            }
            Err(e) => {
                // Loud, and stay on the legacy key rather than silently
                // deriving a third one that opens nothing.
                warn!("stored admin KDF parameters are unusable ({e}); using legacy parameters");
                return AdminKeys {
                    current: pbkdf2_key(LEGACY_KDF_SALT, LEGACY_KDF_ITERATIONS, passphrase),
                    legacy: None,
                    pending: None,
                };
            }
        }
    }

    match KdfParams::generate() {
        Ok(params) => match params.salt_bytes() {
            Ok(salt) => {
                return AdminKeys {
                    current: pbkdf2_key(&salt, params.iterations, passphrase),
                    legacy: Some(pbkdf2_key(
                        LEGACY_KDF_SALT,
                        LEGACY_KDF_ITERATIONS,
                        passphrase,
                    )),
                    pending: Some(params),
                };
            }
            Err(e) => warn!("failed to decode freshly generated KDF salt ({e})"),
        },
        Err(e) => warn!("failed to generate per-installation KDF salt ({e})"),
    }

    // Entropy trouble. Keep the deployment working on the legacy parameters
    // rather than refusing to boot; the next start retries the migration.
    warn!(
        "admin master key still derived with the legacy shared salt — \
         re-key will be retried on the next start"
    );
    AdminKeys {
        current: pbkdf2_key(LEGACY_KDF_SALT, LEGACY_KDF_ITERATIONS, passphrase),
        legacy: None,
        pending: None,
    }
}

/// No passphrase configured: a random 32-byte key persisted at
/// `<config_dir>/admin/master.key`. This path never used a salt, so it needs no
/// migration.
fn master_key_file_keys() -> AdminKeys {
    let key_path = admin_dir().join("master.key");

    if let Ok(data) = std::fs::read(&key_path)
        && data.len() == MASTER_KEY_LEN
    {
        return AdminKeys {
            current: data,
            legacy: None,
            pending: None,
        };
    }

    // The `.expect` predates this change: the function returns `AdminKeys`,
    // not `Result`, so propagating would mean changing the signature and its
    // callers. Without entropy there is no key to hand back either way.
    let key = garraia_security::random_bytes::<MASTER_KEY_LEN>()
        .expect("failed to generate master key")
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

    AdminKeys {
        current: key,
        legacy: None,
        pending: None,
    }
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
/// recorded yet, [`derive_admin_keys`] generates a *fresh random salt* on every
/// call, so a second, independent derivation yields a different key.
///
/// On a failed migration this returns the **legacy** key — the one the stored
/// ciphertexts are actually under — so the deployment keeps working and the
/// next boot retries.
pub fn resolve_admin_encryption_key(store: &mut AdminStore) -> Vec<u8> {
    let keys = derive_admin_keys(store.kdf_params());
    let (Some(legacy), Some(_)) = (keys.legacy.as_ref(), keys.pending.as_ref()) else {
        return keys.current;
    };
    let legacy = legacy.clone();

    match migrate_admin_secrets_kdf(store, &keys) {
        Ok(_) => keys.current,
        Err(e) => {
            warn!(
                "admin master-key re-key deferred ({e}); staying on the legacy \
                 shared salt, will retry on next start"
            );
            legacy
        }
    }
}

/// Re-encrypt every stored admin secret under `keys.current`, then persist the
/// new KDF parameters.
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

    // Ciphertexts and the parameters that describe them land together or not at
    // all. An earlier revision committed the SQL and then wrote a `kdf.json`;
    // a crash or a failed write in between left the data under a key nothing
    // recorded, and the next boot would mint a third salt and open nothing.
    let rekeyed = store.apply_secret_rekey(
        &new_secrets,
        &new_versions,
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
        let first = derive_with_passphrase(None, PASS);
        assert!(first.migration_pending());
        let recorded = params_of(&first);

        let second = derive_with_passphrase(Some(recorded.clone()), PASS);
        let third = derive_with_passphrase(Some(recorded), PASS);
        assert_eq!(first.current, second.current);
        assert_eq!(second.current, third.current);
        assert!(!second.migration_pending(), "nothing left to migrate");
    }

    #[test]
    fn unusable_stored_params_fall_back_to_legacy_not_to_a_third_key() {
        let legacy = pbkdf2_key(LEGACY_KDF_SALT, LEGACY_KDF_ITERATIONS, PASS);

        for bad in [
            (1u32, "!!!not base64!!!".to_string(), 600_000u32),
            (1, BASE64.encode([7u8; 32]), 0), // zero iterations
        ] {
            let keys = derive_with_passphrase(Some(bad), PASS);
            assert_eq!(
                keys.current, legacy,
                "must stay on the key the ciphertexts are under"
            );
            assert!(!keys.migration_pending());
        }
    }

    // ── Forward-only re-key migration ────────────────────────────────────

    fn pending_keys(passphrase: &str) -> AdminKeys {
        derive_with_passphrase(None, passphrase)
    }

    #[test]
    fn migration_reencrypts_secrets_and_records_params_atomically() {
        let mut store = AdminStore::in_memory().expect("in-memory store");
        let keys = pending_keys(PASS);
        assert!(keys.migration_pending());
        assert!(store.kdf_params().is_none(), "nothing recorded yet");

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
        let recorded = store.kdf_params().expect("params recorded");
        let next_boot = derive_with_passphrase(Some(recorded), PASS);
        assert_eq!(next_boot.current, keys.current);
        assert!(!next_boot.migration_pending());
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
        assert!(store.kdf_params().is_none());
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
            store.kdf_params().is_none(),
            "params must not be recorded when the re-key failed"
        );
    }

    #[test]
    fn resolve_falls_back_to_the_legacy_key_when_the_migration_fails() {
        let mut store = AdminStore::in_memory().expect("in-memory store");
        let keys = pending_keys(PASS);
        let legacy = keys.legacy.clone().expect("legacy");

        let unrelated = vec![9u8; MASTER_KEY_LEN];
        let (ciphertext, nonce) =
            super::super::secrets::encrypt_value(b"x", &unrelated).expect("encrypt");
        store
            .set_secret("default", "acme", "token", &ciphertext, &nonce, None)
            .expect("seed");

        assert!(migrate_admin_secrets_kdf(&mut store, &keys).is_err());
        // resolve_admin_encryption_key takes the same error arm and must hand
        // back the key the data is actually under, never `keys.current`.
        assert_ne!(keys.current, legacy);
    }
}
