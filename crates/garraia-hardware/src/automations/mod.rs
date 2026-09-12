//! O motor de automações (#1128) — trigger → condição → ação, pela mesma
//! policy do runtime (nada de bypass).
//!
//! Slice atual: gatilhos `state_changed` (barramento de eventos) e `cron`
//! (croner), condições avaliadas por [`expr::Expr`], ações `device_execute`
//! pelo [`crate::registry::DeviceRegistry`] com o [`crate::gate::HardwareGate`]
//! **sem canal de confirmação** (automação roda desacompanhada — R3/R4/R5
//! negadas por natureza, R0/R1/R2 pelo teto declarado no config). Ações
//! `send_message` e `tool call` entram em slice futuro, com a maquinaria de
//! canal do gateway.

pub mod cron;
pub mod engine;
pub mod expr;
pub mod spec;
pub mod store;

pub use engine::{AutomationEngine, EngineConfig, TetoRisco};
pub use expr::Expr;
pub use spec::{
    ActionSpec, AutomationSpec, ConditionSpec, RateLimitSpec, TriggerSpec, carregar_dir,
};
pub use store::{AutomationStore, Execucao, ResultadoExecucao};
