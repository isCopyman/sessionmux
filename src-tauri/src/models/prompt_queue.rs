use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::acp::types::PromptInputBlock;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptQueueDraft {
    pub blocks: Vec<PromptInputBlock>,
    pub display_text: String,
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
    pub mode_id: Option<String>,
    pub state: PromptQueueItemState,
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
}

#[derive(Debug, Clone)]
pub(crate) struct ClaimedPromptQueueItem {
    pub id: String,
    pub conversation_id: i32,
    pub draft: PromptQueueDraft,
    /// Cross-Session collaboration event materialized into this execution
    /// queue item. `None` keeps ordinary same-Session follow-ups unchanged.
    pub origin_event_id: Option<String>,
    pub mode_id: Option<String>,
    pub claimed_by: String,
}
