//! Decide how mailbox attention reaches a target, and when it is due.
//!
//! New mail is due immediately. Unread follow-ups and read-but-unreplied
//! mail use a 5-minute clock. There is no urgency dimension. Busy Sessions
//! are injected when native steering exists, otherwise the reminder waits in
//! the same durable queue as ordinary follow-ups. Closed Sessions are not
//! cold-started.

use chrono::{DateTime, Duration, Utc};

/// First unread reminder is due as soon as the Delivery exists.
pub const UNREAD_AFTER_SECS: i64 = 0;
/// Awaiting reply after the Agent actually received the body.
pub const REPLY_AFTER_SECS: i64 = 5 * 60;
/// After a successful reminder, wait before nagging the same Session again.
pub const REMINDER_COOLDOWN_SECS: i64 = 5 * 60;
pub const MAX_REMINDER_REPEATS: u32 = 3;
pub const REMINDER_SCAN_SECS: u64 = 5;

/// Who the Delivery is addressed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReminderAudience {
    AgentSession,
    Human,
}

/// Host-visible runtime of a Session. Closed / no connection is `Missing`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReminderRuntime {
    Missing,
    ConnectedBusy,
    ConnectedIdle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReminderTargetState {
    pub audience: ReminderAudience,
    pub runtime: ReminderRuntime,
    /// Agent has not yet received the body in a real turn.
    pub has_unread: bool,
    /// Agent received the body and still owes a linked reply.
    pub has_awaiting_reply: bool,
    /// `_session/steering` passed Codeg's native-steering gates for this connection.
    pub native_steering: bool,
}

/// How Codeg should surface mailbox attention right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollaborationReminderLane {
    /// Busy + native steering: inject a short digest into the running turn.
    InjectSteer,
    /// Busy, no native steer: persist the digest behind the current turn.
    QueueAfterTurn,
    /// Connected and idle: host starts a turn so mail cannot sit forever.
    IdleStart,
    /// Closed Session: keep mail; do not cold-start.
    HoldClosed,
    /// Mail for the host user: not implemented.
    HumanOverlay,
    /// Nothing to surface.
    Silent,
}

impl ReminderTargetState {
    pub fn needs_attention(self) -> bool {
        self.has_unread || self.has_awaiting_reply
    }
}

pub fn choose_reminder_lane(state: ReminderTargetState) -> CollaborationReminderLane {
    if !state.needs_attention() {
        return CollaborationReminderLane::Silent;
    }
    if state.audience == ReminderAudience::Human {
        return CollaborationReminderLane::HumanOverlay;
    }
    match state.runtime {
        ReminderRuntime::ConnectedBusy if state.native_steering => {
            CollaborationReminderLane::InjectSteer
        }
        ReminderRuntime::ConnectedBusy => CollaborationReminderLane::QueueAfterTurn,
        ReminderRuntime::ConnectedIdle => CollaborationReminderLane::IdleStart,
        ReminderRuntime::Missing => CollaborationReminderLane::HoldClosed,
    }
}

pub fn unread_is_due(created_at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    now >= created_at + Duration::seconds(UNREAD_AFTER_SECS)
}

pub fn reply_is_due(received_at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    now >= received_at + Duration::seconds(REPLY_AFTER_SECS)
}

pub fn reminder_in_cooldown(last_reminded_at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    now < last_reminded_at + Duration::seconds(REMINDER_COOLDOWN_SECS)
}

/// Short host-authored digest. Never includes the original letter body.
pub fn reminder_digest_text(unread: u32, awaiting_reply: u32) -> String {
    let mut parts = Vec::new();
    if unread > 0 {
        parts.push(format!("有 {unread} 封未读会话消息"));
    }
    if awaiting_reply > 0 {
        parts.push(format!("有 {awaiting_reply} 封已读但仍需回复的会话消息"));
    }
    if parts.is_empty() {
        return String::new();
    }
    format!(
        "Codeg 信箱：{}。请用 list_inbox 查看，需要时用 read_message 打开信件。",
        parts.join("；")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(runtime: ReminderRuntime) -> ReminderTargetState {
        ReminderTargetState {
            audience: ReminderAudience::AgentSession,
            runtime,
            has_unread: true,
            has_awaiting_reply: false,
            native_steering: false,
        }
    }

    #[test]
    fn idle_session_is_started_so_mail_cannot_sit() {
        assert_eq!(
            choose_reminder_lane(agent(ReminderRuntime::ConnectedIdle)),
            CollaborationReminderLane::IdleStart
        );
    }

    #[test]
    fn busy_without_steer_waits_in_the_shared_queue() {
        assert_eq!(
            choose_reminder_lane(agent(ReminderRuntime::ConnectedBusy)),
            CollaborationReminderLane::QueueAfterTurn
        );
    }

    #[test]
    fn busy_with_native_steer_injects() {
        let mut state = agent(ReminderRuntime::ConnectedBusy);
        state.native_steering = true;
        assert_eq!(
            choose_reminder_lane(state),
            CollaborationReminderLane::InjectSteer
        );
    }

    #[test]
    fn closed_session_is_held_not_cold_started() {
        assert_eq!(
            choose_reminder_lane(agent(ReminderRuntime::Missing)),
            CollaborationReminderLane::HoldClosed
        );
    }

    #[test]
    fn clocks_are_immediate_unread_and_five_minute_reply() {
        let start = DateTime::parse_from_rfc3339("2026-08-17T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(unread_is_due(start, start));
        assert!(!reply_is_due(start, start + Duration::minutes(4)));
        assert!(reply_is_due(start, start + Duration::minutes(5)));
        assert!(reminder_in_cooldown(start, start + Duration::minutes(4)));
        assert!(!reminder_in_cooldown(start, start + Duration::minutes(5)));
    }

    #[test]
    fn digest_text_is_chinese_and_does_not_repeat_the_letter_body() {
        assert_eq!(
            reminder_digest_text(2, 1),
            "Codeg 信箱：有 2 封未读会话消息；有 1 封已读但仍需回复的会话消息。请用 list_inbox 查看，需要时用 read_message 打开信件。"
        );
    }
}
