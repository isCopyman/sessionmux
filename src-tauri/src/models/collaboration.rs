use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::prompt_queue::PromptQueueItemState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollaborationInvocationPolicy {
    StoreOnly,
    InvokeWhenIdle,
}

impl CollaborationInvocationPolicy {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::StoreOnly => "store_only",
            Self::InvokeWhenIdle => "invoke_when_idle",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "store_only" => Some(Self::StoreOnly),
            "invoke_when_idle" => Some(Self::InvokeWhenIdle),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollaborationDeliveryHint {
    Default,
    SteerIfSupported,
}

impl CollaborationDeliveryHint {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::SteerIfSupported => "steer_if_supported",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "default" => Some(Self::Default),
            "steer_if_supported" => Some(Self::SteerIfSupported),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollaborationUrgency {
    Normal,
    Urgent,
}

impl CollaborationUrgency {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Urgent => "urgent",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "normal" => Some(Self::Normal),
            "urgent" => Some(Self::Urgent),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollaborationDeliveryState {
    Pending,
    Queued,
    Embedding,
    Embedded,
    Dismissed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollaborationInterruptState {
    Requested,
    Cancelling,
    TerminalObserved,
    WaitingForTerminal,
    Ready,
    Dispatching,
    Completed,
    Failed,
}

impl CollaborationInterruptState {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "requested" => Some(Self::Requested),
            "cancelling" => Some(Self::Cancelling),
            "terminal_observed" => Some(Self::TerminalObserved),
            "waiting_for_terminal" => Some(Self::WaitingForTerminal),
            "ready" => Some(Self::Ready),
            "dispatching" => Some(Self::Dispatching),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

impl CollaborationDeliveryState {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "queued" => Some(Self::Queued),
            "embedding" => Some(Self::Embedding),
            "embedded" => Some(Self::Embedded),
            "dismissed" => Some(Self::Dismissed),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollaborationSessionSnapshot {
    pub conversation_id: i32,
    pub title: Option<String>,
    pub agent_type: Option<String>,
    pub folder_path: Option<String>,
    pub backend: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollaborationDeliveryView {
    pub id: String,
    pub event_id: String,
    pub source: CollaborationSessionSnapshot,
    pub target: CollaborationSessionSnapshot,
    pub body: String,
    pub reply_to_event_id: Option<String>,
    pub expects_reply: bool,
    /// Whether this target Session has sent at least one reply linked to this
    /// event. This is derived from the immutable reply relation rather than
    /// stored as a second mutable source of truth.
    pub reply_received: bool,
    pub urgency: CollaborationUrgency,
    pub invocation_policy: CollaborationInvocationPolicy,
    pub delivery_hint: CollaborationDeliveryHint,
    pub state: CollaborationDeliveryState,
    /// Current execution-queue projection for `invoke_when_idle` deliveries.
    /// The delivery state remains the communication fact; these optional
    /// fields explain whether its pending Harness turn is queued or paused.
    pub queue_item_id: Option<String>,
    pub queue_state: Option<PromptQueueItemState>,
    pub queue_paused_reason: Option<String>,
    pub ui_seen_at: Option<DateTime<Utc>>,
    pub embedded_turn_ref: Option<String>,
    pub attempts: i32,
    pub error: Option<String>,
    pub interrupt_operation_id: Option<String>,
    pub interrupt_state: Option<CollaborationInterruptState>,
    pub interrupt_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollaborationFeed {
    pub conversation_id: i32,
    pub revision: i64,
    pub unread_count: u32,
    pub inbound: Vec<CollaborationDeliveryView>,
    pub outbound: Vec<CollaborationDeliveryView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollaborationUnreadSession {
    pub conversation_id: i32,
    pub unread_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollaborationUnreadOverview {
    pub total_unread_count: u32,
    pub sessions: Vec<CollaborationUnreadSession>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendCollaborationMessageInput {
    pub source_conversation_id: i32,
    pub target_conversation_ids: Vec<i32>,
    pub body: String,
    pub client_dedupe_id: String,
    #[serde(default = "default_invocation_policy")]
    pub invocation_policy: CollaborationInvocationPolicy,
    #[serde(default = "default_delivery_hint")]
    pub delivery_hint: CollaborationDeliveryHint,
    #[serde(default)]
    pub expects_reply: bool,
    #[serde(default = "default_urgency")]
    pub urgency: CollaborationUrgency,
    #[serde(default)]
    pub reply_to_event_id: Option<String>,
}

fn default_invocation_policy() -> CollaborationInvocationPolicy {
    CollaborationInvocationPolicy::StoreOnly
}

fn default_delivery_hint() -> CollaborationDeliveryHint {
    CollaborationDeliveryHint::Default
}

fn default_urgency() -> CollaborationUrgency {
    CollaborationUrgency::Normal
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollaborationSendResult {
    pub event_id: String,
    pub deliveries: Vec<CollaborationDeliveryView>,
    pub affected_conversation_ids: Vec<i32>,
    pub deduplicated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterruptCollaborationInput {
    pub event_id: String,
    pub target_conversation_id: i32,
    pub client_dedupe_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollaborationInterruptOperationView {
    pub id: String,
    pub event_id: String,
    pub target_conversation_id: i32,
    pub client_dedupe_id: String,
    pub reason: String,
    pub state: CollaborationInterruptState,
    pub connection_id_snapshot: Option<String>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollaborationInterruptResult {
    pub operation: CollaborationInterruptOperationView,
    pub deduplicated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendAndInterruptCollaborationInput {
    pub message: SendCollaborationMessageInput,
    pub interrupt_client_dedupe_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendAndInterruptCollaborationResult {
    pub message: CollaborationSendResult,
    pub interrupt: Option<CollaborationInterruptResult>,
    pub interrupt_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollaborationChanged {
    pub conversation_ids: Vec<i32>,
}
