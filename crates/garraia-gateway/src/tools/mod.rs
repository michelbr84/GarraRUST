//! Agent tools that need gateway state.
//!
//! Most tools live in `garraia-agents`, which depends only on
//! `garraia-common` and `garraia-db`. A tool that has to reach a channel
//! adapter or the live `AppState` cannot: that would mean an
//! `garraia-agents → garraia-channels` edge. Such tools implement
//! `garraia_agents::Tool` here instead and close over `Arc<AppState>`.

pub mod channel_send_tool;
pub mod channel_send_voice_tool;
pub mod garra_status_tool;

pub use channel_send_tool::TelegramSendTool;
pub use channel_send_voice_tool::TelegramSendVoiceTool;
pub use garra_status_tool::GarraStatusTool;
