//! Durable per-Session idle continuation timers.
//!
//! A timer is intentionally not a second Goal system. It waits for its
//! Session to finish a Turn, then submits `prompt_text` as the next ordinary
//! user follow-up. The same Agent owns completion: it pauses or stops the
//! timer before ending once the objective is done.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const DEFAULT_IDLE_GRACE_SECS: i64 = 2;
pub const MAX_IDLE_GRACE_SECS: i64 = 300;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTimerInfo {
    pub id: String,
    pub conversation_id: i32,
    pub idle_grace_secs: i64,
    pub prompt_text: String,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_fired_at: Option<DateTime<Utc>>,
    pub fire_count: i32,
    /// Consecutive continuations that fired while the Session was waiting on
    /// unanswered outbound letters and no new information had arrived. Resets
    /// to zero on any real news (inbound letter, resolved obligation) and on
    /// every user edit.
    pub strike_count: i32,
    /// Set when the no-progress brake parked this timer. `enabled` stays
    /// true: new mailbox information or a user edit clears this
    /// automatically.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_paused_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_pause_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionTimerInput {
    pub conversation_id: i32,
    pub prompt_text: String,
    #[serde(default = "default_idle_grace_secs")]
    pub idle_grace_secs: i64,
    #[serde(default)]
    pub client_dedupe_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSessionTimerInput {
    #[serde(default)]
    pub prompt_text: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub idle_grace_secs: Option<i64>,
    /// Optimistic concurrency guard shared by UI and Agent control paths.
    pub expected_updated_at: DateTime<Utc>,
}

pub const fn default_idle_grace_secs() -> i64 {
    DEFAULT_IDLE_GRACE_SECS
}
