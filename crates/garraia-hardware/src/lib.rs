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
//! `device_read`, `device_execute`, em `garraia-agents`) listam nada. Os
//! transportes entram como features, todas OFF por default (mesmo padrão do
//! `storage-s3`: quem não usa um transporte não paga a árvore de deps dele),
//! e cada uma traz o próprio risco avaliado no PR correspondente:
//!
//! | Feature | Módulo | Transporte | Issue |
//! |---|---|---|---|
//! | `mqtt` | [`adapter_mqtt`] | broker MQTT, convenção `garra/devices/...` | #1126 |
//! | `home-assistant` | [`adapter_homeassistant`] | REST + WebSocket contra o hub | #1127 |
//! | `hardware-serial` | [`adapter_serial`] | JSONL por USB/serial (Arduino/ESP32) | #1130 |
//! | `hardware-gpio` | [`adapter_gpio`] | pinos do Raspberry Pi via `/dev/gpiomem` | #1130 |
//!
//! Os dois primeiros pegam o risk class do que o hub/dispositivo declara
//! (MQTT) ou de uma tabela por domínio (Home Assistant). Os dois últimos
//! compartilham a tabela **fechada** de [`perifericos`] — `digital_read` e
//! `analog_read` em R0, `digital_write` e `pwm` em R2 — porque uma placa
//! plugada num cabo USB não passa por ACL nenhuma e não pode ser a fonte da
//! própria classificação de risco.
//!
//! # O motor de automações (#1128)
//!
//! O barramento [`HardwareEventBus`] recebe as mudanças de estado que os
//! adapters veem; o [`automations::AutomationEngine`] assina, casa com as
//! regras declarativas do usuário (TOML/JSON, feature `automations`) e
//! executa pela **mesma policy do runtime** — o gate sem canal de confirmação
//! (automação roda desacompanhada) com teto de risco declarado no config.
//! Nada de bypass: o que o usuário não pediria ao agente diretamente, a
//! automação também não faz.
//!
//! O estado online/offline dos dispositivos ([`DeviceStateStore`]) segue o
//! padrão do repo: SQLite via rusqlite bundled, acesso sync sob mutex.

pub mod capability;
pub mod device;
pub mod error;
pub mod events;
pub mod gate;
pub mod registry;
pub mod risk;
pub mod schema;
pub mod state;

#[cfg(feature = "automations")]
pub mod automations;

#[cfg(feature = "mock-device")]
pub mod mock;

#[cfg(feature = "mqtt")]
pub mod adapter_mqtt;

#[cfg(feature = "home-assistant")]
pub mod adapter_homeassistant;

/// A tabela fechada de capabilities de placa, compartilhada pelos adapters
/// serial e GPIO (#1130).
#[cfg(any(feature = "hardware-serial", feature = "hardware-gpio"))]
pub mod perifericos;

#[cfg(feature = "hardware-serial")]
pub mod adapter_serial;

#[cfg(feature = "hardware-gpio")]
pub mod adapter_gpio;

pub use capability::Capability;
pub use device::{Device, DeviceSummary};
pub use error::HardwareError;
pub use events::{EstadoObservado, HardwareEvent, HardwareEventBus, StateChanged};
pub use gate::HardwareGate;
pub use registry::DeviceRegistry;
pub use risk::{ExecutionDecision, RiskClass};
pub use state::DeviceStateStore;

#[cfg(feature = "automations")]
pub use automations::{
    AutomationEngine, AutomationSpec, AutomationStore, carregar_dir as carregar_automacoes,
};

#[cfg(feature = "mock-device")]
pub use mock::MockDevice;

#[cfg(feature = "mqtt")]
pub use adapter_mqtt::{DeviceManifest, MqttAdapterConfig, MqttAdapterManager, MqttDevice};

#[cfg(feature = "home-assistant")]
pub use adapter_homeassistant::{HaAdapterConfig, HaAdapterManager, HaDevice};

#[cfg(feature = "hardware-serial")]
pub use adapter_serial::{
    ManifestoSerial, SerialAdapterConfig, SerialAdapterManager, SerialDevice, adotar_stream,
};

#[cfg(feature = "hardware-gpio")]
pub use adapter_gpio::{GpioAdapterConfig, GpioDevice, PlanoPinos};

/// Resultado das operações de hardware.
pub type Result<T> = std::result::Result<T, HardwareError>;
