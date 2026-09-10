use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use ring::pbkdf2;
use rusqlite::{Connection, params};
use std::num::NonZeroU32;
use std::path::Path;
use subtle::ConstantTimeEq;
use tracing::info;

use super::rbac::Role;

const PBKDF2_ITERATIONS: u32 = 600_000;
const SALT_LEN: usize = 32;
const SESSION_TOKEN_LEN: usize = 32;
const CSRF_TOKEN_LEN: usize = 32;
const SESSION_DURATION_SECS: i64 = 86400; // 24 hours
/// Bytes of entropy in a recovery code (256 bits). #1122.
const RECOVERY_CODE_LEN: usize = 32;

#[derive(Debug, Clone, serde::Serialize)]
pub struct AdminUser {
    pub id: String,
    pub username: String,
    pub role: Role,
    pub created_at: String,
    pub updated_at: String,
    pub last_login: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AdminSession {
    pub token: String,
    pub user_id: String,
    pub username: String,
    pub role: Role,
    pub csrf_token: String,
    pub created_at: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AuditEntry {
    pub id: i64,
    pub timestamp: String,
    pub user_id: Option<String>,
    pub username: Option<String>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<String>,
    pub details: Option<String>,
    pub ip_address: Option<String>,
    pub outcome: String,
}

/// PBKDF2 parameters the admin master key was derived with, as stored in the
/// single row of `kdf_params`: `(version, salt_base64, iterations)`.
pub type StoredKdfParams = (u32, String, u32);

/// One `secrets` row for the master-key re-key: `(id, encrypted_value, nonce)`.
pub type SecretCiphertext = (String, Vec<u8>, Vec<u8>);
/// One `secret_versions` row: `(id, encrypted_value, nonce)`. Keyed by an
/// autoincrement integer rather than a uuid, hence the separate alias.
pub type SecretVersionCiphertext = (i64, Vec<u8>, Vec<u8>);

pub struct AdminStore {
    conn: Connection,
}

impl AdminStore {
    pub fn open(db_path: &Path) -> Result<Self, String> {
        let conn =
            Connection::open(db_path).map_err(|e| format!("failed to open admin db: {e}"))?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| format!("failed to set pragmas: {e}"))?;

        let store = Self { conn };
        store.run_migrations()?;
        Ok(store)
    }

    pub fn in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory()
            .map_err(|e| format!("failed to open in-memory admin db: {e}"))?;

        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(|e| format!("failed to set pragmas: {e}"))?;

        let store = Self { conn };
        store.run_migrations()?;
        Ok(store)
    }

    fn run_migrations(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS admin_users (
                    id TEXT PRIMARY KEY,
                    username TEXT UNIQUE NOT NULL,
                    password_hash TEXT NOT NULL,
                    password_salt TEXT NOT NULL,
                    role TEXT NOT NULL DEFAULT 'viewer',
                    created_at TEXT NOT NULL DEFAULT (datetime('now')),
                    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                    last_login TEXT
                );

                CREATE TABLE IF NOT EXISTS admin_sessions (
                    token TEXT PRIMARY KEY,
                    user_id TEXT NOT NULL REFERENCES admin_users(id) ON DELETE CASCADE,
                    csrf_token TEXT NOT NULL,
                    created_at TEXT NOT NULL DEFAULT (datetime('now')),
                    expires_at TEXT NOT NULL,
                    ip_address TEXT,
                    user_agent TEXT
                );

                CREATE INDEX IF NOT EXISTS idx_admin_sessions_user
                    ON admin_sessions(user_id);
                CREATE INDEX IF NOT EXISTS idx_admin_sessions_expires
                    ON admin_sessions(expires_at);

                CREATE TABLE IF NOT EXISTS audit_log (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    timestamp TEXT NOT NULL DEFAULT (datetime('now')),
                    user_id TEXT,
                    username TEXT,
                    action TEXT NOT NULL,
                    resource_type TEXT NOT NULL,
                    resource_id TEXT,
                    details TEXT,
                    ip_address TEXT,
                    outcome TEXT NOT NULL DEFAULT 'success'
                );

                CREATE INDEX IF NOT EXISTS idx_audit_log_timestamp
                    ON audit_log(timestamp);
                CREATE INDEX IF NOT EXISTS idx_audit_log_user
                    ON audit_log(user_id);

                CREATE TABLE IF NOT EXISTS secrets (
                    id TEXT PRIMARY KEY,
                    tenant_id TEXT NOT NULL DEFAULT 'default',
                    provider TEXT NOT NULL,
                    key_name TEXT NOT NULL,
                    encrypted_value BLOB NOT NULL,
                    nonce BLOB NOT NULL,
                    is_set INTEGER NOT NULL DEFAULT 1,
                    version INTEGER NOT NULL DEFAULT 1,
                    created_at TEXT NOT NULL DEFAULT (datetime('now')),
                    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                    created_by TEXT,
                    UNIQUE(tenant_id, provider, key_name)
                );

                CREATE TABLE IF NOT EXISTS secret_versions (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    secret_id TEXT NOT NULL REFERENCES secrets(id) ON DELETE CASCADE,
                    version INTEGER NOT NULL,
                    encrypted_value BLOB NOT NULL,
                    nonce BLOB NOT NULL,
                    created_at TEXT NOT NULL DEFAULT (datetime('now')),
                    created_by TEXT
                );

                CREATE TABLE IF NOT EXISTS kdf_params (
                    id INTEGER PRIMARY KEY CHECK (id = 1),
                    version INTEGER NOT NULL,
                    salt TEXT NOT NULL,
                    iterations INTEGER NOT NULL,
                    created_at TEXT NOT NULL DEFAULT (datetime('now'))
                );

                CREATE TABLE IF NOT EXISTS config_versions (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    version INTEGER NOT NULL,
                    config_yaml TEXT NOT NULL,
                    created_at TEXT NOT NULL DEFAULT (datetime('now')),
                    created_by TEXT,
                    comment TEXT
                );

                CREATE INDEX IF NOT EXISTS idx_config_versions_version
                    ON config_versions(version);

                -- #1122: one-time recovery codes. Only the PBKDF2 hash of the
                -- code is stored; the plaintext is handed to the operator
                -- out-of-band (file on the host, read by `garra admin
                -- recovery`) and never persisted here.
                CREATE TABLE IF NOT EXISTS admin_recovery_tokens (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    user_id TEXT NOT NULL REFERENCES admin_users(id) ON DELETE CASCADE,
                    code_hash TEXT NOT NULL,
                    code_salt TEXT NOT NULL,
                    created_at TEXT NOT NULL DEFAULT (datetime('now')),
                    expires_at TEXT NOT NULL,
                    used_at TEXT
                );

                CREATE INDEX IF NOT EXISTS idx_admin_recovery_tokens_user
                    ON admin_recovery_tokens(user_id);",
            )
            .map_err(|e| format!("admin db migration failed: {e}"))?;

