use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::prompt_queue::PromptQueueItemState;

/// When a persisted delivery should start an Agent turn.
///
/// This is the Host scheduler field, not the sender-facing letter priority.
/// Senders pick `priority=high|normal`; Host maps it here:
/// - high → [`Self::InvokeWhenIdle`] (steer if the busy target supports it,
///   otherwise interrupt; resume a closed Session)
/// - normal → [`Self::StoreOnly`] (attach to the next ordinary turn; resume
///   a closed Session so that turn can happen)
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollaborationInvocationPolicy {
    #[default]
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

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollaborationDeliveryHint {
    #[default]
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

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollaborationUrgency {
    #[default]
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
pub enum CollaborationAttentionState {
    Unread,
    Opened,
}

impl CollaborationAttentionState {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "unread" => Some(Self::Unread),
            "opened" => Some(Self::Opened),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollaborationAgentReceiptKind {
    ManagedAcp,
    LegacyEmbedded,
}

impl CollaborationAgentReceiptKind {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "managed_acp" => Some(Self::ManagedAcp),
            "legacy_embedded" => Some(Self::LegacyEmbedded),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollaborationObligationState {
    None,
    AwaitingReply,
    Resolved,
}

impl CollaborationObligationState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::AwaitingReply => "awaiting_reply",
            Self::Resolved => "resolved",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::None),
            "awaiting_reply" => Some(Self::AwaitingReply),
            "resolved" => Some(Self::Resolved),
            _ => None,
        }
    }
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
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Queued => "queued",
            Self::Embedding => "embedding",
            Self::Embedded => "embedded",
            Self::Dismissed => "dismissed",
            Self::Failed => "failed",
        }
    }

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
    #[serde(default)]
    pub subject: String,
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
    pub attention_state: CollaborationAttentionState,
    pub opened_at: Option<DateTime<Utc>>,
    pub agent_received_at: Option<DateTime<Utc>>,
    pub agent_receipt_kind: Option<CollaborationAgentReceiptKind>,
    pub agent_receipt_ref: Option<String>,
    pub obligation_state: CollaborationObligationState,
    pub obligation_created_at: Option<DateTime<Utc>>,
    pub obligation_resolved_at: Option<DateTime<Utc>>,
    /// Compatibility aliases retained while existing clients migrate to the
    /// explicit mailbox lifecycle fields above.
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

