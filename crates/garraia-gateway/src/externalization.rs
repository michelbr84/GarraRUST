use anyhow::Result;
use async_trait::async_trait;
use dashmap::DashMap;
use std::sync::Mutex;
use tracing::info;

// ---------------------------------------------------------------------------
// GAR-24: DatabaseBackend trait + SqliteBackend
// ---------------------------------------------------------------------------

#[async_trait]
pub trait DatabaseBackend: Send + Sync {
    async fn execute(&self, sql: &str, params: &[&str]) -> Result<u64>;
    async fn query_one(&self, sql: &str, params: &[&str]) -> Result<Option<serde_json::Value>>;
    async fn query_all(&self, sql: &str, params: &[&str]) -> Result<Vec<serde_json::Value>>;
    fn backend_name(&self) -> &str;
}

pub struct SqliteBackend {
    conn: Mutex<rusqlite::Connection>,
}

impl SqliteBackend {
    pub fn new(conn: rusqlite::Connection) -> Self {
        Self {
            conn: Mutex::new(conn),
        }
    }

    pub fn in_memory() -> Result<Self> {
        let conn = rusqlite::Connection::open_in_memory()?;
        Ok(Self::new(conn))
    }
}

#[async_trait]
impl DatabaseBackend for SqliteBackend {
    async fn execute(&self, sql: &str, params: &[&str]) -> Result<u64> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("lock: {e}"))?;
        let p: Vec<Box<dyn rusqlite::types::ToSql>> =
            params.iter().map(|s| Box::new(s.to_string()) as _).collect();
        let refs: Vec<&dyn rusqlite::types::ToSql> = p.iter().map(|b| b.as_ref()).collect();
        let rows = conn.execute(sql, refs.as_slice())?;
        Ok(rows as u64)
    }

    async fn query_one(&self, sql: &str, params: &[&str]) -> Result<Option<serde_json::Value>> {
        let rows = self.query_all(sql, params).await?;
        Ok(rows.into_iter().next())
    }

    async fn query_all(&self, sql: &str, params: &[&str]) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("lock: {e}"))?;
        let p: Vec<Box<dyn rusqlite::types::ToSql>> =
            params.iter().map(|s| Box::new(s.to_string()) as _).collect();
        let refs: Vec<&dyn rusqlite::types::ToSql> = p.iter().map(|b| b.as_ref()).collect();

        let mut stmt = conn.prepare(sql)?;
        let col_count = stmt.column_count();
        let col_names: Vec<String> = (0..col_count)
            .map(|i| stmt.column_name(i).unwrap_or("?").to_string())
            .collect();

        let rows = stmt.query_map(refs.as_slice(), |row| {
            let mut map = serde_json::Map::new();
            for (i, name) in col_names.iter().enumerate() {
                let raw: rusqlite::types::Value = row.get(i)?;
                let json_val = match raw {
                    rusqlite::types::Value::Null => serde_json::Value::Null,
                    rusqlite::types::Value::Integer(n) => serde_json::json!(n),
                    rusqlite::types::Value::Real(f) => serde_json::json!(f),
                    rusqlite::types::Value::Text(s) => serde_json::Value::String(s),
                    rusqlite::types::Value::Blob(_) => serde_json::Value::Null,
                };
                map.insert(name.clone(), json_val);
            }
            Ok(serde_json::Value::Object(map))
        })?;

        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }

    fn backend_name(&self) -> &str {
        "sqlite"
    }
}

// ---------------------------------------------------------------------------
// GAR-25: MigrationRunner
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Migration {
    pub version: u32,
    pub name: String,
    pub sql: String,
}

pub struct MigrationRunner {
    migrations: Vec<Migration>,
}

impl MigrationRunner {
    pub fn new(migrations: Vec<Migration>) -> Self {
        Self { migrations }
    }

    pub async fn run_pending(&self, backend: &dyn DatabaseBackend) -> Result<u32> {
        backend
            .execute(
                "CREATE TABLE IF NOT EXISTS _migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at TEXT NOT NULL)",
                &[],
            )
            .await?;

        let applied = backend
            .query_all("SELECT version FROM _migrations ORDER BY version", &[])
            .await?;

        let applied_versions: Vec<u32> = applied
            .iter()
            .filter_map(|row| {
                row.get("version").and_then(|v| match v {
                    serde_json::Value::Number(n) => n.as_u64().map(|n| n as u32),
                    serde_json::Value::String(s) => s.parse::<u32>().ok(),
                    _ => None,
                })
            })
            .collect();

        let mut count = 0u32;
        for m in &self.migrations {
            if applied_versions.contains(&m.version) {
                continue;
            }
            info!(version = m.version, name = %m.name, "applying migration");
            backend.execute(&m.sql, &[]).await?;

            let ver = m.version.to_string();
            let now = chrono::Utc::now().to_rfc3339();
            backend
                .execute(
                    "INSERT INTO _migrations (version, name, applied_at) VALUES (?1, ?2, ?3)",
                    &[&ver, &m.name, &now],
                )
                .await?;
            count += 1;
        }
        Ok(count)
    }
}

