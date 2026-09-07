//! Runtime helpers do GarraIA.
//!
//! # O `mode.rs` daqui foi removido (#985)
//!
//! Este crate tinha um `AgentMode` / `ModeProfile` / `ToolPolicy` proprio, com
//! ~910 linhas, que **nunca foi instanciado**: o unico consumidor do crate e o
//! `garraia-gateway`, e ele importa apenas o [`RuntimeSettings`]. Os ~250 usos
//! de `ModeEngine::new()` que o arquivo tinha eram todos testes dele mesmo.
//!
//! Ele nao era so redundante, era **divergente**: o `ToolPolicy` de la tinha
//! `read_only: Vec<String>` e `required: Option<String>`, enquanto o de
//! `garraia_agents::modes` tem `required: Vec<String>` e `whitelist_mode`. A
//! #985 descreveu isso como risco de "validar um modo e executar outro" — o
//! risco real era outro e menor, porque codigo inalcancavel nao executa nada:
//! o custo era manutencao dupla e leitura enganosa. Um `read_only` que so
//! existe na copia morta chegou a ser citado como implementavel na #988.
//!
//! **O canonico e `garraia_agents::modes`**: e o que o `/mode`, o
//! `POST /api/mode/select` e o `GET /api/modes` usam de verdade. O roteamento
//! automatico e o `garraia_agents::auto_router` — que morava em
//! `garraia_gateway` e mudou de crate na #979, para o runtime tambem poder
//! resolver `/mode auto`. O `AutoRouter` de `garraia_agents::agent_mode`, que
//! esta nota citava como o concorrente sem chamadores, foi removido junto: o
//! modulo inteiro era codigo morto. O vocabulario pt-BR dele, esse sim, foi
//! levado para o roteador vivo.

pub mod executor;
pub mod meta_controller;
pub mod state;

pub use executor::{run_turn, RuntimeSettings};
pub use meta_controller::MetaController;
pub use state::TaskState;
