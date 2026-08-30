use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::pbkdf2;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

const PBKDF2_ITERATIONS: u32 = 600_000;
const SALT_LEN: usize = 32;
const KEY_LEN: usize = 32; // 256 bits

/// On-disk representation of the encrypted vault.
#[derive(Debug, Serialize, Deserialize)]
struct VaultFile {
    salt: String,
    nonce: String,
    ciphertext: String,
}

/// Encrypted key-value credential store backed by AES-256-GCM.
///
/// Credentials are kept in memory as a plain `HashMap` after decryption.
/// Call [`save`] to persist changes back to disk.
pub struct CredentialVault {
    path: PathBuf,
    derived_key: Vec<u8>,
    salt: Vec<u8>,
    entries: HashMap<String, String>,
}

impl CredentialVault {
    /// Check whether a vault file exists at `path`.
    pub fn exists(path: &Path) -> bool {
        path.is_file()
    }

    /// Create a brand-new vault at `path`, encrypted with `passphrase`.
    pub fn create(path: &Path, passphrase: &str) -> Result<Self, CredentialError> {
        if path.exists() {
            return Err(CredentialError::AlreadyExists(path.display().to_string()));
        }

        let salt = crate::random_bytes::<SALT_LEN>()
            .map_err(|_| CredentialError::Crypto("failed to generate salt".into()))?
            .to_vec();

        let derived_key = derive_key(passphrase, &salt);

        let vault = Self {
            path: path.to_path_buf(),
            derived_key,
            salt,
            entries: HashMap::new(),
        };
        vault.save()?;

        info!("created new credential vault at {}", path.display());
        Ok(vault)
    }

    /// Open an existing vault, decrypting with `passphrase`.
    pub fn open(path: &Path, passphrase: &str) -> Result<Self, CredentialError> {
        let contents = std::fs::read_to_string(path).map_err(|e| {
            CredentialError::Io(format!("failed to read vault at {}: {e}", path.display()))
        })?;

        let vault_file: VaultFile = serde_json::from_str(&contents)
            .map_err(|e| CredentialError::Format(format!("invalid vault format: {e}")))?;

        let salt = BASE64
            .decode(&vault_file.salt)
            .map_err(|e| CredentialError::Format(format!("invalid salt encoding: {e}")))?;
        let nonce_bytes = BASE64
            .decode(&vault_file.nonce)
            .map_err(|e| CredentialError::Format(format!("invalid nonce encoding: {e}")))?;
        let mut ciphertext = BASE64
            .decode(&vault_file.ciphertext)
            .map_err(|e| CredentialError::Format(format!("invalid ciphertext encoding: {e}")))?;

        let derived_key = derive_key(passphrase, &salt);

        // Decrypt in place
        let key = make_aead_key(&derived_key)?;
        let nonce = Nonce::try_assume_unique_for_key(&nonce_bytes)
            .map_err(|_| CredentialError::Crypto("invalid nonce length".into()))?;

        let plaintext = key
            .open_in_place(nonce, Aad::empty(), &mut ciphertext)
            .map_err(|_| CredentialError::WrongPassphrase)?;

        let entries: HashMap<String, String> = serde_json::from_slice(plaintext)
            .map_err(|e| CredentialError::Format(format!("corrupted vault data: {e}")))?;

        info!(
            "opened credential vault at {} ({} keys)",
            path.display(),
            entries.len()
        );

        Ok(Self {
            path: path.to_path_buf(),
            derived_key,
            salt,
            entries,
        })
    }