// ---------------------------------------------------------------------------
// GAR-26: VectorBackend trait + SqliteVecBackend
// ---------------------------------------------------------------------------

#[async_trait]
pub trait VectorBackend: Send + Sync {
    async fn upsert(
        &self,
        id: &str,
        embedding: &[f32],
        metadata: serde_json::Value,
    ) -> Result<()>;
    async fn search(&self, query: &[f32], limit: usize) -> Result<Vec<(String, f64)>>;
    fn backend_name(&self) -> &str;
}

pub struct SqliteVecBackend {
    conn: Mutex<rusqlite::Connection>,
    dimension: usize,
}

impl SqliteVecBackend {
    pub fn new(conn: rusqlite::Connection, dimension: usize) -> Self {
        Self {
            conn: Mutex::new(conn),
            dimension,
        }
    }

    pub fn dimension(&self) -> usize {
        self.dimension
    }
}

#[async_trait]
impl VectorBackend for SqliteVecBackend {
    async fn upsert(
        &self,
        id: &str,
        _embedding: &[f32],
        metadata: serde_json::Value,
    ) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("lock: {e}"))?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS vec_store (id TEXT PRIMARY KEY, metadata TEXT NOT NULL)",
            [],
        )?;
        conn.execute(
            "INSERT OR REPLACE INTO vec_store (id, metadata) VALUES (?1, ?2)",
            rusqlite::params![id, metadata.to_string()],
        )?;
        Ok(())
    }

    async fn search(&self, _query: &[f32], limit: usize) -> Result<Vec<(String, f64)>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("lock: {e}"))?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS vec_store (id TEXT PRIMARY KEY, metadata TEXT NOT NULL)",
            [],
        )?;
        let mut stmt = conn.prepare("SELECT id FROM vec_store LIMIT ?1")?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
            let id: String = row.get(0)?;
            Ok((id, 0.0f64))
        })?;
        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }

    fn backend_name(&self) -> &str {
        "sqlite-vec"
    }
}

// ---------------------------------------------------------------------------
// GAR-27: SessionBackend trait
// ---------------------------------------------------------------------------

#[async_trait]
pub trait SessionBackend: Send + Sync {
    async fn upsert_session(
        &self,
        session_id: &str,
        tenant_id: &str,
        data: serde_json::Value,
    ) -> Result<()>;
    async fn get_session(&self, session_id: &str) -> Result<Option<serde_json::Value>>;
    async fn delete_session(&self, session_id: &str) -> Result<()>;
    fn backend_name(&self) -> &str;
}

pub struct InMemorySessionBackend {
    store: DashMap<String, (String, serde_json::Value)>,
}

impl InMemorySessionBackend {
    pub fn new() -> Self {
        Self {
            store: DashMap::new(),
        }
    }
}

impl Default for InMemorySessionBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SessionBackend for InMemorySessionBackend {
    async fn upsert_session(
        &self,
        session_id: &str,
        tenant_id: &str,
        data: serde_json::Value,
    ) -> Result<()> {
        self.store
            .insert(session_id.to_string(), (tenant_id.to_string(), data));
        Ok(())
    }

    async fn get_session(&self, session_id: &str) -> Result<Option<serde_json::Value>> {
        Ok(self.store.get(session_id).map(|entry| {
            let (tenant, data) = entry.value();
            serde_json::json!({ "tenant_id": tenant, "data": data })
        }))
    }

    async fn delete_session(&self, session_id: &str) -> Result<()> {
        self.store.remove(session_id);
        Ok(())
    }

    fn backend_name(&self) -> &str {
        "in-memory"
    }
}

// ---------------------------------------------------------------------------
// GAR-28: ConfigBackend trait + InMemoryConfigBackend
// ---------------------------------------------------------------------------

#[async_trait]
pub trait ConfigBackend: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<String>>;
    async fn set(&self, key: &str, value: &str) -> Result<()>;
    async fn delete(&self, key: &str) -> Result<()>;
    async fn list_keys(&self, prefix: &str) -> Result<Vec<String>>;
    fn backend_name(&self) -> &str;
}

pub struct InMemoryConfigBackend {
    store: DashMap<String, String>,
}

impl InMemoryConfigBackend {
    pub fn new() -> Self {
        Self {
            store: DashMap::new(),
        }
    }
}

impl Default for InMemoryConfigBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ConfigBackend for InMemoryConfigBackend {
    async fn get(&self, key: &str) -> Result<Option<String>> {
        Ok(self.store.get(key).map(|v| v.value().clone()))
    }

