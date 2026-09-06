use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, Utc};
use garraia_common::{Error, Result};
use rusqlite::{Connection, OptionalExtension, params};
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
    /// Prazo de validade (#959). Passado esse instante a entrada some do
    /// recall — mesmo antes de a compactacao rodar — e a proxima compactacao
    /// a apaga. `None` = sem prazo.
    #[serde(default)]
    pub ttl_expires_at: Option<DateTime<Utc>>,
    /// Quando foi fixada (#959). Entrada fixada **nunca** e apagada pela
    /// compactacao, por mais antiga que fique. `None` = nao fixada.
    #[serde(default)]
    pub pinned_at: Option<DateTime<Utc>>,
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
/// reindexação (#953), e `entries_missing_model > 0` é a fila legada.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrityReport {
    pub entries_total: usize,
    pub entries_with_embedding: usize,
    pub entries_without_embedding: usize,
    /// Entradas COM vetor gravado mas **sem** `embedding_model` — legado de
    /// antes do #954. Elas perdem o eixo semântico do score (0.7) sempre que
    /// o recall chega com um modelo definido, e o caminho SQL não avisa nada:
    /// este contador é o único sinal que o operador tem. Zera com reindex
    /// (#953).
    pub entries_missing_model: usize,
    /// Entradas fixadas — as que a compactacao nunca apaga (#959).
    pub entries_pinned: usize,
    /// Entradas com prazo ja vencido. Elas ja nao aparecem no recall; ficam
    /// no banco ate a proxima compactacao (#959).
    pub entries_expired: usize,
    /// Linhas em `vec_id_map`.
    pub map_rows: usize,
    /// `(tabela, linhas)` por tabela `vec_embeddings_*` existente.
    pub vec_rows_by_table: Vec<(String, usize)>,
    /// Ids mapeados no índice sem entrada correspondente em `memory_entries`.
    pub orphan_map_entries: Vec<String>,
}

/// Uma linha do agrupamento por modelo do `garra memory stats` (#950).
///
/// `embedding_model: None` com `embedding_dimensions: None` é a fila de
/// reindexação; `None` com dimensão preenchida é o legado de antes do #954 —
/// vetor gravado sem registrar quem o produziu.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingBreakdownRow {
    pub embedding_model: Option<String>,
    pub embedding_dimensions: Option<usize>,
    pub entries: usize,
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

/// A condicao de compactacao (#959), escrita **inteira** nas duas sentencas.
///
/// Nao ha `format!` montando SQL aqui de proposito (regra absoluta 5): as duas
/// sao literais estaticas e o unico valor vai por bind. O preco e a repeticao
/// da clausula, e o `where_clauses_stay_in_sync` cobra que ela seja identica
/// nas duas — se divergirem, o DELETE apaga linha que o SELECT nao listou e o
/// vetor dela fica orfao no indice.
const COMPACT_SELECT_SQL: &str = "SELECT id FROM memory_entries WHERE pinned_at IS NULL \
     AND (datetime(created_at) < datetime(?1) \
          OR (ttl_expires_at IS NOT NULL AND datetime(ttl_expires_at) <= datetime('now')))";

