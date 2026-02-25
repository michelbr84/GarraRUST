use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Per-request context carrying tenant isolation and tracing metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestContext {
    /// Identifies the tenant that owns this request.
    pub tenant_id: String,
    /// Unique identifier for tracing this request across logs and spans.
    pub request_id: String,
}

impl RequestContext {
    /// Build a new context for the given tenant, generating a fresh request_id.
    pub fn new(tenant_id: impl Into<String>) -> Self {
        Self {
            tenant_id: tenant_id.into(),
            request_id: Uuid::new_v4().to_string(),
        }
    }

    /// Default context for single-tenant / legacy deployments.
    pub fn default_tenant() -> Self {
        Self::new("default")
    }
}

impl Default for RequestContext {
    fn default() -> Self {
        Self::default_tenant()
    }
}

#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionId(String);

#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct UserId(String);

#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChannelId(String);

macro_rules! impl_id_type {
    ($t:ty) => {
        impl $t {
            pub fn new() -> Self {
                Self(Uuid::new_v4().to_string())
            }

            pub fn from_string(s: impl Into<String>) -> Self {
                Self(s.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $t {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl Default for $t {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

impl_id_type!(SessionId);
impl_id_type!(UserId);
impl_id_type!(ChannelId);

/// Result of agent execution - supports multi-turn workflows
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentResponse {
    /// Agent completed with final response
    Completed(String),
    /// Turn limit reached, caller should continue in next turn
    ContinueNextTurn,
    /// Task limit reached, cannot continue
    TaskLimitReached(String),
    /// Loop detected
    LoopDetected(String),
    /// Tool timeout
    ToolTimeout(String),
    /// Other error
    Error(String),
}

impl AgentResponse {
    /// Returns the response text if completed, None otherwise
    pub fn text(&self) -> Option<&str> {
        match self {
            AgentResponse::Completed(text) => Some(text),
            _ => None,
        }
    }
    
    /// Returns true if execution should continue in next turn
    pub fn should_continue(&self) -> bool {
        matches!(self, AgentResponse::ContinueNextTurn)
    }
    
    /// Returns true if execution is complete
    pub fn is_complete(&self) -> bool {
        matches!(self, AgentResponse::Completed(_))
    }
}

impl From<String> for AgentResponse {
    fn from(s: String) -> Self {
        AgentResponse::Completed(s)
    }
}

impl From<&str> for AgentResponse {
    fn from(s: &str) -> Self {
        AgentResponse::Completed(s.to_string())
    }
}
