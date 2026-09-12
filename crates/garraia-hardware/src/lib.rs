//! GarraIA — Hardware: a camada que deixa o agente ver e agir no mundo físico.
//!
//! Fundação do epic [#1124](https://github.com/michelbr84/GarraRUST/issues/1124)
//! (issues [#1125](https://github.com/michelbr84/GarraRUST/issues/1125) +
//! [#1129](https://github.com/michelbr84/GarraRUST/issues/1129)), decidida em
//! [ADR 0020](../../docs/adr/0020-crate-garraia-hardware.md).
//!
//! # O que é o core
//!
//! A crate define **só a abstração** — sem driver, sem transporte:
//!
//! - [`Device`]: o trait que um adaptador (MQTT, Home Assistant, serial —
//!   issues #1126/#1127/#1130) implementa para expor um dispositivo real;
//! - [`Capability`]: o que um dispositivo sabe fazer, **com o risk class
//!   R0–R5 embutido** — o risco nasce com o tipo, não vem depois (#1129);
//! - [`DeviceRegistry`]: os dispositivos registrados em `Arc<dyn Device>`
//!   por adapters/skills no boot, com descoberta dinâmica;
//! - [`HardwareGate`]: a tabela de decisão R0–R5 que reusa o gate já
//!   existente do repo (GAR-187/GAR-497) em vez de criar um sistema paralelo.
//!
//! # O que este crate NÃO faz
//!
//! Nenhum dispositivo físico é conectado aqui. Sem adaptador registrado, o
//! [`DeviceRegistry`] fica vazio e as tools do agente (`device_list`,
//! `device_read`, `device_execute`, em `garraia-agents`) listam nada. I/O
//! físico de verdade só existe a partir de #1126, cada adapter com o próprio
//! risco avaliado no PR correspondente.
//!
//! O estado online/offline dos dispositivos ([`DeviceStateStore`]) segue o
//! padrão do repo: SQLite via rusqlite bundled, acesso sync sob mutex.

pub mod capability;
pub mod device;
pub mod error;
pub mod gate;
pub mod registry;
pub mod risk;
pub mod state;

#[cfg(feature = "mock-device")]
pub mod mock;

pub use capability::Capability;
pub use device::{Device, DeviceSummary};
pub use error::HardwareError;
pub use gate::HardwareGate;
pub use registry::DeviceRegistry;
pub use risk::{ExecutionDecision, RiskClass};
pub use state::DeviceStateStore;

#[cfg(feature = "mock-device")]
pub use mock::MockDevice;

/// Resultado das operações de hardware.
pub type Result<T> = std::result::Result<T, HardwareError>;
