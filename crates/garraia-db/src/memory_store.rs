use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, Utc};
use garraia_common::{Error, Result};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use tracing::info;
use uuid::Uuid;

use crate::VectorStore;
use crate::migrations::MEMORY_SCHEMA_V1;

const DEFAULT_RECALL_LIMIT: usize = 20;
const MAX_RECALL_LIMIT: usize = 200;

/// Persisted memory entry used for retrieval and context assembly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub tenant_id: String,
    pub session_id: String,
    pub channel_id: Option<String>,
    pub user_id: Option<String>,
    /// Logical continuity bucket for shared memory across channels/sessions.
    pub continuity_key: Option<String>,
    pub role: MemoryRole,
    pub content: String,
    pub embedding: Option<Vec<f32>>,
    pub embedding_model: Option<String>,
    pub embedding_dimensions: Option<usize>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

/// Insert shape for new memory records before persistence assigns ID/timestamps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewMemoryEntry {
    pub tenant_id: String,
    pub session_id: String,
    pub channel_id: Option<String>,
    pub user_id: Option<String>,
    pub continuity_key: Option<String>,
    pub role: MemoryRole,
    pub content: String,
    pub embedding: Option<Vec<f32>>,
    pub embedding_model: Option<String>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryRole {
    User,
    Assistant,
    System,
    Tool,
}

