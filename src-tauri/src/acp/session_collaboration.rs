//! Session-to-session communication exposed to managed agents.
//!
//! The companion never chooses the sender. The listener authenticates its
//! per-launch token, resolves that token's parent connection to Codeg's stable
//! conversation id, and passes the id to [`SessionCollaborationAccess`]. This
//! keeps human-readable titles and roles out of the trust boundary: they are
//! selectors only, while delivery always uses stable Session ids.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

pub const DEFAULT_SESSION_LIST_LIMIT: u32 = 50;
pub const MAX_SESSION_LIST_LIMIT: u32 = 200;
pub const MAX_SESSION_MESSAGE_TARGETS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionMessageDeliveryMode {
    /// Persist and display the message without asking the target Harness to
    /// consume it as a new turn.
    DeliverOnly,
    /// Persist first, then enqueue an origin-event reference for the target's
    /// existing runtime. A closed Session is never cold-started.
    Queue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionAddress {
    pub session_id: i32,
    pub title: Option<String>,
    pub agent_type: String,
    pub status: String,
    pub model: Option<String>,
    pub workspace_name: Option<String>,
    pub workspace_path: Option<String>,
    pub updated_at: DateTime<Utc>,
    pub archived: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionListOutcome {
    pub available: bool,
    pub caller_session_id: Option<i32>,
    pub sessions: Vec<SessionAddress>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl SessionListOutcome {
    pub fn unavailable(caller_session_id: Option<i32>, note: impl Into<String>) -> Self {
        Self {
            available: false,
            caller_session_id,
            sessions: Vec::new(),
            truncated: false,
            note: Some(note.into()),
        }
    }
}

/// Agent-supplied message fields after companion-side validation. The source
/// Session is deliberately absent; the listener supplies it from the token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessageSpec {
    pub target_session_ids: Vec<i32>,
    pub content: String,
    pub delivery_mode: SessionMessageDeliveryMode,
    pub expects_reply: bool,
    pub reply_to_event_id: Option<String>,
    /// Companion-generated from the parent connection + MCP request id. The
    /// model cannot spoof the sender or pick a key that collides with UI sends.
    pub client_dedupe_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionMessageDeliveryOutcome {
    pub target_session_id: i32,
    pub target_title: Option<String>,
    pub target_agent_type: Option<String>,
    pub state: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionSendOutcome {
    pub accepted: bool,
    pub source_session_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    pub deliveries: Vec<SessionMessageDeliveryOutcome>,
    pub deduplicated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl SessionSendOutcome {
    pub fn rejected(source_session_id: Option<i32>, note: impl Into<String>) -> Self {
        Self {
            accepted: false,
            source_session_id,
            event_id: None,
            deliveries: Vec::new(),
            deduplicated: false,
            note: Some(note.into()),
        }
    }
}

#[async_trait]
pub trait SessionCollaborationAccess: Send + Sync {
    async fn list_sessions(
        &self,
        caller_session_id: i32,
        query: Option<String>,
        limit: u32,
    ) -> SessionListOutcome;

    async fn send_message(
        &self,
        source_session_id: i32,
        spec: SessionMessageSpec,
    ) -> SessionSendOutcome;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionCollaborationConfig {
    pub enabled: bool,
}

/// Hot-swappable collaboration capability. Injection reads it to hide the MCP
/// tools, and the Host Core write path reads it again for defense in depth.
#[derive(Clone, Default)]
pub struct SessionCollaborationRuntimeConfig {
    inner: Arc<RwLock<SessionCollaborationConfig>>,
}

impl SessionCollaborationRuntimeConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn snapshot(&self) -> SessionCollaborationConfig {
        self.inner.read().await.clone()
    }

    pub async fn set(&self, config: SessionCollaborationConfig) {
        *self.inner.write().await = config;
    }

    pub async fn is_enabled(&self) -> bool {
        self.inner.read().await.enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn runtime_config_round_trips() {
        let config = SessionCollaborationRuntimeConfig::new();
        assert!(!config.is_enabled().await);
        config
            .set(SessionCollaborationConfig { enabled: true })
            .await;
        assert!(config.is_enabled().await);
    }
}
