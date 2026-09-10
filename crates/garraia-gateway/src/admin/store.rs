use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use ring::pbkdf2;
use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::path::Path;
use std::time::{Duration, Instant};
use tracing::info;

use super::rbac::Role;

const PBKDF2_ITERATIONS: u32 = 600_000;
const SALT_LEN: usize = 32;
const SESSION_TOKEN_LEN: usize = 32;
const CSRF_TOKEN_LEN: usize = 32;
const SESSION_DURATION_SECS: i64 = 86400; // 24 hours

/// Tentativas erradas de TOTP que um usuario acumula dentro de
/// `TOTP_ATTEMPT_WINDOW` antes de o segundo fator travar (#1121).
///
/// Sem isso, o segundo fator e decorativo: um codigo de 6 digitos com a deriva
/// de +-1 janela aceita 3 de cada 1e6 chutes, e `verify_totp` custa
/// microssegundos — ao contrario da senha, que passa por 600k iteracoes de
/// PBKDF2 e portanto ja e cara de forcar.
const TOTP_MAX_ATTEMPTS: usize = 5;
/// Janela da contagem acima. Reiniciar o gateway limpa a contagem junto com o
/// travamento: e estado de processo, nao de banco, de proposito — persisti-lo
/// deixaria um atacante capaz de travar o dono por mais tempo do que o restart
/// resolve.
const TOTP_ATTEMPT_WINDOW: Duration = Duration::from_secs(15 * 60);

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
    /// Tentativas erradas de TOTP por usuario — ver `TOTP_MAX_ATTEMPTS`.
    totp_attempts: HashMap<String, Vec<Instant>>,
}

impl AdminStore {
    pub fn open(db_path: &Path) -> Result<Self, String> {
        let conn =
            Connection::open(db_path).map_err(|e| format!("failed to open admin db: {e}"))?;

        // O `busy_timeout` nao e enfeite: `run_migrations` escreve no schema
        // (ALTER TABLE das colunas de 2FA) e sem ele um segundo processo com o
        // arquivo aberto devolve SQLITE_BUSY na hora, em vez de esperar. A
        // consequencia la na frente e grave — ver `ensure_totp_columns`.
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;",
        )
        .map_err(|e| format!("failed to set pragmas: {e}"))?;

