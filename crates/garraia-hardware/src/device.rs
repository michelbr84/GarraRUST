//! O trait `Device` — o contrato que todo adapter de hardware implementa.
//!
//! `#[async_trait]` porque o trait vive como `dyn Device` no registry — a
//! mesma exceção documentada no `CLAUDE.md` para
//! `garraia_storage::ObjectStore` (AFIT + `dyn` não combinam em Rust stable).

use crate::Result;
use crate::capability::Capability;
use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;

/// Um dispositivo físico (ou virtual) exposto por um adapter.
#[async_trait]
pub trait Device: Send + Sync {
    /// Identificador estável do dispositivo ("living-room-light").
    fn id(&self) -> &str;

    /// O que este dispositivo sabe fazer — a lista é a descoberta.
    fn capabilities(&self) -> Vec<Capability>;

    /// Lê o estado atual de uma capability ("temperature" → `{"celsius": 23}`).
    async fn read(&self, capability: &str) -> Result<Value>;

    /// Executa a capability com argumentos ("light_on" + `{"on": true}`).
    async fn execute(&self, capability: &str, args: Value) -> Result<Value>;
}

/// O retrato de um dispositivo para a descoberta dinâmica — é isto que a
/// tool `device_list` mostra ao agente:
///
/// ```text
/// LivingRoomLight   ├─ power ├─ brightness └─ color
/// TemperatureSensor └─ temperature
/// ```
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DeviceSummary {
    /// Identificador do dispositivo.
    pub id: String,
    /// Capabilities expostas, na ordem de registro.
    pub capabilities: Vec<Capability>,
}

/// O resumo de um dispositivo, com padrão via trait (útil para adapters).
pub fn resumo(device: &dyn Device) -> DeviceSummary {
    DeviceSummary {
        id: device.id().to_string(),
        capabilities: device.capabilities(),
    }
}