    /// Retrieve a credential by key.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(|s| s.as_str())
    }

    /// Store or update a credential.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.entries.insert(key.into(), value.into());
    }

    /// Remove a credential. Returns `true` if it existed.
    pub fn remove(&mut self, key: &str) -> bool {
        self.entries.remove(key).is_some()
    }

    /// List all stored credential keys.
    pub fn list_keys(&self) -> Vec<&str> {
        self.entries.keys().map(|k| k.as_str()).collect()
    }

    /// Encrypt and persist the vault to disk.
    pub fn save(&self) -> Result<(), CredentialError> {
        let plaintext = serde_json::to_vec(&self.entries)
            .map_err(|e| CredentialError::Format(format!("failed to serialize vault: {e}")))?;

        let nonce_bytes = crate::random_bytes::<NONCE_LEN>()
            .map_err(|_| CredentialError::Crypto("failed to generate nonce".into()))?;

        let key = make_aead_key(&self.derived_key)?;
        // `random_bytes` returns `[u8; NONCE_LEN]`, so the length is
        // guaranteed by the type and this cannot fail. `open` above keeps the
        // fallible variant: there the nonce is decoded from the vault file and
        // its length is only known at runtime.
        let nonce = Nonce::assume_unique_for_key(nonce_bytes);

        let mut in_out = plaintext;
        key.seal_in_place_append_tag(nonce, Aad::empty(), &mut in_out)
            .map_err(|_| CredentialError::Crypto("encryption failed".into()))?;

        let vault_file = VaultFile {
            salt: BASE64.encode(&self.salt),
            nonce: BASE64.encode(nonce_bytes),
            ciphertext: BASE64.encode(&in_out),
        };

        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                CredentialError::Io(format!(
                    "failed to create vault directory {}: {e}",
                    parent.display()
                ))
            })?;
        }

        let json = serde_json::to_string_pretty(&vault_file)
            .map_err(|e| CredentialError::Format(format!("failed to serialize vault file: {e}")))?;
        std::fs::write(&self.path, json).map_err(|e| {
            CredentialError::Io(format!(
                "failed to write vault at {}: {e}",
                self.path.display()
            ))
        })?;

        Ok(())
    }
}

/// Canonical environment variable carrying the credential-vault passphrase.
pub const VAULT_PASSPHRASE_ENV: &str = "GARRAIA_VAULT_PASSPHRASE";

/// Deprecated mixed-case spelling of [`VAULT_PASSPHRASE_ENV`]. Historically it
/// only fed the JWT-secret fallback in `garraia-config::auth`, so exporting it
/// with the intent of unlocking the vault failed silently (issue #824). It is
/// now accepted here as a fallback so a casing slip no longer strands the
/// operator, but the canonical all-caps spelling always wins.
pub const VAULT_PASSPHRASE_ENV_LEGACY: &str = "GarraIA_VAULT_PASSPHRASE";

/// Read the vault passphrase from the environment: the canonical
/// [`VAULT_PASSPHRASE_ENV`] first, then the deprecated mixed-case
/// [`VAULT_PASSPHRASE_ENV_LEGACY`] with a deprecation warning. Empty values
/// count as unset on both tiers, matching the key-resolution convention in
/// `garraia-config::provider_keys` (an empty passphrase cannot open a vault).
pub fn vault_passphrase_from_env() -> Option<String> {
    if let Some(v) = std::env::var(VAULT_PASSPHRASE_ENV)
        .ok()
        .filter(|v| !v.is_empty())
    {
        return Some(v);
    }
    let legacy = std::env::var(VAULT_PASSPHRASE_ENV_LEGACY)
        .ok()
        .filter(|v| !v.is_empty())?;
    warn!(
        "vault passphrase read from deprecated mixed-case {VAULT_PASSPHRASE_ENV_LEGACY}; \
         prefer the canonical {VAULT_PASSPHRASE_ENV} spelling"
    );
    Some(legacy)
}

/// Try to load a credential from the vault, returning `None` if the vault
/// doesn't exist or can't be opened (no passphrase prompt in server mode).
pub fn try_vault_get(vault_path: &Path, key: &str) -> Option<String> {
    if !CredentialVault::exists(vault_path) {
        return None;
    }
    // In server mode we cannot prompt for a passphrase, so try the
    // environment (canonical spelling, then the deprecated mixed-case
    // alias) as a fallback.
    let passphrase = vault_passphrase_from_env()?;
    match CredentialVault::open(vault_path, &passphrase) {
        Ok(vault) => vault.get(key).map(|s| s.to_string()),
        Err(e) => {
            warn!("could not open credential vault: {e}");
            None
        }
    }
}

