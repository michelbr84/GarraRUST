pub mod error;
pub mod handoff;
pub mod message;
pub mod safety_gate;
#[cfg(feature = "ssrf")]
pub mod ssrf;
pub mod types;

pub use error::{Error, Result};
pub use handoff::{
    HandoffAction, HandoffActionKind, HandoffError, HandoffState, RedactedString, redact,
};
pub use message::{Message, MessageContent, MessageDirection};
pub use safety_gate::{SafetyDenied, is_risky, safety_gate};
#[cfg(feature = "ssrf")]
pub use ssrf::{
    IpScope, SsrfCategory, SsrfRejection, UrlPolicy, VettedUrl, is_blocked_ip, pinned_client,
    pinned_client_for, vet_url,
};
pub use types::{AgentResponse, ChannelId, RequestContext, SessionId, UserId};