        let store = Self {
            conn,
            totp_attempts: HashMap::new(),
        };
        store.run_migrations()?;
        Ok(store)
    }

    pub fn in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory()
            .map_err(|e| format!("failed to open in-memory admin db: {e}"))?;

        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(|e| format!("failed to set pragmas: {e}"))?;

        let store = Self {
            conn,
            totp_attempts: HashMap::new(),
        };
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
                    ON config_versions(version);",
            )
            .map_err(|e| format!("admin db migration failed: {e}"))?;

        self.ensure_totp_columns()?;

        Ok(())
    }

    /// Colunas do segundo fator (#1121), adicionadas a um banco que ja existe.
    ///
    /// O SQLite nao tem `ADD COLUMN IF NOT EXISTS`, entao a unica forma
    /// idempotente de evoluir `admin_users` e perguntar ao schema antes — o
    /// `CREATE TABLE IF NOT EXISTS` do bloco acima nao toca em tabela que ja
    /// nasceu sem essas colunas. Os identificadores abaixo sao literais:
    /// nenhum deles vem de fora do codigo.
    ///
    /// Ler o schema e depois escrever nao e atomico, e por isso o perdedor da
    /// corrida (dois processos subindo juntos leem o `PRAGMA` antes de
    /// qualquer `ALTER`) nao pode receber `Err`: `AdminStore::open` cairia
    /// para `in_memory()`, que deixa o `/admin/api/setup` reivindicavel por
    /// anonimo. A prova de que a corrida foi vencida por alguem e reler o
    /// schema apos o erro — a mensagem `duplicate column name` do SQLite nao
    /// e contrato estavel entre versoes. Erro de leitura do schema, esse sim,
    /// e propagado: migrar com metade das linhas lidas seria pior que nao
    /// migrar.
    fn ensure_totp_columns(&self) -> Result<(), String> {
        for (column, ddl) in [
            (
                "totp_secret",
                "ALTER TABLE admin_users ADD COLUMN totp_secret TEXT",
            ),
            (
                "totp_enabled",
                "ALTER TABLE admin_users ADD COLUMN totp_enabled INTEGER NOT NULL DEFAULT 0",
            ),
        ] {
            if self.has_admin_user_column(column)? {
                continue;
            }
            if let Err(e) = self.conn.execute(ddl, []) {
                if self.has_admin_user_column(column)? {
                    continue;
                }
                return Err(format!("failed to add admin_users.{column}: {e}"));
            }
        }
        Ok(())
    }

    /// `true` quando a coluna ja existe em `admin_users`. Erro de leitura do
    /// schema e erro mesmo — sem `filter_map(|r| r.ok())` engolindo metade
    /// das linhas e migrando no achismo.
    fn has_admin_user_column(&self, column: &str) -> Result<bool, String> {
        let mut stmt = self
            .conn
            .prepare("PRAGMA table_info(admin_users)")
            .map_err(|e| format!("failed to read admin_users schema: {e}"))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| format!("failed to read admin_users schema: {e}"))?;
        for row in rows {
            let name = row.map_err(|e| format!("failed to read admin_users schema: {e}"))?;
            if name == column {
                return Ok(true);
            }
        }
        Ok(false)
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

    // ── TOTP: segundo fator do painel (#1121) ─────────────────────────

    /// Segredo TOTP do usuario — tanto o pendente de confirmacao quanto o
    /// ativo. `None` tambem significa "coluna existe, valor e NULL".
    ///
    /// O segredo fica **em claro** no `admin.db` (base32, sem cifrar). Isso
    /// nao e paridade com o fluxo mobile — la o segredo e cifrado
    /// (`totp_secret_enc`, AES-256-GCM com a chave do cofre). Aqui a chave
    /// existe e esta na mao do handler (`AdminState::encryption_key`), e o
    /// `admin/secrets.rs` ja cifra as chaves de provider no mesmo arquivo:
    /// cifrar custa uma chamada e nenhuma decisao nova, e e a issue de
    /// seguimento deste PR.
    ///
    /// O que torna o risco aceitavel por enquanto nao e essa comparacao: e o
    /// proprio `admin.db` ja guardar `admin_sessions.token` em texto puro, ou
    /// seja, quem le o arquivo ja leva uma sessao de admin viva sem precisar
    /// do segundo fator. O segredo em claro adiciona comprometimento duravel
    /// do 2FA (sobrevive a expiracao da sessao e a troca de senha), nao uma
    /// porta nova.
    pub fn get_totp_secret(&self, user_id: &str) -> Option<String> {
        self.conn
            .query_row(
                "SELECT totp_secret FROM admin_users WHERE id = ?1",
                params![user_id],
                |row| row.get(0),
            )
            .ok()
            .flatten()
    }

    /// Guarda o segredo como **pendente**: `totp_enabled` so vira 1 em
    /// `enable_totp`, depois de um codigo validar. Quem chama setup e nao
    /// confirma fica com um segredo inerte, nao com 2FA pela metade.
    pub fn set_pending_totp_secret(&self, user_id: &str, secret: &str) -> Result<(), String> {
        Self::set_pending_totp_secret_on(&self.conn, user_id, secret)
    }

    fn set_pending_totp_secret_on(
        conn: &rusqlite::Connection,
        user_id: &str,
        secret: &str,
    ) -> Result<(), String> {
        let affected = conn
            .execute(
                "UPDATE admin_users SET totp_secret = ?1, updated_at = datetime('now')
                 WHERE id = ?2",
                params![secret, user_id],
            )
            .map_err(|e| format!("failed to store totp secret: {e}"))?;
        if affected == 0 {
            return Err("user not found".to_string());
        }
        Ok(())
    }

    /// Guarda o segredo pendente **e grava o evento de auditoria na mesma
    /// transacao**: sem o evento registrado, a mudanca nao acontece — o
    /// `commit` so roda com os dois, e quem chamou recebe `Err`. Nenhuma
    /// operacao de 2FA confirma sem a trilha dela (#1121).
    pub fn set_pending_totp_secret_audited(
        &self,
        user_id: &str,
        secret: &str,
        username: &str,
        ip: Option<&str>,
    ) -> Result<(), String> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| format!("failed to begin transaction: {e}"))?;
        Self::set_pending_totp_secret_on(&tx, user_id, secret)?;
        Self::append_audit_on(
            &tx,
            Some(user_id),
            Some(username),
            "2fa.setup",
            "auth",
            None,
            None,
            ip,
            "success",
        )?;
        tx.commit()
            .map_err(|e| format!("failed to commit 2fa setup: {e}"))
    }

    /// Confirma o enrollment. So deve ser chamado com um codigo ja validado.
    pub fn enable_totp(&self, user_id: &str) -> Result<(), String> {
        Self::enable_totp_on(&self.conn, user_id)
    }

    fn enable_totp_on(conn: &rusqlite::Connection, user_id: &str) -> Result<(), String> {
        let affected = conn
            .execute(
                "UPDATE admin_users SET totp_enabled = 1, updated_at = datetime('now')
                 WHERE id = ?1",
                params![user_id],
            )
            .map_err(|e| format!("failed to enable totp: {e}"))?;
        if affected == 0 {
            return Err("user not found".to_string());
        }
        Ok(())
    }

    /// Liga o segundo fator + grava o evento na mesma transacao — mesma
    /// regra de `set_pending_totp_secret_audited` (#1121).
    pub fn enable_totp_audited(
        &self,
        user_id: &str,
        username: &str,
        ip: Option<&str>,
    ) -> Result<(), String> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| format!("failed to begin transaction: {e}"))?;
        Self::enable_totp_on(&tx, user_id)?;
        Self::append_audit_on(
            &tx,
            Some(user_id),
            Some(username),
            "2fa.verify",
            "auth",
            None,
            None,
            ip,
            "success",
        )?;
        tx.commit()
            .map_err(|e| format!("failed to commit 2fa enable: {e}"))
    }

    /// Desliga o segundo fator e apaga o segredo junto — um segredo guardado
    /// depois de desligado seria uma reativacao sem novo enrollment.
    pub fn disable_totp(&self, user_id: &str) -> Result<(), String> {
        Self::disable_totp_on(&self.conn, user_id)
    }

    fn disable_totp_on(conn: &rusqlite::Connection, user_id: &str) -> Result<(), String> {
        let affected = conn
            .execute(
                "UPDATE admin_users SET totp_secret = NULL, totp_enabled = 0,
                 updated_at = datetime('now')
                 WHERE id = ?1",
                params![user_id],
            )
            .map_err(|e| format!("failed to disable totp: {e}"))?;
        if affected == 0 {
            return Err("user not found".to_string());
        }
        Ok(())
    }

    /// Desliga o segundo fator + grava o evento na mesma transacao — mesma
    /// regra de `set_pending_totp_secret_audited`. Desligar o 2FA e
    /// exatamente o que um invasor com a senha tentaria; sem trilha
    /// gravada, a operacao nao confirma (#1121).
    pub fn disable_totp_audited(
        &self,
        user_id: &str,
        username: &str,
        ip: Option<&str>,
    ) -> Result<(), String> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| format!("failed to begin transaction: {e}"))?;
        Self::disable_totp_on(&tx, user_id)?;
        Self::append_audit_on(
            &tx,
            Some(user_id),
            Some(username),
            "2fa.disable",
            "auth",
            None,
            None,
            ip,
            "success",
        )?;
        tx.commit()
            .map_err(|e| format!("failed to commit 2fa disable: {e}"))
    }

    /// Estado do segundo fator — **ou o erro que impediu de sabe-lo**.
    ///
    /// A assinatura e `Result` de proposito (#1121): quem decide
    /// autenticacao nao pode tratar "nao consegui ler" como "2FA
    /// desligado" — esse era exatamente o fail-open que o PR veio fechar.
    /// Todo chamador ou usa o valor, ou recusa fechado (login responde
    /// 500 sem criar sessao).
    pub fn is_totp_enabled(&self, user_id: &str) -> Result<bool, String> {
        self.conn
            .query_row(
                "SELECT totp_enabled FROM admin_users WHERE id = ?1",
                params![user_id],
                |row| row.get::<_, i64>(0),
            )
            .map(|v| v != 0)
            .map_err(|e| format!("failed to read totp_enabled for user: {e}"))
    }

    /// `true` quando o usuario ja errou `TOTP_MAX_ATTEMPTS` codigos dentro da
    /// janela. Efeito colateral: descarta as tentativas que ja expiraram, para
    /// a contagem nao crescer sem limite.
    pub fn totp_attempts_exhausted(&mut self, user_id: &str) -> bool {
        let attempts = self.totp_attempts.entry(user_id.to_string()).or_default();
        let now = Instant::now();
        attempts.retain(|t| now.duration_since(*t) < TOTP_ATTEMPT_WINDOW);
        attempts.len() >= TOTP_MAX_ATTEMPTS
    }

    /// Conta uma tentativa. Sucesso zera a contagem — travar o dono que acabou
    /// de acertar o codigo seria pior que nao travar.
    pub fn record_totp_attempt(&mut self, user_id: &str, success: bool) {
        if success {
            self.totp_attempts.remove(user_id);
            return;
        }
        self.totp_attempts
            .entry(user_id.to_string())
            .or_default()
            .push(Instant::now());
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

    fn append_audit_on(
        conn: &rusqlite::Connection,
        user_id: Option<&str>,
        username: Option<&str>,
        action: &str,
        resource_type: &str,
        resource_id: Option<&str>,
        details: Option<&str>,
        ip_address: Option<&str>,
        outcome: &str,
    ) -> Result<(), String> {
        conn.execute(
            "INSERT INTO audit_log (user_id, username, action, resource_type, resource_id, details, ip_address, outcome)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![user_id, username, action, resource_type, resource_id, details, ip_address, outcome],
        )
        .map_err(|e| format!("failed to append audit log: {e}"))?;
        Ok(())
    }

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
        Self::append_audit_on(
            &self.conn,
            user_id,
            username,
            action,
            resource_type,
            resource_id,
            details,
            ip_address,
            outcome,
        )?;
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

    let iterations = NonZeroU32::new(PBKDF2_ITERATIONS).expect("iterations > 0");
    let mut hash = vec![0u8; 32];
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        iterations,
        &salt,
        password.as_bytes(),
        &mut hash,
    );

    Ok((BASE64.encode(&hash), BASE64.encode(salt)))
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

    // ── TOTP (#1121) ──────────────────────────────────────────────────

    fn usuario_totp(store: &AdminStore) -> String {
        store.create_user("cofre", "senha", Role::Admin).unwrap().id
    }

    #[test]
    fn segredo_pendente_nao_liga_o_segundo_fator() {
        let store = test_store();
        let id = usuario_totp(&store);

        store
            .set_pending_totp_secret(&id, "JBSWY3DPEHPK3PXP")
            .unwrap();

        assert_eq!(
            store.get_totp_secret(&id).as_deref(),
            Some("JBSWY3DPEHPK3PXP")
        );
        assert!(
            !store.is_totp_enabled(&id).expect("estado 2fa legivel"),
            "guardar o segredo nao pode exigir o codigo; so `enable_totp` liga"
        );
    }

    #[test]
    fn desligar_apaga_o_segredo_junto() {
        let store = test_store();
        let id = usuario_totp(&store);

        store
            .set_pending_totp_secret(&id, "JBSWY3DPEHPK3PXP")
            .unwrap();
        store.enable_totp(&id).unwrap();
        assert!(store.is_totp_enabled(&id).expect("estado 2fa legivel"));

        store.disable_totp(&id).unwrap();

        assert!(!store.is_totp_enabled(&id).expect("estado 2fa legivel"));
        assert_eq!(
            store.get_totp_secret(&id),
            None,
            "segredo sobrevivente seria uma reativacao sem novo enrollment"
        );
    }

    /// #1121 (regressao dos vereditos de seguranca): a mudanca de estado e o
    /// evento de auditoria confirmam na mesma transacao. Se a trilha nao pode
    /// ser gravada (tabela apagada, disco cheio, arquivo corrompido), a
    /// operacao inteira falha — desligar o segundo fator sem deixar rastro e
    /// exatamente o que um invasor com a sessao tentaria.
    #[test]
    fn desligar_sem_trilha_de_auditoria_nao_confirma() {
        let store = test_store();
        let id = usuario_totp(&store);
        store
            .set_pending_totp_secret(&id, "JBSWY3DPEHPK3PXP")
            .unwrap();
        store.enable_totp(&id).unwrap();

        store
            .conn
            .execute("DROP TABLE audit_log", [])
            .expect("derrubar audit_log");

        assert!(
            store
                .disable_totp_audited(&id, "cofre", Some("127.0.0.1"))
                .is_err(),
            "sem trilha gravada a operacao nao pode confirmar"
        );
        assert!(
            store.is_totp_enabled(&id).expect("estado 2fa legivel"),
            "a recusa nao pode deixar o 2FA desligado por baixo da transacao"
        );
    }

    #[test]
    fn usuario_novo_nasce_sem_segundo_fator() {
        let store = test_store();
        let id = usuario_totp(&store);

        assert!(
            !store
                .is_totp_enabled(&id)
                .expect("usuario novo tem estado legivel")
        );
        assert_eq!(store.get_totp_secret(&id), None);
    }

    #[test]
    fn errar_muito_trava_e_um_acerto_zera_a_contagem() {
        let mut store = test_store();
        let id = usuario_totp(&store);

        for _ in 0..TOTP_MAX_ATTEMPTS - 1 {
            store.record_totp_attempt(&id, false);
        }
        assert!(
            !store.totp_attempts_exhausted(&id),
            "travar antes da ultima tentativa valida"
        );

        store.record_totp_attempt(&id, false);
        assert!(store.totp_attempts_exhausted(&id));

        // Acertar nao pode deixar o dono travado.
        store.record_totp_attempt(&id, true);
        assert!(!store.totp_attempts_exhausted(&id));
    }

    #[test]
    fn a_contagem_e_por_usuario() {
        let mut store = test_store();
        let a = usuario_totp(&store);
        let b = store.create_user("outro", "senha", Role::Admin).unwrap().id;

        for _ in 0..TOTP_MAX_ATTEMPTS {
            store.record_totp_attempt(&a, false);
        }

        assert!(store.totp_attempts_exhausted(&a));
        assert!(
            !store.totp_attempts_exhausted(&b),
            "um usuario errando nao pode travar os outros"
        );
    }

    /// `admin_users` criada **antes** do #1121, sem as colunas de 2FA. E o
    /// banco de toda instalacao existente: `CREATE TABLE IF NOT EXISTS` nao
    /// toca nela, entao so o `ALTER TABLE` condicional salva o boot.
    fn banco_antigo_sem_2fa(path: &std::path::Path) {
        let conn = Connection::open(path).expect("db temporario");
        conn.execute_batch(
            "PRAGMA foreign_keys=ON;
             CREATE TABLE admin_users (
                 id TEXT PRIMARY KEY,
                 username TEXT UNIQUE NOT NULL,
                 password_hash TEXT NOT NULL,
                 password_salt TEXT NOT NULL,
                 role TEXT NOT NULL DEFAULT 'viewer',
                 created_at TEXT NOT NULL DEFAULT (datetime('now')),
                 updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                 last_login TEXT
             );",
        )
        .expect("schema antigo");
        conn.execute(
            "INSERT INTO admin_users (id, username, password_hash, password_salt, role)
             VALUES ('u1', 'dono', 'hash', 'salt', 'admin')",
            [],
        )
        .expect("usuario antigo");
    }

    #[test]
    fn instalacao_antiga_ganha_as_colunas_de_2fa_no_boot() {
        let path = std::env::temp_dir().join(format!("garra-2fa-migr-{}.db", uuid::Uuid::new_v4()));
        banco_antigo_sem_2fa(&path);

        let store = AdminStore::open(&path).expect("abrir banco antigo nao pode falhar");

        assert!(
            !store.is_totp_enabled("u1").expect("estado 2fa legivel"),
            "usuario antigo nasce com 2FA desligado"
        );
        store
            .set_pending_totp_secret("u1", "JBSWY3DPEHPK3PXP")
            .expect("escrever segredo");
        assert_eq!(
            store.get_totp_secret("u1").as_deref(),
            Some("JBSWY3DPEHPK3PXP")
        );

        let _ = std::fs::remove_file(&path);
    }

    /// O `PRAGMA table_info` e o `ALTER TABLE` nao sao atomicos: dois processos
    /// podem ler o schema antes de qualquer um escrever. O perdedor tem que
    /// receber sucesso, nao um `Err` que derruba o `open` para store em
    /// memoria — e dai o `/admin/api/setup` fica reivindicavel por anonimo.
    #[test]
    fn rodar_a_migracao_duas_vezes_e_sucesso() {
        let store = test_store();
        store.ensure_totp_columns().expect("segunda passada");
        store.ensure_totp_columns().expect("terceira passada");
    }
}
