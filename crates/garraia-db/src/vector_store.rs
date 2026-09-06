use garraia_common::{Error, Result};
use rusqlite::{Connection, ffi::sqlite3_auto_extension, params};
use std::path::Path;
use std::sync::{Mutex, Once};
use tracing::{info, warn};

static SQLITE_VEC_INIT: Once = Once::new();
static mut SQLITE_VEC_LOADED: bool = false;

/// Register sqlite-vec as an auto-extension. This is process-global and only
/// needs to happen once. Safe to call multiple times (no-op after first).
fn ensure_sqlite_vec_registered() -> bool {
    SQLITE_VEC_INIT.call_once(|| unsafe {
        #[allow(clippy::missing_transmute_annotations)]
        let func = std::mem::transmute(sqlite_vec::sqlite3_vec_init as *const ());
        sqlite3_auto_extension(Some(func));
        SQLITE_VEC_LOADED = true;
        info!("sqlite-vec auto-extension registered");
    });
    unsafe { SQLITE_VEC_LOADED }
}

/// Vector database for semantic search and memory embeddings.
/// Uses sqlite-vec for KNN vector similarity operations with a fallback
/// to in-Rust cosine similarity if the extension cannot be loaded.
pub struct VectorStore {
    conn: Mutex<Connection>,
    vec_enabled: bool,
}

impl VectorStore {
    pub fn open(db_path: &Path) -> Result<Self> {
        info!("opening vector store at {}", db_path.display());
        let vec_enabled = ensure_sqlite_vec_registered();

        let conn = Connection::open(db_path)
            .map_err(|e| Error::Database(format!("failed to open vector database: {e}")))?;

        // Verify sqlite-vec is actually working
        let vec_enabled = if vec_enabled {
            verify_vec_extension(&conn)
        } else {
            false
        };

        let store = Self {
            conn: Mutex::new(conn),
            vec_enabled,
        };
        store.run_migrations()?;
        Ok(store)
    }

    pub fn in_memory() -> Result<Self> {
        let vec_enabled = ensure_sqlite_vec_registered();

        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Database(format!("failed to open in-memory vector db: {e}")))?;

        let vec_enabled = if vec_enabled {
            verify_vec_extension(&conn)
        } else {
            false
        };

