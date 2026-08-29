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
//! `credentials.rs` and `mobile_auth.rs`). Parameters are persisted next to the
//! key material in `<config_dir>/admin/kdf.json` so they survive restarts, and
//! existing installations are migrated forward-only by
//! [`migrate_admin_secrets_kdf`]: it re-encrypts every stored secret under the
//! new key inside one SQLite transaction and only then writes `kdf.json`. If
//! anything fails, nothing is written and the deployment keeps working on the
//! legacy parameters.

use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use ring::rand::{SecureRandom, SystemRandom};
use tokio::sync::Mutex;
use tracing::{info, warn};

use super::store::AdminStore;
use crate::state::SharedState;

/// AES-256-GCM key length. Also the PBKDF2 output length.
const MASTER_KEY_LEN: usize = 32;
/// PBKDF2 salt length for newly initialised installations.
const KDF_SALT_LEN: usize = 32;
/// Matches `admin/store.rs`, `garraia-security/src/credentials.rs` and
/// `garraia-gateway/src/mobile_auth.rs`.
const KDF_ITERATIONS: u32 = 600_000;
/// Sidecar recording the parameters an installation's master key was derived
/// with. Lives beside `master.key` in `<config_dir>/admin/`.
const KDF_PARAMS_FILE: &str = "kdf.json";
/// Schema version of [`KdfParams`]. Bump when the derivation changes shape.
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
const LEGACY_KDF_ITERATIONS: u32 = 100_000;

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
    iterations: u32,
}