impl MemoryRole {
    fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::System => "system",
            Self::Tool => "tool",
        }
    }

    fn from_db(value: &str) -> Result<Self> {
        match value {
            "user" => Ok(Self::User),
            "assistant" => Ok(Self::Assistant),
            "system" => Ok(Self::System),
            "tool" => Ok(Self::Tool),
            other => Err(Error::Database(format!("unknown memory role: {other}"))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallQuery {
    pub tenant_id: Option<String>,
    pub query_text: Option<String>,
    pub query_embedding: Option<Vec<f32>>,
    /// Modelo que produziu `query_embedding` (#954). Quando presente, o
    /// recall só compara vetores gravados por esse modelo — distâncias entre
    /// espaços vetoriais diferentes não são comparáveis, mesmo com a mesma
    /// dimensão. `None` = sem filtro (chamadas só-texto ou legadas).
    #[serde(default)]
    pub embedding_model: Option<String>,
    pub session_id: Option<String>,
    pub continuity_key: Option<String>,
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionContext {
    pub session_id: String,
    pub entries: Vec<MemoryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionReport {
    pub deleted_entries: usize,
    pub before: DateTime<Utc>,
}

/// Consistência entre `memory_entries` e o índice vetorial (#960).
///
/// Íntegro quando: `map_rows == entries_with_embedding` (com KNN ligado),
/// cada tabela `vec_embeddings_*` soma o mesmo total e `orphan_map_entries`
/// está vazio. `entries_without_embedding > 0` não é corrupção — é a fila de
/// reindexação (#953).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrityReport {
    pub entries_total: usize,
    pub entries_with_embedding: usize,
    pub entries_without_embedding: usize,
    /// Linhas em `vec_id_map`.
    pub map_rows: usize,
    /// `(tabela, linhas)` por tabela `vec_embeddings_*` existente.
    pub vec_rows_by_table: Vec<(String, usize)>,
    /// Ids mapeados no índice sem entrada correspondente em `memory_entries`.
    pub orphan_map_entries: Vec<String>,
}

#[async_trait]
pub trait MemoryProvider: Send + Sync {
    async fn remember(&self, entry: NewMemoryEntry) -> Result<String>;
    async fn recall(&self, query: RecallQuery) -> Result<Vec<MemoryEntry>>;
    async fn get_session_context(&self, session_id: &str, limit: usize) -> Result<SessionContext>;
    async fn get_continuity_context(
        &self,
        continuity_key: &str,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>>;
    async fn compact(&self, before: DateTime<Utc>) -> Result<CompactionReport>;
    async fn delete_session_memory(&self, session_id: &str) -> Result<usize>;
}

/// Backing store for long-term and session-scoped memory data.
pub struct MemoryStore {
    conn: Mutex<Connection>,
    /// Optional vector store for KNN search via sqlite-vec.
    vector_store: Option<VectorStore>,
}

impl MemoryStore {
    pub fn open(db_path: &Path) -> Result<Self> {
        info!("opening memory store at {}", db_path.display());
        let conn = Connection::open(db_path)
            .map_err(|e| Error::Database(format!("failed to open memory database: {e}")))?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| Error::Database(format!("failed to set pragmas: {e}")))?;

        // Try to initialize the vector store (same DB path) for KNN search
        let vector_store = match VectorStore::open(db_path) {
            Ok(vs) if vs.vec_enabled() => {
                info!("vector search enabled via sqlite-vec");
                Some(vs)
            }
            Ok(_) => None,
            Err(e) => {
                tracing::warn!("vector store init failed (continuing without KNN): {e}");
                None
            }
        };

        let store = Self {
            conn: Mutex::new(conn),
            vector_store,
        };
        store.run_migrations()?;
        Ok(store)
    }

    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Database(format!("failed to open in-memory database: {e}")))?;

        let store = Self {
            conn: Mutex::new(conn),
            vector_store: None,
        };
        store.run_migrations()?;
        Ok(store)
    }

    /// Como [`Self::in_memory`], mas com o índice vetorial ligado (quando o
    /// sqlite-vec estiver disponível). O vec store vive numa conexão própria
    /// mesmo em produção — aqui ele só é um segundo banco em memória.
    /// Existe para os testes de integridade do índice (#960) e para
    /// ferramentas que precisem exercitar o caminho KNN sem arquivo.
    pub fn in_memory_with_vectors() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Database(format!("failed to open in-memory database: {e}")))?;

        let vector_store = match VectorStore::in_memory() {
            Ok(vs) if vs.vec_enabled() => Some(vs),
            _ => None,
        };

        let store = Self {
            conn: Mutex::new(conn),
            vector_store,
        };
        store.run_migrations()?;
        Ok(store)
    }

    /// O caminho KNN está ativo? (sqlite-vec carregado e verificado.)
    pub fn knn_enabled(&self) -> bool {
        self.vector_store
            .as_ref()
            .is_some_and(|vs| vs.vec_enabled())
    }

    /// Relatório de consistência entre `memory_entries`, `vec_id_map` e as
    /// tabelas `vec_embeddings_*` (#960). Não conserta nada — conta e nomeia
    /// divergências para o operador (e para a CLI `garra memory stats`).
    pub fn integrity_report(&self) -> Result<IntegrityReport> {
        let (entries_total, entries_with_embedding, live_ids) = {
            let conn = self.connection()?;
            let total: i64 = conn
                .query_row("SELECT count(*) FROM memory_entries", [], |row| row.get(0))
                .map_err(|e| Error::Database(format!("failed to count entries: {e}")))?;
            let with_embedding: i64 = conn
                .query_row(
                    "SELECT count(*) FROM memory_entries WHERE embedding IS NOT NULL",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| Error::Database(format!("failed to count embedded entries: {e}")))?;
            let mut stmt = conn
                .prepare("SELECT id FROM memory_entries")
                .map_err(|e| Error::Database(format!("failed to prepare id scan: {e}")))?;
            let ids: Vec<String> = stmt
                .query_map([], |row| row.get(0))
                .map_err(|e| Error::Database(format!("failed to scan entry ids: {e}")))?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| Error::Database(format!("failed to collect entry ids: {e}")))?;
            (total as usize, with_embedding as usize, ids)
        };

        let (map_rows, vec_rows_by_table, orphan_map_entries) = match &self.vector_store {
            Some(vs) => {
                let inventory = vs.index_inventory()?;
                let live_refs: Vec<&str> = live_ids.iter().map(String::as_str).collect();
                let orphans = vs.orphan_map_entries(&live_refs)?;
                (inventory.map_rows, inventory.vec_rows_by_table, orphans)
            }
            None => (0, Vec::new(), Vec::new()),
        };

        Ok(IntegrityReport {
            entries_total,
            entries_with_embedding,
            entries_without_embedding: entries_total - entries_with_embedding,
            map_rows,
            vec_rows_by_table,
            orphan_map_entries,
        })
    }

    fn run_migrations(&self) -> Result<()> {
        let conn = self.connection()?;

        // Migration: add tenant_id column to pre-existing memory_entries tables.
        // Ignore error if the table doesn't exist yet or the column already exists.
        let _ = conn.execute_batch(
            "ALTER TABLE memory_entries ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'default';",
        );

        conn.execute_batch(MEMORY_SCHEMA_V1.sql)
            .map_err(|e| Error::Database(format!("memory migration failed: {e}")))?;

        Ok(())
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| Error::Database("memory database lock poisoned".into()))
    }

    pub async fn remember(&self, entry: NewMemoryEntry) -> Result<String> {
        self.remember_sync(entry)
    }

    pub async fn recall(&self, query: RecallQuery) -> Result<Vec<MemoryEntry>> {
        self.recall_sync(query)
    }

    pub async fn get_session_context(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<SessionContext> {
        let entries = self.recent_entries_sync(Some(session_id), None, limit)?;
        Ok(SessionContext {
            session_id: session_id.to_string(),
            entries,
        })
    }

    pub async fn get_continuity_context(
        &self,
        continuity_key: &str,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        self.recent_entries_sync(None, Some(continuity_key), limit)
    }

    pub async fn compact(&self, before: DateTime<Utc>) -> Result<CompactionReport> {
        // Coletar os ids ANTES de deletar: são eles que limpam o índice
        // vetorial (#960 — deletar entrada não pode deixar vetor órfão).
        let doomed = {
            let conn = self.connection()?;
            let mut stmt = conn
                .prepare("SELECT id FROM memory_entries WHERE datetime(created_at) < datetime(?)")
                .map_err(|e| Error::Database(format!("failed to prepare compact scan: {e}")))?;
            let ids: Vec<String> = stmt
                .query_map(params![before.to_rfc3339()], |row| row.get(0))
                .map_err(|e| Error::Database(format!("failed to scan compact targets: {e}")))?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| Error::Database(format!("failed to collect compact targets: {e}")))?;
            ids
        };

        let deleted = {
            let conn = self.connection()?;
            conn.execute(
                "DELETE FROM memory_entries WHERE datetime(created_at) < datetime(?)",
                params![before.to_rfc3339()],
            )
            .map_err(|e| Error::Database(format!("failed to compact memory entries: {e}")))?
        };

        self.cleanup_vectors(&doomed);

        Ok(CompactionReport {
            deleted_entries: deleted,
            before,
        })
    }

    pub async fn delete_session_memory(&self, session_id: &str) -> Result<usize> {
        let doomed = {
            let conn = self.connection()?;
            let mut stmt = conn
                .prepare("SELECT id FROM memory_entries WHERE session_id = ?")
                .map_err(|e| Error::Database(format!("failed to prepare delete scan: {e}")))?;
            let ids: Vec<String> = stmt
                .query_map(params![session_id], |row| row.get(0))
                .map_err(|e| Error::Database(format!("failed to scan delete targets: {e}")))?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| Error::Database(format!("failed to collect delete targets: {e}")))?;
            ids
        };

        let deleted = {
            let conn = self.connection()?;
            conn.execute(
                "DELETE FROM memory_entries WHERE session_id = ?",
                params![session_id],
            )
            .map_err(|e| Error::Database(format!("failed to delete session memory: {e}")))?
        };

        self.cleanup_vectors(&doomed);

        Ok(deleted)
    }

    /// Best-effort: remove do índice vetorial os vetores das entradas dadas.
    /// Falha vira warn — a entrada já saiu do `memory_entries`, e um vetor
    /// órfão remanescente é inofensivo para o recall (o fetch escopado não o
    /// devolve) e detectável pelo `integrity_report`.
    fn cleanup_vectors(&self, ids: &[String]) {
        let Some(vs) = &self.vector_store else {
            return;
        };
        if ids.is_empty() {
            return;
        }
        let id_refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        if let Err(e) = vs.delete_embeddings(&id_refs) {
            tracing::warn!("failed to clean up vectors for deleted entries: {e}");
        }
    }

    fn remember_sync(&self, entry: NewMemoryEntry) -> Result<String> {
        if entry.content.trim().is_empty() {
            return Err(Error::Database("memory content cannot be empty".into()));
        }

        let id = Uuid::new_v4().to_string();
        let created_at = Utc::now().to_rfc3339();
        let embedding_blob = entry.embedding.as_ref().map(|e| embedding_to_blob(e));
        let embedding_dimensions = entry.embedding.as_ref().map(|e| e.len() as i64);
        let metadata_json = serde_json::to_string(&entry.metadata)
            .map_err(|e| Error::Database(format!("failed to serialize memory metadata: {e}")))?;

        let conn = self.connection()?;
        conn.execute(
            "INSERT INTO memory_entries (
                id, tenant_id, session_id, channel_id, user_id, continuity_key, role, content,
                embedding, embedding_model, embedding_dimensions, metadata, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                id,
                entry.tenant_id,
                entry.session_id,
                entry.channel_id,
                entry.user_id,
                entry.continuity_key,
                entry.role.as_str(),
                entry.content,
                embedding_blob,
                entry.embedding_model,
                embedding_dimensions,
                metadata_json,
                created_at,
            ],
        )
        .map_err(|e| Error::Database(format!("failed to insert memory entry: {e}")))?;

        // Also insert into the sqlite-vec virtual table for KNN search
        if let (Some(vs), Some(emb)) = (&self.vector_store, &entry.embedding) {
            let dims = emb.len();
            if let Err(e) = vs.ensure_vec_table(dims) {
                tracing::warn!("failed to ensure vec table: {e}");
            } else if let Err(e) = vs.insert_embedding(&id, emb, dims) {
                tracing::warn!("failed to insert vec embedding: {e}");
            }
        }

        Ok(id)
    }

    fn recall_sync(&self, mut query: RecallQuery) -> Result<Vec<MemoryEntry>> {
        if query.tenant_id.is_none() {
            query.tenant_id = Some("default".to_string());
        }

        let limit = clamp_limit(query.limit);

        // If we have a query embedding and sqlite-vec is available, use KNN for
        // initial candidate retrieval instead of loading all rows.
        //
        // O fetch dos candidatos reaplica TODOS os filtros da query. O índice
        // vec0 não conhece tenant/sessão/modelo — só distância —, então sem o
        // reescopo o caminho KNN devolvia linhas de qualquer tenant (bypass de
        // isolamento) e de qualquer modelo (#954), enquanto o caminho SQL
        // abaixo sempre filtrou.
        if let (Some(vs), Some(qe)) = (&self.vector_store, &query.query_embedding)
            && vs.vec_enabled()
        {
            let dims = qe.len();
            if let Ok(knn_results) = vs.search_nearest(qe, dims, limit.saturating_mul(4))
                && !knn_results.is_empty()
            {
                let candidate_ids: Vec<&str> =
                    knn_results.iter().map(|(id, _)| id.as_str()).collect();
                let candidates = self.fetch_entries_by_ids_scoped(&candidate_ids, &query)?;

                if !candidates.is_empty() {
                    return self.score_and_rank(candidates, &query, limit);
                }

                // O KNN achou vizinhos, mas nenhum sobreviveu ao escopo. Com
                // filtro de modelo ativo isso é o sintoma clássico de troca de
                // modelo sem reindexar (#954): o índice inteiro pertence ao
                // modelo antigo. Avisar alto — o operador não tem outro sinal.
                if let Some(model) = query.embedding_model.as_deref() {
                    tracing::warn!(
                        model,
                        knn_candidates = knn_results.len(),
                        "recall: nenhum vizinho KNN pertence ao modelo ativo/escopo — \
                         provável troca de modelo de embeddings sem reindexação"
                    );
                }
            }
            // Fall through to SQL-based retrieval if KNN returned nothing
        }

        let candidates = self.query_candidates_with_tenant_sync(
            query.tenant_id.as_deref(),
            query.session_id.as_deref(),
            query.continuity_key.as_deref(),
            query.query_text.as_deref(),
            limit.saturating_mul(4),
        )?;

        if candidates.is_empty() {
            return Ok(Vec::new());
        }

        self.score_and_rank(candidates, &query, limit)
    }

    /// Score candidates by semantic/text/recency and return the top `limit`.
    fn score_and_rank(
        &self,
        candidates: Vec<MemoryEntry>,
        query: &RecallQuery,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        let mut scored: Vec<(f32, MemoryEntry)> = candidates
            .into_iter()
            .map(|entry| {
                // Cosseno entre modelos diferentes não significa nada (#954):
                // com filtro de modelo na query, entrada de outro modelo pontua
                // 0.0 no eixo semântico e compete só por texto/recência.
                let same_model = match (&query.embedding_model, &entry.embedding_model) {
                    (Some(wanted), Some(got)) => wanted == got,
                    (Some(_), None) => false,
                    (None, _) => true,
                };
                let semantic_score = match (&query.query_embedding, &entry.embedding) {
                    (Some(needle), Some(candidate)) if same_model => {
                        cosine_similarity(needle, candidate)
                    }
                    _ => 0.0,
                };
                let text_score = text_match_score(query.query_text.as_deref(), &entry.content);
                let recency_score = recency_score(entry.created_at);

                let score = if query.query_embedding.is_some() {
                    semantic_score * 0.7 + text_score * 0.2 + recency_score * 0.1
                } else {
                    text_score * 0.7 + recency_score * 0.3
                };

                (score, entry)
            })
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal));

        Ok(scored
            .into_iter()
            .take(limit)
            .map(|(_, entry)| entry)
            .collect())
    }

    /// Fetch memory entries by their IDs, reaplicando os filtros da query.
    ///
    /// O vec0 devolve vizinhos por distância sem saber de tenant, sessão,
    /// continuidade ou modelo — este é o ponto onde o escopo volta a valer.
    /// Sem os quatro filtros, o caminho KNN do recall vazava entradas de
    /// outros tenants e misturava modelos de embedding (#954).
    fn fetch_entries_by_ids_scoped(
        &self,
        ids: &[&str],
        query: &RecallQuery,
    ) -> Result<Vec<MemoryEntry>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let conn = self.connection()?;
        let placeholders: String = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, tenant_id, session_id, channel_id, user_id, continuity_key, role, content,
                    embedding, embedding_model, embedding_dimensions, metadata, created_at
             FROM memory_entries
             WHERE id IN ({placeholders})
               AND (? IS NULL OR tenant_id = ?)
               AND (? IS NULL OR session_id = ?)
               AND (? IS NULL OR continuity_key = ?)
               AND (? IS NULL OR embedding_model = ?)"
        );

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| Error::Database(format!("failed to prepare fetch by ids: {e}")))?;

        let mut values: Vec<rusqlite::types::Value> = ids
            .iter()
            .map(|id| rusqlite::types::Value::from(id.to_string()))
            .collect();
        for filter in [
            &query.tenant_id,
            &query.session_id,
            &query.continuity_key,
            &query.embedding_model,
        ] {
            // Cada filtro entra duas vezes: no teste de NULL e na comparação.
            values.push(rusqlite::types::Value::from(filter.clone()));
            values.push(rusqlite::types::Value::from(filter.clone()));
        }

        let rows = stmt
            .query_map(rusqlite::params_from_iter(values), row_to_entry)
            .map_err(|e| Error::Database(format!("failed to fetch entries by ids: {e}")))?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Database(format!("failed to collect entries: {e}")))
    }

    fn recent_entries_sync(
        &self,
        session_id: Option<&str>,
        continuity_key: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        self.query_candidates_sync(session_id, continuity_key, None, limit)
    }

    fn query_candidates_sync(
        &self,
        session_id: Option<&str>,
        continuity_key: Option<&str>,
        query_text: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        self.query_candidates_with_tenant_sync(
            Some("default"),
            session_id,
            continuity_key,
            query_text,
            limit,
        )
    }

    fn query_candidates_with_tenant_sync(
        &self,
        tenant_id: Option<&str>,
        session_id: Option<&str>,
        continuity_key: Option<&str>,
        query_text: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        let query_limit = clamp_limit(limit).min(MAX_RECALL_LIMIT) as i64;
        let conn = self.connection()?;

        let mut stmt = conn
            .prepare(
                "SELECT id, tenant_id, session_id, channel_id, user_id, continuity_key, role, content,
                        embedding, embedding_model, embedding_dimensions, metadata, created_at
                 FROM memory_entries
                 WHERE (?1 IS NULL OR tenant_id = ?1)
                   AND (?2 IS NULL OR session_id = ?2)
                   AND (?3 IS NULL OR continuity_key = ?3)
                   AND (?4 IS NULL OR lower(content) LIKE '%' || lower(?4) || '%')
                 ORDER BY datetime(created_at) DESC
                 LIMIT ?5",
            )
            .map_err(|e| Error::Database(format!("failed to prepare recall query: {e}")))?;

        let rows = stmt
            .query_map(
                params![
                    tenant_id,
                    session_id,
                    continuity_key,
                    query_text,
                    query_limit
                ],
                row_to_entry,
            )
            .map_err(|e| Error::Database(format!("failed to execute recall query: {e}")))?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Database(format!("failed to collect recall rows: {e}")))?
            .pipe(Ok)
    }
}