/// Read-only projection of collaboration deliveries that have actually been
/// embedded in this Session's Harness transcript. The delivery remains the
/// single source of truth; clients place each item by `embedded_turn_ref`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollaborationTimelineProjection {
    pub conversation_id: i32,
    pub revision: i64,
    pub inbound: Vec<CollaborationDeliveryView>,
    #[serde(default)]
    pub outbound: Vec<CollaborationDeliveryView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollaborationUnreadSession {
    pub conversation_id: i32,
    pub revision: i64,
    pub unread_count: u32,
    pub needs_reply_count: u32,
    pub awaiting_reply_count: u32,
    pub failed_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollaborationUnreadOverview {
    pub total_unread_count: u32,
    pub total_needs_reply_count: u32,
    pub total_awaiting_reply_count: u32,
    pub total_failed_count: u32,
    /// Room posts nobody has opened, summed over active Rooms. Kept out of
    /// `total_unread_count`: that one is per-Session mailbox state and the
    /// Session Center filters against `sessions`, which Rooms never populate.
    pub total_room_unread_count: u32,
    /// Outstanding reply obligations inside active Rooms. Same separation as
    /// above — a Room debt belongs to the Room, not to one Session's mailbox.
    pub total_room_needs_reply_count: u32,
    pub sessions: Vec<CollaborationUnreadSession>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendCollaborationMessageInput {
    pub source_conversation_id: i32,
    pub target_conversation_ids: Vec<i32>,
    /// Email-style letter title. Required for new mail.
    #[serde(default)]
    pub subject: String,
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

impl SendCollaborationMessageInput {
    pub fn letter(
        source_conversation_id: i32,
        target_conversation_ids: Vec<i32>,
        client_dedupe_id: impl Into<String>,
        subject: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        Self {
            source_conversation_id,
            target_conversation_ids,
            subject: subject.into(),
            body: body.into(),
            client_dedupe_id: client_dedupe_id.into(),
            invocation_policy: default_invocation_policy(),
            delivery_hint: default_delivery_hint(),
            expects_reply: false,
            urgency: default_urgency(),
            reply_to_event_id: None,
        }
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollaborationVisibility {
    Direct,
    Room,
}

impl CollaborationVisibility {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Room => "room",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CollaborationAuthorKind {
    #[default]
    Session,
    Human,
}

impl CollaborationAuthorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Human => "human",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "session" => Some(Self::Session),
            "human" => Some(Self::Human),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCollaborationRoomInput {
    pub workbench_id: i32,
    pub title: String,
    pub member_conversation_ids: Vec<i32>,
    pub created_by_conversation_id: i32,
    #[serde(default)]
    pub collection_id: Option<i32>,
    #[serde(default)]
    pub root_folder_id: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddCollaborationRoomMembersInput {
    pub room_id: String,
    pub conversation_ids: Vec<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostRoomMessageInput {
    pub room_id: String,
    pub source_conversation_id: i32,
    pub target_conversation_ids: Vec<i32>,
    #[serde(default)]
    pub mention_all: bool,
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
    #[serde(default)]
    pub mention_human: bool,
    #[serde(default)]
    pub author_kind: CollaborationAuthorKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollaborationRoomMember {
    pub conversation_id: i32,
    pub title: Option<String>,
    pub agent_type: Option<String>,
    pub role: String,
    pub joined_at: DateTime<Utc>,
    pub last_read_at: Option<DateTime<Utc>>,
}

/// An extra `@`-search root a Room was given besides its bound
/// `root_folder_id` — a bare filesystem path the user typed in, not a Folder.
/// See `collaboration_room_path` (migration `m20260820_000001`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomAdditionalPath {
    pub id: i32,
    pub path: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollaborationRoomSummary {
    pub id: String,
    pub workbench_id: i32,
    pub title: String,
    pub created_by_conversation_id: i32,
    #[serde(default)]
    pub collection_id: Option<i32>,
    #[serde(default)]
    pub root_folder_id: Option<i32>,
    pub member_count: u32,
    /// Extra `@`-search paths added on top of `root_folder_id`. Host /
    /// Workbench summary lists don't need the full list, only the count.
    #[serde(default)]
    pub additional_path_count: u32,
    pub unread_count: u32,
    /// Agent `list_rooms`: deliveries of Room `@` this member has not consumed.
    /// Host / Workbench lists count posts that `@`-mentioned the user instead —
    /// the human has no Delivery row, so it reads against the Room cursor.
    #[serde(default)]
    pub mention_unread_count: u32,
    /// Agent `list_rooms`: deliveries to this Session in the Room that still
    /// expect a reply. Host / Workbench lists widen it to every member, i.e.
    /// "somebody in this Room still owes an answer".
    #[serde(default)]
    pub needs_reply_count: u32,
    /// Agent `list_rooms`: deliveries from this Session's Room posts that
    /// others have not answered. Host / Workbench lists read the human's own
    /// posts the same way, so it is a subset of `needs_reply_count`.
    #[serde(default)]
    pub awaiting_reply_count: u32,
    pub last_event_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollaborationRoomDetail {
    pub id: String,
    pub workbench_id: i32,
    pub title: String,
    pub created_by_conversation_id: i32,
    #[serde(default)]
    pub collection_id: Option<i32>,
    #[serde(default)]
    pub root_folder_id: Option<i32>,
    pub members: Vec<CollaborationRoomMember>,
    /// Extra `@`-search paths added on top of `root_folder_id`, in insertion
    /// order (oldest first) — the "manage paths" dialog's full list.
    #[serde(default)]
    pub additional_paths: Vec<RoomAdditionalPath>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomTimelineEvent {
    pub id: String,
    pub room_id: String,
    pub source: CollaborationSessionSnapshot,
    pub subject: String,
    pub body: String,
    pub reply_to_event_id: Option<String>,
    pub expects_reply: bool,
    pub urgency: CollaborationUrgency,
    pub mention_conversation_ids: Vec<i32>,
    #[serde(default)]
    pub mention_human: bool,
    #[serde(default)]
    pub author_kind: CollaborationAuthorKind,
    /// How many Sessions this post is still waiting on, as "resolved of
    /// expected". `@`-ing N Sessions files N independent Delivery rows with
    /// their own `obligation_state`, so the ledger is exact — these only read
    /// it back. Both are `Some` exactly when `expects_reply` is set: a post
    /// that asked nothing has no ledger to report, and `None` must not be
    /// rendered as `0/0`.
    ///
    /// Live obligations only: a `dismissed` or `failed` Delivery is not an
    /// obligation at all, so it leaves *both* sides of the fraction rather
    /// than sitting in the denominator forever (the obligation invariant in
    /// `MODEL-AUDIT-RFC-2026-08-21`, slice ③). `expected` can therefore be
    /// `Some(0)` — an ask that only reached `@human`, who has no Delivery row
    /// of its own, or one whose deliveries were all dismissed.
    #[serde(default)]
    pub expected_reply_count: Option<u32>,
    /// Subset of `expected_reply_count` whose obligation is already paid.
    #[serde(default)]
    pub resolved_reply_count: Option<u32>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomTimeline {
    pub room_id: String,
    pub events: Vec<RoomTimelineEvent>,
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomChanged {
    pub room_id: String,
    pub workbench_id: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomPostResult {
    pub event_id: String,
    pub room_id: String,
    pub deliveries: Vec<CollaborationDeliveryView>,
    pub affected_conversation_ids: Vec<i32>,
    pub deduplicated: bool,
    /// The parent event whose `awaiting_reply` obligation *this* post just
    /// settled for its author, or `None` when it settled nothing: no
    /// `reply_to_event_id`, nothing was owed on that parent, or a dedupe
    /// retry that only re-read an already committed event. Posting is the
    /// only way to clear a debt, so the poster must be able to read the
    /// ledger answer off the post instead of re-reading the Room.
    #[serde(default)]
    pub cleared_reply_to_event_id: Option<String>,
    /// Obligations the author still owes inside this Room once the post
    /// landed. `0` means their ledger here is clean.
    #[serde(default)]
    pub open_reply_debt: u32,
}
