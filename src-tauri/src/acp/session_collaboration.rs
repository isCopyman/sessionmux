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
pub const DEFAULT_ROOM_LIST_LIMIT: u32 = 50;
pub const MAX_ROOM_LIST_LIMIT: u32 = 200;
pub const DEFAULT_ROOM_READ_LIMIT: u32 = 50;
pub const MAX_ROOM_READ_LIMIT: u32 = 200;
pub const SEND_MESSAGE_ROOM_HINT: &str =
    "send_message is private mailbox mail only. Use post_room to post in a Room.";

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SessionMessageDeliveryMode {
    /// Legacy alias for [`SessionMessagePriority::Normal`].
    DeliverOnly,
    /// Legacy alias for [`SessionMessagePriority::High`].
    #[default]
    Queue,
}

/// How soon the target Agent should see this letter. Senders pick this;
/// [`SessionMessageDeliveryMode`] remains only so older companions still
/// deserialize.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionMessagePriority {
    /// Persist and attach the letter to the target's next ordinary turn.
    /// A closed Session is started so that turn can happen.
    Normal,
    /// Tell the target now: steer into the running turn when that channel
    /// exists, otherwise stop the turn and deliver. Closed Sessions resume.
    High,
}

impl SessionMessagePriority {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "normal" => Some(Self::Normal),
            "high" | "urgent" => Some(Self::High),
            _ => None,
        }
    }

    pub fn from_legacy_delivery_mode(mode: SessionMessageDeliveryMode) -> Self {
        match mode {
            SessionMessageDeliveryMode::DeliverOnly => Self::Normal,
            SessionMessageDeliveryMode::Queue => Self::High,
        }
    }

    pub fn invocation_policy(self) -> crate::models::CollaborationInvocationPolicy {
        match self {
            Self::Normal => crate::models::CollaborationInvocationPolicy::StoreOnly,
            Self::High => crate::models::CollaborationInvocationPolicy::InvokeWhenIdle,
        }
    }

    pub fn urgency(self) -> crate::models::CollaborationUrgency {
        match self {
            Self::Normal => crate::models::CollaborationUrgency::Normal,
            Self::High => crate::models::CollaborationUrgency::Urgent,
        }
    }
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
    /// Sender-facing priority. Omitted values fall back to `delivery_mode`
    /// so older companions keep working.
    #[serde(default)]
    pub priority: Option<SessionMessagePriority>,
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
    /// Legacy field. New companions omit it; Host Core rejects a value so an
    /// old companion cannot post to a Room through `send_message`.
    #[serde(default)]
    pub room_id: Option<String>,
    #[serde(default)]
    pub mention_all: bool,
}

/// Agent-supplied Room post after companion-side validation. Source Session
/// is filled in by the listener from the launch token. A Room post is one
/// timeline message: no title — the content is the whole post.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomPostSpec {
    pub room_id: String,
    pub content: String,
    #[serde(default)]
    pub mention_session_ids: Vec<i32>,
    #[serde(default)]
    pub mention_all: bool,
    #[serde(default)]
    pub mention_human: bool,
    #[serde(default)]
    pub priority: Option<SessionMessagePriority>,
    #[serde(default)]
    pub expects_reply: bool,
    #[serde(default)]
    pub reply_to_event_id: Option<String>,
    pub client_dedupe_id: String,
}

impl RoomPostSpec {
    pub fn resolved_priority(&self) -> SessionMessagePriority {
        self.priority.unwrap_or_else(|| {
            if self.mention_all || !self.mention_session_ids.is_empty() || self.mention_human {
                SessionMessagePriority::High
            } else {
                SessionMessagePriority::Normal
            }
        })
    }
}

