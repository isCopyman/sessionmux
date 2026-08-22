use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::acp::types::PromptInputBlock;
use crate::models::CollaborationDeliveryHint;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptQueueDraft {
    pub blocks: Vec<PromptInputBlock>,
    pub display_text: String,
}

/// Who put this item into the execution queue. The scheduler claims by
/// class first (user > collaboration/reminder/automation/task > timer), FIFO
/// inside a class: a person's own follow-ups always run before automation,
/// and automation can never jump a letter the user is expecting the Agent
/// to read.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptQueueSource {
    #[default]
    User,
    Collaboration,
    Reminder,
    /// A scheduled prompt an Automation enqueued into an existing Session.
    /// Shares the middle class with letters and reminders: behind the user's
    /// own typing, ahead of idle-continuation timers.
    Automation,
    /// A task-board assignment delivered to an ordinary persistent Session.
    /// It shares the middle scheduling class and never jumps user follow-ups.
    Task,
    Timer,
}

impl PromptQueueSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Collaboration => "collaboration",
            Self::Reminder => "reminder",
            Self::Automation => "automation",
            Self::Task => "task",
            Self::Timer => "timer",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "user" => Some(Self::User),
            "collaboration" => Some(Self::Collaboration),
            "reminder" => Some(Self::Reminder),
            "automation" => Some(Self::Automation),
            "task" => Some(Self::Task),
            "timer" => Some(Self::Timer),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptQueueItemState {
    Queued,
    Claimed,
    Paused,
}

impl PromptQueueItemState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Claimed => "claimed",
            Self::Paused => "paused",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "queued" => Some(Self::Queued),
            "claimed" => Some(Self::Claimed),
            "paused" => Some(Self::Paused),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptQueueItem {
    pub id: String,
    pub conversation_id: i32,
    pub position: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draft: Option<PromptQueueDraft>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode_id: Option<String>,
    pub state: PromptQueueItemState,
    pub source: PromptQueueSource,
    pub client_dedupe_id: String,
    pub attempts: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paused_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptQueueSnapshot {
    pub conversation_id: i32,
    pub revision: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paused_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manual_release_item_id: Option<String>,
    pub items: Vec<PromptQueueItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnqueuePromptQueueItem {
    pub conversation_id: i32,
    pub id: String,
    pub client_dedupe_id: String,
    pub draft: PromptQueueDraft,
    #[serde(default)]
    pub mode_id: Option<String>,
    /// Scheduling class. Clients never send this: the enqueue commands pin
    /// 'user', and only host runtimes (timer, reminder) submit other values.
    #[serde(default, skip_deserializing)]
    pub source: PromptQueueSource,
    /// Set only by the trusted task assignment command. Ordinary queue clients
    /// cannot attach an arbitrary task to their message.
    #[serde(default, skip_deserializing)]
    pub task_id: Option<i32>,
}

#[derive(Debug, Clone)]
pub(crate) struct ClaimedPromptQueueItem {
    pub id: String,
    pub conversation_id: i32,
    pub draft: PromptQueueDraft,
    /// Cross-Session collaboration event materialized into this execution
    /// queue item. `None` keeps ordinary same-Session follow-ups unchanged.
    pub origin_event_id: Option<String>,
    pub task_id: Option<i32>,
    /// Present only for cross-Session deliveries. The queue remains the owner;
    /// this hint merely allows a best-effort native steer while the target is
    /// busy and never changes ordinary same-Session follow-up behavior.
    pub delivery_hint: Option<CollaborationDeliveryHint>,
    pub mode_id: Option<String>,
    pub claimed_by: String,
    /// Extra queue rows claimed with the head so one turn can carry several
    /// collaboration envelopes. Empty for ordinary user drafts and single
    /// letters. Does not include `id`.
    pub batched_claim_ids: Vec<String>,
    /// Origins for `batched_claim_ids`, same order. Does not include
    /// `origin_event_id`.
    pub batched_origin_event_ids: Vec<String>,
}

impl ClaimedPromptQueueItem {
    pub(crate) fn claim_ids(&self) -> Vec<&str> {
        let mut ids = Vec::with_capacity(1 + self.batched_claim_ids.len());
        ids.push(self.id.as_str());
        ids.extend(self.batched_claim_ids.iter().map(String::as_str));
        ids
    }

    pub(crate) fn origin_event_ids(&self) -> Vec<&str> {
        let mut ids = Vec::with_capacity(
            usize::from(self.origin_event_id.is_some()) + self.batched_origin_event_ids.len(),
        );
        if let Some(id) = self.origin_event_id.as_deref() {
            ids.push(id);
        }
        ids.extend(self.batched_origin_event_ids.iter().map(String::as_str));
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::types::PromptInputBlock;

    /// Frozen sandwich: outer `PromptQueueDraft` is camelCase (`displayText`),
    /// inner `PromptInputBlock` is snake_case (`type`/`mime_type`).
    #[test]
    fn prompt_queue_draft_persisted_json_shape_is_pinned() {
        let legacy = r#"{
            "blocks": [
                {"type":"text","text":"hello"},
                {"type":"image","data":"QUJD","mime_type":"image/png"}
            ],
            "displayText": "hello"
        }"#;
        let draft: PromptQueueDraft =
            serde_json::from_str(legacy).expect("legacy draft_json decodes");
        assert_eq!(draft.display_text, "hello");
        assert_eq!(draft.blocks.len(), 2);
        match &draft.blocks[1] {
            PromptInputBlock::Image { mime_type, .. } => {
                assert_eq!(mime_type, "image/png");
            }
            other => panic!("expected image block, got {other:?}"),
        }

        let v = serde_json::to_value(&draft).unwrap();
        assert!(v.get("displayText").is_some());
        assert!(v.get("display_text").is_none());
        assert!(v.get("blocks").is_some());
        let image = &v["blocks"][1];
        assert_eq!(image["type"], "image");
        assert!(image.get("mime_type").is_some());
        assert!(image.get("mimeType").is_none());
    }
}
