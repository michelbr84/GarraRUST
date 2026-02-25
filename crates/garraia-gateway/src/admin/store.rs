use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use ring::pbkdf2;
use ring::rand::{SecureRandom, SystemRandom};
use rusqlite::{Connection, params};
use std::num::NonZeroU32;
use std::path::Path;
use tracing::info;

use super::rbac::Role;

const PBKDF2_ITERATIONS: u32 = 600_000;
const SALT_LEN: usize = 32;
const SESSION_TOKEN_LEN: usize = 32;
const CSRF_TOKEN_LEN: usize = 32;
const SESSION_DURATION_SECS: i64 = 86400; // 24 hours

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

                CREATE TABLE IF NOT EXISTS config_versions (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    version INTEGER NOT NULL,
                    config_yaml TEXT NOT NULL,
                    created_at TEXT NOT NULL DEFAULT (datetime('now')),
                    created_by TEXT,
                    comment TEXT
                );

                CREATE INDEX IF NOT EXISTS idx_config_versions_version
                    ON config_versions(version);",
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
        let token = generate_random_token(SESSION_TOKEN_LEN)?;
        let csrf_token = generate_random_token(CSRF_TOKEN_LEN)?;

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
        let mut sql = String::from(
            "SELECT id, timestamp, user_id, username, action, resource_type, resource_id, details, ip_address, outcome
             FROM audit_log WHERE 1=1",
        );
        let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(rt) = resource_type {
            sql.push_str(" AND resource_type = ?");
            params_vec.push(Box::new(rt.to_string()));
        }
        if let Some(a) = action {
            sql.push_str(" AND action = ?");
            params_vec.push(Box::new(a.to_string()));
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
    let rng = SystemRandom::new();
    let mut salt = vec![0u8; SALT_LEN];
    rng.fill(&mut salt)
        .map_err(|_| "failed to generate salt".to_string())?;

    let iterations = NonZeroU32::new(PBKDF2_ITERATIONS).expect("iterations > 0");
    let mut hash = vec![0u8; 32];
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        iterations,
        &salt,
        password.as_bytes(),
        &mut hash,
    );

    Ok((BASE64.encode(&hash), BASE64.encode(&salt)))
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

fn generate_random_token(len: usize) -> Result<String, String> {
    let rng = SystemRandom::new();
    let mut buf = vec![0u8; len];
    rng.fill(&mut buf)
        .map_err(|_| "failed to generate random token".to_string())?;
    Ok(BASE64.encode(&buf))
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