impl SessionMessageSpec {
    pub fn resolved_priority(&self) -> SessionMessagePriority {
        self.priority.unwrap_or_else(|| {
            SessionMessagePriority::from_legacy_delivery_mode(self.delivery_mode)
        })
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room_id: Option<String>,
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

/// Which side of the mailbox `list_inbox` reads. The filter vocabulary is
/// shared: for `Sent` the same words describe the recipient's side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionMailboxScope {
    Inbox,
    Sent,
}

impl SessionMailboxScope {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "inbox" => Some(Self::Inbox),
            "sent" => Some(Self::Sent),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInboxItem {
    pub event_id: String,
    pub delivery_id: String,
    /// "inbound" or "outbound", relative to the calling Session.
    #[serde(default)]
    pub direction: String,
    /// The other party: the sender for inbox items, the recipient for sent.
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<SessionMailboxScope>,
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
            scope: None,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRoomListItem {
    pub room_id: String,
    pub title: String,
    pub member_count: u32,
    #[serde(default)]
    pub unread_count: u32,
    #[serde(default)]
    pub mention_unread_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_event_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionRoomListOutcome {
    pub available: bool,
    pub caller_session_id: Option<i32>,
    pub rooms: Vec<SessionRoomListItem>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl SessionRoomListOutcome {
    pub fn unavailable(caller_session_id: Option<i32>, note: impl Into<String>) -> Self {
        Self {
            available: false,
            caller_session_id,
            rooms: Vec::new(),
            truncated: false,
            note: Some(note.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRoomMember {
    pub session_id: i32,
    pub title: Option<String>,
    pub agent_type: Option<String>,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRoomEvent {
    pub event_id: String,
    pub from_session_id: i32,
    pub from_title: Option<String>,
    pub title: String,
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_event_id: Option<String>,
    pub mention_session_ids: Vec<i32>,
    #[serde(default)]
    pub mention_human: bool,
    #[serde(default)]
    pub from_author_kind: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionRoomReadOutcome {
    pub available: bool,
    pub caller_session_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub members: Vec<SessionRoomMember>,
    pub events: Vec<SessionRoomEvent>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RoomReadQuery {
    pub room_id: String,
    pub limit: u32,
    pub unread: bool,
    pub before_event_id: Option<String>,
}

impl SessionRoomReadOutcome {
    pub fn unavailable(caller_session_id: Option<i32>, note: impl Into<String>) -> Self {
        Self {
            available: false,
            caller_session_id,
            members: Vec::new(),
            events: Vec::new(),
            truncated: false,
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
            room_id: None,
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
        scope: SessionMailboxScope,
        filter: SessionInboxFilter,
        peer_session_id: Option<i32>,
        limit: u32,
    ) -> SessionInboxOutcome;

    async fn read_message(
        &self,
        caller_session_id: i32,
        event_id: String,
    ) -> SessionMessageReadOutcome;

    async fn list_rooms(
        &self,
        caller_session_id: i32,
        query: Option<String>,
        limit: u32,
    ) -> SessionRoomListOutcome;

    async fn read_room(
        &self,
        caller_session_id: i32,
        query: RoomReadQuery,
    ) -> SessionRoomReadOutcome;

    async fn post_room(&self, source_session_id: i32, spec: RoomPostSpec) -> SessionSendOutcome;
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
    fn letter_priority_maps_to_scheduler_policy() {
        assert_eq!(
            SessionMessagePriority::parse("high"),
            Some(SessionMessagePriority::High)
        );
        assert_eq!(
            SessionMessagePriority::parse("urgent"),
            Some(SessionMessagePriority::High)
        );
        assert_eq!(
            SessionMessagePriority::parse("normal"),
            Some(SessionMessagePriority::Normal)
        );
        assert_eq!(SessionMessagePriority::parse("invoke_when_idle"), None);
        assert_eq!(
            SessionMessagePriority::High.invocation_policy(),
            crate::models::CollaborationInvocationPolicy::InvokeWhenIdle
        );
        assert_eq!(
            SessionMessagePriority::Normal.invocation_policy(),
            crate::models::CollaborationInvocationPolicy::StoreOnly
        );
        let inferred_normal = SessionMessageSpec {
            target_session_ids: vec![1],
            title: "later".into(),
            content: "body".into(),
            delivery_mode: SessionMessageDeliveryMode::DeliverOnly,
            priority: None,
            steer_if_supported: false,
            expects_reply: false,
            reply_to_event_id: None,
            client_dedupe_id: "dedupe".into(),
            room_id: None,
            mention_all: false,
        };
        assert_eq!(
            inferred_normal.resolved_priority(),
            SessionMessagePriority::Normal
        );
        let explicit_high = SessionMessageSpec {
            delivery_mode: SessionMessageDeliveryMode::DeliverOnly,
            priority: Some(SessionMessagePriority::High),
            ..inferred_normal
        };
        assert_eq!(
            explicit_high.resolved_priority(),
            SessionMessagePriority::High
        );
    }

    #[test]
    fn inbox_filter_parses_known_wires_only() {
        assert_eq!(
            SessionMailboxScope::parse("inbox"),
            Some(SessionMailboxScope::Inbox)
        );
        assert_eq!(
            SessionMailboxScope::parse("sent"),
            Some(SessionMailboxScope::Sent)
        );
        assert_eq!(SessionMailboxScope::parse("outbox"), None);
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