#[async_trait]
impl MemoryProvider for MemoryStore {
    async fn remember(&self, entry: NewMemoryEntry) -> Result<String> {
        self.remember(entry).await
    }

    async fn recall(&self, query: RecallQuery) -> Result<Vec<MemoryEntry>> {
        self.recall(query).await
    }

    async fn get_session_context(&self, session_id: &str, limit: usize) -> Result<SessionContext> {
        self.get_session_context(session_id, limit).await
    }

    async fn get_continuity_context(
        &self,
        continuity_key: &str,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        self.get_continuity_context(continuity_key, limit).await
    }

    async fn compact(&self, before: DateTime<Utc>) -> Result<CompactionReport> {
        self.compact(before).await
    }

    async fn delete_session_memory(&self, session_id: &str) -> Result<usize> {
        self.delete_session_memory(session_id).await
    }
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryEntry> {
    let role_str: String = row.get(6)?;
    let role = MemoryRole::from_db(&role_str).map_err(|e| {
        rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e.to_string())))
    })?;

    let embedding_blob: Option<Vec<u8>> = row.get(8)?;
    let embedding = embedding_blob
        .as_deref()
        .map(blob_to_embedding)
        .transpose()
        .map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e.to_string())))
        })?;

    let metadata_str: String = row.get(11)?;
    let metadata = serde_json::from_str(&metadata_str).unwrap_or(serde_json::Value::Null);

    let created_at_str: String = row.get(12)?;
    let created_at = parse_timestamp(&created_at_str).map_err(|e| {
        rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e.to_string())))
    })?;

    Ok(MemoryEntry {
        id: row.get(0)?,
        tenant_id: row.get(1)?,
        session_id: row.get(2)?,
        channel_id: row.get(3)?,
        user_id: row.get(4)?,
        continuity_key: row.get(5)?,
        role,
        content: row.get(7)?,
        embedding,
        embedding_model: row.get(9)?,
        embedding_dimensions: row.get::<_, Option<i64>>(10)?.map(|d| d as usize),
        metadata,
        created_at,
    })
}

