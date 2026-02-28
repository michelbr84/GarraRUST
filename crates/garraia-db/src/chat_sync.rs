//! Chat Sync Session Manager
//!
//! This module provides unified session management for multi-channel conversations,
//! enabling VS Code and Telegram to share the same conversation history.

use crate::session_store::SessionStore;
use garraia_common::Result;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info};

/// Source channels supported by Chat Sync
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChatSource {
    /// Telegram messages
    Telegram,
    /// VS Code extension
    VsCode,
    /// Web UI
    Web,
    /// Discord
    Discord,
    /// Slack
    Slack,
    /// WhatsApp
    WhatsApp,
    /// iMessage
    IMessage,
    /// A2A Protocol
    A2A,
    /// Custom API
    Api,
}

impl ChatSource {
    /// Convert to string for database storage
    pub fn as_str(&self) -> &'static str {
        match self {
            ChatSource::Telegram => "telegram",
            ChatSource::VsCode => "vscode",
            ChatSource::Web => "web",
            ChatSource::Discord => "discord",
            ChatSource::Slack => "slack",
            ChatSource::WhatsApp => "whatsapp",
            ChatSource::IMessage => "imessage",
            ChatSource::A2A => "a2a",
            ChatSource::Api => "api",
        }
    }

    /// Parse from string
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "telegram" => Some(ChatSource::Telegram),
            "vscode" => Some(ChatSource::VsCode),
            "web" => Some(ChatSource::Web),
            "discord" => Some(ChatSource::Discord),
            "slack" => Some(ChatSource::Slack),
            "whatsapp" => Some(ChatSource::WhatsApp),
            "imessage" => Some(ChatSource::IMessage),
            "a2a" => Some(ChatSource::A2A),
            "api" => Some(ChatSource::Api),
            _ => None,
        }
    }
}

/// Session resolution hints for creating new sessions
#[derive(Debug, Clone)]
pub struct SessionHints {
    /// The channel/source (e.g., "telegram", "vscode")
    pub source: ChatSource,
    /// External ID from the channel (e.g., Telegram chat_id)
    pub external_id: Option<String>,
    /// User ID if available
    pub user_id: Option<String>,
    /// Additional metadata
    pub metadata: Option<serde_json::Value>,
}

impl SessionHints {
    /// Create hints from Telegram chat_id
    pub fn from_telegram(chat_id: i64, user_id: Option<i64>) -> Self {
        Self {
            source: ChatSource::Telegram,
            external_id: Some(chat_id.to_string()),
            user_id: user_id.map(|u| u.to_string()),
            metadata: None,
        }
    }

    /// Create hints from VS Code workspace/user
    pub fn from_vscode(workspace_id: &str, user_id: Option<&str>) -> Self {
        Self {
            source: ChatSource::VsCode,
            external_id: Some(workspace_id.to_string()),
            user_id: user_id.map(|u| u.to_string()),
            metadata: None,
        }
    }

    /// Create hints from web session
    pub fn from_web(session_id: &str) -> Self {
        Self {
            source: ChatSource::Web,
            external_id: Some(session_id.to_string()),
            user_id: None,
            metadata: None,
        }
    }
}

/// Session Manager for Chat Sync
///
/// This manager handles:
/// - Resolving external IDs (Telegram chat_id, VS Code workspace) to session_ids
/// - Creating new sessions when needed
/// - Managing session keys for multi-channel access
pub struct ChatSessionManager {
    store: Arc<Mutex<SessionStore>>,
}

impl ChatSessionManager {
    /// Create a new ChatSessionManager
    pub fn new(store: Arc<Mutex<SessionStore>>) -> Self {
        Self { store }
    }

    /// Resolve a session ID from hints
    ///
    /// This function:
    /// 1. Checks if there's an existing session key for the external ID
    /// 2. If found, returns the existing session_id
    /// 3. If not found, creates a new session and maps it
    pub async fn resolve_session(&self, hints: &SessionHints) -> Result<String> {
        let store = self.store.lock().await;

        // Try to find existing session by external key
        if let Some(ref external_id) = hints.external_id
            && let Some(session_id) =
                store.get_session_by_external_key(hints.source.as_str(), external_id)?
        {
            debug!(
                "Found existing session {} for {}:{}",
                session_id,
                hints.source.as_str(),
                external_id
            );
            return Ok(session_id);
        }

        // Generate new session ID
        let session_id = uuid::Uuid::new_v4().to_string();
        let channel_id = hints
            .external_id
            .as_deref()
            .unwrap_or("default")
            .to_string();
        let user_id = hints.user_id.as_deref().unwrap_or("anonymous");
        let metadata = hints.metadata.clone().unwrap_or(serde_json::json!({}));

        // Create the session
        store.upsert_session(&session_id, &channel_id, user_id, &metadata)?;

        // Map external ID to session if provided
        if let Some(ref external_id) = hints.external_id {
            store.upsert_session_key(&session_id, hints.source.as_str(), external_id)?;
            info!(
                "Created new session {} for {}:{}",
                session_id,
                hints.source.as_str(),
                external_id
            );
        } else {
            info!("Created new session {} (no external key)", session_id);
        }

        Ok(session_id)
    }