        Ok(())
    }

    // ── User management ──────────────────────────────────────────────

    pub fn create_user(
        &self,
        username: &str,
        password: &str,
        role: Role,
    ) -> Result<AdminUser, String> {
        let id = uuid::Uuid::new_v4().to_string();
        let (hash, salt) = hash_password(password)?;

        self.conn
            .execute(
                "INSERT INTO admin_users (id, username, password_hash, password_salt, role)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, username, hash, salt, role.as_str()],
            )
            .map_err(|e| format!("failed to create user: {e}"))?;

        info!(
            "created admin user '{username}' with role '{}'",
            role.as_str()
        );

        self.get_user(&id)
            .ok_or_else(|| "user created but not found".to_string())
    }

    pub fn get_user(&self, id: &str) -> Option<AdminUser> {
        self.conn
            .query_row(
                "SELECT id, username, role, created_at, updated_at, last_login
                 FROM admin_users WHERE id = ?1",
                params![id],
                |row| {
                    Ok(AdminUser {
                        id: row.get(0)?,
                        username: row.get(1)?,
                        role: Role::from_str(&row.get::<_, String>(2)?).unwrap_or(Role::Viewer),
                        created_at: row.get(3)?,
                        updated_at: row.get(4)?,
                        last_login: row.get(5)?,
                    })
                },
            )
            .ok()
    }

    pub fn get_user_by_username(&self, username: &str) -> Option<AdminUser> {
        self.conn
            .query_row(
                "SELECT id, username, role, created_at, updated_at, last_login
                 FROM admin_users WHERE username = ?1",
                params![username],
                |row| {
                    Ok(AdminUser {
                        id: row.get(0)?,
                        username: row.get(1)?,
                        role: Role::from_str(&row.get::<_, String>(2)?).unwrap_or(Role::Viewer),
                        created_at: row.get(3)?,
                        updated_at: row.get(4)?,
                        last_login: row.get(5)?,
                    })
                },
            )
            .ok()
    }

    pub fn list_users(&self) -> Vec<AdminUser> {
        let mut stmt = match self.conn.prepare(
            "SELECT id, username, role, created_at, updated_at, last_login
             FROM admin_users ORDER BY created_at",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        stmt.query_map([], |row| {
            Ok(AdminUser {
                id: row.get(0)?,
                username: row.get(1)?,
                role: Role::from_str(&row.get::<_, String>(2)?).unwrap_or(Role::Viewer),
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
                last_login: row.get(5)?,
            })
        })
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
    }

    pub fn update_user_role(&self, id: &str, role: Role) -> Result<(), String> {
        let affected = self
            .conn
            .execute(
                "UPDATE admin_users SET role = ?1, updated_at = datetime('now') WHERE id = ?2",
                params![role.as_str(), id],
            )
            .map_err(|e| format!("failed to update user role: {e}"))?;

        if affected == 0 {
            return Err("user not found".to_string());
        }
        Ok(())
    }

    pub fn update_user_password(&self, id: &str, password: &str) -> Result<(), String> {
        let (hash, salt) = hash_password(password)?;
        let affected = self
            .conn
            .execute(
                "UPDATE admin_users SET password_hash = ?1, password_salt = ?2, updated_at = datetime('now')
                 WHERE id = ?3",
                params![hash, salt, id],
            )
            .map_err(|e| format!("failed to update password: {e}"))?;

        if affected == 0 {
            return Err("user not found".to_string());
        }
        Ok(())
    }

    pub fn delete_user(&self, id: &str) -> Result<(), String> {
        let affected = self
            .conn
            .execute("DELETE FROM admin_users WHERE id = ?1", params![id])
            .map_err(|e| format!("failed to delete user: {e}"))?;

        if affected == 0 {
            return Err("user not found".to_string());
        }
        Ok(())
    }

    pub fn user_count(&self) -> usize {
        self.conn
            .query_row("SELECT COUNT(*) FROM admin_users", [], |row| row.get(0))
            .unwrap_or(0)
    }

    // ── Password recovery (#1122) ────────────────────────────────────

    /// Mint a one-time recovery code for `user_id` and return `(id, code)`.
    ///
    /// The plaintext code is returned exactly once and stored **nowhere** —
    /// the caller owns the out-of-band handoff. Any code the user still had
    /// pending is dropped first, so at most one live code exists per user and
    /// the handoff file cannot disagree with the table.
    pub fn create_recovery_token(
        &mut self,
        user_id: &str,
        ttl_secs: i64,
    ) -> Result<(i64, String), String> {
        let code = generate_random_token::<RECOVERY_CODE_LEN>()?;
        let (hash, salt) = hash_password(&code)?;
        let expires_at = (chrono::Utc::now() + chrono::Duration::seconds(ttl_secs))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();

        let tx = self
            .conn
            .transaction()
            .map_err(|e| format!("failed to open recovery token transaction: {e}"))?;

        tx.execute(
            "DELETE FROM admin_recovery_tokens WHERE user_id = ?1 AND used_at IS NULL",
            params![user_id],
        )
        .map_err(|e| format!("failed to supersede recovery token: {e}"))?;

        tx.execute(
            "INSERT INTO admin_recovery_tokens (user_id, code_hash, code_salt, expires_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![user_id, hash, salt, expires_at],
        )
        .map_err(|e| format!("failed to insert recovery token: {e}"))?;

        let id = tx.last_insert_rowid();
        tx.commit()
            .map_err(|e| format!("failed to commit recovery token: {e}"))?;

        Ok((id, code))
    }

    /// Drop a minted token. Used when the out-of-band handoff failed, so no
    /// code is ever live that the operator has no way to read.
    pub fn discard_recovery_token(&self, id: i64) -> Result<(), String> {
        self.conn
            .execute(
                "DELETE FROM admin_recovery_tokens WHERE id = ?1",
                params![id],
            )
            .map_err(|e| format!("failed to discard recovery token: {e}"))?;
        Ok(())
    }

    /// Every still-usable token of `user_id`, as `(id, code_hash, code_salt)`.
    pub fn live_recovery_secrets(&self, user_id: &str) -> Vec<(i64, String, String)> {
        let mut stmt = match self.conn.prepare(
            "SELECT id, code_hash, code_salt FROM admin_recovery_tokens
             WHERE user_id = ?1 AND used_at IS NULL AND expires_at > datetime('now')",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        stmt.query_map(params![user_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
    }

    /// Spend one PBKDF2 derivation without persisting anything.
    ///
    /// Called by the "user not found" branch of `/recovery/start` so that
    /// branch costs the same as the real one — otherwise response time would
    /// tell an unauthenticated caller which usernames exist (#1122).
    pub fn burn_recovery_work(&self) -> Result<(), String> {
        let code = generate_random_token::<RECOVERY_CODE_LEN>()?;
        let _ = hash_password(&code)?;
        Ok(())
    }

    /// Constant-time check of a submitted code against one stored row.
    ///
    /// `subtle` on the derived bytes (same precedent as the CSRF check in
    /// `admin/middleware.rs`); the code itself is 256 bits of CSPRNG output,
    /// so the PBKDF2 pass is there to keep one derivation scheme in the admin
    /// surface, not to stretch a low-entropy secret.
    pub fn recovery_code_matches(code: &str, expected_hash: &str, salt: &str) -> bool {
        let (Ok(salt), Ok(expected)) = (BASE64.decode(salt), BASE64.decode(expected_hash)) else {
            return false;
        };
        let derived = derive_hash(code, &salt);
        derived.ct_eq(&expected).unwrap_u8() == 1
    }

    /// Atomically claim token `id`. Exactly one caller gets `true` when two
    /// `/recovery/complete` requests race on the same code — the loser is
    /// indistinguishable from a wrong code.
    pub fn claim_recovery_token(&self, id: i64) -> Result<bool, String> {
        let affected = self
            .conn
            .execute(
                "UPDATE admin_recovery_tokens SET used_at = datetime('now')
                 WHERE id = ?1 AND used_at IS NULL",
                params![id],
            )
            .map_err(|e| format!("failed to claim recovery token: {e}"))?;
        Ok(affected == 1)
    }

    /// Best-effort purge of tokens that expired or were consumed.
    pub fn purge_recovery_tokens(&self) -> usize {
        self.conn
            .execute(
                "DELETE FROM admin_recovery_tokens
                 WHERE used_at IS NOT NULL OR expires_at <= datetime('now')",
                [],
            )
            .unwrap_or(0)
    }

    // ── Authentication ───────────────────────────────────────────────

    pub fn verify_password(&self, username: &str, password: &str) -> Option<AdminUser> {
        let row: Option<(String, String, String, String)> = self
            .conn
            .query_row(
                "SELECT id, password_hash, password_salt, role FROM admin_users WHERE username = ?1",
                params![username],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .ok();

        let (user_id, stored_hash, stored_salt, _role) = row?;

        if verify_password_hash(password, &stored_hash, &stored_salt) {
            let _ = self.conn.execute(
                "UPDATE admin_users SET last_login = datetime('now') WHERE id = ?1",
                params![user_id],
            );
            self.get_user(&user_id)
        } else {
            None
        }
    }

    // ── Session management ───────────────────────────────────────────

    pub fn create_session(
        &self,
        user_id: &str,
        ip_address: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<AdminSession, String> {
        let token = generate_random_token::<SESSION_TOKEN_LEN>()?;
        let csrf_token = generate_random_token::<CSRF_TOKEN_LEN>()?;

        let expires_at = chrono::Utc::now() + chrono::Duration::seconds(SESSION_DURATION_SECS);
        let expires_str = expires_at.format("%Y-%m-%d %H:%M:%S").to_string();

        self.conn
            .execute(
                "INSERT INTO admin_sessions (token, user_id, csrf_token, expires_at, ip_address, user_agent)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![token, user_id, csrf_token, expires_str, ip_address, user_agent],
            )
            .map_err(|e| format!("failed to create session: {e}"))?;

        self.validate_session(&token)
            .ok_or_else(|| "session created but not found".to_string())
    }

    pub fn validate_session(&self, token: &str) -> Option<AdminSession> {
        self.conn
            .query_row(
                "SELECT s.token, s.user_id, u.username, u.role, s.csrf_token, s.created_at, s.expires_at
                 FROM admin_sessions s
                 JOIN admin_users u ON u.id = s.user_id
                 WHERE s.token = ?1 AND s.expires_at > datetime('now')",
                params![token],
                |row| {
                    Ok(AdminSession {
                        token: row.get(0)?,
                        user_id: row.get(1)?,
                        username: row.get(2)?,
                        role: Role::from_str(&row.get::<_, String>(3)?).unwrap_or(Role::Viewer),
                        csrf_token: row.get(4)?,
                        created_at: row.get(5)?,
                        expires_at: row.get(6)?,
                    })
                },
            )
            .ok()
    }

    pub fn delete_session(&self, token: &str) -> Result<(), String> {
        self.conn
            .execute(
                "DELETE FROM admin_sessions WHERE token = ?1",
                params![token],
            )
            .map_err(|e| format!("failed to delete session: {e}"))?;
        Ok(())
    }

    pub fn delete_user_sessions(&self, user_id: &str) -> Result<(), String> {
        self.conn
            .execute(
                "DELETE FROM admin_sessions WHERE user_id = ?1",
                params![user_id],
            )
            .map_err(|e| format!("failed to delete user sessions: {e}"))?;
        Ok(())
    }

    pub fn cleanup_expired_sessions(&self) -> usize {
        self.conn
            .execute(
                "DELETE FROM admin_sessions WHERE expires_at <= datetime('now')",
                [],
            )
            .unwrap_or(0)
    }

    // ── Audit log ────────────────────────────────────────────────────

    pub fn append_audit(
        &self,
        user_id: Option<&str>,
        username: Option<&str>,
        action: &str,
        resource_type: &str,
        resource_id: Option<&str>,
        details: Option<&str>,
        ip_address: Option<&str>,
        outcome: &str,
    ) -> Result<i64, String> {
        self.conn
            .execute(
                "INSERT INTO audit_log (user_id, username, action, resource_type, resource_id, details, ip_address, outcome)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![user_id, username, action, resource_type, resource_id, details, ip_address, outcome],
            )
            .map_err(|e| format!("failed to append audit log: {e}"))?;

        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_audit_log(
        &self,
        limit: usize,
        offset: usize,
        resource_type: Option<&str>,
        action: Option<&str>,
    ) -> Vec<AuditEntry> {
        self.list_audit_log_filtered(limit, offset, None, action, resource_type, None, None)
    }

    /// Extended audit-log query supporting all Phase-7.1 filter fields.
    pub fn list_audit_log_filtered(
        &self,
        limit: usize,
        offset: usize,
        user_id: Option<&str>,
        action: Option<&str>,
        resource_type: Option<&str>,
        from: Option<&str>,
        to: Option<&str>,
    ) -> Vec<AuditEntry> {
        let mut sql = String::from(
            "SELECT id, timestamp, user_id, username, action, resource_type, resource_id, details, ip_address, outcome
             FROM audit_log WHERE 1=1",
        );
        let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(uid) = user_id {
            sql.push_str(" AND user_id = ?");
            params_vec.push(Box::new(uid.to_string()));
        }
        if let Some(a) = action {
            sql.push_str(" AND action = ?");
            params_vec.push(Box::new(a.to_string()));
        }
        if let Some(rt) = resource_type {
            sql.push_str(" AND resource_type = ?");
            params_vec.push(Box::new(rt.to_string()));
        }
        if let Some(f) = from {
            sql.push_str(" AND timestamp >= ?");
            params_vec.push(Box::new(f.to_string()));
        }
        if let Some(t) = to {
            sql.push_str(" AND timestamp <= ?");
            params_vec.push(Box::new(t.to_string()));
        }

        sql.push_str(" ORDER BY timestamp DESC LIMIT ? OFFSET ?");
        params_vec.push(Box::new(limit as i64));
        params_vec.push(Box::new(offset as i64));

        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        let mut stmt = match self.conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        stmt.query_map(param_refs.as_slice(), |row| {
            Ok(AuditEntry {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                user_id: row.get(2)?,
                username: row.get(3)?,
                action: row.get(4)?,
                resource_type: row.get(5)?,
                resource_id: row.get(6)?,
                details: row.get(7)?,
                ip_address: row.get(8)?,
                outcome: row.get(9)?,
            })
        })
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
    }

    // ── Secrets store ────────────────────────────────────────────────

    /// One row of the re-key scan: primary key, ciphertext, nonce.
    ///
    /// `secrets` is keyed by a uuid string, `secret_versions` by an autoincrement
    /// integer, hence the two aliases.
    /// Every live ciphertext, as `(secrets.id, encrypted_value, nonce)`.
    ///
    /// Used only by the master-key re-key migration in
    /// [`super::shared::migrate_admin_secrets_kdf`]. Archived history lives in
    /// `secret_versions` and is fetched by [`Self::secret_version_ciphertexts`].
    pub fn secret_ciphertexts(&self) -> Result<Vec<SecretCiphertext>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, encrypted_value, nonce FROM secrets")
            .map_err(|e| format!("failed to prepare secrets scan: {e}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                ))
            })
            .map_err(|e| format!("failed to scan secrets: {e}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("failed to read secrets row: {e}"))
    }

    /// Archived ciphertexts, as `(secret_versions.id, encrypted_value, nonce)`.
    pub fn secret_version_ciphertexts(&self) -> Result<Vec<SecretVersionCiphertext>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, encrypted_value, nonce FROM secret_versions")
            .map_err(|e| format!("failed to prepare secret_versions scan: {e}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                ))
            })
            .map_err(|e| format!("failed to scan secret_versions: {e}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("failed to read secret_versions row: {e}"))
    }

    /// The PBKDF2 parameters the master key was derived with, if this
    /// installation has already been through the re-key.
    pub fn kdf_params(&self) -> Option<StoredKdfParams> {
        self.conn
            .query_row(
                "SELECT version, salt, iterations FROM kdf_params WHERE id = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)? as u32,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)? as u32,
                    ))
                },
            )
            .ok()
            .filter(|(_, salt, iterations)| !salt.is_empty() && *iterations > 0)
    }

    /// Overwrite every secret ciphertext **and** record the KDF parameters that
    /// the new ciphertexts are under, in ONE transaction.
    ///
    /// The parameters live in this table rather than in a sidecar file on
    /// purpose. An earlier revision committed the SQL first and wrote a
    /// `kdf.json` afterwards; if that second write failed (disk full, bad
    /// permissions on the config dir) the ciphertexts were already under the
    /// new key while nothing recorded which key that was, and the next boot
    /// would mint a *third* salt and be unable to decrypt anything. Same
    /// medium, same transaction, no window.
    ///
    /// Fail-closed by construction: the caller decrypts with the old key and
    /// re-encrypts with the new one *before* calling this, so a failed re-key
    /// leaves the store byte-identical (the transaction rolls back on any error
    /// and on drop). Called once, at boot, by
    /// [`super::shared::migrate_admin_secrets_kdf`].
    ///
    /// Returns the number of rows the UPDATEs actually modified — not the
    /// number scanned, which can differ if a secret is deleted between the scan
    /// and the re-key.
    pub fn apply_secret_rekey(
        &mut self,
        secrets: &[SecretCiphertext],
        versions: &[SecretVersionCiphertext],
        kdf: &StoredKdfParams,
    ) -> Result<usize, String> {
        let (version, salt, iterations) = kdf;
        let tx = self
            .conn
            .transaction()
            .map_err(|e| format!("failed to open rekey transaction: {e}"))?;

        let mut changed = 0usize;
        for (id, encrypted, nonce) in secrets {
            changed += tx
                .execute(
                    "UPDATE secrets SET encrypted_value = ?1, nonce = ?2 WHERE id = ?3",
                    params![encrypted, nonce, id],
                )
                .map_err(|e| format!("failed to rekey secret {id}: {e}"))?;
        }
        for (id, encrypted, nonce) in versions {
            changed += tx
                .execute(
                    "UPDATE secret_versions SET encrypted_value = ?1, nonce = ?2 WHERE id = ?3",
                    params![encrypted, nonce, id],
                )
                .map_err(|e| format!("failed to rekey secret version {id}: {e}"))?;
        }

        tx.execute(
            "INSERT INTO kdf_params (id, version, salt, iterations) VALUES (1, ?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET version = ?1, salt = ?2, iterations = ?3",
            params![*version as i64, salt, *iterations as i64],
        )
        .map_err(|e| format!("failed to record kdf params: {e}"))?;

        tx.commit()
            .map_err(|e| format!("failed to commit rekey transaction: {e}"))?;
        Ok(changed)
    }

    pub fn set_secret(
        &self,
        tenant_id: &str,
        provider: &str,
        key_name: &str,
        encrypted_value: &[u8],
        nonce: &[u8],
        created_by: Option<&str>,
    ) -> Result<String, String> {
        let id = uuid::Uuid::new_v4().to_string();

        let existing = self.get_secret_meta(tenant_id, provider, key_name);

        if let Some(existing) = existing {
            let new_version: i64 = self
                .conn
                .query_row(
                    "SELECT COALESCE(MAX(version), 0) + 1 FROM secret_versions WHERE secret_id = ?1",
                    params![existing.id],
                    |row| row.get(0),
                )
                .unwrap_or(1);

            self.conn
                .execute(
                    "INSERT INTO secret_versions (secret_id, version, encrypted_value, nonce, created_by)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![existing.id, existing.version, existing.encrypted_value, existing.nonce, created_by],
                )
                .map_err(|e| format!("failed to archive secret version: {e}"))?;

            self.conn
                .execute(
                    "UPDATE secrets SET encrypted_value = ?1, nonce = ?2, version = ?3, updated_at = datetime('now'), created_by = ?4
                     WHERE id = ?5",
                    params![encrypted_value, nonce, new_version, created_by, existing.id],
                )
                .map_err(|e| format!("failed to update secret: {e}"))?;

            Ok(existing.id)
        } else {
            self.conn
                .execute(
                    "INSERT INTO secrets (id, tenant_id, provider, key_name, encrypted_value, nonce, created_by)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![id, tenant_id, provider, key_name, encrypted_value, nonce, created_by],
                )
                .map_err(|e| format!("failed to insert secret: {e}"))?;

            Ok(id)
        }
    }

    pub fn delete_secret(
        &self,
        tenant_id: &str,
        provider: &str,
        key_name: &str,
    ) -> Result<(), String> {
        let affected = self
            .conn
            .execute(
                "DELETE FROM secrets WHERE tenant_id = ?1 AND provider = ?2 AND key_name = ?3",
                params![tenant_id, provider, key_name],
            )
            .map_err(|e| format!("failed to delete secret: {e}"))?;

        if affected == 0 {
            return Err("secret not found".to_string());
        }
        Ok(())
    }

    pub fn list_secrets(&self, tenant_id: &str) -> Vec<SecretMeta> {
        let mut stmt = match self.conn.prepare(
            "SELECT id, tenant_id, provider, key_name, encrypted_value, nonce, is_set, version, created_at, updated_at, created_by
             FROM secrets WHERE tenant_id = ?1 ORDER BY provider, key_name",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        stmt.query_map(params![tenant_id], |row| {
            Ok(SecretMeta {
                id: row.get(0)?,
                tenant_id: row.get(1)?,
                provider: row.get(2)?,
                key_name: row.get(3)?,
                encrypted_value: row.get(4)?,
                nonce: row.get(5)?,
                is_set: row.get::<_, i32>(6)? != 0,
                version: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
                created_by: row.get(10)?,
            })
        })
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
    }

    pub fn get_secret_meta(
        &self,
        tenant_id: &str,
        provider: &str,
        key_name: &str,
    ) -> Option<SecretMeta> {
        self.conn
            .query_row(
                "SELECT id, tenant_id, provider, key_name, encrypted_value, nonce, is_set, version, created_at, updated_at, created_by
                 FROM secrets WHERE tenant_id = ?1 AND provider = ?2 AND key_name = ?3",
                params![tenant_id, provider, key_name],
                |row| {
                    Ok(SecretMeta {
                        id: row.get(0)?,
                        tenant_id: row.get(1)?,
                        provider: row.get(2)?,
                        key_name: row.get(3)?,
                        encrypted_value: row.get(4)?,
                        nonce: row.get(5)?,
                        is_set: row.get::<_, i32>(6)? != 0,
                        version: row.get(7)?,
                        created_at: row.get(8)?,
                        updated_at: row.get(9)?,
                        created_by: row.get(10)?,
                    })
                },
            )
            .ok()
    }

    pub fn get_secret_raw(
        &self,
        tenant_id: &str,
        provider: &str,
        key_name: &str,
    ) -> Option<(Vec<u8>, Vec<u8>)> {
        self.conn
            .query_row(
                "SELECT encrypted_value, nonce FROM secrets WHERE tenant_id = ?1 AND provider = ?2 AND key_name = ?3",
                params![tenant_id, provider, key_name],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok()
    }

    pub fn list_secret_versions(&self, secret_id: &str) -> Vec<SecretVersionEntry> {
        let mut stmt = match self.conn.prepare(
            "SELECT id, version, created_at, created_by FROM secret_versions
             WHERE secret_id = ?1 ORDER BY version DESC",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        stmt.query_map(params![secret_id], |row| {
            Ok(SecretVersionEntry {
                id: row.get(0)?,
                version: row.get(1)?,
                created_at: row.get(2)?,
                created_by: row.get(3)?,
            })
        })
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
    }

    // ── Config versions ──────────────────────────────────────────────

    pub fn save_config_version(
        &self,
        config_yaml: &str,
        created_by: Option<&str>,
        comment: Option<&str>,
    ) -> Result<i64, String> {
        let version: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) + 1 FROM config_versions",
                [],
                |row| row.get(0),
            )
            .unwrap_or(1);

        self.conn
            .execute(
                "INSERT INTO config_versions (version, config_yaml, created_by, comment)
                 VALUES (?1, ?2, ?3, ?4)",
                params![version, config_yaml, created_by, comment],
            )
            .map_err(|e| format!("failed to save config version: {e}"))?;

        Ok(version)
    }

    pub fn list_config_versions(&self, limit: usize) -> Vec<ConfigVersionEntry> {
        let mut stmt = match self.conn.prepare(
            "SELECT id, version, created_at, created_by, comment
             FROM config_versions ORDER BY version DESC LIMIT ?1",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        stmt.query_map(params![limit as i64], |row| {
            Ok(ConfigVersionEntry {
                id: row.get(0)?,
                version: row.get(1)?,
                created_at: row.get(2)?,
                created_by: row.get(3)?,
                comment: row.get(4)?,
            })
        })
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
    }

    pub fn get_config_version(&self, version: i64) -> Option<String> {
        self.conn
            .query_row(
                "SELECT config_yaml FROM config_versions WHERE version = ?1",
                params![version],
                |row| row.get(0),
            )
            .ok()
    }

    pub fn get_latest_config_version(&self) -> Option<(i64, String)> {
        self.conn
            .query_row(
                "SELECT version, config_yaml FROM config_versions ORDER BY version DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok()
    }
}

