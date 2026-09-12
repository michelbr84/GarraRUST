//! Erros da camada de hardware.
//!
//! Pequeno de propósito: a integração (`garraia-agents`) converte estes
//! erros na mensagem que o modelo enxerga — um erro de hardware nunca é
//! um panic nem um `unwrap`, e sempre diz **o que** falhou (dispositivo
//! e capability) para o modelo poder decidir o próximo passo.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum HardwareError {
    /// O dispositivo não está registrado.
    #[error("dispositivo '{0}' não registrado")]
    DispositivoNaoRegistrado(String),

    /// O dispositivo existe, mas não expõe esta capability.
    #[error("dispositivo '{dispositivo}' não expõe a capability '{capability}'")]
    CapabilityDesconhecida {
        dispositivo: String,
        capability: String,
    },

    /// O adapter declarou uma capability que viola as invariantes (ex.:
    /// read_only com risco > R0). Recusada na entrada.
    #[error("capability '{nome}' inválida ({classe}): {motivo}")]
    CapabilityInvalida {
        nome: String,
        classe: String,
        motivo: String,
    },

    /// O gate negou a execução — o texto cita o risco, para o agente (e o
    /// audit log) saberem por quê.
    #[error("execução negada pelo modelo de risco ({classe}): {motivo}")]
    ExecucaoNegada { classe: String, motivo: String },

    /// O adapter devolveu um erro de transporte/protocolo.
    #[error("falha no dispositivo '{dispositivo}': {fonte}")]
    Adapter { dispositivo: String, fonte: String },

    /// Erro do transporte MQTT no manager (#1126) — sem dispositivo
    /// associado (config malformada, assinatura, event loop).
    #[error("erro MQTT: {0}")]
    Mqtt(String),

    /// Erro do transporte Home Assistant no manager (#1127) — sem
    /// dispositivo associado (URL malformada, descoberta, WebSocket).
    #[error("erro do Home Assistant: {0}")]
    HomeAssistant(String),

    /// Erro do SQLite (o store de presença).
    #[error("erro de SQLite no estado de hardware: {0}")]
    Sqlite(#[from] rusqlite::Error),
}
