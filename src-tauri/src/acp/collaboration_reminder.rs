//! Decide how mailbox attention reaches a target.
//!
//! Mail is persisted first. This module only chooses the *lane*:
//! whisper into a running turn, start an idle Session, hold, or toast a human.
//! It does not send, claim, or mutate mailbox rows.

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
    pub do_not_disturb: bool,
    /// Agent has not yet received the body in a real turn.
    pub has_unread: bool,
    /// Agent (or human) received the body and still owes a linked reply.
    pub has_awaiting_reply: bool,
}

/// How Codeg should surface mailbox attention right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollaborationReminderLane {
    /// Running turn: inject a short digest via hook/checkpoint. Do not interrupt.
    HookWhisper,
    /// Connected and idle: host starts a turn so mail cannot sit forever.
    IdleStart,
    /// Closed Session: keep mail; do not cold-start; tell the human.
    HoldClosed,
    /// Do-not-disturb: no push. The Agent may still pull inbox.
    HoldDnd,
    /// Mail for the host user: desktop overlay, never a Session turn.
    HumanOverlay,
    /// Nothing to surface.
    Silent,
}

impl ReminderTargetState {
    pub fn needs_attention(self) -> bool {
        self.has_unread || self.has_awaiting_reply
    }
}

/// Pick one lane. Priority: human overlay, then DND hold, then runtime.
pub fn choose_reminder_lane(state: ReminderTargetState) -> CollaborationReminderLane {
    if !state.needs_attention() {
        return CollaborationReminderLane::Silent;
    }
    if state.audience == ReminderAudience::Human {
        return CollaborationReminderLane::HumanOverlay;
    }
    if state.do_not_disturb {
        return CollaborationReminderLane::HoldDnd;
    }
    match state.runtime {
        ReminderRuntime::ConnectedBusy => CollaborationReminderLane::HookWhisper,
        ReminderRuntime::ConnectedIdle => CollaborationReminderLane::IdleStart,
        ReminderRuntime::Missing => CollaborationReminderLane::HoldClosed,
    }
}

/// Short whisper for a running turn. Never includes the original letter body.
pub fn hook_whisper_text(unread: u32, awaiting_reply: u32, do_not_disturb: bool) -> String {
    let mut parts = Vec::new();
    if unread > 0 {
        parts.push(format!("{unread} unread Session message(s)"));
    }
    if awaiting_reply > 0 {
        parts.push(format!(
            "{awaiting_reply} read Session message(s) still need a reply"
        ));
    }
    if parts.is_empty() {
        return String::new();
    }
    let mut text = format!(
        "Codeg mailbox: {}. Use list_sessions/send_message; this is not a user approval.",
        parts.join("; ")
    );
    if do_not_disturb {
        text.push_str(" Do-not-disturb is on; this is status only.");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(runtime: ReminderRuntime) -> ReminderTargetState {
        ReminderTargetState {
            audience: ReminderAudience::AgentSession,
            runtime,
            do_not_disturb: false,
            has_unread: true,
            has_awaiting_reply: false,
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
    fn busy_session_gets_a_hook_whisper_not_an_interrupt() {
        assert_eq!(
            choose_reminder_lane(agent(ReminderRuntime::ConnectedBusy)),
            CollaborationReminderLane::HookWhisper
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
    fn dnd_suppresses_push_even_when_idle() {
        let mut state = agent(ReminderRuntime::ConnectedIdle);
        state.do_not_disturb = true;
        assert_eq!(
            choose_reminder_lane(state),
            CollaborationReminderLane::HoldDnd
        );
    }

    #[test]
    fn human_mail_never_starts_a_session() {
        let state = ReminderTargetState {
            audience: ReminderAudience::Human,
            runtime: ReminderRuntime::ConnectedIdle,
            do_not_disturb: false,
            has_unread: true,
            has_awaiting_reply: true,
        };
        assert_eq!(
            choose_reminder_lane(state),
            CollaborationReminderLane::HumanOverlay
        );
    }

    #[test]
    fn nothing_to_say_when_mailbox_is_clear() {
        let mut state = agent(ReminderRuntime::ConnectedIdle);
        state.has_unread = false;
        assert_eq!(
            choose_reminder_lane(state),
            CollaborationReminderLane::Silent
        );
    }

    #[test]
    fn hook_text_does_not_repeat_the_letter_body() {
        let text = hook_whisper_text(2, 1, false);
        assert!(text.contains("2 unread"));
        assert!(text.contains("1 read"));
        assert!(!text.contains("<<<CODEG_SESSION_MESSAGE"));
    }
}