        let store = Self {
            conn: Mutex::new(conn),
            vec_enabled,
        };
        store.run_migrations()?;
        Ok(store)
    }

    /// Whether the sqlite-vec extension is available.
    pub fn vec_enabled(&self) -> bool {
        self.vec_enabled
    }

    fn connection(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| Error::Database("vector store lock poisoned".into()))
    }

    fn run_migrations(&self) -> Result<()> {
        let conn = self.connection()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS embeddings (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                content TEXT NOT NULL,
                embedding BLOB,
                metadata TEXT DEFAULT '{}',
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            -- Mapping table: vec0 requires integer rowids but memory IDs are UUIDs.
            CREATE TABLE IF NOT EXISTS vec_id_map (
                rowid INTEGER PRIMARY KEY AUTOINCREMENT,
                entry_id TEXT NOT NULL UNIQUE
            );",
        )
        .map_err(|e| Error::Database(format!("vector store migration failed: {e}")))?;

        Ok(())
    }

    /// Create or verify that a `vec0` virtual table exists for the given dimensionality.
    /// This is a no-op if sqlite-vec is not loaded.
    pub fn ensure_vec_table(&self, dimensions: usize) -> Result<()> {
        if !self.vec_enabled {
            return Ok(());
        }

        let conn = self.connection()?;
        let table_name = format!("vec_embeddings_{dimensions}");

        // Check if the table already exists
        let exists: bool = conn
            .query_row(
                "SELECT count(*) > 0 FROM sqlite_master WHERE type='table' AND name=?",
                params![table_name],
                |row| row.get(0),
            )
            .map_err(|e| Error::Database(format!("failed to check vec table: {e}")))?;

        if !exists {
            let sql = format!(
                "CREATE VIRTUAL TABLE [{table_name}] USING vec0(embedding float[{dimensions}])"
            );
            conn.execute_batch(&sql)
                .map_err(|e| Error::Database(format!("failed to create vec table: {e}")))?;
            info!("created vec0 table: {table_name} ({dimensions} dims)");
        }

        Ok(())
    }

    /// Insert an embedding vector into the vec0 virtual table.
    /// Maps the string `id` to an integer rowid via `vec_id_map`.
    pub fn insert_embedding(&self, id: &str, embedding: &[f32], dimensions: usize) -> Result<()> {
        if !self.vec_enabled {
            return Ok(());
        }

        let mut conn = self.connection()?;
        let table_name = format!("vec_embeddings_{dimensions}");
        let blob = embedding_to_blob(embedding);

        // Transação: sem ela, um vetor recusado pelo vec0 (dimensão divergente,
        // #961) deixaria o mapeamento já gravado para trás — órfão instantâneo
        // no `vec_id_map`, exatamente o que o #960 cobra.
        let tx = conn
            .transaction()
            .map_err(|e| Error::Database(format!("failed to begin vec insert tx: {e}")))?;

        tx.execute(
            "INSERT OR IGNORE INTO vec_id_map (entry_id) VALUES (?)",
            params![id],
        )
        .map_err(|e| Error::Database(format!("failed to insert vec id mapping: {e}")))?;

        let rowid: i64 = tx
            .query_row(
                "SELECT rowid FROM vec_id_map WHERE entry_id = ?",
                params![id],
                |row| row.get(0),
            )
            .map_err(|e| Error::Database(format!("failed to get vec rowid: {e}")))?;

        tx.execute(
            &format!("INSERT OR REPLACE INTO [{table_name}] (rowid, embedding) VALUES (?, ?)"),
            params![rowid, blob],
        )
        .map_err(|e| Error::Database(format!("failed to insert vec embedding: {e}")))?;

        tx.commit()
            .map_err(|e| Error::Database(format!("failed to commit vec insert tx: {e}")))?;

        Ok(())
    }

    /// Remove os vetores e os mapeamentos das entradas dadas, em TODAS as
    /// tabelas `vec_embeddings_*` existentes (#960: deletar uma entrada não
    /// pode deixar vetor órfão; #954: a limpeza vale mesmo quando a dimensão
    /// da entrada não é a ativa). Devolve quantos mapeamentos foram removidos.
    ///
    /// Idempotente: ids sem mapeamento são ignorados.
    pub fn delete_embeddings(&self, ids: &[&str]) -> Result<usize> {
        if !self.vec_enabled || ids.is_empty() {
            return Ok(0);
        }

        let mut conn = self.connection()?;
        // Transação: as N tabelas dimensionais e o vec_id_map mudam juntos —
        // falhar no meio deixaria vetor sem mapeamento, invisível até para o
        // integrity_report (achado L3 da auditoria).
        let tx = conn
            .transaction()
            .map_err(|e| Error::Database(format!("failed to begin vec delete tx: {e}")))?;

        let placeholders: String = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let rowids: Vec<i64> = {
            let mut stmt = tx
                .prepare(&format!(
                    "SELECT rowid FROM vec_id_map WHERE entry_id IN ({placeholders})"
                ))
                .map_err(|e| Error::Database(format!("failed to prepare vec rowid lookup: {e}")))?;
            stmt.query_map(rusqlite::params_from_iter(ids.iter()), |row| row.get(0))
                .map_err(|e| Error::Database(format!("failed to look up vec rowids: {e}")))?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| Error::Database(format!("failed to collect vec rowids: {e}")))?
        };

        if rowids.is_empty() {
            return Ok(0);
        }

        let rowid_list = rowids
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(",");
        // Identificadores de tabela vêm do próprio sqlite_master (origem
        // fechada, padrão vec_embeddings_<n> criado por ensure_vec_table);
        // os valores (rowids) são inteiros nossos, não input externo.
        for table in vec_table_names(&tx)? {
            tx.execute(
                &format!("DELETE FROM [{table}] WHERE rowid IN ({rowid_list})"),
                [],
            )
            .map_err(|e| Error::Database(format!("failed to delete vec rows: {e}")))?;
        }

        let removed = tx
            .execute(
                &format!("DELETE FROM vec_id_map WHERE entry_id IN ({placeholders})"),
                rusqlite::params_from_iter(ids.iter()),
            )
            .map_err(|e| Error::Database(format!("failed to delete vec id mappings: {e}")))?;

        tx.commit()
            .map_err(|e| Error::Database(format!("failed to commit vec delete tx: {e}")))?;

        Ok(removed)
    }

    /// Inventário do índice para o relatório de integridade (#960):
    /// mapeamentos existentes e linhas por tabela `vec_embeddings_*`.
    pub fn index_inventory(&self) -> Result<VecIndexInventory> {
        let conn = self.connection()?;

        let map_rows: i64 = conn
            .query_row("SELECT count(*) FROM vec_id_map", [], |row| row.get(0))
            .map_err(|e| Error::Database(format!("failed to count vec_id_map: {e}")))?;

        let mut vec_rows_by_table = Vec::new();
        if self.vec_enabled {
            for table in vec_table_names(&conn)? {
                let count: i64 = conn
                    .query_row(&format!("SELECT count(*) FROM [{table}]"), [], |row| {
                        row.get(0)
                    })
                    .map_err(|e| Error::Database(format!("failed to count vec table: {e}")))?;
                vec_rows_by_table.push((table, count as usize));
            }
        }

        Ok(VecIndexInventory {
            map_rows: map_rows as usize,
            vec_rows_by_table,
        })
    }

    /// Ids presentes no `vec_id_map` cujo entry_id não está em `entry_ids`.
    /// Base do relatório de órfãos: o chamador (MemoryStore) passa os ids
    /// vivos de `memory_entries`.
    pub fn orphan_map_entries(&self, live_ids: &[&str]) -> Result<Vec<String>> {
        let conn = self.connection()?;
        let mut stmt = conn
            .prepare("SELECT entry_id FROM vec_id_map")
            .map_err(|e| Error::Database(format!("failed to prepare orphan scan: {e}")))?;
        let all: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| Error::Database(format!("failed to scan vec_id_map: {e}")))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Database(format!("failed to collect vec_id_map: {e}")))?;

        let live: std::collections::HashSet<&str> = live_ids.iter().copied().collect();
        Ok(all
            .into_iter()
            .filter(|id| !live.contains(id.as_str()))
            .collect())
    }

    /// KNN search: find the nearest `limit` embeddings to `query`.
    /// Returns `(entry_id, distance)` pairs ordered by distance ascending.
    pub fn search_nearest(
        &self,
        query: &[f32],
        dimensions: usize,
        limit: usize,
    ) -> Result<Vec<(String, f64)>> {
        if !self.vec_enabled {
            return Ok(Vec::new());
        }

        let conn = self.connection()?;
        let table_name = format!("vec_embeddings_{dimensions}");
        let blob = embedding_to_blob(query);

        let mut stmt = conn
            .prepare(&format!(
                "SELECT m.entry_id, v.distance
                 FROM [{table_name}] v
                 JOIN vec_id_map m ON m.rowid = v.rowid
                 WHERE v.embedding MATCH ? AND k = ?"
            ))
            .map_err(|e| Error::Database(format!("failed to prepare KNN query: {e}")))?;

        let rows = stmt
            .query_map(params![blob, limit as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })
            .map_err(|e| Error::Database(format!("KNN query failed: {e}")))?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Database(format!("failed to collect KNN results: {e}")))
    }
}

