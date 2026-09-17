//! WhatsApp por **dispositivo vinculado** (QR), canal PULL.
//!
//! Este modulo e o irmao de [`crate::whatsapp`], e nao o substituto dele:
//!
//! | | `whatsapp` (Cloud API) | `whatsapp_linked` (este) |
//! |---|---|---|
//! | Conta | WhatsApp Business, aprovada pela Meta | **qualquer numero pessoal** |
//! | Transporte | webhook HTTP de entrada | processo filho Node/Baileys, NDJSON em stdio |
//! | Credencial | `access_token` da Meta, em config | blob de sessao cifrado em disco |
//! | Requisitos | dominio publico com HTTPS | Node 20+ instalado |
//! | Suporte | oficial | **nao oficial — a Meta pode bloquear a conta** |
//!
//! Os dois podem estar ligados ao mesmo tempo e nao compartilham uma linha de
//! codigo. A decisao esta no ADR 0023.
//!
//! # O aviso que nao e opcional
//!
//! Cliente de dispositivo vinculado nao-oficial viola os termos de uso do
//! WhatsApp. A conta **pode ser bloqueada**. A CLI mostra isso em tela cheia e
//! pede confirmacao explicita antes do primeiro QR, e recomenda um numero
//! secundario. Nao ha flag para pular essa tela na primeira vez.
//!
//! # Mapa dos submodulos
//!
//! | Modulo | Responsabilidade | Relogio? | I/O? |
//! |---|---|---|---|
//! | [`protocol`] | tipos serde do NDJSON v1 | nao | nao |
//! | [`state`] | maquina de estados pura | nao (injetado) | nao |
//! | [`qr`] | desenho do QR | nao | nao |
//! | [`session`] | blob cifrado, modos de arquivo | nao | disco |
//! | [`bridge`] | assets, npm, spawn, enquadramento | timeout | processo |
//! | [`runner`] | casa os quatro acima | sim | sim |
//!
//! A separacao nao e estetica: os quatro primeiros sao testaveis sem processo,
//! sem rede e sem `sleep`, e e la que mora quase toda a logica.
//!
//! # Costura para o slice do gateway
//!
//! O canal pull `whatsapp_linked` do gateway precisa de exatamente tres coisas
//! deste modulo, todas ja publicas:
//!
//! 1. [`session::SessionStore`] + [`session::SessionKey`] para abrir o blob;
//! 2. [`bridge::NodeLauncher`] para saber o que lancar;
//! 3. [`runner::InboundSink`], que ele implementa, passado a
//!    [`runner::serve`] — junto com um `mpsc::Receiver<BridgeCommand>` por
//!    onde ele manda `Send`/`Read`/`Typing`.
//!
//! Nada mais deste modulo precisa mudar quando esse slice chegar.

pub mod bridge;
pub mod health;
/// Analise de texto para varredura de fonte (`chamadas_de_log`,
/// `parte_arriscada`, `bindings_contaminados`). Superficie de teste, nao API do
/// canal — e `pub` porque o gateway varre o proprio fonte com as mesmas regras,
/// e duas copias delas foi exatamente o defeito que este modulo corrige.
#[doc(hidden)]
pub mod log_audit;
pub mod protocol;
pub mod qr;
pub mod runner;
pub mod session;
pub mod state;

pub use bridge::{
    BridgeAssets, BridgeConnection, BridgeError, BridgeLauncher, EmbeddedAssets, NodeLauncher,
    NodeRuntime,
};
pub use health::{BridgeView, DiskFacts, LinkHealth, classify};
pub use protocol::{
    BridgeCommand, BridgeEvent, InboundMessage, Jid, PROTOCOL_VERSION, StartMode,
    UNAUTHORIZED_REASON_CODES, session_is_dead,
};
pub use qr::{Style as QrStyle, render as render_qr};
pub use runner::{InboundSink, PairOptions, PairOutcome, PairUi, RunError, pair, pair_with, serve};
pub use session::{
    DEFAULT_ACCOUNT, KeyOrigin, SessionBlob, SessionError, SessionKey, SessionStore,
};
pub use state::{Desired, Effect, Event, Failure, MAX_QR_ATTEMPTS, Machine, Phase, backoff_ms};

/// Chave da secao de config deste canal. Deliberadamente diferente de
/// `whatsapp`: um operador com os dois ligados precisa ver duas entradas.
pub const CONFIG_KEY: &str = "whatsapp_linked";

#[cfg(test)]
mod source_scan;