/// Try to store a credential in the vault, returning `true` on success.
/// Falls back silently if the vault passphrase is not available or the vault
/// cannot be opened/created. This is intended for best-effort key persistence
/// at runtime (e.g. when a user adds a provider via the web UI).
pub fn try_vault_set(vault_path: &Path, key: &str, value: &str) -> bool {
    let Some(passphrase) = vault_passphrase_from_env() else {
        return false;
    };

    let mut vault = if CredentialVault::exists(vault_path) {
        match CredentialVault::open(vault_path, &passphrase) {
            Ok(v) => v,
            Err(e) => {
                warn!("try_vault_set: could not open vault: {e}");
                return false;
            }
        }
    } else {
        match CredentialVault::create(vault_path, &passphrase) {
            Ok(v) => v,
            Err(e) => {
                warn!("try_vault_set: could not create vault: {e}");
                return false;
            }
        }
    };

    vault.set(key, value);
    match vault.save() {
        Ok(()) => {
            info!("stored credential '{key}' in vault");
            true
        }
        Err(e) => {
            warn!("try_vault_set: failed to save vault: {e}");
            false
        }
    }
}

/// Try to remove all vault credentials whose key starts with `prefix`.
/// Returns the number of keys removed, or `0` on any error.
pub fn try_vault_delete_prefix(vault_path: &Path, prefix: &str) -> usize {
    let Some(passphrase) = vault_passphrase_from_env() else {
        return 0;
    };
    if !CredentialVault::exists(vault_path) {
        return 0;
    }
    let mut vault = match CredentialVault::open(vault_path, &passphrase) {
        Ok(v) => v,
        Err(e) => {
            warn!("try_vault_delete_prefix: could not open vault: {e}");
            return 0;
        }
    };
    let keys: Vec<String> = vault
        .list_keys()
        .into_iter()
        .filter(|k| k.starts_with(prefix))
        .map(|k| k.to_string())
        .collect();
    let count = keys.len();
    for k in &keys {
        vault.remove(k);
    }
    if count > 0 {
        match vault.save() {
            Ok(()) => info!("removed {count} credential(s) with prefix '{prefix}' from vault"),
            Err(e) => {
                warn!("try_vault_delete_prefix: failed to save vault: {e}");
                return 0;
            }
        }
    }
    count
}

fn derive_key(passphrase: &str, salt: &[u8]) -> Vec<u8> {
    let iterations = NonZeroU32::new(PBKDF2_ITERATIONS).expect("iterations > 0");
    let mut key = vec![0u8; KEY_LEN];
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        iterations,
        salt,
        passphrase.as_bytes(),
        &mut key,
    );
    key
}

fn make_aead_key(derived: &[u8]) -> Result<LessSafeKey, CredentialError> {
    let unbound = UnboundKey::new(&AES_256_GCM, derived)
        .map_err(|_| CredentialError::Crypto("failed to create AES key".into()))?;
    Ok(LessSafeKey::new(unbound))
}

