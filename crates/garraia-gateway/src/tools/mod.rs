//! Agent tools that need gateway state.
//!
//! Most tools live in `garraia-agents`, which depends only on
//! `garraia-common` and `garraia-db`. A tool that has to reach a channel
//! adapter or the live `AppState` cannot: that would mean an
//! `garraia-agents → garraia-channels` edge. Such tools implement
//! `garraia_agents::Tool` here instead and close over `Arc<AppState>`.

pub mod channel_send_tool;

pub use channel_send_tool::TelegramSendTool;