/// Contagem de linhas do índice vetorial, para o relatório de integridade.
#[derive(Debug, Clone)]
pub struct VecIndexInventory {
    pub map_rows: usize,
    /// `(nome da tabela, linhas)` para cada `vec_embeddings_*` existente.
    pub vec_rows_by_table: Vec<(String, usize)>,
}

/// Tabelas `vec_embeddings_*` existentes, direto do sqlite_master.
///
/// O filtro por `CREATE VIRTUAL TABLE` é obrigatório: o vec0 cria shadow
/// tables (`…_info`, `…_chunks`, `…_rowids`, …) que também casam com o LIKE
/// mas são internas — deletar/contar nelas corromperia ou infl(acion)aria o
/// índice.
fn vec_table_names(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn
        .prepare(
            "SELECT name FROM sqlite_master
             WHERE type='table' AND name LIKE 'vec_embeddings_%'
               AND sql LIKE 'CREATE VIRTUAL TABLE%'",
        )
        .map_err(|e| Error::Database(format!("failed to list vec tables: {e}")))?;
    let names = stmt
        .query_map([], |row| row.get(0))
        .map_err(|e| Error::Database(format!("failed to scan vec tables: {e}")))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| Error::Database(format!("failed to collect vec tables: {e}")))?;
    Ok(names)
}

