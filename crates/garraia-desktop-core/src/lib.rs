//! Núcleo do GarraIA Desktop Control Center — **sem Tauri**.
//!
//! # Por que esta crate existe
//!
//! `garraia-desktop` (a casca Tauri) está excluída de *todos* os gates
//! obrigatórios de CI — clippy, build e test rodam com
//! `--exclude garraia-desktop`, porque o `build.rs` do Tauri exige GTK/webkit
//! que os runners não têm. Consequência registrada na issue #1181: a crate tem
//! **zero cobertura automatizada**. Construir o control center inteiro lá
//! dentro seria escrever milhares de linhas que ninguém sabe quando quebram.
//!
//! Esta crate é o outro lado dessa fronteira: toda a lógica que **não precisa**
//! de janela mora aqui, entra nos gates normais e tem teste. A casca Tauri fica
//! fina de propósito — janelas, bandeja, hotkeys, autostart, updater.
//!
//! Decisão em [`docs/adr/0021-garraia-desktop-control-center.md`], opção D.
//!
//! # Módulos
//!
//! - [`state`] — estado ligado/desligado dos módulos do desktop. Puro: sem
//!   relógio, sem I/O, sem thread. Mesmo padrão do `spinner.rs` da CLI, onde o
//!   estado só avança quando alguém o manda avançar.
//! - [`detect`] — detecção de agentes externos. **Leitura, nunca execução**:
//!   varre `PATH` e configs conhecidas e jamais roda um binário para descobrir
//!   quem ele é.
//! - [`supervise`] — primitivos de supervisão de processo (launch / restart /
//!   kill / morte do filho junto com o pai), extraídos do `gateway.rs` da casca
//!   Tauri sem a dependência do Tauri.
//!
//! # O que esta crate não faz
//!
//! Não abre janela, não desenha, não fala com o gateway por HTTP e não
//! reimplementa os adapters do AgentDeck. O conteúdo das abas é servido pelo
//! gateway que o desktop já roda como sidecar (`/api/*`), e a aba de agentes é
//! **cliente** do AgentDeck.
//!
//! [`docs/adr/0021-garraia-desktop-control-center.md`]: https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/0021-garraia-desktop-control-center.md

pub mod detect;
pub mod state;
pub mod supervise;

pub use detect::{AgentKind, Confidence, DetectedAgent, Detector, Evidence, Filesystem, RealFs};
pub use state::{Desired, ModuleId, ModuleState, Modules, Power, PowerEvent};
pub use supervise::{
    CommandSpawner, ExitStatus, ProcessHandle, ProcessSpec, RestartPolicy, Spawner, SuperviseError,
    Supervisor,
};
