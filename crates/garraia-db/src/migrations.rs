/// Migration system for tracking and applying database schema changes.
///
/// Each migration has a version number and a SQL statement.
/// Migrations are applied in order and tracked in a `_migrations` table.
pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub sql: &'static str,
}

pub const MEMORY_SCHEMA_V1_SQL: &str = "
CREATE TABLE IF NOT EXISTS memory_entries (
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
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    ttl_expires_at TEXT,
    pinned_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_memory_session_created_at
    ON memory_entries(session_id, created_at);

CREATE INDEX IF NOT EXISTS idx_memory_continuity_created_at
    ON memory_entries(continuity_key, created_at);

CREATE INDEX IF NOT EXISTS idx_memory_role
    ON memory_entries(role, created_at);

CREATE INDEX IF NOT EXISTS idx_memory_tenant
    ON memory_entries(tenant_id);

CREATE INDEX IF NOT EXISTS idx_memory_tenant_session
    ON memory_entries(tenant_id, session_id, created_at);

-- Parcial: so as entradas COM prazo entram. Numa base onde quase nada tem
-- TTL, o indice inteiro caberia em uma pagina.
CREATE INDEX IF NOT EXISTS idx_memory_ttl
    ON memory_entries(ttl_expires_at) WHERE ttl_expires_at IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_memory_pinned
    ON memory_entries(pinned_at) WHERE pinned_at IS NOT NULL;
";

/// Colunas de retencao (#959), adicionadas a bancos que ja existem.
///
/// Forward-only e aditivo, como manda a regra 9 do CLAUDE.md — e o SQLite
/// nao tem `ADD COLUMN IF NOT EXISTS`, entao cada `ALTER` roda solto e o
/// erro de "duplicate column name" e ignorado pelo chamador. Um banco novo
/// ja nasce com as colunas pelo `CREATE TABLE` acima; este bloco existe so
/// para os que nao nasceram.
pub const MEMORY_RETENTION_COLUMNS: [&str; 2] = [
    "ALTER TABLE memory_entries ADD COLUMN ttl_expires_at TEXT;",
    "ALTER TABLE memory_entries ADD COLUMN pinned_at TEXT;",
];

pub const MEMORY_SCHEMA_V1: Migration = Migration {
    version: 1,
    name: "memory_schema_v1",
    sql: MEMORY_SCHEMA_V1_SQL,
};