/// Verify that sqlite-vec functions are available on this connection.
fn verify_vec_extension(conn: &Connection) -> bool {
    match conn.query_row("SELECT vec_version()", [], |row| row.get::<_, String>(0)) {
        Ok(version) => {
            info!("sqlite-vec {version} available");
            true
        }
        Err(e) => {
            warn!("sqlite-vec not functional: {e} (falling back to in-Rust cosine similarity)");
            false
        }
    }
}

fn embedding_to_blob(embedding: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(embedding.len() * 4);
    for v in embedding {
        bytes.extend(v.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_memory_creates_embeddings_table() {
        let store = VectorStore::in_memory().expect("should open in-memory vector store");
        let conn = store.connection().expect("lock not poisoned");
        let exists: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='embeddings'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(exists, 1);
    }

    #[test]
    fn vec_table_lifecycle() {
        let store = VectorStore::in_memory().expect("should open in-memory vector store");
        if !store.vec_enabled() {
            eprintln!("sqlite-vec not available, skipping vec table test");
            return;
        }

        store.ensure_vec_table(3).unwrap();

        // Insert
        store.insert_embedding("id-1", &[1.0, 0.0, 0.0], 3).unwrap();
        store.insert_embedding("id-2", &[0.0, 1.0, 0.0], 3).unwrap();

        // Search
        let results = store.search_nearest(&[0.9, 0.1, 0.0], 3, 2).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, "id-1"); // closest
    }

    /// #960: a deleção varre TODAS as tabelas dimensionais — uma entrada pode
    /// ter sido indexada numa dimensão que não é mais a ativa (#954).
    #[test]
    fn delete_embeddings_removes_from_all_tables_and_the_map() {
        let store = VectorStore::in_memory().expect("should open in-memory vector store");
        if !store.vec_enabled() {
            eprintln!("sqlite-vec not available, skipping delete test");
            return;
        }

        store.ensure_vec_table(2).unwrap();
        store.ensure_vec_table(3).unwrap();
        store.insert_embedding("dim2", &[1.0, 0.0], 2).unwrap();
        store.insert_embedding("dim3", &[1.0, 0.0, 0.0], 3).unwrap();

        let removed = store
            .delete_embeddings(&["dim2", "dim3", "inexistente"])
            .unwrap();
        assert_eq!(removed, 2, "ids sem mapeamento são ignorados");

        let inventory = store.index_inventory().unwrap();
        assert_eq!(inventory.map_rows, 0);
        assert!(
            inventory.vec_rows_by_table.iter().all(|(_, n)| *n == 0),
            "sobrou vetor: {:?}",
            inventory.vec_rows_by_table
        );

        // Idempotente: repetir não erra nem remove nada.
        assert_eq!(store.delete_embeddings(&["dim2"]).unwrap(), 0);
    }

    /// #960 item 2: vetor com dimensão diferente da tabela não entra — e a
    /// recusa não pode deixar rastro. É a rede que segura o #961 (provider
    /// que troca de modelo e devolve outra dimensão) no nível do índice.
    #[test]
    fn wrong_dimension_rejected() {
        let store = VectorStore::in_memory().expect("should open in-memory vector store");
        assert!(
            store.vec_enabled(),
            "o sqlite-vec e compilado no binario (rusqlite bundled): sem ele \
             este teste passaria em vazio"
        );

        store.ensure_vec_table(3).unwrap();
        store
            .insert_embedding("certo", &[1.0, 0.0, 0.0], 3)
            .unwrap();

        let recusado = store.insert_embedding("torto", &[1.0, 0.0], 3);
        assert!(
            recusado.is_err(),
            "vetor de 2 dimensoes nao pode entrar numa tabela float[3]: {recusado:?}"
        );

        // E o indice segue integro: nem vetor meio-gravado, nem mapeamento orfao.
        let inventory = store.index_inventory().unwrap();
        assert_eq!(
            inventory.map_rows, 1,
            "a recusa nao pode deixar mapeamento para tras: {inventory:?}"
        );
        assert!(
            store.orphan_map_entries(&["certo"]).unwrap().is_empty(),
            "nenhum mapeamento sobra alem do insert valido"
        );
    }
}