#[derive(Debug, thiserror::Error)]
pub enum CredentialError {
    #[error("vault already exists: {0}")]
    AlreadyExists(String),
    #[error("wrong passphrase or corrupted vault")]
    WrongPassphrase,
    #[error("cryptographic error: {0}")]
    Crypto(String),
    #[error("vault format error: {0}")]
    Format(String),
    #[error("I/O error: {0}")]
    Io(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_vault_path(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "garraia-vault-test-{label}-{}-{nanos}.json",
            std::process::id()
        ))
    }

    #[test]
    fn create_open_round_trip() {
        let path = temp_vault_path("round-trip");
        let passphrase = "test-passphrase-123";

        let mut vault = CredentialVault::create(&path, passphrase).unwrap();
        vault.set("ANTHROPIC_API_KEY", "sk-ant-test");
        vault.set("OPENAI_API_KEY", "sk-openai-test");
        vault.save().unwrap();

        let vault2 = CredentialVault::open(&path, passphrase).unwrap();
        assert_eq!(vault2.get("ANTHROPIC_API_KEY"), Some("sk-ant-test"));
        assert_eq!(vault2.get("OPENAI_API_KEY"), Some("sk-openai-test"));
        assert_eq!(vault2.list_keys().len(), 2);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn wrong_passphrase_fails() {
        let path = temp_vault_path("wrong-pass");
        let vault = CredentialVault::create(&path, "correct").unwrap();
        drop(vault);

        let result = CredentialVault::open(&path, "wrong");
        assert!(result.is_err());

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn remove_key() {
        let path = temp_vault_path("remove");

        let mut vault = CredentialVault::create(&path, "pass").unwrap();
        vault.set("key1", "val1");
        assert!(vault.remove("key1"));
        assert!(!vault.remove("key1")); // already removed
        assert!(vault.get("key1").is_none());

        let _ = fs::remove_file(&path);
    }

    // ── Issue #824: passphrase env-var spelling fallback ──────────────────
    //
    // These tests mutate the process-global environment, so they serialize
    // on a shared lock (same pattern as garraia-config::auth::tests) and
    // snapshot/restore both variables on exit.

    use std::sync::{LazyLock, Mutex};

    static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    struct EnvSnapshot {
        canonical: Option<String>,
        legacy: Option<String>,
    }

    impl EnvSnapshot {
        fn capture_and_clear() -> Self {
            let snap = Self {
                canonical: std::env::var(VAULT_PASSPHRASE_ENV).ok(),
                legacy: std::env::var(VAULT_PASSPHRASE_ENV_LEGACY).ok(),
            };
            // SAFETY: ENV_LOCK held by caller.
            unsafe {
                std::env::remove_var(VAULT_PASSPHRASE_ENV);
                std::env::remove_var(VAULT_PASSPHRASE_ENV_LEGACY);
            }
            snap
        }
    }

    impl Drop for EnvSnapshot {
        fn drop(&mut self) {
            // SAFETY: ENV_LOCK still held while the test unwinds.
            unsafe {
                match self.canonical.take() {
                    Some(v) => std::env::set_var(VAULT_PASSPHRASE_ENV, v),
                    None => std::env::remove_var(VAULT_PASSPHRASE_ENV),
                }
                match self.legacy.take() {
                    Some(v) => std::env::set_var(VAULT_PASSPHRASE_ENV_LEGACY, v),
                    None => std::env::remove_var(VAULT_PASSPHRASE_ENV_LEGACY),
                }
            }
        }
    }

    #[test]
    fn passphrase_env_prefers_canonical_and_falls_back_to_legacy() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _snapshot = EnvSnapshot::capture_and_clear();

        assert_eq!(vault_passphrase_from_env(), None, "unset ⇒ None");

        // SAFETY: ENV_LOCK held.
        unsafe { std::env::set_var(VAULT_PASSPHRASE_ENV_LEGACY, "legacy-pass") };
        assert_eq!(
            vault_passphrase_from_env().as_deref(),
            Some("legacy-pass"),
            "mixed-case alias must unlock the vault when canonical is unset"
        );

        unsafe { std::env::set_var(VAULT_PASSPHRASE_ENV, "canonical-pass") };
        assert_eq!(
            vault_passphrase_from_env().as_deref(),
            Some("canonical-pass"),
            "canonical spelling must win when both are set"
        );

        // Empty values count as unset on both tiers.
        unsafe {
            std::env::set_var(VAULT_PASSPHRASE_ENV, "");
            std::env::set_var(VAULT_PASSPHRASE_ENV_LEGACY, "");
        }
        assert_eq!(vault_passphrase_from_env(), None, "empty ⇒ None");
    }

    #[test]
    fn try_vault_get_accepts_legacy_spelling() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _snapshot = EnvSnapshot::capture_and_clear();

        let path = temp_vault_path("legacy-env");
        let mut vault = CredentialVault::create(&path, "legacy-pass").unwrap();
        vault.set("ANTHROPIC_API_KEY", "sk-ant-legacy");
        vault.save().unwrap();

        // The #824 trap: only the mixed-case spelling exported. Before the
        // fix this returned None and the provider was skipped at boot.
        // SAFETY: ENV_LOCK held.
        unsafe { std::env::set_var(VAULT_PASSPHRASE_ENV_LEGACY, "legacy-pass") };
        assert_eq!(
            try_vault_get(&path, "ANTHROPIC_API_KEY").as_deref(),
            Some("sk-ant-legacy"),
            "mixed-case-only export must now unlock the vault"
        );

        let _ = fs::remove_file(&path);
    }
}
