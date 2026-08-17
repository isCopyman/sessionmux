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
pub const DEFAULT_INBOX_LIMIT: u32 = 20;
pub const MAX_INBOX_LIMIT: u32 = 50;
pub const INBOX_PREVIEW_CHARS: usize = 160;
pub const MAX_LETTER_TITLE_CHARS: usize = 120;

pub fn inbox_preview(body: &str) -> String {
    let collapsed = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut preview: String = collapsed.chars().take(INBOX_PREVIEW_CHARS).collect();
    if collapsed.chars().count() > INBOX_PREVIEW_CHARS {
        preview.push('…');
    }
    preview
}

/// Email-style letter title. New mail must supply this; legacy empty
/// subjects fall back to a body preview so old rows stay listable.
pub fn letter_title(subject: &str, body: &str) -> String {
    let trimmed = subject.split_whitespace().collect::<Vec<_>>().join(" ");
    if !trimmed.is_empty() {
        return trimmed.chars().take(MAX_LETTER_TITLE_CHARS).collect();
    }
    inbox_preview(body)
}

pub fn normalize_letter_title(subject: &str) -> Result<String, String> {
    let trimmed = subject.split_whitespace().collect::<Vec<_>>().join(" ");
    if trimmed.is_empty() {
        return Err("send_message requires a non-empty `title` (letter subject)".to_string());
    }
    if trimmed.chars().count() > MAX_LETTER_TITLE_CHARS {
        return Err(format!(
            "send_message `title` must be at most {MAX_LETTER_TITLE_CHARS} characters"
        ));
    }
    Ok(trimmed)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionMessageDeliveryMode {
    /// Persist and display the message without asking the target Harness to
    /// consume it as a new turn.
    DeliverOnly,
    /// Persist first, then enqueue an origin-event reference. The dispatcher
    /// starts or resumes a closed Session so the notice can be delivered.
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
    /// Short letter title, like an email subject. Required for new mail.
    pub title: String,
    pub content: String,
    pub delivery_mode: SessionMessageDeliveryMode,
    /// Best-effort, non-destructive hint. The Host may inject into a currently
    /// running turn only through a proven native steering channel; otherwise
    /// the durable message remains queued for the next ordinary turn.
    #[serde(default)]
    pub steer_if_supported: bool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionInboxFilter {
    Open,
    Unread,
    AwaitingReply,
    All,
}

impl SessionInboxFilter {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "open" => Some(Self::Open),
            "unread" => Some(Self::Unread),
            "awaiting_reply" => Some(Self::AwaitingReply),
            "all" => Some(Self::All),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInboxItem {
    pub event_id: String,
    pub delivery_id: String,
    pub from_session_id: i32,
    pub from_title: Option<String>,
    pub from_agent_type: Option<String>,
    pub title: String,
    pub preview: String,
    pub unread: bool,
    pub expects_reply: bool,
    pub obligation_state: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionInboxOutcome {
    pub available: bool,
    pub caller_session_id: Option<i32>,
    pub unread_count: u32,
    pub awaiting_reply_count: u32,
    pub items: Vec<SessionInboxItem>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl SessionInboxOutcome {
    pub fn unavailable(caller_session_id: Option<i32>, note: impl Into<String>) -> Self {
        Self {
            available: false,
            caller_session_id,
            unread_count: 0,
            awaiting_reply_count: 0,
            items: Vec::new(),
            truncated: false,
            note: Some(note.into()),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionMessageReadOutcome {
    pub available: bool,
    pub caller_session_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_session_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_agent_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    pub expects_reply: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_event_id: Option<String>,
    pub unread: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub obligation_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl SessionMessageReadOutcome {
    pub fn unavailable(caller_session_id: Option<i32>, note: impl Into<String>) -> Self {
        Self {
            available: false,
            caller_session_id,
            note: Some(note.into()),
            ..Default::default()
        }
    }
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

    async fn list_inbox(
        &self,
        caller_session_id: i32,
        filter: SessionInboxFilter,
        limit: u32,
    ) -> SessionInboxOutcome;

    async fn read_message(
        &self,
        caller_session_id: i32,
        event_id: String,
    ) -> SessionMessageReadOutcome;
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

    #[test]
    fn inbox_preview_collapses_whitespace_and_caps_length() {
        assert_eq!(inbox_preview("  hello   world  "), "hello world");
        let long = "word ".repeat(80);
        let preview = inbox_preview(&long);
        assert!(preview.chars().count() <= INBOX_PREVIEW_CHARS + 1);
        assert!(preview.ends_with('…'));
    }

    #[test]
    fn letter_title_prefers_subject_and_falls_back_to_body() {
        assert_eq!(
            letter_title("  Review  claim  ", "long body"),
            "Review claim"
        );
        assert_eq!(letter_title("   ", "hello   world"), "hello world");
        assert!(normalize_letter_title("").is_err());
        assert!(normalize_letter_title(&"x".repeat(MAX_LETTER_TITLE_CHARS + 1)).is_err());
        assert_eq!(normalize_letter_title("  Ping  ").unwrap(), "Ping");
    }

    #[test]
    fn inbox_filter_parses_known_wires_only() {
        assert_eq!(
            SessionInboxFilter::parse("open"),
            Some(SessionInboxFilter::Open)
        );
        assert_eq!(
            SessionInboxFilter::parse("unread"),
            Some(SessionInboxFilter::Unread)
        );
        assert_eq!(
            SessionInboxFilter::parse("awaiting_reply"),
            Some(SessionInboxFilter::AwaitingReply)
        );
        assert_eq!(
            SessionInboxFilter::parse("all"),
            Some(SessionInboxFilter::All)
        );
        assert_eq!(SessionInboxFilter::parse("urgent"), None);
    }

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