#[derive(Debug, Clone)]
pub struct SecretMeta {
    pub id: String,
    pub tenant_id: String,
    pub provider: String,
    pub key_name: String,
    pub encrypted_value: Vec<u8>,
    pub nonce: Vec<u8>,
    pub is_set: bool,
    pub version: i64,
    pub created_at: String,
    pub updated_at: String,
    pub created_by: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SecretVersionEntry {
    pub id: i64,
    pub version: i64,
    pub created_at: String,
    pub created_by: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ConfigVersionEntry {
    pub id: i64,
    pub version: i64,
    pub created_at: String,
    pub created_by: Option<String>,
    pub comment: Option<String>,
}

// ── Password hashing ─────────────────────────────────────────────────

fn hash_password(password: &str) -> Result<(String, String), String> {
    let salt = garraia_security::random_bytes::<SALT_LEN>()
        .map_err(|_| "failed to generate salt".to_string())?;

    let hash = derive_hash(password, &salt);
    Ok((BASE64.encode(&hash), BASE64.encode(salt)))
}

/// Single PBKDF2-HMAC-SHA256 derivation. Shared by password hashing (#1122
/// keeps one scheme in the admin surface) and by recovery-code verification.
fn derive_hash(secret: &str, salt: &[u8]) -> Vec<u8> {
    let iterations = NonZeroU32::new(PBKDF2_ITERATIONS).expect("iterations > 0");
    let mut hash = vec![0u8; 32];
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        iterations,
        salt,
        secret.as_bytes(),
        &mut hash,
    );
    hash
}

fn verify_password_hash(password: &str, stored_hash: &str, stored_salt: &str) -> bool {
    let Ok(salt) = BASE64.decode(stored_salt) else {
        return false;
    };
    let Ok(expected_hash) = BASE64.decode(stored_hash) else {
        return false;
    };

    let iterations = NonZeroU32::new(PBKDF2_ITERATIONS).expect("iterations > 0");
    pbkdf2::verify(
        pbkdf2::PBKDF2_HMAC_SHA256,
        iterations,
        &salt,
        password.as_bytes(),
        &expected_hash,
    )
    .is_ok()
}

fn generate_random_token<const N: usize>() -> Result<String, String> {
    let buf = garraia_security::random_bytes::<N>()
        .map_err(|_| "failed to generate random token".to_string())?;
    Ok(BASE64.encode(buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> AdminStore {
        AdminStore::in_memory().expect("in-memory store should work")
    }

    #[test]
    fn create_and_verify_user() {
        let store = test_store();
        let user = store
            .create_user("admin", "secret123", Role::Admin)
            .unwrap();
        assert_eq!(user.username, "admin");
        assert_eq!(user.role, Role::Admin);

        let verified = store.verify_password("admin", "secret123");
        assert!(verified.is_some());

        let bad = store.verify_password("admin", "wrong");
        assert!(bad.is_none());
    }

    #[test]
    fn session_lifecycle() {
        let store = test_store();
        let user = store.create_user("op", "pass", Role::Operator).unwrap();

        let session = store
            .create_session(&user.id, Some("127.0.0.1"), None)
            .unwrap();
        assert!(!session.token.is_empty());
        assert!(!session.csrf_token.is_empty());

        let validated = store.validate_session(&session.token);
        assert!(validated.is_some());
        assert_eq!(validated.unwrap().user_id, user.id);

        store.delete_session(&session.token).unwrap();
        assert!(store.validate_session(&session.token).is_none());
    }

    #[test]
    fn audit_log_round_trip() {
        let store = test_store();
        store
            .append_audit(
                Some("user1"),
                Some("admin"),
                "create",
                "secret",
                Some("OPENAI_KEY"),
                None,
                Some("127.0.0.1"),
                "success",
            )
            .unwrap();

        let entries = store.list_audit_log(10, 0, None, None);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].action, "create");
        assert_eq!(entries[0].resource_type, "secret");
    }

    #[test]
    fn user_count() {
        let store = test_store();
        assert_eq!(store.user_count(), 0);
        store.create_user("a", "p", Role::Viewer).unwrap();
        assert_eq!(store.user_count(), 1);
    }

    #[test]
    fn duplicate_username_fails() {
        let store = test_store();
        store.create_user("admin", "pass", Role::Admin).unwrap();
        let result = store.create_user("admin", "pass2", Role::Viewer);
        assert!(result.is_err());
    }

    #[test]
    fn delete_user_cascades_sessions() {
        let store = test_store();
        let user = store.create_user("temp", "pass", Role::Viewer).unwrap();
        let session = store.create_session(&user.id, None, None).unwrap();

        store.delete_user(&user.id).unwrap();
        assert!(store.validate_session(&session.token).is_none());
    }

    // ── #1122: recovery tokens ───────────────────────────────────────

    #[test]
    fn recovery_token_is_single_use() {
        let mut store = test_store();
        let user = store.create_user("admin", "pass", Role::Admin).unwrap();

        let (id, code) = store.create_recovery_token(&user.id, 600).unwrap();
        assert!(!code.is_empty());

        let live = store.live_recovery_secrets(&user.id);
        assert_eq!(live.len(), 1);
        assert!(AdminStore::recovery_code_matches(
            &code, &live[0].1, &live[0].2
        ));
        assert!(!AdminStore::recovery_code_matches(
            "wrong", &live[0].1, &live[0].2
        ));

        // Exactly one winner: the second claim is indistinguishable from a
        // bad code, so concurrent /complete calls cannot both reset.
        assert!(store.claim_recovery_token(id).unwrap());
        assert!(!store.claim_recovery_token(id).unwrap());
        assert!(store.live_recovery_secrets(&user.id).is_empty());
    }

    #[test]
    fn recovery_token_expires() {
        let mut store = test_store();
        let user = store.create_user("admin", "pass", Role::Admin).unwrap();

        let (_id, _code) = store.create_recovery_token(&user.id, -1).unwrap();
        assert!(store.live_recovery_secrets(&user.id).is_empty());
    }

    #[test]
    fn recovery_token_reissue_supersedes_the_previous_one() {
        let mut store = test_store();
        let user = store.create_user("admin", "pass", Role::Admin).unwrap();

        let (_old_id, old_code) = store.create_recovery_token(&user.id, 600).unwrap();
        let (_new_id, new_code) = store.create_recovery_token(&user.id, 600).unwrap();
        assert_ne!(old_code, new_code);

        let live = store.live_recovery_secrets(&user.id);
        assert_eq!(live.len(), 1);
        assert!(AdminStore::recovery_code_matches(
            &new_code, &live[0].1, &live[0].2
        ));
        assert!(!AdminStore::recovery_code_matches(
            &old_code, &live[0].1, &live[0].2
        ));
    }

    #[test]
    fn recovery_token_discard_and_purge() {
        let mut store = test_store();
        let user = store.create_user("admin", "pass", Role::Admin).unwrap();

        let (id, _code) = store.create_recovery_token(&user.id, 600).unwrap();
        store.discard_recovery_token(id).unwrap();
        assert!(store.live_recovery_secrets(&user.id).is_empty());

        store.create_recovery_token(&user.id, -1).unwrap();
        assert_eq!(store.purge_recovery_tokens(), 1);
    }

    #[test]
    fn config_version_round_trip() {
        let store = test_store();
        let v1 = store
            .save_config_version("key: value1", Some("admin"), Some("initial"))
            .unwrap();
        assert_eq!(v1, 1);

        let v2 = store
            .save_config_version("key: value2", Some("admin"), Some("update"))
            .unwrap();
        assert_eq!(v2, 2);

        let versions = store.list_config_versions(10);
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].version, 2);

        let yaml = store.get_config_version(1).unwrap();
        assert_eq!(yaml, "key: value1");
    }
}