fn clamp_limit(limit: usize) -> usize {
    if limit == 0 {
        DEFAULT_RECALL_LIMIT
    } else {
        limit.min(MAX_RECALL_LIMIT)
    }
}

fn parse_timestamp(raw: &str) -> Result<DateTime<Utc>> {
    if let Ok(ts) = DateTime::parse_from_rfc3339(raw) {
        return Ok(ts.with_timezone(&Utc));
    }

    if let Ok(naive) = NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S") {
        return Ok(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc));
    }

    Err(Error::Database(format!("invalid timestamp format: {raw}")))
}

fn embedding_to_blob(embedding: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(embedding.len() * 4);
    for v in embedding {
        bytes.extend(v.to_le_bytes());
    }
    bytes
}

fn blob_to_embedding(blob: &[u8]) -> Result<Vec<f32>> {
    if !blob.len().is_multiple_of(4) {
        return Err(Error::Database("invalid embedding blob length".into()));
    }

    let mut out = Vec::with_capacity(blob.len() / 4);
    for chunk in blob.as_chunks::<4>().0 {
        out.push(f32::from_le_bytes(*chunk));
    }
    Ok(out)
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0;
    let mut norm_a = 0.0;
    let mut norm_b = 0.0;

    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    dot / (norm_a.sqrt() * norm_b.sqrt())
}

