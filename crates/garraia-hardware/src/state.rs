//! Estado online/offline dos dispositivos — SQLite, padrão do repo.
//!
//! O registry sabe **quem** está registrado; este store sabe **como o
//! dispositivo estava** (online/offline, `last_seen`). rusqlite bundled,
//! conexão sync sob `tokio::sync::Mutex` — o mesmo arranjo do
//! `SessionStore` (`garraia-db`), porque a operação é rápida e não deve
//! segurar `.await` sob lock.
//!
//! Quem marca: os adapters, a cada contato bem-sucedido (`marcar(id, true)`)
//! e a cada falha de contato (`marcar(id, false)`). A leitura alimenta a
//! descoberta (`device_list` anota o estado quando o store está presente).

use crate::Result;
use rusqlite::OptionalExtension;
use serde::Serialize;
use std::path::Path;
use tokio::sync::Mutex;

/// Presença de um dispositivo, como o store a guarda.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DevicePresence {
    /// Id do dispositivo.
    pub device_id: String,
    /// Visto online na última marcação?
    pub online: bool,
    /// Último contato, em segundos desde o epoch (UTC).
    pub last_seen: u64,
}

/// A loja de presença, atrás de um mutex tokio (padrão `SessionStore`).
pub struct DeviceStateStore {
    conn: Mutex<rusqlite::Connection>,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS hardware_devices (
    device_id TEXT PRIMARY KEY,
    online    INTEGER NOT NULL DEFAULT 1,
    last_seen INTEGER NOT NULL
);
";

impl DeviceStateStore {
    /// Abre (e migra) o store num arquivo SQLite.
    pub fn abrir_em(path: &Path) -> Result<Self> {
        let conn = rusqlite::Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Store in-memory — testes e o CLI sem diretório de dados.
    pub fn em_memoria() -> Result<Self> {
        let conn = rusqlite::Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Marca o dispositivo visto (online ou offline) agora.
    pub async fn marcar(&self, device_id: &str, online: bool) -> Result<()> {
        let conn = self.conn.lock().await;
        // Epoch secs gerado pelo SQLite (UTC) — sem relógio no Rust e sem
        // relógio do sistema anterior ao epoch para tratar.
        conn.execute(
            "INSERT INTO hardware_devices (device_id, online, last_seen)
             VALUES (?1, ?2, CAST(strftime('%s', 'now') AS INTEGER))
             ON CONFLICT(device_id) DO UPDATE
             SET online = excluded.online, last_seen = excluded.last_seen",
            rusqlite::params![device_id, online],
        )?;
        Ok(())
    }

    /// A presença de um dispositivo, se já marcado.
    pub async fn estado(&self, device_id: &str) -> Result<Option<DevicePresence>> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            "SELECT device_id, online, last_seen
             FROM hardware_devices WHERE device_id = ?1",
        )?;
        let found = stmt
            .query_row(rusqlite::params![device_id], |row| {
                Ok(DevicePresence {
                    device_id: row.get(0)?,
                    online: row.get::<_, i64>(1)? != 0,
                    last_seen: row.get::<_, i64>(2)?.max(0) as u64,
                })
            })
            .optional()?;
        Ok(found)
    }

    /// Todas as presenças, ordenadas por id (determinístico).
    pub async fn todos(&self) -> Result<Vec<DevicePresence>> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            "SELECT device_id, online, last_seen
             FROM hardware_devices ORDER BY device_id",
        )?;
        let linhas = stmt
            .query_map([], |row| {
                Ok(DevicePresence {
                    device_id: row.get(0)?,
                    online: row.get::<_, i64>(1)? != 0,
                    last_seen: row.get::<_, i64>(2)?.max(0) as u64,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(linhas)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Marcações consecutivas atualizam em upsert — nunca duplicam linha.
    #[tokio::test]
    async fn marca_e_atualiza_presenca() {
        let store = DeviceStateStore::em_memoria().expect("abre em memória");
        store
            .marcar("sensor-sala", true)
            .await
            .expect("marca online");
        let p = store
            .estado("sensor-sala")
            .await
            .expect("lê")
            .expect("existe");
        assert_eq!(p.device_id, "sensor-sala");
        assert!(p.online);

        store
            .marcar("sensor-sala", false)
            .await
            .expect("marca offline");
        let p = store
            .estado("sensor-sala")
            .await
            .expect("lê")
            .expect("existe");
        assert!(!p.online, "segunda marcação substitui");
    }

    /// Dispositivo nunca marcado é `None`, não erro.
    #[tokio::test]
    async fn desconhecido_volta_none() {
        let store = DeviceStateStore::em_memoria().expect("abre");
        let p = store.estado("nada").await.expect("lê");
        assert!(p.is_none());
    }

    /// `last_seen` é unix epoch em segundos — marcações posteriores têm
    /// timestamp ≥.
    #[tokio::test]
    async fn last_seen_e_epoch_e_cresce() {
        let store = DeviceStateStore::em_memoria().expect("abre");
        store.marcar("a", true).await.expect("marca");
        store.marcar("b", true).await.expect("marca");
        let todos = store.todos().await.expect("lista");
        assert_eq!(todos.len(), 2);
        let ids: Vec<&str> = todos.iter().map(|p| p.device_id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b"], "ordenado por id");
        let agora = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("relógio do sistema")
            .as_secs();
        for p in &todos {
            assert!(p.last_seen <= agora, "last_seen não está no futuro");
        }
    }

    /// O store abre num arquivo real (o modo de produção) e persiste
    /// entre aberturas.
    #[tokio::test]
    async fn arquivo_persiste_entre_aberturas() {
        let dir = tempfile_dir();
        let caminho = dir.join("hardware.db");
        {
            let store = DeviceStateStore::abrir_em(&caminho).expect("abre arquivo");
            store.marcar("lampada-sala", true).await.expect("marca");
        }
        let store = DeviceStateStore::abrir_em(&caminho).expect("reabre");
        let p = store
            .estado("lampada-sala")
            .await
            .expect("lê")
            .expect("persistiu");
        assert!(p.online);
        let _ = std::fs::remove_file(&caminho);
    }

    fn tempfile_dir() -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("garraia-hardware-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }
}
