//! O snapshot do registro de confiabilidade (#1438): o contrato das
//! superficies. As chaves sao em ingles, como o resto da API do gateway
//! (`GET /admin/api/reliability`); o registro em si, e a regra do que entra
//! aqui, estao em [`super`].
//!
//! Nada nestes tipos carrega texto livre de quem chama: `tool` e nome de
//! ferramenta registrada ou balde constante, `server` e nome de servidor MCP
//! da config, `channel` e `resource` sao `&'static str` de conjunto fechado.

use std::collections::BTreeMap;

use serde::Serialize;

/// Tudo o que o registro sabe, pronto para serializar.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Snapshot {
    /// Por ferramenta, em ordem lexica do nome.
    pub tools: Vec<ToolSnapshot>,
    /// A soma de todas as ferramentas.
    pub tools_total: ToolStats,
    pub mcp: Vec<McpSnapshot>,
    pub channels: Vec<ChannelSnapshot>,
    pub storage: Vec<StorageSnapshot>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ToolSnapshot {
    pub tool: String,
    #[serde(flatten)]
    pub stats: ToolStats,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ToolStats {
    /// Toda chamada que passou pelo despacho (soma de `outcomes`).
    pub calls: u64,
    /// Cada desfecho, mesmo zerado.
    pub outcomes: BTreeMap<&'static str, u64>,
    /// Quantas vezes uma saida desta ferramenta ABRIU o breaker de uma sessao.
    pub breaker_opened: u64,
    /// Sobre as que executaram com veredito (sucesso + erro + timeout).
    pub success_rate: Option<f64>,
    pub error_rate: Option<f64>,
    pub timeout_rate: Option<f64>,
    pub latency_ms: LatencySnapshot,
    pub last_failure_ago_s: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct LatencySnapshot {
    pub count: u64,
    pub avg: Option<u64>,
    pub p50: Option<u64>,
    pub p95: Option<u64>,
    pub max: Option<u64>,
    /// Contagem POR balde (nao acumulada); `le: null` e o `+Inf`.
    pub buckets: Vec<BucketSnapshot>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct BucketSnapshot {
    pub le: Option<u64>,
    pub count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct McpSnapshot {
    pub server: String,
    /// Transicoes vivo -> morto vistas pelo health monitor.
    pub drops: u64,
    pub reconnect_attempts: u64,
    pub reconnects_ok: u64,
    pub reconnects_failed: u64,
    pub last_reconnect_ago_s: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ChannelSnapshot {
    pub channel: &'static str,
    pub connected: bool,
    pub connections: u64,
    /// Conexoes depois da primeira.
    pub reconnects: u64,
    /// Conectado -> caiu.
    pub drops: u64,
    /// Tentativas que terminaram sem chegar a conectar.
    pub failed_attempts: u64,
    pub last_change_ago_s: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct StorageSnapshot {
    pub resource: &'static str,
    pub current: Option<u64>,
    /// `current` menos a amostra mais antiga da janela.
    pub delta: Option<i64>,
    /// Da amostra mais antiga ate `agora`.
    pub window_s: u64,
    /// Da mais antiga para a mais nova.
    pub samples: Vec<SampleSnapshot>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SampleSnapshot {
    pub ago_s: u64,
    pub value: u64,
}