fn text_match_score(query_text: Option<&str>, content: &str) -> f32 {
    let Some(query) = query_text else {
        return 0.0;
    };
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return 0.0;
    }

    let content_lc = content.to_lowercase();
    if content_lc.contains(&query) {
        return 1.0;
    }

    let terms: Vec<&str> = query.split_whitespace().filter(|s| !s.is_empty()).collect();
    if terms.is_empty() {
        return 0.0;
    }

    let matches = terms
        .iter()
        .filter(|term| content_lc.contains(**term))
        .count();
    matches as f32 / terms.len() as f32
}

fn recency_score(created_at: DateTime<Utc>) -> f32 {
    let age_secs = (Utc::now() - created_at).num_seconds().max(0) as f32;
    1.0 / (1.0 + age_secs / 3600.0)
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}
impl<T> Pipe for T {}

#[cfg(test)]
mod tests {
    use super::{MemoryRole, MemoryStore, NewMemoryEntry, RecallQuery};
    use chrono::{Duration, Utc};

    fn entry(
        session_id: &str,
        continuity_key: Option<&str>,
        content: &str,
        role: MemoryRole,
        embedding: Option<Vec<f32>>,
    ) -> NewMemoryEntry {
        NewMemoryEntry {
            tenant_id: "default".to_string(),
            session_id: session_id.to_string(),
            channel_id: None,
            user_id: Some("user-1".to_string()),
            continuity_key: continuity_key.map(|s| s.to_string()),
            role,
            content: content.to_string(),
            embedding,
            embedding_model: Some("unit-test".to_string()),
            metadata: serde_json::json!({}),
        }
    }