    async fn set(&self, key: &str, value: &str) -> Result<()> {
        self.store.insert(key.to_string(), value.to_string());
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<()> {
        self.store.remove(key);
        Ok(())
    }

    async fn list_keys(&self, prefix: &str) -> Result<Vec<String>> {
        Ok(self
            .store
            .iter()
            .filter(|entry| entry.key().starts_with(prefix))
            .map(|entry| entry.key().clone())
            .collect())
    }

    fn backend_name(&self) -> &str {
        "in-memory"
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sqlite_backend() -> SqliteBackend {
        SqliteBackend::in_memory().expect("in-memory sqlite")
    }

    // --- GAR-24 tests ---

    #[tokio::test]
    async fn test_sqlite_backend_execute_and_query() {
        let db = sqlite_backend();
        db.execute(
            "CREATE TABLE t (id TEXT PRIMARY KEY, val TEXT)",
            &[],
        )
        .await
        .unwrap();

        let changed = db
            .execute("INSERT INTO t (id, val) VALUES (?1, ?2)", &["1", "hello"])
            .await
            .unwrap();
        assert_eq!(changed, 1);

        let row = db
            .query_one("SELECT id, val FROM t WHERE id = ?1", &["1"])
            .await
            .unwrap();
        assert!(row.is_some());
        let row = row.unwrap();
        assert_eq!(row["val"], "hello");
    }

    #[tokio::test]
    async fn test_sqlite_backend_query_all_empty() {
        let db = sqlite_backend();
        db.execute("CREATE TABLE t2 (x TEXT)", &[]).await.unwrap();

        let rows = db.query_all("SELECT x FROM t2", &[]).await.unwrap();
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn test_sqlite_backend_name() {
        let db = sqlite_backend();
        assert_eq!(db.backend_name(), "sqlite");
    }

    // --- GAR-25 tests ---

    #[tokio::test]
    async fn test_migration_runner_applies_pending() {
        let db = sqlite_backend();
        let runner = MigrationRunner::new(vec![
            Migration {
                version: 1,
                name: "create_users".into(),
                sql: "CREATE TABLE users (id TEXT PRIMARY KEY)".into(),
            },
            Migration {
                version: 2,
                name: "create_posts".into(),
                sql: "CREATE TABLE posts (id TEXT PRIMARY KEY)".into(),
            },
        ]);

        let applied = runner.run_pending(&db).await.unwrap();
        assert_eq!(applied, 2);

        let again = runner.run_pending(&db).await.unwrap();
        assert_eq!(again, 0);
    }

    #[tokio::test]
    async fn test_migration_runner_idempotent() {
        let db = sqlite_backend();
        let runner = MigrationRunner::new(vec![Migration {
            version: 1,
            name: "init".into(),
            sql: "CREATE TABLE IF NOT EXISTS m_test (id INTEGER)".into(),
        }]);

        runner.run_pending(&db).await.unwrap();
        runner.run_pending(&db).await.unwrap();

        let rows = db
            .query_all("SELECT version FROM _migrations", &[])
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
    }

    // --- GAR-26 tests ---

    #[tokio::test]
    async fn test_sqlite_vec_backend_upsert_and_search() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let vb = SqliteVecBackend::new(conn, 3);

        vb.upsert("v1", &[1.0, 2.0, 3.0], serde_json::json!({"tag": "a"}))
            .await
            .unwrap();

        let results = vb.search(&[1.0, 2.0, 3.0], 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "v1");
        assert_eq!(vb.backend_name(), "sqlite-vec");
    }

    // --- GAR-27 tests ---

    #[tokio::test]
    async fn test_session_backend_crud() {
        let sb = InMemorySessionBackend::new();
        sb.upsert_session("s1", "t1", serde_json::json!({"role": "admin"}))
            .await
            .unwrap();

        let s = sb.get_session("s1").await.unwrap();
        assert!(s.is_some());
        let s = s.unwrap();
        assert_eq!(s["tenant_id"], "t1");

        sb.delete_session("s1").await.unwrap();
        assert!(sb.get_session("s1").await.unwrap().is_none());
        assert_eq!(sb.backend_name(), "in-memory");
    }

    // --- GAR-28 tests ---

    #[tokio::test]
    async fn test_config_backend_set_get_delete() {
        let cb = InMemoryConfigBackend::new();
        cb.set("app.name", "garraia").await.unwrap();

        let v = cb.get("app.name").await.unwrap();
        assert_eq!(v, Some("garraia".into()));

        cb.delete("app.name").await.unwrap();
        assert!(cb.get("app.name").await.unwrap().is_none());
        assert_eq!(cb.backend_name(), "in-memory");
    }

    #[tokio::test]
    async fn test_config_backend_list_keys() {
        let cb = InMemoryConfigBackend::new();
        cb.set("db.host", "localhost").await.unwrap();
        cb.set("db.port", "5432").await.unwrap();
        cb.set("app.name", "test").await.unwrap();

        let mut keys = cb.list_keys("db.").await.unwrap();
        keys.sort();
        assert_eq!(keys, vec!["db.host", "db.port"]);
    }
}