impl KdfParams {
    fn generate(rng: &SystemRandom) -> Result<Self, String> {
        let mut salt = vec![0u8; KDF_SALT_LEN];
        rng.fill(&mut salt)
            .map_err(|_| "failed to generate KDF salt".to_string())?;
        Ok(Self {
            version: KDF_PARAMS_VERSION,
            salt: BASE64.encode(&salt),
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
    /// Parameters to persist **after** a successful re-key. `None` when there
    /// is nothing to commit.
    pending: Option<(PathBuf, KdfParams)>,
}

impl AdminKeys {
    /// True when [`migrate_admin_secrets_kdf`] still has work to do.
    pub fn migration_pending(&self) -> bool {
        self.legacy.is_some() && self.pending.is_some()
    }
}

fn pbkdf2_key(salt: &[u8], iterations: u32, passphrase: &str) -> Result<Vec<u8>, String> {
    let iterations = NonZeroU32::new(iterations)
        .ok_or_else(|| "PBKDF2 iteration count must be non-zero".to_string())?;
    let mut key = vec![0u8; MASTER_KEY_LEN];
    ring::pbkdf2::derive(
        ring::pbkdf2::PBKDF2_HMAC_SHA256,
        iterations,
        salt,
        passphrase.as_bytes(),
        &mut key,
    );
    Ok(key)
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

fn load_kdf_params(dir: &Path) -> Option<KdfParams> {
    let raw = std::fs::read_to_string(dir.join(KDF_PARAMS_FILE)).ok()?;
    match serde_json::from_str::<KdfParams>(&raw) {
        Ok(params) if params.iterations > 0 && !params.salt.is_empty() => Some(params),
        Ok(_) => {
            warn!("admin kdf.json is present but has empty/zero parameters; ignoring");
            None
        }
        Err(e) => {
            warn!("admin kdf.json is unreadable ({e}); ignoring");
            None
        }
    }
}

fn store_kdf_params(dir: &Path, params: &KdfParams) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("failed to create {}: {e}", dir.display()))?;
    let json = serde_json::to_string_pretty(params)
        .map_err(|e| format!("failed to serialize kdf params: {e}"))?;
    // Write-then-rename so a crash mid-write cannot leave a truncated file that
    // would make every stored secret undecryptable.
    let tmp = dir.join(format!("{KDF_PARAMS_FILE}.tmp"));
    std::fs::write(&tmp, json).map_err(|e| format!("failed to write kdf params: {e}"))?;
    std::fs::rename(&tmp, dir.join(KDF_PARAMS_FILE))
        .map_err(|e| format!("failed to commit kdf params: {e}"))
}

/// Resolve the admin master key, reporting whether a re-key is still pending.
///
/// Three paths, in order:
///
/// 1. A passphrase is configured and `kdf.json` exists → derive with the
///    recorded per-installation parameters. Steady state, nothing pending.
/// 2. A passphrase is configured and `kdf.json` does **not** exist → this is
///    either a fresh install or one still on the legacy constant salt. Generate
///    per-installation parameters, derive both the new and the legacy key, and
///    report the migration as pending. `kdf.json` is written only after
///    [`migrate_admin_secrets_kdf`] succeeds.
/// 3. No passphrase → the pre-existing random `master.key` file path, which was
///    never affected by the constant-salt problem. Unchanged.
pub fn derive_admin_keys() -> AdminKeys {
    let rng = SystemRandom::new();
    let dir = admin_dir();

    if let Some(passphrase) = admin_passphrase() {
        if let Some(params) = load_kdf_params(&dir) {
            match params
                .salt_bytes()
                .and_then(|salt| pbkdf2_key(&salt, params.iterations, &passphrase))
            {
                Ok(current) => {
                    return AdminKeys {
                        current,
                        legacy: None,
                        pending: None,
                    };
                }
                Err(e) => {
                    // Fail loudly rather than silently deriving a different key
                    // and making every stored secret look corrupt.
                    warn!("admin kdf.json is unusable ({e}); falling back to legacy parameters");
                    let current = pbkdf2_key(LEGACY_KDF_SALT, LEGACY_KDF_ITERATIONS, &passphrase)
                        .expect("legacy iteration count is a non-zero constant");
                    return AdminKeys {
                        current,
                        legacy: None,
                        pending: None,
                    };
                }
            }
        }

        match KdfParams::generate(&rng) {
            Ok(params) => {
                let derived = params
                    .salt_bytes()
                    .and_then(|salt| pbkdf2_key(&salt, params.iterations, &passphrase));
                match derived {
                    Ok(current) => {
                        let legacy =
                            pbkdf2_key(LEGACY_KDF_SALT, LEGACY_KDF_ITERATIONS, &passphrase)
                                .expect("legacy iteration count is a non-zero constant");
                        return AdminKeys {
                            current,
                            legacy: Some(legacy),
                            pending: Some((dir, params)),
                        };
                    }
                    Err(e) => warn!("failed to derive per-installation admin key ({e})"),
                }
            }
            Err(e) => warn!("failed to generate per-installation KDF salt ({e})"),
        }

        // Entropy or filesystem trouble. Keep the deployment working on the
        // legacy parameters rather than refusing to boot; the next start
        // retries the migration.
        warn!(
            "admin master key still derived with the legacy shared salt — \
             re-key will be retried on the next start"
        );
        let current = pbkdf2_key(LEGACY_KDF_SALT, LEGACY_KDF_ITERATIONS, &passphrase)
            .expect("legacy iteration count is a non-zero constant");
        return AdminKeys {
            current,
            legacy: None,
            pending: None,
        };
    }

    // No passphrase: random 32-byte key persisted at <config_dir>/admin/master.key.
    // This path never used a salt, so it needs no migration.
    let key_path = dir.join("master.key");

    if let Ok(data) = std::fs::read(&key_path)
        && data.len() == MASTER_KEY_LEN
    {
        return AdminKeys {
            current: data,
            legacy: None,
            pending: None,
        };
    }

    let mut key = vec![0u8; MASTER_KEY_LEN];
    rng.fill(&mut key).expect("failed to generate master key");

    if let Some(parent) = key_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&key_path, &key);

    AdminKeys {
        current: key,
        legacy: None,
        pending: None,
    }
}

/// Resolve the master encryption key for the admin secrets store, running the
/// forward-only re-key first when one is pending.
///
/// **This is the only entry point callers should use.** Deriving separately in
/// two places would be a correctness bug, not just waste: when `kdf.json` is
/// absent, [`derive_admin_keys`] generates a *fresh random salt* on every call,
/// so a second, independent derivation yields a different key. If the re-key
/// then fails, the process would be holding a key that opens nothing.
///
/// On a failed migration this returns the **legacy** key — the one the stored
/// ciphertexts are actually under — so the deployment keeps working and the
/// next boot retries.
pub fn resolve_admin_encryption_key(store: &mut AdminStore) -> Vec<u8> {
    let keys = derive_admin_keys();
    if !keys.migration_pending() {
        return keys.current;
    }
    match migrate_admin_secrets_kdf(store, &keys) {
        Ok(_) => keys.current,
        Err(e) => {
            warn!(
                "admin master-key re-key deferred ({e}); staying on the legacy \
                 shared salt, will retry on next start"
            );
            keys.legacy
                .expect("migration_pending() implies a legacy key is present")
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
pub fn migrate_admin_secrets_kdf(
    store: &mut AdminStore,
    keys: &AdminKeys,
) -> Result<usize, String> {
    let (Some(legacy), Some((dir, params))) = (keys.legacy.as_ref(), keys.pending.as_ref()) else {
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

    let rekeyed = store.apply_secret_rekey(&new_secrets, &new_versions)?;
    store_kdf_params(dir, params)?;

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

    #[test]
    fn kdf_params_roundtrip_through_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let rng = SystemRandom::new();
        let params = KdfParams::generate(&rng).expect("generate");

        assert!(load_kdf_params(dir.path()).is_none());
        store_kdf_params(dir.path(), &params).expect("store");

        let loaded = load_kdf_params(dir.path()).expect("load");
        assert_eq!(loaded.version, KDF_PARAMS_VERSION);
        assert_eq!(loaded.iterations, KDF_ITERATIONS);
        assert_eq!(loaded.salt, params.salt);
        assert_eq!(loaded.salt_bytes().expect("decode").len(), KDF_SALT_LEN);
    }

    #[test]
    fn generated_salts_differ_between_installations() {
        let rng = SystemRandom::new();
        let a = KdfParams::generate(&rng).expect("a");
        let b = KdfParams::generate(&rng).expect("b");
        assert_ne!(
            a.salt, b.salt,
            "two installations must not share a PBKDF2 salt"
        );

        // The regression this whole change exists for: same passphrase, two
        // installations, two different master keys.
        let key_a = pbkdf2_key(&a.salt_bytes().unwrap(), 1, "same-passphrase").unwrap();
        let key_b = pbkdf2_key(&b.salt_bytes().unwrap(), 1, "same-passphrase").unwrap();
        assert_ne!(key_a, key_b);
    }

    #[test]
    fn new_parameters_differ_from_legacy() {
        // Compile-time so a future edit that weakens the parameters fails the
        // build rather than the test run.
        const _: () = assert!(KDF_ITERATIONS >= 600_000);
        const _: () = assert!(KDF_ITERATIONS != LEGACY_KDF_ITERATIONS);
        const _: () = assert!(KDF_SALT_LEN == 32);
        // Keep a runtime assertion too, so the test has an observable body.
        assert_ne!(KDF_ITERATIONS, LEGACY_KDF_ITERATIONS);
    }

    #[test]
    fn malformed_kdf_json_is_ignored_not_trusted() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join(KDF_PARAMS_FILE), "{not json").expect("write");
        assert!(load_kdf_params(dir.path()).is_none());

        std::fs::write(
            dir.path().join(KDF_PARAMS_FILE),
            r#"{"version":1,"salt":"","iterations":0}"#,
        )
        .expect("write");
        assert!(load_kdf_params(dir.path()).is_none());
    }

    // ── Forward-only re-key migration ────────────────────────────────────

    /// Build an `AdminKeys` describing a pending migration, exactly as
    /// `derive_admin_keys` does on an installation that has never written
    /// `kdf.json`.
    fn pending_keys(dir: &Path, passphrase: &str) -> AdminKeys {
        let rng = SystemRandom::new();
        let params = KdfParams::generate(&rng).expect("params");
        let current =
            pbkdf2_key(&params.salt_bytes().unwrap(), params.iterations, passphrase).unwrap();
        let legacy = pbkdf2_key(LEGACY_KDF_SALT, LEGACY_KDF_ITERATIONS, passphrase).unwrap();
        AdminKeys {
            current,
            legacy: Some(legacy),
            pending: Some((dir.to_path_buf(), params)),
        }
    }

    #[test]
    fn migration_reencrypts_secrets_and_commits_params() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut store = AdminStore::in_memory().expect("in-memory store");
        let keys = pending_keys(dir.path(), "correct horse battery staple");
        assert!(keys.migration_pending());

        // A secret as it exists on a legacy install: encrypted with the key
        // derived from the shared constant salt.
        let legacy = keys.legacy.clone().expect("legacy key");
        let (ciphertext, nonce) =
            super::super::secrets::encrypt_value(b"sk-live-do-not-leak", &legacy).expect("encrypt");
        store
            .set_secret("default", "openai", "api_key", &ciphertext, &nonce, None)
            .expect("seed secret");

        let rekeyed = migrate_admin_secrets_kdf(&mut store, &keys).expect("migration");
        assert_eq!(rekeyed, 1, "one live secret, no archived versions yet");

        // The stored ciphertext now opens with the per-installation key and no
        // longer opens with the legacy one.
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

        // Params are committed only after the re-key, so the next boot derives
        // the same key instead of retrying the migration.
        let committed = load_kdf_params(dir.path()).expect("kdf.json written");
        assert_eq!(committed.iterations, KDF_ITERATIONS);
        let rederived = pbkdf2_key(
            &committed.salt_bytes().unwrap(),
            committed.iterations,
            "correct horse battery staple",
        )
        .unwrap();
        assert_eq!(rederived, keys.current);
    }

    #[test]
    fn resolve_returns_the_legacy_key_when_the_migration_fails() {
        // The bug this guards against: `derive_admin_keys` mints a fresh random
        // salt whenever kdf.json is absent, so resolving twice would produce two
        // different keys. Resolution therefore happens once, and a failed re-key
        // must hand back the key the ciphertexts are actually under.
        let dir = tempfile::tempdir().expect("tempdir");
        let mut store = AdminStore::in_memory().expect("in-memory store");
        let keys = pending_keys(dir.path(), "passphrase");
        let legacy = keys.legacy.clone().expect("legacy");

        let foreign = vec![9u8; MASTER_KEY_LEN];
        let (ciphertext, nonce) =
            super::super::secrets::encrypt_value(b"x", &foreign).expect("encrypt");
        store
            .set_secret("default", "acme", "token", &ciphertext, &nonce, None)
            .expect("seed");

        assert!(migrate_admin_secrets_kdf(&mut store, &keys).is_err());
        // Same decision `resolve_admin_encryption_key` makes on the error arm.
        assert_ne!(keys.current, legacy);
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
    }

    #[test]
    fn migration_leaves_the_store_untouched_when_a_secret_does_not_decrypt() {
        // Fail-closed: a ciphertext that does not open with the legacy key means
        // our assumption about this installation is wrong. Nothing may be
        // written — not the rows, and not kdf.json, or the next boot would
        // derive a key that opens nothing.
        let dir = tempfile::tempdir().expect("tempdir");
        let mut store = AdminStore::in_memory().expect("in-memory store");
        let keys = pending_keys(dir.path(), "passphrase");

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
            load_kdf_params(dir.path()).is_none(),
            "kdf.json must not be committed on a failed migration"
        );
    }

    #[test]
    fn pbkdf2_key_rejects_zero_iterations() {
        assert!(pbkdf2_key(b"salt", 0, "pw").is_err());
    }
}