    #[test]
    fn in_memory_creates_memory_entries_table() {
        let store = MemoryStore::in_memory().expect("failed to create in-memory memory store");
        let conn = store.connection().expect("lock should not be poisoned");
        let exists: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='memory_entries'",
                [],
                |row| row.get(0),
            )
            .expect("failed to query sqlite_master");

        assert_eq!(exists, 1);
    }

    #[test]
    fn schema_has_embedding_and_continuity_columns() {
        let store = MemoryStore::in_memory().expect("failed to create in-memory memory store");
        let conn = store.connection().expect("lock should not be poisoned");
        let mut stmt = conn
            .prepare("PRAGMA table_info(memory_entries)")
            .expect("failed to prepare pragma statement");

        let columns: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .expect("failed to read table info")
            .collect::<std::result::Result<Vec<_>, _>>()
            .expect("failed to collect columns");

        assert!(columns.iter().any(|c| c == "embedding"));
        assert!(columns.iter().any(|c| c == "embedding_model"));
        assert!(columns.iter().any(|c| c == "embedding_dimensions"));
        assert!(columns.iter().any(|c| c == "continuity_key"));
    }

    #[tokio::test]
    async fn remember_and_get_session_context_round_trip() {
        let store = MemoryStore::in_memory().expect("failed to create in-memory memory store");
        store
            .remember(entry(
                "session-a",
                Some("continuity-1"),
                "hello from telegram",
                MemoryRole::User,
                None,
            ))
            .await
            .expect("remember should succeed");

        let context = store
            .get_session_context("session-a", 10)
            .await
            .expect("session context should load");

        assert_eq!(context.session_id, "session-a");
        assert_eq!(context.entries.len(), 1);
        assert_eq!(context.entries[0].content, "hello from telegram");
    }

    #[tokio::test]
    async fn continuity_context_spans_multiple_sessions() {
        let store = MemoryStore::in_memory().expect("failed to create in-memory memory store");
        store
            .remember(entry(
                "session-telegram",
                Some("user-42"),
                "telegram memory",
                MemoryRole::User,
                None,
            ))
            .await
            .expect("first remember should succeed");
        store
            .remember(entry(
                "session-tui",
                Some("user-42"),
                "tui memory",
                MemoryRole::Assistant,
                None,
            ))
            .await
            .expect("second remember should succeed");

        let shared = store
            .get_continuity_context("user-42", 10)
            .await
            .expect("continuity context should load");

        assert_eq!(shared.len(), 2);
        assert!(shared.iter().any(|m| m.content == "telegram memory"));
        assert!(shared.iter().any(|m| m.content == "tui memory"));
    }

    #[tokio::test]
    async fn recall_prefers_embedding_similarity() {
        let store = MemoryStore::in_memory().expect("failed to create in-memory memory store");
        store
            .remember(entry(
                "session-a",
                Some("continuity-1"),
                "first",
                MemoryRole::User,
                Some(vec![1.0, 0.0, 0.0]),
            ))
            .await
            .expect("first remember should succeed");
        store
            .remember(entry(
                "session-a",
                Some("continuity-1"),
                "second",
                MemoryRole::User,
                Some(vec![0.0, 1.0, 0.0]),
            ))
            .await
            .expect("second remember should succeed");

        let recalled = store
            .recall(RecallQuery {
                tenant_id: None,
                query_text: None,
                query_embedding: Some(vec![0.95, 0.05, 0.0]),
                embedding_model: None,
                session_id: Some("session-a".to_string()),
                continuity_key: None,
                limit: 1,
            })
            .await
            .expect("recall should succeed");

        assert_eq!(recalled.len(), 1);
        assert_eq!(recalled[0].content, "first");
    }

    /// #954: cosseno entre modelos diferentes não é comparável. Com o filtro
    /// de modelo na query, a entrada do modelo errado perde o eixo semântico
    /// mesmo que o vetor dela seja "mais próximo".
    #[tokio::test]
    async fn recall_ignores_semantic_score_of_other_models() {
        let store = MemoryStore::in_memory().expect("failed to create in-memory memory store");
        let mut right_model = entry(
            "session-a",
            None,
            "resposta do modelo certo",
            MemoryRole::User,
            Some(vec![0.9, 0.1, 0.0]),
        );
        right_model.embedding_model = Some("modelo-ativo".to_string());
        store.remember(right_model).await.expect("remember");

        let mut wrong_model = entry(
            "session-a",
            None,
            "vetor identico, modelo trocado",
            MemoryRole::User,
            Some(vec![1.0, 0.0, 0.0]),
        );
        wrong_model.embedding_model = Some("modelo-antigo".to_string());
        store.remember(wrong_model).await.expect("remember");

        let recalled = store
            .recall(RecallQuery {
                tenant_id: None,
                query_text: None,
                query_embedding: Some(vec![1.0, 0.0, 0.0]),
                embedding_model: Some("modelo-ativo".to_string()),
                session_id: Some("session-a".to_string()),
                continuity_key: None,
                limit: 1,
            })
            .await
            .expect("recall should succeed");

        assert_eq!(recalled.len(), 1);
        assert_eq!(
            recalled[0].content, "resposta do modelo certo",
            "cosseno perfeito do modelo errado não pode vencer"
        );
    }

    /// Bypass de tenant no caminho KNN (achado da Trilha C): o vec0 não sabe
    /// de tenant, então o fetch dos candidatos TEM de reescopar. Antes desta
    /// correção, este teste devolvia a memória do tenant-b.
    #[tokio::test]
    async fn knn_recall_never_leaks_other_tenants() {
        let store =
            MemoryStore::in_memory_with_vectors().expect("failed to create in-memory store");
        if !store.knn_enabled() {
            eprintln!("sqlite-vec not available, skipping KNN isolation test");
            return;
        }

        let mut ours = entry(
            "session-a",
            None,
            "segredo do tenant-a",
            MemoryRole::User,
            Some(vec![0.9, 0.1, 0.0]),
        );
        ours.tenant_id = "tenant-a".to_string();
        store.remember(ours).await.expect("remember");

        let mut theirs = entry(
            "session-x",
            None,
            "segredo do tenant-b",
            MemoryRole::User,
            Some(vec![1.0, 0.0, 0.0]),
        );
        theirs.tenant_id = "tenant-b".to_string();
        store.remember(theirs).await.expect("remember");

        let recalled = store
            .recall(RecallQuery {
                tenant_id: Some("tenant-a".to_string()),
                query_text: None,
                query_embedding: Some(vec![1.0, 0.0, 0.0]),
                embedding_model: None,
                session_id: None,
                continuity_key: None,
                limit: 10,
            })
            .await
            .expect("recall should succeed");

        assert!(!recalled.is_empty(), "o próprio tenant continua achável");
        assert!(
            recalled.iter().all(|m| m.tenant_id == "tenant-a"),
            "linha de outro tenant vazou pelo KNN: {recalled:?}"
        );
    }

    /// O mesmo reescopo vale para session_id — o KNN devolvia vizinhos de
    /// qualquer sessão e o score não filtrava nada.
    #[tokio::test]
    async fn knn_recall_respects_session_filter() {
        let store =
            MemoryStore::in_memory_with_vectors().expect("failed to create in-memory store");
        if !store.knn_enabled() {
            eprintln!("sqlite-vec not available, skipping KNN session test");
            return;
        }

        store
            .remember(entry(
                "session-a",
                None,
                "da sessao a",
                MemoryRole::User,
                Some(vec![0.9, 0.1, 0.0]),
            ))
            .await
            .expect("remember");
        store
            .remember(entry(
                "session-b",
                None,
                "da sessao b",
                MemoryRole::User,
                Some(vec![1.0, 0.0, 0.0]),
            ))
            .await
            .expect("remember");

        let recalled = store
            .recall(RecallQuery {
                tenant_id: None,
                query_text: None,
                query_embedding: Some(vec![1.0, 0.0, 0.0]),
                embedding_model: None,
                session_id: Some("session-a".to_string()),
                continuity_key: None,
                limit: 10,
            })
            .await
            .expect("recall should succeed");

        assert!(
            recalled.iter().all(|m| m.session_id == "session-a"),
            "linha de outra sessao vazou pelo KNN: {recalled:?}"
        );
    }

    /// #960: deletar entradas não pode deixar vetor nem mapeamento órfão.
    #[tokio::test]
    async fn delete_session_memory_removes_vectors_and_mapping() {
        let store =
            MemoryStore::in_memory_with_vectors().expect("failed to create in-memory store");
        if !store.knn_enabled() {
            eprintln!("sqlite-vec not available, skipping orphan test");
            return;
        }

        store
            .remember(entry(
                "session-a",
                None,
                "vai embora",
                MemoryRole::User,
                Some(vec![1.0, 0.0, 0.0]),
            ))
            .await
            .expect("remember");

        let before = store.integrity_report().expect("report");
        assert_eq!(before.map_rows, 1);

        let deleted = store
            .delete_session_memory("session-a")
            .await
            .expect("delete");
        assert_eq!(deleted, 1);

        let after = store.integrity_report().expect("report");
        assert_eq!(after.entries_total, 0);
        assert_eq!(after.map_rows, 0, "mapeamento órfão sobrou");
        assert!(
            after.vec_rows_by_table.iter().all(|(_, n)| *n == 0),
            "vetor órfão sobrou: {:?}",
            after.vec_rows_by_table
        );
    }

    /// #960 + #956: o compact também limpa o índice, não só as linhas.
    #[tokio::test]
    async fn compact_removes_vectors_too() {
        let store =
            MemoryStore::in_memory_with_vectors().expect("failed to create in-memory store");
        if !store.knn_enabled() {
            eprintln!("sqlite-vec not available, skipping compact orphan test");
            return;
        }

        store
            .remember(entry(
                "session-a",
                None,
                "antigo",
                MemoryRole::User,
                Some(vec![0.0, 1.0, 0.0]),
            ))
            .await
            .expect("remember");

        let report = store
            .compact(Utc::now() + Duration::seconds(5))
            .await
            .expect("compact");
        assert_eq!(report.deleted_entries, 1);

        let after = store.integrity_report().expect("report");
        assert_eq!(after.map_rows, 0);
        assert!(after.vec_rows_by_table.iter().all(|(_, n)| *n == 0));
    }

    /// #960: o relatório conta com/sem embedding (a fila de reindex do #953)
    /// e denuncia mapeamentos órfãos plantados à mão.
    #[tokio::test]
    async fn integrity_report_counts_and_finds_orphans() {
        let store =
            MemoryStore::in_memory_with_vectors().expect("failed to create in-memory store");

        store
            .remember(entry(
                "session-a",
                None,
                "com vetor",
                MemoryRole::User,
                Some(vec![1.0, 0.0, 0.0]),
            ))
            .await
            .expect("remember");
        store
            .remember(entry(
                "session-a",
                None,
                "sem vetor (fila de reindex)",
                MemoryRole::User,
                None,
            ))
            .await
            .expect("remember");

        let report = store.integrity_report().expect("report");
        assert_eq!(report.entries_total, 2);
        assert_eq!(report.entries_with_embedding, 1);
        assert_eq!(report.entries_without_embedding, 1);

        if store.knn_enabled() {
            assert_eq!(report.map_rows, 1);
            assert!(report.orphan_map_entries.is_empty());

            // Órfão plantado: mapeamento sem entrada correspondente.
            let vs = store.vector_store.as_ref().expect("vec store present");
            vs.insert_embedding("fantasma", &[0.0, 0.0, 1.0], 3)
                .expect("insert orphan");
            let report = store.integrity_report().expect("report");
            assert_eq!(report.orphan_map_entries, vec!["fantasma".to_string()]);
        }
    }

    #[tokio::test]
    async fn compact_and_delete_session_memory_work() {
        let store = MemoryStore::in_memory().expect("failed to create in-memory memory store");
        store
            .remember(entry(
                "session-a",
                Some("continuity-1"),
                "transient",
                MemoryRole::User,
                None,
            ))
            .await
            .expect("remember should succeed");

        let report = store
            .compact(Utc::now() + Duration::seconds(5))
            .await
            .expect("compact should succeed");
        assert_eq!(report.deleted_entries, 1);

        store
            .remember(entry(
                "session-a",
                Some("continuity-1"),
                "persist-me",
                MemoryRole::Assistant,
                None,
            ))
            .await
            .expect("remember should succeed");

        let deleted = store
            .delete_session_memory("session-a")
            .await
            .expect("delete should succeed");
        assert_eq!(deleted, 1);
    }
}
