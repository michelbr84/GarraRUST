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

use std::sync::Arc;

use ring::rand::{SecureRandom, SystemRandom};
use tokio::sync::Mutex;

use super::store::AdminStore;
use crate::state::SharedState;

/// Shared state for admin API handlers.
#[derive(Clone)]
pub struct AdminState {
    pub store: Arc<Mutex<AdminStore>>,
    pub app_state: SharedState,
    /// Master encryption key (derived or loaded at startup) for secrets encryption.
    pub encryption_key: Arc<Vec<u8>>,
}

/// Derive or generate a master encryption key for the admin secrets store.
pub fn derive_encryption_key() -> Vec<u8> {
    if let Ok(passphrase) = std::env::var("GARRAIA_ADMIN_KEY") {
        let salt = b"garraia-admin-secrets-v1";
        let iterations = std::num::NonZeroU32::new(100_000).unwrap();
        let mut key = vec![0u8; 32];
        ring::pbkdf2::derive(
            ring::pbkdf2::PBKDF2_HMAC_SHA256,
            iterations,
            salt,
            passphrase.as_bytes(),
            &mut key,
        );
        return key;
    }

    if let Ok(passphrase) = std::env::var("GARRAIA_VAULT_PASSPHRASE") {
        let salt = b"garraia-admin-secrets-v1";
        let iterations = std::num::NonZeroU32::new(100_000).unwrap();
        let mut key = vec![0u8; 32];
        ring::pbkdf2::derive(
            ring::pbkdf2::PBKDF2_HMAC_SHA256,
            iterations,
            salt,
            passphrase.as_bytes(),
            &mut key,
        );
        return key;
    }

    let key_path = garraia_config::ConfigLoader::default_config_dir()
        .join("admin")
        .join("master.key");

    if let Ok(data) = std::fs::read(&key_path)
        && data.len() == 32
    {
        return data;
    }

    let rng = SystemRandom::new();
    let mut key = vec![0u8; 32];
    rng.fill(&mut key).expect("failed to generate master key");

    if let Some(parent) = key_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&key_path, &key);

    key
}