const COMPACT_DELETE_SQL: &str = "DELETE FROM memory_entries WHERE pinned_at IS NULL \
     AND (datetime(created_at) < datetime(?1) \
          OR (ttl_expires_at IS NOT NULL AND datetime(ttl_expires_at) <= datetime('now')))";

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
        let (
            entries_total,
            entries_with_embedding,
            entries_missing_model,
            entries_pinned,
            entries_expired,
            live_ids,
        ) = {
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
            let missing_model: i64 = conn
                .query_row(
                    "SELECT count(*) FROM memory_entries
                     WHERE embedding IS NOT NULL AND embedding_model IS NULL",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| {
                    Error::Database(format!("failed to count entries missing model: {e}"))
                })?;
            let pinned: i64 = conn
                .query_row(
                    "SELECT count(*) FROM memory_entries WHERE pinned_at IS NOT NULL",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| Error::Database(format!("failed to count pinned entries: {e}")))?;
            let expired: i64 = conn
                .query_row(
                    "SELECT count(*) FROM memory_entries
                     WHERE ttl_expires_at IS NOT NULL
                       AND datetime(ttl_expires_at) <= datetime('now')",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| Error::Database(format!("failed to count expired entries: {e}")))?;
            let mut stmt = conn
                .prepare("SELECT id FROM memory_entries")
                .map_err(|e| Error::Database(format!("failed to prepare id scan: {e}")))?;
            let ids: Vec<String> = stmt
                .query_map([], |row| row.get(0))
                .map_err(|e| Error::Database(format!("failed to scan entry ids: {e}")))?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| Error::Database(format!("failed to collect entry ids: {e}")))?;
            (
                total as usize,
                with_embedding as usize,
                missing_model as usize,
                pinned as usize,
                expired as usize,
                ids,
            )
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
            entries_missing_model,
            entries_pinned,
            entries_expired,
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

        // Colunas de retencao (#959) num banco que ja existia. Mesmo padrao do
        // `tenant_id` acima, e pela mesma razao: o SQLite nao tem
        // `ADD COLUMN IF NOT EXISTS`, entao a segunda execucao devolve
        // "duplicate column name" — que aqui e sucesso, nao falha.
        //
        // **Antes** do `execute_batch` abaixo, nao depois: o schema V1 cria um
        // indice sobre `ttl_expires_at`, e num banco que ja existe o
        // `CREATE TABLE IF NOT EXISTS` e no-op — a coluna so aparece por este
        // `ALTER`. Invertida, a ordem quebrava toda instalacao existente na
        // atualizacao, com `no such column: ttl_expires_at`. Coberto por
        // `banco_antigo_ganha_as_colunas_de_retencao_na_abertura`.
        for sql in crate::migrations::MEMORY_RETENTION_COLUMNS {
            let _ = conn.execute_batch(sql);
        }

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

    /// Apaga o que passou do prazo ou ficou velho demais.
    ///
    /// Duas condicoes, unidas por `OR` (#959):
    ///
    /// - **vencida** — `ttl_expires_at` no passado, por mais nova que seja;
    /// - **velha** — criada antes de `before`.
    ///
    /// E uma excecao que vence as duas: **entrada fixada nunca e apagada**.
    /// E para isso que o pin existe — proteger o que importa de uma politica
    /// de retencao que nao sabe o que importa.
    pub async fn compact(&self, before: DateTime<Utc>) -> Result<CompactionReport> {
        // Coletar os ids ANTES de deletar (são eles que limpam o índice
        // vetorial, #960) e SOB O MESMO guard do DELETE: soltar o mutex entre
        // os dois abriria uma janela TOCTOU em que uma inserção concorrente
        // casando a condição seria deletada sem entrar na lista de limpeza
        // (achado M1 da auditoria).
        let (doomed, deleted) = {
            let conn = self.connection()?;
            let mut stmt = conn
                .prepare(COMPACT_SELECT_SQL)
                .map_err(|e| Error::Database(format!("failed to prepare compact scan: {e}")))?;
            let ids: Vec<String> = stmt
                .query_map(params![before.to_rfc3339()], |row| row.get(0))
                .map_err(|e| Error::Database(format!("failed to scan compact targets: {e}")))?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| Error::Database(format!("failed to collect compact targets: {e}")))?;
            drop(stmt);

            let deleted = conn
                .execute(COMPACT_DELETE_SQL, params![before.to_rfc3339()])
                .map_err(|e| Error::Database(format!("failed to compact memory entries: {e}")))?;
            (ids, deleted)
        };

        self.cleanup_vectors(&doomed);

        Ok(CompactionReport {
            deleted_entries: deleted,
            before,
        })
    }

    pub async fn delete_session_memory(&self, session_id: &str) -> Result<usize> {
        // SELECT e DELETE sob o mesmo guard — ver o comentário em `compact`.
        let (doomed, deleted) = {
            let conn = self.connection()?;
            let mut stmt = conn
                .prepare("SELECT id FROM memory_entries WHERE session_id = ?")
                .map_err(|e| Error::Database(format!("failed to prepare delete scan: {e}")))?;
            let ids: Vec<String> = stmt
                .query_map(params![session_id], |row| row.get(0))
                .map_err(|e| Error::Database(format!("failed to scan delete targets: {e}")))?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| Error::Database(format!("failed to collect delete targets: {e}")))?;
            drop(stmt);

            let deleted = conn
                .execute(
                    "DELETE FROM memory_entries WHERE session_id = ?",
                    params![session_id],
                )
                .map_err(|e| Error::Database(format!("failed to delete session memory: {e}")))?;
            (ids, deleted)
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

    /// Entradas gravadas **sem vetor** — a fila de reindexação (#953).
    ///
    /// O critério é só `embedding IS NULL`. A issue propunha exigir também
    /// `embedding_model IS NULL`, mas isso perderia exatamente as entradas
    /// mais antigas: até o #948 o modelo era gravado mesmo sem vetor, então
    /// as linhas legadas têm um modelo que mente. Filtrar por ele deixaria
    /// justamente elas de fora da reindexação.
    ///
    /// Ordena da mais antiga para a mais nova: numa base grande, reindexar
    /// em lotes sucessivos avança de forma previsível em vez de reprocessar
    /// as mesmas linhas.
    pub fn entries_missing_embeddings(&self, limit: usize) -> Result<Vec<MemoryEntry>> {
        let conn = self.connection()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, tenant_id, session_id, channel_id, user_id, continuity_key, role,
                        content, embedding, embedding_model, embedding_dimensions, metadata,
                        created_at, ttl_expires_at, pinned_at
                 FROM memory_entries
                 WHERE embedding IS NULL
                   -- Vencida nao entra na fila: gerar vetor para uma entrada
                   -- que a proxima compactacao apaga e chamada de provider
                   -- paga por trabalho que vai para o lixo (#959).
                   AND (ttl_expires_at IS NULL OR datetime(ttl_expires_at) > datetime('now'))
                 ORDER BY datetime(created_at) ASC
                 LIMIT ?",
            )
            .map_err(|e| Error::Database(format!("failed to prepare reindex scan: {e}")))?;

        let rows = stmt
            .query_map(params![limit as i64], row_to_entry)
            .map_err(|e| Error::Database(format!("failed to scan entries to reindex: {e}")))?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Database(format!("failed to collect entries to reindex: {e}")))
    }

    /// Grava o vetor de uma entrada que estava sem, e a indexa (#953).
    ///
    /// Devolve `false` quando o id não existe — a entrada pode ter sido
    /// apagada entre a listagem e a gravação, e isso não é erro.
    ///
    /// O modelo é gravado **junto** com o vetor, nunca antes: é a mesma regra
    /// que o #948 estabeleceu na ingestão, e ela vale aqui pelo mesmo motivo
    /// — a coluna descreve o vetor.
    pub fn set_embedding(&self, id: &str, embedding: &[f32], model: &str) -> Result<bool> {
        let blob = embedding_to_blob(embedding);
        let dims = embedding.len();

        let updated = {
            let conn = self.connection()?;
            conn.execute(
                "UPDATE memory_entries
                 SET embedding = ?, embedding_model = ?, embedding_dimensions = ?
                 WHERE id = ?",
                params![blob, model, dims as i64, id],
            )
            .map_err(|e| Error::Database(format!("failed to write embedding: {e}")))?
        };

        if updated == 0 {
            return Ok(false);
        }

        // A coluna e o indice vivem em conexoes diferentes (por desenho:
        // o `VectorStore` tem a sua), entao nao ha transacao que cubra os
        // dois. Se o indice recusar o vetor, a coluna volta a NULL: caso
        // contrario a entrada sairia da fila do reindex (`embedding IS NULL`)
        // sem ter entrado no indice, e ficaria invisivel para a busca
        // semantica **sem nenhum caminho de reparo**. Melhor continuar na
        // fila e ser tentada de novo.
        if let Some(vs) = &self.vector_store
            && let Err(e) = vs
                .ensure_vec_table(dims)
                .and_then(|()| vs.insert_embedding(id, embedding, dims))
        {
            self.clear_embedding_columns(id);
            return Err(e);
        }

        Ok(true)
    }

    /// Desfaz a escrita da coluna quando o indice recusou o vetor.
    ///
    /// Best-effort de propria natureza: se este UPDATE tambem falhar, o
    /// banco fica no estado que a chamada tentava evitar — e ai o `warn!` e
    /// o unico sinal, porque o proximo `garra memory stats` so mostra a
    /// divergencia agregada (`map_rows` menor que `entries_with_embedding`).
    fn clear_embedding_columns(&self, id: &str) {
        let Ok(conn) = self.connection() else {
            tracing::warn!("nao foi possivel reverter a coluna de embedding: lock envenenado");
            return;
        };
        if let Err(e) = conn.execute(
            "UPDATE memory_entries
             SET embedding = NULL, embedding_model = NULL, embedding_dimensions = NULL
             WHERE id = ?",
            params![id],
        ) {
            tracing::warn!(
                "o indice recusou o vetor e a coluna nao pode ser revertida; a entrada \
                 ficou marcada como indexada sem estar: {e}"
            );
        }
    }

    /// Copia o banco inteiro para `dest`, consistente (#955).
    ///
    /// Usa `VACUUM INTO`, e não `cp`. A diferença importa: o banco roda em
    /// WAL, então copiar o arquivo com `cp` pega uma foto sem as transações
    /// que ainda estão no `-wal`, e o resultado é um backup que parece bom e
    /// está incompleto. O `VACUUM INTO` lê sob uma transação e escreve o
    /// estado **commitado** inteiro — sem precisar de checkpoint, sem parar o
    /// gateway. De brinde, compacta: páginas livres não vão junto.
    ///
    /// **O índice vetorial vai junto.** Foi verificado, não presumido: uma
    /// sonda contra um banco com `vec_embeddings_*` real confirmou que as
    /// tabelas virtuais vec0, as sombras delas e o `vec_id_map` chegam
    /// íntegros do outro lado — a cópia reabre com o mesmo
    /// `integrity_report`. Era o risco de verdade aqui; o WAL, que a issue
    /// levantou, o `VACUUM INTO` já resolve sozinho.
    ///
    /// Devolve o tamanho do arquivo gerado, em bytes.
    ///
    /// Falha se `dest` já existir — é o SQLite que exige, e é a regra certa:
    /// um backup que sobrescreve outro em silêncio é um backup a menos.
    pub fn backup_to(&self, dest: &Path) -> Result<u64> {
        if dest.exists() {
            return Err(Error::Database(format!(
                "backup destination already exists: {}",
                dest.display()
            )));
        }

        {
            let conn = self.connection()?;
            conn.execute("VACUUM INTO ?", params![dest.to_string_lossy()])
                .map_err(|e| Error::Database(format!("failed to back up memory database: {e}")))?;
        }

        std::fs::metadata(dest)
            .map(|m| m.len())
            .map_err(|e| Error::Database(format!("backup written but not readable: {e}")))
    }

    /// Reinsere no indice os vetores que ja estao na coluna mas nao no mapa.
    ///
    /// Este e o estado que o `remember_sync` produz na operacao normal: ele
    /// grava a linha e insere no indice em best-effort, com `warn!`, entao um
    /// sqlite-vec momentaneamente indisponivel deixa entradas com vetor na
    /// coluna e fora da busca semantica — e o `reindex` por si so nao as
    /// alcanca, porque ele procura `embedding IS NULL`.
    ///
    /// Nao chama provider nenhum: o vetor ja existe, so falta indexa-lo. Por
    /// isso roda sempre, inclusive sem provider configurado.
    ///
    /// Devolve quantas foram reindexadas. `Ok(0)` quando o KNN esta desligado
    /// — sem indice nao ha o que reparar.
    pub fn reindex_missing_index_rows(&self, limit: usize) -> Result<usize> {
        let Some(vs) = &self.vector_store else {
            return Ok(0);
        };
        if !vs.vec_enabled() {
            return Ok(0);
        }

        // O conjunto de indexados vem da conexão do próprio `VectorStore`:
        // em produção ele é o mesmo arquivo, mas em memória é um segundo
        // banco, e um `JOIN` daqui não enxergaria o `vec_id_map`.
        let mapeados: std::collections::HashSet<String> = vs.mapped_ids()?.into_iter().collect();

        // Só os ids primeiro (barato), e o blob depois, apenas das que estão
        // realmente fora do índice: numa base de milhares, carregar todo
        // vetor para descobrir que quase nenhum falta seria o custo errado.
        let faltantes: Vec<String> = {
            let conn = self.connection()?;
            let mut stmt = conn
                .prepare(
                    "SELECT id FROM memory_entries
                     WHERE embedding IS NOT NULL
                       -- Mesma razao do `entries_missing_embeddings`: nao
                       -- devolver ao indice o vetor de uma entrada vencida.
                       AND (ttl_expires_at IS NULL OR datetime(ttl_expires_at) > datetime('now'))
                     ORDER BY datetime(created_at) ASC",
                )
                .map_err(|e| Error::Database(format!("failed to prepare index repair: {e}")))?;

            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| Error::Database(format!("failed to scan index repair: {e}")))?;

            let mut out = Vec::new();
            for row in rows {
                let id =
                    row.map_err(|e| Error::Database(format!("failed to read entry id: {e}")))?;
                if !mapeados.contains(&id) {
                    out.push(id);
                    if out.len() >= limit {
                        break;
                    }
                }
            }
            out
        };

        let mut reparadas = 0usize;
        for id in faltantes {
            let vetor = {
                let conn = self.connection()?;
                let blob: Option<Vec<u8>> = conn
                    .query_row(
                        "SELECT embedding FROM memory_entries WHERE id = ?",
                        params![id],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(|e| Error::Database(format!("failed to read embedding: {e}")))?
                    .flatten();
                match blob {
                    Some(b) => blob_to_embedding(&b)?,
                    // A entrada sumiu (ou perdeu o vetor) entre a listagem e
                    // aqui: não é erro, é concorrência.
                    None => continue,
                }
            };

            let dims = vetor.len();
            vs.ensure_vec_table(dims)?;
            vs.insert_embedding(&id, &vetor, dims)?;
            reparadas += 1;
        }
        Ok(reparadas)
    }

    /// Quantas entradas por `(modelo, dimensoes)` do vetor gravado (#950).
    ///
    /// `(None, None)` é a fila de reindexação; `(None, Some(d))` é uma linha
    /// com vetor e sem modelo — o legado de antes do #954 que o
    /// `IntegrityReport` conta agregado e que aqui aparece com a dimensão.
    ///
    /// Ordena por contagem decrescente: numa base misturada, o modelo que
    /// domina é o que responde "com o que este índice foi construído?".
    pub fn embedding_breakdown(&self) -> Result<Vec<EmbeddingBreakdownRow>> {
        let conn = self.connection()?;
        let mut stmt = conn
            .prepare(
                "SELECT embedding_model, embedding_dimensions, count(*)
                 FROM memory_entries
                 GROUP BY embedding_model, embedding_dimensions
                 ORDER BY count(*) DESC",
            )
            .map_err(|e| Error::Database(format!("failed to prepare breakdown: {e}")))?;

        let rows = stmt
            .query_map([], |row| {
                let model: Option<String> = row.get(0)?;
                let dims: Option<i64> = row.get(1)?;
                let total: i64 = row.get(2)?;
                Ok(EmbeddingBreakdownRow {
                    embedding_model: model,
                    embedding_dimensions: dims.map(|d| d as usize),
                    entries: total as usize,
                })
            })
            .map_err(|e| Error::Database(format!("failed to run breakdown: {e}")))?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Database(format!("failed to collect breakdown: {e}")))
    }

    /// Distâncias cruas dos vizinhos mais próximos, para a CLI mostrar (#950).
    ///
    /// Existe **só** para anotar o resultado do [`Self::recall`], nunca para
    /// substituí-lo: o índice vec0 não conhece tenant, sessão nem modelo, e
    /// ler dele direto foi exatamente o bypass de isolamento que o #971
    /// fechou. Por isso devolve `(id, distância)` e não entradas — quem chama
    /// já tem as linhas que passaram pelo escopo e usa isto para enriquecer o
    /// que já ganhou.
    ///
    /// Vazio quando o KNN está desligado.
    pub fn knn_distances(&self, embedding: &[f32], limit: usize) -> Result<Vec<(String, f64)>> {
        match &self.vector_store {
            Some(vs) if vs.vec_enabled() => vs.search_nearest(embedding, embedding.len(), limit),
            _ => Ok(Vec::new()),
        }
    }

    /// Entradas mais recentes, sem filtro de sessão — o que a CLI lista.
    pub fn recent_entries(&self, limit: usize) -> Result<Vec<MemoryEntry>> {
        self.query_candidates_with_tenant_sync(None, None, None, None, limit)
    }

    /// Define (ou remove, com `None`) o prazo de validade de uma entrada.
    ///
    /// Devolve `false` se o id nao existe. Um prazo no passado nao apaga a
    /// entrada na hora: ela some do recall imediatamente e a proxima
    /// compactacao a remove — a separacao entre "invisivel" e "apagada" e o
    /// que torna o TTL reversivel enquanto o `compact` nao roda.
    pub fn set_ttl(&self, id: &str, expires_at: Option<DateTime<Utc>>) -> Result<bool> {
        let conn = self.connection()?;
        let updated = conn
            .execute(
                "UPDATE memory_entries SET ttl_expires_at = ? WHERE id = ?",
                params![expires_at.map(|t| t.to_rfc3339()), id],
            )
            .map_err(|e| Error::Database(format!("failed to set ttl: {e}")))?;
        Ok(updated > 0)
    }

    /// Fixa (ou solta) uma entrada. Fixada nunca e apagada pela **compactacao**.
    ///
    /// O alcance do pin e exatamente esse, e vale ser explicito sobre o que
    /// ele **nao** cobre: [`Self::delete_entry`] e
    /// [`Self::delete_session_memory`] apagam entrada fixada sem perguntar.
    /// Sao deleoes que alguem pediu nominalmente — o pin protege da politica
    /// automatica, que apaga sem saber o que esta apagando, nao da mao do
    /// operador.
    ///
    /// Devolve `false` se o id nao existe.
    pub fn set_pinned(&self, id: &str, pinned: bool) -> Result<bool> {
        let conn = self.connection()?;
        let updated = conn
            .execute(
                "UPDATE memory_entries SET pinned_at = ? WHERE id = ?",
                params![pinned.then(|| Utc::now().to_rfc3339()), id],
            )
            .map_err(|e| Error::Database(format!("failed to set pin: {e}")))?;
        Ok(updated > 0)
    }

    /// Apaga uma entrada pelo id, junto do vetor e do mapeamento.
    ///
    /// Devolve `false` se o id não existia.
    pub fn delete_entry(&self, id: &str) -> Result<bool> {
        let deleted = {
            let conn = self.connection()?;
            conn.execute("DELETE FROM memory_entries WHERE id = ?", params![id])
                .map_err(|e| Error::Database(format!("failed to delete entry: {e}")))?
        };

        if deleted == 0 {
            return Ok(false);
        }

        self.cleanup_vectors(&[id.to_string()]);
        Ok(true)
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
        // Quantos candidatos perderam o eixo semântico por modelo divergente.
        // Entrada legada (modelo NULL) cai aqui e some do ranking sem nenhum
        // outro aviso — o contador durável está no `integrity_report`.
        let mut demoted_other_model = 0usize;

        let mut scored: Vec<(f32, MemoryEntry)> = candidates
            .into_iter()
            .map(|entry| {
                // Cosseno entre modelos diferentes não significa nada (#954):
                // com filtro de modelo na query, entrada de outro modelo pontua
                // 0.0 no eixo semântico e compete só por texto/recência.
                // `(None, _) => true` e deliberado: query sem modelo declarado
                // e chamada legada ou so-texto, e ali o comportamento antigo
                // (comparar com o que houver) vale mais que recusar tudo. Quem
                // manda o modelo — todo recall do runtime desde o #954 — cai
                // nos dois primeiros bracos e fica protegido.
                let same_model = match (&query.embedding_model, &entry.embedding_model) {
                    (Some(wanted), Some(got)) => wanted == got,
                    (Some(_), None) => false,
                    (None, _) => true,
                };
                let semantic_score = match (&query.query_embedding, &entry.embedding) {
                    (Some(needle), Some(candidate)) if same_model => {
                        cosine_similarity(needle, candidate)
                    }
                    _ => {
                        if !same_model && entry.embedding.is_some() {
                            demoted_other_model += 1;
                        }
                        0.0
                    }
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

        if demoted_other_model > 0 {
            tracing::debug!(
                demoted = demoted_other_model,
                model = query.embedding_model.as_deref().unwrap_or("<nenhum>"),
                "recall: candidatos com vetor de outro modelo (ou legado sem \
                 modelo) pontuaram 0.0 no eixo semantico"
            );
        }

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
                    embedding, embedding_model, embedding_dimensions, metadata, created_at,
                    ttl_expires_at, pinned_at
             FROM memory_entries
             WHERE id IN ({placeholders})
               AND (? IS NULL OR tenant_id = ?)
               AND (? IS NULL OR session_id = ?)
               AND (? IS NULL OR continuity_key = ?)
               AND (? IS NULL OR embedding_model = ?)
               -- Expirada nao volta pelo caminho KNN tambem (#959). O indice
               -- vec0 nao conhece prazo — so distancia —, entao sem esta linha
               -- uma entrada vencida voltaria pelo KNN enquanto o caminho SQL
               -- a filtrava. E a mesma classe de furo que o #971 fechou para
               -- tenant.
               AND (ttl_expires_at IS NULL OR datetime(ttl_expires_at) > datetime('now'))"
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
                        embedding, embedding_model, embedding_dimensions, metadata, created_at,
                        ttl_expires_at, pinned_at
                 FROM memory_entries
                 WHERE (?1 IS NULL OR tenant_id = ?1)
                   AND (?2 IS NULL OR session_id = ?2)
                   AND (?3 IS NULL OR continuity_key = ?3)
                   AND (?4 IS NULL OR lower(content) LIKE '%' || lower(?4) || '%')
                   -- Entrada vencida sai do recall na hora, sem esperar a
                   -- compactacao rodar (#959).
                   AND (ttl_expires_at IS NULL OR datetime(ttl_expires_at) > datetime('now'))
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

    // Um timestamp ilegivel nestas duas colunas nao derruba a leitura da
    // entrada — recusar a linha inteira tiraria o conteudo da vista do
    // operador por causa de uma coluna auxiliar. Vira `None` e sai um `warn!`.
    //
    // Sobre o pin, vale ser exato: **a protecao nao depende deste parse**. A
    // compactacao filtra por `pinned_at IS NULL` no SQL, e uma string
    // ilegivel nao e NULL — a entrada continua protegida. O que o `None` aqui
    // causa e so a CLI mostrar como nao-fixada uma entrada que esta fixada, e
    // e essa discrepancia que o aviso denuncia.
    let id_para_aviso: String = row.get(0)?;
    let ttl_expires_at = parse_optional_timestamp(row.get(13)?, "ttl_expires_at", &id_para_aviso);
    let pinned_at = parse_optional_timestamp(row.get(14)?, "pinned_at", &id_para_aviso);

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
        ttl_expires_at,
        pinned_at,
    })
}

/// Le um timestamp opcional, avisando quando a coluna existe e nao parseia.
///
/// O aviso carrega o id da entrada e o nome da coluna — nunca o conteudo dela
/// (regra absoluta 6). O id e o unico jeito de o operador achar a linha para
/// consertar.
fn parse_optional_timestamp(
    raw: Option<String>,
    coluna: &str,
    entry_id: &str,
) -> Option<DateTime<Utc>> {
    let raw = raw?;
    match parse_timestamp(&raw) {
        Ok(ts) => Some(ts),
        Err(_) => {
            tracing::warn!(
                entry_id,
                coluna,
                "coluna de retencao com timestamp ilegivel; tratada como ausente na leitura \
                 (a protecao do pin, que e feita em SQL, nao depende deste parse)"
            );
            None
        }
    }
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

    /// O `remember_sync` grava a linha e insere no indice em best-effort:
    /// com o sqlite-vec fora do ar por um momento, a entrada fica com vetor
    /// na coluna e **fora** da busca semantica. O `reindex` normal nao a
    /// alcanca, porque ele procura `embedding IS NULL` — quem repara e este
    /// caminho, e sem custo de provider, porque o vetor ja existe.
    #[tokio::test]
    async fn reindex_missing_index_rows_devolve_ao_indice_o_vetor_orfao_de_coluna() {
        let store = MemoryStore::in_memory_with_vectors().expect("store com vetores");
        assert!(
            store.knn_enabled(),
            "sqlite-vec e compilado no binario; sem ele este teste nao afirma nada"
        );

        let id = store
            .remember(entry(
                "s1",
                None,
                "entrada que perdeu o indice",
                MemoryRole::User,
                None,
            ))
            .await
            .expect("insere sem vetor");

        // Estado que o best-effort do `remember_sync` produz: coluna escrita,
        // indice intocado.
        {
            let conn = store.connection().expect("lock");
            conn.execute(
                "UPDATE memory_entries
                 SET embedding = ?, embedding_model = 'unit-test', embedding_dimensions = 4
                 WHERE id = ?",
                rusqlite::params![super::embedding_to_blob(&[0.1, 0.2, 0.3, 0.4]), id],
            )
            .expect("simula a coluna escrita sem indice");
        }

        let antes = store.integrity_report().expect("report");
        assert_eq!(antes.entries_with_embedding, 1);
        assert_eq!(antes.map_rows, 0, "o indice ainda nao conhece a entrada");

        let reparadas = store.reindex_missing_index_rows(100).expect("reparo");
        assert_eq!(reparadas, 1);

        let depois = store.integrity_report().expect("report");
        assert_eq!(depois.map_rows, 1);

        // E agora ela e alcancavel pelo caminho KNN.
        let achados = store
            .recall(RecallQuery {
                tenant_id: None,
                query_text: None,
                query_embedding: Some(vec![0.1, 0.2, 0.3, 0.4]),
                embedding_model: Some("unit-test".to_string()),
                session_id: None,
                continuity_key: None,
                limit: 5,
            })
            .await
            .expect("recall");
        assert_eq!(achados.len(), 1);
        assert_eq!(achados[0].id, id);
    }

    /// Rodar o reparo com o indice ja consistente nao e erro nem trabalho.
    #[tokio::test]
    async fn reindex_missing_index_rows_e_no_op_quando_tudo_esta_indexado() {
        let store = MemoryStore::in_memory_with_vectors().expect("store com vetores");
        assert!(store.knn_enabled());

        store
            .remember(entry(
                "s1",
                None,
                "ja indexada",
                MemoryRole::User,
                Some(vec![0.5, 0.5, 0.5, 0.5]),
            ))
            .await
            .expect("insere com vetor");

        assert_eq!(store.reindex_missing_index_rows(100).expect("reparo"), 0);
    }

    /// A ordem e da mais antiga para a mais nova e o teto e respeitado: numa
    /// base grande, reparar em fatias precisa avancar, nao repetir.
    #[tokio::test]
    async fn reindex_missing_index_rows_respeita_o_teto() {
        let store = MemoryStore::in_memory_with_vectors().expect("store com vetores");
        assert!(store.knn_enabled());

        for i in 0..3 {
            let id = store
                .remember(entry(
                    "s1",
                    None,
                    &format!("entrada {i}"),
                    MemoryRole::User,
                    None,
                ))
                .await
                .expect("insere");
            let conn = store.connection().expect("lock");
            conn.execute(
                "UPDATE memory_entries SET embedding = ?, embedding_dimensions = 4 WHERE id = ?",
                rusqlite::params![super::embedding_to_blob(&[0.1, 0.2, 0.3, 0.4]), id],
            )
            .expect("simula coluna sem indice");
        }

        assert_eq!(store.reindex_missing_index_rows(2).expect("reparo"), 2);
        assert_eq!(store.reindex_missing_index_rows(2).expect("reparo"), 1);
        assert_eq!(store.reindex_missing_index_rows(2).expect("reparo"), 0);
    }

    /// As duas sentencas da compactacao precisam ter a MESMA clausula: se
    /// divergirem, o DELETE apaga linha que o SELECT nao listou e o vetor
    /// dela fica orfao no indice — o achado M1 da auditoria, na outra
    /// direcao.
    #[test]
    fn compact_where_clauses_stay_in_sync() {
        let select = super::COMPACT_SELECT_SQL
            .split_once("WHERE ")
            .expect("SELECT tem WHERE")
            .1;
        let delete = super::COMPACT_DELETE_SQL
            .split_once("WHERE ")
            .expect("DELETE tem WHERE")
            .1;
        assert_eq!(select, delete);
    }

    /// Fixar protege da compactacao — e e para isso que o pin existe.
    #[tokio::test]
    async fn compact_nunca_apaga_entrada_fixada() {
        let store = MemoryStore::in_memory().expect("store");
        let fixada = store
            .remember(entry("s1", None, "importante", MemoryRole::User, None))
            .await
            .expect("insere");
        store
            .remember(entry("s1", None, "descartavel", MemoryRole::User, None))
            .await
            .expect("insere");

        assert!(store.set_pinned(&fixada, true).expect("fixa"));

        // Corte no futuro: sem o pin, as duas iriam embora.
        let report = store
            .compact(Utc::now() + Duration::seconds(5))
            .await
            .expect("compact");

        assert_eq!(report.deleted_entries, 1);
        let restantes = store.recent_entries(10).expect("lista");
        assert_eq!(restantes.len(), 1);
        assert_eq!(restantes[0].id, fixada);
        assert!(restantes[0].pinned_at.is_some());
    }

    /// Soltar o pin devolve a entrada a politica de retencao.
    #[tokio::test]
    async fn soltar_o_pin_devolve_a_entrada_a_compactacao() {
        let store = MemoryStore::in_memory().expect("store");
        let id = store
            .remember(entry("s1", None, "temporaria", MemoryRole::User, None))
            .await
            .expect("insere");
        assert!(store.set_pinned(&id, true).expect("fixa"));
        assert!(store.set_pinned(&id, false).expect("solta"));

        let report = store
            .compact(Utc::now() + Duration::seconds(5))
            .await
            .expect("compact");
        assert_eq!(report.deleted_entries, 1);
    }

    /// Entrada vencida sai do recall **na hora**, sem esperar a compactacao —
    /// e sai tanto pelo caminho textual quanto pelo KNN.
    #[tokio::test]
    async fn entrada_vencida_some_do_recall_antes_de_ser_apagada() {
        let store = MemoryStore::in_memory_with_vectors().expect("store com vetores");
        assert!(store.knn_enabled());

        let vencida = store
            .remember(entry(
                "s1",
                None,
                "segredo vencido",
                MemoryRole::User,
                Some(vec![0.1, 0.2, 0.3, 0.4]),
            ))
            .await
            .expect("insere");
        store
            .remember(entry(
                "s1",
                None,
                "segredo vigente",
                MemoryRole::User,
                Some(vec![0.1, 0.2, 0.3, 0.41]),
            ))
            .await
            .expect("insere");

        assert!(
            store
                .set_ttl(&vencida, Some(Utc::now() - Duration::hours(1)))
                .expect("define prazo")
        );

        // Caminho KNN: o indice vec0 nao conhece prazo, entao a filtragem tem
        // de estar no fetch dos candidatos.
        let semantico = store
            .recall(RecallQuery {
                tenant_id: None,
                query_text: None,
                query_embedding: Some(vec![0.1, 0.2, 0.3, 0.4]),
                embedding_model: Some("unit-test".to_string()),
                session_id: None,
                continuity_key: None,
                limit: 10,
            })
            .await
            .expect("recall");
        assert!(
            semantico.iter().all(|e| e.id != vencida),
            "entrada vencida voltou pelo KNN"
        );

        // Caminho textual.
        let textual = store
            .recall(RecallQuery {
                tenant_id: None,
                query_text: Some("segredo".to_string()),
                query_embedding: None,
                embedding_model: None,
                session_id: None,
                continuity_key: None,
                limit: 10,
            })
            .await
            .expect("recall");
        assert!(
            textual.iter().all(|e| e.id != vencida),
            "entrada vencida voltou pelo caminho textual"
        );

        // Ainda esta no banco — invisivel nao e apagada.
        let report = store.integrity_report().expect("report");
        assert_eq!(report.entries_total, 2);
        assert_eq!(report.entries_expired, 1);
    }

    /// A compactacao apaga a vencida mesmo que ela seja novissima.
    #[tokio::test]
    async fn compact_apaga_vencida_por_mais_nova_que_seja() {
        let store = MemoryStore::in_memory().expect("store");
        let vencida = store
            .remember(entry("s1", None, "vencida", MemoryRole::User, None))
            .await
            .expect("insere");
        store
            .remember(entry("s1", None, "vigente", MemoryRole::User, None))
            .await
            .expect("insere");
        assert!(
            store
                .set_ttl(&vencida, Some(Utc::now() - Duration::seconds(1)))
                .expect("define prazo")
        );

        // Corte no passado: pela idade, nenhuma das duas sairia.
        let report = store
            .compact(Utc::now() - Duration::days(365))
            .await
            .expect("compact");

        assert_eq!(report.deleted_entries, 1);
        let restantes = store.recent_entries(10).expect("lista");
        assert_eq!(restantes.len(), 1);
        assert_eq!(restantes[0].content, "vigente");
    }

    /// Prazo futuro nao esconde nada, e limpar o prazo e reversivel.
    #[tokio::test]
    async fn prazo_futuro_nao_esconde_e_limpar_reverte() {
        let store = MemoryStore::in_memory().expect("store");
        let id = store
            .remember(entry("s1", None, "viva", MemoryRole::User, None))
            .await
            .expect("insere");

        assert!(
            store
                .set_ttl(&id, Some(Utc::now() + Duration::days(30)))
                .expect("prazo futuro")
        );
        assert_eq!(store.recent_entries(10).expect("lista").len(), 1);
        assert_eq!(store.integrity_report().expect("report").entries_expired, 0);

        assert!(
            store
                .set_ttl(&id, Some(Utc::now() - Duration::seconds(1)))
                .expect("prazo passado")
        );
        assert_eq!(store.integrity_report().expect("report").entries_expired, 1);

        assert!(store.set_ttl(&id, None).expect("limpa o prazo"));
        assert_eq!(store.integrity_report().expect("report").entries_expired, 0);
    }

    /// Entrada vencida nao entra na fila de reindexacao: gerar vetor para o
    /// que a proxima compactacao apaga e pagar chamada de provider por
    /// trabalho que vai para o lixo.
    #[tokio::test]
    async fn vencida_fica_fora_da_fila_de_reindexacao() {
        let store = MemoryStore::in_memory().expect("store");
        let vencida = store
            .remember(entry("s1", None, "vencida", MemoryRole::User, None))
            .await
            .expect("insere");
        store
            .remember(entry("s1", None, "vigente", MemoryRole::User, None))
            .await
            .expect("insere");
        assert!(
            store
                .set_ttl(&vencida, Some(Utc::now() - Duration::seconds(1)))
                .expect("prazo")
        );

        let fila = store.entries_missing_embeddings(10).expect("fila");
        assert_eq!(fila.len(), 1, "a vencida entrou na fila: {fila:?}");
        assert_eq!(fila[0].content, "vigente");
    }

    #[tokio::test]
    async fn set_ttl_e_set_pinned_devolvem_false_para_id_inexistente() {
        let store = MemoryStore::in_memory().expect("store");
        assert!(!store.set_ttl("nao-existe", None).expect("set_ttl"));
        assert!(!store.set_pinned("nao-existe", true).expect("set_pinned"));
    }

    /// Um banco criado por uma versao anterior — sem `ttl_expires_at` nem
    /// `pinned_at` — tem de abrir e ganhar as colunas.
    ///
    /// Regressao real, achada num teste fim a fim e invisivel para os testes
    /// em memoria (que sempre criam banco novo, onde o `CREATE TABLE` ja traz
    /// as colunas): o schema V1 cria um indice sobre `ttl_expires_at`, e num
    /// banco existente o `CREATE TABLE IF NOT EXISTS` e no-op. Com os `ALTER`
    /// rodando depois do batch, toda instalacao existente quebrava na
    /// atualizacao com `no such column: ttl_expires_at`.
    #[test]
    fn banco_antigo_ganha_as_colunas_de_retencao_na_abertura() {
        let dir = std::env::temp_dir().join(format!(
            "garraia-memory-upgrade-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("relogio")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("cria dir");
        let db = dir.join("memory.db");

        // Schema de antes do #959, escrito a mao: e a unica forma de afirmar
        // o caminho de atualizacao sem guardar um arquivo binario no repo.
        {
            let conn = rusqlite::Connection::open(&db).expect("cria banco antigo");
            conn.execute_batch(
                "CREATE TABLE memory_entries (
                    id TEXT PRIMARY KEY,
                    tenant_id TEXT NOT NULL DEFAULT 'default',
                    session_id TEXT NOT NULL,
                    channel_id TEXT,
                    user_id TEXT,
                    continuity_key TEXT,
                    role TEXT NOT NULL,
                    content TEXT NOT NULL,
                    embedding BLOB,
                    embedding_model TEXT,
                    embedding_dimensions INTEGER,
                    metadata TEXT DEFAULT '{}',
                    created_at TEXT NOT NULL DEFAULT (datetime('now'))
                );",
            )
            .expect("schema antigo");
            conn.execute(
                "INSERT INTO memory_entries (id, tenant_id, session_id, role, content, created_at)
                 VALUES ('legado', 'default', 's1', 'user', 'memoria de antes', datetime('now'))",
                [],
            )
            .expect("linha legada");
        }

        let store = MemoryStore::open(&db).expect("abrir banco antigo nao pode falhar");

        // A linha antiga sobreviveu, e as colunas novas existem e sao nulas.
        let entradas = store.recent_entries(10).expect("lista");
        assert_eq!(entradas.len(), 1);
        assert_eq!(entradas[0].content, "memoria de antes");
        assert!(entradas[0].ttl_expires_at.is_none());
        assert!(entradas[0].pinned_at.is_none());

        // E as colunas sao utilizaveis.
        assert!(store.set_pinned("legado", true).expect("fixa"));
        assert_eq!(store.integrity_report().expect("report").entries_pinned, 1);

        // Reabrir de novo tambem tem de funcionar (o ALTER ja rodou).
        drop(store);
        let store = MemoryStore::open(&db).expect("segunda abertura");
        assert_eq!(store.integrity_report().expect("report").entries_total, 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Diretorio temporario proprio, apagado no fim do teste.
    fn dir_temporario(rotulo: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "garraia-memory-{rotulo}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("relogio")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("cria dir");
        dir
    }

    /// O backup tem de trazer o indice vetorial junto — e o que separa uma
    /// copia utilizavel de um arquivo que reabre vazio de vetor.
    #[tokio::test]
    async fn backup_preserva_entradas_e_indice_vetorial() {
        let dir = dir_temporario("backup");
        let origem = dir.join("memory.db");
        let destino = dir.join("backup.db");

        {
            let store = MemoryStore::open(&origem).expect("abre");
            assert!(
                store.knn_enabled(),
                "sem sqlite-vec o teste nao afirma o que existe para afirmar"
            );
            store
                .remember(entry(
                    "s1",
                    None,
                    "com vetor",
                    MemoryRole::User,
                    Some(vec![0.1, 0.2, 0.3, 0.4]),
                ))
                .await
                .expect("insere");
            store
                .remember(entry("s1", None, "sem vetor", MemoryRole::User, None))
                .await
                .expect("insere");

            let bytes = store.backup_to(&destino).expect("backup");
            assert!(bytes > 0, "backup vazio");
        }

        let copia = MemoryStore::open(&destino).expect("a copia tem de abrir");
        let report = copia.integrity_report().expect("report");
        assert_eq!(report.entries_total, 2);
        assert_eq!(report.entries_with_embedding, 1);
        assert_eq!(report.map_rows, 1, "o vec_id_map nao veio no backup");
        assert_eq!(
            report.vec_rows_by_table,
            vec![("vec_embeddings_4".to_string(), 1)],
            "a tabela vec0 nao veio no backup"
        );
        assert!(report.orphan_map_entries.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Um backup que sobrescreve outro em silencio e um backup a menos.
    #[tokio::test]
    async fn backup_recusa_sobrescrever_arquivo_existente() {
        let dir = dir_temporario("backup-existente");
        let origem = dir.join("memory.db");
        let destino = dir.join("ja-existe.db");
        std::fs::write(&destino, b"nao me apague").expect("escreve");

        let store = MemoryStore::open(&origem).expect("abre");
        let erro = store.backup_to(&destino).expect_err("deveria recusar");
        assert!(
            format!("{erro}").contains("already exists"),
            "mensagem inesperada: {erro}"
        );
        assert_eq!(
            std::fs::read(&destino).expect("le"),
            b"nao me apague",
            "o arquivo existente foi tocado"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A copia e independente: escrever na origem depois do backup nao mexe
    /// nele.
    #[tokio::test]
    async fn backup_e_um_retrato_do_momento() {
        let dir = dir_temporario("backup-retrato");
        let origem = dir.join("memory.db");
        let destino = dir.join("backup.db");

        let store = MemoryStore::open(&origem).expect("abre");
        store
            .remember(entry("s1", None, "antes", MemoryRole::User, None))
            .await
            .expect("insere");
        store.backup_to(&destino).expect("backup");

        store
            .remember(entry("s1", None, "depois", MemoryRole::User, None))
            .await
            .expect("insere");

        assert_eq!(store.integrity_report().expect("report").entries_total, 2);
        let copia = MemoryStore::open(&destino).expect("abre copia");
        assert_eq!(copia.integrity_report().expect("report").entries_total, 1);

        let _ = std::fs::remove_dir_all(&dir);
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
        assert!(
            store.knn_enabled(),
            "o sqlite-vec e compilado no binario (rusqlite bundled): sem ele \
             este teste de isolamento nao exercitaria o caminho KNN e passaria \
             em vazio"
        );

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
        assert!(
            store.knn_enabled(),
            "sem sqlite-vec este teste nao exercitaria o caminho KNN"
        );

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
        assert!(
            store.knn_enabled(),
            "sem sqlite-vec nao ha indice para ficar orfao — teste em vazio"
        );

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
        assert!(
            store.knn_enabled(),
            "sem sqlite-vec nao ha indice para o compact limpar — teste em vazio"
        );

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

    /// M2 da auditoria: entrada legada (vetor gravado antes do #954, sem
    /// modelo registrado) perde o eixo semântico em silêncio quando o recall
    /// chega com modelo. O relatório conta essa fila para o operador.
    #[tokio::test]
    async fn integrity_report_counts_legacy_entries_without_model() {
        let store = MemoryStore::in_memory().expect("failed to create in-memory memory store");

        let mut legada = entry(
            "session-a",
            None,
            "vetor sem modelo",
            MemoryRole::User,
            Some(vec![1.0, 0.0, 0.0]),
        );
        legada.embedding_model = None;
        store.remember(legada).await.expect("remember");

        store
            .remember(entry(
                "session-a",
                None,
                "vetor com modelo",
                MemoryRole::User,
                Some(vec![0.0, 1.0, 0.0]),
            ))
            .await
            .expect("remember");

        store
            .remember(entry(
                "session-a",
                None,
                "sem vetor nenhum",
                MemoryRole::User,
                None,
            ))
            .await
            .expect("remember");

        let report = store.integrity_report().expect("report");
        assert_eq!(report.entries_total, 3);
        assert_eq!(report.entries_with_embedding, 2);
        assert_eq!(report.entries_without_embedding, 1);
        assert_eq!(
            report.entries_missing_model, 1,
            "só a entrada com vetor E sem modelo conta como legado"
        );
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
