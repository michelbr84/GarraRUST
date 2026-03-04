pub mod chat_sync;
pub mod db_trait;
pub mod memory_store;
pub mod migrations;
pub mod session_store;
pub mod sqlite_db;
pub mod vector_store;

#[cfg(feature = "postgres")]
pub mod postgres_db;

pub use chat_sync::{ChatSessionManager, ChatSource, SessionHints, SessionKeyStrategy, SessionResolverConfig};
pub use db_trait::GarraDb;
pub use memory_store::{
    CompactionReport, MemoryEntry, MemoryProvider, MemoryRole, MemoryStore, NewMemoryEntry,
    RecallQuery, SessionContext,
};
pub use session_store::{ScheduledTask, SessionStore};
pub use sqlite_db::SqliteDb;
pub use vector_store::VectorStore;

#[cfg(feature = "postgres")]
pub use postgres_db::PostgresDb;
