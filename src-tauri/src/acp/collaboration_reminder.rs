//! Decide how mailbox attention reaches a target, and when it is due.
//!
//! New mail is due immediately. Unread follow-ups and read-but-unreplied
//! mail use a 5-minute clock. There is no urgency dimension. Busy Sessions
//! are injected when native steering exists, otherwise the reminder waits in
//! the same durable queue as ordinary follow-ups. Closed Sessions are started
//! or resumed so the notice can be delivered.

use chrono::{DateTime, Duration, Utc};

use super::types::ConnectionStatus;

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
    /// Closed / missing runtime: start or resume the Session, then deliver.
    EnsureRuntime,
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

/// Map ACP connection status onto reminder runtime. `Connecting` still has a
/// live process, so overdue mail is queued instead of treated as closed.
pub fn reminder_runtime(status: ConnectionStatus, turn_in_flight: bool) -> ReminderRuntime {
    match status {
        ConnectionStatus::Disconnected | ConnectionStatus::Error => ReminderRuntime::Missing,
        ConnectionStatus::Prompting => ReminderRuntime::ConnectedBusy,
        ConnectionStatus::Connected | ConnectionStatus::Connecting => {
            if turn_in_flight {
                ReminderRuntime::ConnectedBusy
            } else {
                ReminderRuntime::ConnectedIdle
            }
        }
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
        ReminderRuntime::Missing => CollaborationReminderLane::EnsureRuntime,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReminderLetterLine {
    pub event_id: String,
    pub from_session_id: i32,
    pub from_title: String,
    pub letter_title: String,
    pub awaiting_reply: bool,
}

/// Short host-authored digest. Never includes the original letter body.
pub fn reminder_digest_text(unread: u32, awaiting_reply: u32) -> String {
    reminder_digest_text_with_letters(unread, awaiting_reply, &[])
}

pub fn reminder_digest_text_with_letters(
    unread: u32,
    awaiting_reply: u32,
    letters: &[ReminderLetterLine],
) -> String {
    let mut parts = Vec::new();
    if unread > 0 {
        parts.push(format!("有 {unread} 封未读会话信件"));
    }
    if awaiting_reply > 0 {
        parts.push(format!("有 {awaiting_reply} 封已读但仍需回复的会话信件"));
    }
    if parts.is_empty() {
        return String::new();
    }
    let mut text = format!(
        "Codeg 系统信箱提醒（不是来自某个 Session 的信）：{}。",
        parts.join("；")
    );
    if !letters.is_empty() {
        text.push_str(" 标题：");
        let lines: Vec<String> = letters
            .iter()
            .take(8)
            .map(|letter| {
                let flag = if letter.awaiting_reply {
                    "已读未回"
                } else {
                    "未读"
                };
                format!(
                    "《{}》来自 {} #{}（{}，event_id={}）",
                    letter.letter_title,
                    letter.from_title,
                    letter.from_session_id,
                    flag,
                    letter.event_id
                )
            })
            .collect();
        text.push_str(&lines.join("；"));
        text.push('。');
    }
    text.push_str("请用 list_inbox 查看标题，需要时用 read_message 打开正文。");
    text
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
    fn connecting_session_is_queued_not_treated_as_closed() {
        assert_eq!(
            reminder_runtime(ConnectionStatus::Connecting, false),
            ReminderRuntime::ConnectedIdle
        );
        assert_eq!(
            reminder_runtime(ConnectionStatus::Connecting, true),
            ReminderRuntime::ConnectedBusy
        );
        assert_eq!(
            reminder_runtime(ConnectionStatus::Disconnected, false),
            ReminderRuntime::Missing
        );
    }

    #[test]
    fn closed_session_is_started_so_mail_can_arrive() {
        assert_eq!(
            choose_reminder_lane(agent(ReminderRuntime::Missing)),
            CollaborationReminderLane::EnsureRuntime
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
            "Codeg 系统信箱提醒（不是来自某个 Session 的信）：有 2 封未读会话信件；有 1 封已读但仍需回复的会话信件。请用 list_inbox 查看标题，需要时用 read_message 打开正文。"
        );
        let with_titles = reminder_digest_text_with_letters(
            1,
            0,
            &[ReminderLetterLine {
                event_id: "e1".into(),
                from_session_id: 290,
                from_title: "Session C".into(),
                letter_title: "Ping".into(),
                awaiting_reply: false,
            }],
        );
        assert!(with_titles.contains("《Ping》来自 Session C #290"));
        assert!(!with_titles.contains("MAILBOX-PING from"));
    }
}