    /// Get session ID by external key directly
    pub async fn get_session_by_external_key(
        &self,
        source: ChatSource,
        external_id: &str,
    ) -> Result<Option<String>> {
        let store = self.store.lock().await;
        store.get_session_by_external_key(source.as_str(), external_id)
    }

    /// Create a new session without external key mapping
    pub async fn create_session(&self, channel_id: &str, user_id: &str) -> Result<String> {
        let store = self.store.lock().await;
        let session_id = uuid::Uuid::new_v4().to_string();
        store.upsert_session(
            &session_id,
            channel_id,
            user_id,
            &serde_json::json!({}),
        )?;
        Ok(session_id)
    }

    /// Map an existing session to a new external key
    pub async fn map_session_to_external(
        &self,
        session_id: &str,
        source: ChatSource,
        external_id: &str,
    ) -> Result<()> {
        let store = self.store.lock().await;
        store.upsert_session_key(session_id, source.as_str(), external_id)?;
        Ok(())
    }

    /// Delete a session key mapping
    pub async fn unmap_session(
        &self,
        source: ChatSource,
        external_id: &str,
    ) -> Result<()> {
        let store = self.store.lock().await;
        store.delete_session_key(source.as_str(), external_id)?;
        Ok(())
    }

    /// Get message count for a session
    pub async fn get_message_count(&self, session_id: &str) -> Result<i32> {
        let store = self.store.lock().await;
        store.get_message_count(session_id)
    }

    /// Save a session summary (for long conversations)
    pub async fn save_summary(
        &self,
        session_id: &str,
        summary_text: &str,
        message_count: i32,
    ) -> Result<()> {
        let store = self.store.lock().await;
        store.save_session_summary(session_id, summary_text, message_count)
    }

    /// Get the latest summary for a session
    pub async fn get_latest_summary(
        &self,
        session_id: &str,
    ) -> Result<Option<(String, i32)>> {
        let store = self.store.lock().await;
        store.get_latest_session_summary(session_id)
    }
}

/// Strategy for resolving session ID from incoming requests
#[derive(Debug, Clone)]
pub enum SessionKeyStrategy {
    /// Use X-Session-Id header
    Header,
    /// Use metadata.session_id in request body
    BodyMetadata,
    /// Derive from user_id + workspace_id
    Derived,
    /// Create new session if not provided
    CreateNew,
}

impl Default for SessionKeyStrategy {
    fn default() -> Self {
        SessionKeyStrategy::Header
    }
}

/// Session resolver configuration
#[derive(Debug, Clone)]
pub struct SessionResolverConfig {
    /// Strategy for resolving session keys
    pub strategy: SessionKeyStrategy,
    /// Maximum messages to load from history
    pub max_history_messages: usize,
    /// Whether to create new sessions automatically
    pub auto_create_sessions: bool,
}

impl Default for SessionResolverConfig {
    fn default() -> Self {
        Self {
            strategy: SessionKeyStrategy::Header,
            max_history_messages: 50,
            auto_create_sessions: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_source_conversion() {
        assert_eq!(ChatSource::Telegram.as_str(), "telegram");
        assert_eq!(ChatSource::VsCode.as_str(), "vscode");
        assert_eq!(ChatSource::parse("telegram"), Some(ChatSource::Telegram));
        assert_eq!(ChatSource::parse("unknown"), None);
    }

    #[test]
    fn test_session_hints() {
        let hints = SessionHints::from_telegram(123456789, Some(987654321));
        assert_eq!(hints.source, ChatSource::Telegram);
        assert_eq!(hints.external_id, Some("123456789".to_string()));
        assert_eq!(hints.user_id, Some("987654321".to_string()));
    }
}
